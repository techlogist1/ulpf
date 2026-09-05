//! v2 done-item 3: the entity index, and the normalizer's entity/provenance reporting it
//! is fed from. Every sample event goes through the production pipeline; the postings are
//! built the way the engine's output thread will build them (this file is the worked
//! example for that wiring).

use std::path::PathBuf;

use ulpf::pipeline::Pipeline;
use ulpf::pivot::{Order, PivotIndex, PivotQuery, PivotWriter, Posting, index_path, rebuild};
use ulpf_normalize::{EntityKind, Mapping};
use ulpf_parse::Parsed;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ulpf-pivot-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pipeline() -> Pipeline {
    let (p, problems) = Pipeline::load(&repo().join("parsers"), &repo().join("mappings"), None, 0).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    p
}

fn samples() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo().join("samples"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "log"))
        .collect();
    v.sort();
    v
}

/// The value at a dotted path in an emitted line, as text.
fn at(v: &serde_json::Value, path: &str) -> Option<String> {
    let mut cur = v;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    match cur {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// Everything one emitted event carries that the output thread needs for its postings.
struct Emitted {
    line: serde_json::Value,
    raw_id: u64,
    time_ms: i64,
    parser: Option<String>,
    source: String,
    /// (kind, value) pairs, from `NormalizeStats::entities`.
    entities: Vec<(EntityKind, String)>,
    offset: u64,
    len: u32,
}

/// Runs every sample through the pipeline exactly as a worker does, and turns each event's
/// `NormalizeStats::entities` into the (kind, value) pairs the index stores. This is the
/// whole contract between the normalizer and the index.
fn run_samples(out: &mut Vec<u8>) -> Vec<Emitted> {
    let pipeline = pipeline();
    let mapping = &pipeline.mapping;
    let entity_paths = mapping.entities();
    let mut scratch = pipeline.registry.scratch();
    let mut events = Vec::new();
    let mut raw_id = 0u64;
    for sample in samples() {
        let bytes = std::fs::read(&sample).unwrap();
        let name = sample.file_name().unwrap().to_string_lossy().into_owned();
        let mut hint = None;
        let mut parsed = Parsed::default();
        for range in ulpf_store::Framer::new(&bytes, true) {
            let offset = out.len() as u64;
            let outcome = pipeline.process(&bytes[range], raw_id, &name, 1_700_000_000_000_000_000, &mut hint, &mut scratch, &mut parsed, out);
            let len = (out.len() as u64 - offset - 1) as u32; // without the newline
            let line: serde_json::Value = serde_json::from_slice(&out[offset as usize..out.len() - 1]).unwrap();
            let mut entities = Vec::new();
            for kind in EntityKind::ALL {
                let Some(i) = outcome.stats.entities[kind as usize] else { continue };
                let f = &parsed.fields[i as usize];
                // the index stores the emitted (normalized) value at the entity path
                let path = entity_paths.path(kind).unwrap();
                let value = at(&line, path).unwrap_or_else(|| String::from_utf8_lossy(&f.value).into_owned());
                entities.push((kind, value));
            }
            events.push(Emitted {
                raw_id,
                time_ms: line.get("time").and_then(serde_json::Value::as_i64).unwrap_or(0),
                parser: line.pointer("/ulpf/parser").and_then(serde_json::Value::as_str).map(str::to_owned),
                source: name.clone(),
                line,
                entities,
                offset,
                len,
            });
            raw_id += 1;
        }
    }
    events
}

fn device_of(e: &Emitted, mapping: &Mapping) -> String {
    mapping
        .entities()
        .path(EntityKind::Device)
        .and_then(|p| at(&e.line, p))
        .unwrap_or_else(|| e.source.clone())
}

#[test]
fn entities_are_declared_by_the_mapping_and_validated_at_compile() {
    let m = pipeline().mapping;
    let e = m.entities();
    assert_eq!(e.src_ip.as_deref(), Some("src_endpoint.ip"));
    assert_eq!(e.dst_ip.as_deref(), Some("dst_endpoint.ip"));
    assert_eq!(e.user.as_deref(), Some("user.name"));
    assert_eq!(e.dst_port.as_deref(), Some("dst_endpoint.port"));
    assert_eq!(e.device.as_deref(), Some("device.hostname"));
    for k in EntityKind::ALL {
        assert_eq!(EntityKind::from_name(k.name()), Some(k));
        assert_eq!(EntityKind::from_index(k as usize), Some(k));
    }
    assert_eq!(EntityKind::from_name("srcip"), None);

    let bad = "[schema]\nname = \"x\"\n[fields]\n\"a.b\" = [\"k\"]\n[entities]\nsrc_ip = \"a.typo\"\n";
    let err = match Mapping::compile(toml::from_str(bad).unwrap()) {
        Err(e) => e,
        Ok(_) => panic!("[entities] accepted a path nothing can set"),
    };
    assert!(err.contains("[entities] src_ip") && err.contains("a.typo"), "{err}");

    // an unknown key in [entities] is rejected by the format, not silently kept
    assert!(toml::from_str::<ulpf_normalize::MappingFile>("[schema]\nname = \"x\"\n[entities]\nsrc_mac = \"a.b\"\n").is_err());

    let ok = "[schema]\nname = \"x\"\n[fields]\n\"a.b\" = [\"k\"]\n[entities]\nsrc_ip = \"a.b\"\ndevice = \"activity_name\"\n[default_class]\nuid = 0\nname = \"n\"\ncategory_uid = 0\ncategory_name = \"c\"\nconstants = { activity_name = \"Traffic\" }\n";
    let m = Mapping::compile(toml::from_str(ok).unwrap()).unwrap();
    // a path only a constant sets is legal and simply never has a source field
    assert_eq!(m.entities().device.as_deref(), Some("activity_name"));
}

#[test]
fn normalize_reports_the_source_field_behind_every_entity() {
    let pipeline = pipeline();
    let mapping = &pipeline.mapping;
    let mut scratch = pipeline.registry.scratch();
    let mut checked = 0;
    let mut seen = [0u32; 5];
    for sample in samples() {
        let bytes = std::fs::read(&sample).unwrap();
        let name = sample.file_name().unwrap().to_string_lossy().into_owned();
        let mut hint = None;
        let mut parsed = Parsed::default();
        for (i, range) in ulpf_store::Framer::new(&bytes, true).enumerate() {
            let mut out = Vec::new();
            let outcome = pipeline.process(&bytes[range], i as u64, &name, 1_700_000_000_000_000_000, &mut hint, &mut scratch, &mut parsed, &mut out);
            let line: serde_json::Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
            for kind in EntityKind::ALL {
                let path = mapping.entities().path(kind).unwrap();
                match outcome.stats.entities[kind as usize] {
                    Some(fi) => {
                        let f = &parsed.fields[fi as usize];
                        let emitted = at(&line, path).unwrap_or_else(|| panic!("{}: entity {kind} points at {fi} but {path} is absent", sample.display()));
                        let raw = String::from_utf8_lossy(&f.value).into_owned();
                        assert_eq!(emitted, raw, "{}: {kind} came from `{}`", sample.display(), String::from_utf8_lossy(&f.key));
                        seen[kind as usize] += 1;
                    }
                    // no source field fed the path, so the path must be absent from the line
                    None => assert!(at(&line, path).is_none(), "{}: {kind} at {path} was emitted with no source field", sample.display()),
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 500, "{checked}");
    for kind in EntityKind::ALL {
        assert!(seen[kind as usize] > 0, "no sample event carried a {kind}");
    }
}

#[test]
fn provenance_agrees_with_the_line_normalize_emitted() {
    let pipeline = pipeline();
    let mapping = &pipeline.mapping;
    let mut scratch = pipeline.registry.scratch();
    let enum_paths: Vec<String> = mapping.file().enums.iter().map(|e| e.field.clone()).collect();
    let int_paths = mapping.file().types.int.clone();
    let mut entries = 0;
    let mut canonical = 0;
    for sample in samples() {
        let bytes = std::fs::read(&sample).unwrap();
        let name = sample.file_name().unwrap().to_string_lossy().into_owned();
        let mut hint = None;
        let mut parsed = Parsed::default();
        for (i, range) in ulpf_store::Framer::new(&bytes, true).enumerate() {
            let mut out = Vec::new();
            pipeline.process(&bytes[range], i as u64, &name, 1_700_000_000_000_000_000, &mut hint, &mut scratch, &mut parsed, &mut out);
            let line: serde_json::Value = serde_json::from_slice(&out[..out.len() - 1]).unwrap();
            for p in mapping.provenance(&parsed) {
                let emitted = at(&line, &p.path).unwrap_or_else(|| panic!("{}: provenance names {} but the line has no such path", sample.display(), p.path));
                assert_eq!(emitted, p.value, "{}: {} value disagrees", sample.display(), p.path);
                // canonical is exactly "the mapping rewrote it", i.e. this path is an enum
                assert_eq!(p.canonical, enum_paths.contains(&p.path), "{}: {} canonical flag", sample.display(), p.path);
                // a non-enum value is the source text itself, unless `[types] int` retyped it
                if !p.canonical && !int_paths.contains(&p.path) {
                    let src = &parsed.fields[p.field_index as usize];
                    assert_eq!(String::from_utf8_lossy(&src.value), p.value, "{}: {} is not the source value", sample.display(), p.path);
                }
                entries += 1;
                canonical += p.canonical as u32;
            }
        }
    }
    assert!(entries > 1000, "{entries}");
    assert!(canonical > 0, "no value was canonicalised in any sample");
}

#[test]
fn writer_and_reader_round_trip_and_rebuild_matches_the_live_index() {
    let dir = temp("roundtrip");
    let output = dir.join("out.jsonl");
    let mut buf = Vec::new();
    let events = run_samples(&mut buf);
    std::fs::write(&output, &buf).unwrap();
    let mapping = pipeline().mapping;

    let mut writer = PivotWriter::start(&output, 8).unwrap();
    let counters = writer.counters();
    let devices: Vec<String> = events.iter().map(|e| device_of(e, &mapping)).collect();
    for (batch, chunk) in events.chunks(64).enumerate() {
        let mut postings = Vec::new();
        for (i, e) in chunk.iter().enumerate() {
            let device = &devices[batch * 64 + i];
            for (kind, value) in &e.entities {
                postings.push(Posting {
                    raw_id: e.raw_id,
                    time_ms: e.time_ms,
                    kind: *kind,
                    value: value.as_bytes(),
                    device: device.as_bytes(),
                    parser: e.parser.as_deref(),
                    offset: e.offset,
                    len: e.len,
                });
            }
        }
        // the device kind falls back to the ingest source name, so every event has one
        writer.push_batch(&postings);
    }
    writer.finish();
    let expected_postings: u64 = events.iter().map(|e| e.entities.len() as u64).sum();
    assert_eq!(counters.postings.load(std::sync::atomic::Ordering::Relaxed), expected_postings);
    assert_eq!(counters.errors.load(std::sync::atomic::Ordering::Relaxed), 0);

    // the busiest src_ip: total is exact, the timeline reads its lines back from the output
    let index = PivotIndex::open(&output).unwrap();
    let top = index.entities(Some(EntityKind::SrcIp), "", 5).unwrap();
    assert!(!top.is_empty());
    let busiest = top[0].value.clone();
    let page = index
        .query(&PivotQuery { kind: EntityKind::SrcIp, value: busiest.as_bytes(), limit: 200, before: None, before_id: None, after: None, after_id: None, order: Order::Desc })
        .unwrap();
    assert_eq!(page.total, top[0].events);
    assert_eq!(page.value, busiest);
    assert!(!page.devices.is_empty());
    assert!(page.devices.iter().any(|d| !d.parsers.is_empty()));
    assert!(!page.events.is_empty());
    for e in &page.events {
        assert_eq!(at(&e.line, "src_endpoint.ip").as_deref(), Some(busiest.as_str()));
        assert_eq!(e.line.pointer("/ulpf/raw_id").and_then(serde_json::Value::as_u64), Some(e.raw_id));
    }
    // newest first
    let times: Vec<i64> = page.events.iter().map(|e| e.time).collect();
    assert!(times.windows(2).all(|w| w[0] >= w[1]), "{times:?}");
    assert!(page.related.values().any(|v| !v.is_empty()), "no co-occurring entity found");
    assert!(page.related_over > 0);

    // prefix search
    let pre = &busiest[..busiest.len().min(3)];
    let hits = index.entities(Some(EntityKind::SrcIp), pre, 100).unwrap();
    assert!(hits.iter().any(|h| h.value == busiest), "prefix `{pre}` did not find {busiest}");
    assert!(hits.iter().all(|h| h.value.starts_with(pre)));

    // an unknown value is an empty page, not an error
    let empty = index
        .query(&PivotQuery { kind: EntityKind::User, value: b"nobody@nowhere", limit: 10, before: None, before_id: None, after: None, after_id: None, order: Order::Desc })
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.events.is_empty());

    // rebuild from the output alone reproduces the same index
    let live: Vec<(String, String, u64, u64)> = summary(&index);
    drop(index);
    let report = rebuild(&output, &mapping, 1024).unwrap();
    assert_eq!(report.events, events.len() as u64);
    let rebuilt = PivotIndex::open(&output).unwrap();
    assert_eq!(summary(&rebuilt), live);
    assert_eq!(report.postings, expected_postings, "rebuild derived a different number of postings");
}

fn summary(index: &PivotIndex) -> Vec<(String, String, u64, u64)> {
    let mut all: Vec<(String, String, u64, u64)> = Vec::new();
    for kind in EntityKind::ALL {
        for e in index.entities(Some(kind), "", 1000).unwrap() {
            all.push((kind.name().to_owned(), e.value, e.events, e.devices));
        }
    }
    all.sort();
    all
}

/// The bound the API promises: an entity with a million postings answers a 200-event page
/// without reading the posting list whole.
#[test]
#[ignore = "timing; run with --ignored"]
fn a_million_postings_answer_a_page_in_bounded_time() {
    let dir = temp("million");
    let output = dir.join("out.jsonl");
    std::fs::write(&output, b"{}\n").unwrap();
    let mut writer = PivotWriter::start(&output, 32).unwrap();
    let started = std::time::Instant::now();
    let batch = 1024u64;
    let n: u64 = 1_000_000 - 1_000_000 % batch; // whole batches, as the engine sends them
    let mut raw_id = 0u64;
    while raw_id + batch <= n {
        let mut owned: Vec<(String, String)> = Vec::new();
        for i in 0..batch {
            // one hot entity in every event, plus a spread of others: typical cardinality
            owned.push((format!("10.0.0.{}", (raw_id + i) % 251), format!("dev{}", (raw_id + i) % 7)));
        }
        let mut postings = Vec::with_capacity(batch as usize * 2);
        for (i, (dst, dev)) in owned.iter().enumerate() {
            let id = raw_id + i as u64;
            postings.push(Posting {
                raw_id: id,
                time_ms: 1_700_000_000_000 + id as i64,
                kind: EntityKind::SrcIp,
                value: b"10.1.1.1",
                device: dev.as_bytes(),
                parser: Some("cisco_asa"),
                offset: 0,
                len: 2,
            });
            postings.push(Posting {
                raw_id: id,
                time_ms: 1_700_000_000_000 + id as i64,
                kind: EntityKind::DstIp,
                value: dst.as_bytes(),
                device: dev.as_bytes(),
                parser: Some("cisco_asa"),
                offset: 0,
                len: 2,
            });
        }
        writer.push_batch(&postings);
        raw_id += batch;
    }
    let counters = writer.counters();
    writer.finish();
    let wrote = started.elapsed();
    let postings = counters.postings.load(std::sync::atomic::Ordering::Relaxed);
    let size = std::fs::metadata(index_path(&output)).unwrap().len();
    eprintln!(
        "wrote {postings} postings for {n} events in {:.2} s = {:.0} postings/s, {:.0} events/s of index work; {} bytes = {:.1} bytes/event",
        wrote.as_secs_f64(),
        postings as f64 / wrote.as_secs_f64(),
        n as f64 / wrote.as_secs_f64(),
        size,
        size as f64 / n as f64
    );

    let index = PivotIndex::open(&output).unwrap();
    let t = std::time::Instant::now();
    let page = index
        .query(&PivotQuery { kind: EntityKind::SrcIp, value: b"10.1.1.1", limit: 200, before: None, before_id: None, after: None, after_id: None, order: Order::Desc })
        .unwrap();
    let elapsed = t.elapsed();
    eprintln!("page of {} of {} events, related_over {}, in {:.3} s", page.events.len(), page.total, page.related_over, elapsed.as_secs_f64());
    assert_eq!(page.total, n);
    assert_eq!(page.events.len(), 200);
    assert!(elapsed.as_secs_f64() < 1.0, "a 200-event page took {elapsed:?}");
    assert!(page.related["dst_ip"].len() == 10);
}
