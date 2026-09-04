//! `Template`: constant tokens plus typed slots. The inference engine's product, and the
//! single source of truth for the `{name:type}` pattern syntax — the pattern strategy
//! compiles through `Template::from_pattern`, so anything a Template holds is, by
//! construction, expressible in a definition file.

use std::fmt::Write as _;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// when the slot is the final token, where a lazy `text` would match one byte.
    pub(crate) fn regex(self, last: bool) -> &'static str {
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
            SlotKind::Timestamp => r"(?:[A-Z][a-z]{2} +[0-9]{1,2}(?: +[0-9]{4})? +[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?: +[0-9]{4})?|[0-9]{4}[-/][0-9]{2}[-/][0-9]{2}[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?|[0-9]{9,19}(?:\.[0-9]+)?)",
            SlotKind::Quoted => r#""(?:[^"\\]|\\.)*""#,
        }
    }
}

impl Template {
    /// Parses the `{name:type}` syntax. `{{` and `}}` are literal braces; a slot with no
    /// type is `text`. Errors name the offending slot.
    pub fn from_pattern(pattern: &str) -> Result<Template, String> {
        let mut tokens = Vec::new();
        let mut constant = String::new();
        let mut chars = pattern.chars().peekable();
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
                    if !constant.is_empty() {
                        tokens.push(Token::Const(std::mem::take(&mut constant)));
                    }
                    tokens.push(Token::Slot { name: name.to_owned(), kind });
                }
                '}' => return Err("stray `}` (write `}}` for a literal brace)".into()),
                _ => constant.push(c),
            }
        }
        if !constant.is_empty() {
            tokens.push(Token::Const(constant));
        }
        Ok(Template { tokens })
    }

    /// The inverse of `from_pattern`: `Template::from_pattern(t.to_pattern()) == t`.
    pub fn to_pattern(&self) -> String {
        let mut out = String::new();
        for t in &self.tokens {
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
            }
        }
        out
    }

    /// Emits a complete parser definition for this template. `contains` are the
    /// signature substrings the inference engine found constant across the cluster.
    pub fn to_definition(&self, name: &str, vendor: &str, product: &str, contains: Vec<String>) -> ParserDefinition {
        ParserDefinition {
            parser: Meta { name: name.to_owned(), vendor: vendor.to_owned(), product: product.to_owned(), description: None },
            matcher: Matcher { contains, starts_with: None, regex: None, priority: 0 },
            envelope: Envelope { syslog: true },
            strategy: Strategy::pattern(&self.to_pattern()),
            timestamp: vec![],
            sub: vec![],
        }
    }
}
