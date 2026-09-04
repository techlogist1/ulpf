use ulpf_time::{Context, Format, format_rfc3339, parse};

const CORPUS: &str = include_str!("corpus.txt");

// "quoted" keeps surrounding whitespace; \xHH inserts a byte, \t a tab.
fn unescape(field: &str) -> Vec<u8> {
    let f = field.trim();
    let f = f
        .strip_prefix('"')
        .and_then(|f| f.strip_suffix('"'))
        .unwrap_or(f);
    let b = f.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1)) {
            (b'\\', Some(b'x')) => {
                out.push(u8::from_str_radix(&f[i + 2..i + 4], 16).expect("bad \\x escape"));
                i += 4;
            }
            (b'\\', Some(b't')) => {
                out.push(b'\t');
                i += 2;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    out
}

// The receipt field's own offset doubles as the default offset for the case.
fn context(receipt: &str) -> Context {
    let zero = Context {
        receipt_epoch_nanos: 0,
        default_offset_secs: 0,
    };
    let receipt_epoch_nanos = parse(receipt.as_bytes(), &Format::Rfc3339, &zero)
        .unwrap_or_else(|e| panic!("bad receipt {receipt}: {e:?}"))
        .epoch_nanos;
    let default_offset_secs = match &receipt.as_bytes()[receipt.len() - 6..] {
        &[sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] => {
            let v = i32::from(h1 - b'0') * 36000
                + i32::from(h2 - b'0') * 3600
                + i32::from(m1 - b'0') * 600
                + i32::from(m2 - b'0') * 60;
            if sign == b'-' { -v } else { v }
        }
        _ => 0,
    };
    Context {
        receipt_epoch_nanos,
        default_offset_secs,
    }
}

#[test]
fn corpus() {
    let mut failures = Vec::new();
    let mut cases = 0;
    for (idx, line) in CORPUS.lines().enumerate() {
        let lineno = idx + 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').collect();
        assert_eq!(fields.len(), 5, "line {lineno}: expected 5 fields");
        cases += 1;
        let input = unescape(fields[0]);
        let format = Format::from_spec(fields[1].trim())
            .unwrap_or_else(|e| panic!("line {lineno}: {}", e.message));
        let ctx = context(fields[2].trim());
        let expected = fields[3].trim();
        let mut want_policies: Vec<&str> = match fields[4].trim() {
            "-" => Vec::new(),
            p => p.split(',').map(str::trim).collect(),
        };
        want_policies.sort_unstable();

        let got = parse(&input, &format, &ctx);
        let actual = match got {
            Err(e) => format!("ERR:{}", e.reason()),
            Ok(ts) => {
                let mut s = String::new();
                if expected.starts_with("ns=") {
                    s.push_str("ns=");
                    s.push_str(&ts.epoch_nanos.to_string());
                } else {
                    format_rfc3339(ts.epoch_nanos, &mut s);
                }
                s
            }
        };
        let mut got_policies: Vec<&str> = got
            .map(|t| t.policies.names().collect())
            .unwrap_or_default();
        got_policies.sort_unstable();
        if actual != expected || got_policies != want_policies {
            failures.push(format!(
                "line {lineno}: {} | {}\n    expected {expected} {want_policies:?}\n    got      {actual} {got_policies:?}",
                String::from_utf8_lossy(&input),
                fields[1].trim()
            ));
        }
    }
    assert!(cases >= 60, "only {cases} corpus cases");
    assert!(
        failures.is_empty(),
        "{} of {cases} corpus cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
