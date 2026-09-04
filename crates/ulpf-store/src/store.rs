//! Append-only raw event store.
//!
//! Layout inside the store directory:
//! - `raw.seg`  — 8-byte file magic, then records: `ULPF` u32 magic, id u64, receipt ns i64,
//!   source u32, len u32, sha256 [32], then `len` bytes. Little-endian.
//! - `raw.idx`  — u64 segment offset per record; the record id is its position.
//! - `catalog.sqlite` — sources, ingests (one row per file/stream), runs (one row per CLI
//!   run with its counter report). Never per event: the offsets file is the event index.
//!
//! Crash recovery (`recover`): the index is authoritative for every entry whose record is
//! fully present in the segment. Trailing index entries that point past the segment (the
//! index buffer drained before the segment buffer) are dropped; complete records the
//! segment holds beyond the last index entry (the segment drained first) are indexed
//! again, so an id that was handed out is never reissued; both files are cut to the
//! recovered end. The engine flushes both buffers before it lets an id escape into the
//! output, so only power loss can reach the reindexing path.
//!
//! One writer at a time: the catalogue connection is opened in SQLite's exclusive locking
//! mode and holds the file lock until it closes; a second writer, or a reader of the
//! catalogue, gets "store is in use". The OS releases the lock if the process dies, so
//! there is no lock file to go stale.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use sha2::{Digest, Sha256};

const FILE_MAGIC: &[u8; 8] = b"ULPFSEG\x01";
const REC_MAGIC: u32 = 0x4650_4C55; // "ULPF" little-endian
const HEADER_LEN: usize = 4 + 8 + 8 + 4 + 4 + 32;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RawId(pub u64);

pub struct RawRecord<'a> {
    pub id: RawId,
    pub receipt_nanos: i64,
    pub source: u32,
    pub sha256: [u8; 32],
    pub bytes: &'a [u8],
}

/// A record read back through the writer (the server's traceback path): the bytes are
/// copied out so the store lock is held only for the read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRecord {
    pub id: RawId,
    pub receipt_nanos: i64,
    pub source: u32,
    pub sha256: [u8; 32],
    pub bytes: Vec<u8>,
}

pub struct RawStore {
    seg: BufWriter<File>,
    idx: BufWriter<File>,
    next_id: u64,
    seg_len: u64,
    catalog: rusqlite::Connection,
    sources: HashMap<String, u32>,
}

impl RawStore {
    /// Opens (creating if needed) the store in `dir`. Only append and read exist beyond this.
    pub fn open(dir: &Path) -> io::Result<RawStore> {
        std::fs::create_dir_all(dir)?;
        // Taken first: the writer lock lives on this connection (see the module doc).
        let catalog = open_catalog(&dir.join("catalog.sqlite"), true).map_err(|e| in_use(dir, e))?;
        let seg_path = dir.join("raw.seg");
        let idx_path = dir.join("raw.idx");
        let mut seg = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&seg_path)?;
        let mut idx = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&idx_path)?;
        if seg.metadata()?.len() == 0 {
            seg.write_all(FILE_MAGIC)?;
        } else {
            let mut magic = [0u8; 8];
            seg.read_exact(&mut magic)?;
            if &magic != FILE_MAGIC {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "raw.seg: bad file magic"));
            }
        }
        let (next_id, seg_len) = recover(&mut seg, &mut idx)?;
        seg.seek(SeekFrom::Start(seg_len))?;
        idx.seek(SeekFrom::Start(next_id * 8))?;
        Ok(RawStore {
            seg: BufWriter::with_capacity(1 << 20, seg),
            idx: BufWriter::with_capacity(1 << 16, idx),
            next_id,
            seg_len,
            catalog,
            sources: HashMap::new(),
        })
    }

    /// Number of records ever appended; also the next id.
    pub fn len(&self) -> u64 {
        self.next_id
    }

    pub fn is_empty(&self) -> bool {
        self.next_id == 0
    }

    /// Returns the stable id for a source name, creating it on first sight.
    pub fn source_id(&mut self, name: &str) -> io::Result<u32> {
        if let Some(id) = self.sources.get(name) {
            return Ok(*id);
        }
        self.catalog
            .execute("INSERT OR IGNORE INTO sources(name) VALUES (?1)", [name])
            .map_err(sql_err)?;
        let id: u32 = self
            .catalog
            .query_row("SELECT id FROM sources WHERE name = ?1", [name], |r| r.get(0))
            .map_err(sql_err)?;
        self.sources.insert(name.to_owned(), id);
        Ok(id)
    }

    /// Appends one event. The digest is computed from `bytes` as given (a slice of the
    /// memory-mapped input on the hot path). Returns the permanent id.
    pub fn append(&mut self, source: u32, receipt_nanos: i64, bytes: &[u8]) -> io::Result<RawId> {
        let len = u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "event exceeds 4 GiB"))?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let id = self.next_id;
        let mut hdr = [0u8; HEADER_LEN];
        hdr[0..4].copy_from_slice(&REC_MAGIC.to_le_bytes());
        hdr[4..12].copy_from_slice(&id.to_le_bytes());
        hdr[12..20].copy_from_slice(&receipt_nanos.to_le_bytes());
        hdr[20..24].copy_from_slice(&source.to_le_bytes());
        hdr[24..28].copy_from_slice(&len.to_le_bytes());
        hdr[28..60].copy_from_slice(&digest);
        self.seg.write_all(&hdr)?;
        self.seg.write_all(bytes)?;
        self.idx.write_all(&self.seg_len.to_le_bytes())?;
        self.seg_len += HEADER_LEN as u64 + bytes.len() as u64;
        self.next_id += 1;
        Ok(RawId(id))
    }

    /// Reads one record back through the writer. `None` when the id was never issued.
    /// Flushes first so an id that escaped into the output is always readable; the read
    /// is positional and never moves the append cursor.
    pub fn get(&mut self, id: RawId) -> io::Result<Option<OwnedRecord>> {
        if id.0 >= self.next_id {
            return Ok(None);
        }
        self.flush(false)?;
        let mut off = [0u8; 8];
        self.idx.get_ref().read_exact_at(&mut off, id.0 * 8)?;
        let off = u64::from_le_bytes(off);
        let mut hdr = [0u8; HEADER_LEN];
        self.seg.get_ref().read_exact_at(&mut hdr, off)?;
        if u32::from_le_bytes(hdr[0..4].try_into().expect("4 bytes")) != REC_MAGIC || u64::from_le_bytes(hdr[4..12].try_into().expect("8 bytes")) != id.0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("raw id {} points at a damaged record", id.0)));
        }
        let len = u32::from_le_bytes(hdr[24..28].try_into().expect("4 bytes")) as usize;
        if off + HEADER_LEN as u64 + len as u64 > self.seg_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("raw id {} claims {len} bytes past the end of the segment", id.0)));
        }
        let mut bytes = vec![0u8; len];
        self.seg.get_ref().read_exact_at(&mut bytes, off + HEADER_LEN as u64)?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&hdr[28..60]);
        Ok(Some(OwnedRecord {
            id,
            receipt_nanos: i64::from_le_bytes(hdr[12..20].try_into().expect("8 bytes")),
            source: u32::from_le_bytes(hdr[20..24].try_into().expect("4 bytes")),
            sha256,
            bytes,
        }))
    }

    /// Source names by id, through the writer's own catalogue connection.
    pub fn source_names(&self) -> io::Result<HashMap<u32, String>> {
        let mut stmt = self.catalog.prepare("SELECT id, name FROM sources").map_err(sql_err)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))).map_err(sql_err)?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(sql_err)?;
            out.insert(id, name);
        }
        Ok(out)
    }

    /// Bytes ever ingested per source name, summed over the catalogue's ingest rows: the
    /// offset a restarted tailer resumes from.
    pub fn ingested_bytes(&self) -> io::Result<HashMap<String, u64>> {
        let mut stmt = self
            .catalog
            .prepare("SELECT s.name, SUM(i.byte_count) FROM ingests i JOIN sources s ON s.id = i.source_id GROUP BY s.name")
            .map_err(sql_err)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(sql_err)?;
        let mut out = HashMap::new();
        for row in rows {
            let (name, bytes) = row.map_err(sql_err)?;
            out.insert(name, bytes.max(0) as u64);
        }
        Ok(out)
    }

    /// Flushes buffers to the OS. `durable` additionally fsyncs both files.
    pub fn flush(&mut self, durable: bool) -> io::Result<()> {
        self.seg.flush()?;
        self.idx.flush()?;
        if durable {
            self.seg.get_ref().sync_data()?;
            self.idx.get_ref().sync_data()?;
        }
        Ok(())
    }

    /// Records one ingested file or stream in the catalogue.
    pub fn record_ingest(&mut self, source: u32, first: Option<RawId>, count: u64, bytes: u64, started_nanos: i64) -> io::Result<()> {
        self.catalog
            .execute(
                "INSERT INTO ingests(source_id, first_raw_id, event_count, byte_count, started_nanos) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![source, first.map(|r| r.0 as i64), count as i64, bytes as i64, started_nanos],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    /// Records one CLI run and its counter report (JSON text) in the catalogue.
    pub fn record_run(&mut self, started_nanos: i64, finished_nanos: i64, report_json: &str) -> io::Result<()> {
        self.catalog
            .execute(
                "INSERT INTO runs(started_nanos, finished_nanos, report) VALUES (?1, ?2, ?3)",
                rusqlite::params![started_nanos, finished_nanos, report_json],
            )
            .map_err(sql_err)?;
        Ok(())
    }
}

impl Drop for RawStore {
    fn drop(&mut self) {
        let _ = self.flush(false);
    }
}

/// Returns `(record count, segment end)` after cutting both files to the last state that
/// is consistent in both directions (module doc).
fn recover(seg: &mut File, idx: &mut File) -> io::Result<(u64, u64)> {
    let seg_file_len = seg.metadata()?.len();
    let mut n = idx.metadata()?.len() / 8;
    let mut seg_len = FILE_MAGIC.len() as u64;
    while n > 0 {
        idx.seek(SeekFrom::Start((n - 1) * 8))?;
        let mut off = [0u8; 8];
        idx.read_exact(&mut off)?;
        if let Some(end) = record_end(seg, u64::from_le_bytes(off), n - 1, seg_file_len, false)? {
            seg_len = end;
            break;
        }
        n -= 1;
    }
    while let Some(end) = record_end(seg, seg_len, n, seg_file_len, true)? {
        idx.seek(SeekFrom::Start(n * 8))?;
        idx.write_all(&seg_len.to_le_bytes())?;
        n += 1;
        seg_len = end;
    }
    idx.set_len(n * 8)?;
    seg.set_len(seg_len)?;
    Ok((n, seg_len))
}

/// End offset of the record at `off` if a complete record carrying `id` is there. With
/// `check_digest` the bytes must also hash to the stored digest (an unindexed trailing
/// record may be torn in the middle of its bytes).
fn record_end(seg: &mut File, off: u64, id: u64, seg_file_len: u64, check_digest: bool) -> io::Result<Option<u64>> {
    if off < FILE_MAGIC.len() as u64 || off + HEADER_LEN as u64 > seg_file_len {
        return Ok(None);
    }
    seg.seek(SeekFrom::Start(off))?;
    let mut hdr = [0u8; HEADER_LEN];
    seg.read_exact(&mut hdr)?;
    if u32::from_le_bytes(hdr[0..4].try_into().unwrap()) != REC_MAGIC || u64::from_le_bytes(hdr[4..12].try_into().unwrap()) != id {
        return Ok(None);
    }
    let len = u32::from_le_bytes(hdr[24..28].try_into().unwrap()) as u64;
    let end = off + HEADER_LEN as u64 + len;
    if end > seg_file_len {
        return Ok(None);
    }
    if check_digest {
        let mut bytes = vec![0u8; len as usize];
        seg.read_exact(&mut bytes)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if digest != hdr[28..60] {
            return Ok(None);
        }
    }
    Ok(Some(end))
}

/// `writer` opens in exclusive locking mode and takes the lock with a first write, so it
/// is held until the connection closes.
fn open_catalog(path: &Path, writer: bool) -> io::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path).map_err(sql_err)?;
    conn.busy_timeout(std::time::Duration::ZERO).map_err(sql_err)?;
    if writer {
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;").map_err(sql_err)?;
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS sources (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS ingests (
            id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL REFERENCES sources(id),
            first_raw_id INTEGER, event_count INTEGER NOT NULL, byte_count INTEGER NOT NULL,
            started_nanos INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY, started_nanos INTEGER NOT NULL, finished_nanos INTEGER NOT NULL,
            report TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS writer (id INTEGER PRIMARY KEY, pid INTEGER NOT NULL);",
    )
    .map_err(sql_err)?;
    if writer {
        conn.execute("INSERT OR REPLACE INTO writer(id, pid) VALUES (1, ?1)", [std::process::id() as i64]).map_err(sql_err)?;
    }
    Ok(conn)
}

fn sql_err(e: rusqlite::Error) -> io::Error {
    io::Error::other(format!("catalog: {e}"))
}

fn in_use(dir: &Path, e: io::Error) -> io::Error {
    let text = e.to_string();
    if text.contains("locked") || text.contains("busy") {
        io::Error::other(format!("store {} is in use by another process", dir.display()))
    } else {
        e
    }
}

/// Read-only view of a store. Maps the segment and index as they were at open time.
pub struct RawReader {
    seg: Mmap,
    idx: Mmap,
    count: u64,
    dir: PathBuf,
}

pub struct VerifyReport {
    pub checked: u64,
    pub corrupt: Vec<RawId>,
}

impl RawReader {
    pub fn open(dir: &Path) -> io::Result<RawReader> {
        let seg_file = File::open(dir.join("raw.seg"))?;
        let idx_file = File::open(dir.join("raw.idx"))?;
        // SAFETY: the files are only ever appended to; bytes below the mapped length are
        // never modified by any code path in this crate.
        let seg = unsafe { Mmap::map(&seg_file)? };
        let idx = unsafe { Mmap::map(&idx_file)? };
        if seg.len() < FILE_MAGIC.len() || &seg[..FILE_MAGIC.len()] != FILE_MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "raw.seg: bad file magic"));
        }
        let count = (idx.len() / 8) as u64;
        Ok(RawReader { seg, idx, count, dir: dir.to_path_buf() })
    }

    pub fn len(&self) -> u64 {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The record with this id, or `None` if the id was never issued or the record is
    /// structurally damaged (bad magic, length past end of segment).
    pub fn get(&self, id: RawId) -> Option<RawRecord<'_>> {
        if id.0 >= self.count {
            return None;
        }
        let p = (id.0 * 8) as usize;
        let off = u64::from_le_bytes(self.idx[p..p + 8].try_into().ok()?) as usize;
        let hdr = self.seg.get(off..off + HEADER_LEN)?;
        if u32::from_le_bytes(hdr[0..4].try_into().ok()?) != REC_MAGIC {
            return None;
        }
        let rec_id = u64::from_le_bytes(hdr[4..12].try_into().ok()?);
        let receipt_nanos = i64::from_le_bytes(hdr[12..20].try_into().ok()?);
        let source = u32::from_le_bytes(hdr[20..24].try_into().ok()?);
        let len = u32::from_le_bytes(hdr[24..28].try_into().ok()?) as usize;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&hdr[28..60]);
        let bytes = self.seg.get(off + HEADER_LEN..off + HEADER_LEN + len)?;
        if rec_id != id.0 {
            return None;
        }
        Some(RawRecord { id, receipt_nanos, source, sha256, bytes })
    }

    pub fn iter(&self) -> impl Iterator<Item = Option<RawRecord<'_>>> + '_ {
        (0..self.count).map(move |i| self.get(RawId(i)))
    }

    /// Recomputes every digest. Records that are unreadable or whose bytes no longer hash
    /// to the stored digest are listed as corrupt.
    pub fn verify(&self) -> VerifyReport {
        let mut corrupt = Vec::new();
        for i in 0..self.count {
            match self.get(RawId(i)) {
                Some(rec) => {
                    let d: [u8; 32] = Sha256::digest(rec.bytes).into();
                    if d != rec.sha256 {
                        corrupt.push(RawId(i));
                    }
                }
                None => corrupt.push(RawId(i)),
            }
        }
        VerifyReport { checked: self.count, corrupt }
    }

    /// Source names by id, from the catalogue.
    pub fn source_names(&self) -> io::Result<HashMap<u32, String>> {
        let conn = rusqlite::Connection::open_with_flags(
            self.dir.join("catalog.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(sql_err)?;
        conn.busy_timeout(std::time::Duration::ZERO).map_err(sql_err)?;
        let mut stmt = conn.prepare("SELECT id, name FROM sources").map_err(sql_err).map_err(|e| in_use(&self.dir, e))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql_err)
            .map_err(|e| in_use(&self.dir, e))?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(sql_err)?;
            out.insert(id, name);
        }
        Ok(out)
    }
}
