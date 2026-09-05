//! Live syslog: UDP datagrams and TCP streams (octet-counted, newline-framed, and a
//! connection that closes mid-event) enter the same store, queue and output as files,
//! byte for byte, in raw id order, with the counters reconciling exactly.

use std::io::Write;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use ulpf::engine::{Config, Live};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ulpf-syslog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("watch")).unwrap();
    d
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn udp_and_tcp_events_enter_the_store_and_output_exactly_once_in_order() {
    let dir = tmp("both");
    let cfg = Config {
        inputs: vec![dir.join("watch")],
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: root().join("parsers"),
        mappings: root().join("mappings"),
        schema: None,
        threads: 2,
        default_offset_secs: 0,
        batch_events: 64,
        queue_batches: 4,
        pending: None,
        infer_threshold: 0,
        tail_capacity: 64,
        receipt_nanos: None,
        syslog_udp: Some("127.0.0.1:0".parse().unwrap()),
        syslog_tcp: Some("127.0.0.1:0".parse().unwrap()),
        parquet: None,
        parquet_roll: None,
    };
    let live = Live::open(&cfg, false).unwrap();
    let serve = {
        let live = Arc::clone(&live);
        std::thread::spawn(move || ulpf::engine::serve(&live, Duration::from_millis(50)))
    };
    let (udp, tcp): (SocketAddr, SocketAddr) = {
        let mut got = (None, None);
        wait_until("listeners to bind", || {
            got = *live.syslog_bound.lock().unwrap();
            got.0.is_some() && got.1.is_some()
        });
        (got.0.unwrap(), got.1.unwrap())
    };

    let lines: Vec<Vec<u8>> = {
        let bytes = std::fs::read(root().join("heldout/mikrotik.log")).unwrap();
        ulpf_store::Framer::new(&bytes, true).map(|r| bytes[r.start..r.end].trim_ascii_end().to_vec()).collect()
    };
    // UDP: 3000 datagrams, no terminator, paced so loopback never drops
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_n = 3000usize;
    for i in 0..udp_n {
        sock.send_to(&lines[i % lines.len()], udp).unwrap();
        if i % 250 == 249 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    // TCP, newline framed: 2000 events in irregular writes
    let tcp_lines = 2000usize;
    {
        let mut s = TcpStream::connect(tcp).unwrap();
        let mut buf = Vec::new();
        for i in 0..tcp_lines {
            buf.extend_from_slice(&lines[i % lines.len()]);
            buf.push(b'\n');
        }
        for chunk in buf.chunks(7001) {
            s.write_all(chunk).unwrap();
        }
    }
    // TCP, RFC 6587 octet counting: 500 frames
    let tcp_frames = 500usize;
    {
        let mut s = TcpStream::connect(tcp).unwrap();
        let mut buf = Vec::new();
        for i in 0..tcp_frames {
            let l = &lines[i % lines.len()];
            buf.extend_from_slice(format!("{} ", l.len()).as_bytes());
            buf.extend_from_slice(l);
        }
        for chunk in buf.chunks(4099) {
            s.write_all(chunk).unwrap();
        }
    }
    // TCP, a connection that dies mid-event: the bytes are one partial event
    {
        let mut s = TcpStream::connect(tcp).unwrap();
        s.write_all(b"<134>Sep  4 06:00:00 gw firewall,info half a li").unwrap();
    }
    let expected = (udp_n + tcp_lines + tcp_frames + 1) as u64;
    wait_until("every event to be emitted", || live.metrics.emitted.load(Relaxed) >= expected);
    std::thread::sleep(Duration::from_millis(200));
    live.stop();
    let report = serve.join().unwrap().unwrap();
    let s = &report.snapshot;
    assert_eq!(s.syslog_udp_datagrams, udp_n as u64, "{s}");
    assert_eq!(s.syslog_tcp_connections, 3);
    assert_eq!(s.syslog_tcp_events, (tcp_lines + tcp_frames + 1) as u64, "{s}");
    assert_eq!(s.syslog_tcp_partial, 1);
    assert_eq!(s.syslog_tcp_refused, 0);
    assert_eq!(s.syslog_errors, 0);
    assert_eq!((s.framed, s.stored, s.emitted), (expected, expected, expected), "{s}");
    assert_eq!(s.no_parser, expected, "MikroTik lines are an unknown format by design: every one is still emitted: {s}");

    // the store holds the exact bytes: a datagram with no terminator, a line with its newline
    // (the engine's handle must be gone before a second reader opens the catalogue)
    drop(live);
    let reader = ulpf_store::RawReader::open(&cfg.store).unwrap();
    assert_eq!(reader.len(), expected);
    assert!(reader.verify().ok());
    let names = reader.source_names().unwrap();
    let mut by_source: std::collections::BTreeMap<String, u64> = Default::default();
    let mut newline_terminated = 0u64;
    for rec in reader.iter().map(|r| r.unwrap()) {
        *by_source.entry(names[&rec.source].clone()).or_default() += 1;
        newline_terminated += rec.bytes.ends_with(b"\n") as u64;
    }
    assert_eq!(by_source.get("udp/127.0.0.1").copied(), Some(udp_n as u64), "{by_source:?}");
    assert_eq!(by_source.get("tcp/127.0.0.1").copied(), Some((tcp_lines + tcp_frames + 1) as u64), "{by_source:?}");
    assert_eq!(newline_terminated, tcp_lines as u64, "only newline-framed TCP events keep a terminator");

    // the output is complete and in raw id order across three producers
    let out = std::fs::read_to_string(&cfg.output).unwrap();
    let ids: Vec<u64> = out.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["ulpf"]["raw_id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len() as u64, expected);
    assert!(ids.windows(2).all(|w| w[1] == w[0] + 1), "raw ids are contiguous and ordered in the output");
    let _ = std::fs::remove_dir_all(&dir);
}
