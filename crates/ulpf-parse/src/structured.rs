//! Self-describing formats. JSON goes through `serde_json::Value` and therefore
//! allocates (ponytail: acceptable while JSON sources are a minority; a streaming
//! flattener is the upgrade if they dominate the throughput file). CEF and LEEF are
//! scanned in place; their two position buffers live in `StructuredScratch`.

use std::borrow::Cow;
use std::ops::Range;

use crate::{ParseFailure, Parsed};

/// Per-thread position buffers for CEF/LEEF, so a parse allocates nothing after warm-up.
#[derive(Default)]
pub(crate) struct StructuredScratch {
    parts: Vec<Range<usize>>,
    eqs: Vec<(usize, usize)>,
}

pub(crate) fn apply_json<'a>(text: &'a [u8], out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let value: serde_json::Value = serde_json::from_slice(text).map_err(|_| ParseFailure::InvalidJson)?;
    let serde_json::Value::Object(map) = value else { return Err(ParseFailure::InvalidJson) };
    let mut prefix = String::new();
    for (k, v) in map {
        flatten(&mut prefix, &k, v, out);
    }
    Ok(())
}

// Values are moved, not cloned: the tree is dropped as it is walked.
fn flatten<'a>(prefix: &mut String, key: &str, v: serde_json::Value, out: &mut Parsed<'a>) {
    let base = prefix.len();
    if !prefix.is_empty() {
        prefix.push('.');
    }
    prefix.push_str(key);
    match v {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten(prefix, &k, v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.into_iter().enumerate() {
                flatten(prefix, &i.to_string(), v, out);
            }
        }
        serde_json::Value::Null => {}
        serde_json::Value::String(s) => out.push(Cow::Owned(prefix.clone().into_bytes()), Cow::Owned(s.into_bytes())),
        other => out.push(Cow::Owned(prefix.clone().into_bytes()), Cow::Owned(other.to_string().into_bytes())),
    }
    prefix.truncate(base);
}

// `cef_severity`, not `severity`: CEF's scale is 0-10 (0-3 Low .. 9-10 Very-High) while the
// devices that emit a bare `severity` are on the syslog 0-7 scale, and one source name can
// only carry one scale. The mapping buckets this name (mappings/ocsf.toml, [[enum]] severity).
static CEF_HEADER: [&[u8]; 7] = [
    b"cef_version", b"device_vendor", b"device_product", b"device_version", b"signature_id", b"name", b"cef_severity",
];

fn unescape_if_needed(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.contains(&b'\\') {
        return Cow::Borrowed(raw);
    }
    let mut v = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            match raw[i + 1] {
                b'n' => v.push(b'\n'),
                b'r' => v.push(b'\r'),
                c => v.push(c),
            }
            i += 2;
        } else {
            v.push(raw[i]);
            i += 1;
        }
    }
    Cow::Owned(v)
}

/// Splits on unescaped `|` into `parts`, at most `max` pieces (the last takes the rest).
fn split_pipes(text: &[u8], max: usize, parts: &mut Vec<Range<usize>>) {
    parts.clear();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() && parts.len() + 1 < max {
        match text[i] {
            b'\\' => i += 2,
            b'|' => {
                parts.push(start..i);
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    parts.push(start.min(text.len())..text.len());
}

pub(crate) fn apply_cef<'a>(text: &'a [u8], scratch: &mut StructuredScratch, out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let start = memchr::memmem::find(text, b"CEF:").ok_or(ParseFailure::InvalidCef)?;
    let body = &text[start + 4..];
    split_pipes(body, 8, &mut scratch.parts);
    if scratch.parts.len() < 8 {
        return Err(ParseFailure::InvalidCef);
    }
    for (name, part) in CEF_HEADER.iter().zip(&scratch.parts) {
        out.push(*name, unescape_if_needed(&body[part.clone()]));
    }
    extension_pairs(&body[scratch.parts[7].clone()], &mut scratch.eqs, out);
    Ok(())
}

/// CEF extension: `key=value key2=value with spaces`. A key is the run of key bytes
/// immediately before an unescaped `=`; a value runs until the next such key.
fn extension_pairs<'a>(ext: &'a [u8], eqs: &mut Vec<(usize, usize)>, out: &mut Parsed<'a>) {
    let n = ext.len();
    let is_key_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-';
    // positions of every unescaped '=' that has a key before it: (key_start, eq_pos)
    eqs.clear();
    let mut i = 0;
    while i < n {
        match ext[i] {
            b'\\' => i += 2,
            b'=' => {
                let mut k = i;
                while k > 0 && is_key_byte(ext[k - 1]) {
                    k -= 1;
                }
                if k < i && (k == 0 || ext[k - 1] == b' ' || ext[k - 1] == b'\t') {
                    eqs.push((k, i));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    for idx in 0..eqs.len() {
        let (ks, eq) = eqs[idx];
        let vs = eq + 1;
        let ve = match eqs.get(idx + 1) {
            Some(&(next_ks, _)) => {
                let mut e = next_ks;
                while e > vs && (ext[e - 1] == b' ' || ext[e - 1] == b'\t') {
                    e -= 1;
                }
                e
            }
            None => n,
        };
        out.push(&ext[ks..eq], unescape_if_needed(&ext[vs..ve.max(vs)]));
    }
}

static LEEF_HEADER: [&[u8]; 5] = [b"leef_version", b"device_vendor", b"device_product", b"device_version", b"event_id"];

/// LEEF 2.0's optional delimiter field. IBM: "You can use a single character or the hex
/// value for that character. The hex value can be represented by the prefix 0x or x".
/// Empty means tab. A prefixed value whose digits do not parse is a counted failure rather
/// than a fall back to tab, which would split the attributes on the wrong byte in silence.
fn delimiter_byte(d: &[u8]) -> Result<u8, ParseFailure> {
    let hex = match d {
        [] => return Ok(b'\t'),
        // A single byte is the delimiter itself, even where it is `x` or `0`.
        [one] => return Ok(*one),
        [b'0', b'x' | b'X', rest @ ..] | [b'x' | b'X', rest @ ..] => rest,
        [first, ..] => return Ok(*first),
    };
    // The digits must be hex digits: `from_str_radix` alone would read `0x+5` as byte 0x05.
    if !matches!(hex.len(), 1 | 2) || !hex.iter().all(u8::is_ascii_hexdigit) {
        return Err(ParseFailure::InvalidLeef);
    }
    let text = std::str::from_utf8(hex).map_err(|_| ParseFailure::InvalidLeef)?;
    u8::from_str_radix(text, 16).map_err(|_| ParseFailure::InvalidLeef)
}

pub(crate) fn apply_leef<'a>(text: &'a [u8], scratch: &mut StructuredScratch, out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let start = memchr::memmem::find(text, b"LEEF:").ok_or(ParseFailure::InvalidLeef)?;
    let body = &text[start + 5..];
    let is_v2 = body.starts_with(b"2.");
    split_pipes(body, if is_v2 { 7 } else { 6 }, &mut scratch.parts);
    let parts = &scratch.parts;
    if parts.len() < 6 {
        return Err(ParseFailure::InvalidLeef);
    }
    // The delimiter is read before any field is pushed, so a bad one fails with nothing half-written.
    let (delim, attrs): (u8, &[u8]) = if is_v2 && parts.len() == 7 {
        (delimiter_byte(&body[parts[5].clone()])?, &body[parts[6].clone()])
    } else {
        (b'\t', &body[parts[parts.len() - 1].clone()])
    };
    for (name, part) in LEEF_HEADER.iter().zip(parts) {
        out.push(*name, unescape_if_needed(&body[part.clone()]));
    }
    for pair in attrs.split(|&b| b == delim) {
        if let Some(eq) = memchr::memchr(b'=', pair) {
            let key = &pair[..eq];
            if !key.is_empty() {
                out.push(key, unescape_if_needed(&pair[eq + 1..]));
            }
        }
    }
    Ok(())
}
