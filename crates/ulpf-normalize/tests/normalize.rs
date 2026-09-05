use std::path::PathBuf;

use serde_json::Value;
use ulpf_normalize::{Mapping, Provenance, load_dir, load_files};
use ulpf_parse::{Context, Parsed, Registry, Scratch};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mapping() -> Mapping {
    mapping_named("ocsf")
}

/// `mappings/` holds one file per output schema; every test names the one it asserts
/// against, so adding a schema never silently re-points an existing test.
fn mapping_named(name: &str) -> Mapping {
    let mut report = load_dir(&repo().join("mappings")).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>());
    let idx = report
        .mappings
        .iter()
        .position(|m| m.schema_name() == name)
        .unwrap_or_else(|| panic!("no mapping named `{name}` in {:?}", report.mappings.iter().map(|m| m.schema_name()).collect::<Vec<_>>()));
    report.mappings.remove(idx)
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

/// The parser/mapping wall, from the mapping side: a mapping speaks source-field
/// vocabulary only, so no vendor or parser identity may appear in the file at all.
fn assert_no_vendor_vocabulary(file: &str) {
    let text = std::fs::read_to_string(repo().join("mappings").join(file)).unwrap().to_ascii_lowercase();
    for banned in ["fortinet", "cisco", "palo", "checkpoint", "check point", "juniper", "pfsense", "sonicwall", "sophos", "suricata", "squid", "openvpn", "vendor =", "parser ="] {
        assert!(!text.contains(banned), "{file} must not reference `{banned}`");
    }
}

#[test]
fn ocsf_mapping_loads_and_has_no_vendor_vocabulary() {
    let m = mapping();
    assert_eq!(m.schema_name(), "ocsf");
    assert_no_vendor_vocabulary("ocsf.toml");
}

#[test]
fn ecs_mapping_loads_and_has_no_vendor_vocabulary() {
    let m = mapping_named("ecs");
    assert_eq!(m.schema_name(), "ecs");
    assert_eq!(m.file().schema.version.as_deref(), Some("9.5.0"), "the ECS release the field names were taken from");
    assert_no_vendor_vocabulary("ecs.toml");
    // The pivot kinds are fixed; the paths are the schema's business. Load already
    // rejects a path nothing can set, so this only pins them to the ECS field sets:
    // a perimeter device observes traffic that is not its own, so `observer`, not `host`.
    let e = m.entities();
    assert_eq!(
        (e.src_ip.as_deref(), e.dst_ip.as_deref(), e.user.as_deref(), e.dst_port.as_deref(), e.device.as_deref()),
        (Some("source.ip"), Some("destination.ip"), Some("user.name"), Some("destination.port"), Some("observer.hostname"))
    );
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

#[test]
fn a_repeated_source_field_keeps_every_value() {
    let map = mapping();
    let mut parsed = Parsed::default();
    parsed.push(&b"src"[..], &b"10.0.0.1"[..]);
    parsed.push(&b"src"[..], &b"10.0.0.2"[..]);
    parsed.push(&b"srcip"[..], &b"10.0.0.3"[..]);
    let prov = Provenance {
        raw_id: 3,
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
    assert_eq!(v.pointer("/src_endpoint/ip").unwrap(), "10.0.0.3", "the higher-ranked alias wins");
    let mut kept = vec![v["unmapped"]["src"].as_str().unwrap(), v["unmapped"]["src#2"].as_str().unwrap()];
    kept.sort();
    assert_eq!(kept, ["10.0.0.1", "10.0.0.2"], "both values of the repeated field survive");
    assert_eq!(stats.unmapped, 2);
    assert_eq!(stats.mapped, 1);
}

#[test]
fn absurd_class_uid_is_rejected_at_load() {
    let dir = std::env::temp_dir().join(format!("ulpf-map-uid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ocsf.toml");
    std::fs::write(&path, "[schema]\nname = \"ocsf\"\n[[class]]\nuid = 100000000000000000\nname = \"Huge\"\ncategory_uid = 1\ncategory_name = \"x\"\n").unwrap();
    let report = load_files(&[path]);
    assert!(report.mappings.is_empty());
    let text = report.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("outside"), "{text}");
}

/// Done-item 8: the same twelve parsers, a second output schema, zero parser changes.
/// One row per device family: a field the parser emits in the vendor's own vocabulary,
/// and the ECS 9.5.0 path and value it must land on.
#[test]
fn ecs_maps_every_family_onto_ecs_paths() {
    use serde_json::json;
    let reg = registry();
    let map = mapping_named("ecs");
    // (parser, event index in its sample, expected ECS path → value)
    let cases = vec![
        // A firewall verdict is event.type in ECS (allowed/denied), never event.outcome.
        (
            "cisco_asa",
            1,
            vec![
                ("event.code", json!("106023")),
                ("event.action", json!("deny")),
                ("event.category", json!(["network"])),
                ("event.type", json!(["connection", "denied"])),
                ("source.ip", json!("203.0.113.9")),
                ("source.port", json!(44321)),
                ("destination.ip", json!("10.0.0.7")),
                ("destination.port", json!(22)),
                ("network.transport", json!("tcp")),
                ("rule.name", json!("outside_in")),
                ("observer.hostname", json!("asa-edge-01")),
                ("observer.ingress.interface.name", json!("outside")),
                ("log.level", json!("low")),
                ("event.severity", json!(2)),
            ],
        ),
        (
            "cisco_ios",
            0,
            vec![
                ("event.code", json!("IPACCESSLOGP")),
                ("event.action", json!("denied")),
                ("event.type", json!(["connection", "denied"])),
                ("rule.name", json!("outside-in")),
                ("network.packets", json!(1)),
            ],
        ),
        // The one ECS field ECS itself calls "not standardized" keeps the vendor's word.
        (
            "fortinet_fortigate",
            0,
            vec![
                ("event.action", json!("accept")),
                ("event.type", json!(["connection", "allowed"])),
                ("source.ip", json!("10.0.0.5")),
                ("source.bytes", json!(1234)),
                ("destination.bytes", json!(5678)),
                ("network.application", json!("HTTPS.BROWSER")),
                ("rule.name", json!("LAN-to-WAN")),
                ("rule.id", json!("1")),
                ("observer.serial_number", json!("FGT60ETK19001234")),
                ("observer.egress.interface.name", json!("wan1")),
            ],
        ),
        (
            "palo_alto_panos",
            0,
            vec![
                ("event.action", json!("allow")),
                ("event.reason", json!("aged-out")),
                ("network.application", json!("dns")),
                ("network.bytes", json!(190)),
                ("source.nat.ip", json!("203.0.113.1")),
                ("destination.nat.port", json!(53)),
                ("observer.serial_number", json!("007951000012345")),
                ("observer.ingress.zone", json!("trust")),
                ("rule.uuid", json!("e8f3a2b1-1234-5678-9abc-def012345678")),
            ],
        ),
        (
            "check_point",
            0,
            vec![
                ("event.action", json!("Accept")),
                ("event.type", json!(["connection", "allowed"])),
                ("source.ip", json!("10.0.1.10")),
                ("network.transport", json!("udp")),
                ("network.direction", json!("inbound")),
                ("rule.name", json!("Allow DNS")),
                ("observer.hostname", json!("gw-01")),
            ],
        ),
        (
            "juniper_srx",
            0,
            vec![
                ("event.code", json!("RT_FLOW_SESSION_CREATE")),
                ("source.nat.ip", json!("203.0.113.1")),
                ("source.nat.port", json!(12345)),
                ("observer.ingress.zone", json!("trust")),
                ("observer.egress.zone", json!("untrust")),
                ("rule.name", json!("allow-dns")),
            ],
        ),
        (
            "pfsense_filterlog",
            0,
            vec![
                ("event.action", json!("block")),
                ("event.type", json!(["connection", "denied"])),
                ("network.type", json!("ipv4")),
                ("network.direction", json!("inbound")),
                ("rule.id", json!("1000000103")),
                ("observer.ingress.interface.name", json!("igb0")),
            ],
        ),
        (
            "sonicwall",
            0,
            vec![
                ("event.code", json!("98")),
                ("source.mac", json!("00:11:22:33:44:55")),
                ("destination.mac", json!("00:17:c5:aa:bb:cc")),
                ("observer.serial_number", json!("18B1690729A8")),
                ("observer.egress.zone", json!("WAN")),
            ],
        ),
        (
            "sophos_xg",
            0,
            vec![
                ("event.code", json!("010101600001")),
                ("event.action", json!("Allow")),
                ("rule.name", json!("LAN to WAN")),
                ("observer.serial_number", json!("A11111AAA1F9R30")),
                ("destination.geo.country_iso_code", json!("USA")),
            ],
        ),
        (
            "squid_access",
            0,
            vec![
                ("event.category", json!(["network", "web"])),
                ("event.type", json!(["connection", "access"])),
                ("event.action", json!("TCP_MISS")),
                ("url.original", json!("http://example.com/")),
                ("http.request.method", json!("GET")),
                ("http.response.status_code", json!(200)),
                ("http.response.mime_type", json!("text/html")),
                ("network.bytes", json!(1234)),
            ],
        ),
        (
            "suricata_eve",
            0,
            vec![
                ("event.kind", json!("alert")),
                ("event.category", json!(["intrusion_detection"])),
                ("rule.id", json!("2012234")),
                ("rule.name", json!("ET WEB_SERVER Possible SQL Injection Attempt")),
                ("rule.category", json!("Web Application Attack")),
                ("network.community_id", json!("1:LQU9qZlK+B5F3KDmev6m5PMibrg=")),
                ("network.protocol", json!("tls")),
            ],
        ),
        (
            "openvpn",
            4,
            vec![
                ("event.category", json!(["authentication"])),
                ("event.type", json!(["info"])),
                ("user.name", json!("jdoe")),
                ("source.ip", json!("203.0.113.9")),
                ("network.type", json!("ipv4")),
            ],
        ),
        // event.outcome is the success or failure of the reporting entity, so it lands on
        // an authentication result and not on a filtering verdict.
        (
            "cisco_ios",
            11,
            vec![("event.category", json!(["authentication"])), ("event.outcome", json!("success")), ("user.name", json!("jdoe"))],
        ),
    ];
    let mut families = std::collections::BTreeSet::new();
    for (parser, idx, expect) in cases {
        families.insert(parser);
        let evs = events(&format!("{parser}.log"));
        let (v, _) = normalize_line(&reg, &map, parser, &evs[idx], idx as u64);
        for (path, want) in expect {
            assert_eq!(get(&v, path), &want, "{parser}[{idx}] {path}");
        }
    }
    assert_eq!(families.len(), 12, "every shipped parser family is covered: {families:?}");
}

/// The wall from the output side: two schemas over one parsed event share every source
/// field and no output path. Neither file could name the other's vocabulary if it tried.
#[test]
fn the_same_parsed_event_lands_on_two_disjoint_schemas() {
    let reg = registry();
    let evs = events("fortinet_fortigate.log");
    let (ecs, ecs_stats) = normalize_line(&reg, &mapping_named("ecs"), "fortinet_fortigate", &evs[0], 1);
    let (ocsf, ocsf_stats) = normalize_line(&reg, &mapping(), "fortinet_fortigate", &evs[0], 1);
    for ocsf_only in ["src_endpoint", "dst_endpoint", "connection_info", "firewall_rule", "finding_info", "traffic", "action", "action_id", "severity_id", "app_name", "type_uid"] {
        assert!(ecs.get(ocsf_only).is_none(), "ECS output must not carry `{ocsf_only}`: {ecs}");
    }
    for ecs_only in ["event", "rule", "log", "source", "destination", "network", "url", "http", "dns", "tls"] {
        assert!(ocsf.get(ecs_only).is_none(), "OCSF output must not carry `{ecs_only}`: {ocsf}");
    }
    assert_eq!(get(&ecs, "source.ip"), get(&ocsf, "src_endpoint.ip"), "one source field, two schema paths");
    assert_eq!(
        ecs_stats.mapped + ecs_stats.unmapped,
        ocsf_stats.mapped + ocsf_stats.unmapped,
        "every parsed field is accounted for under either schema"
    );
}
