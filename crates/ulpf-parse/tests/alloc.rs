//! The zero-copy invariant, measured: after warm-up, `Registry::detect` and
//! `Parser::parse` allocate nothing for every family whose values are borrowed spans.
//! The documented exceptions (JSON values, a quoted value with escapes, a sub on a
//! materialised value) are exercised separately and bounded.

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

const ZERO_ALLOCATION_FAMILIES: [&str; 12] = [
    "cisco_asa", "cisco_ios", "fortinet_fortigate", "juniper_srx", "openvpn", "palo_alto_panos",
    "pfsense_filterlog", "sonicwall", "sophos_xg", "squid_access", "cef", "leef",
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
        }
        assert!(detected > 0, "{family}: nothing detected");
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
    let mut scratch = ulpf_parse::Scratch::default();
    let mut out = Parsed::default();
    for _ in 0..3 {
        cef.parse(cef_line, &ctx(), &mut scratch, &mut out).unwrap();
        leef.parse(leef_line, &ctx(), &mut scratch, &mut out).unwrap();
    }
    // minimum over three attempts: the runner's own threads may allocate once, the parser never
    let allocations = (0..3)
        .map(|_| {
            let before = ALLOCS.load(Relaxed);
            for _ in 0..100 {
                cef.parse(cef_line, &ctx(), &mut scratch, &mut out).unwrap();
                leef.parse(leef_line, &ctx(), &mut scratch, &mut out).unwrap();
            }
            ALLOCS.load(Relaxed) - before
        })
        .min()
        .unwrap_or(0);
    assert_eq!(allocations, 0);
    assert_field(&out, "usrName", b"jdoe");
}
