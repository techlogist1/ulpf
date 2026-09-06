//! The zero-copy invariant, measured: after warm-up, `Registry::detect` and
//! `Parser::parse` allocate nothing for every family whose values are borrowed spans.
//! The documented exceptions (JSON values, a quoted value with escapes, a sub on a
//! materialised value, an xml value with an entity reference) are exercised separately
//! and bounded.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use common::*;
use ulpf_parse::{Parsed, Registry};

/// The counter is process-wide, so the two tests must not run at the same time.
static SERIAL: Mutex<()> = Mutex::new(());

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const ZERO_ALLOCATION_FAMILIES: [&str; 13] = [
    "cisco_asa", "cisco_ios", "fortinet_fortigate", "juniper_srx", "openvpn", "palo_alto_panos",
    "pfsense_filterlog", "sonicwall", "sophos_xg", "squid_access", "cef", "leef", "windows_event",
];

#[test]
fn detect_and_parse_allocate_nothing_after_warm_up() {
    let _serial = SERIAL.lock().unwrap();
    let report = ulpf_parse::load_dir(&repo().join("parsers")).unwrap();
    assert!(report.errors.is_empty());
    let reg = Registry::new(report.parsers);
    let mut scratch = reg.scratch();
    let ctx = ctx();
    for family in ZERO_ALLOCATION_FAMILIES {
        let evs = events(&repo().join("samples").join(format!("{family}.log")));
        let mut out = Parsed::default();
        let idx = reg.index_of(family).unwrap();
        let p = reg.get(idx);
        for _ in 0..3 {
            for ev in &evs {
                let _ = reg.detect(ev, None);
                let _ = p.parse(ev, &ctx, &mut scratch, &mut out);
            }
        }
        let mut detected = 0;
        let (mut family_allocations, mut family_owned) = (0, 0);
        for (i, ev) in evs.iter().enumerate() {
            if reg.detect(ev, None) == Some(idx) {
                detected += 1;
            }
            // The counter is process-wide and the test runner has threads of its own, so
            // one measurement can catch a stray allocation; the parse is deterministic,
            // so a real per-event allocation shows in every attempt: take the minimum.
            let allocations = (0..3)
                .map(|_| {
                    let before = ALLOCS.load(Relaxed);
                    let _ = reg.detect(ev, None);
                    let _ = p.parse(ev, &ctx, &mut scratch, &mut out);
                    ALLOCS.load(Relaxed) - before
                })
                .min()
                .unwrap_or(0);
            // The one documented allocation on a span-valued family: a quoted value with
            // an escape is unescaped into an owned buffer, one per such value.
            let owned = out.fields.iter().filter(|f| matches!(f.value, Cow::Owned(_))).count();
            assert!(
                allocations <= owned,
                "{family} event {}: {allocations} allocations, {owned} materialised values",
                i + 1
            );
            family_allocations += allocations;
            family_owned += owned;
        }
        assert!(detected > 0, "{family}: nothing detected");
        // `--nocapture` shows the number the invariant is about, per family, without instrumenting.
        println!(
            "{family}: {} events, {family_allocations} allocations, {family_owned} materialised values",
            evs.len()
        );
    }
}

#[test]
fn cef_and_leef_allocate_nothing_after_warm_up() {
    let _serial = SERIAL.lock().unwrap();
    let cef = parser(r#"
[parser]
name = "cef"
vendor = "v"
product = "p"
[match]
contains = ["CEF:"]
[strategy]
kind = "cef"
"#);
    let leef = parser(r#"
[parser]
name = "leef"
vendor = "v"
product = "p"
[match]
contains = ["LEEF:"]
[strategy]
kind = "leef"
"#);
    let cef_line = b"CEF:0|Vendor|Product|1.0|100|Name of the event|5|src=10.0.0.5 spt=51234 dst=10.0.0.7 dpt=443 act=allow msg=plain words here cs1=a cs1Label=b";
    let leef_line = b"LEEF:2.0|Vendor|Product|1.0|100|^|src=10.0.0.5^dst=10.0.0.7^act=allow^usrName=jdoe";
    // The hex delimiter spelling reads the same span, so it must not allocate either.
    let leef_hex = b"LEEF:2.0|Vendor|Product|1.0|100|0x5E|src=10.0.0.5^dst=10.0.0.7^act=allow^usrName=jdoe";
    let mut scratch = ulpf_parse::Scratch::default();
    let mut out = Parsed::default();
    for _ in 0..3 {
        cef.parse(cef_line, &ctx(), &mut scratch, &mut out).unwrap();
        leef.parse(leef_line, &ctx(), &mut scratch, &mut out).unwrap();
        leef.parse(leef_hex, &ctx(), &mut scratch, &mut out).unwrap();
    }
    // minimum over three attempts: the runner's own threads may allocate once, the parser never
    let allocations = (0..3)
        .map(|_| {
            let before = ALLOCS.load(Relaxed);
            for _ in 0..100 {
                cef.parse(cef_line, &ctx(), &mut scratch, &mut out).unwrap();
                leef.parse(leef_line, &ctx(), &mut scratch, &mut out).unwrap();
                leef.parse(leef_hex, &ctx(), &mut scratch, &mut out).unwrap();
            }
            ALLOCS.load(Relaxed) - before
        })
        .min()
        .unwrap_or(0);
    assert_eq!(allocations, 0);
    assert_field(&out, "usrName", b"jdoe");
}

/// xml: keys are dotted paths that exist nowhere in the event, so they are owned, but
/// pooled in `Parsed` and recycled by `clear`; after warm-up a line with no entity
/// reference allocates nothing and a line with one entity-bearing value allocates once.
#[test]
fn xml_allocates_only_for_entity_bearing_values_after_warm_up() {
    let _serial = SERIAL.lock().unwrap();
    let p = parser(r#"
[parser]
name = "xml"
vendor = "v"
product = "p"
[match]
contains = ["<Event"]
[strategy]
kind = "xml"
[[timestamp]]
field = "System.TimeCreated.SystemTime"
format = "rfc3339"
[[sub]]
field = "System.EventID"
when = { "System.EventID" = "4624" }
kind = "pattern"
pattern = "{_:int}"
anchor = "full"
constants = { event_name = "An account was successfully logged on" }
"#);
    let plain: &[u8] = b"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Security-Auditing' Guid='{54849625-5478-4994-A5BA-3E3B0328C30D}'/><EventID>4624</EventID><Version>2</Version><Level>0</Level><Task>12544</Task><Opcode>0</Opcode><Keywords>0x8020000000000000</Keywords><TimeCreated SystemTime='2015-11-12T00:24:35.079785200Z'/><EventRecordID>211</EventRecordID><Correlation/><Execution ProcessID='716' ThreadID='760'/><Channel>Security</Channel><Computer>WIN-GG82ULGC9GO</Computer><Security/></System><EventData><Data Name='SubjectUserSid'>S-1-5-18</Data><Data Name='SubjectUserName'>WIN-GG82ULGC9GO$</Data><Data Name='SubjectDomainName'>WORKGROUP</Data><Data Name='SubjectLogonId'>0x3e7</Data><Data Name='TargetUserSid'>S-1-5-21-1377283216-344919071-3415362939-500</Data><Data Name='TargetUserName'>Administrator</Data><Data Name='TargetDomainName'>WIN-GG82ULGC9GO</Data><Data Name='TargetLogonId'>0x8dcdc</Data><Data Name='LogonType'>2</Data><Data Name='LogonProcessName'>User32</Data><Data Name='AuthenticationPackageName'>Negotiate</Data><Data Name='WorkstationName'>WIN-GG82ULGC9GO</Data><Data Name='ProcessId'>0x44c</Data><Data Name='ProcessName'>C:\\Windows\\System32\\svchost.exe</Data><Data Name='IpAddress'>127.0.0.1</Data><Data Name='IpPort'>0</Data></EventData></Event>";
    let with_entity: Vec<u8> = String::from_utf8_lossy(plain).replace("Administrator", "R&amp;D").into_bytes();
    // A provider without a manifest template renders EventData as an unnamed <Data> list;
    // the sibling counter (Data, Data2, ... Data20) is formatted on the stack.
    let unnamed: Vec<u8> = format!(
        "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><EventID>1000</EventID><TimeCreated SystemTime='2015-11-12T00:24:35.079785200Z'/></System><EventData>{}</EventData></Event>",
        (1..=20).map(|i| format!("<Data>v{i}</Data>")).collect::<String>()
    ).into_bytes();
    let mut scratch = ulpf_parse::Scratch::default();
    let mut out = Parsed::default();
    for _ in 0..3 {
        p.parse(plain, &ctx(), &mut scratch, &mut out).unwrap();
        p.parse(&with_entity, &ctx(), &mut scratch, &mut out).unwrap();
        p.parse(&unnamed, &ctx(), &mut scratch, &mut out).unwrap();
    }
    fn count<'a>(p: &'a ulpf_parse::Parser, line: &'a [u8], scratch: &mut ulpf_parse::Scratch, out: &mut Parsed<'a>) -> usize {
        (0..3)
            .map(|_| {
                let before = ALLOCS.load(Relaxed);
                for _ in 0..100 {
                    p.parse(line, &ctx(), scratch, out).unwrap();
                }
                ALLOCS.load(Relaxed) - before
            })
            .min()
            .unwrap_or(0)
    }
    let plain_allocs = count(&p, plain, &mut scratch, &mut out);
    assert_eq!(plain_allocs, 0, "a line with no entity reference");
    assert_field(&out, "EventData.TargetUserName", b"Administrator");
    assert_field(&out, "event_name", b"An account was successfully logged on");
    assert!(out.timestamp.is_some());
    let entity_allocs = count(&p, &with_entity, &mut scratch, &mut out);
    assert_eq!(entity_allocs, 100, "one allocation per entity-bearing value, per parse");
    assert_field(&out, "EventData.TargetUserName", b"R&D");
    assert_eq!(out.fields.iter().filter(|f| matches!(f.value, Cow::Owned(_))).count(), 1);
    let unnamed_allocs = count(&p, &unnamed, &mut scratch, &mut out);
    assert_eq!(unnamed_allocs, 0, "twenty unnamed siblings, no allocation for the counter");
    assert_field(&out, "EventData.Data", b"v1");
    assert_field(&out, "EventData.Data20", b"v20");
}
