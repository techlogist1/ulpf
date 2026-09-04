//! One event through detect → parse → normalize. The engine workers and the fixture
//! harness both call `Pipeline::process`, so a fixture proves the production path.

use std::path::Path;

use anyhow::{Context as _, Result};
use ulpf_normalize::{Mapping, NormalizeStats, Provenance};
use ulpf_parse::{Context, ParseFailure, Parsed, Registry, Scratch, SubStatus};

pub struct Pipeline {
    pub registry: Registry,
    pub mapping: Mapping,
    pub default_offset_secs: i32,
}

pub struct Outcome {
    pub parser: Option<usize>,
    pub parse: Result<(), ParseFailure>,
    pub sub: SubStatus,
    pub time_error: Option<&'static str>,
    pub stats: NormalizeStats,
}

pub fn sub_status_name(s: SubStatus) -> &'static str {
    match s {
        SubStatus::NotApplicable => "not_applicable",
        SubStatus::Matched => "matched",
        SubStatus::NoMatch => "no_match",
    }
}

impl Pipeline {
    /// Loads `parsers_dir` and `mappings_dir`. Per-file problems come back as strings and
    /// never prevent the rest from loading; only a missing directory or no usable mapping
    /// is fatal.
    pub fn load(parsers_dir: &Path, mappings_dir: &Path, schema: Option<&str>, default_offset_secs: i32) -> Result<(Pipeline, Vec<String>)> {
        let mut problems = Vec::new();
        let parsers = ulpf_parse::load_dir(parsers_dir).with_context(|| format!("parsers directory {}", parsers_dir.display()))?;
        problems.extend(parsers.errors.iter().map(|e| format!("parser: {e}")));
        let mut maps = ulpf_normalize::load_dir(mappings_dir).with_context(|| format!("mappings directory {}", mappings_dir.display()))?;
        problems.extend(maps.errors.iter().map(|e| format!("mapping: {e}")));
        let idx = match schema {
            Some(name) => maps.mappings.iter().position(|m| m.schema_name() == name).with_context(|| format!("no mapping named `{name}` in {}", mappings_dir.display()))?,
            None => {
                anyhow::ensure!(!maps.mappings.is_empty(), "no usable mapping in {}", mappings_dir.display());
                0
            }
        };
        let mapping = maps.mappings.swap_remove(idx);
        Ok((Pipeline { registry: Registry::new(parsers.parsers), mapping, default_offset_secs }, problems))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process<'a>(
        &'a self,
        event: &'a [u8],
        raw_id: u64,
        source: &str,
        receipt_nanos: i64,
        hint: &mut Option<usize>,
        scratch: &mut Scratch,
        parsed: &mut Parsed<'a>,
        out: &mut Vec<u8>,
    ) -> Outcome {
        let ctx = Context { receipt_epoch_nanos: receipt_nanos, default_offset_secs: self.default_offset_secs };
        let idx = self.registry.detect(event, *hint);
        let (parse, status, vendor, product, name) = match idx {
            Some(i) => {
                *hint = Some(i);
                let p = self.registry.get(i);
                let r = p.parse(event, &ctx, scratch, parsed);
                let status = match r {
                    Ok(()) => "parsed",
                    Err(e) => e.reason(),
                };
                let d = p.definition();
                (r, status, Some(d.parser.vendor.as_str()), Some(d.parser.product.as_str()), Some(p.name()))
            }
            None => {
                parsed.clear();
                let mut body = event;
                while let Some((&last, rest)) = body.split_last() {
                    if last == b'\n' || last == b'\r' {
                        body = rest;
                    } else {
                        break;
                    }
                }
                parsed.push(&b"raw_message"[..], body);
                (Ok(()), "no_parser", None, None, None)
            }
        };
        let prov = Provenance {
            raw_id,
            source,
            parser: name,
            vendor,
            product,
            receipt_nanos,
            parse_status: status,
            sub_status: sub_status_name(parsed.sub),
        };
        let stats = self.mapping.normalize(parsed, &prov, out);
        Outcome { parser: idx, parse, sub: parsed.sub, time_error: parsed.timestamp_error, stats }
    }
}
