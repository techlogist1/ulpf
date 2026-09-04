//! Delimiter strategy: positional columns. Short rows emit what is present; extra
//! columns are named `column_N` (1-based) so nothing is dropped, or, when `rest` is set,
//! stay together as one unsplit span for a `[[sub]]` to split by the row's own type. An
//! empty remainder (row ends after the named columns, with or without a trailing
//! delimiter) emits no `rest` field, like an empty pattern capture.

use std::borrow::Cow;

use crate::Parsed;

pub(crate) struct DelimConfig {
    pub delimiter: u8,
    pub quote: Option<u8>,
    /// `None` entries are skipped columns (`_`).
    pub fields: Vec<Option<Vec<u8>>>,
    /// Name for everything after the last named column, unsplit.
    pub rest: Option<Vec<u8>>,
}

impl DelimConfig {
    pub(crate) fn new(delimiter: &str, quote: Option<&str>, fields: &[String], rest: Option<&str>) -> Result<Self, String> {
        let delimiter = match delimiter {
            "\\t" | "tab" => b'\t',
            d if d.len() == 1 => d.as_bytes()[0],
            d => return Err(format!("delimiter must be a single byte or `tab`, got `{d}`")),
        };
        let quote = match quote {
            None | Some("") => None,
            Some(q) if q.len() == 1 => Some(q.as_bytes()[0]),
            Some(q) => return Err(format!("quote must be a single byte, got `{q}`")),
        };
        if fields.is_empty() {
            return Err("delimiter strategy needs a non-empty `fields` list".into());
        }
        let fields = fields.iter().map(|f| if f == "_" || f.is_empty() { None } else { Some(f.as_bytes().to_vec()) }).collect();
        let rest = match rest {
            None | Some("") | Some("_") => None,
            Some(r) => Some(r.as_bytes().to_vec()),
        };
        Ok(DelimConfig { delimiter, quote, fields, rest })
    }

    fn emit<'a>(&'a self, col: usize, value: &'a [u8], out: &mut Parsed<'a>) {
        match self.fields.get(col) {
            Some(Some(name)) => out.push(name.as_slice(), value),
            Some(None) => {}
            None => match &self.rest {
                // nothing left after the named columns is the absence of a remainder
                Some(rest) if col == self.fields.len() => {
                    if !value.is_empty() {
                        out.push(rest.as_slice(), value);
                    }
                }
                _ => out.push(Cow::Owned(format!("column_{}", col + 1).into_bytes()), value),
            },
        }
    }

    /// Returns the number of columns found (the `rest` span counts as one).
    pub(crate) fn apply<'a>(&'a self, text: &'a [u8], out: &mut Parsed<'a>) -> usize {
        let n = text.len();
        let mut i = 0;
        let mut col = 0;
        loop {
            if col == self.fields.len()
                && let Some(rest) = &self.rest
            {
                if i < n {
                    out.push(rest.as_slice(), &text[i..]);
                }
                col += 1;
                break;
            }
            let (value, next): (&'a [u8], usize) = match self.quote {
                Some(q) if i < n && text[i] == q => {
                    let mut k = i + 1;
                    while k < n && text[k] != q {
                        k += 1;
                    }
                    let v = &text[i + 1..k.min(n)];
                    let mut after = (k + 1).min(n);
                    while after < n && text[after] != self.delimiter {
                        after += 1; // tolerate junk between closing quote and delimiter
                    }
                    (v, after)
                }
                _ => {
                    let end = memchr::memchr(self.delimiter, &text[i..]).map_or(n, |p| i + p);
                    (&text[i..end], end)
                }
            };
            self.emit(col, value, out);
            col += 1;
            if next >= n {
                break;
            }
            i = next + 1;
            if i >= n {
                // trailing delimiter: one final empty column
                self.emit(col, &text[n..], out);
                col += 1;
                break;
            }
        }
        col
    }
}
