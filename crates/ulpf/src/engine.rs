//! The threaded pipeline.
//!
//! One ingest thread memory-maps each input, frames it, appends every event to the raw
//! store (before anything else), and sends batches of byte ranges into a bounded
//! `sync_channel`. When the queue is full the ingest thread blocks: that is the entire
//! saturation policy, because dropping would break raw completeness. N workers detect,
//! parse, normalize and serialize each batch into one buffer. The output thread reorders
//! buffers by batch sequence so the JSON Lines order equals raw id order.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use memmap2::Mmap;
use ulpf_parse::{Parsed, SubStatus};
use ulpf_store::{Framer, RawStore};

use crate::metrics::{LocalCounts, Metrics, Snapshot};
use crate::pipeline::Pipeline;

pub struct Config {
    pub inputs: Vec<PathBuf>,
    pub store: PathBuf,
    /// `-` for stdout.
    pub output: PathBuf,
    pub parsers: PathBuf,
    pub mappings: PathBuf,
    pub schema: Option<String>,
    pub threads: usize,
    pub default_offset_secs: i32,
    pub batch_events: usize,
    pub queue_batches: usize,
}

#[derive(Debug)]
pub struct Report {
    pub snapshot: Snapshot,
    pub load_problems: Vec<String>,
    pub input_problems: Vec<String>,
    pub parsers_loaded: usize,
}

struct FileCtx {
    mmap: Option<Mmap>,
    name: String,
}

impl FileCtx {
    fn bytes(&self) -> &[u8] {
        self.mmap.as_deref().unwrap_or(&[])
    }
}

struct Batch {
    seq: u64,
    file: Arc<FileCtx>,
    receipt_nanos: i64,
    first_raw_id: u64,
    ranges: Vec<std::ops::Range<usize>>,
}

fn now_nanos() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)
}

/// Expands directories (recursively) into a sorted file list.
pub fn collect_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for p in inputs {
        let meta = std::fs::metadata(p).with_context(|| format!("input {}", p.display()))?;
        if meta.is_dir() {
            walk(p, &mut files)?;
        } else {
            files.push(p.clone());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.')) {
            out.push(path);
        }
    }
    Ok(())
}

pub fn run(cfg: &Config) -> Result<Report> {
    let (pipeline, load_problems) = Pipeline::load(&cfg.parsers, &cfg.mappings, cfg.schema.as_deref(), cfg.default_offset_secs)?;
    let parsers_loaded = pipeline.registry.len();
    let pipeline = Arc::new(pipeline);
    let files = collect_inputs(&cfg.inputs)?;
    let mut store = RawStore::open(&cfg.store).with_context(|| format!("opening store {}", cfg.store.display()))?;
    let metrics = Arc::new(Metrics::default());
    let threads = cfg.threads.max(1);
    let queue_cap = cfg.queue_batches.max(1);
    let batch_events = cfg.batch_events.max(1);
    let started_nanos = now_nanos();
    let start = Instant::now();
    let mut input_problems = Vec::new();

    let (batch_tx, batch_rx) = sync_channel::<Batch>(queue_cap);
    let batch_rx = Arc::new(Mutex::new(batch_rx));
    let (out_tx, out_rx) = sync_channel::<(u64, Vec<u8>, u64)>(queue_cap * 2);
    let in_flight = Arc::new(AtomicI64::new(0));

    let output_result: Result<()> = std::thread::scope(|scope| {
        let writer = scope.spawn({
            let metrics = Arc::clone(&metrics);
            let output = cfg.output.clone();
            move || output_thread(out_rx, &output, &metrics)
        });
        let mut workers = Vec::new();
        for _ in 0..threads {
            let rx = Arc::clone(&batch_rx);
            let tx = out_tx.clone();
            let pipeline = Arc::clone(&pipeline);
            let metrics = Arc::clone(&metrics);
            let in_flight = Arc::clone(&in_flight);
            workers.push(scope.spawn(move || worker_thread(rx, tx, &pipeline, &metrics, &in_flight)));
        }
        drop(out_tx);

        let ingest_result = ingest(&files, &mut store, &batch_tx, &metrics, &in_flight, batch_events, &mut input_problems);
        drop(batch_tx);
        for w in workers {
            w.join().expect("worker thread panicked");
        }
        let writer_result = writer.join().expect("output thread panicked");
        ingest_result.and(writer_result)
    });
    output_result?;

    store.flush(true)?;
    let elapsed = start.elapsed().as_secs_f64();
    let snapshot = metrics.snapshot(elapsed, threads, queue_cap);
    store.record_run(started_nanos, now_nanos(), &serde_json::to_string(&snapshot)?)?;
    Ok(Report { snapshot, load_problems, input_problems, parsers_loaded })
}

fn ingest(
    files: &[PathBuf],
    store: &mut RawStore,
    tx: &SyncSender<Batch>,
    metrics: &Metrics,
    in_flight: &AtomicI64,
    batch_events: usize,
    problems: &mut Vec<String>,
) -> Result<()> {
    let mut seq = 0u64;
    for path in files {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                metrics.files_failed.fetch_add(1, Relaxed);
                problems.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        // SAFETY: inputs are treated as read-only; a concurrent writer would only affect
        // bytes we hash and copy immediately, never memory safety of this process.
        let mmap = if len == 0 {
            None
        } else {
            match unsafe { Mmap::map(&file) } {
                Ok(m) => Some(m),
                Err(e) => {
                    metrics.files_failed.fetch_add(1, Relaxed);
                    problems.push(format!("{}: mmap failed: {e}", path.display()));
                    continue;
                }
            }
        };
        metrics.files.fetch_add(1, Relaxed);
        metrics.bytes.fetch_add(len, Relaxed);
        let source = store.source_id(&name)?;
        let ingest_started = now_nanos();
        let ctx = Arc::new(FileCtx { mmap, name });
        let mut count = 0u64;
        let mut first_id = None;
        let mut ranges = Vec::with_capacity(batch_events);
        let mut batch_first = 0u64;
        let mut receipt = now_nanos();
        for range in Framer::new(ctx.bytes(), true) {
            let id = store.append(source, receipt, &ctx.bytes()[range.clone()]).context("raw store append failed; aborting to avoid an incomplete store")?;
            if first_id.is_none() {
                first_id = Some(id);
            }
            if ranges.is_empty() {
                batch_first = id.0;
            }
            ranges.push(range);
            count += 1;
            if ranges.len() == batch_events {
                send_batch(tx, metrics, in_flight, &mut seq, &ctx, receipt, batch_first, std::mem::replace(&mut ranges, Vec::with_capacity(batch_events)));
                receipt = now_nanos();
            }
        }
        if !ranges.is_empty() {
            send_batch(tx, metrics, in_flight, &mut seq, &ctx, receipt, batch_first, ranges);
        }
        metrics.framed.fetch_add(count, Relaxed);
        metrics.stored.fetch_add(count, Relaxed);
        store.flush(false)?;
        store.record_ingest(source, first_id, count, len, ingest_started)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn send_batch(tx: &SyncSender<Batch>, metrics: &Metrics, in_flight: &AtomicI64, seq: &mut u64, ctx: &Arc<FileCtx>, receipt: i64, first_raw_id: u64, ranges: Vec<std::ops::Range<usize>>) {
    metrics.batches.fetch_add(1, Relaxed);
    let batch = Batch { seq: *seq, file: Arc::clone(ctx), receipt_nanos: receipt, first_raw_id, ranges };
    *seq += 1;
    // A closed receiver means every worker died; the join below surfaces that panic.
    // Blocking here is the backpressure policy.
    let _ = tx.send(batch);
    // Depth counts batches sitting in the channel; a worker may already have taken this
    // one, so the counter can transiently dip below zero and is clamped for the high-water.
    let depth = in_flight.fetch_add(1, Relaxed) + 1;
    metrics.queue_high_water.fetch_max(depth.max(0) as u64, Relaxed);
}

fn worker_thread(rx: Arc<Mutex<Receiver<Batch>>>, tx: SyncSender<(u64, Vec<u8>, u64)>, pipeline: &Pipeline, metrics: &Metrics, in_flight: &AtomicI64) {
    let mut scratch = pipeline.registry.scratch();
    loop {
        let batch = {
            let guard = rx.lock().expect("batch receiver poisoned");
            guard.recv()
        };
        let Ok(batch) = batch else { break };
        in_flight.fetch_sub(1, Relaxed);
        let bytes = batch.file.bytes();
        let mut out = Vec::with_capacity(batch.ranges.len() * 512);
        let mut counts = LocalCounts::default();
        let mut hint = None;
        let mut parsed = Parsed::default();
        for (i, range) in batch.ranges.iter().enumerate() {
            let outcome = pipeline.process(&bytes[range.clone()], batch.first_raw_id + i as u64, &batch.file.name, batch.receipt_nanos, &mut hint, &mut scratch, &mut parsed, &mut out);
            match outcome.parser {
                Some(_) => counts.detected += 1,
                None => counts.no_parser += 1,
            }
            if outcome.parser.is_some() {
                match outcome.parse {
                    Ok(()) => counts.parsed += 1,
                    Err(f) => counts.parse_failed(f),
                }
            }
            match outcome.sub {
                SubStatus::Matched => counts.sub_matched += 1,
                SubStatus::NoMatch => counts.sub_no_match += 1,
                SubStatus::Uncovered => counts.sub_uncovered += 1,
                SubStatus::NotApplicable => {}
            }
            if let Some(r) = outcome.time_error {
                counts.time_error(r);
            }
            let s = outcome.stats;
            counts.normalized += 1;
            counts.time_from_receipt += s.time_from_receipt as u64;
            counts.class_unknown += (s.class_uid == 0) as u64;
            counts.enum_other += s.enum_other as u64;
            counts.unmapped_fields += s.unmapped as u64;
            counts.utf8_lossy += s.utf8_lossy as u64;
        }
        metrics.add(&counts);
        if tx.send((batch.seq, out, batch.ranges.len() as u64)).is_err() {
            break;
        }
    }
}

fn output_thread(rx: Receiver<(u64, Vec<u8>, u64)>, output: &Path, metrics: &Metrics) -> Result<()> {
    let stdout;
    let file;
    let mut w: Box<dyn Write> = if output.as_os_str() == "-" {
        stdout = std::io::stdout();
        Box::new(BufWriter::with_capacity(1 << 20, stdout.lock()))
    } else {
        file = File::create(output).with_context(|| format!("creating output {}", output.display()))?;
        Box::new(BufWriter::with_capacity(1 << 20, file))
    };
    let mut pending: BTreeMap<u64, (Vec<u8>, u64)> = BTreeMap::new();
    let mut next = 0u64;
    while let Ok((seq, buf, count)) = rx.recv() {
        pending.insert(seq, (buf, count));
        while let Some((buf, count)) = pending.remove(&next) {
            w.write_all(&buf).context("writing output")?;
            metrics.emitted.fetch_add(count, Relaxed);
            metrics.output_bytes.fetch_add(buf.len() as u64, Relaxed);
            next += 1;
        }
    }
    w.flush().context("flushing output")?;
    Ok(())
}
