//! Done-item 1 and 2: the engine runs a directory end to end, and every raw event read
//! back from the store is byte-identical to the framed input with a matching digest.

use std::path::PathBuf;

use sha2::Digest;
use ulpf::engine::{Config, Filter, collect_inputs, run};
use ulpf_store::{Framer, RawId, RawReader};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-e2e-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(inputs: Vec<PathBuf>, dir: &std::path::Path, threads: usize) -> Config {
    Config {
        inputs,
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: repo().join("parsers"),
        mappings: repo().join("mappings"),
        schema: None,
        threads,
        default_offset_secs: 0,
        batch_events: 4, // tiny batches so ordering and reorder logic is actually exercised
        queue_batches: 2,
        pending: None,
        infer_threshold: 0,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        pivot_index: true,
        parquet: None,
        parquet_roll: None,
        filter: Default::default(),
    }
}

#[test]
fn samples_directory_round_trips_through_store_and_output_in_order() {
    let dir = temp("samples");
    let cfg = config(vec![repo().join("samples")], &dir, 7);
    let report = run(&cfg).unwrap();
    assert!(report.load_problems.is_empty(), "{:?}", report.load_problems);
    assert!(report.input_problems.is_empty(), "{:?}", report.input_problems);
    let s = &report.snapshot;
    assert!(s.framed > 0);
    assert_eq!(s.framed, s.stored);
    assert_eq!(s.framed, s.normalized);
    assert_eq!(s.framed, s.emitted);
    assert_eq!(s.detected + s.no_parser, s.framed);
    assert_eq!(s.parsed + s.parse_failed.iter().map(|(_, n)| n).sum::<u64>(), s.detected);
    assert!(s.events_per_sec > 0.0);
    assert!(s.queue_high_water <= s.queue_capacity);

    // Every record equals the framed event of its file, in file order, with a good digest.
    let (files, _) = collect_inputs(&cfg.inputs, &cfg.filter).unwrap();
    let reader = RawReader::open(&cfg.store).unwrap();
    let names = reader.source_names().unwrap();
    let mut id = 0u64;
    let mut multiline = 0;
    let mut non_utf8 = 0;
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut concat = Vec::new();
        for range in Framer::new(&bytes, true) {
            let rec = reader.get(RawId(id)).unwrap_or_else(|| panic!("record {id} missing"));
            assert_eq!(rec.bytes, &bytes[range.clone()], "raw id {id} from {name}");
            let expect: [u8; 32] = sha2::Sha256::digest(rec.bytes).into();
            assert_eq!(rec.sha256, expect, "digest raw id {id}");
            assert_eq!(names[&rec.source], name);
            if rec.bytes.iter().filter(|&&b| b == b'\n').count() > 1 {
                multiline += 1;
            }
            if std::str::from_utf8(rec.bytes).is_err() {
                non_utf8 += 1;
            }
            concat.extend_from_slice(rec.bytes);
            id += 1;
        }
        assert_eq!(concat, bytes, "records concatenate back to {name}");
    }
    assert_eq!(id, s.framed);
    // one: the continuation line a collector folded into samples/fortinet_fortigate.log.
    // The folding rules themselves are covered in ulpf-store's framing_is_lossless_and_
    // groups_continuations (stack trace, CRLF, blank lines, every chunk boundary).
    assert!(multiline >= 1, "corpus must include multi-line events, saw {multiline}");
    assert!(non_utf8 >= 2, "corpus must include non-UTF-8 events, saw {non_utf8}");
    assert!(reader.verify().corrupt.is_empty());

    // Output: one JSON line per event, raw ids in order, provenance present.
    let out = std::fs::read_to_string(dir.join("out.jsonl")).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len() as u64, s.framed);
    for (i, line) in lines.iter().enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("line {i}: {e}"));
        assert_eq!(v["ulpf"]["raw_id"].as_u64(), Some(i as u64), "output order equals raw id order");
        assert!(v["class_uid"].is_number());
        assert!(v["time"].is_number());
        assert!(v["metadata"]["log_name"].is_string());
    }
    let unknown = lines.iter().filter(|l| l.contains("\"parse_status\":\"no_parser\"")).count() as u64;
    assert_eq!(unknown, s.no_parser, "unknown-format events are emitted, not dropped");
    assert!(unknown >= 2, "samples/squid_access.log carries two lines no parser claims, by design (fixtures/squid_access.expected.jsonl:24,26)");

    // A second run on the same store appends; nothing earlier changes. (The same file again
    // would be resumed past, D59, so the second run brings a new source.)
    let before: Vec<Vec<u8>> = reader.iter().map(|r| r.unwrap().bytes.to_vec()).collect();
    drop(reader);
    let cfg2 = Config { output: dir.join("out2.jsonl"), ..config(vec![repo().join("heldout/mikrotik.log")], &dir, 2) };
    let report2 = run(&cfg2).unwrap();
    let reader = RawReader::open(&cfg.store).unwrap();
    assert_eq!(reader.len(), s.framed + report2.snapshot.framed);
    for (i, b) in before.iter().enumerate() {
        assert_eq!(reader.get(RawId(i as u64)).unwrap().bytes, b.as_slice());
    }
    let out2 = std::fs::read_to_string(dir.join("out2.jsonl")).unwrap();
    let first: serde_json::Value = serde_json::from_str(out2.lines().next().unwrap()).unwrap();
    assert_eq!(first["ulpf"]["raw_id"].as_u64(), Some(s.framed), "ids continue across runs");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn single_thread_and_many_threads_produce_identical_output() {
    let a = temp("t1");
    let b = temp("t8");
    run(&config(vec![repo().join("samples")], &a, 1)).unwrap();
    run(&config(vec![repo().join("samples")], &b, 8)).unwrap();
    let strip = |s: String| -> Vec<String> {
        // processed_time differs between runs; everything else must be identical
        s.lines()
            .map(|l| {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["metadata"].as_object_mut().unwrap().remove("processed_time");
                if v["ulpf"]["time_policies"].as_array().is_some_and(|a| a.iter().any(|p| p == "receipt_fallback")) {
                    v.as_object_mut().unwrap().remove("time");
                    v["metadata"].as_object_mut().unwrap().remove("event_time_rfc3339");
                }
                v.to_string()
            })
            .collect()
    };
    let oa = strip(std::fs::read_to_string(a.join("out.jsonl")).unwrap());
    let ob = strip(std::fs::read_to_string(b.join("out.jsonl")).unwrap());
    assert_eq!(oa.len(), ob.len());
    assert_eq!(oa, ob);
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();
}

/// Every `ParseFailure` reaches the counters through the engine: the metrics arrays are
/// sized from `ParseFailure::ALL`, so a variant added to the parser crate cannot index
/// past them (an unterminated xml tag panicked a worker before this test existed).
#[test]
fn malformed_xml_is_counted_and_emitted_never_a_panic() {
    let dir = temp("bad-xml");
    let head = "<Event xmlns=\"http://schemas.microsoft.com/win/2004/08/events/event\"><System><EventID>4624</EventID><TimeCreated SystemTime=\"2015-11-12T00:24:35.079785200Z\"/></System>";
    let mut bad = Vec::new();
    bad.extend_from_slice(format!("{head}<EventData><Data Name=\"X\n").as_bytes()); // unterminated tag
    bad.extend_from_slice(format!("{head}<EventData>a < b</EventData></Event>\n").as_bytes()); // stray <
    bad.extend_from_slice(format!("{head}<EventData><Data Name=\"X\">").as_bytes());
    bad.extend_from_slice(b"\xff\xfe</Data></EventData></Event>\n"); // not UTF-8
    bad.extend_from_slice(format!("{head}<EventData><Data Name=\"X\">ok</Data></EventData></Event>\n").as_bytes());
    let input = dir.join("bad.log");
    std::fs::write(&input, &bad).unwrap();
    let report = run(&config(vec![input], &dir, 2)).unwrap();
    let s = &report.snapshot;
    assert_eq!(s.framed, 4);
    assert_eq!(s.detected, 4, "windows_event claims every line by its schema URI");
    assert_eq!(s.parsed, 1);
    assert_eq!(s.parse_failed, vec![("invalid_xml", 3)]);
    assert_eq!(s.emitted, 4, "a parse failure is an output line, not a drop");
    let out = std::fs::read_to_string(dir.join("out.jsonl")).unwrap();
    assert_eq!(out.lines().filter(|l| l.contains("\"parse_status\":\"invalid_xml\"")).count(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// D83: a directory yields only its logs, at every depth. A README, a dotfile, a notes
/// file and a hidden directory are counted and named, never ingested; `--include` narrows.
#[test]
fn directory_inputs_exclude_documentation_and_dotfiles() {
    let dir = temp("filter");
    let input = dir.join("in");
    std::fs::create_dir_all(input.join("sub")).unwrap();
    std::fs::create_dir_all(input.join(".hiddendir")).unwrap();
    std::fs::write(input.join("fw.log"), "%ASA-6-302013: Built inbound TCP connection\n").unwrap();
    std::fs::write(input.join("README.md"), "not a log\n").unwrap();
    std::fs::write(input.join("notes.md"), "not a log either\n").unwrap();
    std::fs::write(input.join(".hidden"), "hidden\n").unwrap();
    std::fs::write(input.join("sub/deep.log"), "%ASA-6-302014: Teardown TCP connection\n").unwrap();
    std::fs::write(input.join("sub/PROVENANCE.md"), "prose at depth\n").unwrap();
    std::fs::write(input.join(".hiddendir/config"), "secret\n").unwrap();

    let cfg = config(vec![input.clone()], &dir, 2);
    let report = run(&cfg).unwrap();
    assert_eq!(report.snapshot.files, 2, "the two logs, at both depths");
    assert_eq!(report.snapshot.framed, 2);
    // README.md, notes.md, .hidden, sub/PROVENANCE.md and .hiddendir (pruned whole)
    assert_eq!(report.snapshot.files_excluded, 5);
    assert!(report.excluded.iter().any(|e| e.ends_with("PROVENANCE.md (*.md)")), "{:?}", report.excluded);
    assert!(report.excluded.iter().any(|e| e.ends_with(".hiddendir (.*)")), "{:?}", report.excluded);
    let out = std::fs::read_to_string(dir.join("out.jsonl")).unwrap();
    assert!(!out.contains("secret"), "a hidden directory is never read");

    // --include narrows: the .txt beside the log is not a log, at any depth.
    std::fs::write(input.join("sub/other.txt"), "text\n").unwrap();
    let cfg2 = Config {
        store: dir.join("store2"),
        output: dir.join("out2.jsonl"),
        filter: Filter { include: vec!["*.log".into()], exclude: Vec::new() },
        ..config(vec![input], &dir, 2)
    };
    let report2 = run(&cfg2).unwrap();
    assert_eq!(report2.snapshot.files, 2, "--include '*.log' takes the nested log too");
    // no --exclude at all, so nothing is pruned and .hiddendir/config is walked and fails the include
    assert_eq!(report2.snapshot.files_excluded, 6, "README, notes, .hidden, PROVENANCE, the .txt, .hiddendir/config");
    std::fs::remove_dir_all(&dir).unwrap();
}
