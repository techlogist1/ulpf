//! Replay: a deliberate parser bug, a run, the fix, a replay; every past event corrected,
//! the diff says which file changed, and the raw store is untouched.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use ulpf::engine::{Config, Live, ReplayError};
use ulpf::pipeline::Pipeline;
use ulpf::replay::{self, Job, Versions};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ulpf-replay-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(dir: &Path, inputs: Vec<PathBuf>) -> Config {
    Config {
        inputs,
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: dir.join("parsers"),
        mappings: root().join("mappings"),
        schema: None,
        threads: 3,
        default_offset_secs: 0,
        batch_events: 16,
        queue_batches: 4,
        pending: None,
        infer_threshold: 0,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
    }
}

fn copy_parsers(dir: &Path) {
    std::fs::create_dir_all(dir.join("parsers")).unwrap();
    for e in std::fs::read_dir(root().join("parsers")).unwrap().flatten() {
        std::fs::copy(e.path(), dir.join("parsers").join(e.file_name())).unwrap();
    }
}

/// The deliberate bug: every `dst_ip` slot, whatever its type, is renamed, so the mapping
/// no longer finds the destination address (it lands in `unmapped.dst_addr`).
fn break_asa(dir: &Path) {
    let p = dir.join("parsers/cisco_asa.toml");
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("{dst_ip:"));
    std::fs::write(&p, text.replace("{dst_ip:", "{dst_addr:")).unwrap();
}

fn fix_asa(dir: &Path) {
    std::fs::copy(root().join("parsers/cisco_asa.toml"), dir.join("parsers/cisco_asa.toml")).unwrap();
}

fn store_digest(dir: &Path) -> (u64, Vec<u8>) {
    let seg = std::fs::read(dir.join("store/raw.seg")).unwrap();
    let idx = std::fs::read(dir.join("store/raw.idx")).unwrap();
    (idx.len() as u64, seg)
}

#[test]
fn a_fixed_parser_replays_every_past_event_without_touching_the_store() {
    let dir = tmp("cli");
    copy_parsers(&dir);
    break_asa(&dir);
    let cfg = config(&dir, vec![root().join("samples")]);
    let report = ulpf::engine::run(&cfg).unwrap();
    let total = report.snapshot.emitted;
    let v1 = std::fs::read_to_string(dir.join("out.jsonl")).unwrap();
    let buggy = v1.lines().filter(|l| l.contains("\"dst_addr\"")).count() as u64;
    assert!(buggy > 0, "the bug must be visible in v1");
    let before = store_digest(&dir);
    let versions = Versions::new(&cfg.output);
    assert_eq!(versions.list().len(), 1, "the live output is version 1 with a meta");

    fix_asa(&dir);
    let reader = ulpf_store::RawReader::open(&cfg.store).unwrap();
    let names = reader.source_names().unwrap();
    let (pipeline, problems) = Pipeline::load(&cfg.parsers, &cfg.mappings, None, 0).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let total = reader.len();
    let job = Job { versions: versions.clone(), version: versions.next(), pipeline: Arc::new(pipeline), threads: 3, batch: 16, parsers_generation: 0, names, reader, total };
    let r = replay::run(job, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();

    assert_eq!(r.version, 2);
    assert_eq!(r.previous_version, Some(1));
    assert_eq!(r.events, total);
    assert_eq!(r.summary.only_in_new + r.summary.only_in_old, 0);
    assert_eq!(r.summary.changed, buggy, "{:?}", r.summary);
    assert_eq!(r.summary.unchanged, total - buggy);
    assert_eq!(r.summary.fields_added, buggy, "one field added per corrected event");
    assert_eq!(r.summary.fields_lost, buggy);
    assert!(r.summary.by_field.iter().any(|f| f.path == "dst_endpoint.ip" && f.added == buggy), "{:?}", r.summary.by_field);
    assert!(r.summary.by_field.iter().any(|f| f.path == "unmapped.dst_addr" && f.lost == buggy));
    assert!(r.why.iter().any(|w| w.contains("cisco_asa.toml changed since v1")), "{:?}", r.why);
    let v2 = std::fs::read_to_string(dir.join("out.v2.jsonl")).unwrap();
    assert!(!v2.contains("\"dst_addr\""), "every past event is corrected");
    assert_eq!(v2.lines().count() as u64, total);
    assert_eq!(store_digest(&dir), before, "the raw store is byte for byte what it was");
    assert_eq!(ulpf_store::RawReader::open(&cfg.store).unwrap().verify().corrupt.len(), 0);

    // the diff pages by raw id, in order, with the parser on both sides
    let index = replay::index_diff(&versions.diff_path(2)).unwrap();
    let (page, next) = replay::page(&versions.diff_path(2), &index, None, 3, Some("changed")).unwrap();
    assert_eq!(page.len(), 3);
    assert!(page.windows(2).all(|w| w[0].raw_id < w[1].raw_id));
    assert_eq!(page[0].parser_before.as_deref(), Some("cisco_asa"));
    assert!(page[0].added.contains_key("dst_endpoint.ip"), "{:?}", page[0].added);
    let (rest, _) = replay::page(&versions.diff_path(2), &index, next, 500, None).unwrap();
    assert_eq!(rest.len() as u64, buggy - 3);
    assert_eq!(versions.list().len(), 2);
    assert_eq!(versions.next(), 3);

    // replaying again with nothing changed: v3 identical to v2, and the why says so
    let reader = ulpf_store::RawReader::open(&cfg.store).unwrap();
    let names = reader.source_names().unwrap();
    let (pipeline, _) = Pipeline::load(&cfg.parsers, &cfg.mappings, None, 0).unwrap();
    let job = Job { versions: versions.clone(), version: 3, pipeline: Arc::new(pipeline), threads: 2, batch: 64, parsers_generation: 0, names, reader, total };
    let r3 = replay::run(job, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
    assert_eq!(r3.summary.changed, 0);
    assert_eq!(r3.summary.unchanged, total);
    assert!(r3.why.iter().any(|w| w.contains("unchanged")), "{:?}", r3.why);
}

#[test]
fn the_server_replays_through_the_writer_and_reports_progress() {
    let dir = tmp("live");
    copy_parsers(&dir);
    break_asa(&dir);
    let cfg = config(&dir, vec![root().join("samples")]);
    ulpf::engine::run(&cfg).unwrap();
    fix_asa(&dir);
    // a serve-mode Live holds the writer lock; the replay reads through its files
    let live = Live::open(&cfg, true).unwrap();
    live.reload_parsers();
    let (version, total) = live.start_replay(None).unwrap();
    assert_eq!(version, 2);
    assert_eq!(total, live.store.lock().unwrap().len());
    let buggy = std::fs::read_to_string(dir.join("out.jsonl")).unwrap().lines().filter(|l| l.contains("\"dst_addr\"")).count() as u64;
    assert!(buggy > 0);
    assert!(matches!(live.start_replay(None), Err(ReplayError::Running)) || live.replay_progress().is_none(), "a second replay while one runs is a conflict");
    let started = Instant::now();
    while live.replay_progress().is_some() {
        assert!(started.elapsed() < Duration::from_secs(30), "replay did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    let state = live.replay.lock().unwrap();
    let report = state.last.as_ref().expect("report");
    assert_eq!(report.summary.changed, buggy);
    assert_eq!(report.parsers_generation, 1, "the replay records the generation it used");
    assert!(report.why.iter().any(|w| w.contains("cisco_asa.toml changed")));
    drop(state);
    let (page, _) = live.replay_diff(2, None, 500, None).unwrap();
    assert_eq!(page.len() as u64, buggy);
    assert!(matches!(live.replay_diff(9, None, 10, None), Err(ReplayError::Invalid(_))));
    assert!(live.replay_generation.load(Relaxed) >= 2);
    // the live meta recorded the reload
    let meta = Versions::new(&cfg.output).read_meta(1).unwrap();
    assert_eq!(meta.history.len(), 1, "v1 reloaded once, the previous file set is kept");
}

#[test]
fn replay_of_an_empty_store_is_an_empty_version_not_an_error() {
    let dir = tmp("empty");
    copy_parsers(&dir);
    std::fs::write(dir.join("empty.log"), b"").unwrap();
    let cfg = config(&dir, vec![dir.join("empty.log")]);
    ulpf::engine::run(&cfg).unwrap();
    let reader = ulpf_store::RawReader::open(&cfg.store).unwrap();
    let names = reader.source_names().unwrap();
    let (pipeline, _) = Pipeline::load(&cfg.parsers, &cfg.mappings, None, 0).unwrap();
    let versions = Versions::new(&cfg.output);
    let job = Job { versions: versions.clone(), version: versions.next(), pipeline: Arc::new(pipeline), threads: 2, batch: 16, parsers_generation: 0, names, reader, total: 0 };
    let r = replay::run(job, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
    assert_eq!(r.events, 0);
    assert_eq!(r.summary.changed + r.summary.unchanged + r.summary.only_in_new + r.summary.only_in_old, 0);
    assert!(versions.path(2).exists());
}
