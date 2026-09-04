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
}

impl ParseFailure {
    pub const ALL: [ParseFailure; 6] = [
        ParseFailure::PatternNoMatch, ParseFailure::NoPairs, ParseFailure::NoColumns,
        ParseFailure::InvalidJson, ParseFailure::InvalidCef, ParseFailure::InvalidLeef,
    ];
    pub fn reason(self) -> &'static str {
        match self {
            ParseFailure::PatternNoMatch => "pattern_no_match",
            ParseFailure::NoPairs => "no_pairs",
            ParseFailure::NoColumns => "no_columns",
            ParseFailure::InvalidJson => "invalid_json",
            ParseFailure::InvalidCef => "invalid_cef",
            ParseFailure::InvalidLeef => "invalid_leef",
        }
    }
}

/// Outcome of the `[[sub]]` stage. `NoMatch` is the 4am signal that a device emitted a
/// message shape the definition has not seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubStatus {
    #[default]
    NotApplicable,
    Matched,
    NoMatch,
}

pub(crate) enum CompiledStrategy {
    Kv(Box<KvConfig>),
    Delimiter(DelimConfig),
    Json,
    Cef,
    Leef,
    Pattern(Vec<CompiledPattern>),
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
                CompiledStrategy::Delimiter(DelimConfig::new(d, s.quote.as_deref(), &s.fields)?)
            }
            StrategyKind::Json => CompiledStrategy::Json,
            StrategyKind::Cef => CompiledStrategy::Cef,
            StrategyKind::Leef => CompiledStrategy::Leef,
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
            CompiledStrategy::Cef => structured::apply_cef(text, out),
            CompiledStrategy::Leef => structured::apply_leef(text, out),
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

/// Per-thread scratch: capture locations per compiled pattern, a join buffer for
/// multi-field timestamps. Grows on first use, then allocates nothing.
#[derive(Default)]
pub struct Scratch {
    locs: Vec<Option<CaptureLocations>>,
    join: Vec<u8>,
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
            subs.push(compile_sub(s).map_err(|e| format!("[[sub]] #{}: {e}", i + 1))?);
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
        self.resolve_timestamp(ctx, scratch, out);
        result
    }

    fn run_subs<'a>(&'a self, scratch: &mut Scratch, out: &mut Parsed<'a>) -> SubStatus {
        let mut eligible = false;
        for sub in &self.subs {
            if !sub.when.iter().all(|(k, v)| out.get(k).is_some_and(|val| v.contains(val))) {
                continue;
            }
            // Subs run on spans of the event only; an owned (unescaped/JSON) value cannot
            // be borrowed for the event's lifetime.
            let Some(Cow::Borrowed(input)) = out.get(&sub.field) else { continue };
            let input: &'a [u8] = input;
            eligible = true;
            let mark = out.fields.len();
            if sub.strategy.apply(input, scratch, out).is_ok() {
                for (k, v) in &sub.constants {
                    out.push(k.as_slice(), v.as_slice());
                }
                return SubStatus::Matched;
            }
            out.fields.truncate(mark);
        }
        if eligible { SubStatus::NoMatch } else { SubStatus::NotApplicable }
    }

    fn resolve_timestamp<'a>(&self, ctx: &Context, scratch: &mut Scratch, out: &mut Parsed<'a>) {
        for t in &self.timestamps {
            if let Some(f) = &t.field {
                let Some(val) = out.get(f).cloned() else { continue };
                match ulpf_time::parse(&val, &t.format, ctx) {
                    Ok(ts) => {
                        out.timestamp = Some(ts);
                        out.timestamp_text = Some(val);
                        return;
                    }
                    Err(e) => out.timestamp_error = Some(e.reason()),
                }
            } else {
                scratch.join.clear();
                let mut complete = true;
                for (i, f) in t.fields.iter().enumerate() {
                    match out.get(f) {
                        Some(v) => {
                            if i > 0 {
                                scratch.join.push(b' ');
                            }
                            scratch.join.extend_from_slice(v);
                        }
                        None => {
                            complete = false;
                            break;
                        }
                    }
                }
                if !complete {
                    continue;
                }
                match ulpf_time::parse(&scratch.join, &t.format, ctx) {
                    Ok(ts) => {
                        out.timestamp = Some(ts);
                        out.timestamp_text = Some(Cow::Owned(scratch.join.clone()));
                        return;
                    }
                    Err(e) => out.timestamp_error = Some(e.reason()),
                }
            }
        }
        if let Some(val) = out.get(b"syslog_timestamp").cloned() {
            match ulpf_time::parse(&val, &Format::Auto, ctx) {
                Ok(ts) => {
                    out.timestamp = Some(ts);
                    out.timestamp_text = Some(val);
                }
                Err(e) => out.timestamp_error = Some(e.reason()),
            }
        }
    }
}

fn compile_sub(s: &Strategy) -> Result<CompiledSub, String> {
    let strategy = CompiledStrategy::compile(s, true)?;
    Ok(CompiledSub {
        field: s.field.as_deref().unwrap_or_default().as_bytes().to_vec(),
        when: s.when.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.clone())).collect(),
        strategy,
        constants: s.constants.iter().map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect(),
    })
}
