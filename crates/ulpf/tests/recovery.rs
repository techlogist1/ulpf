//! A run killed mid-way restarts with no lost and no duplicated event: the store resumes
//! where it ends (completed ingests plus the torn tail), the output loses its torn last
//! line and gains the records the store held but the output lacked, and a fixed
//! `--receipt` makes two runs byte-identical.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ulpf-recovery-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ulpf() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ulpf"));
    c.current_dir(root());
    c
}

fn big_input(dir: &Path, copies: usize) -> PathBuf {
    let mut all = Vec::new();
    for e in std::fs::read_dir(root().join("samples")).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "log") {
            all.extend_from_slice(&std::fs::read(e.path()).unwrap());
            if !all.ends_with(b"\n") {
                all.push(b'\n');
            }
        }
    }
    let mut out = Vec::with_capacity(all.len() * copies);
    for _ in 0..copies {
        out.extend_from_slice(&all);
    }
    let p = dir.join("big.log");
    std::fs::write(&p, out).unwrap();
    p
}

fn raw_ids(output: &Path) -> Vec<u64> {
    let text = std::fs::read_to_string(output).unwrap();
    text.lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap_or_else(|e| panic!("torn or invalid line survived: {e}: {}", &l[..l.len().min(80)]));
            v["ulpf"]["raw_id"].as_u64().unwrap()
        })
        .collect()
}

fn store_len(store: &Path) -> u64 {
    ulpf_store::RawReader::open(store).unwrap().len()
}

#[test]
fn a_killed_run_restarts_without_loss_or_duplication() {
    let dir = tmp("kill");
    let input = big_input(&dir, 120);
    // the clean answer
    let clean = dir.join("clean.jsonl");
    let ok = ulpf().args(["run"]).arg(&input).arg("--store").arg(dir.join("clean-store")).arg("--output").arg(&clean).args(["--infer-threshold", "0", "-j", "2"]).output().unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    let expected = raw_ids(&clean);
    let total = expected.len() as u64;
    assert!(total > 20_000, "{total}");

    // a run that dies mid-way (retry until the kill lands while records exist and the run is unfinished)
    let store = dir.join("store");
    let out = dir.join("out.jsonl");
    let mut partial = false;
    for attempt in 0..6 {
        let _ = std::fs::remove_dir_all(&store);
        let _ = std::fs::remove_file(&out);
        for extra in ["out.v1.meta.json", "out.jsonl.pivot", "out.jsonl.pivot-wal", "out.jsonl.pivot-shm"] {
            let _ = std::fs::remove_file(dir.join(extra));
        }
        let mut child = ulpf().args(["run"]).arg(&input).arg("--store").arg(&store).arg("--output").arg(&out).args(["--infer-threshold", "0", "-j", "2", "--batch", "64"]).stderr(std::process::Stdio::null()).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(120 + 60 * attempt));
        let _ = child.kill();
        let _ = child.wait();
        let stored = ulpf_store::RawReader::open(&store).map(|r| r.len()).unwrap_or(0);
        if stored > 0 && stored < total {
            partial = true;
            eprintln!("attempt {attempt}: killed with {stored} of {total} records stored, output has {} lines", std::fs::read_to_string(&out).map(|t| t.lines().count()).unwrap_or(0));
            break;
        }
    }
    let stored_after_kill = store_len(&store);
    let lines_after_kill = std::fs::read(&out).map(|b| b.iter().filter(|c| **c == b'\n').count() as u64).unwrap_or(0);

    // the restart: same input, same store, same output
    let again = ulpf().args(["run"]).arg(&input).arg("--store").arg(&store).arg("--output").arg(&out).args(["--infer-threshold", "0", "-j", "2", "--batch", "64"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert!(again.status.success(), "{stderr}");
    let ids = raw_ids(&out);
    assert_eq!(ids.len() as u64, total, "output line count equals the clean run ({stderr})");
    assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), ids.len(), "no raw id is emitted twice");
    assert_eq!(ids, expected, "the output is the clean run's, in raw id order");
    assert_eq!(store_len(&store), total, "the store holds every event exactly once");
    let verify = ulpf().args(["verify", "--store"]).arg(&store).output().unwrap();
    assert!(verify.status.success(), "{}", String::from_utf8_lossy(&verify.stdout));
    if partial {
        assert!(stored_after_kill >= lines_after_kill, "the store is flushed per batch, the output is not");
        if stored_after_kill > lines_after_kill {
            assert!(stderr.contains("recovered:"), "the restart reports the recovered records: {stderr}");
        }
        assert!(stderr.contains("input problem") || stderr.contains("recovered:"), "the restart says what it did: {stderr}");
    } else {
        eprintln!("note: no attempt landed mid-run on this machine; consistency still asserted");
    }
}

#[test]
fn a_fixed_receipt_makes_two_runs_byte_identical() {
    let dir = tmp("receipt");
    let mut outs = Vec::new();
    for i in 0..2 {
        let out = dir.join(format!("out{i}.jsonl"));
        let r = ulpf().args(["run", "samples", "--store"]).arg(dir.join(format!("store{i}"))).arg("--output").arg(&out).args(["--infer-threshold", "0", "--receipt", "2026-09-04T12:00:00Z"]).output().unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        outs.push(std::fs::read(&out).unwrap());
    }
    assert_eq!(outs[0], outs[1]);
    assert!(String::from_utf8_lossy(&outs[0]).contains("\"processed_time\":1788523200000"), "receipt pinned to 2026-09-04T12:00:00Z");
}
