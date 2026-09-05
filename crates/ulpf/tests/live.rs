//! v1 done-items 1 and 2 in process, without HTTP: unknown lines become a proposal, the
//! review workflow moves it to active, and the same lines then take the fast path.
//! Adversarial review cases: approve twice, reject then resubmit, hand-edited proposal
//! not replaced, invalid syntax refused with its problems, traceback of a missing id.

use std::path::{Path, PathBuf};

use ulpf::engine::{Config, Live, TracebackError, run};
use ulpf::pending::{Pending, ReviewError, WriteOutcome};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-live-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A private copy of the parsers directory: approval writes into it.
fn parsers_copy(dir: &Path) -> PathBuf {
    let dst = dir.join("parsers");
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(repo().join("parsers")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, dst.join(p.file_name().unwrap())).unwrap();
    }
    dst
}

fn config(dir: &Path, inputs: Vec<PathBuf>, threshold: usize) -> Config {
    Config {
        inputs,
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: parsers_copy(dir),
        mappings: repo().join("mappings"),
        schema: None,
        threads: 3,
        default_offset_secs: 0,
        batch_events: 64,
        queue_batches: 4,
        pending: Some(dir.join("pending")),
        infer_threshold: threshold,
        tail_capacity: 64,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        pivot_index: true,
        parquet: None,
        parquet_roll: None,
    }
}

#[test]
fn unknown_format_becomes_a_proposal_and_approval_moves_it_to_the_fast_path() {
    let dir = temp("e2e");
    let cfg = config(&dir, vec![repo().join("heldout/mikrotik.log")], 64);
    let report = run(&cfg).unwrap();
    let s = &report.snapshot;
    assert_eq!(s.framed, 250);
    assert_eq!(s.no_parser, 250, "no shipped parser may claim MikroTik lines");
    assert_eq!(s.infer_buffered, 250);
    assert_eq!(s.infer_runs, 1, "batch mode clusters once, after the run");
    assert_eq!(s.proposals_written, 1);
    assert_eq!(report.pending.len(), 1);
    let id = report.pending[0].id.clone();
    assert_eq!(id, "mikrotik");
    assert!(report.pending[0].templates >= 10, "{:?}", report.pending[0]);
    assert_eq!(report.pending[0].problems, 0);

    // the proposal parses nothing until approved: the same file into a fresh store (the
    // same store would resume past it, D59) still reports no_parser
    let again = run(&Config { store: dir.join("store2"), output: dir.join("out2.jsonl"), ..config(&dir, vec![repo().join("heldout/mikrotik.log")], 64) }).unwrap();
    assert_eq!(again.snapshot.no_parser, 250);
    assert_eq!(again.snapshot.proposals_written, 0, "same fingerprint is a duplicate, not a second proposal");
    assert!(again.snapshot.proposals_skipped.iter().any(|(r, n)| *r == "duplicate" && *n == 1), "{:?}", again.snapshot.proposals_skipped);

    // review: the definition is on disk beside its evidence and lines
    let pending = Pending::open(&dir.join("pending")).unwrap();
    let detail = pending.get(&id).unwrap();
    assert!(detail.definition.contains("[parser]"), "{}", detail.definition);
    assert!(detail.problems.is_empty(), "{:?}", detail.problems);
    assert_eq!(detail.record.evidence.lines_seen, 250);
    assert!(detail.record.evidence.decisions.len() > 5);
    assert_eq!(pending.lines(&id).len(), 250);

    // approve through Live so the registry reloads in place and the buffered lines are re-detected
    let live = Live::open(&config(&dir, vec![], 64), true).unwrap();
    let before = live.parser_names().len();
    let approved = live.approve(&id).unwrap();
    assert_eq!(approved.name, "mikrotik_inferred");
    assert_eq!(approved.parsers_loaded, before + 1);
    assert!(approved.problems.is_empty(), "{:?}", approved.problems);
    assert_eq!(approved.now_detected.tested, 250);
    assert_eq!(approved.now_detected.detected, 250, "every buffered line must now be claimed by the new parser");
    assert!(approved.path.exists());
    assert!(matches!(live.approve(&id), Err(ReviewError::NotFound(_))), "approving twice is a 404, not a second parser");
    assert_eq!(live.metrics.approved.load(std::sync::atomic::Ordering::Relaxed), 1);
    drop(live);

    // the same file now takes the fast path in a fresh run: detected, parsed, no proposal
    let fast = run(&Config { store: dir.join("store3"), output: dir.join("out3.jsonl"), ..config(&dir, vec![repo().join("heldout/mikrotik.log")], 64) }).unwrap();
    let s = &fast.snapshot;
    assert_eq!(s.no_parser, 0, "{s}");
    assert_eq!(s.detected, 250);
    assert_eq!(s.parsed, 250, "parse_failed: {:?}", s.parse_failed);
    assert_eq!(s.infer_buffered, 0);
    assert!(fast.pending.is_empty(), "{:?}", fast.pending);
    let out = std::fs::read_to_string(dir.join("out3.jsonl")).unwrap();
    let last = out.lines().last().unwrap();
    let v: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(v["ulpf"]["parser"], "mikrotik_inferred");
    assert_eq!(v["ulpf"]["parse_status"], "parsed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn review_edge_cases_are_errors_as_values() {
    let dir = temp("review");
    let cfg = config(&dir, vec![repo().join("heldout/nginx_access.log")], 32);
    let report = run(&cfg).unwrap();
    assert_eq!(report.pending.len(), 1, "{:?}", report.pending);
    let id = report.pending[0].id.clone();
    let pending = Pending::open(&dir.join("pending")).unwrap();

    // a hand edit into invalid syntax is saved, listed with its problem, and refuses approval
    let problems = pending.put_text(&id, "[parser]\nname = \"x\"\nthis is not toml").unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains(":3:"), "line number expected: {}", problems[0]);
    assert!(pending.list()[0].edited);
    let live = Live::open(&config(&dir, vec![], 32), true).unwrap();
    match live.approve(&id) {
        Err(ReviewError::Invalid(p)) => assert_eq!(p.len(), 1),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(live.pending.as_ref().unwrap().get(&id).is_ok(), "a failed approval leaves the proposal pending");

    // an edited proposal is never replaced by the engine
    let lines = pending.lines(&id);
    let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
    let proposal = ulpf_infer::infer("nginx_access.log", &refs[..100], &ulpf_infer::Params::default());
    assert_eq!(pending.write(&proposal, &lines[..100]).unwrap(), WriteOutcome::SkippedEdited);

    // name conflict: a definition named like an active parser is refused
    let text = format!("[parser]\nname = \"cisco_asa\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"HTTP\"]\n[strategy]\nkind = \"pattern\"\npattern = '{}'\n", "{a:rest}");
    assert!(pending.put_text(&id, &text).unwrap().is_empty());
    assert!(matches!(live.approve(&id), Err(ReviewError::Conflict(n)) if n == "cisco_asa"));

    // regenerate from a subset of templates rewrites patterns and keeps the human's [match]
    let detail = pending.get(&id).unwrap();
    let first = detail.record.evidence.templates[0].id;
    let (regenerated, problems) = live.regenerate(&id, &[first], &[]).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(regenerated.contains("contains = [\"HTTP\"]"), "{regenerated}");
    assert!(regenerated.contains("patterns = ["), "{regenerated}");
    assert!(regenerated.contains("{timestamp:timestamp}"), "{regenerated}");

    // reject, then the same fingerprint offered again is skipped as rejected
    let moved = live.reject(&id).unwrap();
    assert!(moved.exists());
    assert!(matches!(live.reject(&id), Err(ReviewError::NotFound(_))));
    let all: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
    let same = ulpf_infer::infer("nginx_access.log", &all, &ulpf_infer::Params::default());
    let reopened = Pending::open(&dir.join("pending")).unwrap();
    assert_eq!(reopened.write(&same, &lines).unwrap(), WriteOutcome::SkippedRejected, "rejected fingerprints survive a restart");
    assert_eq!(live.metrics.rejected.load(std::sync::atomic::Ordering::Relaxed), 1);

    // traceback: an issued id reads back with matching digests; a missing id is not found
    let t = live.traceback(0).unwrap();
    assert!(t.digest_match);
    assert_eq!(t.stored_sha256, t.recomputed_sha256);
    assert_eq!(t.source, "nginx_access.log");
    assert!(t.text.contains("HTTP/1.1"), "{}", t.text);
    assert_eq!(t.now.parse_status, "no_parser");
    match live.traceback(1_000_000) {
        Err(TracebackError::NotFound { store_len }) => assert_eq!(store_len, 250),
        other => panic!("expected NotFound, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_source_below_the_threshold_is_still_clustered_at_the_end_of_a_run() {
    let dir = temp("small");
    // 5 unknown lines, threshold 64: never reaches it; the final pass still proposes
    let small = dir.join("small.log");
    std::fs::write(&small, "<134>Sep  4 10:15:23 gw foo,info thing 1 happened to 10.0.0.1\n<134>Sep  4 10:15:24 gw foo,info thing 2 happened to 10.0.0.2\n<134>Sep  4 10:15:25 gw foo,info thing 3 happened to 10.0.0.3\n<134>Sep  4 10:15:26 gw foo,info thing 4 happened to 10.0.0.4\n<134>Sep  4 10:15:27 gw foo,info thing 5 happened to 10.0.0.5\n").unwrap();
    let report = run(&config(&dir, vec![small], 64)).unwrap();
    assert_eq!(report.snapshot.no_parser, 5);
    assert_eq!(report.pending.len(), 1, "{:?}", report.pending);
    assert_eq!(report.pending[0].templates, 1);
    // and with inference off nothing is buffered or written
    let dir2 = temp("off");
    let mut cfg = config(&dir2, vec![repo().join("heldout/nginx_access.log")], 0);
    cfg.pending = None;
    let report = run(&cfg).unwrap();
    assert_eq!(report.snapshot.infer_buffered, 0);
    assert!(report.pending.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}
