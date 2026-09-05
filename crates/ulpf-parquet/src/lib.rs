//! Parquet sink for normalized events: a columnar copy of the JSON Lines output.
//!
//! A Parquet file is unreadable until its footer is written, so this is always an
//! *additional* sink and every file is built as `<path>.part` and renamed on close: a
//! reader that lists `*.parquet` only ever sees complete files.
//!
//! The writer takes plain scalars (`Row`), never a parsed or normalized event: it knows
//! nothing about vendors and nothing about the output schema beyond these ten columns,
//! so the parser/mapping wall gains no third side.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parquet::basic::Compression;
use parquet::column::writer::ColumnWriter;
use parquet::data_type::ByteArray;
use parquet::errors::{ParquetError, Result};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;
use parquet::schema::types::TypePtr;

/// The columns, in file order. `normalized` is the emitted JSON line without its
/// newline, so the row is self-describing even when a column below is null.
const SCHEMA: &str = "
message ulpf_event {
  required int64  raw_id;
  required int64  time (TIMESTAMP_MILLIS);
  optional binary parser (UTF8);
  required binary source (UTF8);
  required int32  class_uid;
  required binary normalized (UTF8);
  optional binary src_ip (UTF8);
  optional binary dst_ip (UTF8);
  optional binary user (UTF8);
  optional binary device (UTF8);
  optional int32  dst_port;
}
";

pub struct Row<'a> {
    pub raw_id: i64,
    /// Event time in epoch milliseconds (UTC), the same value as the JSON `time`.
    pub time_ms: i64,
    pub parser: Option<&'a str>,
    pub source: &'a str,
    pub class_uid: i32,
    /// The emitted JSON line, without its trailing newline.
    pub normalized: &'a [u8],
    pub src_ip: Option<&'a str>,
    pub dst_ip: Option<&'a str>,
    pub user: Option<&'a str>,
    pub device: Option<&'a str>,
    pub dst_port: Option<i32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub rows: u64,
    pub files: u64,
}

/// Values plus definition levels for one optional column (0 = null, 1 = present).
#[derive(Default)]
struct Opt<T> {
    vals: Vec<T>,
    def: Vec<i16>,
}

impl<T> Opt<T> {
    fn push(&mut self, v: Option<T>) {
        match v {
            Some(v) => {
                self.vals.push(v);
                self.def.push(1);
            }
            None => self.def.push(0),
        }
    }

    fn clear(&mut self) {
        self.vals.clear();
        self.def.clear();
    }
}

#[derive(Default)]
struct Cols {
    raw_id: Vec<i64>,
    time: Vec<i64>,
    parser: Opt<ByteArray>,
    source: Vec<ByteArray>,
    class_uid: Vec<i32>,
    normalized: Vec<ByteArray>,
    src_ip: Opt<ByteArray>,
    dst_ip: Opt<ByteArray>,
    user: Opt<ByteArray>,
    device: Opt<ByteArray>,
    dst_port: Opt<i32>,
}

impl Cols {
    fn clear(&mut self) {
        self.raw_id.clear();
        self.time.clear();
        self.parser.clear();
        self.source.clear();
        self.class_uid.clear();
        self.normalized.clear();
        self.src_ip.clear();
        self.dst_ip.clear();
        self.user.clear();
        self.device.clear();
        self.dst_port.clear();
    }
}

fn ba(s: &str) -> ByteArray {
    ByteArray::from(s.as_bytes().to_vec())
}

pub struct ParquetWriter {
    writer: SerializedFileWriter<File>,
    schema: TypePtr,
    props: Arc<WriterProperties>,
    /// The file being written: `path` exists as `part` until the footer lands.
    path: PathBuf,
    part: PathBuf,
    cols: Cols,
    row_group: usize,
    stats: Stats,
}

fn part_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

impl ParquetWriter {
    /// Creates `<path>.part`; `finish` or `roll` renames it to `path`. `row_group` rows
    /// are buffered before a row group is flushed (8192 keeps peak memory near 40 MB).
    pub fn create(path: impl Into<PathBuf>, row_group: usize) -> Result<ParquetWriter> {
        let path = path.into();
        let schema: TypePtr = Arc::new(parse_message_type(SCHEMA)?);
        // SNAPPY: the compression cost is what keeps the sink off the critical path;
        // zstd would be ~25% smaller and several times slower to write.
        let props = Arc::new(WriterProperties::builder().set_compression(Compression::SNAPPY).build());
        let part = part_of(&path);
        let writer = SerializedFileWriter::new(File::create(&part)?, Arc::clone(&schema), Arc::clone(&props))?;
        Ok(ParquetWriter { writer, schema, props, path, part, cols: Cols::default(), row_group: row_group.max(1), stats: Stats::default() })
    }

    /// Buffers one row. Infallible by construction: a write can only fail when a row
    /// group is flushed, which is `end_batch`'s job.
    pub fn push(&mut self, row: Row<'_>) {
        let c = &mut self.cols;
        c.raw_id.push(row.raw_id);
        c.time.push(row.time_ms);
        c.parser.push(row.parser.map(ba));
        c.source.push(ba(row.source));
        c.class_uid.push(row.class_uid);
        c.normalized.push(ByteArray::from(row.normalized.to_vec()));
        c.src_ip.push(row.src_ip.map(ba));
        c.dst_ip.push(row.dst_ip.map(ba));
        c.user.push(row.user.map(ba));
        c.device.push(row.device.map(ba));
        c.dst_port.push(row.dst_port);
    }

    pub fn buffered(&self) -> usize {
        self.cols.raw_id.len()
    }

    pub fn stats(&self) -> Stats {
        Stats { rows: self.stats.rows + self.buffered() as u64, files: self.stats.files }
    }

    /// Flushes a row group if the buffer reached `row_group`. Called once per engine
    /// batch, never per event.
    pub fn end_batch(&mut self) -> Result<()> {
        if self.buffered() >= self.row_group {
            self.flush_row_group()?;
        }
        Ok(())
    }

    fn flush_row_group(&mut self) -> Result<()> {
        if self.cols.raw_id.is_empty() {
            return Ok(());
        }
        let rows = self.cols.raw_id.len() as u64;
        let mut rg = self.writer.next_row_group()?;
        let c = &self.cols;
        let mut i = 0usize;
        while let Some(mut col) = rg.next_column()? {
            match (i, col.untyped()) {
                (0, ColumnWriter::Int64ColumnWriter(w)) => w.write_batch(&c.raw_id, None, None).map(|_| ()),
                (1, ColumnWriter::Int64ColumnWriter(w)) => w.write_batch(&c.time, None, None).map(|_| ()),
                (2, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.parser.vals, Some(&c.parser.def), None).map(|_| ()),
                (3, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.source, None, None).map(|_| ()),
                (4, ColumnWriter::Int32ColumnWriter(w)) => w.write_batch(&c.class_uid, None, None).map(|_| ()),
                (5, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.normalized, None, None).map(|_| ()),
                (6, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.src_ip.vals, Some(&c.src_ip.def), None).map(|_| ()),
                (7, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.dst_ip.vals, Some(&c.dst_ip.def), None).map(|_| ()),
                (8, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.user.vals, Some(&c.user.def), None).map(|_| ()),
                (9, ColumnWriter::ByteArrayColumnWriter(w)) => w.write_batch(&c.device.vals, Some(&c.device.def), None).map(|_| ()),
                (10, ColumnWriter::Int32ColumnWriter(w)) => w.write_batch(&c.dst_port.vals, Some(&c.dst_port.def), None).map(|_| ()),
                _ => Err(ParquetError::General(format!("column {i} does not match the ulpf_event schema"))),
            }?;
            col.close()?;
            i += 1;
        }
        rg.close()?;
        self.cols.clear();
        self.stats.rows += rows;
        Ok(())
    }

    /// Writes the footer and renames `<path>.part` to `path`.
    fn close(&mut self) -> Result<()> {
        self.flush_row_group()?;
        self.writer.finish()?;
        std::fs::rename(&self.part, &self.path)?;
        self.stats.files += 1;
        Ok(())
    }

    /// Closes the current file (footer + rename) and starts `next`. The returned stats
    /// are cumulative across every file this writer produced.
    pub fn roll(&mut self, next: impl Into<PathBuf>) -> Result<Stats> {
        self.close()?;
        let next = next.into();
        let part = part_of(&next);
        self.writer = SerializedFileWriter::new(File::create(&part)?, Arc::clone(&self.schema), Arc::clone(&self.props))?;
        self.path = next;
        self.part = part;
        Ok(self.stats)
    }

    pub fn finish(mut self) -> Result<Stats> {
        self.close()?;
        Ok(self.stats)
    }
}
