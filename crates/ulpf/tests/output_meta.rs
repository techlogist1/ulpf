//! The version meta beside the output agrees with the run (D101): an output that is the
//! null device leaves no meta, no index and nothing in the working directory, and
//! `out.v1.meta.json` counts the lines the file holds, which is what the run emitted when
//! the file started empty.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ulpf-meta-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A run from `cwd`; every other path is absolute, so anything relative that appears is a bug.
fn run(cwd: &Path, inputs: &[PathBuf], store: &Path, output: &str, report: &Path) -> serde_json::Value {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ulpf"));
    c.current_dir(cwd).arg("run").args(inputs);
    c.arg("--store").arg(store).arg("--output").arg(output).arg("--report-json").arg(report);
    c.arg("--parsers").arg(root().join("parsers")).arg("--mappings").arg(root().join("mappings"));
    c.args(["--infer-threshold", "0", "--receipt", "2026-09-06T00:00:00Z"]);
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn lines(path: &Path) -> u64 {
    std::fs::read(path).unwrap().iter().filter(|b| **b == b'\n').count() as u64
}

#[test]
fn a_null_device_output_leaves_nothing_beside_it_or_in_the_cwd() {
    let dir = tmp("null");
    let cwd = dir.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let device = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let report = run(&cwd, &[root().join("samples")], &dir.join("store"), device, &dir.join("report.json"));
    assert!(report["emitted"].as_u64().unwrap() > 0, "{report}");
    assert_eq!(names(&cwd), Vec::<String>::new(), "a device output has nothing beside it, so nothing may land in the working directory");
    let store = names(&dir.join("store"));
    assert!(store.iter().all(|n| n.starts_with("raw.") || n.starts_with("catalog.sqlite")), "only the store in the store directory: {store:?}");
    assert_eq!(names(&dir), vec!["cwd", "report.json", "store"], "no version meta, no entity index anywhere");
}

#[test]
fn the_version_meta_counts_the_lines_the_output_holds() {
    let dir = tmp("count");
    let cwd = dir.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let output = dir.join("o.jsonl");
    let meta_path = dir.join("o.v1.meta.json");
    let store = dir.join("store");
    let events = |m: &Path| serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(m).unwrap()).unwrap()["events"].as_u64().unwrap();

    // a fresh output: the meta counts what the counter block says was emitted
    let r1 = run(&cwd, &[root().join("samples")], &store, output.to_str().unwrap(), &dir.join("r1.json"));
    let emitted1 = r1["emitted"].as_u64().unwrap();
    assert!(emitted1 > 0);
    assert_eq!(lines(&output), emitted1);
    assert_eq!(events(&meta_path), emitted1, "the meta records what the run emitted");

    // the same inputs again: nothing new is emitted, and the meta still says what the file holds
    let r2 = run(&cwd, &[root().join("samples")], &store, output.to_str().unwrap(), &dir.join("r2.json"));
    assert_eq!(r2["emitted"].as_u64().unwrap(), 0, "{r2}");
    assert_eq!(events(&meta_path), emitted1, "a run that appended nothing leaves the count as the file's");

    // more input appended to the same output: the meta is the whole file, not this run alone
    let more = dir.join("more");
    std::fs::create_dir_all(&more).unwrap();
    std::fs::copy(root().join("heldout/mikrotik.log"), more.join("gw.log")).unwrap();
    let r3 = run(&cwd, &[root().join("samples"), more], &store, output.to_str().unwrap(), &dir.join("r3.json"));
    let emitted3 = r3["emitted"].as_u64().unwrap();
    assert!(emitted3 > 0);
    assert_eq!(lines(&output), emitted1 + emitted3);
    assert_eq!(events(&meta_path), emitted1 + emitted3, "counted from the file when the output was not empty at start");
    assert_eq!(names(&cwd), Vec::<String>::new());
}
