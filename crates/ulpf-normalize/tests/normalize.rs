use std::path::PathBuf;

use serde_json::Value;
use ulpf_normalize::{Mapping, Provenance, load_dir, load_files};
use ulpf_parse::{Context, Parsed, Registry, Scratch};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mapping() -> Mapping {
    let mut report = load_dir(&repo().join("mappings")).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>());
    assert_eq!(report.mappings.len(), 1);
    report.mappings.remove(0)
}

fn registry() -> Registry {
    let report = ulpf_parse::load_dir(&repo().join("parsers")).unwrap();
    assert!(report.errors.is_empty());
    Registry::new(report.parsers)
}

fn events(name: &str) -> Vec<Vec<u8>> {
    let bytes = std::fs::read(repo().join("samples").join(name)).unwrap();
    ulpf_store::Framer::new(&bytes, true).map(|r| bytes[r].to_vec()).collect()
}

const RECEIPT: i64 = 1_788_523_200_000_000_000;

fn normalize_line(reg: &Registry, map: &Mapping, parser: &str, event: &[u8], raw_id: u64) -> (Value, ulpf_normalize::NormalizeStats) {
    let idx = reg.index_of(parser).unwrap();
    let p = reg.get(idx);
    let mut scratch = Scratch::default();
    let mut parsed = Parsed::default();
    let ctx = Context { receipt_epoch_nanos: RECEIPT, default_offset_secs: 0 };
    let status = match p.parse(event, &ctx, &mut scratch, &mut parsed) {
        Ok(()) => "parsed",
        Err(e) => e.reason(),
    };
    let def = p.definition();
    let prov = Provenance {
        raw_id,
        source: "sample.log",
        parser: Some(p.name()),
        vendor: Some(&def.parser.vendor),
        product: Some(&def.parser.product),
        receipt_nanos: RECEIPT,
        parse_status: status,
        sub_status: "matched",
    };
    let mut out = Vec::new();
    let stats = map.normalize(&parsed, &prov, &mut out);
    assert_eq!(out.last(), Some(&b'\n'));
    let line = std::str::from_utf8(&out).unwrap();
    assert_eq!(line.matches('\n').count(), 1, "exactly one JSON line");
    (serde_json::from_str(line.trim_end()).unwrap(), stats)
}

fn get<'a>(v: &'a Value, path: &str) -> &'a Value {
    let mut cur = v;
    for p in path.split('.') {
        cur = cur.get(p).unwrap_or_else(|| panic!("missing {path} in {v}"));
    }
    cur
}

#[test]
fn ocsf_mapping_loads_and_has_no_vendor_vocabulary() {
    let m = mapping();
    assert_eq!(m.schema_name(), "ocsf");
    let text = std::fs::read_to_string(repo().join("mappings/ocsf.toml")).unwrap().to_ascii_lowercase();
    for banned in ["fortinet", "cisco", "palo", "checkpoint", "check point", "juniper", "pfsense", "sonicwall", "sophos", "suricata", "squid", "openvpn", "vendor =", "parser ="] {
        assert!(!text.contains(banned), "mapping must not reference `{banned}`");
    }
}

#[test]
fn fortinet_traffic_becomes_network_activity() {
    let reg = registry();
    let map = mapping();
    let evs = events("fortinet_fortigate.log");
    let (v, stats) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[0], 7);
    assert_eq!(get(&v, "class_uid"), 4001);
    assert_eq!(get(&v, "class_name"), "Network Activity");
    assert_eq!(get(&v, "type_uid"), 400106);
    assert_eq!(get(&v, "src_endpoint.ip"), "10.0.0.5");
    assert_eq!(get(&v, "src_endpoint.port"), 51234);
    assert_eq!(get(&v, "dst_endpoint.ip"), "142.250.72.14");
    assert_eq!(get(&v, "dst_endpoint.port"), 443);
    assert_eq!(get(&v, "action"), "Allowed");
    assert_eq!(get(&v, "action_id"), 1);
    assert_eq!(get(&v, "severity"), "Informational");
    assert_eq!(get(&v, "severity_id"), 1);
    assert_eq!(get(&v, "connection_info.protocol_name"), "tcp");
    assert_eq!(get(&v, "connection_info.protocol_num"), 6);
    assert_eq!(get(&v, "traffic.bytes_out"), 1234);
    assert_eq!(get(&v, "traffic.bytes_in"), 5678);
    assert_eq!(get(&v, "firewall_rule.name"), "LAN-to-WAN");
    assert_eq!(get(&v, "firewall_rule.uid"), 1);
    assert_eq!(get(&v, "device.hostname"), "FGT60E-LAB");
    assert_eq!(get(&v, "time"), 1_788_516_923_123i64);
    assert_eq!(get(&v, "metadata.original_time"), "1788516923123456789");
    assert_eq!(get(&v, "metadata.product.vendor_name"), "Fortinet");
    assert_eq!(get(&v, "metadata.version"), "1.3.0");
    assert_eq!(get(&v, "ulpf.raw_id"), 7);
    assert_eq!(get(&v, "ulpf.parser"), "fortinet_fortigate");
    assert_eq!(get(&v, "ulpf.time_policies").as_array().unwrap().len(), 0);
    assert_eq!(get(&v, "unmapped.vd"), "root");
    assert_eq!(get(&v, "unmapped.syslog_pri"), "189");
    assert!(v.get("unmapped").unwrap().get("srcip").is_none(), "mapped fields do not also appear in unmapped");
    assert_eq!(stats.class_uid, 4001);
    assert!(stats.mapped > 15 && stats.unmapped > 5, "{stats:?}");
    assert!(!stats.time_from_receipt);

    let (v, _) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[1], 8);
    assert_eq!(get(&v, "action"), "Denied");
    assert_eq!(get(&v, "action_id"), 2);
    assert_eq!(get(&v, "severity"), "Informational");

    let (v, _) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[2], 9);
    assert_eq!(get(&v, "class_uid"), 2004);
    assert_eq!(get(&v, "finding_info.title"), "Apache.Struts.2.Remote.Code.Execution");
    assert_eq!(get(&v, "finding_info.uid"), 45832);
    assert_eq!(get(&v, "severity"), "High");
    assert_eq!(get(&v, "action"), "Denied");

    // relayed, no eventtime: time from date+time, tz assumed
    let (v, _) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[3], 10);
    assert_eq!(get(&v, "time"), 1_788_516_926_000i64);
    assert_eq!(get(&v, "ulpf.time_policies"), &serde_json::json!(["tz_assumed"]));
    assert_eq!(get(&v, "metadata.original_time"), "2026-09-04 10:15:26");
    assert_eq!(get(&v, "device.hostname"), "FGT60E-LAB", "devname outranks syslog_host");

    // non-UTF-8 user, truncated action value → Other
    let (v, stats) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[4], 11);
    assert_eq!(get(&v, "class_uid"), 3002);
    assert_eq!(get(&v, "user.name"), "admin\u{FFFD}");
    assert_eq!(get(&v, "ulpf.utf8_lossy"), true);
    assert!(stats.utf8_lossy);
    let (v, stats) = normalize_line(&reg, &map, "fortinet_fortigate", &evs[5], 12);
    assert_eq!(get(&v, "action"), "Other");
    assert_eq!(get(&v, "action_id"), 99);
    assert_eq!(get(&v, "unmapped.action"), "acc", "unrecognised enum value is kept verbatim");
    assert_eq!(stats.enum_other, 1);
}

#[test]
fn cisco_asa_lines_map_by_message_id() {
    let reg = registry();
    let map = mapping();
    let evs = events("cisco_asa.log");
    let (v, _) = normalize_line(&reg, &map, "cisco_asa", &evs[0], 1);
    assert_eq!(get(&v, "class_uid"), 4001);
    assert_eq!(get(&v, "connection_info.direction"), "Outbound");
    assert_eq!(get(&v, "connection_info.direction_id"), 2);
    assert_eq!(get(&v, "src_endpoint.ip"), "10.0.0.5");
    assert_eq!(get(&v, "dst_endpoint.port"), 443);
    assert_eq!(get(&v, "connection_info.uid"), 12345);
    assert_eq!(get(&v, "severity"), "Informational");
    assert_eq!(get(&v, "metadata.event_code"), "302013");
    assert_eq!(get(&v, "action"), "Allowed", "the vendor's verb `built` is attached by the parser sub and canonicalised here");
    assert_eq!(get(&v, "unmapped.syslog_facility"), "20", "facility is kept but never mistaken for a log level");
    assert_eq!(get(&v, "device.hostname"), "asa-edge-01");
    assert_eq!(get(&v, "time"), 1_788_516_923_000i64);
    assert_eq!(get(&v, "ulpf.time_policies"), &serde_json::json!(["tz_assumed"]));

    let (v, _) = normalize_line(&reg, &map, "cisco_asa", &evs[1], 2);
    assert_eq!(get(&v, "action"), "Denied");
    assert_eq!(get(&v, "firewall_rule.name"), "outside_in");
    assert_eq!(get(&v, "severity"), "Low");
    assert_eq!(get(&v, "ulpf.time_policies"), &serde_json::json!(["year_assumed", "tz_assumed"]));

    let (v, _) = normalize_line(&reg, &map, "cisco_asa", &evs[5], 6);
    assert_eq!(get(&v, "class_uid"), 3002);
    assert_eq!(get(&v, "user.name"), "jdoe");
    assert_eq!(get(&v, "status"), "Successful");

    let (v, _) = normalize_line(&reg, &map, "cisco_asa", &evs[9], 10);
    assert_eq!(get(&v, "class_uid"), 3002);
    assert_eq!(get(&v, "dst_endpoint.ip"), "10.99.0.5");
    assert_eq!(get(&v, "user.domain"), "RemoteAccess");

    // headerless line: no timestamp anywhere → receipt fallback
    let (v, stats) = normalize_line(&reg, &map, "cisco_asa", &evs[11], 12);
    assert!(stats.time_from_receipt);
    assert_eq!(get(&v, "time"), 1_788_523_200_000i64);
    assert_eq!(get(&v, "ulpf.time_policies"), &serde_json::json!(["receipt_fallback"]));
    assert!(v.get("metadata").unwrap().get("original_time").is_none());
}

#[test]
fn fragments_merge_and_bad_files_are_reported() {
    let dir = std::env::temp_dir().join(format!("ulpf-map-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.toml"), "[schema]\nname = \"x\"\n[fields]\n\"a.b\" = [\"one\"]\n[[enum]]\nfield = \"a.b\"\n[[enum.values]]\nvalue = \"One\"\nraw = [\"1\"]\n[[class]]\nuid = 1\nname = \"c\"\ncategory_uid = 1\ncategory_name = \"c\"\nwhen = [{ one = [\"*\"] }]\n").unwrap();
    std::fs::write(dir.join("b.toml"), "[schema]\nname = \"x\"\n[fields]\n\"a.b\" = [\"two\", \"one\"]\n[[enum]]\nfield = \"a.b\"\n[[enum.values]]\nvalue = \"One\"\nraw = [\"uno\"]\n[[class]]\nuid = 1\nname = \"c\"\ncategory_uid = 1\ncategory_name = \"c\"\nwhen = [{ two = [\"*\"] }]\n").unwrap();
    std::fs::write(dir.join("c_bad.toml"), "[schema]\nname = \"y\"\nvendor = \"Acme\"\n").unwrap();
    std::fs::write(dir.join("d_syntax.toml"), "[schema\nname = \"z\"\n").unwrap();
    let report = load_dir(&dir).unwrap();
    assert_eq!(report.mappings.len(), 1);
    let m = &report.mappings[0];
    assert_eq!(m.file().fields["a.b"], vec!["one", "two"]);
    assert_eq!(m.file().enums[0].values[0].raw, vec!["1", "uno"]);
    assert_eq!(m.file().class[0].when.len(), 2);
    let msgs: Vec<String> = report.errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(msgs[0].contains("vendor"), "vendor key must be rejected: {}", msgs[0]);
    assert!(msgs[1].contains("d_syntax.toml:1"), "{}", msgs[1]);
    std::fs::remove_dir_all(&dir).unwrap();
    let none = load_files(&[]);
    assert!(none.mappings.is_empty() && none.errors.is_empty());
}

#[test]
fn absent_values_are_neither_mapped_nor_unmapped_nor_class_evidence() {
    let map = mapping();
    let mut parsed = Parsed::default();
    parsed.push(&b"user"[..], &b"-"[..]);
    parsed.push(&b"src_ip"[..], &b"N/A"[..]);
    parsed.push(&b"dst_ip"[..], &b"10.0.0.7"[..]);
    parsed.push(&b"method"[..], &b"GET"[..]);
    parsed.push(&b"url"[..], &b""[..]);
    let prov = Provenance {
        raw_id: 1,
        source: "t.log",
        parser: None,
        vendor: None,
        product: None,
        receipt_nanos: RECEIPT,
        parse_status: "parsed",
        sub_status: "not_applicable",
    };
    let mut out = Vec::new();
    let stats = map.normalize(&parsed, &prov, &mut out);
    let v: Value = serde_json::from_slice(out.trim_ascii_end()).unwrap();
    assert!(v.get("user").is_none());
    assert!(v.pointer("/src_endpoint/ip").is_none());
    assert_eq!(v.pointer("/dst_endpoint/ip").unwrap(), "10.0.0.7");
    assert!(v.get("unmapped").is_none(), "{v}");
    assert_eq!(v["class_uid"], 0, "method + url would be HTTP Activity, but an empty url is not evidence");
    assert_eq!(stats.mapped, 2);
    assert_eq!(stats.unmapped, 0);
}

#[test]
fn class_rule_wildcard_means_present() {
    let map = mapping();
    let mut parsed = Parsed::default();
    parsed.push(&b"source-address"[..], &b"10.0.0.5"[..]);
    parsed.push(&b"destination-address"[..], &b"8.8.8.8"[..]);
    let prov = Provenance {
        raw_id: 2,
        source: "t.log",
        parser: None,
        vendor: None,
        product: None,
        receipt_nanos: RECEIPT,
        parse_status: "parsed",
        sub_status: "not_applicable",
    };
    let mut out = Vec::new();
    map.normalize(&parsed, &prov, &mut out);
    let v: Value = serde_json::from_slice(out.trim_ascii_end()).unwrap();
    assert_eq!(v["class_uid"], 4001, "{v}");
}
