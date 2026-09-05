//! The Parquet sink, driven by the output thread.
//!
//! Parquet is an *additional* output, never the primary one: a file is unreadable until
//! its footer lands, so the JSON Lines file stays the thing that is always complete.
//! The row's ten columns are read back out of the line the pipeline just emitted, so a
//! Parquet row can never disagree with its JSON: same bytes, same source of truth. The
//! whole sink lives on the output thread, off the parallel per-event path; with
//! `--parquet` unset nothing here is constructed and nothing is called.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use serde::Deserialize;
use ulpf_parquet::{ParquetWriter, Row};

use crate::metrics::Metrics;

/// Rows buffered before a row group is flushed: 8192 keeps peak memory near 40 MB.
const ROW_GROUP: usize = 8192;

/// The fields the columns come from. Everything else in the line is ignored; the whole
/// line is still stored verbatim in `normalized`.
#[derive(Deserialize, Default)]
struct Line<'a> {
    #[serde(default)]
    time: i64,
    #[serde(default)]
    class_uid: i32,
    #[serde(borrow, default)]
    ulpf: Ulpf<'a>,
    #[serde(borrow, default)]
    metadata: Meta<'a>,
    #[serde(borrow, default)]
    src_endpoint: Endpoint<'a>,
    #[serde(borrow, default)]
    dst_endpoint: Endpoint<'a>,
    #[serde(borrow, default)]
    user: User<'a>,
    #[serde(borrow, default)]
    device: Device<'a>,
}

#[derive(Deserialize, Default)]
struct Ulpf<'a> {
    #[serde(borrow, default)]
    parser: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Default)]
struct Meta<'a> {
    #[serde(borrow, default)]
    log_name: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Default)]
struct Endpoint<'a> {
    #[serde(borrow, default)]
    ip: Option<Cow<'a, str>>,
    #[serde(default)]
    port: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct User<'a> {
    #[serde(borrow, default)]
    name: Option<Cow<'a, str>>,
}

#[derive(Deserialize, Default)]
struct Device<'a> {
    #[serde(borrow, default)]
    hostname: Option<Cow<'a, str>>,
}

/// A port the mapping could not turn into an integer stays a string in the JSON; the
/// column is typed, so it takes the integer or nothing.
fn port(v: &Option<serde_json::Value>) -> Option<i32> {
    let v = v.as_ref()?;
    let n = v.as_i64().or_else(|| v.as_str()?.parse().ok())?;
    i32::try_from(n).ok()
}

pub struct Sink {
    w: ParquetWriter,
    /// `run`: the exact `--parquet` path, one file. `serve`: the stem that
    /// `<stem>.<seq>.parquet` is built from.
    base: PathBuf,
    roll: Option<(u64, Duration)>,
    seq: u64,
    rows_in_file: u64,
    opened: Instant,
}

/// `--parquet out/events.parquet` in serve mode rolls `out/events.0.parquet`,
/// `out/events.1.parquet`, ...: the extension is the sink's, not part of the stem.
fn stem(path: &Path) -> PathBuf {
    match path.extension() {
        Some(e) if e == "parquet" => path.with_extension(""),
        _ => path.to_path_buf(),
    }
}

fn nth(base: &Path, seq: u64) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(format!(".{seq}.parquet"));
    PathBuf::from(s)
}

impl Sink {
    /// Opens the first file. `roll` is `(rows, seconds)` in watch mode and `None` in
    /// batch mode, where one run writes one file.
    pub fn open(path: &Path, roll: Option<(u64, Duration)>) -> Result<Sink, String> {
        let base = if roll.is_some() { stem(path) } else { path.to_path_buf() };
        let first = if roll.is_some() { nth(&base, 0) } else { base.clone() };
        let w = ParquetWriter::create(&first, ROW_GROUP).map_err(|e| format!("{}: {e}", first.display()))?;
        Ok(Sink { w, base, roll, seq: 0, rows_in_file: 0, opened: Instant::now() })
    }

    /// One row from one emitted line. A line that will not parse as JSON (impossible
    /// from this pipeline, but never a panic) still becomes a row: raw id and the bytes.
    pub fn push(&mut self, raw_id: u64, line: &[u8], metrics: &Metrics) {
        // ponytail: the ten columns are read back out of the line the pipeline just
        // emitted, which costs one serde pass over it (measured: 2.6 us of the 4.1 us a
        // row costs on this thread; the rest is the copy and SNAPPY of the JSON column).
        // Upgrade path when that matters: carry the same scalars out of the worker, which
        // already holds them before serialising, and push them straight into `Row`.
        let parsed: Line<'_> = match serde_json::from_slice(line) {
            Ok(l) => l,
            Err(_) => {
                metrics.parquet_errors.fetch_add(1, Relaxed);
                Line::default()
            }
        };
        self.w.push(Row {
            raw_id: raw_id as i64,
            time_ms: parsed.time,
            parser: parsed.ulpf.parser.as_deref(),
            source: parsed.metadata.log_name.as_deref().unwrap_or_default(),
            class_uid: parsed.class_uid,
            normalized: line,
            src_ip: parsed.src_endpoint.ip.as_deref(),
            dst_ip: parsed.dst_endpoint.ip.as_deref(),
            user: parsed.user.name.as_deref(),
            device: parsed.device.hostname.as_deref(),
            dst_port: port(&parsed.dst_endpoint.port),
        });
        self.rows_in_file += 1;
    }

    /// Once per engine batch: flush a row group if one is full, then roll if this file
    /// has had enough rows or enough time. Returns the sink, or the error that killed it.
    pub fn end_batch(&mut self, metrics: &Metrics) -> Result<(), String> {
        self.w.end_batch().map_err(|e| e.to_string())?;
        metrics.parquet_rows.store(self.w.stats().rows, Relaxed);
        let Some((rows, secs)) = self.roll else { return Ok(()) };
        if self.rows_in_file < rows && self.opened.elapsed() < secs {
            return Ok(());
        }
        self.seq += 1;
        let next = nth(&self.base, self.seq);
        let stats = self.w.roll(&next).map_err(|e| format!("{}: {e}", next.display()))?;
        metrics.parquet_files.store(stats.files, Relaxed);
        metrics.parquet_rows.store(stats.rows, Relaxed);
        self.rows_in_file = 0;
        self.opened = Instant::now();
        Ok(())
    }

    /// Footer, rename, done. The rows only become readable here.
    pub fn finish(self, metrics: &Metrics) {
        match self.w.finish() {
            Ok(stats) => {
                metrics.parquet_files.store(stats.files, Relaxed);
                metrics.parquet_rows.store(stats.rows, Relaxed);
            }
            Err(e) => {
                metrics.parquet_errors.fetch_add(1, Relaxed);
                eprintln!("ulpf: parquet: closing the file failed, its rows are not readable: {e}");
            }
        }
    }
}
