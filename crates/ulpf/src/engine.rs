//! The threaded pipeline and the one shared object behind it.
//!
//! `Live` holds everything a running engine knows: counters, the parser pipeline behind a
//! swap (read once per batch, so a reload never touches the per-event path), the raw
//! store behind one lock taken once per batch, per-source counts, the bounded tail of
//! emitted lines, the inference buffers and the pending directory. `run` drives it over a
//! fixed file list and returns; `serve` drives it from a polling tailer until told to
//! stop. The HTTP server is a reader of `Live` and owns nothing.
//!
//! One ingest thread memory-maps each input, frames it, appends every event to the raw
//! store (before anything else), and sends batches of byte ranges into a bounded
//! `sync_channel`. When the queue is full the ingest thread blocks: that is the entire
//! saturation policy, because dropping would break raw completeness. N workers detect,
//! parse, normalize and serialize each batch into one buffer. The output thread reorders
//! buffers by batch sequence so the JSON Lines order equals raw id order.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, anyhow};
use memmap2::Mmap;
use serde::Serialize;
use sha2::Digest;
use ulpf_parse::{Parsed, SubStatus};
use ulpf_store::{Framer, RawId, RawStore};

use crate::inference::Inference;
use crate::metrics::{LocalCounts, Metrics, Snapshot};
use crate::pending::{Pending, PendingSummary, ReviewError};
use crate::pipeline::Pipeline;
use crate::tail::Tail;

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
    /// Where proposals go. `None` turns inference off entirely.
    pub pending: Option<PathBuf>,
    /// Unknown lines a source needs before its first proposal; 0 turns inference off.
    pub infer_threshold: usize,
    /// Emitted lines kept for the server's tail.
    pub tail_capacity: usize,
}

#[derive(Debug)]
pub struct Report {
    pub snapshot: Snapshot,
    pub load_problems: Vec<String>,
    pub input_problems: Vec<String>,
    pub parsers_loaded: usize,
    pub pending: Vec<PendingSummary>,
    pub inference_secs: f64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SourceStats {
    pub events: u64,
    pub detected: u64,
    pub no_parser: u64,
    pub last_seen_nanos: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReloadReport {
    pub parsers_loaded: usize,
    pub problems: Vec<String>,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproveReport {
    pub name: String,
    pub path: PathBuf,
    pub parsers_loaded: usize,
    pub problems: Vec<String>,
    pub now_detected: NowDetected,
}

#[derive(Debug, Clone, Serialize)]
pub struct NowDetected {
    pub tested: u64,
    pub detected: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Traceback {
    pub raw_id: u64,
    pub source: String,
    pub receipt: String,
    pub receipt_nanos: i64,
    pub bytes_len: u64,
    pub text: String,
    pub hex: String,
    pub stored_sha256: String,
    pub recomputed_sha256: String,
    pub digest_match: bool,
    pub emitted: Option<serde_json::Value>,
    pub now: NowParse,
}

#[derive(Debug, Clone, Serialize)]
pub struct NowParse {
    pub parser: Option<String>,
    pub parse_status: String,
    pub normalized: serde_json::Value,
}

#[derive(Debug)]
pub enum TracebackError {
    NotFound { store_len: u64 },
    Io(String),
}

pub struct Live {
    pub metrics: Metrics,
    pipeline: RwLock<Arc<Pipeline>>,
    pub store: Mutex<RawStore>,
    pub tail: Tail,
    pub sources: Mutex<BTreeMap<String, SourceStats>>,
    /// Events routed to each parser by name, this process.
    pub parser_hits: Mutex<HashMap<String, u64>>,
    pub inference: Inference,
    pub pending: Option<Pending>,
    pub parsers_dir: PathBuf,
    pub mappings_dir: PathBuf,
    pub schema: Option<String>,
    pub default_offset_secs: i32,
    pub output: PathBuf,
    pub store_dir: PathBuf,
    pub watch: Vec<PathBuf>,
    pub threads: usize,
    pub queue_cap: usize,
    pub batch_events: usize,
    pub started: Instant,
    pub started_nanos: i64,
    pub generation: AtomicU64,
    pub pending_generation: AtomicU64,
    pub review_errors: AtomicU64,
    pub sse_clients: AtomicU64,
    pub load_problems: Mutex<Vec<String>>,
    parsers_mtime: Mutex<Option<SystemTime>>,
    stop: AtomicBool,
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

type OutMsg = (u64, Vec<u8>, u64, u64);

pub fn now_nanos() -> i64 {
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

fn parsers_mtime(dir: &Path) -> Option<SystemTime> {
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "toml")).filter_map(|e| e.metadata().ok()?.modified().ok()).max()
}

impl Live {
    /// Loads parsers and mappings, opens the store (taking the writer lock) and the
    /// pending directory. `live_inference` runs clustering as lines arrive; off, the
    /// buffers are clustered once when the engine stops.
    pub fn open(cfg: &Config, live_inference: bool) -> Result<Arc<Live>> {
        let (pipeline, load_problems) = Pipeline::load(&cfg.parsers, &cfg.mappings, cfg.schema.as_deref(), cfg.default_offset_secs)?;
        let store = RawStore::open(&cfg.store).with_context(|| format!("opening store {}", cfg.store.display()))?;
        let pending = match &cfg.pending {
            Some(dir) if cfg.infer_threshold > 0 => Some(Pending::open(dir).with_context(|| format!("pending directory {}", dir.display()))?),
            _ => None,
        };
        let threshold = if pending.is_some() { cfg.infer_threshold } else { 0 };
        Ok(Arc::new(Live {
            metrics: Metrics::default(),
            pipeline: RwLock::new(Arc::new(pipeline)),
            store: Mutex::new(store),
            tail: Tail::new(cfg.tail_capacity.max(1)),
            sources: Mutex::new(BTreeMap::new()),
            parser_hits: Mutex::new(HashMap::new()),
            inference: Inference::new(ulpf_infer::Params::default(), threshold, 4096, Duration::from_secs(5), live_inference),
            pending,
            parsers_dir: cfg.parsers.clone(),
            mappings_dir: cfg.mappings.clone(),
            schema: cfg.schema.clone(),
            default_offset_secs: cfg.default_offset_secs,
            output: cfg.output.clone(),
            store_dir: cfg.store.clone(),
            watch: cfg.inputs.clone(),
            threads: cfg.threads.max(1),
            queue_cap: cfg.queue_batches.max(1),
            batch_events: cfg.batch_events.max(1),
            started: Instant::now(),
            started_nanos: now_nanos(),
            generation: AtomicU64::new(0),
            pending_generation: AtomicU64::new(0),
            review_errors: AtomicU64::new(0),
            sse_clients: AtomicU64::new(0),
            load_problems: Mutex::new(load_problems),
            parsers_mtime: Mutex::new(parsers_mtime(&cfg.parsers)),
            stop: AtomicBool::new(false),
        }))
    }

    pub fn pipeline(&self) -> Arc<Pipeline> {
        Arc::clone(&self.pipeline.read().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn parser_names(&self) -> Vec<String> {
        self.pipeline().registry.iter().map(|p| p.name().to_string()).collect()
    }

    /// Reloads the parsers directory into a fresh registry and swaps it in. Workers pick
    /// the new registry up at their next batch; the mapping is reloaded with it.
    pub fn reload_parsers(&self) -> ReloadReport {
        match Pipeline::load(&self.parsers_dir, &self.mappings_dir, self.schema.as_deref(), self.default_offset_secs) {
            Ok((pipeline, problems)) => {
                let loaded = pipeline.registry.len();
                *self.pipeline.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(pipeline);
                *self.load_problems.lock().unwrap_or_else(|e| e.into_inner()) = problems.clone();
                *self.parsers_mtime.lock().unwrap_or_else(|e| e.into_inner()) = parsers_mtime(&self.parsers_dir);
                self.metrics.reloads.fetch_add(1, Relaxed);
                let generation = self.generation.fetch_add(1, Relaxed) + 1;
                ReloadReport { parsers_loaded: loaded, problems, generation }
            }
            Err(e) => ReloadReport { parsers_loaded: self.pipeline().registry.len(), problems: vec![format!("reload failed, previous registry kept: {e:#}")], generation: self.generation.load(Relaxed) },
        }
    }

    /// True when a `*.toml` in the parsers directory changed since the last load.
    fn parsers_dir_changed(&self) -> bool {
        let now = parsers_mtime(&self.parsers_dir);
        let mut last = self.parsers_mtime.lock().unwrap_or_else(|e| e.into_inner());
        if now != *last {
            *last = now;
            return true;
        }
        false
    }

    pub fn snapshot(&self) -> Snapshot {
        self.metrics.snapshot(self.started.elapsed().as_secs_f64(), self.threads, self.queue_cap)
    }

    pub fn stop(&self) {
        self.stop.store(true, Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Relaxed)
    }

    fn pending(&self) -> Result<&Pending, ReviewError> {
        self.pending.as_ref().ok_or_else(|| ReviewError::Io("inference is disabled: no pending directory".into()))
    }

    /// Approval: the definition moves to the parsers directory, the registry reloads, and
    /// the source's buffered unknown lines are re-detected to prove the fast path.
    pub fn approve(&self, id: &str) -> Result<ApproveReport, ReviewError> {
        let pending = self.pending()?;
        let mut lines = pending.lines(id);
        let approved = pending.approve(id, &self.parsers_dir, &self.parser_names())?;
        if lines.is_empty() {
            lines = self.inference.lines(&approved.source);
        }
        let reload = self.reload_parsers();
        let pipeline = self.pipeline();
        let idx = pipeline.registry.index_of(&approved.name);
        let detected = lines.iter().filter(|l| idx.is_some() && pipeline.registry.detect(l, idx) == idx).count() as u64;
        self.inference.clear(&approved.source);
        self.metrics.approved.fetch_add(1, Relaxed);
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(ApproveReport { name: approved.name, path: approved.path, parsers_loaded: reload.parsers_loaded, problems: reload.problems, now_detected: NowDetected { tested: lines.len() as u64, detected } })
    }

    pub fn reject(&self, id: &str) -> Result<PathBuf, ReviewError> {
        let moved = self.pending()?.reject(id)?;
        self.metrics.rejected.fetch_add(1, Relaxed);
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(moved)
    }

    pub fn regenerate(&self, id: &str, keep: &[u64], merge: &[Vec<u64>]) -> Result<(String, Vec<String>), ReviewError> {
        let r = self.pending()?.regenerate(id, keep, merge, &self.inference.params)?;
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(r)
    }

    pub fn put_text(&self, id: &str, text: &str) -> Result<Vec<String>, ReviewError> {
        let r = self.pending()?.put_text(id, text)?;
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(r)
    }

    /// One raw record with its stored and recomputed digest, the line as emitted (if
    /// still in the tail) and the same bytes through the current parsers.
    pub fn traceback(&self, id: u64) -> Result<Traceback, TracebackError> {
        let (rec, names) = {
            let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let rec = store.get(RawId(id)).map_err(|e| TracebackError::Io(e.to_string()))?;
            match rec {
                Some(r) => (r, store.source_names().unwrap_or_default()),
                None => return Err(TracebackError::NotFound { store_len: store.len() }),
            }
        };
        let source = names.get(&rec.source).cloned().unwrap_or_else(|| format!("source#{}", rec.source));
        let recomputed: [u8; 32] = sha2::Sha256::digest(&rec.bytes).into();
        let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let pipeline = self.pipeline();
        let mut scratch = pipeline.registry.scratch();
        let mut parsed = Parsed::default();
        let mut out = Vec::new();
        let mut hint = None;
        let outcome = pipeline.process(&rec.bytes, id, &source, rec.receipt_nanos, &mut hint, &mut scratch, &mut parsed, &mut out);
        let normalized = serde_json::from_slice(&out).unwrap_or(serde_json::Value::Null);
        let parser = outcome.parser.map(|i| pipeline.registry.get(i).name().to_string());
        let parse_status = match (outcome.parser, outcome.parse) {
            (None, _) => "no_parser".to_string(),
            (Some(_), Ok(())) => "parsed".to_string(),
            (Some(_), Err(f)) => f.reason().to_string(),
        };
        let mut receipt = String::new();
        ulpf_time::format_rfc3339(rec.receipt_nanos, &mut receipt);
        Ok(Traceback {
            raw_id: id,
            source,
            receipt,
            receipt_nanos: rec.receipt_nanos,
            bytes_len: rec.bytes.len() as u64,
            text: escape_text(&rec.bytes),
            hex: hex(&rec.bytes),
            stored_sha256: hex(&rec.sha256),
            recomputed_sha256: hex(&recomputed),
            digest_match: recomputed == rec.sha256,
            emitted: self.tail.find(id).and_then(|l| serde_json::from_slice(&l).ok()),
            now: NowParse { parser, parse_status, normalized },
        })
    }
}

/// Lossy UTF-8 with control bytes other than tab and newline shown as `\xNN`.
fn escape_text(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len());
    for c in String::from_utf8_lossy(bytes).chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            s.push_str(&format!("\\x{:02x}", c as u32));
        } else {
            s.push(c);
        }
    }
    s
}

struct Threads<'scope> {
    batch_tx: SyncSender<Batch>,
    workers: Vec<std::thread::ScopedJoinHandle<'scope, ()>>,
    writer: std::thread::ScopedJoinHandle<'scope, Result<()>>,
    inference: std::thread::ScopedJoinHandle<'scope, ()>,
    in_flight: Arc<AtomicI64>,
    seq: u64,
}

fn start<'scope, 'env: 'scope>(scope: &'scope std::thread::Scope<'scope, 'env>, live: &'env Arc<Live>) -> Threads<'scope> {
    let (batch_tx, batch_rx) = sync_channel::<Batch>(live.queue_cap);
    let batch_rx = Arc::new(Mutex::new(batch_rx));
    let (out_tx, out_rx) = sync_channel::<OutMsg>(live.queue_cap * 2);
    let in_flight = Arc::new(AtomicI64::new(0));
    let writer = scope.spawn(move || output_thread(live, out_rx));
    let inference = scope.spawn(move || {
        if let Some(p) = &live.pending {
            live.inference.run_thread(p, &live.metrics, &live.pending_generation);
        }
    });
    let mut workers = Vec::new();
    for _ in 0..live.threads {
        let rx = Arc::clone(&batch_rx);
        let tx = out_tx.clone();
        let in_flight = Arc::clone(&in_flight);
        workers.push(scope.spawn(move || worker_thread(live, rx, tx, &in_flight)));
    }
    // Only the workers hold the receiver now: when the output stage fails they exit, the
    // channel disconnects, and ingest's next send fails instead of blocking forever.
    drop(out_tx);
    drop(batch_rx);
    Threads { batch_tx, workers, writer, inference, in_flight, seq: 0 }
}

/// Joins everything after ingest is done. The throughput clock stops when the output
/// thread finishes; inference's final pass runs after that and is timed separately.
fn finish(live: &Arc<Live>, t: Threads<'_>, ingest_result: Result<()>) -> Result<(Duration, Duration)> {
    drop(t.batch_tx);
    for w in t.workers {
        w.join().map_err(|_| anyhow!("worker thread panicked"))?;
    }
    let writer_result = t.writer.join().map_err(|_| anyhow!("output thread panicked"))?;
    let elapsed = live.started.elapsed();
    let infer_started = Instant::now();
    live.inference.stop();
    t.inference.join().map_err(|_| anyhow!("inference thread panicked"))?;
    let inference_secs = infer_started.elapsed();
    match (writer_result, ingest_result) {
        (Err(w), _) => Err(w),
        (Ok(()), Err(i)) => Err(i),
        (Ok(()), Ok(())) => Ok((elapsed, inference_secs)),
    }
}

fn report(live: &Arc<Live>, elapsed: Duration, inference: Duration, input_problems: Vec<String>) -> Result<Report> {
    let snapshot = live.metrics.snapshot(elapsed.as_secs_f64(), live.threads, live.queue_cap);
    {
        let mut store = live.store.lock().unwrap_or_else(|e| e.into_inner());
        store.flush(true)?;
        store.record_run(live.started_nanos, now_nanos(), &serde_json::to_string(&snapshot)?)?;
    }
    Ok(Report {
        snapshot,
        load_problems: live.load_problems.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        input_problems,
        parsers_loaded: live.pipeline().registry.len(),
        pending: live.pending.as_ref().map(Pending::list).unwrap_or_default(),
        inference_secs: inference.as_secs_f64(),
    })
}

/// Batch mode: every input once, then the counter block.
pub fn run(cfg: &Config) -> Result<Report> {
    let live = Live::open(cfg, false)?;
    let files = collect_inputs(&cfg.inputs)?;
    let mut input_problems = Vec::new();
    let timing = std::thread::scope(|scope| {
        let mut t = start(scope, &live);
        let ingest_result = (|| {
            for path in &files {
                ingest_file(&live, path, 0, true, true, &t.batch_tx, &t.in_flight, &mut t.seq, &mut input_problems)?;
            }
            Ok(())
        })();
        finish(&live, t, ingest_result)
    });
    // Whatever happened downstream, every appended record reaches disk before we report.
    live.store.lock().unwrap_or_else(|e| e.into_inner()).flush(true)?;
    let (elapsed, inference) = timing?;
    report(&live, elapsed, inference, input_problems)
}

struct Tailed {
    consumed: u64,
    last_size: u64,
    stable_ticks: u32,
    growing_ticks: u32,
}

/// Watch mode: polls the input directories until `live.stop()`. A file that stopped
/// growing for two ticks is ingested to its end; a file still growing after four ticks
/// is ingested up to its last complete line. Offsets resume from the catalogue so a
/// restart does not re-ingest what it already stored.
pub fn serve(live: &Arc<Live>, poll: Duration) -> Result<Report> {
    let mut input_problems = Vec::new();
    let timing = std::thread::scope(|scope| {
        let mut t = start(scope, live);
        let ingest_result = poll_loop(live, poll, &t.batch_tx, &t.in_flight, &mut t.seq, &mut input_problems);
        finish(live, t, ingest_result)
    });
    live.store.lock().unwrap_or_else(|e| e.into_inner()).flush(true)?;
    let (elapsed, inference) = timing?;
    report(live, elapsed, inference, input_problems)
}

#[allow(clippy::too_many_arguments)]
fn poll_loop(live: &Arc<Live>, poll: Duration, tx: &SyncSender<Batch>, in_flight: &AtomicI64, seq: &mut u64, problems: &mut Vec<String>) -> Result<()> {
    let resume = live.store.lock().unwrap_or_else(|e| e.into_inner()).ingested_bytes().unwrap_or_default();
    let mut files: HashMap<PathBuf, Tailed> = HashMap::new();
    while !live.stopped() {
        let paths = collect_inputs(&live.watch).unwrap_or_default();
        for path in paths {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let size = meta.len();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let entry = files.entry(path.clone()).or_insert_with(|| {
                let consumed = resume.get(&name).copied().unwrap_or(0);
                live.metrics.files.fetch_add(1, Relaxed);
                Tailed { consumed, last_size: size, stable_ticks: 0, growing_ticks: 0 }
            });
            if size < entry.consumed {
                // truncated or replaced: start over, and say so once
                problems.push(format!("{}: shrank below the ingested offset, re-reading from the start", path.display()));
                entry.consumed = 0;
            }
            if size == entry.consumed {
                entry.stable_ticks = 0;
                entry.growing_ticks = 0;
                entry.last_size = size;
                continue;
            }
            if size == entry.last_size {
                entry.stable_ticks += 1;
            } else {
                entry.stable_ticks = 0;
                entry.growing_ticks += 1;
                entry.last_size = size;
            }
            let finalize = entry.stable_ticks >= 2;
            let stream = entry.growing_ticks >= 4;
            if finalize || stream {
                let before = entry.consumed;
                match ingest_file(live, &path, entry.consumed, finalize, false, tx, in_flight, seq, problems) {
                    Ok(consumed) => {
                        entry.consumed = consumed;
                        live.metrics.bytes.fetch_add(consumed.saturating_sub(before), Relaxed);
                        if finalize {
                            entry.growing_ticks = 0;
                            entry.stable_ticks = 0;
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        if live.parsers_dir_changed() {
            let r = live.reload_parsers();
            for p in &r.problems {
                eprintln!("ulpf: reload: {p}");
            }
        }
        std::thread::sleep(poll);
    }
    Ok(())
}

/// Frames and stores `path` from byte `start`. With `eof` the whole remainder is
/// consumed; without it the last line is withheld until more bytes arrive. Returns the
/// new consumed offset. Batch mode passes `count_file` and the whole file is counted
/// here; the tailer counts a file when it first sees it and bytes as it consumes them.
#[allow(clippy::too_many_arguments)]
fn ingest_file(live: &Arc<Live>, path: &Path, start: u64, eof: bool, count_file: bool, tx: &SyncSender<Batch>, in_flight: &AtomicI64, seq: &mut u64, problems: &mut Vec<String>) -> Result<u64> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            live.metrics.files_failed.fetch_add(1, Relaxed);
            problems.push(format!("{}: {e}", path.display()));
            return Ok(start);
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
                live.metrics.files_failed.fetch_add(1, Relaxed);
                problems.push(format!("{}: mmap failed: {e}", path.display()));
                return Ok(start);
            }
        }
    };
    if count_file {
        live.metrics.files.fetch_add(1, Relaxed);
        live.metrics.bytes.fetch_add(len, Relaxed);
    }
    let start = (start as usize).min(len as usize);
    let source = live.store.lock().unwrap_or_else(|e| e.into_inner()).source_id(&name)?;
    let ingest_started = now_nanos();
    let ctx = Arc::new(FileCtx { mmap, name });
    let mut count = 0u64;
    let mut first_id = None;
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(live.batch_events);
    let mut receipt = now_nanos();
    let mut consumed = start;
    let bytes = ctx.bytes();
    let mut framer = Framer::new(&bytes[start..], eof);
    loop {
        let next = framer.next();
        let done = next.is_none();
        match next {
            Some(r) => {
                ranges.push(r.start + start..r.end + start);
                consumed = r.end + start;
                if ranges.len() < live.batch_events {
                    continue;
                }
            }
            None => {
                if ranges.is_empty() {
                    break;
                }
            }
        }
        // One store lock per batch: ids are issued, bytes reach the OS, then the batch
        // escapes, so a crash can never reissue an id that was already emitted.
        let batch_first = {
            let mut store = live.store.lock().unwrap_or_else(|e| e.into_inner());
            let mut first = None;
            for r in &ranges {
                let id = store.append(source, receipt, &bytes[r.clone()]).context("raw store append failed; aborting to avoid an incomplete store")?;
                first.get_or_insert(id);
            }
            store.flush(false).context("raw store flush failed")?;
            first.expect("non-empty batch").0
        };
        first_id.get_or_insert(RawId(batch_first));
        count += ranges.len() as u64;
        let ranges = std::mem::replace(&mut ranges, Vec::with_capacity(live.batch_events));
        send_batch(tx, &live.metrics, in_flight, live.queue_cap, seq, &ctx, receipt, batch_first, ranges)?;
        receipt = now_nanos();
        if done {
            break;
        }
    }
    live.metrics.framed.fetch_add(count, Relaxed);
    live.metrics.stored.fetch_add(count, Relaxed);
    if count > 0 {
        let mut store = live.store.lock().unwrap_or_else(|e| e.into_inner());
        store.flush(false)?;
        store.record_ingest(source, first_id, count, (consumed - start) as u64, ingest_started)?;
    }
    Ok(consumed as u64)
}

#[allow(clippy::too_many_arguments)]
fn send_batch(tx: &SyncSender<Batch>, metrics: &Metrics, in_flight: &AtomicI64, queue_cap: usize, seq: &mut u64, ctx: &Arc<FileCtx>, receipt: i64, first_raw_id: u64, ranges: Vec<std::ops::Range<usize>>) -> Result<()> {
    metrics.batches.fetch_add(1, Relaxed);
    let batch = Batch { seq: *seq, file: Arc::clone(ctx), receipt_nanos: receipt, first_raw_id, ranges };
    *seq += 1;
    // Depth counts batches handed to the channel and not yet taken by a worker. Counted
    // before the send and clamped to the capacity, so the high-water can never claim a
    // depth the channel cannot hold; the worker decrements after its receive.
    let depth = in_flight.fetch_add(1, Relaxed) + 1;
    metrics.queue_high_water.fetch_max(depth.clamp(0, queue_cap as i64) as u64, Relaxed);
    // Backpressure is measured, not inferred: a full queue is counted, then we block.
    let batch = match tx.try_send(batch) {
        Ok(()) => return Ok(()),
        Err(TrySendError::Full(b)) => b,
        Err(TrySendError::Disconnected(_)) => return Err(anyhow!("processing stopped before ingest finished; see the output error")),
    };
    metrics.backpressure_blocks.fetch_add(1, Relaxed);
    tx.send(batch).map_err(|_| anyhow!("processing stopped before ingest finished; see the output error"))
}

fn worker_thread(live: &Live, rx: Arc<Mutex<Receiver<Batch>>>, tx: SyncSender<OutMsg>, in_flight: &AtomicI64) {
    let mut pipeline = live.pipeline();
    let mut scratch = pipeline.registry.scratch();
    let mut hint = None;
    let mut hits: Vec<u64> = Vec::new();
    loop {
        let batch = {
            let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
            guard.recv()
        };
        let Ok(batch) = batch else { break };
        in_flight.fetch_sub(1, Relaxed);
        // A reload swaps the pipeline between batches, never inside one; the hint is an
        // index into the old registry and is dropped with it.
        let current = live.pipeline();
        if !Arc::ptr_eq(&current, &pipeline) {
            pipeline = current;
            hint = None;
        }
        hits.clear();
        hits.resize(pipeline.registry.len(), 0);
        let bytes = batch.file.bytes();
        let mut out = Vec::with_capacity(batch.ranges.len() * 512);
        let mut counts = LocalCounts::default();
        // `Parsed` borrows the batch's bytes and the current registry, so it lives per batch
        let mut parsed = Parsed::default();
        let pipeline = &*pipeline;
        for (i, range) in batch.ranges.iter().enumerate() {
            let event = &bytes[range.clone()];
            let outcome = pipeline.process(event, batch.first_raw_id + i as u64, &batch.file.name, batch.receipt_nanos, &mut hint, &mut scratch, &mut parsed, &mut out);
            match outcome.parser {
                Some(p) => {
                    counts.detected += 1;
                    hits[p] += 1;
                }
                None => {
                    counts.no_parser += 1;
                    live.inference.offer(&batch.file.name, batch.first_raw_id + i as u64, event, &live.metrics);
                }
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
        live.metrics.add(&counts);
        {
            let mut sources = live.sources.lock().unwrap_or_else(|e| e.into_inner());
            let s = sources.entry(batch.file.name.clone()).or_default();
            s.events += batch.ranges.len() as u64;
            s.detected += counts.detected;
            s.no_parser += counts.no_parser;
            s.last_seen_nanos = batch.receipt_nanos;
        }
        if hits.iter().any(|h| *h > 0) {
            let mut ph = live.parser_hits.lock().unwrap_or_else(|e| e.into_inner());
            for (i, h) in hits.iter().enumerate() {
                if *h > 0 {
                    *ph.entry(pipeline.registry.get(i).name().to_string()).or_default() += h;
                }
            }
        }
        if tx.send((batch.seq, out, batch.ranges.len() as u64, batch.first_raw_id)).is_err() {
            break;
        }
    }
}

fn output_thread(live: &Live, rx: Receiver<OutMsg>) -> Result<()> {
    let stdout;
    let file;
    let mut w: Box<dyn Write> = if live.output.as_os_str() == "-" {
        stdout = std::io::stdout();
        Box::new(BufWriter::with_capacity(1 << 20, stdout.lock()))
    } else {
        file = File::options().create(true).append(true).open(&live.output).with_context(|| format!("creating output {}", live.output.display()))?;
        Box::new(BufWriter::with_capacity(1 << 20, file))
    };
    let mut pending: BTreeMap<u64, (Vec<u8>, u64, u64)> = BTreeMap::new();
    let mut next = 0u64;
    let mut since_flush = Instant::now();
    while let Ok((seq, buf, count, first_raw_id)) = rx.recv() {
        pending.insert(seq, (buf, count, first_raw_id));
        while let Some((buf, count, first_raw_id)) = pending.remove(&next) {
            w.write_all(&buf).context("writing output")?;
            live.metrics.emitted.fetch_add(count, Relaxed);
            live.metrics.output_bytes.fetch_add(buf.len() as u64, Relaxed);
            // the buffer moves into the tail; the ring keeps ranges into it, no copy
            live.tail.push_batch(first_raw_id, buf);
            next += 1;
        }
        // in watch mode batches trickle in; a reader of the output file should not wait
        // for a megabyte to accumulate
        if since_flush.elapsed() > Duration::from_millis(500) {
            w.flush().context("flushing output")?;
            since_flush = Instant::now();
        }
    }
    w.flush().context("flushing output")?;
    Ok(())
}
