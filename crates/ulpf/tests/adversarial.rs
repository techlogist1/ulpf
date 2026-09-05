//! The harsh-reviewer pass: inputs designed to crash or silently corrupt a pipeline.
//! Each must be handled, counted and reported.

use std::path::PathBuf;

use ulpf::engine::{Config, run};
use ulpf::pipeline::Pipeline;
use ulpf_store::{RawId, RawReader};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-adv-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(inputs: Vec<PathBuf>, dir: &std::path::Path, parsers: PathBuf) -> Config {
    Config {
        inputs,
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers,
        mappings: repo().join("mappings"),
        schema: None,
        threads: 3,
        default_offset_secs: 0,
        batch_events: 3,
        queue_batches: 1,
        pending: None,
        infer_threshold: 0,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        pivot_index: true,
        parquet: None,
        parquet_roll: None,
    }
}

fn lines(dir: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join("out.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn hostile_inputs_are_counted_not_crashed() {
    let dir = temp("inputs");
    let input = dir.join("in");
    std::fs::create_dir_all(input.join("nested/deeper")).unwrap();
    std::fs::write(input.join("empty.log"), b"").unwrap();
    std::fs::write(input.join("only_newlines.log"), b"\n\n\n").unwrap();
    let huge = vec![b'x'; 8 * 1024 * 1024];
    std::fs::write(input.join("one_enormous_line.log"), &huge).unwrap();
    std::fs::write(input.join("unknown_format.log"), b"totally unknown 1 2 3\nanother line here\n").unwrap();
    let garbage: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    std::fs::write(input.join("binary.bin"), &garbage).unwrap();
    std::fs::write(input.join("bom_crlf.log"), b"\xEF\xBB\xBF<166>Sep 04 2026 10:15:23 asa : %ASA-6-302014: Teardown TCP connection 1 for outside:1.1.1.1/443 to inside:10.0.0.5/1 duration 0:00:01 bytes 1\r\n<166>Sep 04 2026 10:15:24 asa : %ASA-6-999999: unknown id\r\n").unwrap();
    std::fs::write(input.join("nested/deeper/asa.log"), b"%ASA-4-106023: Deny tcp src outside:1.1.1.1/1 dst inside:10.0.0.7/22 by access-group \"x\" [0x0, 0x0]\n").unwrap();
    std::fs::write(input.join(".hidden"), b"ignored\n").unwrap();
    std::fs::write(input.join("truncated_kv.log"), b"<189>date=2026-09-04 time=10:15:28 devname=\"F\" logid=\"1\" msg=\"unterminated").unwrap();

    let cfg = config(vec![input.clone()], &dir, repo().join("parsers"));
    let report = run(&cfg).unwrap();
    let s = &report.snapshot;
    assert_eq!(s.files, 8, "hidden file skipped, nested file found: {:?}", report.input_problems);
    assert_eq!(s.files_failed, 0);
    // empty: 0 events; only_newlines: 1; enormous: 1; unknown: 2; binary: >=1; bom_crlf: 2; nested: 1; truncated: 1
    assert_eq!(s.framed, s.stored);
    assert_eq!(s.framed, s.emitted);
    assert!(s.framed >= 9, "{s}");
    assert!(s.no_parser >= 4, "{s}");
    assert!(s.detected >= 4, "{s}");
    assert!(s.sub_uncovered >= 1, "unknown ASA message id must be signalled: {s}");
    assert_eq!(s.parsed, s.detected, "a BOM must not defeat the envelope: {s}");
    assert!(s.time_from_receipt >= 4);
    let out = lines(&dir);
    assert_eq!(out.len() as u64, s.framed);
    let enormous = out.iter().find(|v| v["metadata"]["log_name"] == "one_enormous_line.log").unwrap();
    assert_eq!(enormous["ulpf"]["parse_status"], "no_parser");
    assert_eq!(enormous["message"].as_str().unwrap().len(), huge.len());
    let bom = out.iter().find(|v| v["metadata"]["log_name"] == "bom_crlf.log" && v["metadata"]["event_code"] == "302014").unwrap();
    assert_eq!(bom["class_uid"], 4001);
    assert!(bom["ulpf"]["utf8_lossy"].is_null(), "a UTF-8 BOM is valid UTF-8");
    let reader = RawReader::open(&cfg.store).unwrap();
    assert_eq!(reader.len(), s.framed);
    assert!(reader.verify().corrupt.is_empty());
    let stored_huge = reader.iter().map(|r| r.unwrap()).find(|r| r.bytes.len() == huge.len()).unwrap();
    assert_eq!(stored_huge.bytes, huge.as_slice());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn broken_parser_file_is_reported_and_the_rest_still_run() {
    let dir = temp("parsers");
    let parsers = dir.join("parsers");
    std::fs::create_dir_all(&parsers).unwrap();
    for name in ["cisco_asa.toml", "fortinet_fortigate.toml"] {
        std::fs::copy(repo().join("parsers").join(name), parsers.join(name)).unwrap();
    }
    std::fs::write(parsers.join("broken.toml"), "[parser]\nname = \"broken\"\nvendor = \"v\"\n[match\ncontains = [\"x\"]\n").unwrap();
    std::fs::write(parsers.join("bad_regex.toml"), "[parser]\nname = \"bad\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"x\"]\n[strategy]\nkind = \"pattern\"\nregex = \"(unclosed\"\n").unwrap();
    std::fs::write(parsers.join("schema_leak.toml"), "[parser]\nname = \"leak\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"x\"]\n[strategy]\nkind = \"kv\"\n[[timestamp]]\nfield = \"t\"\nformat = \"%Q\"\n").unwrap();
    let (pipeline, problems) = Pipeline::load(&parsers, &repo().join("mappings"), None, 0).unwrap();
    assert_eq!(pipeline.registry.len(), 2, "good files load");
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].contains("bad_regex.toml") && problems[0].contains("compile"), "{}", problems[0]);
    assert!(problems[1].contains("broken.toml:4"), "syntax errors carry a line: {}", problems[1]);
    assert!(problems[2].contains("schema_leak.toml") && problems[2].contains("timestamp"), "{}", problems[2]);

    let cfg = config(vec![repo().join("samples/cisco_asa.log")], &dir, parsers);
    let report = run(&cfg).unwrap();
    assert_eq!(report.load_problems.len(), 3);
    assert_eq!(report.parsers_loaded, 2);
    assert_eq!(report.snapshot.detected, report.snapshot.framed);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_directories_and_zero_parsers_are_explicit() {
    let dir = temp("dirs");
    let empty = dir.join("no_parsers");
    std::fs::create_dir_all(&empty).unwrap();
    let cfg = config(vec![repo().join("samples/cisco_asa.log")], &dir, empty);
    let report = run(&cfg).unwrap();
    assert_eq!(report.parsers_loaded, 0);
    assert_eq!(report.snapshot.no_parser, report.snapshot.framed, "with no parsers, every event is still emitted as unknown");
    assert_eq!(report.snapshot.class_unknown, report.snapshot.framed);

    let missing = config(vec![repo().join("samples/cisco_asa.log")], &dir, dir.join("does_not_exist"));
    let err = run(&missing).unwrap_err().to_string();
    assert!(err.contains("parsers directory"), "{err}");
    let bad_input = config(vec![dir.join("nope.log")], &dir, repo().join("parsers"));
    let err = run(&bad_input).unwrap_err().to_string();
    assert!(err.contains("nope.log"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn chunk_boundary_mid_event_matches_whole_file_framing() {
    // The framer is chunk-safe (store tests); the engine maps whole files, so the
    // equivalent hazard is a batch boundary mid-file. Batch size 1 vs 1000 must agree.
    let a = temp("b1");
    let b = temp("b1000");
    let mut ca = config(vec![repo().join("samples")], &a, repo().join("parsers"));
    ca.batch_events = 1;
    let mut cb = config(vec![repo().join("samples")], &b, repo().join("parsers"));
    cb.batch_events = 1000;
    let ra = run(&ca).unwrap();
    let rb = run(&cb).unwrap();
    assert_eq!(ra.snapshot.framed, rb.snapshot.framed);
    let reader_a = RawReader::open(&ca.store).unwrap();
    let reader_b = RawReader::open(&cb.store).unwrap();
    for i in 0..ra.snapshot.framed {
        assert_eq!(reader_a.get(RawId(i)).unwrap().bytes, reader_b.get(RawId(i)).unwrap().bytes);
    }
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();
}

#[test]
fn output_failure_aborts_instead_of_hanging() {
    let dir = temp("outfail");
    let mut cfg = config(vec![repo().join("samples")], &dir, repo().join("parsers"));
    cfg.output = dir.clone(); // a directory: creating the output file fails
    cfg.queue_batches = 1;
    cfg.batch_events = 1;
    cfg.threads = 2;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run(&cfg).map(|_| ()).map_err(|e| format!("{e:#}")));
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("run hung after the output stage failed");
    let err = result.unwrap_err();
    assert!(err.contains("creating output"), "{err}");
    // everything appended before the failure is on disk and intact
    let reader = RawReader::open(&dir.join("store")).unwrap();
    assert!(reader.verify().corrupt.is_empty());
}

#[test]
fn queue_high_water_never_exceeds_capacity() {
    let dir = temp("queue");
    let mut cfg = config(vec![repo().join("samples")], &dir, repo().join("parsers"));
    cfg.queue_batches = 1;
    cfg.batch_events = 1;
    cfg.threads = 8;
    let s = run(&cfg).unwrap().snapshot;
    assert!(s.queue_high_water <= s.queue_capacity, "{s:?}");
    assert!(s.backpressure_blocks <= s.batches, "{s:?}");
    assert_eq!(s.emitted, s.framed);
}
