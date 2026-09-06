//! Self-describing formats. JSON goes through `serde_json::Value` and therefore
//! allocates (ponytail: acceptable while JSON sources are a minority; a streaming
//! flattener is the upgrade if they dominate the throughput file). CEF and LEEF are
//! scanned in place; their two position buffers live in `StructuredScratch`. XML is
//! tokenized by `xmlparser` (pull tokenizer over the event, zero allocation, measured
//! in D75): values are spans of the event, keys are dotted paths from the pool in
//! `Parsed`, and only a value with an entity reference is materialised.

use std::borrow::Cow;
use std::ops::Range;

use crate::{ParseFailure, Parsed};

/// Per-thread position buffers for CEF/LEEF, so a parse allocates nothing after warm-up.
#[derive(Default)]
pub(crate) struct StructuredScratch {
    parts: Vec<Range<usize>>,
    eqs: Vec<(usize, usize)>,
    /// xml: the attributes of the start tag being read, (name, value) spans.
    attrs: Vec<(Range<usize>, Range<usize>)>,
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
        // Anything longer without a hex prefix (`09`, a multi-byte character) is outside the
        // documented forms; taking its first byte would split the attributes on a digit.
        _ => return Err(ParseFailure::InvalidLeef),
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

/// Elements nest into dotted keys the way json keys do; the root element is the
/// document and is not part of any key (`<Event><System><EventID>` is `System.EventID`,
/// `<Provider Name=..>` under it is `System.Provider.Name`). Namespace prefixes are
/// stripped from names and `xmlns` attributes are not fields. An element whose only
/// attribute is `Name` and which carries text is a named value: `<Data Name="X">v</Data>`
/// is `EventData.X`, the Windows `EventData` shape. A key already present in this
/// event gets a counter (`EventData.Data`, `EventData.Data2`, ...). Whitespace-only text
/// between elements (pretty-printed input) is not a field.
pub(crate) fn apply_xml<'a>(text: &'a [u8], scratch: &mut StructuredScratch, out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    use xmlparser::{ElementEnd, Token};
    let Ok(s) = std::str::from_utf8(text) else { return Err(ParseFailure::InvalidXml) };
    let mark = out.fields.len();
    // `parts` is the element stack: the span each open element contributes to a key.
    let StructuredScratch { parts: stack, attrs, .. } = scratch;
    stack.clear();
    attrs.clear();
    let mut elements = 0usize;
    // Inside a start tag: the tokenizer ends silently on `<Event` cut before its `>`.
    let mut in_tag = false;
    // The `Name` of an open element whose only attribute it was; its text takes it.
    let mut pending: Option<Range<usize>> = None;
    for token in xmlparser::Tokenizer::from(s) {
        match token {
            Ok(Token::ElementStart { local, .. }) => {
                stack.push(local.start()..local.end());
                attrs.clear();
                pending = None;
                elements += 1;
                in_tag = true;
            }
            Ok(Token::Attribute { prefix, local, value, .. }) => {
                if prefix.as_str() != "xmlns" && local.as_str() != "xmlns" {
                    attrs.push((local.start()..local.end(), value.start()..value.end()));
                }
            }
            Ok(Token::ElementEnd { end, .. }) => {
                in_tag = false;
                let open = matches!(end, ElementEnd::Open);
                if open && attrs.len() == 1 && &text[attrs[0].0.clone()] == b"Name" {
                    pending = Some(attrs[0].1.clone());
                } else {
                    for (k, v) in attrs.iter() {
                        push_xml_field(text, stack, Some(k.clone()), &text[v.clone()], true, mark, out);
                    }
                }
                attrs.clear();
                if !open {
                    stack.pop();
                    pending = None;
                }
            }
            Ok(Token::Text { text: t }) => {
                let v = &text[t.start()..t.end()];
                if v.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                if let (Some(name), Some(top)) = (pending.take(), stack.last_mut()) {
                    *top = name;
                }
                push_xml_field(text, stack, None, v, true, mark, out);
            }
            Ok(Token::Cdata { text: t, .. }) => {
                if let (Some(name), Some(top)) = (pending.take(), stack.last_mut()) {
                    *top = name;
                }
                push_xml_field(text, stack, None, &text[t.start()..t.end()], false, mark, out);
            }
            Ok(_) => {}
            Err(_) => {
                out.fields.truncate(mark);
                return Err(ParseFailure::InvalidXml);
            }
        }
    }
    if elements == 0 || in_tag {
        out.fields.truncate(mark);
        return Err(ParseFailure::InvalidXml);
    }
    Ok(())
}

fn push_xml_field<'a>(text: &'a [u8], stack: &[Range<usize>], leaf: Option<Range<usize>>, value: &'a [u8], decode: bool, mark: usize, out: &mut Parsed<'a>) {
    let mut key = out.take_key();
    for (i, r) in stack.iter().enumerate().skip(1) {
        if i > 1 {
            key.push(b'.');
        }
        key.extend_from_slice(&text[r.clone()]);
    }
    if let Some(r) = leaf {
        if !key.is_empty() {
            key.push(b'.');
        }
        key.extend_from_slice(&text[r]);
    }
    if key.is_empty() {
        out.give_back_key(key);
        return;
    }
    // ponytail: O(fields) scan per key for a repeated sibling; an event has tens of
    // fields and equal-length keys are few, so a map would cost more than it saves.
    let base = key.len();
    let mut n = 1u32;
    while out.fields[mark..].iter().any(|f| *f.key == *key) {
        n += 1;
        key.truncate(base);
        push_decimal(&mut key, n);
    }
    let value = if decode { decode_entities(value) } else { Cow::Borrowed(value) };
    out.push(Cow::Owned(key), value);
}

// The sibling counter on the stack: `to_string` was one allocation per repeated unnamed
// element, quadratic over an unnamed `<Data>` list.
fn push_decimal(key: &mut Vec<u8>, mut n: u32) {
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    key.extend_from_slice(&buf[i..]);
}

/// `&amp; &lt; &gt; &quot; &apos; &#N; &#xN;`; anything else after `&` is kept as
/// written (an entity cut off at the end of input, an unknown name). Borrowed when the
/// value has no `&` at all: the one materialisation the xml strategy makes.
fn decode_entities(raw: &[u8]) -> Cow<'_, [u8]> {
    if !raw.contains(&b'&') {
        return Cow::Borrowed(raw);
    }
    let mut v = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'&'
            && let Some(semi) = memchr::memchr(b';', &raw[i..]).map(|p| i + p)
            && entity(&raw[i + 1..semi], &mut v)
        {
            i = semi + 1;
            continue;
        }
        v.push(raw[i]);
        i += 1;
    }
    Cow::Owned(v)
}

/// Appends the reference's bytes; false leaves `out` untouched for an unknown one.
fn entity(name: &[u8], out: &mut Vec<u8>) -> bool {
    let byte = match name {
        b"amp" => Some(b'&'),
        b"lt" => Some(b'<'),
        b"gt" => Some(b'>'),
        b"quot" => Some(b'"'),
        b"apos" => Some(b'\''),
        _ => None,
    };
    if let Some(b) = byte {
        out.push(b);
        return true;
    }
    let Some(digits) = name.strip_prefix(b"#") else { return false };
    let code = match digits.strip_prefix(b"x").or_else(|| digits.strip_prefix(b"X")) {
        Some(hex) => std::str::from_utf8(hex).ok().and_then(|h| u32::from_str_radix(h, 16).ok()),
        None => std::str::from_utf8(digits).ok().and_then(|d| d.parse::<u32>().ok()),
    };
    match code.and_then(char::from_u32) {
        Some(c) => {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            true
        }
        None => false,
    }
}
