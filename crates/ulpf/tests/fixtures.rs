//! Done-item 4 harness: every `fixtures/<parser>.expected.jsonl` is run against
//! `samples/<parser>.log` through the production pipeline. Reports every mismatch with
//! file and line, then fails once.

use std::path::PathBuf;

use ulpf::fixture::{Expected, compare, run_event};
use ulpf::pipeline::Pipeline;
use ulpf_parse::Parsed;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_fixture_matches_its_sample() {
    let (pipeline, problems) = Pipeline::load(&repo().join("parsers"), &repo().join("mappings"), None, 0).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(repo().join("fixtures"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".expected.jsonl"))
        .collect();
    fixtures.sort();
    assert!(!fixtures.is_empty());
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut covered = Vec::new();
    for fx in &fixtures {
        let stem = fx.file_name().unwrap().to_string_lossy().trim_end_matches(".expected.jsonl").to_owned();
        let sample = repo().join("samples").join(format!("{stem}.log"));
        let Ok(bytes) = std::fs::read(&sample) else {
            failures.push(format!("{}: no sample at {}", fx.display(), sample.display()));
            continue;
        };
        let events: Vec<&[u8]> = ulpf_store::Framer::new(&bytes, true).map(|r| &bytes[r]).collect();
        let text = std::fs::read_to_string(fx).unwrap();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).collect();
        if lines.len() != events.len() {
            failures.push(format!("{}: {} fixture lines but {} events in {}", fx.display(), lines.len(), events.len(), sample.display()));
        }
        let mut scratch = pipeline.registry.scratch();
        for (i, (line, event)) in lines.iter().zip(&events).enumerate() {
            let exp: Expected = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(e) => {
                    failures.push(format!("{}:{}: invalid fixture JSON: {e}", fx.display(), i + 1));
                    continue;
                }
            };
            let mut parsed = Parsed::default();
            let act = run_event(&pipeline, event, i as u64, &format!("{stem}.log"), &mut scratch, &mut parsed).unwrap();
            for err in compare(&exp, &act) {
                failures.push(format!("{}:{}: {err}", fx.display(), i + 1));
            }
            if let Some(p) = &exp.parser
                && p != "none"
                && !covered.contains(p)
            {
                covered.push(p.clone());
            }
            checked += 1;
        }
    }
    assert!(failures.is_empty(), "{} fixture mismatches:\n{}", failures.len(), failures.join("\n"));
    assert!(checked > 0);
    // every parser definition must be covered by a fixture
    for p in pipeline.registry.iter() {
        assert!(covered.contains(&p.name().to_owned()), "parser `{}` has no fixture asserting it", p.name());
    }
}
