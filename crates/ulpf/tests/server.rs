//! v1 done-items 3 and 4 over real HTTP: the watch loop ingests a dropped file, a proposal
//! appears on the API, an invalid edit is refused with its problems, approval activates
//! the parser without restart, traceback shows both digests, the same format then takes
//! the fast path, and a client that disconnects mid-stream is dropped from the count.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use serde_json::Value;
use ulpf::engine::{Config, Live};
use ulpf::server::Server;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-server-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("watch")).unwrap();
    dir
}

fn parsers_copy(dir: &Path) -> PathBuf {
    let dst = dir.join("parsers");
    std::fs::create_dir_all(&dst).unwrap();
    for e in std::fs::read_dir(repo().join("parsers")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, dst.join(p.file_name().unwrap())).unwrap();
    }
    dst
}

struct Resp {
    status: u16,
    body: String,
}

fn http(addr: &str, method: &str, path: &str, body: Option<&str>) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let body_bytes = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body_bytes}",
        body_bytes.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
    // chunked bodies from hyper: join the chunks
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_string() };
    Resp { status, body }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&after[..n.min(after.len())]);
        rest = after.get(n + 2..).unwrap_or("");
    }
    out
}

fn json(addr: &str, method: &str, path: &str, body: Option<&str>) -> (u16, Value) {
    let r = http(addr, method, path, body);
    let v = serde_json::from_str(&r.body).unwrap_or_else(|e| panic!("{method} {path}: {e}: {}", r.body));
    (r.status, v)
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_server_is_a_window_onto_a_live_engine() {
    let dir = temp("api");
    let cfg = Config {
        inputs: vec![dir.join("watch")],
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: parsers_copy(&dir),
        mappings: repo().join("mappings"),
        schema: None,
        threads: 2,
        default_offset_secs: 0,
        batch_events: 32,
        queue_batches: 4,
        pending: Some(dir.join("pending")),
        infer_threshold: 20,
        tail_capacity: 500,
        receipt_nanos: None,
    };
    let live = Live::open(&cfg, true).unwrap();
    let server = Server::start(Arc::clone(&live), "127.0.0.1:0".parse().unwrap(), None).unwrap();
    let addr = server.addr.to_string();
    let engine = {
        let live = Arc::clone(&live);
        std::thread::spawn(move || ulpf::engine::serve(&live, Duration::from_millis(50)))
    };

    // status and the UI are served
    let (st, status) = json(&addr, "GET", "/api/status", None);
    assert_eq!(st, 200);
    assert_eq!(status["infer_threshold"], 20);
    assert_eq!(status["tail_capacity"], 500);
    let index = http(&addr, "GET", "/", None);
    assert_eq!(index.status, 200);
    assert!(index.body.contains("/app.js"), "{}", index.body);
    assert_eq!(http(&addr, "GET", "/app.js", None).status, 200);
    assert_eq!(http(&addr, "GET", "/app.css", None).status, 200);

    // drop an unknown format into the watch directory: it is ingested and a proposal appears
    std::fs::copy(repo().join("heldout/mikrotik.log"), dir.join("watch/mikrotik.log")).unwrap();
    wait_until("250 events framed", || json(&addr, "GET", "/api/metrics", None).1["engine"]["framed"] == 250);
    wait_until("a pending proposal", || json(&addr, "GET", "/api/pending", None).1.as_array().is_some_and(|a| !a.is_empty()));
    // live mode proposes at the threshold, again at each doubling, and once more when the
    // source goes quiet; a file that lands in one tick needs only a run or two
    wait_until("the proposal built from all 250 lines", || json(&addr, "GET", "/api/pending/mikrotik", None).1["evidence"]["lines_seen"] == 250);
    let (_, m) = json(&addr, "GET", "/api/metrics", None);
    let runs = m["engine"]["infer_runs"].as_u64().unwrap();
    assert!(runs >= 1, "{m}");
    assert_eq!(m["engine"]["proposals_written"], 1, "one pending file per source, replaced in place: {}", m["engine"]);
    assert_eq!(m["engine"]["infer_buffered"], 250);
    let (_, metrics) = json(&addr, "GET", "/api/metrics", None);
    assert_eq!(metrics["engine"]["no_parser"], 250);
    let src = &metrics["sources"][0];
    assert_eq!(src["name"], "mikrotik.log");
    assert_eq!(src["no_parser"], 250);
    assert_eq!(src["pending_id"], "mikrotik");
    assert!(metrics["parsers"].as_array().unwrap().len() >= 12);

    // the tail carries the emitted lines with their raw ids
    let (_, tail) = json(&addr, "GET", "/api/tail?limit=5", None);
    assert_eq!(tail["events"].as_array().unwrap().len(), 5);
    assert_eq!(tail["latest_raw_id"], 249);
    assert_eq!(tail["events"][4]["raw_id"], 249);
    assert_eq!(tail["events"][4]["line"]["ulpf"]["parse_status"], "no_parser");

    // review: evidence beside the definition; an invalid edit is saved and refused
    let (st, detail) = json(&addr, "GET", "/api/pending/mikrotik", None);
    assert_eq!(st, 200);
    let definition = detail["definition"].as_str().unwrap().to_string();
    assert!(detail["evidence"]["templates"].as_array().unwrap().len() >= 10);
    assert!(detail["evidence"]["decisions"].as_array().unwrap().len() > 5);
    assert_eq!(detail["problems"].as_array().unwrap().len(), 0);
    let (st, put) = json(&addr, "PUT", "/api/pending/mikrotik", Some(&serde_json::json!({ "definition": "[parser]\nname = 'x'\nnot toml" }).to_string()));
    assert_eq!(st, 200);
    assert_eq!(put["problems"].as_array().unwrap().len(), 1);
    let (st, err) = json(&addr, "POST", "/api/pending/mikrotik/approve", None);
    assert_eq!(st, 422);
    assert_eq!(err["reason"], "invalid");
    assert_eq!(err["problems"].as_array().unwrap().len(), 1);
    let (st, _) = json(&addr, "PUT", "/api/pending/mikrotik", Some(&serde_json::json!({ "definition": definition }).to_string()));
    assert_eq!(st, 200);
    let (st, err) = json(&addr, "GET", "/api/pending/nope", None);
    assert_eq!(st, 404);
    assert_eq!(err["reason"], "not_found");

    // approve: active without restart, and the buffered lines are re-detected
    let (st, approved) = json(&addr, "POST", "/api/pending/mikrotik/approve", None);
    assert_eq!(st, 200, "{approved}");
    assert_eq!(approved["name"], "mikrotik_inferred");
    assert_eq!(approved["now_detected"]["tested"], 250);
    assert_eq!(approved["now_detected"]["detected"], 250);
    let (st, err) = json(&addr, "POST", "/api/pending/mikrotik/approve", None);
    assert_eq!(st, 404, "approving twice: {err}");
    let (_, parsers) = json(&addr, "GET", "/api/parsers", None);
    assert!(parsers.as_array().unwrap().iter().any(|p| p["name"] == "mikrotik_inferred" && p["origin"] == "approved"));

    // traceback: digests side by side, the line as emitted, and how it parses now
    let (st, t) = json(&addr, "GET", "/api/events/0", None);
    assert_eq!(st, 200);
    assert_eq!(t["digest_match"], true);
    assert_eq!(t["stored_sha256"], t["recomputed_sha256"]);
    assert_eq!(t["source"], "mikrotik.log");
    assert_eq!(t["emitted"]["ulpf"]["parse_status"], "no_parser", "the line as it was emitted, before approval");
    assert_eq!(t["now"]["parser"], "mikrotik_inferred", "the same bytes through the current parsers");
    assert_eq!(t["now"]["parse_status"], "parsed");
    let (st, err) = json(&addr, "GET", "/api/events/424242", None);
    assert_eq!(st, 404);
    assert_eq!(err["store_len"], 250);

    // the same format again takes the fast path
    std::fs::copy(repo().join("heldout/mikrotik.log"), dir.join("watch/mikrotik2.log")).unwrap();
    wait_until("second file processed", || json(&addr, "GET", "/api/metrics", None).1["engine"]["framed"] == 500);
    let (_, metrics) = json(&addr, "GET", "/api/metrics", None);
    let second = metrics["sources"].as_array().unwrap().iter().find(|s| s["name"] == "mikrotik2.log").unwrap().clone();
    assert_eq!(second["detected"], 250, "{second}");
    assert_eq!(second["no_parser"], 0);
    assert_eq!(metrics["engine"]["approved"], 1);
    // approval reloads once; the directory poller may notice the new file first and reload too
    assert!(metrics["engine"]["reloads"].as_u64().unwrap() >= 1);
    assert_eq!(metrics["server"]["review_errors"], 3, "422, two 404s");

    // SSE: hello then metrics; a client that disconnects is dropped from the count
    {
        let mut s = TcpStream::connect(&addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(b"GET /api/stream?tail=3 HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut got = String::new();
        let mut buf = [0u8; 4096];
        let start = Instant::now();
        while !(got.contains("event: hello") && got.contains("event: metrics")) {
            assert!(start.elapsed() < Duration::from_secs(10), "no hello/metrics: {got}");
            let n = s.read(&mut buf).unwrap();
            got.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(got.contains("\"pending_count\""), "{got}");
        assert!(got.contains("\"latest_raw_id\":499"), "{got}");
        assert_eq!(live.sse_clients.load(Relaxed), 1);
    }
    wait_until("client dropped", || live.sse_clients.load(Relaxed) == 0);

    live.stop();
    let report = engine.join().unwrap().unwrap();
    server.shutdown();
    assert_eq!(report.snapshot.framed, 500);
    assert_eq!(report.snapshot.stored, 500);
    assert_eq!(report.snapshot.emitted, 500);
    let _ = std::fs::remove_dir_all(&dir);
}
