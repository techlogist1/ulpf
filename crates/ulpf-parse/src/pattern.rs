//! Pattern strategy: a `Template` compiled to one bytes regex. Spaces in constants match
//! any run of spaces/tabs (real devices jitter their spacing). Capture locations are
//! reused via `Scratch`, so a match allocates nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

use regex::bytes::{CaptureLocations, Regex};

use crate::Parsed;
use crate::def::Anchor;
use crate::template::{SlotKind, Template, Token};

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct CompiledPattern {
    regex: Regex,
    /// Field name per capture group index; `None` for group 0 and discarded slots.
    names: Vec<Option<Vec<u8>>>,
    /// Index into `Scratch::locs`, unique per compiled pattern in the process.
    pub(crate) slot: usize,
}

/// Appends the regex for `tokens`. `last` says whether the run ends the whole pattern,
/// where a lazy `text` slot would otherwise match a single byte.
fn emit_tokens(tokens: &[Token], last: bool, re: &mut String) {
    let n = tokens.len();
    for (idx, tok) in tokens.iter().enumerate() {
        let is_last = last && idx + 1 == n;
        match tok {
            Token::Const(s) => {
                for c in s.chars() {
                    if c == ' ' {
                        re.push_str("[ \\t]+");
                    } else {
                        re.push_str(&regex::escape(&c.to_string()));
                    }
                }
            }
            Token::Slot { name, kind } => {
                let body = kind.regex(is_last);
                if name == "_" {
                    re.push_str("(?:");
                    re.push_str(body);
                    re.push(')');
                } else if *kind == SlotKind::Quoted {
                    re.push_str(&format!("\"(?P<{name}>(?:[^\"\\\\]|\\\\.)*)\""));
                } else {
                    re.push_str(&format!("(?P<{name}>{body})"));
                }
            }
            Token::Optional(inner) => {
                re.push_str("(?:");
                emit_tokens(inner, false, re);
                re.push_str(")?");
            }
        }
    }
}

impl CompiledPattern {
    pub(crate) fn from_template(t: &Template, anchor: Anchor) -> Result<Self, String> {
        let mut re = String::from("(?s-u)");
        if anchor != Anchor::None {
            re.push('^');
        }
        emit_tokens(&t.tokens, true, &mut re);
        if anchor == Anchor::Full {
            re.push('$');
        }
        Self::from_regex_str(&re)
    }

    pub(crate) fn from_raw_regex(raw: &str, anchor: Anchor) -> Result<Self, String> {
        let mut re = String::from("(?s-u)");
        if anchor != Anchor::None && !raw.starts_with('^') {
            re.push('^');
        }
        re.push_str(raw);
        if anchor == Anchor::Full && !raw.ends_with('$') {
            re.push('$');
        }
        Self::from_regex_str(&re)
    }

    fn from_regex_str(re: &str) -> Result<Self, String> {
        let regex = Regex::new(re).map_err(|e| format!("pattern does not compile: {e}"))?;
        let names = regex.capture_names().map(|n| n.map(|s| s.as_bytes().to_vec())).collect();
        Ok(CompiledPattern { regex, names, slot: NEXT_SLOT.fetch_add(1, Ordering::Relaxed) })
    }

    pub(crate) fn locations(&self) -> CaptureLocations {
        self.regex.capture_locations()
    }

    /// Runs the pattern on `text`. On a match, pushes every named group that captured
    /// something as a field; an empty capture (`rest` with nothing left, `quoted` of `""`)
    /// is the absence of a field, not a field with no value.
    pub(crate) fn apply<'a>(&'a self, text: &'a [u8], locs: &mut CaptureLocations, out: &mut Parsed<'a>) -> bool {
        if self.regex.captures_read(locs, text).is_none() {
            return false;
        }
        for (i, name) in self.names.iter().enumerate().skip(1) {
            if let (Some(name), Some((s, e))) = (name, locs.get(i))
                && s < e
            {
                out.push(name.as_slice(), &text[s..e]);
            }
        }
        true
    }
}
