//! Done-item 3: a definition emitted from a `Template` loads through the same path as a
//! hand-written file and parses identically.
mod common;
use common::*;
use ulpf_parse::def::{Anchor, Envelope, Matcher, Meta, ParserDefinition, Strategy, StrategyKind, TimestampSpec};
use ulpf_parse::{Parsed, Scratch, SlotKind, Template, Token, load_dir, load_str};

fn slot(name: &str, kind: SlotKind) -> Token {
    Token::Slot { name: name.into(), kind }
}
fn c(s: &str) -> Token {
    Token::Const(s.into())
}

#[test]
fn pattern_syntax_is_a_bijection_for_every_repo_pattern() {
    let report = load_dir(&repo().join("parsers")).unwrap();
    let mut seen = 0;
    for p in &report.parsers {
        let mut pats = Vec::new();
        let collect = |s: &Strategy, pats: &mut Vec<String>| {
            pats.extend(s.pattern.iter().cloned());
            pats.extend(s.patterns.iter().cloned());
        };
        collect(&p.definition().strategy, &mut pats);
        for s in &p.definition().sub {
            collect(s, &mut pats);
        }
        for pat in pats {
            let t = Template::from_pattern(&pat).unwrap();
            assert_eq!(t.to_pattern(), pat);
            assert_eq!(Template::from_pattern(&t.to_pattern()).unwrap(), t);
            seen += 1;
        }
    }
    assert!(seen >= 10, "expected the ASA definition's patterns, saw {seen}");
    let t = Template::from_pattern("a {{b}} {x} {y:int}").unwrap();
    assert_eq!(t.tokens, vec![c("a {b} "), slot("x", SlotKind::Text), c(" "), slot("y", SlotKind::Int)]);
    assert_eq!(t.to_pattern(), "a {{b}} {x:text} {y:int}");
    assert!(Template::from_pattern("{x:nope}").is_err());
    assert!(Template::from_pattern("{x").is_err());
    assert!(Template::from_pattern("a } b").is_err());
    assert!(Template::from_pattern("{bad name}").is_err());
}

#[test]
fn every_repo_definition_survives_serialize_deserialize() {
    let report = load_dir(&repo().join("parsers")).unwrap();
    for p in &report.parsers {
        let text = toml::to_string(p.definition()).unwrap();
        let back: ParserDefinition = toml::from_str(&text).unwrap();
        assert_eq!(&back, p.definition(), "{}", p.name());
    }
}

#[test]
fn all_strategies_are_machine_emittable() {
    let meta = |n: &str| Meta { name: n.into(), vendor: "v".into(), product: "p".into(), description: Some("d".into()), version: 1 };
    let matcher = Matcher { contains: vec!["a".into()], starts_with: Some("<".into()), regex: Some("x".into()), priority: 5 };
    let strategies = vec![
        Strategy { kind: StrategyKind::Kv, key_value_separator: Some(":".into()), pair_separator: Some("; ".into()), ..Default::default() },
        Strategy { kind: StrategyKind::Delimiter, delimiter: Some(",".into()), quote: Some("\"".into()), fields: vec!["a".into(), "_".into()], ..Default::default() },
        Strategy { kind: StrategyKind::Json, ..Default::default() },
        Strategy { kind: StrategyKind::Cef, ..Default::default() },
        Strategy { kind: StrategyKind::Leef, ..Default::default() },
        Strategy { kind: StrategyKind::Pattern, patterns: vec!["a {b:int}".into()], regex: Some("(?P<z>.)".into()), anchor: Some(Anchor::Full), ..Default::default() },
    ];
    for (i, s) in strategies.into_iter().enumerate() {
        let def = ParserDefinition {
            parser: meta(&format!("s{i}")),
            matcher: matcher.clone(),
            envelope: Envelope { syslog: i % 2 == 0 },
            strategy: s,
            timestamp: vec![TimestampSpec { field: Some("t".into()), fields: vec![], format: "auto".into() }],
            sub: vec![Strategy {
                kind: StrategyKind::Kv,
                field: Some("msg".into()),
                when: [("k".to_string(), ulpf_parse::OneOrMany::Many(vec!["1".into(), "2".into()]))].into_iter().collect(),
                constants: [("c".to_string(), "v".to_string())].into_iter().collect(),
                ..Default::default()
            }],
        };
        let text = toml::to_string(&def).unwrap();
        let back: ParserDefinition = toml::from_str(&text).unwrap();
        assert_eq!(back, def, "strategy {i}:\n{text}");
        load_str(std::path::Path::new("gen"), &text).unwrap_or_else(|e| panic!("strategy {i} does not compile: {e}\n{text}"));
    }
}

#[test]
fn generated_definition_parses_identically_to_hand_written() {
    let hand = r#"
[parser]
name = "asa_deny_hand"
vendor = "Cisco"
product = "ASA"

[match]
contains = ["%ASA-4-106023"]

[envelope]
syslog = true

[strategy]
kind = "pattern"
pattern = '%ASA-{severity:int}-{msg_id:int}: Deny {protocol:word} src {src_interface:word}:{src_ip:ip}/{src_port:int} dst {dst_interface:word}:{dst_ip:ip}/{dst_port:int} by access-group "{acl:text}" [{hash_1:hex}, {hash_2:hex}]'
"#;
    let template = Template {
        tokens: vec![
            c("%ASA-"), slot("severity", SlotKind::Int), c("-"), slot("msg_id", SlotKind::Int), c(": Deny "),
            slot("protocol", SlotKind::Word), c(" src "), slot("src_interface", SlotKind::Word), c(":"),
            slot("src_ip", SlotKind::Ip), c("/"), slot("src_port", SlotKind::Int), c(" dst "),
            slot("dst_interface", SlotKind::Word), c(":"), slot("dst_ip", SlotKind::Ip), c("/"),
            slot("dst_port", SlotKind::Int), c(" by access-group \""), slot("acl", SlotKind::Text), c("\" ["),
            slot("hash_1", SlotKind::Hex), c(", "), slot("hash_2", SlotKind::Hex), c("]"),
        ],
    };
    let generated_def = template.to_definition("asa_deny_generated", "Cisco", "ASA", vec!["%ASA-4-106023".into()]);
    let generated_text = toml::to_string(&generated_def).unwrap();
    let reloaded: ParserDefinition = toml::from_str(&generated_text).unwrap();
    assert_eq!(reloaded, generated_def, "generated file must reload to the same value");

    let hand_p = parser(hand);
    let gen_p = load_str(std::path::Path::new("generated.toml"), &generated_text).unwrap();
    assert_eq!(hand_p.definition().strategy, gen_p.definition().strategy, "same pattern text");

    let evs = events(&repo().join("samples/cisco_asa.log"));
    let mut scratch = Scratch::default();
    let mut a = Parsed::default();
    let mut b = Parsed::default();
    let mut compared = 0;
    for e in &evs {
        let ra = hand_p.parse(e, &ctx(), &mut scratch, &mut a);
        let rb = gen_p.parse(e, &ctx(), &mut scratch, &mut b);
        assert_eq!(ra, rb);
        assert_eq!(pairs(&a), pairs(&b));
        assert_eq!(a.timestamp, b.timestamp);
        assert_eq!(a.timestamp_text, b.timestamp_text);
        if ra.is_ok() {
            compared += 1;
            assert_field(&a, "acl", b"outside_in");
        }
    }
    assert_eq!(compared, 1, "exactly the tcp deny line matches");
}

#[test]
fn optional_groups_round_trip_and_match_with_or_without_the_segment() {
    let pat = "fw {action:word} src {src_ip:ip}{? len={len:int}} proto {proto:word}";
    let t = Template::from_pattern(pat).unwrap();
    assert_eq!(t.to_pattern(), pat);
    assert_eq!(Template::from_pattern(&t.to_pattern()).unwrap(), t);
    assert_eq!(t.tokens.len(), 7, "{:?}", t.tokens);
    assert!(matches!(&t.tokens[4], Token::Optional(inner) if inner.len() == 2));
    assert_eq!(t.slots().map(|(n, _)| n).collect::<Vec<_>>(), vec!["action", "src_ip", "len", "proto"]);
    for bad in ["{? a}{? b}", "{?}", "{? a {? b}}", "{? a"] {
        let r = Template::from_pattern(bad);
        if bad == "{? a}{? b}" {
            assert!(r.is_ok(), "two sibling groups are fine");
        } else {
            assert!(r.is_err(), "{bad}");
        }
    }

    let def = format!(
        "[parser]\nname = \"opt\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"fw \"]\n[strategy]\nkind = \"pattern\"\npattern = '{pat}'\n"
    );
    let p = parser(&def);
    let mut scratch = Scratch::default();
    let mut out = Parsed::default();
    p.parse(b"fw drop src 10.0.0.1 len=60 proto tcp", &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "len", b"60");
    assert_field(&out, "proto", b"tcp");
    p.parse(b"fw drop src 10.0.0.1 proto tcp", &ctx(), &mut scratch, &mut out).unwrap();
    assert!(out.get(b"len").is_none(), "absent segment emits no field");
    assert_field(&out, "proto", b"tcp");
    assert!(p.parse(b"fw drop src 10.0.0.1 len=x proto tcp", &ctx(), &mut scratch, &mut out).is_err());

    let emitted = toml::to_string(&t.to_definition("opt_gen", "v", "p", vec!["fw ".into()])).unwrap();
    load_str(std::path::Path::new("gen"), &emitted).unwrap();
}

#[test]
fn timestamp_slot_reads_the_common_log_format_the_time_module_accepts() {
    let def = "[parser]\nname = \"clf\"\nvendor = \"v\"\nproduct = \"p\"\n[match]\ncontains = [\"HTTP\"]\n[strategy]\nkind = \"pattern\"\npattern = '{client:ip} - {user:word} [{ts:timestamp}] \"{request:text}\" {status:int} {bytes:int}'\n[[timestamp]]\nfield = \"ts\"\nformat = \"auto\"\n";
    let p = parser(def);
    let mut scratch = Scratch::default();
    let mut out = Parsed::default();
    p.parse(b"203.0.113.9 - - [04/Sep/2026:10:15:23 +0000] \"GET /index.html HTTP/1.1\" 200 5124", &ctx(), &mut scratch, &mut out).unwrap();
    assert_field(&out, "ts", b"04/Sep/2026:10:15:23 +0000");
    assert_field(&out, "status", b"200");
    assert!(out.timestamp.is_some(), "the slot text must be something ulpf_time::parse reads: {:?}", out.timestamp_error);
}
