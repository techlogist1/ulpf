//! The v4 API contract (docs/api.md, "v4 additions") over real HTTP: the metrics frame's
//! queue and windowed rate, traceback's `emitted_from` and bytes route, the export route,
//! and the pivot cursor with its timings. One test per contract item; every assertion
//! names the field it is about, so a failure reads as "this field is missing" and not as
//! a panic in the harness.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use ulpf::engine::{Config, Live};
use ulpf::server::Server;

// ---------------------------------------------------------------- harness
// There is no `common` module under crates/ulpf/tests, so this is the copy of
// server.rs's harness these tests need (bytes-preserving body, headers kept).

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.log` under samples/, the corpus these tests feed.
const LOGS: &[&str] = &[
    "check_point.log",
    "cisco_asa.log",
    "cisco_ios.log",
    "fortinet_fortigate.log",
    "juniper_srx.log",
    "openvpn.log",
    "palo_alto_panos.log",
    "pfsense_filterlog.log",
    "sonicwall.log",
    "sophos_xg.log",
    "squid_access.log",
    "suricata_eve.log",
];

struct Resp {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Resp {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The first value of a response header, named in lowercase.
    fn header(&self, name: &str) -> Option<String> {
        self.head
            .lines()
            .skip(1)
            .find(|l| l.to_ascii_lowercase().starts_with(&format!("{name}:")))
            .map(|l| l[name.len() + 1..].trim().to_string())
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn http(addr: &str, method: &str, path: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let split = find(&buf, b"\r\n\r\n").unwrap_or(buf.len().saturating_sub(4));
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let body = buf.get(split + 4..).unwrap_or(&[]).to_vec();
    let status: u16 = head.split_whitespace().nth(1).unwrap_or("0").parse().unwrap_or(0);
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(&body) } else { body };
    Resp { status, head, body }
}

/// Chunked bodies from hyper, joined without going through UTF-8.
fn dechunk(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = b;
    while let Some(i) = find(rest, b"\r\n") {
        let size = String::from_utf8_lossy(&rest[..i]);
        let size = size.split(';').next().unwrap_or("").trim().to_string();
        let n = usize::from_str_radix(&size, 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        let start = i + 2;
        let end = (start + n).min(rest.len());
        out.extend_from_slice(&rest[start..end]);
        rest = rest.get(end + 2..).unwrap_or(&[]);
    }
    out
}

fn json(addr: &str, method: &str, path: &str) -> (u16, Value) {
    let r = http(addr, method, path);
    let v = serde_json::from_slice(&r.body).unwrap_or_else(|e| panic!("{method} {path}: body is not JSON: {e}: {}", r.text()));
    (r.status, v)
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out after 30 s waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ------------------------------------------------------- reading a JSON body

fn brief(v: &Value) -> String {
    v.to_string().chars().take(400).collect()
}

fn at<'a>(v: &'a Value, path: &str) -> &'a Value {
    let mut cur = v;
    for part in path.split('.') {
        cur = &cur[part];
    }
    cur
}

/// The value at a dotted path, with the field named in the failure.
fn present<'a>(v: &'a Value, path: &str, what: &str) -> &'a Value {
    let f = at(v, path);
    assert!(!f.is_null(), "{what}: the v4 contract requires `{path}`; it is missing or null in {}", brief(v));
    f
}

fn u64_at(v: &Value, path: &str, what: &str) -> u64 {
    let f = present(v, path, what);
    f.as_u64().unwrap_or_else(|| panic!("{what}: `{path}` must be a u64, got {f}"))
}

fn f64_at(v: &Value, path: &str, what: &str) -> f64 {
    let f = present(v, path, what);
    f.as_f64().unwrap_or_else(|| panic!("{what}: `{path}` must be a number, got {f}"))
}

// ------------------------------------------------------------ a live server

struct Running {
    dir: PathBuf,
    addr: String,
    live: Arc<Live>,
    server: Server,
    engine: std::thread::JoinHandle<String>,
}

impl Running {
    fn start(name: &str, tail_capacity: usize, pivot_index: bool, threads: usize) -> Running {
        let dir = std::env::temp_dir().join(format!("ulpf-v4-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("watch")).unwrap();
        let cfg = Config {
            inputs: vec![dir.join("watch")],
            store: dir.join("store"),
            output: dir.join("out.jsonl"),
            parsers: repo().join("parsers"),
            mappings: repo().join("mappings"),
            schema: None,
            threads,
            default_offset_secs: 0,
            batch_events: 8,
            queue_batches: 64,
            pending: None, // inference off: these tests are about the routes
            infer_threshold: 0,
            tail_capacity,
            receipt_nanos: Some(1_788_652_800_000_000_000), // 2026-09-06T00:00:00Z
            syslog_udp: None,
            syslog_tcp: None,
            pivot_index,
            parquet: None,
            parquet_roll: None,
        };
        let live = Live::open(&cfg, true).unwrap();
        let server = Server::start(Arc::clone(&live), "127.0.0.1:0".parse().unwrap(), None).unwrap();
        let addr = server.addr.to_string();
        let engine = {
            let live = Arc::clone(&live);
            std::thread::spawn(move || match ulpf::engine::serve(&live, Duration::from_millis(50)) {
                Ok(_) => String::new(),
                Err(e) => format!("{e:#}"),
            })
        };
        Running { dir, addr, live, server, engine }
    }

    fn output(&self) -> PathBuf {
        self.dir.join("out.jsonl")
    }

    fn metrics(&self) -> Value {
        json(&self.addr, "GET", "/api/metrics").1
    }

    /// Copies samples into the watch directory and returns how many events they frame.
    fn feed(&self, files: &[&str]) -> u64 {
        let mut events = 0;
        for f in files {
            let bytes = std::fs::read(repo().join("samples").join(f)).unwrap();
            events += event_count(&bytes);
            std::fs::write(self.dir.join("watch").join(f), &bytes).unwrap();
        }
        events
    }

    /// Waits for the whole corpus through every stage and onto disk (the output thread
    /// flushes a quiet watch directory within half a second).
    fn wait_for(&self, events: u64) {
        wait_until(&format!("{events} events framed"), || self.metrics()["engine"]["framed"].as_u64() == Some(events));
        wait_until(&format!("{events} events emitted"), || self.metrics()["engine"]["emitted"].as_u64() == Some(events));
        let out = self.output();
        wait_until(&format!("{events} lines flushed to {}", out.display()), || {
            std::fs::read(&out).map(|b| b.iter().filter(|&&c| c == b'\n').count() as u64).unwrap_or(0) >= events
        });
    }

    fn stop(self) {
        self.live.stop();
        let err = self.engine.join().unwrap();
        self.server.shutdown();
        assert!(err.is_empty(), "the engine thread failed: {err}");
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The framer's rule: a line starting with space, tab or a bare terminator continues the
/// event before it (crates/ulpf-store/src/frame.rs).
fn event_count(bytes: &[u8]) -> u64 {
    bytes
        .split_inclusive(|&c| c == b'\n')
        .filter(|l| !matches!(l.first(), Some(b' ' | b'\t' | b'\r' | b'\n')))
        .count() as u64
}

/// The exact bytes of the first event in a file, terminator included, as the store keeps them.
fn first_event(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in bytes.split_inclusive(|&c| c == b'\n') {
        if out.is_empty() || matches!(line.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            out.extend_from_slice(line);
        } else {
            break;
        }
    }
    out
}

fn raw_id_of(line: &str) -> u64 {
    let v: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("output line is not JSON: {e}: {line}"));
    v["ulpf"]["raw_id"].as_u64().unwrap_or_else(|| panic!("output line has no ulpf.raw_id: {line}"))
}

// ------------------------------------------------------------------- item 1

/// `GET /api/metrics` carries the queue as it is and a windowed rate.
#[test]
fn the_metrics_frame_carries_queue_depth_and_a_windowed_rate() {
    let r = Running::start("rate", 500, false, 2);
    let what = "GET /api/metrics";

    let (st, status) = json(&r.addr, "GET", "/api/status");
    assert_eq!(st, 200, "GET /api/status: {status}");
    let queue_capacity = u64_at(&status, "queue_capacity", "GET /api/status");

    // the first frame, before anything is ingested: the older of the two samples
    let first_frame = Instant::now();
    let before = r.metrics();
    let depth = u64_at(&before, "queue.depth", what);
    let capacity = u64_at(&before, "queue.capacity", what);
    assert!(depth <= capacity, "{what}: queue.depth ({depth}) must be at most queue.capacity ({capacity})");
    assert_eq!(capacity, queue_capacity, "{what}: queue.capacity ({capacity}) must equal /api/status queue_capacity ({queue_capacity})");
    for k in ["over_secs", "framed_per_sec", "emitted_per_sec"] {
        let v = f64_at(&before, &format!("rate.{k}"), what);
        assert!(v >= 0.0, "{what}: rate.{k} must not be negative, got {v}");
    }

    let events = r.feed(LOGS);
    r.wait_for(events);

    // the second frame at least 300 ms after the first (the server caches a frame for
    // 200 ms, so wait 500 ms to be sure the frame it hands back is late enough)
    let since = first_frame.elapsed();
    if since < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(500) - since);
    }
    let after = r.metrics();
    let depth = u64_at(&after, "queue.depth", what);
    let capacity = u64_at(&after, "queue.capacity", what);
    assert!(depth <= capacity, "{what}: queue.depth ({depth}) must be at most queue.capacity ({capacity})");
    let over = f64_at(&after, "rate.over_secs", what);
    let emitted_per_sec = f64_at(&after, "rate.emitted_per_sec", what);
    assert!(over >= 0.0, "{what}: rate.over_secs must not be negative, got {over}");
    assert!(emitted_per_sec >= 0.0, "{what}: rate.emitted_per_sec must not be negative, got {emitted_per_sec}");
    let framed_per_sec = f64_at(&after, "rate.framed_per_sec", what);
    assert!(
        framed_per_sec > 0.0,
        "{what}: rate.framed_per_sec must be positive after {events} events were framed between two frames at least 300 ms apart (rate.over_secs {over}, engine.framed {})",
        after["engine"]["framed"]
    );
    r.stop();
}

// ---------------------------------------------------------- item 1b: the tail frame

/// `GET /api/tail`: `cut` is the part of `skipped` the frame's limit left behind, so
/// `skipped - cut` is the part the ring evicted and is the only part that is gone.
#[test]
fn the_tail_frame_separates_eviction_from_the_frames_own_limit() {
    // a ring of 64 against 200-plus events, so a reader at id 0 has genuinely lost lines
    let r = Running::start("tailcut", 64, false, 2);
    let events = r.feed(LOGS);
    assert!(events > 200, "the corpus must be larger than one frame's limit, got {events}");
    r.wait_for(events);
    let what = "GET /api/tail";

    // the whole ring in one frame: nothing is cut, and what is missing was evicted
    let (st, whole) = json(&r.addr, "GET", "/api/tail?after=0&limit=500");
    assert_eq!(st, 200, "{what}: {whole}");
    let held = whole["events"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
    assert_eq!(held, 64, "{what}: the ring holds its capacity after {events} events, got {held}");
    let cut_whole = u64_at(&whole, "cut", what);
    let evicted = u64_at(&whole, "skipped", what);
    assert_eq!(cut_whole, 0, "{what}: a frame whose limit is above the ring's size cuts nothing, got {cut_whole}");
    assert_eq!(evicted, events - 1 - held, "{what}: skipped must be the {events} events minus id 0, minus the {held} still held");

    // the same position under a smaller limit: the extra lines are cut, not evicted
    let (st, small) = json(&r.addr, "GET", "/api/tail?after=0&limit=8");
    assert_eq!(st, 200, "{what}: {small}");
    let carried = small["events"].as_array().map(|a| a.len()).unwrap_or(0) as u64;
    assert_eq!(carried, 8, "{what}: ?limit=8 carries 8 events, got {carried}");
    let cut = u64_at(&small, "cut", what);
    let skipped = u64_at(&small, "skipped", what);
    assert_eq!(cut, held - carried, "{what}: cut must be the {held} held lines the frame did not carry");
    assert_eq!(skipped, evicted + cut, "{what}: skipped stays the total: {evicted} evicted + {cut} cut");

    // a caller who is up to date: no eviction, no cut, and a cut line is still in the ring
    let latest = u64_at(&whole, "latest_raw_id", what);
    let (_, fresh) = json(&r.addr, "GET", &format!("/api/tail?after={}&limit=500", latest - 2));
    assert_eq!(
        (u64_at(&fresh, "skipped", what), u64_at(&fresh, "cut", what)),
        (0, 0),
        "{what}: a caller two events behind missed nothing"
    );
    let (_, again) = json(&r.addr, "GET", "/api/tail?after=0&limit=500");
    assert_eq!(u64_at(&again, "cut", what), 0, "{what}: a cut line stays in the ring, so the next full frame carries it");

    r.stop();
}

// ------------------------------------------------------------------- item 2

/// `GET /api/events/{id}`: the emitted line from the tail or from the output, `?bytes=0`,
/// the bytes route, and a 404 on both routes for an id that was never issued.
#[test]
fn traceback_reads_the_emitted_line_and_serves_the_raw_bytes() {
    // a tail ring of 5 events, so an early id can only come from the output
    let r = Running::start("traceback", 5, false, 1);
    let bytes = std::fs::read(repo().join("samples/cisco_asa.log")).unwrap();
    let events = r.feed(&["cisco_asa.log"]);
    r.wait_for(events);
    assert!(events > 5, "the sample must outrun the tail ring: {events} events");

    // an id still in the ring
    let last = events - 1;
    let what = format!("GET /api/events/{last}");
    let (st, t) = json(&r.addr, "GET", &format!("/api/events/{last}"));
    assert_eq!(st, 200, "{what}: {t}");
    present(&t, "emitted", &what);
    assert_eq!(t["emitted_from"], "tail", "{what}: `emitted_from` must be \"tail\" for an id still in the tail ring, got {}", t["emitted_from"]);

    // an id that scrolled out of the ring: read from the JSON Lines output
    let what = "GET /api/events/0";
    let (st, t0) = json(&r.addr, "GET", "/api/events/0");
    assert_eq!(st, 200, "{what}: {t0}");
    present(&t0, "emitted", what);
    assert_eq!(
        t0["emitted_from"], "output",
        "{what}: with a tail ring of 5 and {events} events, `emitted_from` must be \"output\" (`emitted` stays non-null, never null because it scrolled out), got {}",
        t0["emitted_from"]
    );
    assert_eq!(u64_at(&t0, "emitted.ulpf.raw_id", what), 0, "{what}: emitted.ulpf.raw_id must be the requested id");
    let bytes_len = u64_at(&t0, "bytes_len", what);

    // ?bytes=0: text and hex dropped, everything else as before
    let what = "GET /api/events/0?bytes=0";
    let (st, thin) = json(&r.addr, "GET", "/api/events/0?bytes=0");
    assert_eq!(st, 200, "{what}: {thin}");
    assert!(thin["text"].is_null(), "{what}: `text` must be null, got {}", brief(&thin["text"]));
    assert!(thin["hex"].is_null(), "{what}: `hex` must be null, got {}", brief(&thin["hex"]));
    assert_eq!(u64_at(&thin, "bytes_len", what), bytes_len, "{what}: `bytes_len` must still be the record's length");
    for f in ["raw_id", "source", "receipt", "receipt_nanos", "stored_sha256", "recomputed_sha256", "digest_match", "emitted", "emitted_from", "now"] {
        present(&thin, f, what);
    }

    // the bytes on their own
    let what = "GET /api/events/0/bytes";
    let resp = http(&r.addr, "GET", "/api/events/0/bytes");
    assert_eq!(resp.status, 200, "{what}: expected 200, got {}: {}", resp.status, resp.text());
    let ct = resp.header("content-type").unwrap_or_default();
    assert!(ct.contains("application/octet-stream"), "{what}: Content-Type must be application/octet-stream, got `{ct}`");
    assert_eq!(
        resp.body,
        first_event(&bytes),
        "{what}: the body must be the record's exact bytes, terminator included (got {:?}, want {:?})",
        String::from_utf8_lossy(&resp.body),
        String::from_utf8_lossy(&first_event(&bytes))
    );
    let len = resp.header("content-length").unwrap_or_default();
    assert_eq!(len, bytes_len.to_string(), "{what}: Content-Length must equal the traceback's bytes_len ({bytes_len}), got `{len}`");

    // an id that was never issued, on both routes
    let (st, e) = json(&r.addr, "GET", "/api/events/424242");
    assert_eq!(st, 404, "GET /api/events/424242: {e}");
    assert_eq!(e["reason"], "not_found", "GET /api/events/424242: `reason` must be \"not_found\", got {}", e["reason"]);
    let resp = http(&r.addr, "GET", "/api/events/424242/bytes");
    assert_eq!(resp.status, 404, "GET /api/events/424242/bytes: expected 404, got {}: {}", resp.status, resp.text());
    let e: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    assert_eq!(e["reason"], "not_found", "GET /api/events/424242/bytes: the 404 body must carry `reason` = \"not_found\", got {}", resp.text());
    r.stop();
}

// ------------------------------------------------------------------- item 3

/// The eleven Parquet columns, in D64's order (crates/ulpf-parquet/src/lib.rs `SCHEMA`).
const CSV_COLUMNS: [&str; 11] = ["raw_id", "time", "parser", "source", "class_uid", "normalized", "src_ip", "dst_ip", "user", "device", "dst_port"];

/// One RFC 4180 row into its fields (no embedded newline: a JSON line carries none).
fn csv_fields(row: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = row.chars().peekable();
    let mut quoted = false;
    let mut fresh = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' && fresh {
            quoted = true;
            fresh = false;
        } else if c == ',' {
            out.push(std::mem::take(&mut cur));
            fresh = true;
        } else {
            cur.push(c);
            fresh = false;
        }
    }
    out.push(cur);
    out
}

/// `GET /api/export`: the output file streamed, bounded by raw id, filtered by terms, as CSV.
#[test]
fn export_streams_the_output_file_bounded_filtered_and_as_csv() {
    let r = Running::start("export", 500, false, 1);
    let events = r.feed(LOGS);
    r.wait_for(events);
    let on_disk = String::from_utf8(std::fs::read(r.output()).unwrap()).unwrap();
    let lines: Vec<&str> = on_disk.lines().collect();
    assert_eq!(lines.len() as u64, events, "the output file should hold every emitted line before the export is asked for");
    let joined = |ls: &[&str]| ls.iter().map(|l| format!("{l}\n")).collect::<String>();

    // the default: the output as it is on disk, up to the last complete line
    let what = "GET /api/export";
    let resp = http(&r.addr, "GET", "/api/export");
    assert_eq!(resp.status, 200, "{what}: expected 200, got {}: {}", resp.status, resp.text());
    let ct = resp.header("content-type").unwrap_or_default();
    assert!(ct.contains("application/x-ndjson"), "{what}: Content-Type must be application/x-ndjson, got `{ct}`");
    let cd = resp.header("content-disposition").unwrap_or_default();
    assert!(cd.contains("attachment"), "{what}: Content-Disposition must name an attachment, got `{cd}`");
    assert_eq!(resp.text(), joined(&lines), "{what}: the body must be the output file's content up to the last complete line");

    // from/to bound the lines by ulpf.raw_id, inclusively
    let from = raw_id_of(lines[5]);
    let to = raw_id_of(lines[15]);
    let what = "GET /api/export?from&to";
    let want: Vec<&str> = lines.iter().copied().filter(|l| (from..=to).contains(&raw_id_of(l))).collect();
    assert_eq!(want.len(), 11, "the eleven lines between raw ids {from} and {to} inclusive");
    let resp = http(&r.addr, "GET", &format!("/api/export?from={from}&to={to}"));
    assert_eq!(resp.status, 200, "{what}: expected 200, got {}: {}", resp.status, resp.text());
    assert_eq!(resp.text(), joined(&want), "{what}: from={from} and to={to} must bound the lines by ulpf.raw_id inclusively");

    // q: every term in the line's text, case-insensitively
    let what = "GET /api/export?q=DENY+tcp";
    let want: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("deny") && lower.contains("tcp")
        })
        .collect();
    assert!(!want.is_empty() && want.len() < lines.len(), "the samples must hold some but not all lines with both terms, got {} of {}", want.len(), lines.len());
    let resp = http(&r.addr, "GET", "/api/export?q=DENY+tcp");
    assert_eq!(resp.status, 200, "{what}: expected 200, got {}: {}", resp.status, resp.text());
    assert_eq!(resp.text(), joined(&want), "{what}: `q` must send exactly the lines whose text contains every term, case-insensitively");

    // csv: the eleven columns, one row per line, RFC 4180 quoting
    let what = "GET /api/export?format=csv";
    let resp = http(&r.addr, "GET", "/api/export?format=csv");
    assert_eq!(resp.status, 200, "{what}: expected 200, got {}: {}", resp.status, resp.text());
    let cd = resp.header("content-disposition").unwrap_or_default();
    assert!(cd.contains("attachment"), "{what}: Content-Disposition must name an attachment, got `{cd}`");
    let body = resp.text();
    let rows: Vec<&str> = body.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()).collect();
    assert!(!rows.is_empty(), "{what}: the body must carry a header row and one row per line, got nothing");
    assert_eq!(rows[0], CSV_COLUMNS.join(","), "{what}: the header row must be the eleven columns in D64's order");
    assert_eq!(rows.len() - 1, lines.len(), "{what}: one row per exported line ({} lines, {} rows)", lines.len(), rows.len() - 1);
    assert!(rows[1..].iter().any(|r| r.contains("\"\"")), "{what}: the `normalized` cell must be RFC 4180 quoted with its inner quotes doubled");
    for (row, line) in rows[1..].iter().zip(&lines) {
        let f = csv_fields(row);
        assert_eq!(f.len(), CSV_COLUMNS.len(), "{what}: every row has eleven fields; this one has {}: {row}", f.len());
        assert_eq!(f[0], raw_id_of(line).to_string(), "{what}: the `raw_id` column must match the line's ulpf.raw_id");
        assert_eq!(f[5], *line, "{what}: the `normalized` column is the JSON line itself, unquoting back to exactly what the sink wrote");
    }
    r.stop();
}

// ------------------------------------------------------------------- item 4

/// `GET /api/pivot`: the cursor pair pages without repeating or skipping an event, and
/// every query reports where its time went.
#[test]
fn pivot_pages_by_the_cursor_pair_and_reports_its_timings() {
    let r = Running::start("pivot", 500, true, 1);
    let events = r.feed(LOGS);
    r.wait_for(events);

    // the busiest dst_port in the index: the samples repeat a handful of ports
    let mut value = String::new();
    wait_until("a dst_port with more than three events in the pivot index", || {
        let (st, v) = json(&r.addr, "GET", "/api/entities?kind=dst_port&limit=1");
        if st != 200 {
            return false;
        }
        let top = &v["entities"][0];
        if top["events"].as_u64().unwrap_or(0) > 3 {
            value = top["value"].as_str().unwrap_or_default().to_string();
            true
        } else {
            false
        }
    });

    let what = "GET /api/pivot";
    let limit = 5u64;
    let (st, page) = json(&r.addr, "GET", &format!("/api/pivot?kind=dst_port&value={value}&limit={limit}"));
    assert_eq!(st, 200, "{what}: {page}");
    let total = u64_at(&page, "total", what);
    assert!(total > limit, "the test needs an entity with more than one page: dst_port {value} has {total} events");

    // elapsed_ms: an object naming each part of the query
    let e = present(&page, "elapsed_ms", what);
    assert!(e.is_object(), "{what}: `elapsed_ms` must be an object, got {e}");
    for k in ["header", "timeline", "related", "lines", "total"] {
        let ms = f64_at(&page, &format!("elapsed_ms.{k}"), what);
        assert!(ms >= 0.0, "{what}: elapsed_ms.{k} must not be negative, got {ms}");
    }
    let _ = u64_at(&page, "related_over", what);

    // paging: the (time, raw id) cursor neither repeats nor skips an event
    let mut seen: Vec<u64> = Vec::new();
    let mut page = page;
    for guard in 0.. {
        assert!(guard < 200, "{what}: paging did not reach the last page in 200 pages");
        let rows = page["events"].as_array().unwrap_or_else(|| panic!("{what}: `events` must be an array, got {}", brief(&page)));
        assert!(rows.len() as u64 <= limit, "{what}: a page holds at most `limit` ({limit}) events, got {}", rows.len());
        for ev in rows {
            seen.push(ev["raw_id"].as_u64().unwrap_or_else(|| panic!("{what}: every event carries `raw_id`, got {ev}")));
        }
        let next_before = page["next_before"].clone();
        let next_before_id = page["next_before_id"].clone();
        if next_before.is_null() {
            assert!(next_before_id.is_null(), "{what}: `next_before_id` must be null on the last page, where `next_before` is null; got {next_before_id}");
            break;
        }
        let before = next_before.as_i64().unwrap_or_else(|| panic!("{what}: `next_before` must be a time in ms, got {next_before}"));
        assert!(!next_before_id.is_null(), "{what}: `next_before_id` must be present whenever `next_before` is (here {before})");
        let before_id = next_before_id.as_u64().unwrap_or_else(|| panic!("{what}: `next_before_id` must be a raw id, got {next_before_id}"));
        let (st, next) = json(&r.addr, "GET", &format!("/api/pivot?kind=dst_port&value={value}&limit={limit}&before={before}&before_id={before_id}"));
        assert_eq!(st, 200, "{what}: paging with before={before} before_id={before_id}: {next}");
        page = next;
    }
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), seen.len(), "{what}: paging with before+before_id repeated an event: {} rows, {} distinct raw ids", seen.len(), unique.len());
    assert_eq!(seen.len() as u64, total, "{what}: paging must visit every one of dst_port {value}'s {total} events, saw {}", seen.len());
    r.stop();
}
