//! Append-only raw event store.
//!
//! Layout inside the store directory:
//! - `raw.seg`  — 8-byte file magic, then records: `ULPF` u32 magic, id u64, receipt ns i64,
//!   source u32, len u32, sha256 [32], then `len` bytes. Little-endian.
//! - `raw.idx`  — `ULPFIDX\x02` magic and a 16-byte store id, then one fixed 40-byte entry
//!   per record: u64 segment offset, `[32]` chain value. The record id is its position.
//! - `catalog.sqlite` — sources, ingests (one row per file/stream), runs (one row per CLI
//!   run with its counter report). Never per event: the offsets file is the event index.
//!
//! Integrity chain: `chain_i = SHA-256(chain_{i-1} || sha256_i)`, `chain_{-1} = genesis =
//! SHA-256("ULPF chain genesis" || store id)`. Changing any byte of any record, its stored
//! digest included, changes every chain value from that record on, so an attestation taken
//! earlier (`attest`) names the first record a rewrite touched even when the rewrite was
//! internally consistent. An index without the magic is a pre-chain store and is refused.
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
const IDX_MAGIC: &[u8; 8] = b"ULPFIDX\x02";
const IDX_HEADER_LEN: u64 = 8 + 16; // magic + store id
const IDX_ENTRY_LEN: u64 = 8 + 32; // offset + chain
/// One checkpoint per this many records in an attestation (plus the last record).
pub const CHECKPOINT_EVERY: u64 = 4096;

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
    dir: PathBuf,
    store_id: [u8; 16],
    genesis: [u8; 32],
    head: [u8; 32],
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
        let store_id = idx_header(&mut idx, dir, seg.metadata()?.len())?;
        let genesis = genesis_of(&store_id);
        let (next_id, seg_len, head) = recover(&mut seg, &mut idx, genesis)?;
        seg.seek(SeekFrom::Start(seg_len))?;
        idx.seek(SeekFrom::Start(IDX_HEADER_LEN + next_id * IDX_ENTRY_LEN))?;
        Ok(RawStore {
            seg: BufWriter::with_capacity(1 << 20, seg),
            idx: BufWriter::with_capacity(1 << 16, idx),
            next_id,
            seg_len,
            catalog,
            sources: HashMap::new(),
            dir: dir.to_path_buf(),
            store_id,
            genesis,
            head,
        })
    }

    /// The store's identity: 16 random bytes written when the store was created.
    pub fn store_id(&self) -> [u8; 16] {
        self.store_id
    }

    /// `SHA-256("ULPF chain genesis" || store id)`: the chain value before record 0.
    pub fn genesis(&self) -> [u8; 32] {
        self.genesis
    }

    /// Chain value of the last record; `None` for an empty store.
    pub fn head(&self) -> Option<[u8; 32]> {
        (self.next_id > 0).then_some(self.head)
    }

    /// Chain value stored for one record. `None` when the id was never issued.
    pub fn chain(&mut self, id: RawId) -> io::Result<Option<[u8; 32]>> {
        if id.0 >= self.next_id {
            return Ok(None);
        }
        self.flush(false)?;
        let mut chain = [0u8; 32];
        self.idx.get_ref().read_exact_at(&mut chain, IDX_HEADER_LEN + id.0 * IDX_ENTRY_LEN + 8)?;
        Ok(Some(chain))
    }

    /// A read-only snapshot of the records written so far, through the writer's own files
    /// (D42): flushes, then maps the store bounded to the current record count. Records
    /// appended after this call are invisible to the reader; nothing is written.
    pub fn reader(&mut self) -> io::Result<RawReader> {
        self.flush(false)?;
        let mut reader = RawReader::open(&self.dir)?;
        reader.count = reader.count.min(self.next_id);
        Ok(reader)
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
    /// memory-mapped input on the hot path); the chain value costs one more SHA-256 over
    /// 64 bytes. Returns the permanent id.
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
        let chain = chain_step(&self.head, &digest);
        let mut entry = [0u8; IDX_ENTRY_LEN as usize];
        entry[0..8].copy_from_slice(&self.seg_len.to_le_bytes());
        entry[8..40].copy_from_slice(&chain);
        self.seg.write_all(&hdr)?;
        self.seg.write_all(bytes)?;
        self.idx.write_all(&entry)?;
        self.seg_len += HEADER_LEN as u64 + bytes.len() as u64;
        self.next_id += 1;
        self.head = chain;
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
        self.idx.get_ref().read_exact_at(&mut off, IDX_HEADER_LEN + id.0 * IDX_ENTRY_LEN)?;
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
    /// Bytes already stored per source name, summed from the records themselves through a
    /// snapshot: framing is lossless, so a source's records concatenate to the exact prefix
    /// of its input already consumed, whatever mix of producers wrote the store and however
    /// it stopped. O(records) once at startup; a record that cannot be read is an error, not
    /// a silent zero, because a wrong offset would store bytes twice.
    pub fn ingested_bytes(&mut self) -> io::Result<HashMap<String, u64>> {
        let names = self.source_names()?;
        let reader = self.reader()?;
        let mut by_id: HashMap<u32, u64> = HashMap::new();
        for id in 0..reader.len() {
            let rec = reader.get(RawId(id)).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("raw id {id} is unreadable while reconciling ingest offsets; run `ulpf verify`")))?;
            *by_id.entry(rec.source).or_default() += rec.bytes.len() as u64;
        }
        let mut out = HashMap::new();
        for (source, bytes) in by_id {
            let name = names.get(&source).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("source id {source} has records but no catalogue row")))?;
            out.insert(name.clone(), bytes);
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

/// Reads (creating for a fresh store) the index header and returns the store id. An index
/// that has records but no `ULPFIDX` magic is a pre-chain store: refused, never migrated.
fn idx_header(idx: &mut File, dir: &Path, seg_file_len: u64) -> io::Result<[u8; 16]> {
    let idx_len = idx.metadata()?.len();
    let mut header = [0u8; IDX_HEADER_LEN as usize];
    if idx_len >= IDX_HEADER_LEN {
        idx.seek(SeekFrom::Start(0))?;
        idx.read_exact(&mut header)?;
        if &header[..8] != IDX_MAGIC {
            return Err(pre_chain(dir));
        }
        return Ok(header[8..24].try_into().expect("16 bytes"));
    }
    // Too short to hold one entry: a fresh store, or a header torn before any record
    // reached the segment. Anything else with a short index is pre-chain.
    if seg_file_len > FILE_MAGIC.len() as u64 {
        return Err(pre_chain(dir));
    }
    let store_id = new_store_id();
    header[..8].copy_from_slice(IDX_MAGIC);
    header[8..24].copy_from_slice(&store_id);
    idx.seek(SeekFrom::Start(0))?;
    idx.write_all(&header)?;
    idx.set_len(IDX_HEADER_LEN)?;
    Ok(store_id)
}

fn pre_chain(dir: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("store {} predates the integrity chain (raw.idx has no ULPFIDX header); there is no migration path", dir.display()),
    )
}

/// 16 random bytes from `/dev/urandom`, or a digest of time and pid if it is unreadable.
fn new_store_id() -> [u8; 16] {
    let mut id = [0u8; 16];
    if File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut id)).is_ok() {
        return id;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    let digest: [u8; 32] = h.finalize().into();
    id.copy_from_slice(&digest[..16]);
    id
}

fn genesis_of(store_id: &[u8; 16]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ULPF chain genesis");
    h.update(store_id);
    h.finalize().into()
}

fn chain_step(prev: &[u8; 32], digest: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(digest);
    h.finalize().into()
}

/// Lowercase hex of a digest or store id.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Returns `(record count, segment end, chain of the last record)` after cutting both files
/// to the last state that is consistent in both directions (module doc). Reindexed records
/// get their chain value recomputed from the surviving head, so the chain of the surviving
/// records is intact by construction.
fn recover(seg: &mut File, idx: &mut File, genesis: [u8; 32]) -> io::Result<(u64, u64, [u8; 32])> {
    let seg_file_len = seg.metadata()?.len();
    let mut n = idx.metadata()?.len().saturating_sub(IDX_HEADER_LEN) / IDX_ENTRY_LEN;
    let mut seg_len = FILE_MAGIC.len() as u64;
    let mut head = genesis;
    while n > 0 {
        idx.seek(SeekFrom::Start(IDX_HEADER_LEN + (n - 1) * IDX_ENTRY_LEN))?;
        let mut entry = [0u8; IDX_ENTRY_LEN as usize];
        idx.read_exact(&mut entry)?;
        let off = u64::from_le_bytes(entry[0..8].try_into().expect("8 bytes"));
        if let Some((end, _)) = record_end(seg, off, n - 1, seg_file_len, false)? {
            seg_len = end;
            head = entry[8..40].try_into().expect("32 bytes");
            break;
        }
        n -= 1;
    }
    while let Some((end, digest)) = record_end(seg, seg_len, n, seg_file_len, true)? {
        head = chain_step(&head, &digest);
        let mut entry = [0u8; IDX_ENTRY_LEN as usize];
        entry[0..8].copy_from_slice(&seg_len.to_le_bytes());
        entry[8..40].copy_from_slice(&head);
        idx.seek(SeekFrom::Start(IDX_HEADER_LEN + n * IDX_ENTRY_LEN))?;
        idx.write_all(&entry)?;
        n += 1;
        seg_len = end;
    }
    idx.set_len(IDX_HEADER_LEN + n * IDX_ENTRY_LEN)?;
    seg.set_len(seg_len)?;
    Ok((n, seg_len, head))
}

/// End offset and stored digest of the record at `off` if a complete record carrying `id`
/// is there. With `check_digest` the bytes must also hash to the stored digest (an
/// unindexed trailing record may be torn in the middle of its bytes).
fn record_end(seg: &mut File, off: u64, id: u64, seg_file_len: u64, check_digest: bool) -> io::Result<Option<(u64, [u8; 32])>> {
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
    let stored: [u8; 32] = hdr[28..60].try_into().expect("32 bytes");
    if check_digest {
        let mut bytes = vec![0u8; len as usize];
        seg.read_exact(&mut bytes)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if digest != stored {
            return Ok(None);
        }
    }
    Ok(Some((end, stored)))
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
    store_id: [u8; 16],
    genesis: [u8; 32],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyReason {
    /// The bytes no longer hash to the record's stored digest (or the record is unreadable).
    Digest,
    /// The stored chain value does not follow from the predecessor's chain and this digest.
    Chain,
}

impl VerifyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifyReason::Digest => "digest",
            VerifyReason::Chain => "chain",
        }
    }
}

pub struct VerifyReport {
    pub checked: u64,
    pub corrupt: Vec<RawId>,
    /// Lowest id whose digest or chain value is wrong.
    pub first_bad: Option<(RawId, VerifyReason)>,
    /// Attestation checkpoints compared (0 without an attestation).
    pub checkpoints: u64,
    /// First checkpoint whose chain value in the store disagrees with the attestation.
    pub bad_checkpoint: Option<RawId>,
    /// The attestation does not describe this store at all (wrong store, shorter store).
    pub attestation_problem: Option<String>,
}

impl VerifyReport {
    /// Nothing disagreed: digests, chain, and every checkpoint that was checked.
    pub fn ok(&self) -> bool {
        self.corrupt.is_empty() && self.bad_checkpoint.is_none() && self.attestation_problem.is_none()
    }
}

/// The document `ulpf attest` writes: enough for a stranger with the store directory to
/// name the first record a later rewrite touched. Field order is the contract's order.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Attestation {
    pub format: String,
    pub generated: String,
    pub store_id: String,
    pub records: u64,
    pub genesis: String,
    pub head: String,
    pub checkpoints: Vec<Checkpoint>,
    pub record_digest: String,
    pub chain: String,
    pub verify: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Checkpoint {
    pub id: u64,
    pub chain: String,
}

impl RawReader {
    pub fn open(dir: &Path) -> io::Result<RawReader> {
        let seg_file = File::open(dir.join("raw.seg"))?;
        let idx_file = File::open(dir.join("raw.idx"))?;
        // The writer flushes the segment before the index, so an index mapped at time T
        // names only records the segment already held at T, and a segment mapped after T
        // is a superset: map the index first, or a reader beside a live writer could see
        // entries whose records lie past its segment mapping and call them corrupt.
        // SAFETY: the files are only appended to while a writer runs; recovery truncates
        // and rewrites only bytes above the last complete record, before any reader that
        // this writer hands out exists (D7, D33).
        let idx = unsafe { Mmap::map(&idx_file)? };
        let seg = unsafe { Mmap::map(&seg_file)? };
        if seg.len() < FILE_MAGIC.len() || &seg[..FILE_MAGIC.len()] != FILE_MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "raw.seg: bad file magic"));
        }
        if idx.len() < IDX_HEADER_LEN as usize || &idx[..8] != IDX_MAGIC {
            return Err(pre_chain(dir));
        }
        let store_id: [u8; 16] = idx[8..24].try_into().expect("16 bytes");
        let count = (idx.len() as u64 - IDX_HEADER_LEN) / IDX_ENTRY_LEN;
        Ok(RawReader { seg, idx, count, dir: dir.to_path_buf(), store_id, genesis: genesis_of(&store_id) })
    }

    pub fn store_id(&self) -> [u8; 16] {
        self.store_id
    }

    pub fn genesis(&self) -> [u8; 32] {
        self.genesis
    }

    /// Chain value of the last record; `None` for an empty store.
    pub fn head(&self) -> Option<[u8; 32]> {
        self.chain(RawId(self.count.checked_sub(1)?))
    }

    /// Chain value stored for one record. `None` when the id was never issued.
    pub fn chain(&self, id: RawId) -> Option<[u8; 32]> {
        if id.0 >= self.count {
            return None;
        }
        let p = (IDX_HEADER_LEN + id.0 * IDX_ENTRY_LEN + 8) as usize;
        self.idx.get(p..p + 32)?.try_into().ok()
    }

    pub fn len(&self) -> u64 {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The whole mapped segment; a record's bytes are a sub-slice of it, so a caller can
    /// keep byte ranges into the segment instead of copying records.
    pub fn segment(&self) -> &[u8] {
        &self.seg
    }

    /// The record with this id, or `None` if the id was never issued or the record is
    /// structurally damaged (bad magic, length past end of segment).
    pub fn get(&self, id: RawId) -> Option<RawRecord<'_>> {
        if id.0 >= self.count {
            return None;
        }
        let p = (IDX_HEADER_LEN + id.0 * IDX_ENTRY_LEN) as usize;
        let off = u64::from_le_bytes(self.idx.get(p..p + 8)?.try_into().ok()?) as usize;
        let hdr = self.seg.get(off..off.checked_add(HEADER_LEN)?)?;
        if u32::from_le_bytes(hdr[0..4].try_into().ok()?) != REC_MAGIC {
            return None;
        }
        let rec_id = u64::from_le_bytes(hdr[4..12].try_into().ok()?);
        let receipt_nanos = i64::from_le_bytes(hdr[12..20].try_into().ok()?);
        let source = u32::from_le_bytes(hdr[20..24].try_into().ok()?);
        let len = u32::from_le_bytes(hdr[24..28].try_into().ok()?) as usize;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&hdr[28..60]);
        let body = off.checked_add(HEADER_LEN)?;
        let bytes = self.seg.get(body..body.checked_add(len)?)?;
        if rec_id != id.0 {
            return None;
        }
        Some(RawRecord { id, receipt_nanos, source, sha256, bytes })
    }

    pub fn iter(&self) -> impl Iterator<Item = Option<RawRecord<'_>>> + '_ {
        (0..self.count).map(move |i| self.get(RawId(i)))
    }

    /// Recomputes every digest and every chain link. Records that are unreadable, whose
    /// bytes no longer hash to the stored digest, or whose chain value does not follow from
    /// the predecessor's are listed as corrupt; `first_bad` is the lowest of them.
    pub fn verify(&self) -> VerifyReport {
        self.check(None)
    }

    /// `verify`, plus every checkpoint in an attestation taken earlier. A store rewritten
    /// consistently from record N passes `verify` and fails here at the first checkpoint
    /// at or after N.
    pub fn verify_against(&self, att: &Attestation) -> VerifyReport {
        self.check(Some(att))
    }

    fn check(&self, att: Option<&Attestation>) -> VerifyReport {
        let mut corrupt = Vec::new();
        let mut first_bad = None;
        let mut prev = self.genesis;
        for i in 0..self.count {
            let stored_chain = self.chain(RawId(i));
            let reason = match self.get(RawId(i)) {
                None => Some(VerifyReason::Digest),
                Some(rec) => {
                    let d: [u8; 32] = Sha256::digest(rec.bytes).into();
                    if d != rec.sha256 {
                        Some(VerifyReason::Digest)
                    } else if stored_chain != Some(chain_step(&prev, &rec.sha256)) {
                        Some(VerifyReason::Chain)
                    } else {
                        None
                    }
                }
            };
            if let Some(reason) = reason {
                corrupt.push(RawId(i));
                first_bad.get_or_insert((RawId(i), reason));
            }
            // The next link is checked against what this record actually stores, so one
            // break is reported once instead of poisoning every later record.
            prev = stored_chain.unwrap_or(prev);
        }
        let mut report = VerifyReport { checked: self.count, corrupt, first_bad, checkpoints: 0, bad_checkpoint: None, attestation_problem: None };
        if let Some(att) = att {
            self.check_attestation(att, &mut report);
        }
        report
    }

    fn check_attestation(&self, att: &Attestation, report: &mut VerifyReport) {
        if !att.store_id.eq_ignore_ascii_case(&hex(&self.store_id)) || !att.genesis.eq_ignore_ascii_case(&hex(&self.genesis)) {
            report.attestation_problem = Some(format!("attestation is for store {} (this store is {})", att.store_id, hex(&self.store_id)));
            return;
        }
        if att.records > self.count {
            report.attestation_problem = Some(format!("attestation covers {} records, the store holds {}", att.records, self.count));
        }
        // the head is the strongest single fact in the document: the last attested record's
        // chain value must be what the store holds for that id (a store rewritten from any
        // point below it, however consistently, cannot reproduce it without the store id)
        if att.records > 0 && att.records <= self.count {
            let last = RawId(att.records - 1);
            if !self.chain(last).map(|c| hex(&c)).is_some_and(|c| c.eq_ignore_ascii_case(&att.head)) {
                report.bad_checkpoint = Some(last);
            }
        }
        if att.checkpoints.is_empty() && att.records > 0 {
            report.attestation_problem.get_or_insert_with(|| "attestation carries no checkpoints; refusing to call it verified".to_string());
        }
        for cp in &att.checkpoints {
            if cp.id >= self.count {
                report.attestation_problem.get_or_insert_with(|| format!("attestation checkpoint at id {} is past the end of the store", cp.id));
                break;
            }
            report.checkpoints += 1;
            if !self.chain(RawId(cp.id)).map(|c| hex(&c)).is_some_and(|c| c.eq_ignore_ascii_case(&cp.chain)) {
                report.bad_checkpoint = Some(RawId(cp.id));
                break;
            }
        }
    }

    /// The attestation document for the store as it is now: one checkpoint every
    /// `CHECKPOINT_EVERY` records plus the last.
    pub fn attest(&self) -> Attestation {
        let mut checkpoints = Vec::new();
        let mut id = 0;
        while id < self.count {
            checkpoints.push(Checkpoint { id, chain: self.chain(RawId(id)).map(|c| hex(&c)).unwrap_or_default() });
            id += CHECKPOINT_EVERY;
        }
        if let Some(last) = self.count.checked_sub(1)
            && checkpoints.last().is_none_or(|c| c.id != last)
        {
            checkpoints.push(Checkpoint { id: last, chain: self.chain(RawId(last)).map(|c| hex(&c)).unwrap_or_default() });
        }
        let mut generated = String::new();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
        ulpf_time::format_rfc3339(nanos, &mut generated);
        Attestation {
            format: "ulpf-attestation/1".to_owned(),
            generated,
            store_id: hex(&self.store_id),
            records: self.count,
            genesis: hex(&self.genesis),
            head: hex(&self.head().unwrap_or(self.genesis)),
            checkpoints,
            record_digest: "sha256(bytes)".to_owned(),
            chain: "sha256(prev_chain || record_digest)".to_owned(),
            verify: "ulpf verify --store DIR --attestation FILE".to_owned(),
        }
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
