//! A `Parser` is a definition compiled once (regexes, finders, timestamp formats) and
//! shared read-only across threads. Per-thread mutable state lives in `Scratch`.

use std::borrow::Cow;

use regex::bytes::CaptureLocations;
use ulpf_time::{Context, Format};

use crate::def::{Anchor, OneOrMany, ParserDefinition, Strategy, StrategyKind};
use crate::delimiter::DelimConfig;
use crate::detect::CompiledMatcher;
use crate::kv::KvConfig;
use crate::pattern::CompiledPattern;
use crate::template::Template;
use crate::structured::StructuredScratch;
use crate::{Parsed, envelope, structured};

/// Why a strategy produced nothing. Each variant is a counter reason, never a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseFailure {
    PatternNoMatch,
    NoPairs,
    NoColumns,
    InvalidJson,
    InvalidCef,
    InvalidLeef,
    /// Not UTF-8, no element, or the tokenizer stopped (unterminated tag, stray `<`).
    InvalidXml,
}

impl ParseFailure {
    pub const ALL: [ParseFailure; 7] = [
        ParseFailure::PatternNoMatch, ParseFailure::NoPairs, ParseFailure::NoColumns,
        ParseFailure::InvalidJson, ParseFailure::InvalidCef, ParseFailure::InvalidLeef,
        ParseFailure::InvalidXml,
    ];
    pub fn reason(self) -> &'static str {
        match self {
            ParseFailure::PatternNoMatch => "pattern_no_match",
            ParseFailure::NoPairs => "no_pairs",
            ParseFailure::NoColumns => "no_columns",
            ParseFailure::InvalidJson => "invalid_json",
            ParseFailure::InvalidCef => "invalid_cef",
            ParseFailure::InvalidLeef => "invalid_leef",
            ParseFailure::InvalidXml => "invalid_xml",
        }
    }
}

/// Outcome of the `[[sub]]` stage over every field that has subs. `NoMatch` (a gate
/// matched but no strategy did) and `Uncovered` (a field with subs exists but none is
/// gated for this event) are the 4am signals that a device emitted a message shape the
/// definition has not seen. `NoMatch` wins over `Uncovered`, which wins over `Matched`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubStatus {
    /// The definition declares no subs, or none of the fields its subs re-parse is present
    /// in this event (Junos structured logs carry no message part at all).
    #[default]
    NotApplicable,
    Matched,
    NoMatch,
    Uncovered,
}

pub(crate) enum CompiledStrategy {
    Kv(Box<KvConfig>),
    Delimiter(DelimConfig),
    Json,
    Cef,
    Leef,
    Pattern(Vec<CompiledPattern>),
    Xml,
}

impl CompiledStrategy {
    fn compile(s: &Strategy, is_sub: bool) -> Result<Self, String> {
        s.validate(is_sub)?;
        Ok(match s.kind {
            StrategyKind::Kv => CompiledStrategy::Kv(Box::new(KvConfig::new(
                s.key_value_separator.as_deref().unwrap_or("="),
                s.pair_separator.as_deref().unwrap_or(" "),
                Some(s.quote.as_deref().unwrap_or("\"")),
            )?)),
            StrategyKind::Delimiter => {
                let d = s.delimiter.as_deref().ok_or("delimiter strategy needs `delimiter`")?;
                CompiledStrategy::Delimiter(DelimConfig::new(d, s.quote.as_deref(), &s.fields, s.rest.as_deref())?)
            }
            StrategyKind::Json => CompiledStrategy::Json,
            StrategyKind::Cef => CompiledStrategy::Cef,
            StrategyKind::Leef => CompiledStrategy::Leef,
            StrategyKind::Xml => CompiledStrategy::Xml,
            StrategyKind::Pattern => {
                let anchor = s.anchor.unwrap_or(Anchor::Start);
                let mut compiled = Vec::new();
                for p in s.pattern.iter().chain(s.patterns.iter()) {
                    let t = Template::from_pattern(p).map_err(|e| format!("pattern `{p}`: {e}"))?;
                    compiled.push(CompiledPattern::from_template(&t, anchor).map_err(|e| format!("pattern `{p}`: {e}"))?);
                }
                if let Some(r) = &s.regex {
                    compiled.push(CompiledPattern::from_raw_regex(r, anchor)?);
                }
                if compiled.is_empty() {
                    return Err("pattern strategy needs `pattern`, `patterns` or `regex`".into());
                }
                CompiledStrategy::Pattern(compiled)
            }
        })
    }

    fn apply<'a>(&'a self, text: &'a [u8], scratch: &mut Scratch, out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
        match self {
            CompiledStrategy::Kv(cfg) => {
                if cfg.apply(text, out) == 0 { Err(ParseFailure::NoPairs) } else { Ok(()) }
            }
            CompiledStrategy::Delimiter(cfg) => {
                if text.is_empty() || cfg.apply(text, out) == 0 { Err(ParseFailure::NoColumns) } else { Ok(()) }
            }
            CompiledStrategy::Json => structured::apply_json(text, out),
            CompiledStrategy::Cef => structured::apply_cef(text, &mut scratch.structured, out),
            CompiledStrategy::Leef => structured::apply_leef(text, &mut scratch.structured, out),
            CompiledStrategy::Xml => structured::apply_xml(text, &mut scratch.structured, out),
            CompiledStrategy::Pattern(list) => {
                for p in list {
                    if p.apply(text, scratch.locs(p), out) {
                        return Ok(());
                    }
                }
                Err(ParseFailure::PatternNoMatch)
            }
        }
    }
}

struct CompiledTimestamp {
    field: Option<Vec<u8>>,
    fields: Vec<Vec<u8>>,
    format: Format,
}

struct CompiledSub {
    field: Vec<u8>,
    /// Index of the first sub on the same field; subs on one field are alternatives.
    group: usize,
    when: Vec<(Vec<u8>, OneOrMany)>,
    strategy: CompiledStrategy,
    constants: Vec<(Vec<u8>, Vec<u8>)>,
}

pub struct Parser {
    def: ParserDefinition,
    pub(crate) matcher: CompiledMatcher,
    strategy: CompiledStrategy,
    timestamps: Vec<CompiledTimestamp>,
    subs: Vec<CompiledSub>,
}

/// Per-thread scratch: capture locations per compiled pattern, sub-group states, and the
/// CEF/LEEF position buffers. Grows on first use, then allocates nothing.
#[derive(Default)]
pub struct Scratch {
    locs: Vec<Option<CaptureLocations>>,
    sub_state: Vec<u8>,
    structured: StructuredScratch,
}

impl Scratch {
    fn locs(&mut self, p: &CompiledPattern) -> &mut CaptureLocations {
        if self.locs.len() <= p.slot {
            self.locs.resize_with(p.slot + 1, || None);
        }
        self.locs[p.slot].get_or_insert_with(|| p.locations())
    }
}

impl Parser {
    pub fn from_definition(def: ParserDefinition) -> Result<Parser, String> {
        let matcher = CompiledMatcher::compile(&def.matcher)?;
        let strategy = CompiledStrategy::compile(&def.strategy, false).map_err(|e| format!("[strategy] {e}"))?;
        let mut timestamps = Vec::new();
        for (i, t) in def.timestamp.iter().enumerate() {
            if t.field.is_none() && t.fields.is_empty() {
                return Err(format!("[[timestamp]] #{}: needs `field` or `fields`", i + 1));
            }
            let format = Format::from_spec(&t.format).map_err(|e| format!("[[timestamp]] #{}: {}", i + 1, e.message))?;
            timestamps.push(CompiledTimestamp {
                field: t.field.as_ref().map(|f| f.as_bytes().to_vec()),
                fields: t.fields.iter().map(|f| f.as_bytes().to_vec()).collect(),
                format,
            });
        }
        let mut subs = Vec::new();
        for (i, s) in def.sub.iter().enumerate() {
            let group = def.sub[..i].iter().position(|p| p.field == s.field).unwrap_or(i);
            subs.push(compile_sub(s, group).map_err(|e| format!("[[sub]] #{}: {e}", i + 1))?);
        }
        Ok(Parser { def, matcher, strategy, timestamps, subs })
    }

    pub fn definition(&self) -> &ParserDefinition {
        &self.def
    }

    pub fn name(&self) -> &str {
        &self.def.parser.name
    }

    /// Signature check only; no fields are extracted.
    pub fn matches(&self, event: &[u8]) -> bool {
        self.matcher.matches(event)
    }

    /// Parses one event. `out` is cleared first. On `Err` the strategy found nothing, but
    /// any envelope fields and timestamp are still in `out` — the event is never lost.
    pub fn parse<'a>(&'a self, event: &'a [u8], ctx: &Context, scratch: &mut Scratch, out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
        out.clear();
        let mut body = event;
        while let Some(&last) = body.last() {
            if last == b'\n' || last == b'\r' {
                body = &body[..body.len() - 1];
            } else {
                break;
            }
        }
        if self.def.envelope.syslog {
            body = envelope::strip_syslog(body, out);
        }
        let result = self.strategy.apply(body, scratch, out);
        if result.is_ok() && !self.subs.is_empty() {
            out.sub = self.run_subs(scratch, out);
        }
        self.resolve_timestamp(ctx, out);
        result
    }

    /// Subs run in file order. Each field is re-parsed by at most one sub: the first
    /// eligible one whose strategy matches it. A later sub may gate on fields an earlier
    /// one produced (pfSense: the IP-version column decides how the tail is split).
    fn run_subs<'a>(&'a self, scratch: &mut Scratch, out: &mut Parsed<'a>) -> SubStatus {
        // per field group: 0 field absent, 1 present but no sub gated, 2 gated sub failed, 3 matched
        scratch.sub_state.clear();
        scratch.sub_state.resize(self.subs.len(), 0);
        for sub in &self.subs {
            if scratch.sub_state[sub.group] == 3 {
                continue;
            }
            // A borrowed value is a span of the event and its sub-fields borrow it too. A
            // materialised value (JSON, an unescaped quoted value, an RFC 5424 param with
            // escapes) is copied once and its sub-fields are owned: the one documented
            // allocation in the sub stage, paid only where the value already allocated.
            let input: Result<&'a [u8], Vec<u8>> = match out.get(&sub.field) {
                None => continue,
                Some(Cow::Borrowed(b)) => Ok(*b),
                Some(Cow::Owned(o)) => Err(o.clone()),
            };
            if !sub.when.iter().all(|(k, v)| out.get(k).is_some_and(|val| v.contains(val))) {
                scratch.sub_state[sub.group] = scratch.sub_state[sub.group].max(1);
                continue;
            }
            let matched = match &input {
                Ok(borrowed) => {
                    let mark = out.fields.len();
                    let ok = sub.strategy.apply(borrowed, scratch, out).is_ok();
                    if !ok {
                        out.fields.truncate(mark);
                    }
                    ok
                }
                Err(owned) => {
                    let mut tmp = Parsed::default();
                    let ok = sub.strategy.apply(owned, scratch, &mut tmp).is_ok();
                    if ok {
                        for f in tmp.fields {
                            out.push(Cow::Owned(f.key.into_owned()), Cow::Owned(f.value.into_owned()));
                        }
                    }
                    ok
                }
            };
            if matched {
                for (k, v) in &sub.constants {
                    out.push(k.as_slice(), v.as_slice());
                }
                scratch.sub_state[sub.group] = 3;
            } else {
                scratch.sub_state[sub.group] = 2;
            }
        }
        let state = &scratch.sub_state;
        if state.contains(&2) {
            SubStatus::NoMatch
        } else if state.contains(&1) {
            SubStatus::Uncovered
        } else if state.contains(&3) {
            SubStatus::Matched
        } else {
            SubStatus::NotApplicable
        }
    }

    /// `timestamp_error` is set only when no candidate yields a time: it names the reason
    /// the last present-but-unusable candidate failed, so it never accompanies a resolved
    /// timestamp and the counter means "device time present, unreadable".
    fn resolve_timestamp<'a>(&self, ctx: &Context, out: &mut Parsed<'a>) {
        let mut error = None;
        for t in &self.timestamps {
            if let Some(f) = &t.field {
                let Some(val) = out.get(f).cloned() else { continue };
                match ulpf_time::parse(&val, &t.format, ctx) {
                    Ok(ts) => {
                        out.timestamp = Some(ts);
                        out.timestamp_text = Some(val);
                        return;
                    }
                    Err(e) => error = Some(e.reason()),
                }
            } else {
                // The join buffer belongs to `out`: it becomes `timestamp_text` on success
                // and comes back on the next `clear`, so this allocates once per thread.
                let mut join = out.take_spare();
                let mut complete = true;
                for (i, f) in t.fields.iter().enumerate() {
                    match out.get(f) {
                        Some(v) => {
                            if i > 0 {
                                join.push(b' ');
                            }
                            join.extend_from_slice(v);
                        }
                        None => {
                            complete = false;
                            break;
                        }
                    }
                }
                if !complete {
                    out.give_back(join);
                    continue;
                }
                match ulpf_time::parse(&join, &t.format, ctx) {
                    Ok(ts) => {
                        out.timestamp = Some(ts);
                        out.timestamp_text = Some(Cow::Owned(join));
                        return;
                    }
                    Err(e) => {
                        error = Some(e.reason());
                        out.give_back(join);
                    }
                }
            }
        }
        if let Some(val) = out.get(b"syslog_timestamp").cloned() {
            match ulpf_time::parse(&val, &Format::Auto, ctx) {
                Ok(ts) => {
                    out.timestamp = Some(ts);
                    out.timestamp_text = Some(val);
                    return;
                }
                Err(e) => error = Some(e.reason()),
            }
        }
        out.timestamp_error = error;
    }
}

fn compile_sub(s: &Strategy, group: usize) -> Result<CompiledSub, String> {
    let strategy = CompiledStrategy::compile(s, true)?;
    Ok(CompiledSub {
        field: s.field.as_deref().unwrap_or_default().as_bytes().to_vec(),
        group,
        when: s.when.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.clone())).collect(),
        strategy,
        constants: s.constants.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect(),
    })
}
