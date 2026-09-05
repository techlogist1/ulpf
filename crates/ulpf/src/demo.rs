//! `ulpf demo`: the demo of PROGRESS.md played by the binary itself, so it runs where no
//! shell does (D67). Orchestration only — existing subcommands, the watch directory and the
//! localhost API; nothing here touches the engine, the store or the hot path. `--check`
//! proves the titles and commands it prints are still the ones PROGRESS.md carries.

use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

pub struct Args {
    pub auto: bool,
    pub check: bool,
    pub reset: bool,
    pub dir: PathBuf,
    pub listen: SocketAddr,
    pub syslog: SocketAddr,
    pub repo: PathBuf,
}

/// The step headings of the demo section of PROGRESS.md, verbatim: the presenter's text and
/// the machine's are the same bytes, and `--check` fails if they stop being.
const TITLES: [&str; 14] = [
    "0. reset between rehearsals (the server uses demo/parsers and demo/pending, so nothing lands in the repo)",
    "1. server + UI (terminal 1): watches demo/watch, listens for syslog on UDP and TCP 5514",
    "2. known formats and a live device: counters, sources and the tail move within 500 ms (one file a second, so the feed visibly moves)",
    "3. an unknown format from a file and from the socket: clustered at 64 lines, \"Review (2)\" appears",
    "4. approve (UI: `a` opens the confirmation, Enter approves, Esc backs out; or:)",
    "5. the same events take the fast path; the pivot sees them",
    "6. traceback with provenance: click a tail row, or open http://127.0.0.1:7878/#/trace/0",
    "7. replay: a parser bug, the fix, every past event corrected, the store untouched",
    "8. drift: a device changes its format mid-stream; the update proposal carries a diff",
    "9. integrity: verify from the UI (Integrity -> Verify) or offline, and hand a stranger the attestation",
    "10. a second output schema with zero parser changes",
    "11. throughput (terminal 2, quiet machine; the bench file is gitignored, generate once, ~25 s, 1.5 GB)",
    "12. kill recovery: kill -9 a run, restart it, same output id for id",
    "13. isolation and container",
];

/// Checked but not printed: step 8 builds the drift file itself, so this is the line of the
/// PROGRESS python that says which file it is built from.
const ANCHORS: [&str; 1] = ["lines=open('heldout/mikrotik.log','rb').read().splitlines()"];

#[cfg(windows)]
const BOLD: (&str, &str) = ("", "");
#[cfg(not(windows))]
const BOLD: (&str, &str) = ("\x1b[1m", "\x1b[0m");

/// The shell equivalent of each step, printed so the presenter sees what happened, and
/// grepped in PROGRESS.md by `--check` (with the default directory, ports and repo).
struct Cmds {
    reset: String,
    mk: String,
    serve: String,
    samples: String,
    udp: String,
    unknown: String,
    approve: String,
    again: String,
    trace: String,
    bug: String,
    underbug: String,
    fix: String,
    replay: String,
    verify: String,
    attest: String,
    verify_att: String,
    tamper: String,
}

impl Cmds {
    fn new(a: &Args) -> Cmds {
        let d = a.dir.display().to_string();
        let api = format!("http://{}", a.listen);
        Cmds {
            reset: format!("rm -rf {d}"),
            mk: format!("mkdir -p {d}/watch {d}/parsers {d}/pending && cp parsers/*.toml {d}/parsers/"),
            serve: format!(
                "./target/release/ulpf serve {d}/watch --store {d}/store --output {d}/out.jsonl --pending {d}/pending --parsers {d}/parsers --syslog-udp {} --syslog-tcp {} --infer-threshold 64",
                a.syslog, a.syslog
            ),
            samples: format!("for f in samples/*.log; do cp \"$f\" {d}/watch/; sleep 1; done"),
            udp: format!(
                "python3 -c \"import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);[s.sendto(l,('{}',{})) for l in open('heldout/edgerouter.log','rb').read().splitlines()]\"",
                a.syslog.ip(),
                a.syslog.port()
            ),
            unknown: format!("cp heldout/mikrotik.log {d}/watch/"),
            approve: format!("curl -s -X POST {api}/api/pending/mikrotik/approve"),
            again: format!("cp heldout/mikrotik.log {d}/watch/mikrotik-again.log"),
            trace: format!("curl -s {api}/api/events/0 | python3 -m json.tool | head -40"),
            bug: format!("sed -i '' 's/{{dst_ip:ip}}/{{dst_addr:ip}}/g' {d}/parsers/cisco_asa.toml"),
            underbug: format!("cp samples/cisco_asa.log {d}/watch/asa-under-the-bug.log"),
            fix: format!("cp parsers/cisco_asa.toml {d}/parsers/"),
            replay: format!("curl -s -X POST {api}/api/replay"),
            verify: format!("./target/release/ulpf verify --store {d}/store"),
            attest: format!("./target/release/ulpf attest --store {d}/store --out {d}/attestation.json"),
            verify_att: format!("./target/release/ulpf verify --store {d}/store --attestation {d}/attestation.json"),
            tamper: format!("printf 'X' | dd of={d}/store/raw.seg bs=1 seek=100 conv=notrunc 2>/dev/null"),
        }
    }

    fn all(&self) -> Vec<&str> {
        [
            &self.reset,
            &self.mk,
            &self.serve,
            &self.samples,
            &self.udp,
            &self.unknown,
            &self.approve,
            &self.again,
            &self.trace,
            &self.bug,
            &self.underbug,
            &self.fix,
            &self.replay,
            &self.verify,
            &self.attest,
            &self.verify_att,
            &self.tamper,
        ]
        .into_iter()
        .map(String::as_str)
        .collect()
    }
}

fn say(title: &str) {
    println!("\n{}{title}{}", BOLD.0, BOLD.1);
}
fn cmd(s: &str) {
    println!("   $ {s}");
}
fn hint(s: &str) {
    println!("   -> {s}");
}
fn note(s: &str) {
    println!("   {s}");
}

fn next(a: &Args) {
    if a.auto {
        std::thread::sleep(Duration::from_secs(3));
    } else {
        enter();
    }
}

fn enter() {
    print!("\n   [Enter] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

// --- the localhost API: one request per call, no HTTP client crate (the shape of tests/server.rs) ---

fn http(addr: &SocketAddr, method: &str, path: &str, body: &str) -> Result<(u16, String)> {
    let mut s = TcpStream::connect(addr).with_context(|| format!("connecting to {addr}"))?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(body) } else { body.to_string() };
    Ok((status, body))
}

/// A request the demo makes on stage: a failure is printed where the answer would be, never
/// an abort — the presenter is mid-sentence and the store is about to be reset.
fn call(addr: &SocketAddr, method: &str, path: &str, body: &str) -> String {
    match http(addr, method, path, body) {
        Ok((_, answer)) => answer,
        Err(e) => format!("(request failed: {e:#})"),
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        let mut end = n.min(after.len());
        while end > 0 && !after.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&after[..end]);
        rest = after.get(n + 2..).unwrap_or("");
    }
    out
}

fn wait_http(addr: &SocketAddr, secs: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if matches!(http(addr, "GET", "/api/status", ""), Ok((200, _))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// Poll `/api/pending` for `pattern` and say how long it took, or that it is not there yet.
fn wait_for(addr: &SocketAddr, label: &str, pattern: &str) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if pending_has(addr, pattern) {
            note(&format!("({label} after {:.1} s)", start.elapsed().as_secs_f64()));
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    note(&format!("({label} not seen after 30 s: open Review and check by hand)"));
    false
}

fn pending_has(addr: &SocketAddr, pattern: &str) -> bool {
    matches!(http(addr, "GET", "/api/pending", ""), Ok((200, body)) if body.contains(pattern))
}

// --- the child processes: this binary's own subcommands ---

fn child_status(bin: &Path, args: &[&str]) -> Result<i32> {
    let status = Command::new(bin).args(args).status().with_context(|| format!("running {} {}", bin.display(), args.join(" ")))?;
    Ok(status.code().unwrap_or(-1))
}

struct Serve {
    child: Child,
    pid_file: PathBuf,
}

impl Serve {
    /// Killed rather than asked to stop: a killed run restarts to the same output and store
    /// as an uninterrupted one (D59), and std has no cross-platform signal.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.pid_file);
    }
}

fn start_server(a: &Args, bin: &Path) -> Result<Serve> {
    let d = &a.dir;
    let child = Command::new(bin)
        .arg("serve")
        .arg(d.join("watch"))
        .arg("--store")
        .arg(d.join("store"))
        .arg("--output")
        .arg(d.join("out.jsonl"))
        .arg("--pending")
        .arg(d.join("pending"))
        .arg("--parsers")
        .arg(d.join("parsers"))
        .arg("--syslog-udp")
        .arg(a.syslog.to_string())
        .arg("--syslog-tcp")
        .arg(a.syslog.to_string())
        .arg("--infer-threshold")
        .arg("64")
        .arg("--listen")
        .arg(a.listen.to_string())
        .spawn()
        .with_context(|| format!("spawning {} serve", bin.display()))?;
    let pid_file = d.join("serve.pid");
    let _ = std::fs::write(&pid_file, child.id().to_string());
    Ok(Serve { child, pid_file })
}

/// A leftover server from an interrupted rehearsal: its pid is in `<dir>/serve.pid`, and is
/// killed only if that pid is still a `ulpf serve`.
fn kill_leftover(dir: &Path) {
    let path = dir.join("serve.pid");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let Ok(pid) = text.trim().parse::<u32>() else { return };
    if is_ulpf_serve(pid) {
        note(&format!("stopping the server left by an earlier run (pid {pid})"));
        kill_pid(pid);
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    // SAFETY: kill(2) on a pid this runner wrote and just identified as a `ulpf serve`.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status();
}

#[cfg(unix)]
fn is_ulpf_serve(pid: u32) -> bool {
    let Ok(out) = Command::new("ps").args(["-o", "args=", "-p", &pid.to_string()]).output() else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    text.contains("ulpf") && text.contains("serve")
}

#[cfg(windows)]
fn is_ulpf_serve(pid: u32) -> bool {
    let Ok(out) = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).to_ascii_lowercase().contains("ulpf")
}

// --- the steps ---

fn copy_into(src: &Path, dst_dir: &Path, name: &str) -> Result<()> {
    std::fs::copy(src, dst_dir.join(name)).with_context(|| format!("copying {} into {}", src.display(), dst_dir.display()))?;
    Ok(())
}

fn logs_in(dir: &Path, ext: &str) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    files.sort();
    Ok(files)
}

fn send_udp(file: &Path, to: &SocketAddr) -> Result<usize> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let socket = UdpSocket::bind(("127.0.0.1", 0)).context("binding a UDP source port")?;
    let mut sent = 0;
    for line in bytes.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        socket.send_to(line, to).with_context(|| format!("sending to {to}"))?;
        sent += 1;
    }
    Ok(sent)
}

/// 1,250 known lines to establish the baseline, three seconds of quiet, then 400 of a message
/// type no pattern covers: the file the PROGRESS python writes, written here directly.
fn write_drift(repo: &Path, path: &Path) -> Result<()> {
    let src = repo.join("heldout").join("mikrotik.log");
    let bytes = std::fs::read(&src).with_context(|| format!("reading {}", src.display()))?;
    let lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()).collect();
    let first = lines.first().context("heldout/mikrotik.log is empty")?;
    let hdr = String::from_utf8_lossy(first).split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).with_context(|| format!("writing {}", path.display()))?;
    for _ in 0..5 {
        for l in &lines {
            f.write_all(l)?;
            f.write_all(b"\n")?;
        }
    }
    f.flush()?;
    std::thread::sleep(Duration::from_secs(3));
    for i in 0..400 {
        writeln!(f, "{hdr} interface,info ether{} link up (speed {}G, full duplex)", 1 + i % 8, [1, 10, 25][i % 3])?;
    }
    f.flush()?;
    Ok(())
}

/// One byte of the raw segment overwritten so `verify` can name the record it broke.
/// Rehearsal only: the store is thrown away by the reset that follows.
fn tamper(seg: &Path) -> Result<()> {
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(seg).with_context(|| format!("opening {}", seg.display()))?;
    // byte 100 is inside record 0's body whatever the first sample is (the segment and record
    // headers end at 68); byte 200 landed in record 1's receipt time once cef.log sorted first,
    // and the digest and chain cover the record bytes, not the header
    f.seek(SeekFrom::Start(100))?;
    f.write_all(b"X")?;
    f.flush()?;
    Ok(())
}

/// A CLI approve writes the generated parser (`origin = "inferred"`, priority -1) into the
/// repo's `parsers/`, and a demo copy or a bundle made after it knows the unseen format already,
/// so the demo could not raise a proposal. The reset removes them before the copy is made.
fn purge_generated(repo: &Path) -> Result<usize> {
    let mut n = 0;
    for p in logs_in(&repo.join("parsers"), "toml")? {
        let text = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        let generated = text.lines().any(|l| l.trim_start().starts_with("origin") && l.contains("inferred"));
        if generated {
            std::fs::remove_file(&p).with_context(|| format!("removing {}", p.display()))?;
            n += 1;
        }
    }
    Ok(n)
}

fn play(a: &Args, bin: &Path, srv: &mut Option<Serve>) -> Result<()> {
    let c = Cmds::new(a);
    let d = &a.dir;
    let watch = d.join("watch");
    let repo = &a.repo;

    say(TITLES[0]);
    cmd(&c.reset);
    if d.exists() {
        std::fs::remove_dir_all(d).with_context(|| format!("removing {}", d.display()))?;
    }
    let purged = purge_generated(repo)?;
    if purged > 0 {
        hint(&format!("removed {purged} generated parser(s) from {}: nothing is approved from the CLI before a demo", repo.join("parsers").display()));
    }

    say(TITLES[1]);
    cmd(&c.mk);
    std::fs::create_dir_all(&watch)?;
    std::fs::create_dir_all(d.join("parsers"))?;
    std::fs::create_dir_all(d.join("pending"))?;
    for p in logs_in(&repo.join("parsers"), "toml")? {
        let name = p.file_name().context("parser file has no name")?.to_string_lossy().into_owned();
        copy_into(&p, &d.join("parsers"), &name)?;
    }
    cmd(&c.serve);
    *srv = Some(start_server(a, bin)?);
    if !wait_http(&a.listen, 20) {
        bail!("the server did not answer GET /api/status on {} within 20 s", a.listen);
    }
    hint(&format!("open http://{}  (0 Flow, 1 Live, 2 Review, 3 Traceback, 4 Pivot, 5 Replay, 6 Drift, 7 Integrity; ? = keys)", a.listen));
    next(a);

    say(TITLES[2]);
    hint("watch Flow or Live while the fifteen samples land one per second");
    cmd(&c.samples);
    for p in logs_in(&repo.join("samples"), "log")? {
        let name = p.file_name().context("sample has no name")?.to_string_lossy().into_owned();
        copy_into(&p, &watch, &name)?;
        std::thread::sleep(Duration::from_secs(1));
    }
    cmd(&c.udp);
    let sent = send_udp(&repo.join("heldout").join("edgerouter.log"), &a.syslog)?;
    note(&format!("({sent} datagrams sent to {})", a.syslog));
    hint("Live -> sources: udp/127.0.0.1 (250 events, no parser yet), 15 sample sources parsed");
    next(a);

    say(TITLES[3]);
    cmd(&c.unknown);
    copy_into(&repo.join("heldout").join("mikrotik.log"), &watch, "mikrotik.log")?;
    wait_for(&a.listen, "proposal mikrotik", "\"mikrotik\"");
    hint("Review -> mikrotik: every slot has a name and the REASON it was chosen; uncheck a template + Regenerate to drop it");
    next(a);

    say(TITLES[4]);
    let mut approve = true;
    if !a.auto {
        hint("click Approve in the UI, then Enter here (Enter without approving runs the request below)");
        enter();
        approve = pending_has(&a.listen, "\"mikrotik\"");
    }
    if approve {
        cmd(&c.approve);
        note(&call(&a.listen, "POST", "/api/pending/mikrotik/approve", "{}"));
    }
    hint(&format!("Live -> parsers: mikrotik_inferred, origin approved ({}/parsers/mikrotik_inferred.toml)", d.display()));
    next(a);

    say(TITLES[5]);
    cmd(&c.again);
    copy_into(&repo.join("heldout").join("mikrotik.log"), &watch, "mikrotik-again.log")?;
    hint("Live -> mikrotik-again.log detected 250. Pivot -> src_ip 203.0.113.9: one attacker across every device; click a related value to pivot again");
    next(a);

    say(TITLES[6]);
    hint(&format!("click any tail row, or open http://{}/#/trace/0 ; hover a normalized field and its bytes light up", a.listen));
    cmd(&c.trace);
    let body = call(&a.listen, "GET", "/api/events/0", "");
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) => {
            for line in serde_json::to_string_pretty(&v)?.lines().take(40) {
                println!("{line}");
            }
        }
        Err(_) => println!("{body}"),
    }
    next(a);

    say(TITLES[7]);
    cmd(&c.bug);
    let asa = d.join("parsers").join("cisco_asa.toml");
    let text = std::fs::read_to_string(&asa).with_context(|| format!("reading {}", asa.display()))?;
    std::fs::write(&asa, text.replace("{dst_ip:ip}", "{dst_addr:ip}")).with_context(|| format!("writing {}", asa.display()))?;
    std::thread::sleep(Duration::from_secs(1));
    cmd(&c.underbug);
    copy_into(&repo.join("samples").join("cisco_asa.log"), &watch, "asa-under-the-bug.log")?;
    std::thread::sleep(Duration::from_secs(2));
    cmd(&c.fix);
    copy_into(&repo.join("parsers").join("cisco_asa.toml"), &d.join("parsers"), "cisco_asa.toml")?;
    std::thread::sleep(Duration::from_secs(1));
    cmd(&c.replay);
    note(&call(&a.listen, "POST", "/api/replay", "{}"));
    hint(&format!("Replay -> v2: changed = the ASA events written under the bug; WHY names {}/parsers/cisco_asa.toml", d.display()));
    let store = d.join("store").display().to_string();
    cmd(&c.verify);
    note(&format!("(exit {})", child_status(bin, &["verify", "--store", &store])?));
    next(a);

    say(TITLES[8]);
    note(&format!("(writing {}/watch/gw-drift.log: 1,250 known lines, 3 s of quiet, then 400 of a new message type)", d.display()));
    write_drift(repo, &watch.join("gw-drift.log"))?;
    wait_for(&a.listen, "update proposal for mikrotik_inferred", "\"updates\":\"mikrotik_inferred\"");
    hint("Drift -> gw-drift.log tripped; Review -> mikrotik_inferred v2 with the diff (one pattern added); Approve makes it v2");
    next(a);

    say(TITLES[9]);
    let att = d.join("attestation.json").display().to_string();
    cmd(&c.attest);
    note(&format!("(exit {})", child_status(bin, &["attest", "--store", &store, "--out", &att])?));
    cmd(&c.verify_att);
    note(&format!("(exit {})", child_status(bin, &["verify", "--store", &store, "--attestation", &att])?));
    hint("the tamper below breaks the store on purpose (last step; the reset follows)");
    cmd(&c.tamper);
    tamper(&d.join("store").join("raw.seg"))?;
    cmd(&c.verify);
    note(&format!("(exit {}: that exit code and the record it named are the point)", child_status(bin, &["verify", "--store", &store])?));
    next(a);

    say("10-13 (terminal 2, not played here): see PROGRESS.md");
    for t in &TITLES[10..] {
        note(t);
    }
    if !a.auto {
        hint(&format!("the server is still up for questions; Enter stops it and resets {}", d.display()));
        enter();
    }
    Ok(())
}

// --- --check: the inputs, the ports, and no drift from PROGRESS.md ---

fn item(label: &str, ok: bool, detail: &str) -> bool {
    if ok {
        println!("ok    {label}");
    } else {
        println!("DRIFT {label}: {detail}");
    }
    ok
}

fn demo_section(progress: &str) -> Result<&str> {
    let start = progress.find("## Demo (").context("PROGRESS.md has no `## Demo (` section")?;
    let rest = &progress[start..];
    let end = rest.find("\n---\n").unwrap_or(rest.len());
    Ok(&rest[..end])
}

fn port_free(addr: &SocketAddr, udp: bool) -> bool {
    TcpListener::bind(addr).is_ok() && (!udp || UdpSocket::bind(addr).is_ok())
}

fn check(a: &Args) -> Result<i32> {
    let repo = &a.repo;
    let mut ok = true;
    let samples = logs_in(&repo.join("samples"), "log").unwrap_or_default();
    ok &= item("samples/*.log (12 or more)", samples.len() >= 12, &format!("{} found in {}", samples.len(), repo.join("samples").display()));
    for rel in ["heldout/mikrotik.log", "heldout/edgerouter.log", "mappings/ocsf.toml"] {
        ok &= item(rel, repo.join(rel).is_file(), "missing");
    }
    let parsers = logs_in(&repo.join("parsers"), "toml").unwrap_or_default();
    ok &= item("parsers/*.toml", !parsers.is_empty(), "none found");
    ok &= item(&format!("port {} free", a.listen), port_free(&a.listen, false), "in use: stop whatever holds it");
    ok &= item(&format!("port {} free (udp and tcp)", a.syslog), port_free(&a.syslog, true), "in use: stop whatever holds it");

    let path = repo.join("PROGRESS.md");
    let progress = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let section = demo_section(&progress)?;
    for (i, title) in TITLES.iter().enumerate() {
        ok &= item(&format!("step {i} title"), section.contains(&format!("# {title}")), title);
    }
    for (i, text) in Cmds::new(a).all().iter().enumerate() {
        ok &= item(&format!("command {i}"), section.contains(text), text);
    }
    for text in ANCHORS {
        ok &= item("step 8 input", section.contains(text), text);
    }
    println!("{}", if ok { "demo --check: no drift" } else { "demo --check: DRIFT (PROGRESS.md and the runner must read the same)" });
    Ok(if ok { 0 } else { 1 })
}

pub fn main(a: Args) -> Result<i32> {
    if a.check {
        return check(&a);
    }
    if a.reset {
        kill_leftover(&a.dir);
        if a.dir.exists() {
            std::fs::remove_dir_all(&a.dir).with_context(|| format!("removing {}", a.dir.display()))?;
        }
        let purged = purge_generated(&a.repo)?;
        println!("reset: {} removed, {purged} generated parser(s) removed from {}", a.dir.display(), a.repo.join("parsers").display());
        return Ok(0);
    }
    let bin = std::env::current_exe().context("finding this binary")?;
    for rel in ["samples", "heldout/mikrotik.log", "heldout/edgerouter.log", "parsers", "PROGRESS.md"] {
        if !a.repo.join(rel).exists() {
            bail!("{} not found: run from the repository root or pass --repo", a.repo.join(rel).display());
        }
    }
    kill_leftover(&a.dir);
    let mut srv = None;
    let result = play(&a, &bin, &mut srv);
    if let Some(s) = &mut srv {
        s.stop();
    }
    if a.dir.exists() {
        let _ = std::fs::remove_dir_all(&a.dir);
    }
    match result {
        Ok(()) => {
            say(&format!("done: stopped and reset ({} removed)", a.dir.display()));
            Ok(0)
        }
        Err(e) => {
            eprintln!("ulpf demo: {e:#}");
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The check mode's own guarantee, run by `cargo test`: every title and command the runner
    // prints is still the text PROGRESS.md carries.
    #[test]
    fn the_runner_and_progress_read_the_same() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let progress = std::fs::read_to_string(repo.join("PROGRESS.md")).unwrap();
        let section = demo_section(&progress).unwrap();
        let a = Args {
            auto: false,
            check: true,
            reset: false,
            dir: PathBuf::from("demo"),
            listen: "127.0.0.1:7878".parse().unwrap(),
            syslog: "127.0.0.1:5514".parse().unwrap(),
            repo,
        };
        for title in TITLES {
            assert!(section.contains(&format!("# {title}")), "PROGRESS.md lost the heading: {title}");
        }
        for text in Cmds::new(&a).all() {
            assert!(section.contains(text), "PROGRESS.md lost the command: {text}");
        }
        for text in ANCHORS {
            assert!(section.contains(text), "PROGRESS.md lost: {text}");
        }
    }

    // Every route the runner calls answers with content-length, so this path is unreached; it
    // still may not panic, which a byte length landing inside a multibyte char used to do.
    #[test]
    fn dechunk_never_panics_on_a_length_it_cannot_trust() {
        assert_eq!(dechunk("4\r\nabcd\r\n0\r\n\r\n"), "abcd");
        assert_eq!(dechunk("2\r\n\u{e9}\r\n0\r\n\r\n"), "\u{e9}");
        assert_eq!(dechunk("1\r\n\u{e9}\r\n0\r\n\r\n"), "");
        assert_eq!(dechunk("99\r\nshort\r\n"), "short\r\n"); // truncated: what is left, not a panic
    }
}
