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

use std::borrow::Cow;
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
use ulpf_normalize::EntityKind;
use ulpf_parse::{Parsed, SubStatus};
use ulpf_store::{Attestation, Framer, RawId, RawReader, RawStore, VerifyReason};

use crate::inference::Inference;
use crate::metrics::{LocalCounts, Metrics, Snapshot};
use crate::pending::{Pending, PendingSummary, ReviewError};
use crate::pipeline::Pipeline;
use crate::pivot::{PivotCounters, PivotIndex, PivotPage, PivotQuery, PivotWriter, Posting};
use crate::replay::{self, ReplayProgress, ReplayReport, Versions};
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
    pub parse_failed: u64,
    pub last_seen_nanos: i64,
    /// Events routed to each parser, so the established parser is the one with most.
    pub by_parser: BTreeMap<String, u64>,
    pub window_events: u64,
    pub window_misses: u64,
    /// Completed windows while not tripped: the long-run miss rate the window is judged against.
    pub baseline_events: u64,
    pub baseline_misses: u64,
    pub window_rate: f64,
    pub drift: DriftState,
    pub drift_since_nanos: i64,
    pub drift_lines_routed: u64,
    pub drift_clean_windows: u32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DriftState {
    #[default]
    None,
    Watching,
    Tripped,
    Proposed,
    Cleared,
}

/// Drift thresholds (D54): a window of events per source, judged once it fills.
pub const DRIFT_WINDOW: u64 = 512;
pub const DRIFT_ESTABLISHED: u64 = 1024;
pub const DRIFT_MIN_MISSES: u64 = 32;
pub const DRIFT_DELTA: f64 = 0.25;
pub const DRIFT_BASELINE_MAX: f64 = 0.2;

impl SourceStats {
    pub fn established_parser(&self) -> Option<&str> {
        self.by_parser.iter().max_by_key(|(_, n)| **n).map(|(k, _)| k.as_str())
    }

    pub fn baseline_rate(&self) -> f64 {
        if self.baseline_events == 0 { 0.0 } else { self.baseline_misses as f64 / self.baseline_events as f64 }
    }

    /// Folds one batch into the window; returns the established parser's name when this
    /// batch completed a window that trips.
    fn observe(&mut self, events: u64, misses: u64) -> Option<String> {
        self.window_events += events;
        self.window_misses += misses;
        if self.window_events < DRIFT_WINDOW {
            return None;
        }
        let rate = self.window_misses as f64 / self.window_events as f64;
        self.window_rate = rate;
        let baseline = self.baseline_rate();
        let established = self.baseline_events >= DRIFT_ESTABLISHED && baseline < DRIFT_BASELINE_MAX && self.established_parser().is_some();
        let mut tripped = None;
        match self.drift {
            DriftState::None | DriftState::Watching | DriftState::Cleared => {
                if established && self.window_misses >= DRIFT_MIN_MISSES && rate >= baseline + DRIFT_DELTA {
                    self.drift = DriftState::Tripped;
                    self.drift_since_nanos = now_nanos();
                    self.drift_clean_windows = 0;
                    tripped = self.established_parser().map(str::to_string);
                } else {
                    if established {
                        self.drift = DriftState::Watching;
                    }
                    self.baseline_events += self.window_events;
                    self.baseline_misses += self.window_misses;
                }
            }
            DriftState::Tripped | DriftState::Proposed => {
                if rate < baseline + DRIFT_DELTA {
                    self.drift_clean_windows += 1;
                } else {
                    self.drift_clean_windows = 0;
                }
            }
        }
        self.window_events = 0;
        self.window_misses = 0;
        tripped
    }

    fn routing(&self) -> bool {
        matches!(self.drift, DriftState::Tripped | DriftState::Proposed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DriftAlert {
    pub source: String,
    pub parser: String,
    pub state: DriftState,
    pub since: String,
    pub window: DriftWindow,
    pub baseline_rate: f64,
    pub lines_routed: u64,
    pub pending_id: Option<String>,
    pub proposed_version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriftWindow {
    pub events: u64,
    pub misses: u64,
    pub rate: f64,
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
    pub replaced_version: Option<u64>,
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
    pub chain: String,
    pub prev_chain: String,
    pub chain_match: bool,
    pub emitted: Option<serde_json::Value>,
    pub now: NowParse,
}

#[derive(Debug, Clone, Serialize)]
pub struct NowParse {
    pub parser: Option<String>,
    pub parse_status: String,
    pub normalized: serde_json::Value,
    /// The parser's own fields with their byte ranges in the raw record (D15: borrowed spans).
    pub fields: Vec<TraceField>,
    /// Schema path -> the source field that fed it.
    pub provenance: Vec<TraceProvenance>,
    pub time: TraceTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceField {
    pub key: String,
    pub value: String,
    pub span: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceProvenance {
    pub path: String,
    pub source_key: String,
    pub span: Option<(u64, u64)>,
    pub canonical: bool,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceTime {
    pub text_span: Option<(u64, u64)>,
    pub policies: Vec<String>,
}

/// Half-open byte range of `v` inside `base` when `v` borrows from it (a zero-copy span);
/// `None` for a materialised or constant value. The `Cow` variant is the information here.
#[allow(clippy::ptr_arg)]
fn span_in(base: &[u8], v: &Cow<'_, [u8]>) -> Option<(u64, u64)> {
    let Cow::Borrowed(b) = v else { return None };
    let (s, bs) = (b.as_ptr() as usize, base.as_ptr() as usize);
    (s >= bs && s + b.len() <= bs + base.len()).then(|| ((s - bs) as u64, (s - bs + b.len()) as u64))
}

#[derive(Default)]
pub struct IntegrityState {
    pub running: bool,
    pub last: Option<LastVerify>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LastVerify {
    pub at: String,
    pub records: u64,
    pub ok: bool,
    pub corrupt: u64,
    pub first_bad: Option<u64>,
    pub reason: Option<&'static str>,
    pub elapsed_secs: f64,
    pub against_attestation: bool,
}

#[derive(Debug)]
pub enum IntegrityError {
    Running,
    Io(String),
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
    pub replay: Mutex<ReplayState>,
    /// Bumped whenever the replay state changes (start, progress, done), for the SSE feed.
    pub replay_generation: AtomicU64,
    /// Bumped whenever a source's drift state changes, for the SSE feed.
    pub drift_generation: AtomicU64,
    pub integrity: Mutex<IntegrityState>,
    pub integrity_generation: AtomicU64,
    /// The output thread's index writer counters, once it has opened the index.
    pub pivot_counters: Mutex<Option<Arc<PivotCounters>>>,
    pivot_index: Mutex<Option<PivotIndex>>,
    parsers_signature: Mutex<Option<(usize, Option<SystemTime>, u64)>>,
    stop: AtomicBool,
}

/// What the server can say about replays: the one running (if any), the last report,
/// and the sparse indexes of diff files it has opened.
#[derive(Default)]
pub struct ReplayState {
    pub running: Option<(ReplayProgress, Arc<AtomicU64>)>,
    pub last: Option<ReplayReport>,
    pub last_error: Option<String>,
    pub diff_indexes: HashMap<u64, Arc<replay::DiffIndex>>,
}

#[derive(Debug)]
pub enum ReplayError {
    Running,
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::Running => write!(f, "a replay is already running"),
            ReplayError::Invalid(s) | ReplayError::Io(s) => write!(f, "{s}"),
        }
    }
}

/// Where a batch's bytes live: a memory-mapped input file, or the raw store's segment
/// when a replay reads records back.
pub(crate) enum Backing {
    Mapped(Option<Mmap>),
    Store(RawReader),
}

pub(crate) struct FileCtx {
    pub(crate) backing: Backing,
    pub(crate) name: String,
    /// Source id -> name, for batches whose events come from several sources (replay).
    pub(crate) names: HashMap<u32, String>,
}

impl FileCtx {
    fn bytes(&self) -> &[u8] {
        match &self.backing {
            Backing::Mapped(m) => m.as_deref().unwrap_or(&[]),
            Backing::Store(r) => r.segment(),
        }
    }
}

pub(crate) struct Batch {
    pub(crate) seq: u64,
    pub(crate) file: Arc<FileCtx>,
    pub(crate) receipt_nanos: i64,
    pub(crate) first_raw_id: u64,
    pub(crate) ranges: Vec<std::ops::Range<usize>>,
    /// Per-event receipt and source when they vary within the batch (replay); empty
    /// means every event took the batch's receipt and the file's name (live ingest).
    pub(crate) receipts: Vec<i64>,
    pub(crate) sources: Vec<u32>,
}

impl Batch {
    fn receipt(&self, i: usize) -> i64 {
        self.receipts.get(i).copied().unwrap_or(self.receipt_nanos)
    }

    fn source(&self, i: usize) -> &str {
        match self.sources.get(i) {
            Some(s) => self.file.names.get(s).map(String::as_str).unwrap_or(&self.file.name),
            None => &self.file.name,
        }
    }
}

/// One worker's serialised batch on its way to the output thread.
pub(crate) struct Emitted {
    pub(crate) seq: u64,
    pub(crate) buf: Vec<u8>,
    pub(crate) count: u64,
    pub(crate) first_raw_id: u64,
    pub(crate) entities: EntityBatch,
}

/// The entity values of one batch, copied out of the event bytes so the output thread can
/// index them after the batch's mapping is gone: one arena and one fixed record per event.
#[derive(Default)]
pub(crate) struct EntityBatch {
    pub(crate) arena: Vec<u8>,
    pub(crate) events: Vec<EventEntities>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EventEntities {
    pub(crate) raw_id: u64,
    pub(crate) time_ms: i64,
    /// The emitted line's range inside the batch buffer (without its newline).
    pub(crate) line: (u32, u32),
    /// Arena ranges; an empty range is absent.
    pub(crate) parser: (u32, u32),
    pub(crate) device: (u32, u32),
    pub(crate) values: [(u32, u32); 5],
}

impl EntityBatch {
    fn put(&mut self, bytes: &[u8]) -> (u32, u32) {
        let s = self.arena.len() as u32;
        self.arena.extend_from_slice(bytes);
        (s, self.arena.len() as u32)
    }

    fn slice(&self, r: (u32, u32)) -> &[u8] {
        &self.arena[r.0 as usize..r.1 as usize]
    }
}

/// Every event of one batch through the pipeline into `out`, with the counts, per-parser
/// hits and the unknown lines (for inference) collected for the caller. The live worker
/// and the replay worker both call this; neither has its own per-event path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_batch<'a>(pipeline: &'a Pipeline, batch: &'a Batch, scratch: &mut ulpf_parse::Scratch, hint: &mut Option<usize>, parsed: &mut Parsed<'a>, out: &mut Vec<u8>, counts: &mut LocalCounts, hits: &mut [u64], unknown: &mut Vec<(u64, &'a [u8])>, failed: &mut Vec<(u64, &'a [u8])>, mut entities: Option<&mut EntityBatch>) {
    let bytes = batch.file.bytes();
    for (i, range) in batch.ranges.iter().enumerate() {
        let event = &bytes[range.clone()];
        let raw_id = batch.first_raw_id + i as u64;
        let line_start = out.len();
        let outcome = pipeline.process(event, raw_id, batch.source(i), batch.receipt(i), hint, scratch, parsed, out);
        if let Some(eb) = entities.as_deref_mut() {
            // up to five small values per event, copied once; the index thread does the rest
            let mut e = EventEntities { raw_id, time_ms: outcome.stats.time_ms, line: (line_start as u32, out.len().saturating_sub(1) as u32), ..EventEntities::default() };
            if let Some(p) = outcome.parser {
                e.parser = eb.put(pipeline.registry.get(p).name().as_bytes());
            }
            for (k, slot) in outcome.stats.entities.iter().enumerate() {
                if let Some(idx) = slot
                    && let Some(f) = parsed.fields.get(*idx as usize)
                {
                    e.values[k] = eb.put(&f.value);
                }
            }
            let dev = e.values[EntityKind::Device as usize];
            e.device = if dev.0 == dev.1 { eb.put(batch.source(i).as_bytes()) } else { dev };
            eb.events.push(e);
        }
        match outcome.parser {
            Some(p) => {
                counts.detected += 1;
                if let Some(h) = hits.get_mut(p) {
                    *h += 1;
                }
            }
            None => {
                counts.no_parser += 1;
                unknown.push((raw_id, event));
            }
        }
        if let Some(p) = outcome.parser {
            match outcome.parse {
                Ok(()) => counts.parsed += 1,
                Err(f) => {
                    counts.parse_failed(f);
                    // a generated parser's signature is loose by construction (D45): a
                    // line it claims but cannot parse is still an unknown line for inference
                    if pipeline.registry.get(p).definition().matcher.priority < 0 {
                        unknown.push((raw_id, event));
                    } else {
                        failed.push((raw_id, event));
                    }
                }
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
}

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
        // symlinked directories are not followed: a loop would recurse forever
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            walk(&path, out)?;
        } else if path.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.')) {
            out.push(path);
        }
    }
    Ok(())
}

/// A source is a file named by its path relative to the input root it was found under
/// (`fw/syslog.log`), or its basename when the file itself was the input. Two roots with
/// a `syslog.log` each therefore stay two sources, with two resume offsets.
pub fn source_name(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string()),
    }
}

/// Count, newest modification and total size of the `*.toml` files: a delete or a
/// `cp -p` with an older timestamp changes it too.
fn parsers_signature(dir: &Path) -> Option<(usize, Option<SystemTime>, u64)> {
    let metas: Vec<std::fs::Metadata> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "toml")).filter_map(|e| e.metadata().ok()).collect();
    Some((metas.len(), metas.iter().filter_map(|m| m.modified().ok()).max(), metas.iter().map(std::fs::Metadata::len).sum()))
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
        if cfg.output.as_os_str() != "-" {
            Versions::new(&cfg.output).write_live_meta(pipeline.mapping.schema_name(), 0, pipeline.files.clone()).with_context(|| format!("writing the version meta beside {}", cfg.output.display()))?;
        }
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
            replay: Mutex::new(ReplayState::default()),
            replay_generation: AtomicU64::new(0),
            drift_generation: AtomicU64::new(0),
            integrity: Mutex::new(IntegrityState::default()),
            integrity_generation: AtomicU64::new(0),
            pivot_counters: Mutex::new(None),
            pivot_index: Mutex::new(None),
            parsers_signature: Mutex::new(parsers_signature(&cfg.parsers)),
            stop: AtomicBool::new(false),
        }))
    }

    /// Starts a replay of every stored record on its own thread. The store is flushed and
    /// snapshotted (ids below its length now) through the writer's files; the pipeline is
    /// the current one, or a fresh load when a different schema is asked for.
    pub fn start_replay(self: &Arc<Self>, schema: Option<&str>) -> Result<(u64, u64), ReplayError> {
        if self.output.as_os_str() == "-" {
            return Err(ReplayError::Invalid("replay needs a file output, not stdout".into()));
        }
        let mut state = self.replay.lock().unwrap_or_else(|e| e.into_inner());
        if state.running.is_some() {
            return Err(ReplayError::Running);
        }
        let pipeline = match schema {
            Some(s) if Some(s) != self.pipeline().mapping.schema_name().into() => {
                let (p, _) = Pipeline::load(&self.parsers_dir, &self.mappings_dir, Some(s), self.default_offset_secs).map_err(|e| ReplayError::Invalid(format!("{e:#}")))?;
                Arc::new(p)
            }
            _ => self.pipeline(),
        };
        let (reader, total, names) = {
            let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.flush(false).map_err(|e| ReplayError::Io(e.to_string()))?;
            let total = store.len();
            let names = store.source_names().map_err(|e| ReplayError::Io(e.to_string()))?;
            let reader = RawReader::open(&self.store_dir).map_err(|e| ReplayError::Io(e.to_string()))?;
            let total = total.min(reader.len());
            (reader, total, names)
        };
        let versions = Versions::new(&self.output);
        let version = versions.next();
        let progress = Arc::new(AtomicU64::new(0));
        let mut started = String::new();
        ulpf_time::format_rfc3339(now_nanos(), &mut started);
        state.running = Some((ReplayProgress { version, done: 0, total, started }, Arc::clone(&progress)));
        state.last_error = None;
        drop(state);
        self.replay_generation.fetch_add(1, Relaxed);
        let job = replay::Job { versions, version, pipeline, threads: self.threads, batch: self.batch_events, parsers_generation: self.generation.load(Relaxed), names, reader, total };
        let live = Arc::clone(self);
        std::thread::Builder::new()
            .name("ulpf-replay".into())
            .spawn(move || {
                let cancel = AtomicBool::new(false);
                let result = replay::run(job, &progress, &cancel);
                let mut state = live.replay.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(report) => state.last = Some(report),
                    Err(e) => state.last_error = Some(format!("{e:#}")),
                }
                state.running = None;
                drop(state);
                live.replay_generation.fetch_add(1, Relaxed);
            })
            .map_err(|e| ReplayError::Io(e.to_string()))?;
        Ok((version, total))
    }

    /// Verifies every record below the store's current length on its own thread, through
    /// the writer's flushed files (D42), and records the result for the API.
    pub fn start_verify(self: &Arc<Self>) -> Result<u64, IntegrityError> {
        let mut state = self.integrity.lock().unwrap_or_else(|e| e.into_inner());
        if state.running {
            return Err(IntegrityError::Running);
        }
        let reader = self.store.lock().unwrap_or_else(|e| e.into_inner()).reader().map_err(|e| IntegrityError::Io(e.to_string()))?;
        let records = reader.len();
        state.running = true;
        drop(state);
        self.integrity_generation.fetch_add(1, Relaxed);
        let live = Arc::clone(self);
        std::thread::Builder::new()
            .name("ulpf-verify".into())
            .spawn(move || {
                let started = Instant::now();
                let report = reader.verify();
                let mut at = String::new();
                ulpf_time::format_rfc3339(now_nanos(), &mut at);
                let last = LastVerify {
                    at,
                    records: report.checked,
                    ok: report.ok(),
                    corrupt: report.corrupt.len() as u64,
                    first_bad: report.first_bad.map(|(id, _)| id.0),
                    reason: report.first_bad.map(|(_, r): (RawId, VerifyReason)| r.as_str()),
                    elapsed_secs: started.elapsed().as_secs_f64(),
                    against_attestation: false,
                };
                let mut state = live.integrity.lock().unwrap_or_else(|e| e.into_inner());
                state.last = Some(last);
                state.running = false;
                drop(state);
                live.integrity_generation.fetch_add(1, Relaxed);
            })
            .map_err(|e| IntegrityError::Io(e.to_string()))?;
        Ok(records)
    }

    /// `GET /api/integrity` without the checkpoints: what the store is, and the last verify.
    pub fn integrity_summary(&self) -> serde_json::Value {
        let (records, store_id, genesis, head) = {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            (store.len(), ulpf_store::hex(&store.store_id()), ulpf_store::hex(&store.genesis()), store.head().map(|h| ulpf_store::hex(&h)))
        };
        let state = self.integrity.lock().unwrap_or_else(|e| e.into_inner());
        serde_json::json!({
            "records": records, "store_id": store_id, "genesis": genesis, "head": head,
            "checkpoint_every": ulpf_store::CHECKPOINT_EVERY,
            "last_verify": state.last, "running": state.running,
        })
    }

    pub fn attestation(&self) -> std::io::Result<Attestation> {
        Ok(self.store.lock().unwrap_or_else(|e| e.into_inner()).reader()?.attest())
    }

    /// One entity's timeline from the index beside the output; the index is opened once.
    pub fn pivot(&self, q: &PivotQuery<'_>) -> Result<PivotPage> {
        let mut idx = self.pivot_index.lock().unwrap_or_else(|e| e.into_inner());
        if idx.is_none() {
            *idx = Some(PivotIndex::open(&self.output)?);
        }
        idx.as_ref().expect("opened above").query(q)
    }

    pub fn entities(&self, kind: Option<EntityKind>, prefix: &str, limit: usize) -> Result<Vec<crate::pivot::EntitySummary>> {
        let mut idx = self.pivot_index.lock().unwrap_or_else(|e| e.into_inner());
        if idx.is_none() {
            *idx = Some(PivotIndex::open(&self.output)?);
        }
        idx.as_ref().expect("opened above").entities(kind, prefix, limit)
    }

    /// The current progress of a running replay, if any.
    pub fn replay_progress(&self) -> Option<ReplayProgress> {
        let state = self.replay.lock().unwrap_or_else(|e| e.into_inner());
        state.running.as_ref().map(|(p, done)| ReplayProgress { done: done.load(Relaxed), ..p.clone() })
    }

    /// A page of one version's diff, indexing the file on first use.
    pub fn replay_diff(&self, version: u64, after: Option<u64>, limit: usize, kind: Option<&str>) -> Result<(Vec<replay::DiffEntry>, Option<u64>), ReplayError> {
        let versions = Versions::new(&self.output);
        let path = versions.diff_path(version);
        if !path.exists() {
            return Err(ReplayError::Invalid(format!("version {version} has no diff")));
        }
        let index = {
            let mut state = self.replay.lock().unwrap_or_else(|e| e.into_inner());
            match state.diff_indexes.get(&version) {
                Some(i) => Arc::clone(i),
                None => {
                    let i = Arc::new(replay::index_diff(&path).map_err(|e| ReplayError::Io(e.to_string()))?);
                    state.diff_indexes.insert(version, Arc::clone(&i));
                    i
                }
            }
        };
        replay::page(&path, &index, after, limit, kind).map_err(|e| ReplayError::Io(e.to_string()))
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
                let files = pipeline.files.clone();
                let schema = pipeline.mapping.schema_name().to_string();
                *self.pipeline.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(pipeline);
                *self.load_problems.lock().unwrap_or_else(|e| e.into_inner()) = problems.clone();
                *self.parsers_signature.lock().unwrap_or_else(|e| e.into_inner()) = parsers_signature(&self.parsers_dir);
                self.metrics.reloads.fetch_add(1, Relaxed);
                let generation = self.generation.fetch_add(1, Relaxed) + 1;
                if self.output.as_os_str() != "-"
                    && let Err(e) = Versions::new(&self.output).write_live_meta(&schema, generation, files)
                {
                    eprintln!("ulpf: version meta: {e:#}");
                }
                ReloadReport { parsers_loaded: loaded, problems, generation }
            }
            Err(e) => ReloadReport { parsers_loaded: self.pipeline().registry.len(), problems: vec![format!("reload failed, previous registry kept: {e:#}")], generation: self.generation.load(Relaxed) },
        }
    }

    /// True when the set of `*.toml` files in the parsers directory changed since the last load.
    fn parsers_dir_changed(&self) -> bool {
        let now = parsers_signature(&self.parsers_dir);
        let mut last = self.parsers_signature.lock().unwrap_or_else(|e| e.into_inner());
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

    /// The pending directory, or `NotFound`: with inference off there is nothing to review.
    pub fn pending_or_err(&self) -> Result<&Pending, ReviewError> {
        self.pending.as_ref().ok_or_else(|| ReviewError::NotFound("inference is disabled: no pending directory".into()))
    }

    /// Approval: the definition moves to the parsers directory, the registry reloads, and
    /// the source's buffered unknown lines are re-detected to prove the fast path.
    pub fn approve(&self, id: &str) -> Result<ApproveReport, ReviewError> {
        let pending = self.pending_or_err()?;
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
        self.clear_drift(&approved.source);
        self.metrics.approved.fetch_add(1, Relaxed);
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(ApproveReport { name: approved.name, path: approved.path, parsers_loaded: reload.parsers_loaded, problems: reload.problems, now_detected: NowDetected { tested: lines.len() as u64, detected }, replaced_version: approved.replaced_version })
    }

    pub fn reject(&self, id: &str) -> Result<PathBuf, ReviewError> {
        let pending = self.pending_or_err()?;
        let source = pending.get(id).ok().map(|d| d.source);
        let moved = pending.reject(id)?;
        if let Some(s) = source {
            self.inference.clear(&s);
            self.clear_drift(&s);
        }
        self.metrics.rejected.fetch_add(1, Relaxed);
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(moved)
    }

    /// A tripped source whose update was approved or rejected starts a fresh baseline.
    fn clear_drift(&self, source: &str) {
        let mut sources = self.sources.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = sources.get_mut(source)
            && s.routing()
        {
            s.drift = DriftState::Cleared;
            s.baseline_events = 0;
            s.baseline_misses = 0;
            s.window_events = 0;
            s.window_misses = 0;
            s.drift_since_nanos = now_nanos();
            self.metrics.drift_cleared.fetch_add(1, Relaxed);
            self.drift_generation.fetch_add(1, Relaxed);
        }
    }

    /// Every established source's drift state, tripped and proposed first.
    pub fn drift_alerts(&self) -> Vec<DriftAlert> {
        let sources = self.sources.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<DriftAlert> = sources
            .iter()
            .filter(|(_, s)| s.drift != DriftState::None)
            .map(|(name, s)| {
                let id = Pending::id_for(name);
                let (pending_id, proposed_version, state) = match (&self.pending, s.drift) {
                    (Some(p), DriftState::Tripped | DriftState::Proposed) => match p.get(&id) {
                        Ok(d) if d.record.updates.is_some() => (Some(id), toml::from_str::<ulpf_parse::def::ParserDefinition>(&d.definition).ok().map(|d| d.parser.version), DriftState::Proposed),
                        _ => (None, None, DriftState::Tripped),
                    },
                    _ => (None, None, s.drift),
                };
                let mut since = String::new();
                if s.drift_since_nanos > 0 {
                    ulpf_time::format_rfc3339(s.drift_since_nanos, &mut since);
                }
                DriftAlert {
                    source: name.clone(),
                    parser: s.established_parser().unwrap_or("").to_string(),
                    state,
                    since,
                    window: DriftWindow { events: s.window_events, misses: s.window_misses, rate: s.window_rate },
                    baseline_rate: s.baseline_rate(),
                    lines_routed: s.drift_lines_routed,
                    pending_id,
                    proposed_version,
                }
            })
            .collect();
        out.sort_by_key(|a| match a.state {
            DriftState::Tripped => 0,
            DriftState::Proposed => 1,
            DriftState::Cleared => 2,
            _ => 3,
        });
        out
    }

    pub fn regenerate(&self, id: &str, keep: &[u64], merge: &[Vec<u64>]) -> Result<(String, Vec<String>), ReviewError> {
        let r = self.pending_or_err()?.regenerate(id, keep, merge, &self.inference.params)?;
        self.pending_generation.fetch_add(1, Relaxed);
        Ok(r)
    }

    pub fn put_text(&self, id: &str, text: &str) -> Result<Vec<String>, ReviewError> {
        let r = self.pending_or_err()?.put_text(id, text)?;
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
        let (chain, prev_chain) = {
            let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let chain = store.chain(RawId(id)).map_err(|e| TracebackError::Io(e.to_string()))?.unwrap_or([0; 32]);
            let prev = if id == 0 { store.genesis() } else { store.chain(RawId(id - 1)).map_err(|e| TracebackError::Io(e.to_string()))?.unwrap_or([0; 32]) };
            (chain, prev)
        };
        let expected: [u8; 32] = {
            let mut h = sha2::Sha256::new();
            h.update(prev_chain);
            h.update(rec.sha256);
            h.finalize().into()
        };
        let pipeline = self.pipeline();
        let mut scratch = pipeline.registry.scratch();
        let mut parsed = Parsed::default();
        let mut out = Vec::new();
        let mut hint = None;
        let outcome = pipeline.process(&rec.bytes, id, &source, rec.receipt_nanos, &mut hint, &mut scratch, &mut parsed, &mut out);
        let normalized: serde_json::Value = serde_json::from_slice(&out).unwrap_or(serde_json::Value::Null);
        let fields: Vec<TraceField> = parsed
            .fields
            .iter()
            .map(|f| TraceField { key: String::from_utf8_lossy(&f.key).into_owned(), value: String::from_utf8_lossy(&f.value).into_owned(), span: span_in(&rec.bytes, &f.value) })
            .collect();
        let provenance: Vec<TraceProvenance> = pipeline
            .mapping
            .provenance(&parsed)
            .into_iter()
            .map(|p| {
                let f = parsed.fields.get(p.field_index as usize);
                TraceProvenance {
                    path: p.path,
                    source_key: f.map(|f| String::from_utf8_lossy(&f.key).into_owned()).unwrap_or_default(),
                    span: f.and_then(|f| span_in(&rec.bytes, &f.value)),
                    canonical: p.canonical,
                    value: p.value,
                }
            })
            .collect();
        let time = TraceTime {
            text_span: parsed.timestamp_text.as_ref().and_then(|t| span_in(&rec.bytes, t)),
            policies: normalized.pointer("/ulpf/time_policies").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        };
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
            chain: hex(&chain),
            prev_chain: hex(&prev_chain),
            chain_match: expected == chain,
            emitted: self.tail.find(id).and_then(|l| serde_json::from_slice(&l).ok()),
            now: NowParse { parser, parse_status, normalized, fields, provenance, time },
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
    let (out_tx, out_rx) = sync_channel::<Emitted>(live.queue_cap * 2);
    let in_flight = Arc::new(AtomicI64::new(0));
    let writer = scope.spawn(move || output_thread(live, out_rx));
    let inference = scope.spawn(move || {
        if let Some(p) = &live.pending {
            live.inference.run_thread(p, &live.metrics, &live.pending_generation, &live.drift_generation);
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
    // Every thread is joined before anything returns: an early `?` here would leave the
    // scoped inference thread waiting for a stop that never comes, and the scope would hang.
    let mut first_error: Option<anyhow::Error> = None;
    for w in t.workers {
        if w.join().is_err() && first_error.is_none() {
            first_error = Some(anyhow!("worker thread panicked"));
        }
    }
    let writer_result = t.writer.join().unwrap_or_else(|_| Err(anyhow!("output thread panicked")));
    let elapsed = live.started.elapsed();
    let infer_started = Instant::now();
    live.inference.stop();
    if t.inference.join().is_err() && first_error.is_none() {
        first_error = Some(anyhow!("inference thread panicked"));
    }
    let inference_secs = infer_started.elapsed();
    if let Some(e) = first_error {
        return Err(e);
    }
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
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in &cfg.inputs {
        for path in collect_inputs(std::slice::from_ref(root))? {
            if seen.insert(path.clone()) {
                files.push((path.clone(), source_name(root, &path)));
            }
        }
    }
    let mut input_problems = Vec::new();
    let timing = std::thread::scope(|scope| {
        let mut t = start(scope, &live);
        let ingest_result = (|| {
            for (path, name) in &files {
                ingest_file(&live, path, name, 0, true, true, &t.batch_tx, &t.in_flight, &mut t.seq, &mut input_problems)?;
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
        for root in &live.watch {
            let paths = collect_inputs(std::slice::from_ref(root)).unwrap_or_default();
            for path in paths {
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                let size = meta.len();
                let name = source_name(root, &path);
                let entry = files.entry(path.clone()).or_insert_with(|| {
                    let consumed = resume.get(&name).copied().unwrap_or(0);
                    live.metrics.files.fetch_add(1, Relaxed);
                    Tailed { consumed, last_size: size, stable_ticks: 0, growing_ticks: 0 }
                });
                if size < entry.consumed {
                    // truncated or replaced: start over, and say so now, not at shutdown
                    let msg = format!("{}: shrank below the ingested offset, re-reading from the start", path.display());
                    eprintln!("ulpf: input problem: {msg}");
                    problems.push(msg);
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
                    match ingest_file(live, &path, &name, entry.consumed, finalize, false, tx, in_flight, seq, problems)? {
                        Some(consumed) => {
                            entry.consumed = consumed;
                            live.metrics.bytes.fetch_add(consumed.saturating_sub(before), Relaxed);
                            if finalize {
                                entry.growing_ticks = 0;
                                entry.stable_ticks = 0;
                            }
                        }
                        // unreadable: counted and reported once; tried again only when the file changes
                        None => entry.consumed = size,
                    }
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

/// Frames and stores `path` (source `name`) from byte `start`. With `eof` the whole
/// remainder is consumed; without it the last line is withheld until more bytes arrive.
/// Returns the new consumed offset, or `None` when the file could not be read (counted
/// and reported here, once). Batch mode passes `count_file` and the whole file is counted
/// here; the tailer counts a file when it first sees it and bytes as it consumes them.
#[allow(clippy::too_many_arguments)]
fn ingest_file(live: &Arc<Live>, path: &Path, name: &str, start: u64, eof: bool, count_file: bool, tx: &SyncSender<Batch>, in_flight: &AtomicI64, seq: &mut u64, problems: &mut Vec<String>) -> Result<Option<u64>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            live.metrics.files_failed.fetch_add(1, Relaxed);
            let msg = format!("{}: {e}", path.display());
            if !count_file {
                eprintln!("ulpf: input problem: {msg}");
            }
            problems.push(msg);
            return Ok(None);
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
                return Ok(None);
            }
        }
    };
    if count_file {
        live.metrics.files.fetch_add(1, Relaxed);
        live.metrics.bytes.fetch_add(len, Relaxed);
    }
    let start = (start as usize).min(len as usize);
    let source = live.store.lock().unwrap_or_else(|e| e.into_inner()).source_id(name)?;
    let ingest_started = now_nanos();
    let ctx = Arc::new(FileCtx { backing: Backing::Mapped(mmap), name: name.to_string(), names: HashMap::new() });
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
            let first = store.len();
            for r in &ranges {
                store.append(source, receipt, &bytes[r.clone()]).context("raw store append failed; aborting to avoid an incomplete store")?;
            }
            store.flush(false).context("raw store flush failed")?;
            first
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
    Ok(Some(consumed as u64))
}

#[allow(clippy::too_many_arguments)]
fn send_batch(tx: &SyncSender<Batch>, metrics: &Metrics, in_flight: &AtomicI64, queue_cap: usize, seq: &mut u64, ctx: &Arc<FileCtx>, receipt: i64, first_raw_id: u64, ranges: Vec<std::ops::Range<usize>>) -> Result<()> {
    metrics.batches.fetch_add(1, Relaxed);
    let batch = Batch { seq: *seq, file: Arc::clone(ctx), receipt_nanos: receipt, first_raw_id, ranges, receipts: Vec::new(), sources: Vec::new() };
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

fn worker_thread(live: &Live, rx: Arc<Mutex<Receiver<Batch>>>, tx: SyncSender<Emitted>, in_flight: &AtomicI64) {
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
        let mut out = Vec::with_capacity(batch.ranges.len() * 512);
        let mut counts = LocalCounts::default();
        // `Parsed` borrows the batch's bytes and the current registry, so it lives per batch
        let mut parsed = Parsed::default();
        let mut unknown: Vec<(u64, &[u8])> = Vec::new();
        let mut failed: Vec<(u64, &[u8])> = Vec::new();
        let mut entities = EntityBatch::default();
        let pipeline = &*pipeline;
        process_batch(pipeline, &batch, &mut scratch, &mut hint, &mut parsed, &mut out, &mut counts, &mut hits, &mut unknown, &mut failed, Some(&mut entities));
        live.metrics.add(&counts);
        if !unknown.is_empty() {
            live.inference.offer_batch(&batch.file.name, &unknown, &live.metrics);
        }
        let parse_failed: u64 = counts.parse_failed.iter().sum();
        let (tripped, routing) = {
            let mut sources = live.sources.lock().unwrap_or_else(|e| e.into_inner());
            let s = sources.entry(batch.file.name.clone()).or_default();
            s.events += batch.ranges.len() as u64;
            s.detected += counts.detected;
            s.no_parser += counts.no_parser;
            s.parse_failed += parse_failed;
            s.last_seen_nanos = batch.receipt_nanos;
            for (i, h) in hits.iter().enumerate() {
                if *h > 0 {
                    *s.by_parser.entry(pipeline.registry.get(i).name().to_string()).or_default() += h;
                }
            }
            let tripped = s.observe(batch.ranges.len() as u64, counts.no_parser + parse_failed);
            // the batch that completed the tripping window is judged, not routed
            let routing = s.routing() && tripped.is_none();
            if routing {
                s.drift_lines_routed += (unknown.len() + failed.len()) as u64;
            }
            (tripped, routing)
        };
        if let Some(parser) = tripped {
            // the window that tripped is judged, not clustered: from here on the source's
            // misses (unknown and failed alike) go to inference with the parser as prior
            if let Some(def) = pipeline.registry.index_of(&parser).map(|i| pipeline.registry.get(i).definition().clone()) {
                live.inference.set_prior(&batch.file.name, def);
            }
            live.metrics.drift_tripped.fetch_add(1, Relaxed);
            live.drift_generation.fetch_add(1, Relaxed);
        }
        if routing {
            if !failed.is_empty() {
                live.inference.offer_batch(&batch.file.name, &failed, &live.metrics);
            }
            live.metrics.drift_lines_routed.fetch_add((unknown.len() + failed.len()) as u64, Relaxed);
        }
        if hits.iter().any(|h| *h > 0) {
            let mut ph = live.parser_hits.lock().unwrap_or_else(|e| e.into_inner());
            for (i, h) in hits.iter().enumerate() {
                if *h > 0 {
                    *ph.entry(pipeline.registry.get(i).name().to_string()).or_default() += h;
                }
            }
        }
        if tx.send(Emitted { seq: batch.seq, buf: out, count: batch.ranges.len() as u64, first_raw_id: batch.first_raw_id, entities }).is_err() {
            break;
        }
    }
}

fn output_thread(live: &Live, rx: Receiver<Emitted>) -> Result<()> {
    let stdout;
    let file;
    let mut pos = 0u64;
    let mut pivot: Option<PivotWriter> = None;
    let mut w: Box<dyn Write> = if live.output.as_os_str() == "-" {
        stdout = std::io::stdout();
        Box::new(BufWriter::with_capacity(1 << 20, stdout.lock()))
    } else {
        file = File::options().create(true).append(true).open(&live.output).with_context(|| format!("creating output {}", live.output.display()))?;
        pos = file.metadata().map(|m| m.len()).unwrap_or(0);
        // the entity index beside the output: derived data on its own thread (D55)
        match PivotWriter::start(&live.output, live.queue_cap) {
            Ok(pw) => {
                *live.pivot_counters.lock().unwrap_or_else(|e| e.into_inner()) = Some(pw.counters());
                pivot = Some(pw);
            }
            Err(e) => eprintln!("ulpf: pivot index disabled: {e:#}"),
        }
        Box::new(BufWriter::with_capacity(1 << 20, file))
    };
    let mut pending: BTreeMap<u64, (Vec<u8>, u64, u64, EntityBatch)> = BTreeMap::new();
    let mut next = 0u64;
    let mut since_flush = Instant::now();
    while let Ok(e) = rx.recv() {
        pending.insert(e.seq, (e.buf, e.count, e.first_raw_id, e.entities));
        while let Some((buf, count, first_raw_id, entities)) = pending.remove(&next) {
            w.write_all(&buf).context("writing output")?;
            live.metrics.emitted.fetch_add(count, Relaxed);
            live.metrics.output_bytes.fetch_add(buf.len() as u64, Relaxed);
            if let Some(pw) = pivot.as_mut() {
                let mut postings: Vec<Posting<'_>> = Vec::with_capacity(entities.events.len() * 2);
                for ev in &entities.events {
                    let device = entities.slice(ev.device);
                    let parser = (ev.parser.0 != ev.parser.1).then(|| std::str::from_utf8(entities.slice(ev.parser)).unwrap_or(""));
                    for (k, r) in ev.values.iter().enumerate() {
                        if r.0 != r.1 {
                            postings.push(Posting { raw_id: ev.raw_id, time_ms: ev.time_ms, kind: EntityKind::ALL[k], value: entities.slice(*r), device, parser, offset: pos + ev.line.0 as u64, len: ev.line.1.saturating_sub(ev.line.0) });
                        }
                    }
                }
                pw.push_batch(&postings);
            }
            pos += buf.len() as u64;
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
    if let Some(pw) = pivot {
        pw.finish();
    }
    Ok(())
}
