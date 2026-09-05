//! The Parquet sink, driven by the output thread.
//!
//! Parquet is an *additional* output, never the primary one: a file is unreadable until
//! its footer lands, so the JSON Lines file stays the thing that is always complete.
//! The row's scalar columns are the entity values the worker already copied out for the
//! pivot index (D55), so they follow the mapping's `[entities]` under any schema, and the
//! line itself is stored verbatim. The whole sink lives on the output thread, off the
//! parallel per-event path; with `--parquet` unset nothing here is constructed.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use ulpf_parquet::{ParquetWriter, Row};

use crate::metrics::Metrics;

/// Rows buffered before a row group is flushed: 8192 keeps peak memory near 40 MB.
const ROW_GROUP: usize = 8192;

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
        // a complete file from an earlier run is never overwritten: the sink refuses and
        // the JSON Lines output (which appends) carries on
        if first.exists() {
            return Err(format!("{} already exists; move it or choose another --parquet path", first.display()));
        }
        let w = ParquetWriter::create(&first, ROW_GROUP).map_err(|e| format!("{}: {e}", first.display()))?;
        Ok(Sink { w, base, roll, seq: 0, rows_in_file: 0, opened: Instant::now() })
    }

    /// One emitted event. Infallible: it buffers; `end_batch` does the I/O.
    pub fn push(&mut self, row: Row<'_>) {
        self.w.push(row);
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
