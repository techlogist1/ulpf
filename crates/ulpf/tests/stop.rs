//! Stop means stopped (D82): once `serve` returns, no file under the store, the output or
//! the pending directory is still open in this process. Unix tolerates a leaked handle;
//! Windows refuses to remove the directory while one exists, which is where a Windows
//! tester found this.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use ulpf::engine::{Config, Live, TracebackError, serve};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-stop-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Paths of this process's open file descriptors. Mappings are not descriptors, and the
/// store keeps none for its own maps. Empty where the platform has no way to ask (the
/// directory removal at the end is the check there).
#[cfg(target_os = "macos")]
fn open_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/dev/fd").unwrap().flatten() {
        let Ok(fd) = entry.file_name().to_string_lossy().parse::<i32>() else { continue };
        let mut buf = [0u8; libc::PATH_MAX as usize];
        // SAFETY: F_GETPATH writes at most PATH_MAX bytes into the buffer it is handed.
        if unsafe { libc::fcntl(fd, libc::F_GETPATH, buf.as_mut_ptr()) } != -1 {
            let len = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            out.push(PathBuf::from(String::from_utf8_lossy(&buf[..len]).into_owned()));
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn open_paths() -> Vec<PathBuf> {
    std::fs::read_dir("/proc/self/fd").unwrap().flatten().filter_map(|e| std::fs::read_link(e.path()).ok()).collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_paths() -> Vec<PathBuf> {
    Vec::new()
}

fn open_under(dir: &Path) -> Vec<PathBuf> {
    let dir = dir.canonicalize().unwrap();
    open_paths().into_iter().filter(|p| p.starts_with(&dir)).collect()
}

#[test]
fn stop_releases_every_file_the_engine_opened() {
    let dir = temp("all");
    let watch = dir.join("in");
    std::fs::create_dir_all(&watch).unwrap();
    let sample = std::fs::read(repo().join("samples/cisco_asa.log")).unwrap();
    std::fs::write(watch.join("a.log"), &sample).unwrap();
    let lines = sample.iter().filter(|b| **b == b'\n').count() as u64;
    // everything the engine can open is on: the store, the output, the pivot index and its
    // read side, the Parquet sink rolling files, the pending directory
    let cfg = Config {
        inputs: vec![watch],
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: repo().join("parsers"),
        mappings: repo().join("mappings"),
        schema: None,
        threads: 2,
        default_offset_secs: 0,
        batch_events: 8,
        queue_batches: 4,
        pending: Some(dir.join("pending")),
        infer_threshold: 64,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        pivot_index: true,
        parquet: Some(dir.join("events.parquet")),
        parquet_roll: Some((4, Duration::from_secs(3600))),
    };
    assert!(open_under(&dir).is_empty());
    let live = Live::open(&cfg, true).unwrap();
    let handle = {
        let live = Arc::clone(&live);
        std::thread::spawn(move || serve(&live, Duration::from_millis(50)))
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while live.metrics.emitted.load(Relaxed) < lines && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(live.metrics.emitted.load(Relaxed), lines);
    // the lazily opened handles too: the traceback reads through the writer, an entity
    // query opens the index's read connection
    assert!(live.traceback(0).is_ok());
    assert!(live.entities(None, "", 5).is_ok());
    let during = open_under(&dir);
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(during.iter().any(|p| p.ends_with("raw.seg")), "the store is open while serving: {during:?}");
    }

    live.stop();
    handle.join().unwrap().unwrap();
    let after = open_under(&dir);
    assert!(after.is_empty(), "still open after stop: {after:?}");
    // a request racing shutdown gets a value, not a panic
    assert!(matches!(live.traceback(0), Err(TracebackError::Io(_))));
    assert!(live.attestation().is_err());
    assert!(live.snapshot().emitted == lines, "the counters outlive the files");
    // the check Windows makes for us: a directory with an open file cannot be removed
    std::fs::remove_dir_all(&dir).unwrap();
}
