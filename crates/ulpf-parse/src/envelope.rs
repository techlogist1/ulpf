//! Syslog envelope: `<pri>` plus an RFC 5424 or RFC 3164 header. Lenient by design —
//! every part is optional, because relays add and strip headers unpredictably and
//! Fortinet puts its timestamp in the body.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::bytes::Regex;

use crate::Parsed;
use crate::kv::unescape;

static SEVERITY: [&[u8]; 8] = [b"0", b"1", b"2", b"3", b"4", b"5", b"6", b"7"];
static FACILITY: [&[u8]; 24] = [
    b"0", b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"10", b"11", b"12", b"13",
    b"14", b"15", b"16", b"17", b"18", b"19", b"20", b"21", b"22", b"23",
];

fn bsd_timestamp() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?-u)^(?:[A-Z][a-z]{2} +[0-9]{1,2}(?: +[0-9]{4})? +[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?: +[0-9]{4})?|[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?)",
        )
        .unwrap()
    })
}

#[inline]
fn skip_spaces(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

#[inline]
fn token_end(b: &[u8], i: usize) -> usize {
    memchr::memchr2(b' ', b'\t', &b[i..]).map_or(b.len(), |p| i + p)
}

/// Strips the envelope from `event`, pushing header fields, and returns the message.
/// Exported for the inference engine, which must see bodies the way the runtime does.
pub fn strip_syslog<'a>(event: &'a [u8], out: &mut Parsed<'a>) -> &'a [u8] {
    // A UTF-8 byte-order mark is what Windows-exported logs start with; it is not data.
    let event = event.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(event);
    let mut i = 0;
    if event.first() == Some(&b'<') {
        let end = event.iter().take(5).position(|&c| c == b'>');
        if let Some(e) = end {
            let digits = &event[1..e];
            if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) {
                let pri: usize = digits.iter().fold(0, |acc, &d| acc * 10 + (d - b'0') as usize);
                if pri < 192 {
                    out.push(&b"syslog_pri"[..], digits);
                    out.push(&b"syslog_facility"[..], FACILITY[pri / 8]);
                    out.push(&b"syslog_severity"[..], SEVERITY[pri % 8]);
                    i = e + 1;
                }
            }
        }
    }
    let rest = &event[i..];
    if rest.starts_with(b"1 ")
        && rest.get(2).is_some_and(|c| c.is_ascii_digit() || *c == b'-')
        && let Some(msg) = rfc5424(rest, out)
    {
        return msg;
    }
    // RFC 3164: TIMESTAMP SP HOSTNAME SP MSG, each optional in practice.
    let mut i = 0;
    if let Some(m) = bsd_timestamp().find(rest) {
        out.push(&b"syslog_timestamp"[..], m.as_bytes());
        i = skip_spaces(rest, m.end());
        let end = token_end(rest, i);
        let tok = &rest[i..end];
        let looks_like_host = !tok.is_empty()
            && !tok.contains(&b'=')
            && !tok.contains(&b'[')
            && !tok.ends_with(b":")
            && !matches!(tok[0], b'%' | b':' | b'<');
        if looks_like_host {
            out.push(&b"syslog_host"[..], tok);
            i = skip_spaces(rest, end);
            // Cisco ASA "device-id" form: `host : %ASA-...`
            if rest[i..].starts_with(b": ") {
                i = skip_spaces(rest, i + 1);
            }
        }
    }
    &rest[i..]
}

fn rfc5424<'a>(b: &'a [u8], out: &mut Parsed<'a>) -> Option<&'a [u8]> {
    let mut i = 2;
    let names: [&[u8]; 5] = [b"syslog_timestamp", b"syslog_host", b"syslog_app", b"syslog_procid", b"syslog_msgid"];
    for (idx, name) in names.iter().enumerate() {
        let mut end = token_end(b, i);
        if idx == 0 && is_date(&b[i..end]) {
            // Check Point Log Exporter writes the timestamp as `YYYY-MM-DD HH:MM:SS`
            let t = skip_spaces(b, end);
            if is_clock(&b[t..token_end(b, t)]) {
                end = token_end(b, t);
            }
        }
        let tok = &b[i..end];
        if tok.is_empty() {
            return None;
        }
        if tok != b"-" {
            out.push(*name, tok);
        }
        i = skip_spaces(b, end);
    }
    // STRUCTURED-DATA: `-`, or `[SD-ID PARAM="VALUE" ...]` elements whose params become
    // fields (Juniper puts the whole event there). Brackets that are not valid structured
    // data (Check Point's `[key:"value"; ...]`, a truncated element) are message text.
    let sd_start = i;
    if b.get(i) == Some(&b'-') {
        i += 1;
    } else {
        let mut elements = 0;
        while b.get(i) == Some(&b'[') {
            let Some(j) = sd_element_end(b, i) else { break };
            if !sd_params(&b[i + 1..j], out) {
                break;
            }
            i = j + 1;
            elements += 1;
        }
        if elements > 0 {
            out.push(&b"syslog_sd"[..], &b[sd_start..i]);
        } else if b.get(i) != Some(&b'[') {
            return None;
        }
    }
    let mut msg = &b[skip_spaces(b, i)..];
    if let Some(stripped) = msg.strip_prefix(b"\xEF\xBB\xBF") {
        msg = stripped;
    }
    Some(msg)
}

fn is_date(t: &[u8]) -> bool {
    t.len() == 10 && t.iter().enumerate().all(|(k, c)| if matches!(k, 4 | 7) { *c == b'-' } else { c.is_ascii_digit() })
}

fn is_clock(t: &[u8]) -> bool {
    t.len() >= 8 && t[..8].iter().enumerate().all(|(k, c)| if matches!(k, 2 | 5) { *c == b':' } else { c.is_ascii_digit() })
}

/// Index of the `]` closing the element that opens at `start`, honouring `\` escapes.
fn sd_element_end(b: &[u8], start: usize) -> Option<usize> {
    let mut j = start + 1;
    while j < b.len() && b[j] != b']' {
        if b[j] == b'\\' {
            j += 1;
        }
        j += 1;
    }
    (j < b.len()).then_some(j)
}

fn sd_name_ok(n: &[u8]) -> bool {
    !n.is_empty() && n.len() <= 32 && n.iter().all(|&c| (0x21..=0x7e).contains(&c) && !matches!(c, b'=' | b']' | b'"'))
}

/// Pushes the `PARAM-NAME="PARAM-VALUE"` pairs of one element body (brackets excluded).
/// Returns false, having pushed nothing, when the text is not RFC 5424 structured data.
fn sd_params<'a>(elem: &'a [u8], out: &mut Parsed<'a>) -> bool {
    let mark = out.fields.len();
    let id_end = token_end(elem, 0);
    if !sd_name_ok(&elem[..id_end]) {
        return false;
    }
    let mut i = id_end;
    while i < elem.len() {
        i = skip_spaces(elem, i);
        if i >= elem.len() {
            break;
        }
        let Some(eq) = memchr::memchr(b'=', &elem[i..]).map(|p| i + p) else {
            out.fields.truncate(mark);
            return false;
        };
        let name = &elem[i..eq];
        if !sd_name_ok(name) || elem.get(eq + 1) != Some(&b'"') {
            out.fields.truncate(mark);
            return false;
        }
        let vs = eq + 2;
        let mut k = vs;
        let mut escaped = false;
        while k < elem.len() && elem[k] != b'"' {
            if elem[k] == b'\\' {
                escaped = true;
                k += 1;
            }
            k += 1;
        }
        if k >= elem.len() {
            out.fields.truncate(mark);
            return false;
        }
        let raw = &elem[vs..k];
        let value: Cow<'a, [u8]> = if escaped { Cow::Owned(unescape(raw)) } else { Cow::Borrowed(raw) };
        out.push(name, value);
        i = k + 1;
    }
    true
}
