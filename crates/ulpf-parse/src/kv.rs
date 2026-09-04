//! Key/value strategy. Lenient: bare tokens without a separator are skipped, quoted
//! values may contain separators and close with whichever quote byte opened them, `\"`
//! inside quotes is unescaped (the only case that allocates).

use std::borrow::Cow;

use crate::Parsed;

pub(crate) struct KvConfig {
    pub kv_sep: Vec<u8>,
    /// Treated as a set of separator bytes; a space also implies tab, CR and LF.
    pub pair_seps: [bool; 256],
    /// Quote bytes; a value opened by one closes with the same one.
    pub quotes: [bool; 256],
}

impl KvConfig {
    pub(crate) fn new(kv_sep: &str, pair_sep: &str, quote: Option<&str>) -> Result<Self, String> {
        if kv_sep.is_empty() {
            return Err("key_value_separator must not be empty".into());
        }
        let mut pair_seps = [false; 256];
        for b in pair_sep.bytes() {
            pair_seps[b as usize] = true;
            if b == b' ' {
                for w in [b'\t', b'\r', b'\n'] {
                    pair_seps[w as usize] = true;
                }
            }
        }
        let mut quotes = [false; 256];
        for b in quote.unwrap_or("").bytes() {
            quotes[b as usize] = true;
        }
        Ok(KvConfig { kv_sep: kv_sep.as_bytes().to_vec(), pair_seps, quotes })
    }

    #[inline]
    fn is_sep(&self, b: u8) -> bool {
        self.pair_seps[b as usize]
    }

    /// Returns the number of pairs found.
    pub(crate) fn apply<'a>(&self, text: &'a [u8], out: &mut Parsed<'a>) -> usize {
        let mut i = 0;
        let mut pairs = 0;
        let n = text.len();
        while i < n {
            while i < n && self.is_sep(text[i]) {
                i += 1;
            }
            if i >= n {
                break;
            }
            let key_start = i;
            // key runs to the kv separator, but never across a pair separator
            let mut key_end = None;
            let mut j = i;
            while j < n {
                if text[j..].starts_with(&self.kv_sep) {
                    key_end = Some(j);
                    break;
                }
                if self.is_sep(text[j]) {
                    break;
                }
                j += 1;
            }
            let Some(key_end) = key_end else {
                i = j; // bare token, skip
                continue;
            };
            let mut v = key_end + self.kv_sep.len();
            let value: Cow<'a, [u8]> = match text.get(v) {
                Some(&q) if self.quotes[q as usize] => {
                    let vs = v + 1;
                    let mut k = vs;
                    let mut escaped = false;
                    while k < n && text[k] != q {
                        if text[k] == b'\\' && k + 1 < n {
                            escaped = true;
                            k += 1;
                        }
                        k += 1;
                    }
                    let raw = &text[vs..k.min(n)];
                    i = if k < n { k + 1 } else { n };
                    if escaped { Cow::Owned(unescape(raw)) } else { Cow::Borrowed(raw) }
                }
                _ => {
                    while v < n && !self.is_sep(text[v]) {
                        v += 1;
                    }
                    let raw = &text[key_end + self.kv_sep.len()..v];
                    i = v;
                    Cow::Borrowed(raw)
                }
            };
            if key_end > key_start {
                out.push(&text[key_start..key_end], value);
                pairs += 1;
            }
        }
        pairs
    }
}

pub(crate) fn unescape(raw: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            v.push(raw[i + 1]);
            i += 2;
        } else {
            v.push(raw[i]);
            i += 1;
        }
    }
    v
}
