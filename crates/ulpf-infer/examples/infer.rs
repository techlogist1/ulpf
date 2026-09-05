//! Offline inference over a file: prints the proposal's TOML and an evidence summary.
//! `cargo run -p ulpf-infer --example infer -- heldout/mikrotik.log [--decisions]`
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: infer <file> [--decisions] [--unmatched]");
    let bytes = std::fs::read(path).expect("read file");
    let lines: Vec<&[u8]> = bytes.split_inclusive(|b| *b == b'\n').collect();
    let p = ulpf_infer::infer(path, &lines, &ulpf_infer::Params::default());
    println!("{}", toml::to_string(&p.definition).expect("toml"));
    let e = &p.evidence;
    println!("# lines {} used {} templates {} unmatched {} {:?}", e.lines_seen, e.lines_used, e.templates.len(), e.unmatched.count, e.unmatched.by_reason);
    for t in &e.templates {
        println!("# T{:<3} support {:<4} verified {:<4} {}", t.id, t.support, t.verified, t.pattern);
        if args.iter().any(|a| a == "--slots") {
            for s in &t.slots {
                println!("#      {}:{} <- `{}` ({} distinct) {:?} [{}] {}", s.name, s.kind, s.preceded_by, s.distinct, s.examples, if s.suggested { "suggested" } else { "generic" }, s.reason);
            }
        }
    }
    if args.iter().any(|a| a == "--decisions") {
        for d in &e.decisions {
            println!("# D {d}");
        }
    }
    if args.iter().any(|a| a == "--unmatched") {
        for u in &e.unmatched.examples {
            println!("# U {u}");
        }
    }
}
