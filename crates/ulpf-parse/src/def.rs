//! The parser definition file format (TOML). This is the contract teammates and the
//! inference engine both write to. `deny_unknown_fields` everywhere: a typo is reported,
//! and there is no field in which output-schema vocabulary could be smuggled in.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserDefinition {
    pub parser: Meta,
    #[serde(rename = "match")]
    pub matcher: Matcher,
    #[serde(default)]
    pub envelope: Envelope,
    pub strategy: Strategy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timestamp: Vec<TimestampSpec>,
    /// Each entry is a `Strategy` with `field` (required) and optional `when`/`constants`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub: Vec<Strategy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    pub name: String,
    pub vendor: String,
    pub product: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Bumped by an approved drift update; a hand-written file may set it.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub version: u64,
    /// `inferred` when the inference engine wrote the definition; absent for a hand-written one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

fn one() -> u64 {
    1
}

fn is_one(v: &u64) -> bool {
    *v == 1
}

/// Signature detection. Every `contains` substring must be present; `starts_with` and
/// `regex` are additional requirements when given. Higher `priority` is tried first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matcher {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_with: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// Strip an optional `<pri>` and RFC 3164 / RFC 5424 header before the strategy
    /// runs, exposing `syslog_pri`, `syslog_facility`, `syslog_severity`,
    /// `syslog_timestamp`, `syslog_host` (and for 5424: `syslog_app`, `syslog_procid`,
    /// `syslog_msgid`, `syslog_sd`).
    #[serde(default)]
    pub syslog: bool,
}

/// One strategy block. The same struct serves `[strategy]` and every `[[sub]]`: a flat
/// struct is the only shape serde can both `deny_unknown_fields` on and flatten-free
/// load, and it keeps the file format identical in both places. Which keys apply to
/// which `kind` is validated when the definition compiles (see `Strategy::validate`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strategy {
    pub kind: StrategyKind,
    // kind = "kv": `key=value` pairs. Fortinet, Sophos, SonicWall, Check Point (`key:"v"; `).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_value_separator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_separator: Option<String>,
    /// kv and delimiter: quote character; kv defaults to `"` and accepts several (`"'`,
    /// SonicWall quotes one field with `'`), delimiter to none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    // kind = "delimiter": positional columns. Palo Alto CSV, pfSense filterlog, Squid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Column names in order; `_` skips a column. Extra columns become `column_N`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    /// Everything after the last named column, unsplit, under this name. A `[[sub]]` gated
    /// on one of the named columns then splits it (pfSense, PAN-OS: the tail's layout
    /// depends on a column near the front).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<String>,
    // kind = "pattern": constant text with `{name:type}` slots (docs/parser-format.md).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    /// Escape hatch: a raw regex with `(?P<name>...)` groups instead of a pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    // `[[sub]]` only: which field to re-parse, gated by `when` (every listed field must
    // equal one of the listed values). Subs run in file order; each field is re-parsed by
    // at most one sub, the first eligible one whose strategy matches, which then adds its
    // `constants`. A later sub may gate on a field an earlier sub produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub when: BTreeMap<String, OneOrMany>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub constants: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrategyKind {
    #[default]
    Kv,
    Delimiter,
    Json,
    Cef,
    Leef,
    Pattern,
    Xml,
}

impl Strategy {
    pub fn kv() -> Strategy {
        Strategy { kind: StrategyKind::Kv, ..Default::default() }
    }

    pub fn pattern(p: &str) -> Strategy {
        Strategy { kind: StrategyKind::Pattern, pattern: Some(p.to_owned()), ..Default::default() }
    }

    /// Rejects keys that do not belong to `kind`, and sub-only keys at top level.
    pub fn validate(&self, is_sub: bool) -> Result<(), String> {
        let mut bad = Vec::new();
        let kv = matches!(self.kind, StrategyKind::Kv);
        let delim = matches!(self.kind, StrategyKind::Delimiter);
        let pat = matches!(self.kind, StrategyKind::Pattern);
        if !kv && self.key_value_separator.is_some() { bad.push("key_value_separator"); }
        if !kv && self.pair_separator.is_some() { bad.push("pair_separator"); }
        if !kv && !delim && self.quote.is_some() { bad.push("quote"); }
        if !delim && self.delimiter.is_some() { bad.push("delimiter"); }
        if !delim && !self.fields.is_empty() { bad.push("fields"); }
        if !delim && self.rest.is_some() { bad.push("rest"); }
        if !pat && self.pattern.is_some() { bad.push("pattern"); }
        if !pat && !self.patterns.is_empty() { bad.push("patterns"); }
        if !pat && self.regex.is_some() { bad.push("regex"); }
        if !pat && self.anchor.is_some() { bad.push("anchor"); }
        if !is_sub && self.field.is_some() { bad.push("field"); }
        if !is_sub && !self.when.is_empty() { bad.push("when"); }
        if !is_sub && !self.constants.is_empty() { bad.push("constants"); }
        if let Some(k) = bad.first() {
            return Err(format!("key `{k}` does not apply to kind `{}`{}", self.kind.name(), if is_sub { "" } else { " at top level" }));
        }
        if is_sub && self.field.is_none() {
            return Err("[[sub]] needs `field`".into());
        }
        Ok(())
    }
}

impl StrategyKind {
    pub fn name(self) -> &'static str {
        match self {
            StrategyKind::Kv => "kv",
            StrategyKind::Delimiter => "delimiter",
            StrategyKind::Json => "json",
            StrategyKind::Cef => "cef",
            StrategyKind::Leef => "leef",
            StrategyKind::Pattern => "pattern",
            StrategyKind::Xml => "xml",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    #[default]
    Start,
    Full,
    None,
}

/// One way to find the event time. Candidates are tried in order; `fields` are joined
/// with a single space. `format` is a `ulpf-time` spec (`auto`, `rfc3339`, `syslog`,
/// `epoch`, `epoch_ms`, `epoch_ns`, or a strftime layout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimestampSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    pub format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn contains(&self, v: &[u8]) -> bool {
        match self {
            OneOrMany::One(s) => s.as_bytes() == v,
            OneOrMany::Many(list) => list.iter().any(|s| s.as_bytes() == v),
        }
    }
}
