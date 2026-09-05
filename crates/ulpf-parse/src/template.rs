//! `Template`: constant tokens plus typed slots. The inference engine's product, and the
//! single source of truth for the `{name:type}` pattern syntax — the pattern strategy
//! compiles through `Template::from_pattern`, so anything a Template holds is, by
//! construction, expressible in a definition file.

use std::fmt::Write as _;
use std::sync::OnceLock;

use crate::def::{Envelope, Matcher, Meta, ParserDefinition, Strategy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Const(String),
    /// `name == "_"` matches without emitting a field.
    Slot { name: String, kind: SlotKind },
    /// `{? ...}`: a run of constants and slots that may be absent as a whole. This is how
    /// an inferred template says "some lines carry ` len={len:int}` and some do not"
    /// without duplicating the template; groups do not nest.
    Optional(Vec<Token>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Int,
    Float,
    Word,
    Text,
    Rest,
    Ip,
    Ipv4,
    Ipv6,
    Port,
    Hex,
    Mac,
    Timestamp,
    Quoted,
}

impl SlotKind {
    pub const ALL: [SlotKind; 13] = [
        SlotKind::Int, SlotKind::Float, SlotKind::Word, SlotKind::Text, SlotKind::Rest,
        SlotKind::Ip, SlotKind::Ipv4, SlotKind::Ipv6, SlotKind::Port, SlotKind::Hex,
        SlotKind::Mac, SlotKind::Timestamp, SlotKind::Quoted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SlotKind::Int => "int",
            SlotKind::Float => "float",
            SlotKind::Word => "word",
            SlotKind::Text => "text",
            SlotKind::Rest => "rest",
            SlotKind::Ip => "ip",
            SlotKind::Ipv4 => "ipv4",
            SlotKind::Ipv6 => "ipv6",
            SlotKind::Port => "port",
            SlotKind::Hex => "hex",
            SlotKind::Mac => "mac",
            SlotKind::Timestamp => "timestamp",
            SlotKind::Quoted => "quoted",
        }
    }

    pub fn from_name(s: &str) -> Option<SlotKind> {
        SlotKind::ALL.iter().copied().find(|k| k.name() == s)
    }

    /// Regex fragment (bytes mode, `(?s-u)` flags set by the compiler). `last` is true
    /// when the slot is the final token, where a lazy `text` would match one byte. Public
    /// so the inference tokenizer recognises exactly what a slot will later accept.
    pub fn regex(self, last: bool) -> &'static str {
        match self {
            SlotKind::Int => r"[+-]?[0-9]+",
            SlotKind::Float => r"[+-]?[0-9]+(?:\.[0-9]+)?",
            SlotKind::Word => r"[^ \t\r\n]+",
            SlotKind::Text if last => r".+",
            SlotKind::Text => r".+?",
            SlotKind::Rest => r".*",
            SlotKind::Ipv4 => r"(?:[0-9]{1,3}\.){3}[0-9]{1,3}",
            SlotKind::Ipv6 => r"[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}(?:\.[0-9.]+)?",
            SlotKind::Ip => r"(?:(?:[0-9]{1,3}\.){3}[0-9]{1,3}|[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}(?:\.[0-9.]+)?)",
            SlotKind::Port => r"[0-9]{1,5}",
            SlotKind::Hex => r"(?:0[xX])?[0-9A-Fa-f]+",
            SlotKind::Mac => r"(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}",
            SlotKind::Timestamp => timestamp_regex(),
            SlotKind::Quoted => r#""(?:[^"\\]|\\.)*""#,
        }
    }
}

/// The shapes `ulpf_time::parse` understands: syslog/ctime (optional weekday, Cisco IOS
/// `*`/`.` clock mark, fraction, year before or after the time, a known zone name or an
/// offset), ISO 8601, the Apache/nginx common log form `04/Sep/2026:10:15:23 +0000`, epoch. Zone names come from the time module's table so this slot
/// cannot swallow a following all-caps token that is not a zone.
fn timestamp_regex() -> &'static str {
    static RE: OnceLock<String> = OnceLock::new();
    RE.get_or_init(|| {
        let zones = ulpf_time::zone_names().join("|");
        format!(
            r"(?:(?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun) +)?[*.]?[A-Z][a-z]{{2}} +[0-9]{{1,2}}(?: +[0-9]{{4}})? +[0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}}(?:\.[0-9]+)?(?: +[0-9]{{4}})?(?: +(?:{zones}|[+-][0-9]{{2}}:?[0-9]{{2}}))?|[0-9]{{4}}[-/][0-9]{{2}}[-/][0-9]{{2}}[T ][0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}}(?:\.[0-9]+)?(?:Z|[+-][0-9]{{2}}:?[0-9]{{2}})?|[0-9]{{1,2}}/[A-Z][a-z]{{2}}/[0-9]{{4}}:[0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}}(?: +[+-][0-9]{{4}})?|[0-9]{{9,19}}(?:\.[0-9]+)?)"
        )
    })
}

impl Template {
    /// Parses the `{name:type}` syntax. `{{` and `}}` are literal braces; a slot with no
    /// type is `text`; `{? ...}` wraps an optional group. Errors name the offending slot.
    pub fn from_pattern(pattern: &str) -> Result<Template, String> {
        let mut chars = pattern.chars().peekable();
        let tokens = parse_tokens(&mut chars, false)?;
        Ok(Template { tokens })
    }

    /// The inverse of `from_pattern`: `Template::from_pattern(t.to_pattern()) == t`.
    pub fn to_pattern(&self) -> String {
        let mut out = String::new();
        write_tokens(&self.tokens, &mut out);
        out
    }

    /// Every slot in pattern order, groups flattened.
    pub fn slots(&self) -> impl Iterator<Item = (&str, SlotKind)> {
        self.tokens.iter().flat_map(|t| match t {
            Token::Slot { name, kind } => vec![(name.as_str(), *kind)],
            Token::Optional(inner) => inner.iter().filter_map(|t| match t {
                Token::Slot { name, kind } => Some((name.as_str(), *kind)),
                _ => None,
            }).collect(),
            Token::Const(_) => vec![],
        })
    }

    /// Emits a complete parser definition for this template. `contains` are the
    /// signature substrings the inference engine found constant across the cluster.
    pub fn to_definition(&self, name: &str, vendor: &str, product: &str, contains: Vec<String>) -> ParserDefinition {
        ParserDefinition {
            parser: Meta { name: name.to_owned(), vendor: vendor.to_owned(), product: product.to_owned(), description: None, version: 1, origin: None },
            matcher: Matcher { contains, starts_with: None, regex: None, priority: 0 },
            envelope: Envelope { syslog: true },
            strategy: Strategy::pattern(&self.to_pattern()),
            timestamp: vec![],
            sub: vec![],
        }
    }
}

/// Reads tokens until the end of input, or, inside a group, until the `}` that closes it.
fn parse_tokens(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, in_group: bool) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut constant = String::new();
    let flush = |constant: &mut String, tokens: &mut Vec<Token>| {
        if !constant.is_empty() {
            tokens.push(Token::Const(std::mem::take(constant)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                constant.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                constant.push('}');
            }
            '{' if chars.peek() == Some(&'?') => {
                chars.next();
                if in_group {
                    return Err("optional groups `{? ...}` do not nest".into());
                }
                flush(&mut constant, &mut tokens);
                let inner = parse_tokens(chars, true)?;
                if inner.is_empty() {
                    return Err("empty optional group `{?}`".into());
                }
                tokens.push(Token::Optional(inner));
            }
            '{' => {
                let mut body = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => body.push(ch),
                        None => return Err(format!("unterminated slot `{{{body}`")),
                    }
                }
                let (name, kind) = match body.split_once(':') {
                    Some((n, k)) => (n, SlotKind::from_name(k).ok_or_else(|| format!("slot `{n}`: unknown type `{k}`"))?),
                    None => (body.as_str(), SlotKind::Text),
                };
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    return Err(format!("slot name `{name}` must be [A-Za-z0-9_]+"));
                }
                flush(&mut constant, &mut tokens);
                tokens.push(Token::Slot { name: name.to_owned(), kind });
            }
            '}' if in_group => {
                flush(&mut constant, &mut tokens);
                return Ok(tokens);
            }
            '}' => return Err("stray `}` (write `}}` for a literal brace)".into()),
            _ => constant.push(c),
        }
    }
    if in_group {
        return Err("unterminated optional group `{? ...`".into());
    }
    flush(&mut constant, &mut tokens);
    Ok(tokens)
}

fn write_tokens(tokens: &[Token], out: &mut String) {
    for t in tokens {
        match t {
            Token::Const(s) => {
                for c in s.chars() {
                    match c {
                        '{' => out.push_str("{{"),
                        '}' => out.push_str("}}"),
                        _ => out.push(c),
                    }
                }
            }
            Token::Slot { name, kind } => {
                let _ = write!(out, "{{{name}:{}}}", kind.name());
            }
            Token::Optional(inner) => {
                out.push_str("{?");
                write_tokens(inner, out);
                out.push('}');
            }
        }
    }
}
