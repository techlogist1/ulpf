mod common;
use common::*;
use ulpf_parse::{ParseFailure, Parsed};

fn run<'a>(p: &'a ulpf_parse::Parser, event: &'a [u8], out: &mut Parsed<'a>) -> Result<(), ParseFailure> {
    let mut scratch = ulpf_parse::Scratch::default();
    p.parse(event, &ctx(), &mut scratch, out)
}

#[test]
fn kv_checkpoint_style() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["action:"]
[strategy]
kind = "kv"
key_value_separator = ":"
pair_separator = "; "
quote = '"'
"#);
    let mut out = Parsed::default();
    run(&p, br#"action:"Accept"; flags:"411908"; ifdir:"inbound"; src:"10.0.0.5"; bare; product:"VPN-1 & FireWall-1""#, &mut out).unwrap();
    assert_eq!(out.fields.len(), 5);
    assert_field(&out, "action", b"Accept");
    assert_field(&out, "src", b"10.0.0.5");
    assert_field(&out, "product", b"VPN-1 & FireWall-1");
    assert_eq!(run(&p, b"nothing here", &mut out), Err(ParseFailure::NoPairs));
}

#[test]
fn delimiter_quotes_short_rows_and_extras() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = [","]
[strategy]
kind = "delimiter"
delimiter = ","
quote = '"'
fields = ["x", "_", "z"]
"#);
    let mut out = Parsed::default();
    run(&p, b"a,\"b,c\",d,e,", &mut out).unwrap();
    assert_eq!(pairs(&out), vec![
        (b"x".to_vec(), b"a".to_vec()), (b"z".to_vec(), b"d".to_vec()),
        (b"column_4".to_vec(), b"e".to_vec()), (b"column_5".to_vec(), b"".to_vec()),
    ]);
    run(&p, b"a,b", &mut out).unwrap();
    assert_eq!(out.fields.len(), 1);
    assert_field(&out, "x", b"a");
    assert_eq!(run(&p, b"", &mut out), Err(ParseFailure::NoColumns));
    let tab = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["\t"]
[strategy]
kind = "delimiter"
delimiter = "tab"
fields = ["a", "b"]
"#);
    run(&tab, b"1\t2\n", &mut out).unwrap();
    assert_field(&out, "b", b"2");
}

#[test]
fn json_flattens_nested_and_arrays() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
starts_with = "{"
[strategy]
kind = "json"
[[timestamp]]
field = "timestamp"
format = "rfc3339"
"#);
    let mut out = Parsed::default();
    run(&p, br#"{"timestamp":"2026-09-04T10:15:23.000000+0000","event_type":"alert","src_ip":"1.2.3.4","alert":{"signature":"ET X","severity":2,"nested":{"deep":true}},"tags":["a","b"],"nothing":null}"#, &mut out).unwrap();
    assert_field(&out, "alert.signature", b"ET X");
    assert_field(&out, "alert.severity", b"2");
    assert_field(&out, "alert.nested.deep", b"true");
    assert_field(&out, "tags.1", b"b");
    assert!(field(&out, "nothing").is_none());
    assert_eq!(out.timestamp.unwrap().epoch_nanos, 1_788_516_923_000_000_000);
    assert_eq!(run(&p, b"{not json", &mut out), Err(ParseFailure::InvalidJson));
    assert_eq!(run(&p, b"[1,2]", &mut out), Err(ParseFailure::InvalidJson));
}

#[test]
fn cef_header_escapes_and_spaced_values() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["CEF:"]
[envelope]
syslog = true
[strategy]
kind = "cef"
"#);
    let mut out = Parsed::default();
    run(&p, br#"<134>Sep  4 10:15:23 host CEF:0|Vendor|Product|1.0|100|Name with \| pipe|5|src=10.0.0.5 spt=1234 msg=hello world a\=b dst=1.1.1.1 cs1Label=x cs1="#, &mut out).unwrap();
    assert_field(&out, "syslog_host", b"host");
    assert_field(&out, "cef_version", b"0");
    assert_field(&out, "device_vendor", b"Vendor");
    assert_field(&out, "name", b"Name with | pipe");
    assert_field(&out, "severity", b"5");
    assert_field(&out, "src", b"10.0.0.5");
    assert_field(&out, "spt", b"1234");
    assert_field(&out, "msg", b"hello world a=b");
    assert_field(&out, "dst", b"1.1.1.1");
    assert_field(&out, "cs1", b"");
    assert_eq!(run(&p, b"no cef here", &mut out), Err(ParseFailure::InvalidCef));
    assert_eq!(run(&p, b"CEF:0|only|three", &mut out), Err(ParseFailure::InvalidCef));
}

#[test]
fn leef_versions_and_delimiters() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["LEEF:"]
[strategy]
kind = "leef"
"#);
    let mut out = Parsed::default();
    run(&p, b"LEEF:2.0|V|P|1.0|evt|^|src=1.1.1.1^dst=2.2.2.2^usrName=bob", &mut out).unwrap();
    assert_field(&out, "leef_version", b"2.0");
    assert_field(&out, "event_id", b"evt");
    assert_field(&out, "dst", b"2.2.2.2");
    assert_field(&out, "usrName", b"bob");
    run(&p, b"LEEF:2.0|V|P|1.0|evt|x09|src=1.1.1.1\tdst=3.3.3.3", &mut out).unwrap();
    assert_field(&out, "dst", b"3.3.3.3");
    run(&p, b"LEEF:1.0|V|P|1.0|evt|src=1.1.1.1\tdst=4.4.4.4", &mut out).unwrap();
    assert_field(&out, "leef_version", b"1.0");
    assert_field(&out, "dst", b"4.4.4.4");
    assert_eq!(run(&p, b"LEEF:1.0|V|P", &mut out), Err(ParseFailure::InvalidLeef));
}

#[test]
fn pattern_anchors_braces_discard_and_regex_escape_hatch() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["id="]
[strategy]
kind = "pattern"
anchor = "none"
pattern = 'id={{{id:int}}} {_:word} {rest:rest}'
"#);
    let mut out = Parsed::default();
    run(&p, b"prefix id={42} skip the  rest here\n", &mut out).unwrap();
    assert_eq!(pairs(&out), vec![(b"id".to_vec(), b"42".to_vec()), (b"rest".to_vec(), b"the  rest here".to_vec())]);

    let full = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["x"]
[strategy]
kind = "pattern"
anchor = "full"
patterns = ['x={x:int}', 'x={x:word} tail']
"#);
    assert!(run(&full, b"x=12", &mut out).is_ok());
    assert_eq!(run(&full, b"x=12 more", &mut out), Err(ParseFailure::PatternNoMatch));
    assert!(run(&full, b"x=ab tail", &mut out).is_ok());
    assert_field(&out, "x", b"ab");

    let re = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
regex = "^[A-Z]{3}"
[strategy]
kind = "pattern"
regex = '(?P<code>[A-Z]{3})-(?P<n>\d+)'
"#);
    assert!(re.matches(b"ABC-1"));
    assert!(!re.matches(b"abc-1"));
    run(&re, b"ABC-123 trailing", &mut out).unwrap();
    assert_field(&out, "code", b"ABC");
    assert_field(&out, "n", b"123");

    let quoted = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["q"]
[strategy]
kind = "pattern"
pattern = 'q={q:quoted} ip={ip:ip} mac={mac:mac} t={t:timestamp}'
"#);
    run(&quoted, br#"q="a \"b\" c" ip=fe80::1 mac=00:11:22:aa:bb:cc t=2026-09-04 10:15:23.5"#, &mut out).unwrap();
    assert_field(&out, "q", br#"a \"b\" c"#);
    assert_field(&out, "ip", b"fe80::1");
    assert_field(&out, "mac", b"00:11:22:aa:bb:cc");
    assert_field(&out, "t", b"2026-09-04 10:15:23.5");
}

#[test]
fn syslog_envelope_variants() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = [" "]
[envelope]
syslog = true
[strategy]
kind = "pattern"
pattern = '{message:rest}'
"#);
    let mut out = Parsed::default();
    run(&p, b"<34>1 2003-10-11T22:14:15.003Z mymachine.example.com su - ID47 [exampleSDID@32473 iut=\"3\" eventSource=\"App\"][x@1 a=\"\\]\"] \xEF\xBB\xBF'su root' failed", &mut out).unwrap();
    assert_field(&out, "syslog_pri", b"34");
    assert_field(&out, "syslog_facility", b"4");
    assert_field(&out, "syslog_severity", b"2");
    assert_field(&out, "syslog_host", b"mymachine.example.com");
    assert_field(&out, "syslog_app", b"su");
    assert!(field(&out, "syslog_procid").is_none());
    assert_field(&out, "syslog_msgid", b"ID47");
    assert_field(&out, "syslog_sd", b"[exampleSDID@32473 iut=\"3\" eventSource=\"App\"][x@1 a=\"\\]\"]");
    assert_field(&out, "message", b"'su root' failed");
    assert_eq!(out.timestamp.unwrap().epoch_nanos, 1_065_910_455_003_000_000);

    run(&p, b"Sep  4 10:15:23 sshd[123]: Accepted", &mut out).unwrap();
    assert_field(&out, "syslog_timestamp", b"Sep  4 10:15:23");
    assert!(field(&out, "syslog_host").is_none());
    assert_field(&out, "message", b"sshd[123]: Accepted");

    run(&p, b"<999>not a pri", &mut out).unwrap();
    assert!(field(&out, "syslog_pri").is_none());
    assert_field(&out, "message", b"<999>not a pri");

    run(&p, b"<13>date=2026-09-04 time=1", &mut out).unwrap();
    assert_field(&out, "syslog_pri", b"13");
    assert_field(&out, "message", b"date=2026-09-04 time=1");
}

#[test]
fn rfc5424_structured_data_params_become_fields_and_odd_brackets_stay_message() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = [" "]
[envelope]
syslog = true
[strategy]
kind = "pattern"
pattern = '{message:rest}'
"#);
    let mut out = Parsed::default();
    // Junos-shaped: every field is an SD-PARAM, the message part is empty
    run(&p, b"<14>1 2026-09-04T10:15:23.123Z srx RT_FLOW - RT_FLOW_SESSION_CREATE [junos@2636.1.1.1.2.129 source-address=\"10.0.0.5\" reason=\"a \\\"quoted\\\" \\] end\"]", &mut out).unwrap();
    assert_field(&out, "syslog_msgid", b"RT_FLOW_SESSION_CREATE");
    assert_field(&out, "source-address", b"10.0.0.5");
    assert_field(&out, "reason", b"a \"quoted\" ] end");
    assert!(field(&out, "message").is_none(), "an empty rest capture is no field");
    assert_eq!(out.timestamp.unwrap().epoch_nanos, 1_788_516_923_123_000_000);

    // Check Point-shaped: space inside the timestamp, bracketed body is not SD
    run(&p, b"<134>1 2026-09-04 10:15:20 gw-01 CheckPoint 13752 - [action:\"Accept\"; src:\"10.0.0.5\"]", &mut out).unwrap();
    assert_field(&out, "syslog_timestamp", b"2026-09-04 10:15:20");
    assert_field(&out, "syslog_host", b"gw-01");
    assert_field(&out, "syslog_app", b"CheckPoint");
    assert_field(&out, "syslog_procid", b"13752");
    assert!(field(&out, "syslog_msgid").is_none());
    assert!(field(&out, "syslog_sd").is_none());
    assert_field(&out, "message", b"[action:\"Accept\"; src:\"10.0.0.5\"]");
    assert_eq!(out.timestamp.unwrap().epoch_nanos, 1_788_516_920_000_000_000);

    // truncated element: the header still parses, the remainder is message text
    run(&p, b"<14>1 2026-09-04T10:15:23Z srx RT_FLOW - RT_FLOW_SESSION_CREATE [junos@2636.1.1.1.2.129 source-address=\"10.0.0.5\" sour", &mut out).unwrap();
    assert_field(&out, "syslog_msgid", b"RT_FLOW_SESSION_CREATE");
    assert!(field(&out, "source-address").is_none());
    assert_field(&out, "message", b"[junos@2636.1.1.1.2.129 source-address=\"10.0.0.5\" sour");
}

#[test]
fn delimiter_rest_feeds_subs_gated_on_earlier_columns() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = [","]
[strategy]
kind = "delimiter"
delimiter = ","
fields = ["rule", "proto"]
rest = "tail"
[[sub]]
field = "tail"
when = { proto = "tcp" }
kind = "delimiter"
delimiter = ","
fields = ["src_port", "dst_port"]
[[sub]]
field = "tail"
when = { proto = "udp" }
kind = "delimiter"
delimiter = ","
fields = ["src_port"]
"#);
    let mut out = Parsed::default();
    run(&p, b"1,tcp,100,200,x", &mut out).unwrap();
    assert_field(&out, "tail", b"100,200,x");
    assert_field(&out, "src_port", b"100");
    assert_field(&out, "dst_port", b"200");
    assert_field(&out, "column_3", b"x");
    assert_eq!(out.sub, ulpf_parse::SubStatus::Matched);

    run(&p, b"2,udp,53", &mut out).unwrap();
    assert_field(&out, "src_port", b"53");
    assert!(field(&out, "dst_port").is_none());
    assert_eq!(out.sub, ulpf_parse::SubStatus::Matched);

    run(&p, b"3,icmp,8,0", &mut out).unwrap();
    assert_field(&out, "tail", b"8,0");
    assert_eq!(out.sub, ulpf_parse::SubStatus::Uncovered, "a tail nobody is gated for");

    run(&p, b"4,tcp", &mut out).unwrap();
    assert!(field(&out, "tail").is_none(), "no columns after the named ones: no rest field");
    assert_eq!(out.sub, ulpf_parse::SubStatus::NotApplicable, "nothing for any sub to re-parse");

    run(&p, b"5,tcp,", &mut out).unwrap();
    assert!(field(&out, "tail").is_none(), "a trailing delimiter leaves nothing: no rest field");
    assert_eq!(out.sub, ulpf_parse::SubStatus::NotApplicable);
}

#[test]
fn subs_on_different_fields_all_run_and_same_field_subs_are_alternatives() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = ["src="]
[strategy]
kind = "kv"
quote = "\"'"
[[sub]]
field = "src"
kind = "pattern"
anchor = "full"
pattern = '{src_ip:ip}:{src_port:int}:{src_if:word}'
[[sub]]
field = "dst"
kind = "pattern"
anchor = "full"
pattern = '{dst_ip:ip}:{dst_port:int}:{dst_if:word}'
[[sub]]
field = "proto"
kind = "pattern"
anchor = "full"
pattern = '{protocol:word}/{service:word}'
[[sub]]
field = "proto"
kind = "pattern"
anchor = "full"
pattern = '{protocol:word}'
constants = { service = "none" }
"#);
    let mut out = Parsed::default();
    run(&p, b"src=10.0.0.5:51234:X0 dst=1.1.1.1:443:X1 proto=tcp/https appName='General HTTPS' msg=\"a b\"", &mut out).unwrap();
    assert_field(&out, "src_ip", b"10.0.0.5");
    assert_field(&out, "dst_port", b"443");
    assert_field(&out, "protocol", b"tcp");
    assert_field(&out, "service", b"https");
    assert_field(&out, "appName", b"General HTTPS");
    assert_field(&out, "msg", b"a b");
    assert_eq!(out.sub, ulpf_parse::SubStatus::Matched);

    // first sub on `proto` fails, the alternative matches; dst has a gated sub that fails
    run(&p, b"src=10.0.0.5:51234:X0 dst=bogus proto=icmp", &mut out).unwrap();
    assert_field(&out, "src_if", b"X0");
    assert_field(&out, "protocol", b"icmp");
    assert_field(&out, "service", b"none");
    assert!(field(&out, "dst_ip").is_none());
    assert_eq!(out.sub, ulpf_parse::SubStatus::NoMatch);

    // a field with subs is absent: the others still decide the status
    run(&p, b"src=10.0.0.5:51234:X0 proto=udp/dns", &mut out).unwrap();
    assert_eq!(out.sub, ulpf_parse::SubStatus::Matched);
}

#[test]
fn timestamp_slot_accepts_ctime_and_cisco_ios_shapes_without_eating_hostnames() {
    let p = parser(r#"
[parser]
name = "t"
vendor = "v"
product = "p"
[match]
contains = [" "]
[strategy]
kind = "pattern"
patterns = [
  '{ts:timestamp}: %{facility:word}-{sev:int}-{mn:word}: {message:rest}',
  '{ts:timestamp} {host:word} {message:rest}',
]
[[timestamp]]
field = "ts"
format = "auto"
"#);
    let mut out = Parsed::default();
    run(&p, b"Thu Sep  4 10:15:23 2026 gw hello", &mut out).unwrap();
    assert_field(&out, "ts", b"Thu Sep  4 10:15:23 2026");
    assert_field(&out, "host", b"gw");
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_923_000_000_000);
    assert_eq!(ts.policies, ulpf_parse::Policies::TZ_ASSUMED);

    run(&p, b"*Sep  4 10:15:23.123 UTC: %SEC-6-IPACCESSLOGP: list 1 denied", &mut out).unwrap();
    assert_field(&out, "ts", b"*Sep  4 10:15:23.123 UTC");
    assert_field(&out, "facility", b"SEC");
    assert_field(&out, "mn", b"IPACCESSLOGP");
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_923_123_000_000);
    assert_eq!(ts.policies, ulpf_parse::Policies::YEAR_ASSUMED);

    run(&p, b"Sep  4 10:15:23 CET1 not a zone", &mut out).unwrap();
    assert_field(&out, "ts", b"Sep  4 10:15:23");
    assert_field(&out, "host", b"CET1");
    run(&p, b"Sep  4 10:15:23 CET zone then host", &mut out).unwrap();
    assert_field(&out, "ts", b"Sep  4 10:15:23 CET");
    assert_field(&out, "host", b"zone");
}
