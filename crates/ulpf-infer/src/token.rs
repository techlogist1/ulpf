//! Tokenizer: a line body becomes constant candidates (words, punctuation, spaces) and
//! variable candidates (typed atoms, quoted strings). Atoms are recognised with the
//! regexes the pattern strategy compiles, so a slot the engine emits accepts exactly the
//! text the tokenizer saw; the extra strictness here (octet range, real IPv6 shape) only
//! ever turns an atom back into a word, never the reverse.

use std::sync::OnceLock;

use regex::bytes::Regex;
use ulpf_parse::SlotKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Space,
    Punct,
    Word,
    Quoted,
    /// A long `hh:hh:...` hex chain (netfilter's 14-byte MAC field): opaque, variable,
    /// aligns only with another chain so it can never stand in for an interface name.
    Chain,
    Atom(SlotKind),
}

impl Kind {
    /// Variable by nature: the prototype's worst error was freezing an address into a
    /// constant, so atoms and quoted strings start as slots and are never constants.
    pub fn is_variable(self) -> bool {
        matches!(self, Kind::Quoted | Kind::Chain | Kind::Atom(_))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tok<'a> {
    pub kind: Kind,
    pub text: &'a [u8],
}

const ATOM_ORDER: [SlotKind; 7] = [
    SlotKind::Timestamp, SlotKind::Mac, SlotKind::Ipv6, SlotKind::Ipv4, SlotKind::Hex, SlotKind::Float, SlotKind::Int,
];

fn atom_regexes() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        ATOM_ORDER
            .iter()
            .map(|k| {
                let body = match k {
                    // Bare hex digits are words (`face`, `deadbeef`); only the 0x form is unambiguous.
                    SlotKind::Hex => r"0[xX][0-9A-Fa-f]+".to_string(),
                    SlotKind::Float => r"[+-]?[0-9]+\.[0-9]+".to_string(),
                    k => k.regex(false).to_string(),
                };
                Regex::new(&format!("(?s-u)^(?:{body})")).expect("slot regexes compile")
            })
            .collect()
    })
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn strict(kind: SlotKind, text: &[u8], rest: &[u8]) -> bool {
    let next = rest.first().copied();
    let after = rest.get(1).copied();
    let boundary = next.is_none_or(|b| !is_word_byte(b));
    // `3000-0148` and `192.168.1` are ids, not a number followed by punctuation
    let continues_id = matches!(next, Some(b'.') | Some(b'-')) && after.is_some_and(is_word_byte);
    match kind {
        SlotKind::Ipv4 => boundary && !(next == Some(b'.') && after.is_some_and(|b| b.is_ascii_digit())) && text.split(|b| *b == b'.').all(|o| o.len() <= 3 && o.iter().fold(0u32, |a, d| a * 10 + (d - b'0') as u32) <= 255),
        SlotKind::Ipv6 => {
            let colons = text.iter().filter(|b| **b == b':').count();
            let hex_digits = text.iter().filter(|b| b.is_ascii_hexdigit()).count();
            // `addr:port` is fine when the address is complete; a longer hex chain is not an address
            let port_follows = next == Some(b':') && after.is_some_and(|b| b.is_ascii_digit()) && rest[1..].iter().take_while(|b| b.is_ascii_digit()).count() <= 5 && rest.get(1 + rest[1..].iter().take_while(|b| b.is_ascii_digit()).count()).is_none_or(|b| !is_word_byte(*b) && *b != b':');
            (boundary || port_follows) && hex_digits >= 2 && (text.windows(2).any(|w| w == b"::") || colons == 7)
        }
        SlotKind::Mac => boundary && next != Some(b':'),
        SlotKind::Int | SlotKind::Float => boundary && !continues_id,
        _ => boundary,
    }
}

/// Splits a line body (terminators already removed) into tokens.
pub fn tokenize(body: &[u8]) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    // a JSON object's keys are its schema: `"key":` is a constant, only the value varies
    let json = body.first() == Some(&b'{');
    let mut i = 0;
    while i < body.len() {
        let b = body[i];
        if b == b' ' || b == b'\t' {
            let mut j = i;
            while j < body.len() && (body[j] == b' ' || body[j] == b'\t') {
                j += 1;
            }
            out.push(Tok { kind: Kind::Space, text: &body[i..j] });
            i = j;
            continue;
        }
        // netfilter's 14-byte `MAC=aa:bb:...` chain is one opaque word, not an IPv6 address
        if let Some(len) = hex_chain(body, i) {
            out.push(Tok { kind: Kind::Chain, text: &body[i..i + len] });
            i += len;
            continue;
        }
        // `(SYN,ACK)`, `(none)`, `[WAN_IN-default-D]`: a bracketed value with no spaces is one
        // word, so a flag list or a rule name aligns as one token
        if let Some(len) = bracket_group(body, i) {
            out.push(Tok { kind: Kind::Word, text: &body[i..i + len] });
            i += len;
            continue;
        }
        if let Some((kind, len)) = atom_at(body, i) {
            out.push(Tok { kind: Kind::Atom(kind), text: &body[i..i + len] });
            i += len;
            continue;
        }
        if b == b'"'
            && let Some(end) = quoted_end(body, i)
        {
            let kind = if json && body.get(end) == Some(&b':') { Kind::Word } else { Kind::Quoted };
            out.push(Tok { kind, text: &body[i..end] });
            i = end;
            continue;
        }
        if is_word_byte(b) {
            let mut j = i + 1;
            loop {
                while j < body.len() && is_word_byte(body[j]) {
                    j += 1;
                }
                // `gw.local`, `WAN_IN-default-D`, `3000-0148` stay whole; a trailing `.` does not.
                if j + 1 < body.len() && matches!(body[j], b'.' | b'-') && is_word_byte(body[j + 1]) {
                    j += 1;
                    continue;
                }
                break;
            }
            out.push(Tok { kind: Kind::Word, text: &body[i..j] });
            i = j;
            continue;
        }
        out.push(Tok { kind: Kind::Punct, text: &body[i..i + 1] });
        i += 1;
    }
    out
}

/// Length of a `hh:hh:hh:...` chain of seven or more two-digit hex groups starting at `i`.
fn hex_chain(body: &[u8], i: usize) -> Option<usize> {
    if i > 0 && body[i - 1] == b':' {
        return None;
    }
    let mut j = i;
    let mut groups = 0;
    loop {
        if j + 2 <= body.len() && body[j].is_ascii_hexdigit() && body[j + 1].is_ascii_hexdigit() && body.get(j + 2).is_none_or(|b| !is_word_byte(*b)) {
            groups += 1;
            j += 2;
        } else {
            break;
        }
        if body.get(j) == Some(&b':') && j + 1 < body.len() && body[j + 1].is_ascii_hexdigit() {
            j += 1;
        } else {
            break;
        }
    }
    (groups >= 7).then_some(j - i)
}

/// Length of `(...)` or `[...]` at `i` when the content has no spaces, contains a letter,
/// and closes on this line.
fn bracket_group(body: &[u8], i: usize) -> Option<usize> {
    let close = match body[i] {
        b'(' => b')',
        b'[' => b']',
        _ => return None,
    };
    let mut j = i + 1;
    let mut letter = false;
    while j < body.len() {
        let b = body[j];
        if b == close {
            return (letter && j > i + 1).then_some(j + 1 - i);
        }
        if b == b' ' || b == b'\t' || b == b'(' || b == b'[' || b == b'"' {
            return None;
        }
        letter |= b.is_ascii_alphabetic();
        j += 1;
    }
    None
}

fn atom_at(body: &[u8], i: usize) -> Option<(SlotKind, usize)> {
    let b = body[i];
    // an address never starts right after a colon: that is the tail of a longer chain
    let after_colon = i > 0 && body[i - 1] == b':';
    // Every atom starts with a digit, a hex letter (IPv6, MAC), a month name or weekday (timestamp), or a sign.
    if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'*' || b == b'.') {
        return None;
    }
    for (kind, re) in ATOM_ORDER.iter().zip(atom_regexes()) {
        if after_colon && matches!(kind, SlotKind::Ipv6 | SlotKind::Mac) {
            continue;
        }
        if let Some(m) = re.find(&body[i..]) {
            let text = &body[i..i + m.end()];
            if strict(*kind, text, &body[i + m.end()..]) {
                return Some((*kind, m.end()));
            }
        }
    }
    None
}

/// Index one past the closing quote of the string opening at `start`, if it closes on
/// this line (backslash escapes honoured).
fn quoted_end(body: &[u8], start: usize) -> Option<usize> {
    let mut j = start + 1;
    while j < body.len() {
        match body[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// Same shape for alignment purposes: identical constants, or variables of the same kind.
pub fn same_shape(a: Kind, at: &[u8], b: Kind, bt: &[u8]) -> bool {
    match (a, b) {
        (Kind::Space, Kind::Space) | (Kind::Quoted, Kind::Quoted) | (Kind::Chain, Kind::Chain) => true,
        (Kind::Word, Kind::Word) | (Kind::Punct, Kind::Punct) => at == bt,
        (Kind::Atom(x), Kind::Atom(y)) => x == y || (ip_like(x) && ip_like(y)) || (num_like(x) && num_like(y)),
        _ => false,
    }
}

pub fn ip_like(k: SlotKind) -> bool {
    matches!(k, SlotKind::Ip | SlotKind::Ipv4 | SlotKind::Ipv6)
}

pub fn num_like(k: SlotKind) -> bool {
    matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(Kind, &str)> {
        tokenize(s.as_bytes()).into_iter().filter(|t| t.kind != Kind::Space).map(|t| (t.kind, std::str::from_utf8(t.text).unwrap())).collect()
    }

    #[test]
    fn atoms_words_and_punctuation() {
        let t = kinds("firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:55, proto TCP (SYN), 203.0.113.9:44321->10.0.0.1:443, len 60");
        assert_eq!(t[0], (Kind::Word, "firewall"));
        assert_eq!(t[1], (Kind::Punct, ","));
        assert!(t.contains(&(Kind::Word, "src-mac")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Mac), "00:11:22:33:44:55")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Ipv4), "203.0.113.9")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Int), "44321")));
        assert!(t.contains(&(Kind::Punct, "-")));
        assert!(t.contains(&(Kind::Punct, ">")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Int), "60")));
    }

    #[test]
    fn times_are_not_ipv6_and_ids_stay_words() {
        let t = kinds("at 10:15:23 id 3000-0148 v6 2001:db8::1 full 2001:0db8:85a3:0000:0000:8a2e:0370:7334 tos 0x00 ver 1.1 sig -67 gw.local end.");
        assert!(t.contains(&(Kind::Atom(SlotKind::Int), "10")));
        assert!(t.contains(&(Kind::Word, "3000-0148")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Ipv6), "2001:db8::1")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Ipv6), "2001:0db8:85a3:0000:0000:8a2e:0370:7334")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Hex), "0x00")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Float), "1.1")));
        assert!(t.contains(&(Kind::Atom(SlotKind::Int), "-67")));
        assert!(t.contains(&(Kind::Word, "gw.local")));
        assert!(t.contains(&(Kind::Word, "end")));
        assert_eq!(t.last().unwrap(), &(Kind::Punct, "."));
        assert!(!t.iter().any(|(k, s)| matches!(k, Kind::Atom(SlotKind::Ipv6)) && *s == "10:15:23"));
    }

    #[test]
    fn mac_chains_and_bracket_groups_are_single_words() {
        let t1 = kinds("kernel: [WAN_IN-default-D]IN=eth0 OUT= MAC=9e:62:8f:59:f1:11:f1:24:64:24:f6:25:08:00 SRC=26.24.119.87 PROTO=TCP SPT=39021 ACK PSH URGP=0");
        assert!(t1.contains(&(Kind::Word, "[WAN_IN-default-D]")));
        assert!(t1.contains(&(Kind::Chain, "9e:62:8f:59:f1:11:f1:24:64:24:f6:25:08:00")));
        assert!(!t1.iter().any(|(k, _)| matches!(k, Kind::Atom(SlotKind::Ipv6))), "{t1:?}");
        assert!(t1.contains(&(Kind::Atom(SlotKind::Ipv4), "26.24.119.87")), "{t1:?}");
        let t2 = kinds("src 886c:7f90:666a:e822:9e1:2424:2f5:aac1:25753->10.0.242.169:80, len 1056");
        assert!(t2.contains(&(Kind::Atom(SlotKind::Ipv6), "886c:7f90:666a:e822:9e1:2424:2f5:aac1")), "{t2:?}");
        assert!(t2.contains(&(Kind::Atom(SlotKind::Int), "25753")));
        let t3 = kinds("proto TCP (SYN,ACK), out:(none), sshd[1201]: (root) CMD (run-parts /etc/cron.hourly) x (8)");
        assert!(t3.contains(&(Kind::Word, "(SYN,ACK)")));
        assert!(t3.contains(&(Kind::Word, "(none)")));
        assert!(t3.contains(&(Kind::Atom(SlotKind::Int), "1201")));
        assert!(t3.contains(&(Kind::Word, "(root)")));
        assert!(t3.contains(&(Kind::Word, "run-parts")));
        assert!(t3.contains(&(Kind::Atom(SlotKind::Int), "8")));
        let t4 = kinds("src-mac 00:11:22:33:44:55, x");
        assert!(t4.contains(&(Kind::Atom(SlotKind::Mac), "00:11:22:33:44:55")));
    }

    #[test]
    fn quoted_strings_and_timestamps() {
        let t = kinds(r#"203.0.113.9 - - [04/Sep/2026:10:15:23 +0000] "GET /a?b=1 HTTP/1.1" 200 512 "-" "Mozilla/5.0 (X11)""#);
        assert_eq!(t[0], (Kind::Atom(SlotKind::Ipv4), "203.0.113.9"));
        assert!(t.contains(&(Kind::Atom(SlotKind::Timestamp), "04/Sep/2026:10:15:23 +0000")));
        assert!(t.contains(&(Kind::Quoted, "\"GET /a?b=1 HTTP/1.1\"")));
        assert!(t.contains(&(Kind::Quoted, "\"-\"")));
        let t = kinds("Thu Sep  4 10:15:23 2026 203.0.113.9:44321 TLS: Initial packet");
        assert_eq!(t[0], (Kind::Atom(SlotKind::Timestamp), "Thu Sep  4 10:15:23 2026"));
        let t = kinds("msg=\"unterminated");
        assert!(t.contains(&(Kind::Punct, "\"")));
    }

    #[test]
    fn json_keys_are_constants_and_json_values_stay_quoted() {
        let t = kinds(r#"{"ts":1.5,"id.orig_h":"10.0.0.1","q":"a:b"}"#);
        assert!(t.contains(&(Kind::Word, "\"ts\"")));
        assert!(t.contains(&(Kind::Word, "\"id.orig_h\"")));
        assert!(t.contains(&(Kind::Quoted, "\"10.0.0.1\"")));
        assert!(t.contains(&(Kind::Quoted, "\"a:b\"")), "{t:?}");
        // outside a JSON object a quoted string before a colon is still a value
        let t = kinds(r#"x "ts":1"#);
        assert!(t.contains(&(Kind::Quoted, "\"ts\"")));
    }
}
