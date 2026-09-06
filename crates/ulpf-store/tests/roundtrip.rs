use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Range;

use ulpf_store::{Framer, RawId, RawReader, RawStore};

// raw.idx layout (crates/ulpf-store/src/store.rs): magic + store id, then 40 bytes per record.
const IDX_HEADER: u64 = 24;
const IDX_ENTRY: u64 = 40;

const CORPUS: &[u8] = b"<134>Sep  4 10:15:23 fw01 %ASA-6-302013: Built outbound TCP connection 1 for outside:1.1.1.1/443 to inside:10.0.0.5/51234\n\
date=2026-09-04 time=10:15:23 devname=\"FGT\" msg=\"caf\xc3\xa9 and raw \xff\xfe bytes\" action=\"deny\"\r\n\
Sep  4 10:15:24 host java[123]: Exception in thread main\n\
\tat com.example.Foo.bar(Foo.java:12)\n\
\tat com.example.Baz.qux(Baz.java:34)\n\
\n\
\x20\x20\x20\n\
2026-09-04T10:15:25Z single line\n\
\n\
last line without newline";

fn frame_all(buf: &[u8]) -> Vec<Vec<u8>> {
    Framer::new(buf, true).map(|r| buf[r].to_vec()).collect()
}

fn frame_chunked(buf: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut carry: Vec<u8> = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        let end = (pos + chunk).min(buf.len());
        carry.extend_from_slice(&buf[pos..end]);
        pos = end;
        let eof = pos >= buf.len();
        let mut framer = Framer::new(&carry, eof);
        let ranges: Vec<Range<usize>> = framer.by_ref().collect();
        for r in &ranges {
            out.push(carry[r.clone()].to_vec());
        }
        let rest = framer.remainder().to_vec();
        carry = rest;
    }
    assert!(carry.is_empty(), "carry left over after eof");
    out
}

#[test]
fn framing_is_lossless_and_groups_continuations() {
    let events = frame_all(CORPUS);
    assert_eq!(events.len(), 5, "{:?}", events.iter().map(|e| String::from_utf8_lossy(e).into_owned()).collect::<Vec<_>>());
    assert!(events[1].ends_with(b"\r\n"));
    assert!(events[2].starts_with(b"Sep  4 10:15:24 host java"));
    assert_eq!(events[2].matches_count(b"\tat "), 2);
    assert!(events[2].ends_with(b"\n   \n"), "blank/whitespace lines attach to the previous event");
    assert!(events[3].ends_with(b"single line\n\n"), "blank line attaches to the preceding event");
    assert_eq!(events[4], b"last line without newline".to_vec());
    let joined: Vec<u8> = events.concat();
    assert_eq!(joined, CORPUS);
}

trait Count {
    fn matches_count(&self, needle: &[u8]) -> usize;
}
impl Count for Vec<u8> {
    fn matches_count(&self, needle: &[u8]) -> usize {
        memchr_count(self, needle)
    }
}
fn memchr_count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

#[test]
fn framing_is_identical_across_every_chunk_boundary() {
    let whole = frame_all(CORPUS);
    for chunk in [1usize, 2, 3, 5, 7, 11, 16, 31, 64, 100, 1000] {
        assert_eq!(frame_chunked(CORPUS, chunk), whole, "chunk size {chunk}");
    }
}

#[test]
fn framing_edge_cases() {
    assert!(frame_all(b"").is_empty());
    assert_eq!(frame_all(b"\n"), vec![b"\n".to_vec()]);
    assert_eq!(frame_all(b"\n\n  \n"), vec![b"\n\n  \n".to_vec()]);
    assert_eq!(frame_all(b"a"), vec![b"a".to_vec()]);
    assert_eq!(frame_all(b"a\nb"), vec![b"a\n".to_vec(), b"b".to_vec()]);
    let huge = vec![b'x'; 5 * 1024 * 1024];
    assert_eq!(frame_all(&huge).len(), 1);
    let mut f = Framer::new(b"abc", false);
    assert!(f.next().is_none());
    assert_eq!(f.remainder(), b"abc");
    let mut f = Framer::new(b"abc\n", false);
    assert!(f.next().is_none(), "cannot know whether the next line continues this one");
    let mut f = Framer::new(b"abc\nd", false);
    assert_eq!(f.next(), Some(0..4));
    assert!(f.next().is_none());
    assert_eq!(f.remainder(), b"d");
}

#[test]
fn store_round_trips_bytes_and_digests_and_survives_reopen() {
    let dir = std::env::temp_dir().join(format!("ulpf-store-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let events = frame_all(CORPUS);
    let mut ids = Vec::new();
    {
        let mut store = RawStore::open(&dir).unwrap();
        let src = store.source_id("corpus.log").unwrap();
        assert_eq!(store.source_id("corpus.log").unwrap(), src);
        for (i, e) in events.iter().enumerate() {
            let id = store.append(src, 1_000 + i as i64, e).unwrap();
            assert_eq!(id, RawId(i as u64));
            ids.push(id);
        }
        store.record_ingest(src, ids.first().copied(), ids.len() as u64, CORPUS.len() as u64, 1).unwrap();
        store.flush(true).unwrap();
    }
    let reader = RawReader::open(&dir).unwrap();
    assert_eq!(reader.len(), events.len() as u64);
    for (i, e) in events.iter().enumerate() {
        let rec = reader.get(RawId(i as u64)).expect("record present");
        assert_eq!(rec.bytes, e.as_slice(), "bytes differ for id {i}");
        assert_eq!(rec.receipt_nanos, 1_000 + i as i64);
        let expect: [u8; 32] = sha2::Sha256::digest(e).into();
        assert_eq!(rec.sha256, expect);
    }
    assert!(reader.get(RawId(events.len() as u64)).is_none());
    let report = reader.verify();
    assert_eq!(report.checked, events.len() as u64);
    assert!(report.corrupt.is_empty());
    assert_eq!(reader.source_names().unwrap().len(), 1);

    // Simulate a crash: unindexed junk after the last record, then a torn index entry.
    {
        let mut seg = OpenOptions::new().append(true).open(dir.join("raw.seg")).unwrap();
        seg.write_all(b"PARTIAL RECORD JUNK").unwrap();
        let mut idx = OpenOptions::new().append(true).open(dir.join("raw.idx")).unwrap();
        idx.write_all(&[1, 2, 3]).unwrap();
    }
    {
        let mut store = RawStore::open(&dir).unwrap();
        assert_eq!(store.len(), events.len() as u64);
        let src = store.source_id("second.log").unwrap();
        let id = store.append(src, 7, b"after recovery\n").unwrap();
        assert_eq!(id, RawId(events.len() as u64));
    }
    let reader = RawReader::open(&dir).unwrap();
    assert_eq!(reader.len(), events.len() as u64 + 1);
    assert_eq!(reader.get(RawId(events.len() as u64)).unwrap().bytes, b"after recovery\n");
    let report = reader.verify();
    assert!(report.corrupt.is_empty(), "{:?}", report.corrupt);
    let all: Vec<u8> = reader.iter().take(events.len()).flat_map(|r| r.unwrap().bytes.to_vec()).collect();
    assert_eq!(all, CORPUS, "concatenated records reproduce the original file");
    std::fs::remove_dir_all(&dir).unwrap();
}

use sha2::Digest;

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-store-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn fill(dir: &std::path::Path, n: usize) {
    let mut s = RawStore::open(dir).unwrap();
    let src = s.source_id("a.log").unwrap();
    for i in 0..n {
        s.append(src, 1, format!("event {i}\n").as_bytes()).unwrap();
    }
    s.flush(true).unwrap();
}

fn offset_of(dir: &std::path::Path, id: u64) -> u64 {
    let idx = std::fs::read(dir.join("raw.idx")).unwrap();
    let p = (IDX_HEADER + id * IDX_ENTRY) as usize;
    u64::from_le_bytes(idx[p..p + 8].try_into().unwrap())
}

#[test]
fn a_second_writer_is_refused_while_the_store_is_open() {
    let dir = temp("lock");
    let first = RawStore::open(&dir).unwrap();
    let err = match RawStore::open(&dir) {
        Ok(_) => panic!("a second writer must be refused"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("in use"), "{err}");
    drop(first);
    RawStore::open(&dir).unwrap();
}

#[test]
fn index_ahead_of_segment_recovers_to_the_last_complete_record() {
    let dir = temp("idx-ahead");
    fill(&dir, 50);
    // the index buffer drained, the segment buffer did not: record 40's header is torn
    let cut = offset_of(&dir, 40) + 30;
    OpenOptions::new().write(true).open(dir.join("raw.seg")).unwrap().set_len(cut).unwrap();
    let mut s = RawStore::open(&dir).unwrap();
    assert_eq!(s.len(), 40, "records 40..49 were indexed but never fully written");
    let src = s.source_id("a.log").unwrap();
    assert_eq!(s.append(src, 2, b"after the crash\n").unwrap(), RawId(40));
    s.flush(true).unwrap();
    drop(s);
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.len(), 41);
    assert!(r.verify().corrupt.is_empty());
    assert_eq!(r.get(RawId(39)).unwrap().bytes, b"event 39\n");
    assert_eq!(r.get(RawId(40)).unwrap().bytes, b"after the crash\n");
}

#[test]
fn segment_ahead_of_index_reindexes_complete_records_and_drops_a_torn_tail() {
    let dir = temp("seg-ahead");
    fill(&dir, 50);
    // the segment buffer drained, the index buffer did not (30 entries), and a record
    // was torn at the very end
    OpenOptions::new().write(true).open(dir.join("raw.idx")).unwrap().set_len(IDX_HEADER + 30 * IDX_ENTRY).unwrap();
    let mut seg = OpenOptions::new().append(true).open(dir.join("raw.seg")).unwrap();
    seg.write_all(b"ULPF\x32\x00\x00\x00\x00\x00\x00\x00torn").unwrap();
    drop(seg);
    let mut s = RawStore::open(&dir).unwrap();
    assert_eq!(s.len(), 50, "records 30..49 are complete in the segment and get their ids back");
    let src = s.source_id("a.log").unwrap();
    assert_eq!(s.append(src, 3, b"next\n").unwrap(), RawId(50));
    s.flush(true).unwrap();
    drop(s);
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.len(), 51);
    assert!(r.verify().corrupt.is_empty());
    assert_eq!(r.get(RawId(49)).unwrap().bytes, b"event 49\n");
    assert_eq!(r.get(RawId(50)).unwrap().bytes, b"next\n");
}

fn file_len(path: std::path::PathBuf) -> u64 {
    std::fs::metadata(path).unwrap().len()
}

/// The shape Windows refuses: a reader has the files mapped while a writer reopens the
/// store over a torn tail. Recovery must reclaim the tail without shrinking either file
/// (SetEndOfFile on a mapped file fails with ERROR_USER_MAPPED_FILE; on POSIX the reader
/// would fault instead), the mapped reader keeps every record it had, and a fresh reader
/// does not mistake the reclaimed bytes for records.
#[test]
fn a_reader_mapped_across_a_reopen_keeps_its_records_and_no_file_shrinks() {
    let dir = temp("live-map");
    fill(&dir, 50);
    let mapped = RawReader::open(&dir).unwrap();
    assert_eq!(mapped.len(), 50);
    // the crash: a torn record after 49, a torn index entry after its 50 entries
    OpenOptions::new().append(true).open(dir.join("raw.seg")).unwrap().write_all(b"ULPF\x32\x00\x00\x00\x00\x00\x00\x00torn").unwrap();
    OpenOptions::new().append(true).open(dir.join("raw.idx")).unwrap().write_all(&[1, 2, 3]).unwrap();
    let (seg_before, idx_before) = (file_len(dir.join("raw.seg")), file_len(dir.join("raw.idx")));
    {
        let mut s = RawStore::open(&dir).unwrap();
        assert_eq!(s.len(), 50);
        assert_eq!(file_len(dir.join("raw.seg")), seg_before, "the torn tail is reclaimed by position, never by truncation");
        assert_eq!(file_len(dir.join("raw.idx")), idx_before);
        let src = s.source_id("a.log").unwrap();
        assert_eq!(s.append(src, 4, b"after\n").unwrap(), RawId(50));
        s.flush(true).unwrap();
    }
    assert_eq!(mapped.len(), 50, "the reader mapped across the reopen is bounded to what it opened with");
    assert!(mapped.verify().ok());
    assert_eq!(mapped.get(RawId(49)).unwrap().bytes, b"event 49\n");
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.len(), 51);
    assert!(r.verify().ok(), "{:?}", r.verify().first_bad);
    assert_eq!(r.get(RawId(50)).unwrap().bytes, b"after\n");

    // the index-ahead direction: six entries name records the segment never received;
    // recovery zeroes them in place and a reader stops before them. The test's own cut is
    // made with no mapping alive, since that is exactly what Windows refuses.
    drop(mapped);
    drop(r);
    let cut = offset_of(&dir, 45);
    let seg_len = file_len(dir.join("raw.seg"));
    OpenOptions::new().write(true).open(dir.join("raw.seg")).unwrap().set_len(cut).unwrap();
    let crashed = RawReader::open(&dir).unwrap();
    assert_eq!(crashed.len(), 51, "a reader on the crashed store still counts the six entries the segment never received");
    assert_eq!(crashed.verify().first_bad.map(|(id, _)| id), Some(RawId(45)), "and verify names the first of them");
    drop(crashed);
    let idx_len = file_len(dir.join("raw.idx"));
    {
        let mut s = RawStore::open(&dir).unwrap();
        assert_eq!(s.len(), 45);
        assert_eq!(file_len(dir.join("raw.idx")), idx_len, "six stale entries are zeroed, the index is not cut");
        assert!(file_len(dir.join("raw.seg")) < seg_len, "the test cut the segment, recovery did not extend it");
        let src = s.source_id("a.log").unwrap();
        assert_eq!(s.append(src, 5, b"again\n").unwrap(), RawId(45));
        s.flush(true).unwrap();
    }
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.len(), 46, "five zeroed entries behind the last record are not records");
    assert!(r.verify().ok(), "{:?}", r.verify().first_bad);
    drop(r);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_writer_reads_its_own_records_back_by_id() {
    let dir = std::env::temp_dir().join(format!("ulpf-store-get-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = RawStore::open(&dir).unwrap();
    let src = store.source_id("a.log").unwrap();
    let ids: Vec<RawId> = (0..50u32).map(|i| store.append(src, 1_000 + i as i64, format!("line {i}\n").as_bytes()).unwrap()).collect();
    let rec = store.get(ids[17]).unwrap().unwrap();
    assert_eq!(rec.bytes, b"line 17\n");
    assert_eq!(rec.receipt_nanos, 1_017);
    assert_eq!(rec.source, src);
    assert_eq!(rec.sha256, <[u8; 32]>::from(sha2::Sha256::digest(b"line 17\n")));
    assert!(store.get(RawId(50)).unwrap().is_none());
    // the read did not move the append cursor
    let next = store.append(src, 2_000, b"after\n").unwrap();
    assert_eq!(next, RawId(50));
    assert_eq!(store.get(next).unwrap().unwrap().bytes, b"after\n");
    assert_eq!(store.source_names().unwrap().get(&src).map(String::as_str), Some("a.log"));
    store.record_ingest(src, Some(ids[0]), 50, 400, 0).unwrap();
    store.record_ingest(src, Some(next), 1, 6, 0).unwrap();
    // the resume offset is what the records say (ten 7-byte lines, forty 8-byte lines and
    // "after\n" = 396), not what an ingest row claimed (400 + 6): the rows are provenance,
    // the records are the truth a restart resumes from
    assert_eq!(store.ingested_bytes().unwrap().get("a.log"), Some(&396));
    let _ = std::fs::remove_dir_all(&dir);
}
