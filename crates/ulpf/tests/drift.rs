//! Drift: an established source whose lines stop matching its parser trips a window,
//! its misses go to inference with that parser as the prior, and a versioned update
//! lands in the pending directory with a diff. A source that always mixed two formats
//! never trips.

use std::path::{Path, PathBuf};

use ulpf::engine::{Config, DriftState, Live, DRIFT_WINDOW};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ulpf-drift-{name}-{}", std::process::id()));
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
        threads: 2,
        default_offset_secs: 0,
        batch_events: 16,
        queue_batches: 4,
        pending: Some(dir.join("pending")),
        infer_threshold: 64,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        parquet: None,
        parquet_roll: None,
    }
}

/// An approved MikroTik parser in `parsers/`, exactly as `Pending::approve` would write it.
fn install_mikrotik(dir: &Path) -> Vec<Vec<u8>> {
    std::fs::create_dir_all(dir.join("parsers")).unwrap();
    for e in std::fs::read_dir(root().join("parsers")).unwrap().flatten() {
        std::fs::copy(e.path(), dir.join("parsers").join(e.file_name())).unwrap();
    }
    let bytes = std::fs::read(root().join("heldout/mikrotik.log")).unwrap();
    let lines: Vec<Vec<u8>> = ulpf_store::Framer::new(&bytes, true).map(|r| bytes[r].to_vec()).collect();
    let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
    let proposal = ulpf_infer::infer("mikrotik.log", &refs, &ulpf_infer::Params::default());
    std::fs::write(dir.join("parsers/mikrotik_inferred.toml"), toml::to_string(&proposal.definition).unwrap()).unwrap();
    lines
}

/// A message type the parser has never seen, in the device's own header style.
fn new_shape(known: &[Vec<u8>], n: usize) -> Vec<u8> {
    let first = String::from_utf8_lossy(&known[0]).into_owned();
    // "<ts> <host> topic..." : keep everything up to and including the hostname
    let header: String = first.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    for i in 0..n {
        out.extend_from_slice(format!("{header} interface,info ether{} link up (speed {}G, full duplex)\n", 1 + i % 8, [1, 10, 25][i % 3]).as_bytes());
    }
    out
}

#[test]
fn an_established_source_that_changes_format_gets_a_versioned_update_proposal() {
    let dir = tmp("trip");
    let known = install_mikrotik(&dir);
    assert_eq!(known.len(), 250);
    let mut device = Vec::new();
    for _ in 0..5 {
        for l in &known {
            device.extend_from_slice(l);
        }
    }
    let known_events = 5 * known.len() as u64;
    let new_events = 400usize;
    device.extend_from_slice(&new_shape(&known, new_events));
    std::fs::write(dir.join("device.log"), &device).unwrap();
    let cfg = config(&dir, vec![dir.join("device.log")]);
    let report = ulpf::engine::run(&cfg).unwrap();
    let s = &report.snapshot;
    assert_eq!(s.framed, known_events + new_events as u64);
    assert_eq!(s.drift_tripped, 1, "{s}");
    // the window that trips is the first one whose misses exceed the baseline by 0.25;
    // every miss after it is routed
    let trip_at = ((known_events / DRIFT_WINDOW) + 1) * DRIFT_WINDOW;
    let routed = known_events + new_events as u64 - trip_at;
    assert_eq!(s.drift_lines_routed, routed, "{s}");
    assert_eq!(s.drift_proposals, 1, "{s}");
    assert!(s.no_parser + s.parse_failed.iter().map(|(_, n)| n).sum::<u64>() >= new_events as u64);

    let pending = ulpf::pending::Pending::open(&dir.join("pending")).unwrap();
    let id = ulpf::pending::Pending::id_for("device.log");
    let detail = pending.get(&id).expect("an update proposal for the device");
    let update = detail.record.updates.as_ref().expect("marked as an update");
    assert_eq!(update.name, "mikrotik_inferred");
    assert_eq!(update.current_version, 1);
    assert_eq!(update.kind, "patterns_added");
    let def: ulpf_parse::def::ParserDefinition = toml::from_str(&detail.definition).unwrap();
    assert_eq!(def.parser.name, "mikrotik_inferred");
    assert_eq!(def.parser.version, 2);
    let prior: ulpf_parse::def::ParserDefinition = toml::from_str(&std::fs::read_to_string(dir.join("parsers/mikrotik_inferred.toml")).unwrap()).unwrap();
    assert_eq!(def.strategy.patterns.len(), prior.strategy.patterns.len() + 1, "one template appended");
    assert_eq!(&def.strategy.patterns[..prior.strategy.patterns.len()], &prior.strategy.patterns[..], "the prior's patterns come first, unchanged");
    assert!(def.strategy.patterns.last().unwrap().contains("link up"), "{:?}", def.strategy.patterns.last());
    assert!(detail.record.evidence.decisions.iter().any(|d| d.starts_with("prior: `mikrotik_inferred` v1")), "{:?}", detail.record.evidence.decisions);
    let (current, diff) = pending.current_and_diff(&id, &dir.join("parsers"));
    assert!(current.is_some());
    let diff = diff.unwrap();
    assert!(diff.contains("+++ pending/") && diff.contains("link up"), "{diff}");
    assert!(diff.lines().any(|l| l.starts_with("-version") || l.starts_with("+version = 2")), "{diff}");

    // approval replaces the file, keeps v1, and the new shape now parses on the fast path
    let live = Live::open(&cfg, false).unwrap();
    let approved = live.approve(&id).unwrap();
    assert_eq!(approved.replaced_version, Some(1));
    assert!(dir.join("pending/approved/mikrotik_inferred.v1.toml").exists());
    let now: ulpf_parse::def::ParserDefinition = toml::from_str(&std::fs::read_to_string(dir.join("parsers/mikrotik_inferred.toml")).unwrap()).unwrap();
    assert_eq!(now.parser.version, 2);
    let alerts = live.drift_alerts();
    assert!(alerts.is_empty() || alerts.iter().all(|a| a.state != DriftState::Tripped));
    drop(live);
    std::fs::write(dir.join("again.log"), new_shape(&known, 50)).unwrap();
    let cfg2 = Config { inputs: vec![dir.join("again.log")], store: dir.join("store2"), output: dir.join("out2.jsonl"), ..config(&dir, vec![]) };
    let r2 = ulpf::engine::run(&cfg2).unwrap();
    assert_eq!(r2.snapshot.parsed, 50, "{}", r2.snapshot);
    assert_eq!(r2.snapshot.no_parser, 0);
}

#[test]
fn a_source_that_always_mixed_two_formats_never_trips() {
    let dir = tmp("mixed");
    let known = install_mikrotik(&dir);
    let nginx = std::fs::read(root().join("heldout/nginx_access.log")).unwrap();
    let other: Vec<Vec<u8>> = ulpf_store::Framer::new(&nginx, true).map(|r| nginx[r].to_vec()).collect();
    let mut device = Vec::new();
    for i in 0..1600 {
        let l = if i % 2 == 0 { &known[i % known.len()] } else { &other[i % other.len()] };
        device.extend_from_slice(l);
    }
    std::fs::write(dir.join("mixed.log"), &device).unwrap();
    let cfg = config(&dir, vec![dir.join("mixed.log")]);
    let report = ulpf::engine::run(&cfg).unwrap();
    assert_eq!(report.snapshot.drift_tripped, 0, "{}", report.snapshot);
    assert_eq!(report.snapshot.drift_proposals, 0);
    let live = Live::open(&cfg, false).unwrap();
    let sources = live.sources.lock().unwrap();
    assert!(sources.get("mixed.log").is_none_or(|s| s.drift == DriftState::None));
    drop(sources);
    // the unknown half still reaches ordinary inference as a standalone proposal
    let pending = ulpf::pending::Pending::open(&dir.join("pending")).unwrap();
    let ids = pending.ids();
    assert!(ids.iter().all(|id| pending.get(id).unwrap().record.updates.is_none()), "{ids:?}");
}

#[test]
fn in_serve_mode_a_quiet_source_with_a_partial_window_still_trips_and_gets_an_update() {
    use std::io::Write;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let dir = tmp("serve");
    let known = install_mikrotik(&dir);
    std::fs::create_dir_all(dir.join("watch")).unwrap();
    let cfg = Config { batch_events: 1024, ..config(&dir, vec![dir.join("watch")]) };
    let live = Live::open(&cfg, true).unwrap();
    let serve = {
        let live = Arc::clone(&live);
        std::thread::spawn(move || ulpf::engine::serve(&live, Duration::from_millis(50)))
    };
    // 1250 known lines, written at once (the poller batches them however it sees them)
    let mut f = std::fs::File::create(dir.join("watch/gw.log")).unwrap();
    for _ in 0..5 {
        for l in &known {
            f.write_all(l).unwrap();
        }
    }
    f.flush().unwrap();
    let start = Instant::now();
    while live.sources.lock().unwrap().get("gw.log").map(|s| s.events).unwrap_or(0) < 1250 {
        assert!(start.elapsed() < Duration::from_secs(20), "known lines not ingested");
        std::thread::sleep(Duration::from_millis(50));
    }
    // then fewer drifted lines than a window holds, and silence
    f.write_all(&new_shape(&known, 300)).unwrap();
    f.flush().unwrap();
    drop(f);
    let start = Instant::now();
    while live.metrics.drift_tripped.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        assert!(start.elapsed() < Duration::from_secs(30), "the quiet source never tripped: {:?}", live.sources.lock().unwrap().get("gw.log").map(|s| (s.events, s.no_parser, s.window_events, s.window_misses, s.baseline_events, s.drift)));
        std::thread::sleep(Duration::from_millis(100));
    }
    // the update proposal follows once the buffer has been quiet for the inference idle time
    let pending = ulpf::pending::Pending::open(&dir.join("pending")).unwrap();
    let id = ulpf::pending::Pending::id_for("gw.log");
    let start = Instant::now();
    loop {
        if let Ok(d) = pending.get(&id)
            && d.record.updates.is_some()
        {
            assert_eq!(d.record.updates.as_ref().unwrap().name, "mikrotik_inferred");
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no update proposal arrived");
        std::thread::sleep(Duration::from_millis(100));
    }
    let alerts = live.drift_alerts();
    assert!(alerts.iter().any(|a| a.source == "gw.log" && a.state == DriftState::Proposed && a.proposed_version == Some(2)), "{alerts:?}");
    live.stop();
    let report = serve.join().unwrap().unwrap();
    assert_eq!(report.snapshot.drift_tripped, 1);
    assert_eq!(report.snapshot.drift_proposals, 1, "{}", report.snapshot);
}
