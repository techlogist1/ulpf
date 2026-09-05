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
