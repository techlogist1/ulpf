//! Load generator for `scripts/soak.sh`: mutated copies of the sample families, paced
//! onto an appended file and/or at syslog listeners, counted so the soak can reconcile.
//!
//! Every event carries a unique value derived from a global sequence number (the first
//! IPv4 in the line, else its first free digit run), so no two generated events are ever
//! byte-identical. `--selftest N` proves that and reports how many the real parsers claim.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;

#[derive(Parser)]
#[command(about = "Paced, deterministic log generator for the soak harness")]
struct Args {
    /// Directory of sample logs to mutate.
    #[arg(long, default_value = "samples")]
    samples: PathBuf,
    /// Extra logs whose format no parser claims, mixed in at --unknown-share.
    #[arg(long)]
    unknown: Vec<PathBuf>,
    #[arg(long, default_value_t = 0.005)]
    unknown_share: f64,
    /// `RATE:SECONDS` phase for the appended file, repeatable, applied in order.
    #[arg(long = "phase")]
    phases: Vec<String>,
    /// File to append to (created if missing).
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    udp: Option<String>,
    #[arg(long, default_value_t = 0)]
    udp_rate: u64,
    #[arg(long)]
    tcp: Option<String>,
    #[arg(long, default_value_t = 0)]
    tcp_rate: u64,
    /// Frame TCP events RFC 6587 style (`<len> SP <event>`) instead of by line.
    #[arg(long)]
    tcp_octet_counting: bool,
    /// Seconds the socket senders run; defaults to the sum of the file phases.
    #[arg(long, default_value_t = 0)]
    socket_secs: u64,
    /// Stop every stream once this many events have been generated in total.
    #[arg(long, default_value_t = 0)]
    events_target: u64,
    /// Counts are rewritten here every second and once at exit.
    #[arg(long, default_value = "soak-counts.json")]
    counts: PathBuf,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Generate N events and check them (uniqueness, detection) instead of sending any.
    #[arg(long)]
    selftest: Option<usize>,
    #[arg(long, default_value = "parsers")]
    parsers: PathBuf,
}

// ---------------------------------------------------------------- generator

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64: one multiply-xor chain, no state beyond the seed.
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    CarrierIp,
    CarrierDigits,
    Ip,
    Port,
    Time,
    Frac,
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: Kind,
}

struct Tmpl {
    bytes: Vec<u8>,
    spans: Vec<Span>,
}

/// First octets that read like real perimeter traffic; the carrier takes the low 24 bits
/// of the sequence number, so an event is unique up to 2^27 events.
const BASES: [u8; 8] = [10, 172, 192, 203, 198, 100, 209, 45];
const PORT_KEYS: [&str; 8] = ["port=", "sport=", "dport=", "spt=", "dpt=", "srcport=", "dstport=", "port "];

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

fn ip_spans(b: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !is_digit(b[i]) || (i > 0 && (is_digit(b[i - 1]) || b[i - 1] == b'.')) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut groups = 0;
        loop {
            let start = j;
            while j < b.len() && is_digit(b[j]) && j - start < 3 {
                j += 1;
            }
            if j == start {
                break;
            }
            groups += 1;
            if groups == 4 {
                break;
            }
            if j < b.len() && b[j] == b'.' {
                j += 1;
            } else {
                break;
            }
        }
        let ends_clean = j >= b.len() || (!is_digit(b[j]) && b[j] != b'.');
        if groups == 4 && ends_clean {
            out.push(Span { start: i, end: j, kind: Kind::Ip });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn digit_run(b: &[u8], from: usize) -> usize {
    let mut j = from;
    while j < b.len() && is_digit(b[j]) {
        j += 1;
    }
    j
}

fn port_spans(b: &[u8], ips: &[Span]) -> Vec<Span> {
    let mut out = Vec::new();
    for ip in ips {
        if ip.end < b.len() && (b[ip.end] == b':' || b[ip.end] == b'/') {
            let e = digit_run(b, ip.end + 1);
            if e > ip.end + 1 && e - ip.end - 1 <= 5 {
                out.push(Span { start: ip.end + 1, end: e, kind: Kind::Port });
            }
        }
    }
    for key in PORT_KEYS {
        let k = key.as_bytes();
        let mut i = 0;
        while i + k.len() <= b.len() {
            if b[i..].starts_with(k) {
                let e = digit_run(b, i + k.len());
                if e > i + k.len() && e - i - k.len() <= 5 {
                    out.push(Span { start: i + k.len(), end: e, kind: Kind::Port });
                }
                i += k.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// `dd:dd:dd` with an optional fractional run after it.
fn time_spans(b: &[u8]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= b.len() {
        let w = &b[i..i + 8];
        let shaped = is_digit(w[0]) && is_digit(w[1]) && w[2] == b':' && is_digit(w[3]) && is_digit(w[4]) && w[5] == b':' && is_digit(w[6]) && is_digit(w[7]);
        let clean_before = i == 0 || (!is_digit(b[i - 1]) && b[i - 1] != b':');
        let clean_after = i + 8 >= b.len() || (!is_digit(b[i + 8]) && b[i + 8] != b':');
        if shaped && clean_before && clean_after {
            out.push(Span { start: i, end: i + 8, kind: Kind::Time });
            if i + 9 < b.len() && b[i + 8] == b'.' {
                let e = digit_run(b, i + 9);
                if e > i + 9 {
                    out.push(Span { start: i + 9, end: e, kind: Kind::Frac });
                }
            }
            i += 8;
        } else {
            i += 1;
        }
    }
    out
}

/// A digit run that can carry the sequence number in a line with no IPv4: at least three
/// digits, not a syslog priority, not a year, not already spoken for.
fn carrier_digits(b: &[u8], taken: &[Span]) -> Option<Span> {
    let mut i = 0;
    while i < b.len() {
        if !is_digit(b[i]) || (i > 0 && is_digit(b[i - 1])) {
            i += 1;
            continue;
        }
        let e = digit_run(b, i);
        let len = e - i;
        let year = len == 4 && (b[i..i + 2] == *b"19" || b[i..i + 2] == *b"20");
        let pri = i > 0 && b[i - 1] == b'<';
        let overlaps = taken.iter().any(|s| i < s.end && s.start < e);
        if len >= 3 && !year && !pri && !overlaps {
            return Some(Span { start: i, end: e, kind: Kind::CarrierDigits });
        }
        i = e.max(i + 1);
    }
    None
}

fn template(bytes: Vec<u8>) -> Option<Tmpl> {
    let ips = ip_spans(&bytes);
    let mut spans = ips.clone();
    spans.extend(port_spans(&bytes, &ips));
    spans.extend(time_spans(&bytes));
    spans.sort_by_key(|s| (s.start, s.end));
    let mut kept: Vec<Span> = Vec::with_capacity(spans.len());
    for s in spans {
        if kept.last().is_none_or(|k| k.end <= s.start) {
            kept.push(s);
        }
    }
    match kept.iter().position(|s| s.kind == Kind::Ip) {
        Some(i) => kept[i].kind = Kind::CarrierIp,
        None => {
            let c = carrier_digits(&bytes, &kept)?;
            kept.push(c);
            kept.sort_by_key(|s| s.start);
        }
    }
    Some(Tmpl { bytes, spans: kept })
}

fn load_templates(dir: &PathBuf) -> std::io::Result<(Vec<Tmpl>, usize)> {
    let mut out = Vec::new();
    let mut dropped = 0;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "log")).collect();
    paths.sort();
    for p in paths {
        let bytes = std::fs::read(&p)?;
        for r in ulpf_store::Framer::new(&bytes, true) {
            match template(bytes[r].to_vec()) {
                Some(t) => out.push(t),
                None => dropped += 1,
            }
        }
    }
    Ok((out, dropped))
}

fn push_num(out: &mut Vec<u8>, mut v: u64, width: usize) {
    let mut b = [0u8; 20];
    let mut i = b.len();
    if v == 0 {
        i -= 1;
        b[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    while b.len() - i < width {
        i -= 1;
        b[i] = b'0';
    }
    out.extend_from_slice(&b[i..]);
}

fn push_ip(out: &mut Vec<u8>, o: [u8; 4]) {
    for (i, x) in o.iter().enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        push_num(out, *x as u64, 1);
    }
}

struct Gen {
    known: Arc<Vec<Tmpl>>,
    unknown: Arc<Vec<Tmpl>>,
    unknown_share: f64,
    seq: Arc<AtomicU64>,
    rng: Rng,
}

impl Gen {
    /// One event into `out` (cleared first). Wall-clock time of day keeps the timestamps
    /// moving with the run; the sequence number keeps the line unique.
    fn event(&mut self, out: &mut Vec<u8>, tod: u64) {
        out.clear();
        let seq = self.seq.fetch_add(1, Relaxed);
        let use_unknown = !self.unknown.is_empty() && (self.rng.next() % 10_000) < (self.unknown_share * 10_000.0) as u64;
        let set = if use_unknown { Arc::clone(&self.unknown) } else { Arc::clone(&self.known) };
        let t = &set[self.rng.below(set.len())];
        let mut at = 0usize;
        for s in &t.spans {
            out.extend_from_slice(&t.bytes[at..s.start]);
            match s.kind {
                Kind::CarrierIp => push_ip(out, [BASES[((seq >> 24) & 7) as usize], (seq >> 16) as u8, (seq >> 8) as u8, seq as u8]),
                Kind::CarrierDigits => push_num(out, seq, (s.end - s.start).max(9)),
                Kind::Ip => {
                    let r = self.rng.next();
                    push_ip(out, [BASES[(r & 7) as usize], (r >> 8) as u8, (r >> 16) as u8, (r >> 24) as u8]);
                }
                Kind::Port => push_num(out, 1024 + self.rng.next() % 64_000, 1),
                Kind::Time => {
                    let s = tod % 86_400;
                    push_num(out, s / 3600, 2);
                    out.push(b':');
                    push_num(out, (s / 60) % 60, 2);
                    out.push(b':');
                    push_num(out, s % 60, 2);
                }
                Kind::Frac => push_num(out, self.rng.next() % 10u64.pow((s.end - s.start).min(9) as u32), s.end - s.start),
            }
            at = s.end;
        }
        out.extend_from_slice(&t.bytes[at..]);
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------------------------------------------------------------- counters

#[derive(Default)]
struct Counts {
    file_events: AtomicU64,
    file_bytes: AtomicU64,
    udp_events: AtomicU64,
    udp_bytes: AtomicU64,
    udp_errors: AtomicU64,
    tcp_events: AtomicU64,
    tcp_bytes: AtomicU64,
    tcp_connects: AtomicU64,
    tcp_errors: AtomicU64,
    behind_chunks: AtomicU64,
    phase: AtomicU64,
}

impl Counts {
    fn json(&self, elapsed: f64, done: bool) -> String {
        let g = |a: &AtomicU64| a.load(Relaxed);
        format!(
            "{{\"elapsed_secs\":{elapsed:.3},\"done\":{done},\"phase\":{},\"file_events\":{},\"file_bytes\":{},\"udp_events\":{},\"udp_bytes\":{},\"udp_errors\":{},\"tcp_events\":{},\"tcp_bytes\":{},\"tcp_connects\":{},\"tcp_errors\":{},\"behind_chunks\":{},\"file_rate\":{:.0},\"total_events\":{}}}",
            g(&self.phase),
            g(&self.file_events),
            g(&self.file_bytes),
            g(&self.udp_events),
            g(&self.udp_bytes),
            g(&self.udp_errors),
            g(&self.tcp_events),
            g(&self.tcp_bytes),
            g(&self.tcp_connects),
            g(&self.tcp_errors),
            g(&self.behind_chunks),
            if elapsed > 0.0 { g(&self.file_events) as f64 / elapsed } else { 0.0 },
            g(&self.file_events) + g(&self.udp_events) + g(&self.tcp_events),
        )
    }
}

fn write_counts(path: &PathBuf, body: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Paces `rate` events per second in chunks of ~20 ms, calling `send` per event.
/// Returns when the phase is over or `stop` is set.
fn paced(rate: u64, secs: u64, stop: &AtomicBool, behind: &AtomicU64, mut send: impl FnMut(&mut Vec<u8>, u64)) {
    if rate == 0 || secs == 0 {
        return;
    }
    let chunk = (rate / 50).max(1);
    let total = rate.saturating_mul(secs);
    let start = Instant::now();
    let mut done = 0u64;
    let mut buf = Vec::with_capacity(1024);
    while done < total && !stop.load(Relaxed) {
        let tod = now_secs();
        for _ in 0..chunk.min(total - done) {
            send(&mut buf, tod);
        }
        done += chunk;
        let due = Duration::from_secs_f64(done as f64 / rate as f64);
        match due.checked_sub(start.elapsed()) {
            Some(d) => std::thread::sleep(d),
            None => {
                behind.fetch_add(1, Relaxed);
            }
        }
    }
}

/// The last load thread out sets `stop`, which ends the counts thread and so the process.
fn last_out(live: &AtomicU64, stop: &AtomicBool) {
    if live.fetch_sub(1, Relaxed) == 1 {
        stop.store(true, Relaxed);
    }
}

fn parse_phase(s: &str) -> Option<(u64, u64)> {
    let (r, secs) = s.split_once(':')?;
    Some((r.parse().ok()?, secs.parse().ok()?))
}

// ---------------------------------------------------------------- main

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let (known, dropped) = load_templates(&args.samples)?;
    if known.is_empty() {
        eprintln!("soak_gen: no templates under {}", args.samples.display());
        std::process::exit(2);
    }
    let mut unknown = Vec::new();
    for p in &args.unknown {
        let bytes = std::fs::read(p)?;
        for r in ulpf_store::Framer::new(&bytes, true) {
            if let Some(t) = template(bytes[r].to_vec()) {
                unknown.push(t);
            }
        }
    }
    eprintln!("soak_gen: {} templates ({dropped} dropped, no unique carrier), {} unknown-format templates", known.len(), unknown.len());
    let known = Arc::new(known);
    let unknown = Arc::new(unknown);
    let seq = Arc::new(AtomicU64::new(0));
    let gen_at = |n: u64| Gen {
        known: Arc::clone(&known),
        unknown: Arc::clone(&unknown),
        unknown_share: args.unknown_share,
        seq: Arc::clone(&seq),
        rng: Rng(args.seed.wrapping_mul(0x2545_f491_4f6c_dd1d).wrapping_add(n)),
    };

    if let Some(n) = args.selftest {
        return selftest(gen_at(0), n, &args);
    }

    let phases: Vec<(u64, u64)> = args.phases.iter().filter_map(|p| parse_phase(p)).collect();
    if phases.len() != args.phases.len() {
        eprintln!("soak_gen: a --phase must be RATE:SECONDS");
        std::process::exit(2);
    }
    let socket_secs = if args.socket_secs > 0 { args.socket_secs } else { phases.iter().map(|(_, s)| s).sum() };
    let counts = Arc::new(Counts::default());
    let stop = Arc::new(AtomicBool::new(false));
    // Senders outlive the phases they were given; the counts thread runs until the last one
    // is done, so a socket-only generator terminates as surely as a file one. All of them are
    // registered before any is spawned, or a short first phase could stop the others.
    let want_file = args.file.is_some();
    let want_udp = args.udp.is_some() && args.udp_rate > 0;
    let want_tcp = args.tcp.is_some() && args.tcp_rate > 0;
    let live = Arc::new(AtomicU64::new(want_file as u64 + want_udp as u64 + want_tcp as u64));
    if live.load(Relaxed) == 0 {
        stop.store(true, Relaxed);
    }
    let started = Instant::now();

    std::thread::scope(|scope| -> std::io::Result<()> {
        // counts file + the events-target stop, once a second
        {
            let (counts, stop, seq, path, target) = (Arc::clone(&counts), Arc::clone(&stop), Arc::clone(&seq), args.counts.clone(), args.events_target);
            scope.spawn(move || {
                while !stop.load(Relaxed) {
                    if target > 0 && seq.load(Relaxed) >= target {
                        stop.store(true, Relaxed);
                    }
                    write_counts(&path, &counts.json(started.elapsed().as_secs_f64(), false));
                    std::thread::sleep(Duration::from_millis(1000));
                }
            });
        }
        if let Some(path) = &args.file {
            let mut f = OpenOptions::new().create(true).append(true).open(path)?;
            let (counts, stop, live) = (Arc::clone(&counts), Arc::clone(&stop), Arc::clone(&live));
            let mut g = gen_at(1);
            scope.spawn(move || {
                let mut chunk = Vec::with_capacity(1 << 20);
                for (i, (rate, secs)) in phases.iter().enumerate() {
                    counts.phase.store(i as u64 + 1, Relaxed);
                    let mut n = 0u64;
                    paced(*rate, *secs, &stop, &counts.behind_chunks, |buf, tod| {
                        g.event(buf, tod);
                        chunk.extend_from_slice(buf);
                        n += 1;
                        // one write per ~20 ms of events, no fsync
                        if n.is_multiple_of((rate / 50).max(1)) {
                            if f.write_all(&chunk).is_ok() {
                                counts.file_events.fetch_add(n, Relaxed);
                                counts.file_bytes.fetch_add(chunk.len() as u64, Relaxed);
                            }
                            n = 0;
                            chunk.clear();
                        }
                    });
                    if !chunk.is_empty() && f.write_all(&chunk).is_ok() {
                        counts.file_events.fetch_add(n, Relaxed);
                        counts.file_bytes.fetch_add(chunk.len() as u64, Relaxed);
                        chunk.clear();
                    }
                }
                last_out(&live, &stop);
            });
        }
        if let (Some(addr), true) = (&args.udp, args.udp_rate > 0) {
            let sock = UdpSocket::bind("127.0.0.1:0")?;
            let (counts, stop, addr, live) = (Arc::clone(&counts), Arc::clone(&stop), addr.clone(), Arc::clone(&live));
            let mut g = gen_at(2);
            scope.spawn(move || {
                paced(args.udp_rate, socket_secs, &stop, &counts.behind_chunks, |buf, tod| {
                    g.event(buf, tod);
                    let body = single_line(buf);
                    match sock.send_to(body, addr.as_str()) {
                        Ok(n) => {
                            counts.udp_events.fetch_add(1, Relaxed);
                            counts.udp_bytes.fetch_add(n as u64, Relaxed);
                        }
                        Err(_) => {
                            counts.udp_errors.fetch_add(1, Relaxed);
                        }
                    }
                });
                last_out(&live, &stop);
            });
        }
        if let (Some(addr), true) = (&args.tcp, args.tcp_rate > 0) {
            let (counts, stop, addr, octet, live) =
                (Arc::clone(&counts), Arc::clone(&stop), addr.clone(), args.tcp_octet_counting, Arc::clone(&live));
            let mut g = gen_at(3);
            scope.spawn(move || {
                let mut conn: Option<TcpStream> = None;
                let mut frame = Vec::with_capacity(1024);
                paced(args.tcp_rate, socket_secs, &stop, &counts.behind_chunks, |buf, tod| {
                    if conn.is_none() {
                        match TcpStream::connect(addr.as_str()) {
                            Ok(c) => {
                                let _ = c.set_nodelay(true);
                                counts.tcp_connects.fetch_add(1, Relaxed);
                                conn = Some(c);
                            }
                            Err(_) => {
                                counts.tcp_errors.fetch_add(1, Relaxed);
                                std::thread::sleep(Duration::from_millis(200));
                                return;
                            }
                        }
                    }
                    g.event(buf, tod);
                    frame.clear();
                    let body = single_line(buf);
                    if octet {
                        push_num(&mut frame, body.len() as u64, 1);
                        frame.push(b' ');
                        frame.extend_from_slice(body);
                    } else {
                        frame.extend_from_slice(body);
                        frame.push(b'\n');
                    }
                    let c = conn.as_mut().expect("connected above");
                    match c.write_all(&frame) {
                        Ok(()) => {
                            counts.tcp_events.fetch_add(1, Relaxed);
                            counts.tcp_bytes.fetch_add(frame.len() as u64, Relaxed);
                        }
                        Err(_) => {
                            counts.tcp_errors.fetch_add(1, Relaxed);
                            conn = None;
                        }
                    }
                });
                last_out(&live, &stop);
            });
        }
        Ok(())
    })?;

    stop.store(true, Relaxed);
    let body = counts.json(started.elapsed().as_secs_f64(), true);
    write_counts(&args.counts, &body);
    println!("{body}");
    Ok(())
}

/// Syslog is one message per datagram or per line, so an event that came from a multi-line
/// sample is flattened before it goes on a socket: a line-framed listener would otherwise
/// receive more events than the sender counted, and the soak could never reconcile.
fn single_line(b: &mut Vec<u8>) -> &[u8] {
    while b.last().is_some_and(|c| *c == b'\n' || *c == b'\r') {
        b.pop();
    }
    for x in b.iter_mut() {
        if *x == b'\n' || *x == b'\r' {
            *x = b' ';
        }
    }
    &b[..]
}

/// Generates `n` events and reports what they are worth as load: all distinct, and how
/// many the real parser registry claims. A soak over lines nothing detects is a soak of
/// the inference path, not of the pipeline.
fn selftest(mut g: Gen, n: usize, args: &Args) -> std::io::Result<()> {
    let registry = ulpf_parse::load_dir(&args.parsers).ok().map(|r| ulpf_parse::Registry::new(r.parsers));
    let mut seen: HashSet<Vec<u8>> = HashSet::with_capacity(n);
    let mut buf = Vec::with_capacity(1024);
    let mut detected = 0usize;
    let mut dupes = 0usize;
    let mut hint = None;
    let tod = now_secs();
    let start = Instant::now();
    for _ in 0..n {
        g.event(&mut buf, tod);
        if !seen.insert(buf.clone()) {
            dupes += 1;
        }
        if let Some(reg) = &registry {
            let hit = reg.detect(&buf, hint);
            hint = hit.or(hint);
            if hit.is_some() {
                detected += 1;
            }
        }
    }
    let rate = n as f64 / start.elapsed().as_secs_f64();
    println!(
        "{{\"events\":{n},\"distinct\":{},\"duplicates\":{dupes},\"detected\":{detected},\"detected_pct\":{:.2},\"gen_events_per_sec\":{rate:.0}}}",
        seen.len(),
        detected as f64 * 100.0 / n as f64
    );
    if dupes > 0 {
        eprintln!("soak_gen: FAIL {dupes} duplicate events");
        std::process::exit(1);
    }
    Ok(())
}
