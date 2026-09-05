//! Done-item 13: the Parquet sink is an additional output of the real engine. One row
//! per emitted line, the JSON line stored verbatim beside its columns, and in watch mode
//! files that a reader can open the moment they appear.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::RowAccessor;
use ulpf::engine::{Config, Live, run, serve};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-parquet-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &Path, inputs: Vec<PathBuf>, parquet: Option<PathBuf>, roll: Option<(u64, Duration)>) -> Config {
    Config {
        inputs,
        store: dir.join("store"),
        output: dir.join("out.jsonl"),
        parsers: repo().join("parsers"),
        mappings: repo().join("mappings"),
        schema: None,
        threads: 3,
        default_offset_secs: 0,
        batch_events: 8,
        queue_batches: 4,
        pending: None,
        infer_threshold: 0,
        tail_capacity: 16,
        receipt_nanos: None,
        syslog_udp: None,
        syslog_tcp: None,
        parquet,
        parquet_roll: roll,
    }
}

fn rows(path: &Path) -> Vec<(i64, String)> {
    let reader = SerializedFileReader::new(std::fs::File::open(path).unwrap()).unwrap();
    reader.get_row_iter(None).unwrap().map(|r| { let r = r.unwrap(); (r.get_long(0).unwrap(), r.get_string(5).unwrap().clone()) }).collect()
}

#[test]
fn samples_run_writes_one_row_per_emitted_line() {
    let dir = temp("run");
    let file = dir.join("events.parquet");
    let cfg = config(&dir, vec![repo().join("samples")], Some(file.clone()), None);
    let report = run(&cfg).unwrap();
    let s = &report.snapshot;
    assert!(s.emitted > 0);
    assert_eq!(s.parquet_rows, s.emitted, "every emitted line is a row");
    assert_eq!(s.parquet_files, 1);
    assert_eq!(s.parquet_errors, 0);
    assert!(file.exists(), "the footer landed and the file was renamed");
    assert!(!dir.join("events.parquet.part").exists());

    let reader = SerializedFileReader::new(std::fs::File::open(&file).unwrap()).unwrap();
    assert_eq!(reader.metadata().file_metadata().num_rows() as u64, s.emitted);
    let mut expect = 0i64;
    for (raw_id, normalized) in rows(&file) {
        assert_eq!(raw_id, expect, "raw ids are 0..n in emitted order");
        let v: serde_json::Value = serde_json::from_str(&normalized).expect("the normalized column is the JSON line");
        assert_eq!(v["ulpf"]["raw_id"], raw_id, "the column and the line agree on the raw id");
        expect += 1;
    }
    assert_eq!(expect as u64, s.emitted);

    // the JSON Lines output is untouched by the sink, line for line
    let jsonl = std::fs::read_to_string(dir.join("out.jsonl")).unwrap();
    assert_eq!(jsonl.lines().count() as u64, s.emitted);
    assert_eq!(jsonl.lines().next().unwrap(), rows(&file)[0].1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn watch_mode_rolls_closed_files_and_leaves_no_part_behind() {
    let dir = temp("roll");
    let watch = dir.join("in");
    std::fs::create_dir_all(&watch).unwrap();
    let sample = std::fs::read(repo().join("samples/cisco_asa.log")).unwrap();
    std::fs::write(watch.join("a.log"), &sample).unwrap();
    // roll after 4 rows, so a handful of events produces several closed files
    let cfg = config(&dir, vec![watch.clone()], Some(dir.join("events.parquet")), Some((4, Duration::from_secs(3600))));
    let live = Live::open(&cfg, true).unwrap();
    let handle = {
        let live = Arc::clone(&live);
        std::thread::spawn(move || serve(&live, Duration::from_millis(50)))
    };
    std::thread::sleep(Duration::from_millis(400));
    std::fs::write(watch.join("b.log"), &sample).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while live.metrics.emitted.load(std::sync::atomic::Ordering::Relaxed) < 2 * sample.iter().filter(|b| **b == b'\n').count() as u64 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    live.stop();
    let report = handle.join().unwrap().unwrap();
    let s = &report.snapshot;
    assert!(s.emitted >= 2, "{s}");
    assert_eq!(s.parquet_rows, s.emitted);
    assert!(s.parquet_files >= 2, "rolling produced {} files", s.parquet_files);
    assert_eq!(s.parquet_errors, 0);

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "parquet")).collect();
    files.sort();
    assert_eq!(files.len() as u64, s.parquet_files, "every counted file is on disk, closed");
    assert_eq!(std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().to_string_lossy().ends_with(".part")).count(), 0, "no unfinished footer is left behind");
    let mut all: Vec<(i64, String)> = files.iter().flat_map(|f| rows(f)).collect();
    assert_eq!(all.len() as u64, s.emitted, "the rows of every file together are the whole output");
    all.sort_by_key(|(id, _)| *id);
    for (i, (raw_id, normalized)) in all.iter().enumerate() {
        assert_eq!(*raw_id, i as i64);
        let v: serde_json::Value = serde_json::from_str(normalized).unwrap();
        assert_eq!(v["ulpf"]["raw_id"], *raw_id);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
