//! Reads of the JSON Lines output the sink wrote: the emitted line for a raw id and the
//! export stream. Never the store (D42) and never the writer's handle: a read-only open,
//! bounded to the bytes on disk at that moment cut to the last terminator, so a line the
//! writer is mid-way through is never returned. Lines are in raw id order (D60), which
//! makes the lookup a binary search instead of a scan of the file.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// The raw id an emitted line carries, read from its `ulpf` object without parsing the
/// whole line (a binary search reads about twenty lines, and one may be 4 MB).
pub fn line_id(line: &[u8]) -> Option<u64> {
    let at = memchr::memmem::find(line, b"\"ulpf\":{")?;
    let rest = &line[at..];
    let k = memchr::memmem::find(rest, b"\"raw_id\":")? + "\"raw_id\":".len();
    let n = rest[k..].iter().take_while(|b| b.is_ascii_digit()).count();
    std::str::from_utf8(&rest[k..k + n]).ok()?.parse().ok()
}

pub struct Output {
    file: BufReader<File>,
    /// Bytes on disk when opened, cut to the last line terminator: the snapshot every
    /// read respects.
    pub len: u64,
}

impl Output {
    pub fn open(path: &Path) -> io::Result<Output> {
        let mut f = File::open(path)?;
        let disk = f.metadata()?.len();
        let len = last_terminator(&mut f, disk)?;
        Ok(Output { file: BufReader::with_capacity(1 << 16, f), len })
    }

    /// The line starting at `at` with its terminator, or `None` when it runs past `len`.
    pub fn line_at(&mut self, at: u64) -> io::Result<Option<Vec<u8>>> {
        if at >= self.len {
            return Ok(None);
        }
        self.file.seek(SeekFrom::Start(at))?;
        let mut line = Vec::new();
        self.file.read_until(b'\n', &mut line)?;
        Ok((line.last() == Some(&b'\n') && at + line.len() as u64 <= self.len).then_some(line))
    }

    /// The first line start at or after `at` (`len` when there is none).
    fn next_start(&mut self, at: u64) -> io::Result<u64> {
        if at == 0 {
            return Ok(0);
        }
        if at >= self.len {
            return Ok(self.len);
        }
        self.file.seek(SeekFrom::Start(at - 1))?;
        let mut skipped = 0u64;
        let mut buf = [0u8; 1 << 14];
        loop {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                return Ok(self.len);
            }
            if let Some(i) = memchr::memchr(b'\n', &buf[..n]) {
                return Ok((at - 1 + skipped + i as u64 + 1).min(self.len));
            }
            skipped += n as u64;
        }
    }

    /// Offset of the first line whose raw id is at least `id`; `len` when no line is.
    /// A binary search over line starts: each probe aligns to the next line start and
    /// reads one line, so the cost is logarithmic in the file's size plus the length of
    /// the lines it lands on.
    pub fn lower_bound(&mut self, id: u64) -> io::Result<u64> {
        let (mut lo, mut hi) = (0u64, self.len);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let s = self.next_start(mid)?;
            if s >= hi {
                // no line starts in [mid, hi): the one spanning it began before mid, so
                // [lo, hi) is at most twice that line's length; walk it
                let mut at = lo;
                while at < hi {
                    match self.line_at(at)? {
                        Some(line) if line_id(&line).is_some_and(|x| x >= id) => return Ok(at),
                        Some(line) => at += line.len() as u64,
                        None => break,
                    }
                }
                return Ok(hi);
            }
            match self.line_at(s)? {
                Some(line) if line_id(&line).is_some_and(|x| x >= id) => hi = s,
                Some(line) => lo = s + line.len() as u64,
                None => hi = s,
            }
        }
        Ok(lo)
    }

    /// The emitted line with exactly this raw id, without its terminator.
    pub fn find(&mut self, id: u64) -> io::Result<Option<Vec<u8>>> {
        let at = self.lower_bound(id)?;
        Ok(match self.line_at(at)? {
            Some(mut line) if line_id(&line) == Some(id) => {
                line.pop();
                Some(line)
            }
            _ => None,
        })
    }
}

/// One past the last `\n` at or before `disk` (0 when the file holds none): the torn
/// tail a mid-line flush may leave is excluded.
fn last_terminator(f: &mut File, disk: u64) -> io::Result<u64> {
    let mut end = disk;
    let mut buf = vec![0u8; 1 << 16];
    while end > 0 {
        let start = end.saturating_sub(buf.len() as u64);
        let n = (end - start) as usize;
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut buf[..n])?;
        if let Some(i) = memchr::memrchr(b'\n', &buf[..n]) {
            return Ok(start + i as u64 + 1);
        }
        end = start;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(lines: &[(u64, usize)], torn: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("ulpf-outfile-{}-{}.jsonl", std::process::id(), lines.len()));
        let mut f = File::create(&p).unwrap();
        for (id, pad) in lines {
            writeln!(f, "{{\"message\":\"{}\",\"ulpf\":{{\"parse_status\":\"parsed\",\"raw_id\":{id}}}}}", "x".repeat(*pad)).unwrap();
        }
        f.write_all(torn.as_bytes()).unwrap();
        p
    }

    #[test]
    fn finds_every_id_and_bounds_at_the_last_terminator() {
        // ids with a gap and one long line, plus a torn tail the reader must not see
        let lines: Vec<(u64, usize)> = (0..200u64).filter(|i| *i != 57).map(|i| (i, if i == 100 { 300_000 } else { (i as usize * 7) % 40 })).collect();
        let p = write(&lines, "{\"message\":\"torn\",\"ulpf\":{\"raw_id\":999");
        let mut out = Output::open(&p).unwrap();
        for (id, _) in &lines {
            let line = out.find(*id).unwrap().unwrap_or_else(|| panic!("id {id} not found"));
            assert_eq!(line_id(&line), Some(*id));
            assert!(!line.ends_with(b"\n"));
        }
        assert_eq!(out.find(57).unwrap(), None, "a gap is not filled");
        assert_eq!(out.find(999).unwrap(), None, "the torn tail is invisible");
        assert_eq!(out.find(5000).unwrap(), None);
        // lower_bound lands on the next id across the gap and on len past the end
        let at = out.lower_bound(57).unwrap();
        assert_eq!(line_id(&out.line_at(at).unwrap().unwrap()), Some(58));
        assert_eq!(out.lower_bound(5000).unwrap(), out.len);
        assert_eq!(out.lower_bound(0).unwrap(), 0);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn line_id_reads_the_ulpf_object_only() {
        assert_eq!(line_id(br#"{"unmapped":{"raw_id":"7"},"ulpf":{"parser":"x","raw_id":42,"sub_status":"matched"}}"#), Some(42));
        assert_eq!(line_id(b"{\"a\":1}"), None);
        let p = write(&[], "");
        assert_eq!(Output::open(&p).unwrap().len, 0);
        let _ = std::fs::remove_file(p);
    }
}
