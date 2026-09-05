//! Replay: every record of the raw store through the current parsers and mappings into
//! a new versioned output, then a streaming diff against the previous version. The store
//! is read through a bounded snapshot and never written; the replay has its own counters,
//! output and threads, so the live pipeline, tail and inference buffers see nothing.
//!
//! Versions: the path given as `--output out.jsonl` is version 1; replays write
//! `out.v2.jsonl`, `out.v3.jsonl`, ... beside it, each with `out.vN.meta.json` (what was
//! used, and the summary) and `out.vN.diff.jsonl` (one entry per event that differs).

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ulpf_parse::Parsed;
use ulpf_store::{RawId, RawReader};

use crate::engine::{Backing, Batch, Emitted, FileCtx, now_nanos, process_batch};
use crate::metrics::{LocalCounts, Metrics, Snapshot};
use crate::pipeline::Pipeline;

/// Output versions beside one base path.
#[derive(Debug, Clone)]
pub struct Versions {
    dir: PathBuf,
    stem: String,
    ext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub version: u64,
    pub path: PathBuf,
    pub created: String,
    pub events: u64,
    pub schema: String,
    pub parsers_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDigest {
    pub path: String,
    pub sha256: String,
}

/// `out.vN.meta.json`: what produced a version, and (for a replay) what it found.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Meta {
    pub version: u64,
    pub created: String,
    /// The raw store this output was written from (hex store id), so a restart can tell
    /// its own interrupted output from a fresh one or another store's.
    #[serde(default)]
    pub store_id: String,
    pub schema: String,
    pub parsers_generation: u64,
    pub files: Vec<FileDigest>,
    /// Earlier file sets this version ran with (the live output reloads parsers in place).
    #[serde(default)]
    pub history: Vec<Vec<FileDigest>>,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub events: u64,
    #[serde(default)]
    pub report: Option<ReplayReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffSummary {
    pub unchanged: u64,
    pub changed: u64,
    pub only_in_new: u64,
    pub only_in_old: u64,
    pub fields_added: u64,
    pub fields_lost: u64,
    pub fields_changed: u64,
    pub parser_changes: Vec<ParserChange>,
    pub by_field: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserChange {
    pub from: Option<String>,
    pub to: Option<String>,
    pub events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    pub path: String,
    pub added: u64,
    pub lost: u64,
    pub changed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub version: u64,
    pub previous_version: Option<u64>,
    pub output: PathBuf,
    pub diff: Option<PathBuf>,
    pub events: u64,
    pub elapsed_secs: f64,
    pub events_per_sec: f64,
    pub parsers_generation: u64,
    pub schema: String,
    pub summary: DiffSummary,
    pub why: Vec<String>,
    pub counts: ReplayCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayCounts {
    pub detected: u64,
    pub no_parser: u64,
    pub parsed: u64,
    pub parse_failed: u64,
    pub class_unknown: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayProgress {
    pub version: u64,
    pub done: u64,
    pub total: u64,
    pub started: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffEntry {
    pub raw_id: u64,
    pub kind: &'static str,
    pub parser_before: Option<String>,
    pub parser_after: Option<String>,
    pub added: Map<String, Value>,
    pub lost: Map<String, Value>,
    pub changed: Map<String, Value>,
}

/// Every 1024th diff entry: (raw id, byte offset) so a page can start near `after`.
#[derive(Debug, Clone, Default)]
pub struct DiffIndex(Vec<(u64, u64)>);

const DIFF_INDEX_STRIDE: u64 = 1024;

impl Versions {
    pub fn new(output: &Path) -> Versions {
        let dir = output.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "out.jsonl".into());
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
            _ => (name.clone(), String::new()),
        };
        Versions { dir, stem, ext }
    }

    fn with_ext(&self, middle: &str, ext: &str) -> PathBuf {
        let mut n = self.stem.clone();
        n.push_str(middle);
        if !ext.is_empty() {
            n.push('.');
            n.push_str(ext);
        }
        self.dir.join(n)
    }

    pub fn path(&self, version: u64) -> PathBuf {
        if version <= 1 { self.with_ext("", &self.ext) } else { self.with_ext(&format!(".v{version}"), &self.ext) }
    }

    pub fn meta_path(&self, version: u64) -> PathBuf {
        self.with_ext(&format!(".v{version}.meta"), "json")
    }

    pub fn diff_path(&self, version: u64) -> PathBuf {
        self.with_ext(&format!(".v{version}.diff"), &self.ext)
    }

    pub fn read_meta(&self, version: u64) -> Option<Meta> {
        let text = std::fs::read_to_string(self.meta_path(version)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn write_meta(&self, meta: &Meta) -> Result<()> {
        let path = self.meta_path(meta.version);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(meta)?).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("renaming {}", path.display()))?;
        Ok(())
    }

    /// Versions with a meta file, ascending. Version 1 is listed when its file exists
    /// even without a meta (an output written before metas existed).
    pub fn list(&self) -> Vec<VersionInfo> {
        let mut out = Vec::new();
        let mut v = 1u64;
        loop {
            let path = self.path(v);
            let meta = self.read_meta(v);
            if !path.exists() && meta.is_none() {
                break;
            }
            let events = match &meta {
                Some(m) if m.events > 0 => m.events,
                _ => count_lines(&path),
            };
            out.push(VersionInfo {
                version: v,
                path,
                created: meta.as_ref().map(|m| m.created.clone()).unwrap_or_default(),
                events,
                schema: meta.as_ref().map(|m| m.schema.clone()).unwrap_or_default(),
                parsers_generation: meta.as_ref().map(|m| m.parsers_generation).unwrap_or(0),
            });
            v += 1;
        }
        out
    }

    /// The next version number: one past the highest version that has a file or a meta.
    pub fn next(&self) -> u64 {
        let mut v = 2u64;
        while self.path(v).exists() || self.meta_path(v).exists() {
            v += 1;
        }
        v
    }

    /// The newest complete version below `version`, if any.
    pub fn previous_of(&self, version: u64) -> Option<u64> {
        // a replay that failed or was cancelled leaves a file with no complete meta; it is
        // not a version to compare against
        (1..version).rev().find(|&v| self.path(v).exists() && (v == 1 || self.read_meta(v).is_some_and(|m| m.complete)))
    }

    /// Records what the live output is being written with; called at open and reload.
    pub fn write_live_meta(&self, store_id: &str, schema: &str, generation: u64, files: Vec<FileDigest>) -> Result<()> {
        let mut meta = self.read_meta(1).unwrap_or_default();
        meta.store_id = store_id.to_string();
        if meta.version == 0 {
            meta.version = 1;
            let mut created = String::new();
            ulpf_time::format_rfc3339(now_nanos(), &mut created);
            meta.created = created;
        }
        if !meta.files.is_empty() && meta.files != files {
            let old = std::mem::take(&mut meta.files);
            meta.history.push(old);
        }
        meta.schema = schema.to_string();
        meta.parsers_generation = generation;
        meta.files = files;
        meta.complete = true;
        self.write_meta(&meta)
    }
}

fn count_lines(path: &Path) -> u64 {
    let Ok(f) = File::open(path) else { return 0 };
    let mut r = BufReader::with_capacity(1 << 20, f);
    let mut n = 0u64;
    let mut buf = [0u8; 1 << 16];
    while let Ok(k) = r.read(&mut buf) {
        if k == 0 {
            break;
        }
        n += memchr::memchr_iter(b'\n', &buf[..k]).count() as u64;
    }
    n
}

/// Everything one replay needs, assembled by the caller (CLI or `Live`) so this module
/// never opens a store it was not handed.
pub struct Job {
    pub versions: Versions,
    pub version: u64,
    pub pipeline: Arc<Pipeline>,
    pub threads: usize,
    pub batch: usize,
    pub parsers_generation: u64,
    pub names: HashMap<u32, String>,
    pub reader: RawReader,
    /// Records `0..total` are replayed; the reader may hold more (appended after the snapshot).
    pub total: u64,
}

/// Runs the job to completion: output, meta, diff. `progress` counts emitted events.
pub fn run(job: Job, progress: &AtomicU64, cancel: &AtomicBool) -> Result<ReplayReport> {
    let started = Instant::now();
    let version = job.version;
    let out_path = job.versions.path(version);
    if version <= 1 {
        bail!("replay writes version 2 or later, never over the live output");
    }
    let file = File::create(&out_path).with_context(|| format!("creating {}", out_path.display()))?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let metrics = Metrics::default();
    let ctx = Arc::new(FileCtx { backing: Backing::Store(job.reader), name: "replay".into(), names: job.names });
    let pipeline = Arc::clone(&job.pipeline);
    let total = job.total;
    let threads = job.threads.max(1);
    let batch_size = job.batch.max(1);
    let emitted_total = std::thread::scope(|scope| -> Result<u64> {
        let (batch_tx, batch_rx) = sync_channel::<Batch>(threads * 2);
        let batch_rx = Arc::new(Mutex::new(batch_rx));
        let (out_tx, out_rx) = sync_channel::<Emitted>(threads * 4);
        let mut workers = Vec::new();
        for _ in 0..threads {
            let rx = Arc::clone(&batch_rx);
            let tx = out_tx.clone();
            let pipeline = Arc::clone(&pipeline);
            let metrics = &metrics;
            workers.push(scope.spawn(move || {
                let mut scratch = pipeline.registry.scratch();
                let mut hint = None;
                let mut hits = Vec::new();
                loop {
                    let batch = {
                        let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                        guard.recv()
                    };
                    let Ok(batch) = batch else { break };
                    let mut out = Vec::with_capacity(batch.ranges.len() * 512);
                    let mut counts = LocalCounts::default();
                    let mut parsed = Parsed::default();
                    let mut unknown = Vec::new();
                    let mut failed = Vec::new();
                    hits.clear();
                    hits.resize(pipeline.registry.len(), 0);
                    process_batch(&pipeline, &batch, &mut scratch, &mut hint, &mut parsed, &mut out, &mut counts, &mut hits, &mut unknown, &mut failed, None);
                    metrics.add(&counts);
                    if tx.send(Emitted { seq: batch.seq, buf: out, count: batch.ranges.len() as u64, first_raw_id: batch.first_raw_id, entities: crate::engine::EntityBatch::default() }).is_err() {
                        break;
                    }
                }
            }));
        }
        drop(out_tx);
        drop(batch_rx);
        let reader_ctx = Arc::clone(&ctx);
        let reader = scope.spawn(move || -> Result<()> {
            let Backing::Store(store) = &reader_ctx.backing else { unreachable!() };
            let seg = store.segment();
            let mut seq = 0u64;
            let mut id = 0u64;
            while id < total {
                if cancel.load(Relaxed) {
                    bail!("replay cancelled");
                }
                let first = id;
                let n = ((total - id) as usize).min(batch_size);
                let mut ranges = Vec::with_capacity(n);
                let mut receipts = Vec::with_capacity(n);
                let mut sources = Vec::with_capacity(n);
                for i in 0..n {
                    let rec = store.get(RawId(first + i as u64)).ok_or_else(|| anyhow!("raw id {} is unreadable in the store snapshot", first + i as u64))?;
                    let start = rec.bytes.as_ptr() as usize - seg.as_ptr() as usize;
                    ranges.push(start..start + rec.bytes.len());
                    receipts.push(rec.receipt_nanos);
                    sources.push(rec.source);
                }
                id += n as u64;
                let batch = Batch { seq, file: Arc::clone(&reader_ctx), receipt_nanos: 0, first_raw_id: first, ranges, receipts, sources };
                seq += 1;
                if batch_tx.send(batch).is_err() {
                    break;
                }
            }
            Ok(())
        });
        let mut pending: BTreeMap<u64, (Vec<u8>, u64)> = BTreeMap::new();
        let mut next = 0u64;
        let mut emitted = 0u64;
        while let Ok(e) = out_rx.recv() {
            pending.insert(e.seq, (e.buf, e.count));
            while let Some((buf, count)) = pending.remove(&next) {
                writer.write_all(&buf).context("writing replay output")?;
                emitted += count;
                progress.store(emitted, Relaxed);
                next += 1;
            }
        }
        writer.flush().context("flushing replay output")?;
        let mut first_error = None;
        for w in workers {
            if w.join().is_err() && first_error.is_none() {
                first_error = Some(anyhow!("replay worker panicked"));
            }
        }
        let reader_result = reader.join().unwrap_or_else(|_| Err(anyhow!("replay reader panicked")));
        if let Some(e) = first_error {
            return Err(e);
        }
        reader_result?;
        Ok(emitted)
    })?;
    let elapsed = started.elapsed().as_secs_f64();
    let snap: Snapshot = metrics.snapshot(elapsed, threads, 0);
    let counts = ReplayCounts {
        detected: snap.detected,
        no_parser: snap.no_parser,
        parsed: snap.parsed,
        parse_failed: snap.parse_failed.iter().map(|(_, n)| n).sum(),
        class_unknown: snap.class_unknown,
    };
    let previous = job.versions.previous_of(version);
    let (summary, diff_path, why) = match previous {
        Some(prev) => {
            let diff_path = job.versions.diff_path(version);
            let (summary, _) = diff(&job.versions.path(prev), &out_path, &diff_path)?;
            let why = explain(&job.versions, prev, &job.pipeline.files, &summary);
            (summary, Some(diff_path), why)
        }
        None => (DiffSummary::default(), None, vec!["no previous version to compare against".into()]),
    };
    let mut created = String::new();
    ulpf_time::format_rfc3339(now_nanos(), &mut created);
    let schema = job.pipeline.mapping.schema_name().to_string();
    let report = ReplayReport {
        version,
        previous_version: previous,
        output: out_path,
        diff: diff_path,
        events: emitted_total,
        elapsed_secs: elapsed,
        events_per_sec: if elapsed > 0.0 { emitted_total as f64 / elapsed } else { 0.0 },
        parsers_generation: job.parsers_generation,
        schema: schema.clone(),
        summary,
        why,
        counts,
    };
    job.versions.write_meta(&Meta {
        version,
        created,
        store_id: String::new(),
        schema,
        parsers_generation: job.parsers_generation,
        files: job.pipeline.files.clone(),
        history: Vec::new(),
        complete: true,
        events: emitted_total,
        report: Some(report.clone()),
    })?;
    Ok(report)
}

/// The 4am answer: what changed between the file sets of two versions, in words.
fn explain(versions: &Versions, previous: u64, files: &[FileDigest], summary: &DiffSummary) -> Vec<String> {
    let mut why = Vec::new();
    match versions.read_meta(previous) {
        Some(prev) => {
            // the oldest set is what the previous version's first events were written with
            let base = prev.history.first().unwrap_or(&prev.files);
            let old: BTreeMap<&str, &str> = base.iter().map(|f| (f.path.as_str(), f.sha256.as_str())).collect();
            let new: BTreeMap<&str, &str> = files.iter().map(|f| (f.path.as_str(), f.sha256.as_str())).collect();
            let mut any = false;
            for (path, sha) in &new {
                match old.get(path) {
                    Some(o) if o == sha => {}
                    Some(o) => {
                        any = true;
                        why.push(format!("{path} changed since v{previous} (sha256 {}.. -> {}..)", &o[..8], &sha[..8]));
                    }
                    None => {
                        any = true;
                        why.push(format!("{path} is new since v{previous}"));
                    }
                }
            }
            for path in old.keys() {
                if !new.contains_key(path) {
                    any = true;
                    why.push(format!("{path} was removed since v{previous}"));
                }
            }
            if !prev.history.is_empty() {
                why.push(format!("v{previous} changed its parser files {} time(s) while it was written (reloads or reopens); the comparison is against the set its first events were written with", prev.history.len()));
            }
            if !any {
                let mappings_unchanged = new.keys().any(|p| p.contains("mappings"));
                why.push(if summary.changed > 0 {
                    "no parser or mapping file changed; the difference comes from receipt time, engine version or a reload during the previous run".to_string()
                } else if mappings_unchanged {
                    "parsers and mappings unchanged".to_string()
                } else {
                    "parsers unchanged".to_string()
                });
            }
        }
        None => why.push(format!("v{previous} has no meta file, so the parser files it used are unknown; only the event diff is available")),
    }
    if summary.only_in_new > 0 {
        why.push(format!("{} events only in the new version: stored after v{previous} was written", summary.only_in_new));
    }
    if summary.only_in_old > 0 {
        why.push(format!("{} events only in v{previous}: the store snapshot ended before them, or v{previous} was written from a different store", summary.only_in_old));
    }
    why
}

/// `"ulpf":{...,"raw_id":N,...}` without parsing the line.
pub(crate) fn raw_id_of(line: &[u8]) -> Option<u64> {
    let ulpf = memchr::memmem::find(line, b"\"ulpf\":{")?;
    let rest = &line[ulpf..];
    let k = memchr::memmem::find(rest, b"\"raw_id\":")? + 9;
    let digits = &rest[k..];
    let end = digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    std::str::from_utf8(&digits[..end]).ok()?.parse().ok()
}

fn read_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> Result<bool> {
    buf.clear();
    let n = r.read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(false);
    }
    while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        buf.pop();
    }
    Ok(true)
}

fn flatten(v: &Value, prefix: &mut String, out: &mut BTreeMap<String, Value>) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                let len = prefix.len();
                if !prefix.is_empty() {
                    prefix.push('.');
                }
                prefix.push_str(k);
                flatten(v, prefix, out);
                prefix.truncate(len);
            }
        }
        other => {
            out.insert(prefix.clone(), other.clone());
        }
    }
}

struct DiffAcc {
    summary: DiffSummary,
    parser_changes: BTreeMap<(Option<String>, Option<String>), u64>,
    by_field: BTreeMap<String, (u64, u64, u64)>,
}

impl DiffAcc {
    fn field(&mut self, path: &str, which: usize) {
        let e = self.by_field.entry(path.to_string()).or_default();
        match which {
            0 => e.0 += 1,
            1 => e.1 += 1,
            _ => e.2 += 1,
        }
    }
}

/// Streams both outputs in raw id order and writes one `DiffEntry` per differing event.
pub fn diff(old_path: &Path, new_path: &Path, diff_path: &Path) -> Result<(DiffSummary, DiffIndex)> {
    let mut old = BufReader::with_capacity(1 << 20, File::open(old_path).with_context(|| format!("opening {}", old_path.display()))?);
    let mut new = BufReader::with_capacity(1 << 20, File::open(new_path).with_context(|| format!("opening {}", new_path.display()))?);
    let mut out = BufWriter::with_capacity(1 << 20, File::create(diff_path).with_context(|| format!("creating {}", diff_path.display()))?);
    let mut acc = DiffAcc { summary: DiffSummary::default(), parser_changes: BTreeMap::new(), by_field: BTreeMap::new() };
    let mut index = DiffIndex::default();
    let mut written = 0u64;
    let mut offset = 0u64;
    let (mut ob, mut nb) = (Vec::new(), Vec::new());
    let mut have_old = read_line(&mut old, &mut ob)?;
    let mut have_new = read_line(&mut new, &mut nb)?;
    let mut emit = |entry: &DiffEntry, out: &mut BufWriter<File>| -> Result<()> {
        if written.is_multiple_of(DIFF_INDEX_STRIDE) {
            index.0.push((entry.raw_id, offset));
        }
        let line = serde_json::to_vec(entry)?;
        out.write_all(&line)?;
        out.write_all(b"\n")?;
        offset += line.len() as u64 + 1;
        written += 1;
        Ok(())
    };
    while have_old || have_new {
        let oid = if have_old { raw_id_of(&ob) } else { None };
        let nid = if have_new { raw_id_of(&nb) } else { None };
        match (oid, nid) {
            (Some(o), Some(n)) if o == n => {
                if ob == nb {
                    acc.summary.unchanged += 1;
                } else {
                    let entry = compare(o, &ob, &nb, &mut acc);
                    emit(&entry, &mut out)?;
                }
                have_old = read_line(&mut old, &mut ob)?;
                have_new = read_line(&mut new, &mut nb)?;
            }
            (Some(o), Some(n)) if o < n => {
                acc.summary.only_in_old += 1;
                emit(&only(o, &ob, "only_in_old"), &mut out)?;
                have_old = read_line(&mut old, &mut ob)?;
            }
            (Some(_), Some(n)) => {
                acc.summary.only_in_new += 1;
                emit(&only(n, &nb, "only_in_new"), &mut out)?;
                have_new = read_line(&mut new, &mut nb)?;
            }
            (Some(o), None) => {
                if have_new {
                    // a line with no raw id: skip it, it can never match anything
                    have_new = read_line(&mut new, &mut nb)?;
                    continue;
                }
                acc.summary.only_in_old += 1;
                emit(&only(o, &ob, "only_in_old"), &mut out)?;
                have_old = read_line(&mut old, &mut ob)?;
            }
            (None, Some(n)) => {
                if have_old {
                    have_old = read_line(&mut old, &mut ob)?;
                    continue;
                }
                acc.summary.only_in_new += 1;
                emit(&only(n, &nb, "only_in_new"), &mut out)?;
                have_new = read_line(&mut new, &mut nb)?;
            }
            (None, None) => {
                if have_old {
                    have_old = read_line(&mut old, &mut ob)?;
                }
                if have_new {
                    have_new = read_line(&mut new, &mut nb)?;
                }
            }
        }
    }
    out.flush()?;
    let mut summary = acc.summary;
    summary.parser_changes = acc.parser_changes.into_iter().map(|((from, to), events)| ParserChange { from, to, events }).collect();
    summary.parser_changes.sort_by_key(|p| std::cmp::Reverse(p.events));
    let mut by_field: Vec<FieldChange> = acc.by_field.into_iter().map(|(path, (added, lost, changed))| FieldChange { path, added, lost, changed }).collect();
    by_field.sort_by_key(|f| std::cmp::Reverse(f.added + f.lost + f.changed));
    by_field.truncate(200);
    summary.by_field = by_field;
    Ok((summary, index))
}

fn parser_of(v: &Value) -> Option<String> {
    v.get("ulpf")?.get("parser")?.as_str().map(str::to_string)
}

fn only(raw_id: u64, line: &[u8], kind: &'static str) -> DiffEntry {
    let v: Value = serde_json::from_slice(line).unwrap_or(Value::Null);
    let parser = parser_of(&v);
    let (parser_before, parser_after) = if kind == "only_in_old" { (parser, None) } else { (None, parser) };
    DiffEntry { raw_id, kind, parser_before, parser_after, added: Map::new(), lost: Map::new(), changed: Map::new() }
}

fn compare(raw_id: u64, old: &[u8], new: &[u8], acc: &mut DiffAcc) -> DiffEntry {
    let ov: Value = serde_json::from_slice(old).unwrap_or(Value::Null);
    let nv: Value = serde_json::from_slice(new).unwrap_or(Value::Null);
    let (mut of, mut nf) = (BTreeMap::new(), BTreeMap::new());
    flatten(&ov, &mut String::new(), &mut of);
    flatten(&nv, &mut String::new(), &mut nf);
    let mut entry = DiffEntry { raw_id, kind: "changed", parser_before: parser_of(&ov), parser_after: parser_of(&nv), added: Map::new(), lost: Map::new(), changed: Map::new() };
    for (k, v) in &nf {
        match of.get(k) {
            None => {
                entry.added.insert(k.clone(), v.clone());
                acc.field(k, 0);
            }
            Some(o) if o != v => {
                entry.changed.insert(k.clone(), Value::Array(vec![o.clone(), v.clone()]));
                acc.field(k, 2);
            }
            Some(_) => {}
        }
    }
    for (k, v) in &of {
        if !nf.contains_key(k) {
            entry.lost.insert(k.clone(), v.clone());
            acc.field(k, 1);
        }
    }
    acc.summary.changed += 1;
    acc.summary.fields_added += entry.added.len() as u64;
    acc.summary.fields_lost += entry.lost.len() as u64;
    acc.summary.fields_changed += entry.changed.len() as u64;
    if entry.parser_before != entry.parser_after {
        *acc.parser_changes.entry((entry.parser_before.clone(), entry.parser_after.clone())).or_default() += 1;
    }
    entry
}

/// Builds the sparse index of an existing diff file (the server after a restart).
pub fn index_diff(path: &Path) -> Result<DiffIndex> {
    let mut r = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut index = DiffIndex::default();
    let mut buf = Vec::new();
    let (mut n, mut offset) = (0u64, 0u64);
    loop {
        buf.clear();
        let k = r.read_until(b'\n', &mut buf)?;
        if k == 0 {
            break;
        }
        if n.is_multiple_of(DIFF_INDEX_STRIDE)
            && let Some(id) = raw_id_of_entry(&buf)
        {
            index.0.push((id, offset));
        }
        offset += k as u64;
        n += 1;
    }
    Ok(index)
}

fn raw_id_of_entry(line: &[u8]) -> Option<u64> {
    let k = memchr::memmem::find(line, b"\"raw_id\":")? + 9;
    let digits = &line[k..];
    let end = digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    std::str::from_utf8(&digits[..end]).ok()?.parse().ok()
}

/// A page of diff entries with raw id greater than `after`, bounded by `limit`.
pub fn page(path: &Path, index: &DiffIndex, after: Option<u64>, limit: usize, kind: Option<&str>) -> Result<(Vec<DiffEntry>, Option<u64>)> {
    let mut f = File::open(path)?;
    let start = match after {
        Some(a) => index.0.iter().rev().find(|(id, _)| *id <= a).map(|(_, off)| *off).unwrap_or(0),
        None => 0,
    };
    f.seek(SeekFrom::Start(start))?;
    let mut r = BufReader::with_capacity(1 << 16, f);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    let limit = limit.clamp(1, 500);
    let mut more = None;
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let Some(id) = raw_id_of_entry(&buf) else { continue };
        if after.is_some_and(|a| id <= a) {
            continue;
        }
        let Ok(entry) = serde_json::from_slice::<DiffEntry>(&buf) else { continue };
        if kind.is_some_and(|k| k != entry.kind) {
            continue;
        }
        if out.len() >= limit {
            more = Some(entry.raw_id);
            break;
        }
        out.push(entry);
    }
    let next_after = more.and_then(|_| out.last().map(|e| e.raw_id));
    Ok((out, next_after))
}

impl<'de> Deserialize<'de> for DiffEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            raw_id: u64,
            kind: String,
            parser_before: Option<String>,
            parser_after: Option<String>,
            #[serde(default)]
            added: Map<String, Value>,
            #[serde(default)]
            lost: Map<String, Value>,
            #[serde(default)]
            changed: Map<String, Value>,
        }
        let r = Raw::deserialize(d)?;
        let kind = match r.kind.as_str() {
            "changed" => "changed",
            "only_in_new" => "only_in_new",
            _ => "only_in_old",
        };
        Ok(DiffEntry { raw_id: r.raw_id, kind, parser_before: r.parser_before, parser_after: r.parser_after, added: r.added, lost: r.lost, changed: r.changed })
    }
}

/// Digests of every `*.toml` in a directory, sorted by path, for the version meta.
pub fn digest_dir(dir: &Path) -> Vec<FileDigest> {
    use sha2::Digest as _;
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml")
                && let Ok(bytes) = std::fs::read(&p)
            {
                let d: [u8; 32] = sha2::Sha256::digest(&bytes).into();
                out.push(FileDigest { path: p.display().to_string(), sha256: d.iter().map(|b| format!("{b:02x}")).collect() });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}
