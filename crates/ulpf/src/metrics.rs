//! Per-stage counters. Workers accumulate `LocalCounts` per batch and flush once, so
//! the hot path never touches a shared atomic per event. `Snapshot` is what the CLI
//! prints and what the server session will expose over the wire.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use serde::Serialize;
use ulpf_parse::ParseFailure;

pub const TIME_ERROR_REASONS: [&str; 3] = ["empty", "no_match", "out_of_range"];
pub const SKIP_REASONS: [&str; 4] = ["edited", "duplicate", "rejected", "no_templates"];

#[derive(Default)]
pub struct Metrics {
    pub files: AtomicU64,
    pub files_failed: AtomicU64,
    pub bytes: AtomicU64,
    pub framed: AtomicU64,
    pub stored: AtomicU64,
    pub detected: AtomicU64,
    pub no_parser: AtomicU64,
    pub parsed: AtomicU64,
    pub parse_failed: [AtomicU64; 6],
    pub sub_matched: AtomicU64,
    pub sub_no_match: AtomicU64,
    pub sub_uncovered: AtomicU64,
    pub time_from_receipt: AtomicU64,
    pub time_error: [AtomicU64; 3],
    pub normalized: AtomicU64,
    pub class_unknown: AtomicU64,
    pub enum_other: AtomicU64,
    pub unmapped_fields: AtomicU64,
    pub utf8_lossy: AtomicU64,
    pub emitted: AtomicU64,
    pub output_bytes: AtomicU64,
    /// The additional Parquet sink: rows pushed, files closed (a file is only readable
    /// once closed), and writes that failed. A Parquet error never stops the run.
    pub parquet_rows: AtomicU64,
    pub parquet_files: AtomicU64,
    pub parquet_errors: AtomicU64,
    pub batches: AtomicU64,
    pub queue_high_water: AtomicU64,
    /// Times the ingest thread found the queue full and had to wait.
    pub backpressure_blocks: AtomicU64,
    // v1: inference and review. Buffered lines are copies of unknown events, never the
    // events themselves; a proposal is a file in the pending directory.
    pub infer_buffered: AtomicU64,
    pub infer_buffer_full: AtomicU64,
    pub infer_runs: AtomicU64,
    pub infer_lines_templated: AtomicU64,
    pub infer_lines_unmatched: AtomicU64,
    pub proposals_written: AtomicU64,
    pub proposals_replaced: AtomicU64,
    pub proposals_skipped: [AtomicU64; 4],
    pub approved: AtomicU64,
    pub rejected: AtomicU64,
    pub reloads: AtomicU64,
}

/// One worker's counts for one batch.
#[derive(Default, Clone, Copy)]
pub struct LocalCounts {
    pub detected: u64,
    pub no_parser: u64,
    pub parsed: u64,
    pub parse_failed: [u64; 6],
    pub sub_matched: u64,
    pub sub_no_match: u64,
    pub sub_uncovered: u64,
    pub time_from_receipt: u64,
    pub time_error: [u64; 3],
    pub normalized: u64,
    pub class_unknown: u64,
    pub enum_other: u64,
    pub unmapped_fields: u64,
    pub utf8_lossy: u64,
}

impl LocalCounts {
    pub fn parse_failed(&mut self, f: ParseFailure) {
        self.parse_failed[ParseFailure::ALL.iter().position(|x| *x == f).unwrap_or(0)] += 1;
    }

    pub fn time_error(&mut self, reason: &str) {
        if let Some(i) = TIME_ERROR_REASONS.iter().position(|r| *r == reason) {
            self.time_error[i] += 1;
        }
    }
}

impl Metrics {
    pub fn add(&self, c: &LocalCounts) {
        self.detected.fetch_add(c.detected, Relaxed);
        self.no_parser.fetch_add(c.no_parser, Relaxed);
        self.parsed.fetch_add(c.parsed, Relaxed);
        for (a, v) in self.parse_failed.iter().zip(c.parse_failed) {
            a.fetch_add(v, Relaxed);
        }
        self.sub_matched.fetch_add(c.sub_matched, Relaxed);
        self.sub_no_match.fetch_add(c.sub_no_match, Relaxed);
        self.sub_uncovered.fetch_add(c.sub_uncovered, Relaxed);
        self.time_from_receipt.fetch_add(c.time_from_receipt, Relaxed);
        for (a, v) in self.time_error.iter().zip(c.time_error) {
            a.fetch_add(v, Relaxed);
        }
        self.normalized.fetch_add(c.normalized, Relaxed);
        self.class_unknown.fetch_add(c.class_unknown, Relaxed);
        self.enum_other.fetch_add(c.enum_other, Relaxed);
        self.unmapped_fields.fetch_add(c.unmapped_fields, Relaxed);
        self.utf8_lossy.fetch_add(c.utf8_lossy, Relaxed);
    }

    pub fn skipped(&self, reason: &str) {
        if let Some(i) = SKIP_REASONS.iter().position(|r| *r == reason) {
            self.proposals_skipped[i].fetch_add(1, Relaxed);
        }
    }

    pub fn snapshot(&self, elapsed_secs: f64, threads: usize, queue_capacity: usize) -> Snapshot {
        let g = |a: &AtomicU64| a.load(Relaxed);
        let framed = g(&self.framed);
        let bytes = g(&self.bytes);
        Snapshot {
            elapsed_secs,
            threads,
            events_per_sec: if elapsed_secs > 0.0 { framed as f64 / elapsed_secs } else { 0.0 },
            mb_per_sec: if elapsed_secs > 0.0 { bytes as f64 / 1_048_576.0 / elapsed_secs } else { 0.0 },
            files: g(&self.files),
            files_failed: g(&self.files_failed),
            bytes,
            framed,
            stored: g(&self.stored),
            detected: g(&self.detected),
            no_parser: g(&self.no_parser),
            parsed: g(&self.parsed),
            parse_failed: ParseFailure::ALL.iter().zip(&self.parse_failed).map(|(f, a)| (f.reason(), g(a))).filter(|(_, n)| *n > 0).collect(),
            sub_matched: g(&self.sub_matched),
            sub_no_match: g(&self.sub_no_match),
            sub_uncovered: g(&self.sub_uncovered),
            time_from_receipt: g(&self.time_from_receipt),
            time_error: TIME_ERROR_REASONS.iter().zip(&self.time_error).map(|(r, a)| (*r, g(a))).filter(|(_, n)| *n > 0).collect(),
            normalized: g(&self.normalized),
            class_unknown: g(&self.class_unknown),
            enum_other: g(&self.enum_other),
            unmapped_fields: g(&self.unmapped_fields),
            utf8_lossy: g(&self.utf8_lossy),
            emitted: g(&self.emitted),
            output_bytes: g(&self.output_bytes),
            parquet_rows: g(&self.parquet_rows),
            parquet_files: g(&self.parquet_files),
            parquet_errors: g(&self.parquet_errors),
            batches: g(&self.batches),
            queue_high_water: g(&self.queue_high_water),
            queue_capacity: queue_capacity as u64,
            backpressure_blocks: g(&self.backpressure_blocks),
            infer_buffered: g(&self.infer_buffered),
            infer_buffer_full: g(&self.infer_buffer_full),
            infer_runs: g(&self.infer_runs),
            infer_lines_templated: g(&self.infer_lines_templated),
            infer_lines_unmatched: g(&self.infer_lines_unmatched),
            proposals_written: g(&self.proposals_written),
            proposals_replaced: g(&self.proposals_replaced),
            proposals_skipped: SKIP_REASONS.iter().zip(&self.proposals_skipped).map(|(r, a)| (*r, g(a))).filter(|(_, n)| *n > 0).collect(),
            approved: g(&self.approved),
            rejected: g(&self.rejected),
            reloads: g(&self.reloads),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Snapshot {
    pub elapsed_secs: f64,
    pub threads: usize,
    pub events_per_sec: f64,
    pub mb_per_sec: f64,
    pub files: u64,
    pub files_failed: u64,
    pub bytes: u64,
    pub framed: u64,
    pub stored: u64,
    pub detected: u64,
    pub no_parser: u64,
    pub parsed: u64,
    pub parse_failed: Vec<(&'static str, u64)>,
    pub sub_matched: u64,
    pub sub_no_match: u64,
    pub sub_uncovered: u64,
    pub time_from_receipt: u64,
    pub time_error: Vec<(&'static str, u64)>,
    pub normalized: u64,
    pub class_unknown: u64,
    pub enum_other: u64,
    pub unmapped_fields: u64,
    pub utf8_lossy: u64,
    pub emitted: u64,
    pub output_bytes: u64,
    pub parquet_rows: u64,
    pub parquet_files: u64,
    pub parquet_errors: u64,
    pub batches: u64,
    pub queue_high_water: u64,
    pub queue_capacity: u64,
    pub backpressure_blocks: u64,
    pub infer_buffered: u64,
    pub infer_buffer_full: u64,
    pub infer_runs: u64,
    pub infer_lines_templated: u64,
    pub infer_lines_unmatched: u64,
    pub proposals_written: u64,
    pub proposals_replaced: u64,
    pub proposals_skipped: Vec<(&'static str, u64)>,
    pub approved: u64,
    pub rejected: u64,
    pub reloads: u64,
}

fn by_reason(list: &[(&str, u64)]) -> String {
    if list.is_empty() {
        return "none".into();
    }
    list.iter().map(|(r, n)| format!("{r} {n}")).collect::<Vec<_>>().join(", ")
}

impl std::fmt::Display for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "ulpf: {} files ({} failed), {:.2} MB, {} events in {:.3} s -> {:.0} events/s, {:.1} MB/s, {} worker threads",
            self.files, self.files_failed, self.bytes as f64 / 1_048_576.0, self.framed, self.elapsed_secs, self.events_per_sec, self.mb_per_sec, self.threads
        )?;
        writeln!(
            f,
            "stages: framed {}  stored {}  detected {}  no_parser {}  parsed {}  parse_failed {}  normalized {}  emitted {} ({} bytes)",
            self.framed, self.stored, self.detected, self.no_parser, self.parsed,
            self.parse_failed.iter().map(|(_, n)| n).sum::<u64>(), self.normalized, self.emitted, self.output_bytes
        )?;
        writeln!(f, "parse_failed by reason: {}", by_reason(&self.parse_failed))?;
        writeln!(
            f,
            "signals: sub_matched {}  sub_no_match {}  sub_uncovered {}  time_from_receipt {}  time_error [{}]  class_unknown {}  enum_other {}  unmapped_fields {}  utf8_lossy {}",
            self.sub_matched, self.sub_no_match, self.sub_uncovered, self.time_from_receipt, by_reason(&self.time_error), self.class_unknown, self.enum_other, self.unmapped_fields, self.utf8_lossy
        )?;
        if self.parquet_rows + self.parquet_files + self.parquet_errors > 0 {
            writeln!(f, "parquet: rows {}  files closed {}  errors {}", self.parquet_rows, self.parquet_files, self.parquet_errors)?;
        }
        writeln!(
            f,
            "queue: {} batches, high-water {}/{}, backpressure blocks {} (engaged: {})",
            self.batches, self.queue_high_water, self.queue_capacity, self.backpressure_blocks,
            if self.backpressure_blocks > 0 { "yes" } else { "no" }
        )?;
        write!(
            f,
            "inference: buffered {} (buffer full {})  runs {}  lines templated {} unmatched {}  proposals written {} replaced {} skipped [{}]  approved {}  rejected {}  reloads {}",
            self.infer_buffered, self.infer_buffer_full, self.infer_runs, self.infer_lines_templated, self.infer_lines_unmatched,
            self.proposals_written, self.proposals_replaced, by_reason(&self.proposals_skipped), self.approved, self.rejected, self.reloads
        )
    }
}
