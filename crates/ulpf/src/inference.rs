//! Per-source buffers of unknown events and the thread that turns them into proposals.
//! Workers copy an event here only when no parser claimed it (the one allocation the
//! unknown path pays); the buffer is bounded, and overflow is counted, never a dropped
//! event: the raw store and the output already have it. In `live` mode a source that
//! reaches the threshold (then double it, and so on) is clustered as it arrives; a
//! source that goes quiet with a few lines is clustered after `idle`. In batch mode the
//! whole buffer is clustered once, after the run, so the throughput number stays what
//! it says: ingest to output.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use ulpf_infer::Params;

use crate::metrics::Metrics;
use crate::pending::{Pending, WriteOutcome};

pub struct Inference {
    pub params: Params,
    /// Lines a source needs before its first proposal; 0 disables inference.
    pub threshold: usize,
    pub max_buffer: usize,
    pub idle: Duration,
    pub live: bool,
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    sources: BTreeMap<String, Buffer>,
    stop: bool,
}

struct Buffer {
    /// `(raw id, event bytes)`; workers arrive in any order, inference sees raw-id order
    /// so the same input always yields the same proposal.
    lines: Vec<(u64, Vec<u8>)>,
    last_added: Instant,
    next_run: usize,
    ran_at: usize,
}

fn ordered(lines: &[(u64, Vec<u8>)]) -> Vec<Vec<u8>> {
    let mut v: Vec<&(u64, Vec<u8>)> = lines.iter().collect();
    v.sort_by_key(|(id, _)| *id);
    v.into_iter().map(|(_, l)| l.clone()).collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BufferInfo {
    pub buffered: usize,
    pub runs: bool,
}

impl Inference {
    pub fn new(params: Params, threshold: usize, max_buffer: usize, idle: Duration, live: bool) -> Inference {
        Inference { params, threshold, max_buffer: max_buffer.max(threshold.max(8)), idle, live, state: Mutex::new(State::default()), wake: Condvar::new() }
    }

    pub fn enabled(&self) -> bool {
        self.threshold > 0
    }

    /// An event no parser claimed. Buffers a copy; a full buffer counts and drops the copy.
    pub fn offer(&self, source: &str, raw_id: u64, event: &[u8], metrics: &Metrics) {
        if !self.enabled() {
            return;
        }
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let threshold = self.threshold;
        let buf = st.sources.entry(source.to_string()).or_insert_with(|| Buffer { lines: Vec::new(), last_added: Instant::now(), next_run: threshold, ran_at: 0 });
        if buf.lines.len() >= self.max_buffer {
            metrics.infer_buffer_full.fetch_add(1, Relaxed);
            return;
        }
        buf.lines.push((raw_id, event.to_vec()));
        buf.last_added = Instant::now();
        metrics.infer_buffered.fetch_add(1, Relaxed);
        if self.live && buf.lines.len() >= buf.next_run {
            self.wake.notify_one();
        }
    }

    pub fn buffered(&self) -> BTreeMap<String, usize> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).sources.iter().map(|(k, v)| (k.clone(), v.lines.len())).collect()
    }

    /// A copy of a source's buffered lines (approval re-tests detection over them).
    pub fn lines(&self, source: &str) -> Vec<Vec<u8>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).sources.get(source).map(|b| ordered(&b.lines)).unwrap_or_default()
    }

    /// Forgets a source's buffer: its lines are now covered by an approved parser.
    pub fn clear(&self, source: &str) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).sources.remove(source);
    }

    pub fn stop(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stop = true;
        self.wake.notify_all();
    }

    /// Sources due for a run: over their next threshold, or quiet for `idle` with at least
    /// `min_support` lines the last run did not see (so a burst that stops short of the
    /// next doubling is still clustered in full). `final_flush` takes every such source.
    fn take_due(&self, st: &mut State, final_flush: bool) -> Vec<(String, Vec<Vec<u8>>)> {
        let mut due = Vec::new();
        for (name, buf) in st.sources.iter_mut() {
            let n = buf.lines.len();
            let new_lines = n >= buf.ran_at + self.params.min_support;
            let over = self.live && n >= buf.next_run;
            let idle = self.live && new_lines && buf.last_added.elapsed() >= self.idle;
            let flush = final_flush && new_lines;
            if over || idle || flush {
                buf.ran_at = n;
                buf.next_run = (n * 2).max(n + self.threshold);
                due.push((name.clone(), ordered(&buf.lines)));
            }
        }
        due
    }

    /// The inference thread body. Returns after `stop` and the final flush.
    pub fn run_thread(&self, pending: &Pending, metrics: &Metrics, pending_generation: &AtomicU64) {
        if !self.enabled() {
            return;
        }
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if st.stop {
                break;
            }
            let due = self.take_due(&mut st, false);
            if due.is_empty() {
                st = self.wake.wait_timeout(st, Duration::from_millis(500)).unwrap_or_else(|e| e.into_inner()).0;
                continue;
            }
            drop(st);
            for (source, lines) in due {
                self.run_one(&source, &lines, pending, metrics, pending_generation);
            }
            st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        }
        let due = self.take_due(&mut st, true);
        drop(st);
        for (source, lines) in due {
            self.run_one(&source, &lines, pending, metrics, pending_generation);
        }
    }

    fn run_one(&self, source: &str, lines: &[Vec<u8>], pending: &Pending, metrics: &Metrics, pending_generation: &AtomicU64) {
        let refs: Vec<&[u8]> = lines.iter().map(Vec::as_slice).collect();
        let proposal = ulpf_infer::infer(source, &refs, &self.params);
        metrics.infer_runs.fetch_add(1, Relaxed);
        metrics.infer_lines_templated.fetch_add(proposal.evidence.lines_used, Relaxed);
        metrics.infer_lines_unmatched.fetch_add(proposal.evidence.unmatched.count, Relaxed);
        match pending.write(&proposal, lines) {
            Ok(WriteOutcome::Written) => {
                metrics.proposals_written.fetch_add(1, Relaxed);
                pending_generation.fetch_add(1, Relaxed);
            }
            Ok(WriteOutcome::Replaced) => {
                metrics.proposals_replaced.fetch_add(1, Relaxed);
                pending_generation.fetch_add(1, Relaxed);
            }
            Ok(other) => {
                if let Some(reason) = other.skip_reason() {
                    metrics.skipped(reason);
                }
            }
            Err(e) => {
                // a pending directory that cannot be written is an operator problem; the
                // proposal is lost but the run is not
                eprintln!("ulpf: proposal for {source} not written: {e}");
            }
        }
    }
}
