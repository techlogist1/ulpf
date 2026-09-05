//! The entity index beside the output file (`<output>.pivot`).
//!
//! Derived data: every row can be rebuilt from the JSON Lines output (`rebuild`), so the
//! writer runs with `synchronous = OFF` on its own thread and a lost tail costs a rebuild,
//! never an event. The engine's output thread offers `Posting`s per batch over a bounded
//! channel and blocks when the writer falls behind; nothing is dropped.
//!
//! Never a row per event (D5): one row per `(kind, value, batch)` holds a packed posting
//! list, so a batch of 1024 events costs a handful of inserts instead of a thousand. The
//! index speaks only in `EntityKind`, so no vendor field name reaches it: the mapping's
//! `[entities]` table is the only place the schema paths are named.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use rusqlite::Connection;
use serde::Serialize;
use ulpf_normalize::{EntityKind, Mapping};

/// One (event, entity) pair. Borrowed: the writer copies what it keeps.
pub struct Posting<'a> {
    pub raw_id: u64,
    pub time_ms: i64,
    pub kind: EntityKind,
    pub value: &'a [u8],
    pub device: &'a [u8],
    pub parser: Option<&'a str>,
    /// Byte offset of the emitted line in the output file, and its length without the newline.
    pub offset: u64,
    pub len: u32,
}

/// Packed posting entry: raw_id u64, time_ms i64, offset u64, len u32, device u32, parser u16.
const ENTRY: usize = 8 + 8 + 8 + 4 + 4 + 2;
const NO_PARSER: u16 = u16::MAX;
/// Newest events of an entity that `related` co-occurrence is computed over.
const RELATED_WINDOW: usize = 10_000;
/// Posting rows one `related` scan may read per other kind before it stops and says so.
const RELATED_ROW_BUDGET: usize = 50_000;
/// Posting rows one timeline page may read before it answers with what it has.
const PAGE_ROW_BUDGET: usize = 20_000;
/// Batches the writer joins into one transaction when the producer is ahead of it.
const COMMIT_BATCHES: usize = 8;

pub fn index_path(output: &Path) -> PathBuf {
    let mut s = output.as_os_str().to_os_string();
    s.push(".pivot");
    PathBuf::from(s)
}

#[derive(Debug, Default, Serialize)]
pub struct PivotCounters {
    pub batches: AtomicU64,
    pub postings: AtomicU64,
    /// Times `push_batch` found the writer's queue full and blocked.
    pub blocked: AtomicU64,
    pub errors: AtomicU64,
}

/// One batch on its way to the writer thread: two allocations, whatever the batch size.
#[derive(Default)]
struct BatchBuf {
    recs: Vec<Rec>,
    bytes: Vec<u8>,
}

struct Rec {
    raw_id: u64,
    time_ms: i64,
    kind: u8,
    offset: u64,
    len: u32,
    value: (u32, u32),
    device: (u32, u32),
    /// Empty range means no parser.
    parser: (u32, u32),
}

impl BatchBuf {
    fn push(&mut self, p: &Posting<'_>) {
        let value = self.put(p.value);
        let device = self.put(p.device);
        let parser = self.put(p.parser.unwrap_or_default().as_bytes());
        self.recs.push(Rec { raw_id: p.raw_id, time_ms: p.time_ms, kind: p.kind as u8, offset: p.offset, len: p.len, value, device, parser });
    }

    fn put(&mut self, b: &[u8]) -> (u32, u32) {
        let at = self.bytes.len() as u32;
        self.bytes.extend_from_slice(b);
        (at, b.len() as u32)
    }

    fn slice(&self, r: (u32, u32)) -> &[u8] {
        &self.bytes[r.0 as usize..r.0 as usize + r.1 as usize]
    }
}

/// The engine's handle. Owns the writer thread; `finish` (or drop) drains and joins it.
pub struct PivotWriter {
    tx: Option<SyncSender<BatchBuf>>,
    thread: Option<std::thread::JoinHandle<()>>,
    counters: Arc<PivotCounters>,
}

impl PivotWriter {
    /// Opens (creating if missing) `<output>.pivot` and starts the writer thread.
    pub fn start(output: &Path, queue: usize) -> Result<PivotWriter> {
        let path = index_path(output);
        let conn = open_writer(&path)?;
        let (tx, rx) = sync_channel::<BatchBuf>(queue.max(1));
        let counters = Arc::new(PivotCounters::default());
        let c = Arc::clone(&counters);
        let thread = std::thread::Builder::new()
            .name("pivot".into())
            .spawn(move || {
                let mut w = Writer { conn, devices: HashMap::new(), parsers: HashMap::new(), max_span: 0 };
                let mut drained: Vec<BatchBuf> = Vec::new();
                while let Ok(batch) = rx.recv() {
                    // one transaction per *group* of batches: whatever is already queued
                    // joins this one, so a fast producer costs fewer commits and fewer
                    // entity upserts (the groups merge), never a lost posting
                    drained.clear();
                    drained.push(batch);
                    while drained.len() < COMMIT_BATCHES {
                        match rx.try_recv() {
                            Ok(b) => drained.push(b),
                            Err(_) => break,
                        }
                    }
                    match w.write(&drained) {
                        Ok(n) => {
                            c.batches.fetch_add(drained.len() as u64, Relaxed);
                            c.postings.fetch_add(n, Relaxed);
                        }
                        Err(_) => {
                            c.errors.fetch_add(1, Relaxed);
                        }
                    }
                }
            })
            .context("spawning the pivot writer thread")?;
        Ok(PivotWriter { tx: Some(tx), thread: Some(thread), counters })
    }

    /// Hands one batch's postings to the writer thread. Blocks while the queue is full
    /// (counted); a dead writer thread is counted, never a panic.
    pub fn push_batch(&mut self, entries: &[Posting<'_>]) {
        if entries.is_empty() {
            return;
        }
        let Some(tx) = &self.tx else { return };
        let mut buf = BatchBuf { recs: Vec::with_capacity(entries.len()), bytes: Vec::with_capacity(entries.len() * 32) };
        for p in entries {
            buf.push(p);
        }
        match tx.try_send(buf) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(buf)) => {
                self.counters.blocked.fetch_add(1, Relaxed);
                if tx.send(buf).is_err() {
                    self.counters.errors.fetch_add(1, Relaxed);
                }
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                self.counters.errors.fetch_add(1, Relaxed);
            }
        }
    }

    pub fn counters(&self) -> Arc<PivotCounters> {
        Arc::clone(&self.counters)
    }

    /// Drains the queue and joins the writer thread.
    pub fn finish(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for PivotWriter {
    fn drop(&mut self) {
        self.stop();
    }
}

fn open_writer(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("opening pivot index {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = OFF;
         CREATE TABLE IF NOT EXISTS postings (kind INTEGER NOT NULL, value BLOB NOT NULL,
            first_id INTEGER NOT NULL, last_id INTEGER NOT NULL, n INTEGER NOT NULL, blob BLOB NOT NULL);
         CREATE INDEX IF NOT EXISTS postings_kv ON postings(kind, value, first_id);
         CREATE INDEX IF NOT EXISTS postings_k ON postings(kind, first_id);
         CREATE TABLE IF NOT EXISTS devices (id INTEGER PRIMARY KEY, name BLOB NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS parsers (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS entities (kind INTEGER NOT NULL, value BLOB NOT NULL, events INTEGER NOT NULL,
            first_time INTEGER NOT NULL, last_time INTEGER NOT NULL, PRIMARY KEY (kind, value)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS entity_devices (kind INTEGER NOT NULL, value BLOB NOT NULL, device_id INTEGER NOT NULL,
            parser_id INTEGER NOT NULL, events INTEGER NOT NULL, PRIMARY KEY (kind, value, device_id, parser_id)) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v INTEGER NOT NULL);",
    )
    .context("creating the pivot index schema")?;
    Ok(conn)
}

struct Writer {
    conn: Connection,
    devices: HashMap<Vec<u8>, i64>,
    parsers: HashMap<String, i64>,
    max_span: u64,
}

/// One `(kind, value)` group inside one batch.
#[derive(Default)]
struct Group {
    blob: Vec<u8>,
    n: u64,
    first_id: u64,
    last_id: u64,
    first_time: i64,
    last_time: i64,
    /// (device id, parser id) → events
    devices: HashMap<(i64, i64), u64>,
}

impl Writer {
    fn write(&mut self, batches: &[BatchBuf]) -> rusqlite::Result<u64> {
        let mut groups: HashMap<(u8, &[u8]), Group> = HashMap::new();
        let mut ids: Vec<(u32, i64)> = Vec::new(); // (device id, parser id or -1) per rec
        for batch in batches {
            for r in &batch.recs {
                let device = self.device_id(batch.slice(r.device))?;
                let parser = match r.parser.1 {
                    0 => None,
                    _ => Some(self.parser_id(&String::from_utf8_lossy(batch.slice(r.parser)))?),
                };
                ids.push((device as u32, parser.unwrap_or(-1)));
            }
        }
        let mut at = 0usize;
        for batch in batches {
            for r in &batch.recs {
                let (device, parser) = ids[at];
                at += 1;
                let g = groups.entry((r.kind, batch.slice(r.value))).or_default();
                if g.n == 0 {
                    g.first_id = r.raw_id;
                    g.first_time = r.time_ms;
                    g.last_time = r.time_ms;
                }
                g.n += 1;
                g.last_id = g.last_id.max(r.raw_id);
                g.first_id = g.first_id.min(r.raw_id);
                g.first_time = g.first_time.min(r.time_ms);
                g.last_time = g.last_time.max(r.time_ms);
                let parser_slot = if parser < 0 { NO_PARSER } else { parser as u16 };
                g.blob.extend_from_slice(&r.raw_id.to_le_bytes());
                g.blob.extend_from_slice(&r.time_ms.to_le_bytes());
                g.blob.extend_from_slice(&r.offset.to_le_bytes());
                g.blob.extend_from_slice(&r.len.to_le_bytes());
                g.blob.extend_from_slice(&device.to_le_bytes());
                g.blob.extend_from_slice(&parser_slot.to_le_bytes());
                *g.devices.entry((device as i64, parser)).or_default() += 1;
                self.max_span = self.max_span.max(g.last_id - g.first_id);
            }
        }
        let total = at as u64;
        let span = self.max_span;
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut post = tx.prepare_cached("INSERT INTO postings (kind, value, first_id, last_id, n, blob) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
            let mut ent = tx.prepare_cached(
                "INSERT INTO entities (kind, value, events, first_time, last_time) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, value) DO UPDATE SET events = events + excluded.events,
                   first_time = min(first_time, excluded.first_time), last_time = max(last_time, excluded.last_time)",
            )?;
            let mut dev = tx.prepare_cached(
                "INSERT INTO entity_devices (kind, value, device_id, parser_id, events) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, value, device_id, parser_id) DO UPDATE SET events = events + excluded.events",
            )?;
            for ((kind, value), g) in &groups {
                post.execute(rusqlite::params![kind, value, g.first_id as i64, g.last_id as i64, g.n as i64, &g.blob])?;
                ent.execute(rusqlite::params![kind, value, g.n as i64, g.first_time, g.last_time])?;
                for ((d, p), n) in &g.devices {
                    dev.execute(rusqlite::params![kind, value, d, p, *n as i64])?;
                }
            }
            tx.prepare_cached("INSERT INTO meta (k, v) VALUES ('max_span', ?1) ON CONFLICT(k) DO UPDATE SET v = max(v, excluded.v)")?
                .execute([span as i64])?;
        }
        tx.commit()?;
        Ok(total)
    }

    fn device_id(&mut self, name: &[u8]) -> rusqlite::Result<i64> {
        if let Some(id) = self.devices.get(name) {
            return Ok(*id);
        }
        self.conn.prepare_cached("INSERT OR IGNORE INTO devices (name) VALUES (?1)")?.execute([name])?;
        let id: i64 = self.conn.prepare_cached("SELECT id FROM devices WHERE name = ?1")?.query_row([name], |r| r.get(0))?;
        self.devices.insert(name.to_vec(), id);
        Ok(id)
    }

    fn parser_id(&mut self, name: &str) -> rusqlite::Result<i64> {
        if let Some(id) = self.parsers.get(name) {
            return Ok(*id);
        }
        self.conn.prepare_cached("INSERT OR IGNORE INTO parsers (name) VALUES (?1)")?.execute([name])?;
        let id: i64 = self.conn.prepare_cached("SELECT id FROM parsers WHERE name = ?1")?.query_row([name], |r| r.get(0))?;
        self.parsers.insert(name.to_owned(), id);
        Ok(id)
    }
}

// ------------------------------------------------------------------ reading

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    Desc,
    Asc,
}

pub struct PivotQuery<'a> {
    pub kind: EntityKind,
    pub value: &'a [u8],
    pub limit: usize,
    /// Newest-first paging: only events strictly older than this device time (ms).
    pub before: Option<i64>,
    /// Oldest-first paging: only events strictly newer than this device time (ms).
    pub after: Option<i64>,
    pub order: Order,
}

#[derive(Debug, Clone, Serialize)]
pub struct PivotPage {
    pub kind: &'static str,
    pub value: String,
    pub total: u64,
    pub first_time: Option<i64>,
    pub last_time: Option<i64>,
    pub devices: Vec<DeviceCount>,
    /// kind name → the ten most frequent co-occurring values.
    pub related: std::collections::BTreeMap<String, Vec<RelatedValue>>,
    /// How many of the entity's newest events `related` was computed over.
    pub related_over: u64,
    pub events: Vec<PivotEvent>,
    pub next_before: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceCount {
    pub device: String,
    pub events: u64,
    pub parsers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedValue {
    pub value: String,
    pub events: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PivotEvent {
    pub raw_id: u64,
    pub time: i64,
    pub device: String,
    pub parser: Option<String>,
    pub line: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntitySummary {
    pub kind: &'static str,
    pub value: String,
    pub events: u64,
    pub devices: u64,
    pub first_time: i64,
    pub last_time: i64,
}

/// One decoded posting entry.
#[derive(Debug, Clone, Copy)]
struct Entry {
    raw_id: u64,
    time_ms: i64,
    offset: u64,
    len: u32,
    device: u32,
    parser: u16,
}

fn decode(blob: &[u8], mut f: impl FnMut(Entry)) {
    for c in blob.chunks_exact(ENTRY) {
        f(Entry {
            raw_id: u64::from_le_bytes(c[0..8].try_into().unwrap_or_default()),
            time_ms: i64::from_le_bytes(c[8..16].try_into().unwrap_or_default()),
            offset: u64::from_le_bytes(c[16..24].try_into().unwrap_or_default()),
            len: u32::from_le_bytes(c[24..28].try_into().unwrap_or_default()),
            device: u32::from_le_bytes(c[28..32].try_into().unwrap_or_default()),
            parser: u16::from_le_bytes(c[32..34].try_into().unwrap_or_default()),
        });
    }
}

/// Read side. Opens the index read-only beside the output and reads line text from the
/// output file by offset; WAL lets it run while the writer thread appends.
pub struct PivotIndex {
    conn: Connection,
    output: PathBuf,
    file: Mutex<Option<std::fs::File>>,
}

impl PivotIndex {
    /// `output` is the JSON Lines path; the index is `<output>.pivot` beside it.
    pub fn open(output: &Path) -> Result<PivotIndex> {
        let path = index_path(output);
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI)
            .with_context(|| format!("opening pivot index {}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_millis(2000)).ok();
        Ok(PivotIndex { conn, output: output.to_path_buf(), file: Mutex::new(None) })
    }

    pub fn query(&self, q: &PivotQuery<'_>) -> Result<PivotPage> {
        let kind = q.kind as u8;
        let (total, first_time, last_time): (u64, Option<i64>, Option<i64>) = self
            .conn
            .prepare_cached("SELECT events, first_time, last_time FROM entities WHERE kind = ?1 AND value = ?2")?
            .query_row(rusqlite::params![kind, q.value], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get(2)?)))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok((0, None, None)),
                e => Err(e),
            })?;
        let devices = self.devices(kind, q.value)?;
        let (window, page) = self.walk(q)?;
        let (related, related_over) = self.related(q.kind, &window)?;
        let dict_dev = self.dict_devices()?;
        let dict_par = self.dict_parsers()?;
        let next_before = if page.len() >= q.limit.clamp(1, 500) { page.last().map(|e| e.time_ms) } else { None };
        let events = page
            .iter()
            .map(|e| PivotEvent {
                raw_id: e.raw_id,
                time: e.time_ms,
                device: dict_dev.get(&e.device).cloned().unwrap_or_default(),
                parser: (e.parser != NO_PARSER).then(|| dict_par.get(&e.parser).cloned().unwrap_or_default()),
                line: self.line(e.offset, e.len),
            })
            .collect();
        Ok(PivotPage {
            kind: q.kind.name(),
            value: String::from_utf8_lossy(q.value).into_owned(),
            total,
            first_time,
            last_time,
            devices,
            related,
            related_over,
            events,
            next_before,
        })
    }

    /// Entities by event count, most first; `prefix` filters by value prefix.
    pub fn entities(&self, kind: Option<EntityKind>, prefix: &str, limit: usize) -> Result<Vec<EntitySummary>> {
        let mut sql = String::from(
            "SELECT kind, value, events, first_time, last_time,
                    (SELECT count(DISTINCT device_id) FROM entity_devices d WHERE d.kind = e.kind AND d.value = e.value)
             FROM entities e WHERE 1 = 1",
        );
        if kind.is_some() {
            sql.push_str(" AND kind = :kind");
        }
        if !prefix.is_empty() {
            sql.push_str(" AND value >= :lo AND value < :hi");
        }
        sql.push_str(" ORDER BY events DESC, value ASC LIMIT :limit");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<(&str, &dyn rusqlite::ToSql)> = Vec::new();
        let k = kind.map(|k| k as u8);
        if let Some(k) = &k {
            params.push((":kind", k));
        }
        let lo = prefix.as_bytes().to_vec();
        let hi = prefix_end(prefix.as_bytes());
        if !prefix.is_empty() {
            params.push((":lo", &lo));
            params.push((":hi", &hi));
        }
        let lim = limit.clamp(1, 1000) as i64;
        params.push((":limit", &lim));
        let rows = stmt.query_map(&params[..], |r| {
            Ok(EntitySummary {
                kind: EntityKind::from_index(r.get::<_, i64>(0)? as usize).map(EntityKind::name).unwrap_or("?"),
                value: String::from_utf8_lossy(&r.get::<_, Vec<u8>>(1)?).into_owned(),
                events: r.get::<_, i64>(2)? as u64,
                first_time: r.get(3)?,
                last_time: r.get(4)?,
                devices: r.get::<_, i64>(5)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn devices(&self, kind: u8, value: &[u8]) -> Result<Vec<DeviceCount>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT d.name, p.name, ed.events FROM entity_devices ed
             JOIN devices d ON d.id = ed.device_id
             LEFT JOIN parsers p ON p.id = ed.parser_id
             WHERE ed.kind = ?1 AND ed.value = ?2 ORDER BY ed.events DESC",
        )?;
        let mut out: Vec<DeviceCount> = Vec::new();
        let rows = stmt.query_map(rusqlite::params![kind, value], |r| {
            Ok((String::from_utf8_lossy(&r.get::<_, Vec<u8>>(0)?).into_owned(), r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)? as u64))
        })?;
        for row in rows {
            let (device, parser, events) = row?;
            match out.iter_mut().find(|d| d.device == device) {
                Some(d) => {
                    d.events += events;
                    if let Some(p) = parser
                        && !d.parsers.contains(&p)
                    {
                        d.parsers.push(p);
                    }
                }
                None => out.push(DeviceCount { device, events, parsers: parser.into_iter().collect() }),
            }
        }
        out.sort_by(|a, b| b.events.cmp(&a.events).then_with(|| a.device.cmp(&b.device)));
        Ok(out)
    }

    /// Walks the entity's posting rows in page order, returning (the newest window for
    /// `related`, the page itself). Bounded by `PAGE_ROW_BUDGET` rows.
    fn walk(&self, q: &PivotQuery<'_>) -> Result<(Vec<Entry>, Vec<Entry>)> {
        let desc = q.order == Order::Desc;
        let sql = if desc {
            "SELECT blob FROM postings WHERE kind = ?1 AND value = ?2 ORDER BY first_id DESC"
        } else {
            "SELECT blob FROM postings WHERE kind = ?1 AND value = ?2 ORDER BY first_id ASC"
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt.query(rusqlite::params![q.kind as u8, q.value])?;
        let limit = q.limit.clamp(1, 500);
        // ponytail: the posting list is ordered by raw id, so a page is the newest
        // candidates re-sorted by device time. A device whose clock disagrees with arrival
        // order by more than this many events would need a time-ordered index.
        let candidates = limit.saturating_mul(4).clamp(limit, 2000);
        let mut window: Vec<Entry> = Vec::new();
        let mut page: Vec<Entry> = Vec::new();
        let mut seen_rows = 0usize;
        while let Some(row) = rows.next()? {
            let blob: Vec<u8> = row.get(0)?;
            let mut batch: Vec<Entry> = Vec::new();
            decode(&blob, |e| batch.push(e));
            if desc {
                batch.reverse();
            }
            for e in batch {
                if window.len() < RELATED_WINDOW {
                    window.push(e);
                }
                let wanted = if desc { q.before.is_none_or(|b| e.time_ms < b) } else { q.after.is_none_or(|a| e.time_ms > a) };
                if wanted && page.len() < candidates {
                    page.push(e);
                }
            }
            seen_rows += 1;
            if (page.len() >= candidates && window.len() >= RELATED_WINDOW) || seen_rows >= PAGE_ROW_BUDGET {
                break;
            }
        }
        page.sort_by(|a, b| match desc {
            true => (b.time_ms, b.raw_id).cmp(&(a.time_ms, a.raw_id)),
            false => (a.time_ms, a.raw_id).cmp(&(b.time_ms, b.raw_id)),
        });
        page.truncate(limit);
        Ok((window, page))
    }

    /// Co-occurring values per other kind over `window`, by raw-id range. The scan is
    /// capped: `related_over` reports how many of the window's events it actually covered.
    fn related(&self, kind: EntityKind, window: &[Entry]) -> Result<(std::collections::BTreeMap<String, Vec<RelatedValue>>, u64)> {
        let mut out = std::collections::BTreeMap::new();
        for k in EntityKind::ALL {
            out.insert(k.name().to_owned(), Vec::new());
        }
        if window.is_empty() {
            return Ok((out, 0));
        }
        let ids: std::collections::HashSet<u64> = window.iter().map(|e| e.raw_id).collect();
        let lo = window.iter().map(|e| e.raw_id).min().unwrap_or(0);
        let hi = window.iter().map(|e| e.raw_id).max().unwrap_or(0);
        let span: i64 = self
            .conn
            .prepare_cached("SELECT v FROM meta WHERE k = 'max_span'")?
            .query_row([], |r| r.get(0))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                e => Err(e),
            })?;
        let floor = lo.saturating_sub(span.max(0) as u64);
        let mut covered_from = lo;
        for other in EntityKind::ALL {
            if other == kind {
                continue;
            }
            let mut stmt = self
                .conn
                .prepare_cached("SELECT value, first_id, blob FROM postings WHERE kind = ?1 AND first_id >= ?2 AND first_id <= ?3 ORDER BY first_id DESC LIMIT ?4")?;
            let mut rows = stmt.query(rusqlite::params![other as u8, floor as i64, hi as i64, RELATED_ROW_BUDGET as i64])?;
            let mut counts: HashMap<Vec<u8>, u64> = HashMap::new();
            let mut n_rows = 0usize;
            let mut lowest = lo;
            while let Some(row) = rows.next()? {
                let value: Vec<u8> = row.get(0)?;
                let first_id: i64 = row.get(1)?;
                let blob: Vec<u8> = row.get(2)?;
                let mut hits = 0u64;
                decode(&blob, |e| {
                    if ids.contains(&e.raw_id) {
                        hits += 1;
                    }
                });
                if hits > 0 {
                    *counts.entry(value).or_default() += hits;
                }
                lowest = first_id.max(0) as u64;
                n_rows += 1;
            }
            if n_rows >= RELATED_ROW_BUDGET {
                covered_from = covered_from.max(lowest);
            }
            let mut top: Vec<RelatedValue> =
                counts.into_iter().map(|(v, events)| RelatedValue { value: String::from_utf8_lossy(&v).into_owned(), events }).collect();
            top.sort_by(|a, b| b.events.cmp(&a.events).then_with(|| a.value.cmp(&b.value)));
            top.truncate(10);
            out.insert(other.name().to_owned(), top);
        }
        let over = window.iter().filter(|e| e.raw_id >= covered_from).count() as u64;
        Ok((out, over))
    }

    fn dict_devices(&self) -> Result<HashMap<u32, String>> {
        let mut stmt = self.conn.prepare_cached("SELECT id, name FROM devices")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)? as u32, String::from_utf8_lossy(&r.get::<_, Vec<u8>>(1)?).into_owned())))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    fn dict_parsers(&self) -> Result<HashMap<u16, String>> {
        let mut stmt = self.conn.prepare_cached("SELECT id, name FROM parsers")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)? as u16, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    /// The emitted line at `offset`, parsed. Unreadable (rotated, truncated) is `null`,
    /// never an error: the index is derived and the timeline still lists the event.
    fn line(&self, offset: u64, len: u32) -> serde_json::Value {
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = std::fs::File::open(&self.output).ok();
        }
        let Some(f) = guard.as_mut() else { return serde_json::Value::Null };
        let mut buf = vec![0u8; len as usize];
        if f.seek(SeekFrom::Start(offset)).is_err() || f.read_exact(&mut buf).is_err() {
            return serde_json::Value::Null;
        }
        serde_json::from_slice(&buf).unwrap_or(serde_json::Value::Null)
    }
}

fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut hi = prefix.to_vec();
    while let Some(last) = hi.pop() {
        if last < 0xff {
            hi.push(last + 1);
            break;
        }
    }
    hi
}

// ------------------------------------------------------------------ rebuild

#[derive(Debug, Clone, Serialize)]
pub struct RebuildReport {
    pub events: u64,
    pub postings: u64,
    pub unreadable_lines: u64,
    pub elapsed_secs: f64,
}

/// Re-derives `<output>.pivot` from the JSON Lines output, reading the entity paths from
/// the mapping. The old index file is replaced.
pub fn rebuild(output: &Path, mapping: &Mapping, batch_events: usize) -> Result<RebuildReport> {
    let started = std::time::Instant::now();
    let path = index_path(output);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("pivot-wal"));
    let _ = std::fs::remove_file(path.with_extension("pivot-shm"));
    let text = std::fs::read(output).with_context(|| format!("reading {}", output.display()))?;
    let entities = mapping.entities();
    let paths: Vec<(EntityKind, &str)> = EntityKind::ALL.into_iter().filter_map(|k| entities.path(k).map(|p| (k, p))).collect();

    let mut writer = PivotWriter::start(output, 8)?;
    let mut report = RebuildReport { events: 0, postings: 0, unreadable_lines: 0, elapsed_secs: 0.0 };
    let mut offset = 0u64;
    let mut pending: Vec<OwnedPosting> = Vec::new();
    for line in text.split_inclusive(|b| *b == b'\n') {
        let len = line.len() as u64;
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let at = offset;
        offset += len;
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(trimmed) else {
            report.unreadable_lines += 1;
            continue;
        };
        report.events += 1;
        let raw_id = v.pointer("/ulpf/raw_id").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let time_ms = v.get("time").and_then(serde_json::Value::as_i64).unwrap_or(0);
        let parser = v.pointer("/ulpf/parser").and_then(serde_json::Value::as_str).map(str::to_owned);
        let device = entities
            .path(EntityKind::Device)
            .and_then(|p| at_path(&v, p))
            .or_else(|| v.pointer("/metadata/log_name").map(text_of))
            .unwrap_or_default();
        for (kind, p) in &paths {
            let Some(value) = at_path(&v, p) else { continue };
            if value.is_empty() {
                continue;
            }
            pending.push(OwnedPosting {
                raw_id,
                time_ms,
                kind: *kind,
                value,
                device: device.clone(),
                parser: parser.clone(),
                offset: at,
                len: trimmed.len() as u32,
            });
        }
        if pending.len() >= batch_events {
            report.postings += flush(&mut writer, &pending);
            pending.clear();
        }
    }
    report.postings += flush(&mut writer, &pending);
    writer.finish();
    report.elapsed_secs = started.elapsed().as_secs_f64();
    Ok(report)
}

/// A rebuild's postings own their text (they come from the parsed line, not a mmap).
struct OwnedPosting {
    raw_id: u64,
    time_ms: i64,
    kind: EntityKind,
    value: String,
    device: String,
    parser: Option<String>,
    offset: u64,
    len: u32,
}

fn flush(writer: &mut PivotWriter, pending: &[OwnedPosting]) -> u64 {
    if pending.is_empty() {
        return 0;
    }
    let postings: Vec<Posting<'_>> = pending
        .iter()
        .map(|p| Posting {
            raw_id: p.raw_id,
            time_ms: p.time_ms,
            kind: p.kind,
            value: p.value.as_bytes(),
            device: p.device.as_bytes(),
            parser: p.parser.as_deref(),
            offset: p.offset,
            len: p.len,
        })
        .collect();
    writer.push_batch(&postings);
    postings.len() as u64
}

/// The value at a dotted schema path, as text (numbers included).
fn at_path(v: &serde_json::Value, path: &str) -> Option<String> {
    let mut cur = v;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    match cur {
        serde_json::Value::Null => None,
        other => Some(text_of(other)),
    }
}

fn text_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
