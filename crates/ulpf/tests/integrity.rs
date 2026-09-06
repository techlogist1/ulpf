//! Done item 5 end to end through the binary: run, attest, tamper, verify.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ulpf(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ulpf")).args(args).output().expect("running ulpf");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn a_tampered_record_is_named_by_verify_and_exits_1() {
    let dir = std::env::temp_dir().join(format!("ulpf-integrity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = dir.join("store");
    let (ok, out) = ulpf(&[
        "run",
        repo().join("samples").to_str().unwrap(),
        "--store",
        store.to_str().unwrap(),
        "--output",
        dir.join("out.jsonl").to_str().unwrap(),
        "--parsers",
        repo().join("parsers").to_str().unwrap(),
        "--mappings",
        repo().join("mappings").to_str().unwrap(),
        "--infer-threshold",
        "0",
    ]);
    assert!(ok, "run failed:\n{out}");

    let att = dir.join("attestation.json");
    let (ok, out) = ulpf(&["attest", "--store", store.to_str().unwrap(), "--out", att.to_str().unwrap()]);
    assert!(ok, "attest failed:\n{out}");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&att).unwrap()).unwrap();
    assert_eq!(doc["format"], "ulpf-attestation/1");
    assert_eq!(doc["verify"], "ulpf verify --store DIR --attestation FILE");
    let records = doc["records"].as_u64().unwrap();
    assert!(records > 20, "the samples directory should produce records, got {records}");

    let (ok, out) = ulpf(&["verify", "--store", store.to_str().unwrap(), "--attestation", att.to_str().unwrap()]);
    assert!(ok, "verify of an untouched store must exit 0:\n{out}");
    assert!(out.contains("chain ok"), "{out}");
    assert!(out.contains("checkpoints agree"), "{out}");

    // finding 19/D82: cutting the last index entry is the state a `kill -9` leaves. verify
    // says so and still exits 0 -- the operator is told to re-run, not that someone cut the
    // index. (A run over the store re-indexes it, which is what `roundtrip.rs` asserts.)
    let idx = std::fs::read(store.join("raw.idx")).unwrap();
    std::fs::write(store.join("raw.idx"), &idx[..idx.len() - 40]).unwrap();
    let (ok, out) = ulpf(&["verify", "--store", store.to_str().unwrap()]);
    assert!(ok, "a truncated index is recovery, not corruption; verify must exit 0:\n{out}");
    assert!(out.contains("the index has not indexed; a run over this store re-indexes it (D82)"), "{out}");

    // Flip one byte in record 7's payload. raw.idx: 24-byte header, 40 bytes per record
    // (offset, chain); raw.seg record header is 60 bytes.
    let id = 7u64;
    let idx = std::fs::read(store.join("raw.idx")).unwrap();
    let p = (24 + id * 40) as usize;
    let off = u64::from_le_bytes(idx[p..p + 8].try_into().unwrap());
    let mut seg = std::fs::read(store.join("raw.seg")).unwrap();
    let at = (off + 60) as usize;
    seg[at] ^= 0x20;
    std::fs::write(store.join("raw.seg"), &seg).unwrap();

    let (ok, out) = ulpf(&["verify", "--store", store.to_str().unwrap()]);
    assert!(!ok, "verify must exit 1 on a tampered store:\n{out}");
    assert!(out.contains("chain broken at id 7 (digest)"), "{out}");
    assert!(out.contains("verified"), "{out}");

    // and `ulpf raw 7` still shows the exact bytes that are now in the store
    let (ok, raw) = ulpf(&["raw", "7", "--store", store.to_str().unwrap()]);
    assert!(ok, "{raw}");

    // finding 19: the index header is named by field before the digests run, instead of the
    // store being refused as pre-chain by the open.
    let mut idx = std::fs::read(store.join("raw.idx")).unwrap();
    idx[6] = b'Y';
    std::fs::write(store.join("raw.idx"), &idx).unwrap();
    let (ok, out) = ulpf(&["verify", "--store", store.to_str().unwrap()]);
    assert!(!ok, "verify must exit 1 on a rewritten index header:\n{out}");
    assert!(out.contains(r#"index header: magic says 554c5046494459, the store format writes 554c5046494458 ("ULPFIDX")"#), "{out}");
    // and the sentence the open has always printed for a store this old is still printed
    assert!(out.contains("predates the integrity chain"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
