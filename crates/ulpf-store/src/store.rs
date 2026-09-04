//! Append-only raw event store.
//!
//! Layout inside the store directory:
//! - `raw.seg`  — 8-byte file magic, then records: `ULPF` u32 magic, id u64, receipt ns i64,
//!   source u32, len u32, sha256 [32], then `len` bytes. Little-endian.
//! - `raw.idx`  — u64 segment offset per record; the record id is its position.
//! - `catalog.sqlite` — sources, ingests (one row per file/stream), runs (one row per CLI
//!   run with its counter report). Never per event: the offsets file is the event index.
//!
//! Crash recovery: the index is authoritative. On open, any segment bytes after the last
//! indexed record are unindexed and were never handed out as a `RawId`; the writer resumes
//! at the end of the last indexed record. A partial trailing index entry is ignored.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
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
        let next_id = idx.metadata()?.len() / 8;
        let seg_len = if next_id == 0 {
            FILE_MAGIC.len() as u64
        } else {
            idx.seek(SeekFrom::Start((next_id - 1) * 8))?;
            let mut off = [0u8; 8];
            idx.read_exact(&mut off)?;
            let off = u64::from_le_bytes(off);
            seg.seek(SeekFrom::Start(off))?;
            let mut hdr = [0u8; HEADER_LEN];
            seg.read_exact(&mut hdr)?;
            let len = u32::from_le_bytes(hdr[24..28].try_into().unwrap()) as u64;
            off + HEADER_LEN as u64 + len
        };
        seg.seek(SeekFrom::Start(seg_len))?;
        idx.seek(SeekFrom::Start(next_id * 8))?;
        let catalog = open_catalog(&dir.join("catalog.sqlite"))?;
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

fn open_catalog(path: &Path) -> io::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path).map_err(sql_err)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS sources (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS ingests (
            id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL REFERENCES sources(id),
            first_raw_id INTEGER, event_count INTEGER NOT NULL, byte_count INTEGER NOT NULL,
            started_nanos INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY, started_nanos INTEGER NOT NULL, finished_nanos INTEGER NOT NULL,
            report TEXT NOT NULL);",
    )
    .map_err(sql_err)?;
    Ok(conn)
}

fn sql_err(e: rusqlite::Error) -> io::Error {
    io::Error::other(format!("catalog: {e}"))
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
        let mut stmt = conn.prepare("SELECT id, name FROM sources").map_err(sql_err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql_err)?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(sql_err)?;
            out.insert(id, name);
        }
        Ok(out)
    }
}
