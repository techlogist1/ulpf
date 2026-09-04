//! Delimiter strategy: positional columns. Short rows emit what is present; extra
//! columns are named `column_N` (1-based) so nothing is dropped.

use std::borrow::Cow;

use crate::Parsed;

pub(crate) struct DelimConfig {
    pub delimiter: u8,
    pub quote: Option<u8>,
    /// `None` entries are skipped columns (`_`).
    pub fields: Vec<Option<Vec<u8>>>,
}

impl DelimConfig {
    pub(crate) fn new(delimiter: &str, quote: Option<&str>, fields: &[String]) -> Result<Self, String> {
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
        Ok(DelimConfig { delimiter, quote, fields })
    }

    /// Returns the number of columns found.
    pub(crate) fn apply<'a>(&'a self, text: &'a [u8], out: &mut Parsed<'a>) -> usize {
        let n = text.len();
        let mut i = 0;
        let mut col = 0;
        loop {
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
            match self.fields.get(col) {
                Some(Some(name)) => out.push(name.as_slice(), value),
                Some(None) => {}
                None => out.push(Cow::Owned(format!("column_{}", col + 1).into_bytes()), value),
            }
            col += 1;
            if next >= n {
                break;
            }
            i = next + 1;
            if i >= n {
                // trailing delimiter: one final empty column
                match self.fields.get(col) {
                    Some(Some(name)) => out.push(name.as_slice(), &text[n..]),
                    Some(None) => {}
                    None => out.push(Cow::Owned(format!("column_{}", col + 1).into_bytes()), &text[n..]),
                }
                col += 1;
                break;
            }
        }
        col
    }
}
