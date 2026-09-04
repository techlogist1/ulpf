//! Fixture file format (`fixtures/<parser>.expected.jsonl`): one JSON object per sample
//! event, in sample order. Every key present in the fixture is asserted; keys absent are
//! not. `fields` and `normalized` are subsets. The receipt time is fixed so fixtures are
//! reproducible: 2026-09-04T12:00:00Z.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ulpf_parse::{Parsed, Scratch};

use crate::pipeline::{Pipeline, sub_status_name};

pub const FIXTURE_RECEIPT_NANOS: i64 = 1_788_523_200_000_000_000;

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    /// Parser name, or "none" when no parser should match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parser: Option<String>,
    /// `parsed`, `no_parser`, or a parse failure reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// `not_applicable`, `matched`, `no_match`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// Parsed fields that must be present with exactly these values.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
    /// Parsed field names that must be absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
    /// Normalized values by dotted path.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub normalized: Map<String, Value>,
    /// Event time in epoch milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_policies: Option<Vec<String>>,
}

pub struct Actual {
    pub parser: String,
    pub status: String,
    pub sub: String,
    pub fields: Vec<(String, String)>,
    pub normalized: Value,
}

pub fn run_event<'a>(pipeline: &'a Pipeline, event: &'a [u8], raw_id: u64, source: &str, scratch: &mut Scratch, parsed: &mut Parsed<'a>) -> Result<Actual> {
    let mut out = Vec::new();
    let mut hint = None;
    let outcome = pipeline.process(event, raw_id, source, FIXTURE_RECEIPT_NANOS, &mut hint, scratch, parsed, &mut out);
    let normalized: Value = serde_json::from_slice(&out)?;
    let parser = outcome.parser.map(|i| pipeline.registry.get(i).name().to_owned()).unwrap_or_else(|| "none".into());
    let status = match (outcome.parser, outcome.parse) {
        (None, _) => "no_parser".to_owned(),
        (Some(_), Ok(())) => "parsed".to_owned(),
        (Some(_), Err(e)) => e.reason().to_owned(),
    };
    let fields = parsed.fields.iter().map(|f| (String::from_utf8_lossy(&f.key).into_owned(), String::from_utf8_lossy(&f.value).into_owned())).collect();
    Ok(Actual { parser, status, sub: sub_status_name(outcome.sub).to_owned(), fields, normalized })
}

pub fn lookup<'v>(v: &'v Value, path: &str) -> Option<&'v Value> {
    let mut cur = v;
    for p in path.split('.') {
        cur = cur.get(p)?;
    }
    Some(cur)
}

/// Compares one fixture line against the actual outcome; returns human-readable mismatches.
pub fn compare(exp: &Expected, act: &Actual) -> Vec<String> {
    let mut errs = Vec::new();
    if let Some(p) = &exp.parser
        && *p != act.parser
    {
        errs.push(format!("parser: expected {p}, got {}", act.parser));
    }
    if let Some(s) = &exp.status
        && *s != act.status
    {
        errs.push(format!("status: expected {s}, got {}", act.status));
    }
    if let Some(s) = &exp.sub
        && *s != act.sub
    {
        errs.push(format!("sub: expected {s}, got {}", act.sub));
    }
    for (k, v) in &exp.fields {
        let want = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        match act.fields.iter().find(|(ak, _)| ak == k) {
            Some((_, got)) if *got == want => {}
            Some((_, got)) => errs.push(format!("field {k}: expected {want:?}, got {got:?}")),
            None => errs.push(format!("field {k}: missing")),
        }
    }
    for k in &exp.absent {
        if act.fields.iter().any(|(ak, _)| ak == k) {
            errs.push(format!("field {k}: expected absent"));
        }
    }
    for (path, want) in &exp.normalized {
        match lookup(&act.normalized, path) {
            Some(got) if got == want => {}
            Some(got) => errs.push(format!("normalized {path}: expected {want}, got {got}")),
            None => errs.push(format!("normalized {path}: missing")),
        }
    }
    if let Some(t) = exp.time {
        match lookup(&act.normalized, "time").and_then(Value::as_i64) {
            Some(got) if got == t => {}
            got => errs.push(format!("time: expected {t}, got {got:?}")),
        }
    }
    if let Some(p) = &exp.time_policies {
        let got: Vec<String> = lookup(&act.normalized, "ulpf.time_policies").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        if got != *p {
            errs.push(format!("time_policies: expected {p:?}, got {got:?}"));
        }
    }
    errs
}

const SKELETON_PATHS: [&str; 16] = [
    "class_uid", "class_name", "action", "action_id", "severity", "src_endpoint.ip", "src_endpoint.port",
    "dst_endpoint.ip", "dst_endpoint.port", "connection_info.protocol_name", "user.name", "device.hostname",
    "firewall_rule.name", "finding_info.title", "http_request.url.text", "metadata.event_code",
];

/// A complete fixture line for one event: all fields, a useful normalized subset, time.
pub fn skeleton(pipeline: &Pipeline, event: &[u8], raw_id: u64, source: &str, scratch: &mut Scratch) -> Result<String> {
    let mut parsed = Parsed::default();
    let act = run_event(pipeline, event, raw_id, source, scratch, &mut parsed)?;
    let mut exp = Expected { parser: Some(act.parser.clone()), status: Some(act.status.clone()), sub: Some(act.sub.clone()), ..Default::default() };
    for (k, v) in &act.fields {
        if k != "raw_message" {
            exp.fields.insert(k.clone(), Value::String(v.clone()));
        }
    }
    for p in SKELETON_PATHS {
        if let Some(v) = lookup(&act.normalized, p) {
            exp.normalized.insert(p.to_owned(), v.clone());
        }
    }
    exp.time = lookup(&act.normalized, "time").and_then(Value::as_i64);
    exp.time_policies = lookup(&act.normalized, "ulpf.time_policies").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect());
    Ok(serde_json::to_string(&exp)?)
}
