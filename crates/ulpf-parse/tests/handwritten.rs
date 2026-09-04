mod common;
use common::*;
use ulpf_parse::{Policies, Registry, SubStatus, load_dir};

fn registry() -> Registry {
    let report = load_dir(&repo().join("parsers")).unwrap();
    if let Some(e) = report.errors.first() {
        panic!("{e}");
    }
    Registry::new(report.parsers)
}

#[test]
fn fortinet_sample_parses_with_kv_and_timestamp_policies() {
    let reg = registry();
    let idx = reg.index_of("fortinet_fortigate").unwrap();
    let p = reg.get(idx);
    let evs = events(&repo().join("samples/fortinet_fortigate.log"));
    assert_eq!(evs.len(), 6);
    let mut scratch = reg.scratch();
    let mut out = ulpf_parse::Parsed::default();

    assert_eq!(reg.detect(&evs[0], None), Some(idx));
    p.parse(&evs[0], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "syslog_pri", b"189");
    assert_field(&out, "syslog_facility", b"23");
    assert_field(&out, "syslog_severity", b"5");
    assert_field(&out, "devname", b"FGT60E-LAB");
    assert_field(&out, "srcip", b"10.0.0.5");
    assert_field(&out, "action", b"accept");
    assert_field(&out, "countapp", b"1");
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_923_123_456_789);
    assert!(ts.policies.is_empty(), "{:?}", ts.policies.names().collect::<Vec<_>>());
    assert_eq!(&**out.timestamp_text.as_ref().unwrap(), b"1788516923123456789");
    assert_eq!(out.sub, SubStatus::NotApplicable);

    // relayed with a no-year header, CRLF, no eventtime → date+time fallback
    p.parse(&evs[3], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "syslog_timestamp", b"Sep  4 10:15:26");
    assert_field(&out, "syslog_host", b"fgt-relay");
    assert_field(&out, "appcat", b"unscanned");
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_926_000_000_000);
    assert!(ts.policies.contains(Policies::TZ_ASSUMED));
    assert!(!ts.policies.contains(Policies::YEAR_ASSUMED));
    assert_eq!(&**out.timestamp_text.as_ref().unwrap(), b"2026-09-04 10:15:26");

    // multi-line event, escaped quote, non-UTF-8 byte
    p.parse(&evs[4], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "user", b"admin\xe9");
    assert_field(&out, "msg", b"Administrator admin logged in successfully from https(10.0.0.5) said \"hello\"");
    assert!(field(&out, "continuation").is_none());

    // truncated mid-value still yields what is there
    p.parse(&evs[5], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "action", b"acc");
    assert_field(&out, "sessionid", b"987658");
}

#[test]
fn cisco_asa_sample_parses_header_subs_and_envelopes() {
    let reg = registry();
    let idx = reg.index_of("cisco_asa").unwrap();
    let p = reg.get(idx);
    let evs = events(&repo().join("samples/cisco_asa.log"));
    assert_eq!(evs.len(), 13);
    let mut scratch = reg.scratch();
    let mut out = ulpf_parse::Parsed::default();

    for (i, e) in evs.iter().enumerate() {
        assert_eq!(reg.detect(e, None), Some(idx), "event {i} not detected");
    }

    p.parse(&evs[0], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "syslog_timestamp", b"Sep 04 2026 10:15:23");
    assert_field(&out, "syslog_host", b"asa-edge-01");
    assert_field(&out, "severity", b"6");
    assert_field(&out, "msg_id", b"302013");
    assert_field(&out, "direction", b"outbound");
    assert_field(&out, "protocol", b"TCP");
    assert_field(&out, "dst_ip", b"142.250.72.14");
    assert_field(&out, "dst_port", b"443");
    assert_field(&out, "src_ip", b"10.0.0.5");
    assert_field(&out, "src_mapped_ip", b"203.0.113.5");
    assert_eq!(out.sub, SubStatus::Matched);
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_923_000_000_000);
    assert!(ts.policies.contains(Policies::TZ_ASSUMED));

    // no-year relay header, deny with ACL
    p.parse(&evs[1], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "msg_id", b"106023");
    assert_field(&out, "protocol", b"tcp");
    assert_field(&out, "src_ip", b"203.0.113.9");
    assert_field(&out, "dst_port", b"22");
    assert_field(&out, "acl", b"outside_in");
    assert_field(&out, "action", b"deny");
    assert_field(&out, "hash_1", b"0x8ed66b60");
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_924_000_000_000);
    assert!(ts.policies.contains(Policies::YEAR_ASSUMED));
    assert_eq!(&**out.timestamp_text.as_ref().unwrap(), b"Sep  4 10:15:24");

    p.parse(&evs[2], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "bytes", b"6912");
    assert_field(&out, "teardown_reason", b" TCP FINs");

    p.parse(&evs[3], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "acl", b"inside_out");
    assert_field(&out, "action", b"permitted");
    assert_field(&out, "src_port", b"51235");

    // double space after "server =" is real ASA output
    p.parse(&evs[5], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "server_ip", b"10.0.0.2");
    assert_field(&out, "user", b"jdoe");
    assert_eq!(out.sub, SubStatus::Matched);

    p.parse(&evs[6], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "src_ip", b"203.0.113.9");
    assert_field(&out, "action", b"deny");

    p.parse(&evs[7], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "direction", b"inbound");
    assert_field(&out, "src_ip", b"198.51.100.7");
    assert_field(&out, "dst_ip", b"10.0.0.53");

    p.parse(&evs[8], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "icmp_type", b"8");
    assert_field(&out, "icmp_code", b"0");
    assert_eq!(out.sub, SubStatus::Matched);

    p.parse(&evs[9], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "user", b"j\xf8rgen");
    assert_field(&out, "assigned_ip", b"10.99.0.5");

    // RFC 5424 framing, message id with no sub pattern
    p.parse(&evs[10], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "syslog_host", b"asa-edge-01");
    assert_field(&out, "syslog_timestamp", b"2026-09-04T10:15:33.120Z");
    assert_field(&out, "msg_id", b"609001");
    assert_eq!(out.sub, SubStatus::NotApplicable);
    let ts = out.timestamp.unwrap();
    assert_eq!(ts.epoch_nanos, 1_788_516_933_120_000_000);
    assert!(ts.policies.is_empty());

    // no header at all
    p.parse(&evs[11], &ctx(), &mut scratch, &mut out).unwrap();
    assert!(field(&out, "syslog_pri").is_none());
    assert_field(&out, "msg_id", b"302014");
    assert_field(&out, "bytes", b"312");
    assert!(out.timestamp.is_none());
    assert!(out.timestamp_error.is_none());

    // truncated: header parses, sub does not
    p.parse(&evs[12], &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "msg_id", b"302013");
    assert_eq!(out.sub, SubStatus::NoMatch);
    assert!(field(&out, "dst_ip").is_none());
}
