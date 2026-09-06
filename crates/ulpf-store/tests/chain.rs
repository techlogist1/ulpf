//! Done item 5: the integrity chain. Every test here tampers with the store's files
//! directly, because that is the only way to produce what the chain exists to catch.

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

use sha2::Digest;
use ulpf_store::{Attestation, RawId, RawReader, RawStore, VerifyReason};

// raw.idx layout (crates/ulpf-store/src/store.rs): magic + store id, then 40 bytes per
// record; raw.seg record header is 60 bytes with the digest at byte 28.
const IDX_HEADER: u64 = 24;
const IDX_ENTRY: u64 = 40;
const REC_HEADER: u64 = 60;

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-chain-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn fill(dir: &std::path::Path, n: u64) {
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

/// Overwrites bytes in place: the tamper the store API itself cannot express.
fn poke(path: std::path::PathBuf, at: u64, bytes: &[u8]) {
    let mut f = OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(at)).unwrap();
    f.write_all(bytes).unwrap();
}

fn digest_of(bytes: &[u8]) -> [u8; 32] {
    sha2::Sha256::digest(bytes).into()
}

fn link(prev: [u8; 32], digest: &[u8; 32]) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(prev);
    h.update(digest);
    h.finalize().into()
}

/// Rewrites record `id`'s body and its stored digest, so the per-record digest still passes.
fn rewrite_record(dir: &std::path::Path, id: u64, body: &[u8]) -> [u8; 32] {
    let off = offset_of(dir, id);
    poke(dir.join("raw.seg"), off + REC_HEADER, body);
    let digest = digest_of(body);
    poke(dir.join("raw.seg"), off + 28, &digest);
    digest
}

#[test]
fn the_chain_is_continuous_across_reopen() {
    let dir = temp("reopen");
    fill(&dir, 40);
    let (store_id, genesis, head_40) = {
        let mut s = RawStore::open(&dir).unwrap();
        assert_eq!(s.len(), 40);
        let mut expect = s.genesis();
        for i in 0..40u64 {
            expect = link(expect, &digest_of(format!("event {i}\n").as_bytes()));
            assert_eq!(s.chain(RawId(i)).unwrap(), Some(expect), "chain value for id {i}");
        }
        assert_eq!(s.head(), Some(expect));
        // the next record continues from the head recovered at open, not from genesis
        let src = s.source_id("a.log").unwrap();
        s.append(src, 1, b"after reopen\n").unwrap();
        assert_eq!(s.head(), Some(link(expect, &digest_of(b"after reopen\n"))));
        s.flush(true).unwrap();
        (s.store_id(), s.genesis(), expect)
    };
    let mut h = sha2::Sha256::new();
    h.update(b"ULPF chain genesis");
    h.update(store_id);
    assert_eq!(genesis, <[u8; 32]>::from(h.finalize()), "genesis is SHA-256(label || store id)");

    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.store_id(), store_id, "the store id survives reopen");
    assert_eq!(r.genesis(), genesis);
    assert_eq!(r.chain(RawId(39)), Some(head_40));
    assert_eq!(r.head(), r.chain(RawId(40)));
    let report = r.verify();
    assert!(report.ok() && report.first_bad.is_none(), "{:?}", report.first_bad);
    assert_eq!(report.checked, 41);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_torn_index_entry_recovers_with_the_chain_intact() {
    let dir = temp("torn");
    fill(&dir, 50);
    // power loss with the index buffer half-drained: the last entry is 17 bytes of 40
    OpenOptions::new().write(true).open(dir.join("raw.idx")).unwrap().set_len(IDX_HEADER + 49 * IDX_ENTRY + 17).unwrap();
    let mut s = RawStore::open(&dir).unwrap();
    assert_eq!(s.len(), 50, "record 49 is complete in the segment and is reindexed");
    let src = s.source_id("a.log").unwrap();
    s.append(src, 1, b"after the crash\n").unwrap();
    s.flush(true).unwrap();
    drop(s);
    let r = RawReader::open(&dir).unwrap();
    let mut expect = r.genesis();
    for i in 0..50u64 {
        expect = link(expect, &digest_of(format!("event {i}\n").as_bytes()));
    }
    assert_eq!(r.chain(RawId(49)), Some(expect), "the reindexed record's chain follows its predecessor");
    let report = r.verify();
    assert!(report.ok(), "{:?} {:?}", report.corrupt, report.first_bad);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An indexed record whose bytes no longer hash to its digest (bit rot, a tamper, a body torn
/// under a complete header) is not recovery's to reclaim: its id may already stand in an
/// output line. It keeps its id and its bytes, the next id is not reissued, and `verify` names
/// it once.
#[test]
fn a_bad_digest_in_the_last_record_survives_recovery_with_its_id() {
    let dir = temp("rot");
    fill(&dir, 50);
    poke(dir.join("raw.seg"), offset_of(&dir, 49) + REC_HEADER + 2, b"X");
    let mut s = RawStore::open(&dir).unwrap();
    assert_eq!(s.len(), 50, "an indexed record is kept whatever its bytes hash to");
    let src = s.source_id("a.log").unwrap();
    assert_eq!(s.append(src, 1, b"after\n").unwrap(), RawId(50), "id 49 is not reissued");
    s.flush(true).unwrap();
    drop(s);
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.get(RawId(49)).unwrap().bytes, b"evXnt 49\n", "the bytes are kept as they are, not zeroed");
    let report = r.verify();
    assert_eq!(report.first_bad, Some((RawId(49), VerifyReason::Digest)));
    assert_eq!(report.corrupt, vec![RawId(49)], "the record appended after it follows the stored chain");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A segment that ends before the records the index names (a crash no run has recovered yet,
/// or a file cut short by hand) is named by `verify`, not trimmed out of the count.
#[test]
fn a_segment_cut_short_is_named_not_trimmed() {
    let dir = temp("cut");
    fill(&dir, 50);
    OpenOptions::new().write(true).open(dir.join("raw.seg")).unwrap().set_len(offset_of(&dir, 45)).unwrap();
    let r = RawReader::open(&dir).unwrap();
    assert_eq!(r.len(), 50, "the index still names 50 records");
    let report = r.verify();
    assert!(!report.ok());
    assert_eq!(report.first_bad, Some((RawId(45), VerifyReason::Digest)));
    assert_eq!(report.corrupt.len(), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The header note names a record the next run will re-index. A record beyond the index whose
/// bytes do not hash to its digest is one recovery reclaims instead, so it is not announced.
#[test]
fn a_torn_record_beyond_the_index_is_not_promised_a_reindex() {
    let dir = temp("torn-beyond");
    fill(&dir, 50);
    let at = offset_of(&dir, 49) + REC_HEADER + 2;
    OpenOptions::new().write(true).open(dir.join("raw.idx")).unwrap().set_len(IDX_HEADER + 49 * IDX_ENTRY).unwrap();
    let notes = |dir: &std::path::Path| ulpf_store::index_header_against_store(&RawReader::open(dir).unwrap()).notes;
    assert_eq!(notes(&dir).len(), 1, "a complete record 49 beyond the index is announced");
    poke(dir.join("raw.seg"), at, b"X");
    assert!(notes(&dir).is_empty(), "a record recovery will not re-index is not announced");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_flipped_byte_is_named_with_reason_digest() {
    let dir = temp("flip");
    fill(&dir, 30);
    poke(dir.join("raw.seg"), offset_of(&dir, 12) + REC_HEADER + 2, b"X");
    let report = RawReader::open(&dir).unwrap().verify();
    assert_eq!(report.first_bad, Some((RawId(12), VerifyReason::Digest)));
    assert_eq!(report.corrupt, vec![RawId(12)], "one break is reported once, not for every later record");
    assert!(!report.ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_consistently_rewritten_digest_is_named_with_reason_chain() {
    let dir = temp("digest");
    fill(&dir, 30);
    rewrite_record(&dir, 12, b"evil 12!\n");
    let report = RawReader::open(&dir).unwrap().verify();
    assert_eq!(report.first_bad, Some((RawId(12), VerifyReason::Chain)), "the record hashes to its digest; the chain does not follow");
    assert_eq!(report.corrupt, vec![RawId(12)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_thorough_rewrite_passes_the_store_and_fails_the_attestation() {
    let dir = temp("thorough");
    fill(&dir, 60);
    let att: Attestation = {
        let r = RawReader::open(&dir).unwrap();
        let a = r.attest();
        assert_eq!(a.format, "ulpf-attestation/1");
        assert_eq!(a.records, 60);
        assert_eq!(a.checkpoints.iter().map(|c| c.id).collect::<Vec<_>>(), vec![0, 59], "every 4096th record plus the last");
        assert_eq!(a.head, ulpf_store::hex(&r.head().unwrap()));
        // a stranger receives it as JSON
        serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap()
    };
    // the attacker rewrites record 20 and recomputes every chain value from 20 on
    let n = 20u64;
    let mut prev = RawReader::open(&dir).unwrap().chain(RawId(n - 1)).unwrap();
    let mut digest = rewrite_record(&dir, n, b"evil 20!\n");
    for i in n..60 {
        if i > n {
            digest = digest_of(format!("event {i}\n").as_bytes());
        }
        prev = link(prev, &digest);
        poke(dir.join("raw.idx"), IDX_HEADER + i * IDX_ENTRY + 8, &prev);
    }
    let r = RawReader::open(&dir).unwrap();
    let store_only = r.verify();
    assert!(store_only.ok(), "a consistent rewrite passes the store-only check: {:?}", store_only.first_bad);
    let against = r.verify_against(&att);
    assert_eq!(against.bad_checkpoint, Some(RawId(59)), "the first checkpoint at or after the rewrite");
    assert!(against.attestation_problem.is_none(), "{:?}", against.attestation_problem);
    assert!(!against.ok());

    // an attestation for a different store is refused rather than silently compared
    let other = temp("other");
    fill(&other, 3);
    let foreign = RawReader::open(&other).unwrap().attest();
    assert!(r.verify_against(&foreign).attestation_problem.is_some());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_pre_chain_index_is_refused_by_name() {
    let dir = temp("old");
    fill(&dir, 5);
    // rebuild raw.idx in the v0.1 format: bare u64 offsets, no header
    let idx = std::fs::read(dir.join("raw.idx")).unwrap();
    let old: Vec<u8> = (0..5u64).flat_map(|i| idx[(IDX_HEADER + i * IDX_ENTRY) as usize..][..8].to_vec()).collect();
    std::fs::write(dir.join("raw.idx"), &old).unwrap();
    let err = match RawStore::open(&dir) {
        Ok(_) => panic!("a pre-chain store must be refused"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("predates the integrity chain"), "{err}");
    let err = match RawReader::open(&dir) {
        Ok(_) => panic!("a pre-chain store must be refused"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("predates the integrity chain"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_writers_reader_snapshot_stops_at_the_records_it_flushed() {
    let dir = temp("snapshot");
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = RawStore::open(&dir).unwrap();
    let src = s.source_id("a.log").unwrap();
    for i in 0..10 {
        s.append(src, 1, format!("event {i}\n").as_bytes()).unwrap();
    }
    let snapshot = s.reader().unwrap();
    assert_eq!(snapshot.len(), 10);
    assert_eq!(snapshot.head(), s.head());
    assert!(snapshot.verify().ok());
    for i in 10..20 {
        s.append(src, 1, format!("event {i}\n").as_bytes()).unwrap();
    }
    assert_eq!(snapshot.len(), 10, "records appended after the snapshot are invisible to it");
    assert_eq!(s.reader().unwrap().len(), 20);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The chain's cost, and a break at record N of a million.
/// `cargo test -p ulpf-store --test chain -- --ignored --nocapture`
#[test]
#[ignore = "1M records; run explicitly"]
fn a_million_appends_and_a_break_in_the_middle() {
    let dir = temp("1m");
    let n: u64 = 1_000_000;
    let line = b"<134>Sep  4 10:15:23 fw01 %ASA-6-302013: Built outbound TCP connection 1 for outside:1.1.1.1/443 to inside:10.0.0.5/51234\n";
    let mut s = RawStore::open(&dir).unwrap();
    let src = s.source_id("a.log").unwrap();
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        s.append(src, 1, line).unwrap();
    }
    s.flush(true).unwrap();
    let append_secs = t0.elapsed().as_secs_f64();
    drop(s);
    // the chain's own share: one SHA-256 over 64 bytes per append
    let d = digest_of(line);
    let mut prev = [0u8; 32];
    let t1 = std::time::Instant::now();
    for _ in 0..n {
        prev = link(prev, &d);
    }
    let chain_secs = t1.elapsed().as_secs_f64();
    std::hint::black_box(prev);
    println!(
        "1M appends: {append_secs:.3} s ({:.0} ns/append); chain links alone {chain_secs:.3} s ({:.0} ns/append, {:.1}% of append)",
        append_secs * 1e9 / n as f64,
        chain_secs * 1e9 / n as f64,
        100.0 * chain_secs / append_secs
    );
    assert_eq!(std::fs::metadata(dir.join("raw.idx")).unwrap().len(), IDX_HEADER + n * IDX_ENTRY, "40 bytes per record plus the header");

    let att = RawReader::open(&dir).unwrap().attest();
    assert_eq!(att.checkpoints.len(), (n / 4096) as usize + 2, "one every 4096 records plus the last");
    let broken = 700_000u64;
    let evil = b"<134>Sep  4 10:15:23 fw01 %ASA-6-302013: Built outbound TCP connection 1 for outside:9.9.9.9/443 to inside:10.0.0.5/51234\n";
    let evil_digest = rewrite_record(&dir, broken, evil);
    let r = RawReader::open(&dir).unwrap();
    let t2 = std::time::Instant::now();
    let report = r.verify_against(&att);
    println!("verify of {n} records: {:.3} s", t2.elapsed().as_secs_f64());
    assert_eq!(report.first_bad, Some((RawId(broken), VerifyReason::Chain)), "the break is named by id");
    assert!(report.bad_checkpoint.is_none(), "the store's own chain already catches this one");
    drop(r);

    // now the thorough attacker: every chain value from the break on, recomputed
    let mut idx = std::fs::read(dir.join("raw.idx")).unwrap();
    let mut prev: [u8; 32] = idx[(IDX_HEADER + (broken - 1) * IDX_ENTRY + 8) as usize..][..32].try_into().unwrap();
    for i in broken..n {
        prev = link(prev, if i == broken { &evil_digest } else { &d });
        idx[(IDX_HEADER + i * IDX_ENTRY + 8) as usize..][..32].copy_from_slice(&prev);
    }
    std::fs::write(dir.join("raw.idx"), &idx).unwrap();
    let r = RawReader::open(&dir).unwrap();
    assert!(r.verify().ok(), "a consistent rewrite passes the store-only check");
    assert_eq!(r.verify_against(&att).bad_checkpoint, Some(RawId(700_416)), "the first checkpoint at or after the break");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Finding 19: each field of the index header is named, and the two states a `kill -9` leaves
/// (a truncated index, a torn entry) are named as the recovery D82 already blesses, not as a
/// rewrite. The full sentence is asserted every time: a message that printed the wrong hex
/// would otherwise still pass.
#[test]
fn each_index_header_field_is_named_when_it_is_rewritten() {
    let good = temp("header");
    fill(&good, 8);
    let clean = header_of(&good);
    assert!(clean.problems.is_empty() && clean.notes.is_empty(), "an untouched store has a clean header: {clean:?}");

    // magic, version and the store id are the fields the header carries; a rewrite of any of
    // them is a problem, and `ulpf verify` exits 1 on it.
    let rewritten = |field: &str, break_it: fn(&std::path::Path), expected: &str| {
        let h = broken(&good, field, break_it);
        assert_eq!(h.problems, vec![expected.to_string()], "{field}");
        assert!(h.notes.is_empty(), "{field}: {h:?}");
    };
    rewritten("magic", |d| poke(d.join("raw.idx"), 0, b"ULPFIDY"), r#"index header: magic says 554c5046494459, the store format writes 554c5046494458 ("ULPFIDX")"#);
    rewritten("version", |d| poke(d.join("raw.idx"), 7, &[9]), "index header: version says 9, this build writes 2");
    rewritten("store-id", |d| poke(d.join("raw.idx"), 8, &[0xff; 16]), "index header: record 0's stored chain does not follow from the store id ffffffffffffffffffffffffffffffff in the header; one of the two was rewritten");

    // The count the header implies is checked against the segment, but a shorter index is the
    // documented power-loss state: a note, never a problem, so verify does not exit 1 on it.
    let recovers = |field: &str, break_it: fn(&std::path::Path), expected: &str| {
        let h = broken(&good, field, break_it);
        assert!(h.problems.is_empty(), "{field}: a truncated index is recovery, not corruption: {h:?}");
        assert!(h.notes.iter().any(|n| n == expected), "{field}: expected {expected:?}, got {h:?}");
    };
    recovers(
        "count",
        truncate_3_entries,
        "index header: the index holds 5 entries, the segment holds a complete record at id 5 the index has not indexed; a run over this store re-indexes it (D82)",
    );
    recovers("partial", truncate_7_bytes, "index header: raw.idx is 337 bytes, 33 past the last whole 40-byte entry; a run over this store reclaims the partial entry (D82)");

    let _ = std::fs::remove_dir_all(&good);
}

/// Both halves of the check for a store on disk, the way `ulpf verify` runs them.
fn header_of(dir: &std::path::Path) -> ulpf_store::IndexHeader {
    let mut h = ulpf_store::index_header(dir).unwrap();
    if h.problems.is_empty() {
        let rest = ulpf_store::index_header_against_store(&RawReader::open(dir).unwrap());
        h.problems.extend(rest.problems);
        h.notes.extend(rest.notes);
    }
    h
}

/// Copies the good store, breaks one header field on the copy, and returns what the check says.
/// The copy is a temp directory: no fixture in the repo is ever damaged.
fn broken(good: &std::path::Path, field: &str, break_it: fn(&std::path::Path)) -> ulpf_store::IndexHeader {
    let dir = temp(&format!("header-{field}"));
    copy_dir(good, &dir);
    break_it(&dir);
    let h = header_of(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    h
}

fn truncate_3_entries(d: &std::path::Path) {
    let idx = std::fs::read(d.join("raw.idx")).unwrap();
    std::fs::write(d.join("raw.idx"), &idx[..idx.len() - 3 * IDX_ENTRY as usize]).unwrap();
}

fn truncate_7_bytes(d: &std::path::Path) {
    let idx = std::fs::read(d.join("raw.idx")).unwrap();
    std::fs::write(d.join("raw.idx"), &idx[..idx.len() - 7]).unwrap();
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

/// Finding 1: `flush` writes the segment before the index, so a record the index has not
/// reached is the ordinary state between two flushes of a live writer -- not evidence of
/// anything. The check must stay silent while a writer holds the store, or `ulpf verify`
/// beside `ulpf serve` exits 1 on an untouched store (D52 permits both at once).
#[test]
fn a_record_ahead_of_the_index_is_silent_while_a_writer_holds_the_store() {
    let dir = temp("header-live");
    fill(&dir, 4);
    let writer = RawStore::open(&dir).unwrap();
    // Cut the last entry behind the writer's back: the shape a segment flushed ahead of the
    // index leaves, produced without racing a real one.
    let idx = std::fs::read(dir.join("raw.idx")).unwrap();
    std::fs::write(dir.join("raw.idx"), &idx[..idx.len() - IDX_ENTRY as usize]).unwrap();
    let h = ulpf_store::index_header_against_store(&RawReader::open(&dir).unwrap());
    assert!(h.problems.is_empty() && h.notes.is_empty(), "a writer holds the store: {h:?}");

    drop(writer);
    let h = ulpf_store::index_header_against_store(&RawReader::open(&dir).unwrap());
    assert!(h.problems.is_empty(), "{h:?}");
    assert_eq!(h.notes.len(), 1, "with no writer the same store is named: {h:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
