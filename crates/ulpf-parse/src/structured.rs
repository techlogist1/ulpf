//! Self-describing formats. JSON goes through `serde_json::Value` and therefore
//! allocates (ponytail: acceptable while JSON sources are a minority; a streaming
//! flattener is the upgrade if they dominate the throughput file). CEF and LEEF are
//! scanned in place.

use std::borrow::Cow;

use crate::{ParseFailure, Parsed};

pub(crate) fn apply_json<'a>(text: &'a [u8], out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let value: serde_json::Value = serde_json::from_slice(text).map_err(|_| ParseFailure::InvalidJson)?;
    let serde_json::Value::Object(map) = value else { return Err(ParseFailure::InvalidJson) };
    let mut prefix = String::new();
    for (k, v) in map {
        flatten(&mut prefix, &k, &v, out);
    }
    Ok(())
}

fn flatten<'a>(prefix: &mut String, key: &str, v: &serde_json::Value, out: &mut Parsed<'a>) {
    let base = prefix.len();
    if !prefix.is_empty() {
        prefix.push('.');
    }
    prefix.push_str(key);
    match v {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                flatten(prefix, k, v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                flatten(prefix, &i.to_string(), v, out);
            }
        }
        serde_json::Value::Null => {}
        serde_json::Value::String(s) => out.push(Cow::Owned(prefix.clone().into_bytes()), Cow::Owned(s.clone().into_bytes())),
        other => out.push(Cow::Owned(prefix.clone().into_bytes()), Cow::Owned(other.to_string().into_bytes())),
    }
    prefix.truncate(base);
}

static CEF_HEADER: [&[u8]; 7] = [
    b"cef_version", b"device_vendor", b"device_product", b"device_version", b"signature_id", b"name", b"severity",
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

/// Splits on unescaped `|`, yielding at most `max` pieces (the last takes the rest).
fn split_pipes(text: &[u8], max: usize) -> Vec<&[u8]> {
    let mut parts = Vec::with_capacity(max);
    let mut start = 0;
    let mut i = 0;
    while i < text.len() && parts.len() + 1 < max {
        match text[i] {
            b'\\' => i += 2,
            b'|' => {
                parts.push(&text[start..i]);
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    parts.push(&text[start.min(text.len())..]);
    parts
}

pub(crate) fn apply_cef<'a>(text: &'a [u8], out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let start = memchr::memmem::find(text, b"CEF:").ok_or(ParseFailure::InvalidCef)?;
    let body = &text[start + 4..];
    let parts = split_pipes(body, 8);
    if parts.len() < 8 {
        return Err(ParseFailure::InvalidCef);
    }
    for (name, part) in CEF_HEADER.iter().zip(&parts) {
        out.push(*name, unescape_if_needed(part));
    }
    extension_pairs(parts[7], out);
    Ok(())
}

/// CEF extension: `key=value key2=value with spaces`. A key is the run of key bytes
/// immediately before an unescaped `=`; a value runs until the next such key.
fn extension_pairs<'a>(ext: &'a [u8], out: &mut Parsed<'a>) {
    let n = ext.len();
    let is_key_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-';
    // positions of every unescaped '=' that has a key before it
    let mut eqs: Vec<(usize, usize)> = Vec::new(); // (key_start, eq_pos)
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
    for (idx, &(ks, eq)) in eqs.iter().enumerate() {
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

pub(crate) fn apply_leef<'a>(text: &'a [u8], out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let start = memchr::memmem::find(text, b"LEEF:").ok_or(ParseFailure::InvalidLeef)?;
    let body = &text[start + 5..];
    let is_v2 = body.starts_with(b"2.");
    let parts = split_pipes(body, if is_v2 { 7 } else { 6 });
    if parts.len() < 6 {
        return Err(ParseFailure::InvalidLeef);
    }
    for (name, part) in LEEF_HEADER.iter().zip(&parts) {
        out.push(*name, unescape_if_needed(part));
    }
    // LEEF 2.0 has an optional delimiter field: a literal byte or `xHH`.
    let (delim, attrs): (u8, &[u8]) = if is_v2 && parts.len() == 7 {
        let d = parts[5];
        let byte = if d.is_empty() {
            b'\t'
        } else if d.len() == 3 && (d[0] == b'x' || d[0] == b'X') {
            u8::from_str_radix(std::str::from_utf8(&d[1..]).unwrap_or("09"), 16).unwrap_or(b'\t')
        } else {
            d[0]
        };
        (byte, parts[6])
    } else {
        (b'\t', parts[parts.len() - 1])
    };
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
