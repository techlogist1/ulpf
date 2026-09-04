//! Syslog envelope: `<pri>` plus an RFC 5424 or RFC 3164 header. Lenient by design —
//! every part is optional, because relays add and strip headers unpredictably and
//! Fortinet puts its timestamp in the body.

use std::sync::OnceLock;

use regex::bytes::Regex;

use crate::Parsed;

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
pub(crate) fn strip_syslog<'a>(event: &'a [u8], out: &mut Parsed<'a>) -> &'a [u8] {
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
    for name in names {
        let end = token_end(b, i);
        let tok = &b[i..end];
        if tok.is_empty() {
            return None;
        }
        if tok != b"-" {
            out.push(name, tok);
        }
        i = skip_spaces(b, end);
    }
    // STRUCTURED-DATA: `-` or one or more `[...]` elements.
    let sd_start = i;
    if b.get(i) == Some(&b'-') {
        i += 1;
    } else {
        while b.get(i) == Some(&b'[') {
            let mut j = i + 1;
            while j < b.len() && b[j] != b']' {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j >= b.len() {
                return None;
            }
            i = j + 1;
        }
        if i == sd_start {
            return None;
        }
        out.push(&b"syslog_sd"[..], &b[sd_start..i]);
    }
    let mut msg = &b[skip_spaces(b, i)..];
    if let Some(stripped) = msg.strip_prefix(b"\xEF\xBB\xBF") {
        msg = stripped;
    }
    Some(msg)
}
