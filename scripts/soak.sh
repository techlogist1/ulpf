#!/usr/bin/env bash
# Soaks `ulpf serve` against a continuously appended file and, when asked, against the
# syslog listeners. Everything a run produces lands under --out: the generated counts,
# the server's log and counter block, RSS/thread samples, metrics polls, the SSE frame
# log and the report. Exit 0 only when every event the generator sent is framed, stored,
# emitted and readable back out of the raw store.
#
#   scripts/soak.sh --bin ./target/release/ulpf --minutes 12 --file-rate 150000 \
#                   [--udp 127.0.0.1:5514 --udp-rate 20000] [--tcp 127.0.0.1:5515 --tcp-rate 20000] \
#                   [--events-target 10000000] [--watch DIR] [--out DIR]
#   scripts/soak.sh --selftest [--gen PATH]      # generator + socket senders, ~20 s, no server
#   scripts/soak.sh --report-only DIR            # re-report a finished or interrupted run
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN=""; GEN=""; MINUTES=12; FILE_RATE=100000; UDP=""; UDP_RATE=0; TCP=""; TCP_RATE=0
EVENTS_TARGET=0; WATCH=""; OUT=""; LISTEN=127.0.0.1:7878; BURST_SECS=60; BURST_MULT=3
UNKNOWN="$ROOT/heldout/mikrotik.log"; SELFTEST=0; TCP_OCTET=""; REPORT_ONLY=""

die() { printf 'soak: %s\n' "$*" >&2; exit 2; }

while [ $# -gt 0 ]; do
  case $1 in
    --bin) BIN=$2; shift 2 ;;
    --gen) GEN=$2; shift 2 ;;
    --minutes) MINUTES=$2; shift 2 ;;
    --file-rate) FILE_RATE=$2; shift 2 ;;
    --udp) UDP=$2; shift 2 ;;
    --udp-rate) UDP_RATE=$2; shift 2 ;;
    --tcp) TCP=$2; shift 2 ;;
    --tcp-rate) TCP_RATE=$2; shift 2 ;;
    --tcp-octet-counting) TCP_OCTET=--tcp-octet-counting; shift ;;
    --events-target) EVENTS_TARGET=$2; shift 2 ;;
    --watch) WATCH=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --listen) LISTEN=$2; shift 2 ;;
    --burst-secs) BURST_SECS=$2; shift 2 ;;
    --burst-mult) BURST_MULT=$2; shift 2 ;;
    --unknown) UNKNOWN=$2; shift 2 ;;
    --selftest) SELFTEST=1; shift ;;
    --report-only) REPORT_ONLY=$2; shift 2 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) die "unknown argument $1" ;;
  esac
done

command -v python3 >/dev/null || die "python3 is required (SSE client, samplers, report)"
command -v curl >/dev/null || die "curl is required"

# ---------------------------------------------------------------- report
# `report_run DIR [DRAIN_SECS]` prints the one-screen report for a run directory and
# exits 0 PASS / 1 FAIL / 3 PARTIAL. Every input is optional: a run that was killed
# mid-flight still reports what it proved, and names what it cannot say. With
# SOAK_BIN set it runs `ulpf verify` itself when verify.txt is absent.
report_run() {
  python3 - "$1" "${2:-}" <<'PY'
import json, os, re, subprocess, sys
out = sys.argv[1]
drain = float(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2] else None
BIN = os.environ.get("SOAK_BIN", "")

def read(p, d=""):
    try:
        return open(f"{out}/{p}").read()
    except OSError:
        return d

def jload(p, d=None):
    try:
        return json.load(open(f"{out}/{p}"))
    except Exception:
        return d

def num(pat, text, cast=int, default=None):
    m = re.search(pat, text)
    return cast(m.group(1)) if m else default

def n(v, w=14):
    return f"{v:>{w},}" if isinstance(v, int) else f"{'unknown':>{w}}"

missing = []          # inputs this report did not have
unknown = []          # reconciliation lines it therefore could not decide
fail = []

counts = jload("counts.json")
if counts is None:
    missing.append("counts.json — what the generator sent is unknown, so nothing reconciles against it")
    counts = {}
elif not counts.get("done"):
    missing.append("counts.json is a mid-run sample (\"done\": false): the generator never wrote its final total")
serve = read("serve.log")
if not serve:
    missing.append("serve.log")
sse = jload("sse-summary.json")
if sse is None:
    missing.append("sse-summary.json — the SSE monitor never wrote its summary (killed?)")
    sse = {}
metrics = [json.loads(l) for l in read("metrics.jsonl").splitlines() if l.strip().startswith("{")]
metrics = [m for m in metrics if "engine" in m]
final = jload("final-metrics.json")
last = final or (metrics[-1] if metrics else None)

# ---- engine numbers: the shutdown counter block, else the last metrics sample
KEYS = ("framed", "stored", "detected", "no_parser", "parsed", "normalized", "emitted")
if "stages:" in serve:
    stages = {k: num(rf"stages:.*?\b{k} (\d+)", serve) for k in KEYS}
    source = "the counter block serve printed on a clean shutdown"
    rate = num(r"-> ([\d.]+) events/s", serve, float)
    wall = num(r"events in ([\d.]+) s", serve, float)
    mbs = num(r"events/s, ([\d.]+) MB/s", serve, float)
    hw = num(r"high-water (\d+)/", serve)
    cap = num(r"high-water \d+/(\d+)", serve)
    blocks = num(r"backpressure blocks (\d+)", serve)
    batches = num(r"queue: (\d+) batches", serve)
elif last:
    e = last["engine"]
    stages = {k: e.get(k) for k in KEYS}
    when = "polled just before shutdown" if final else "the LAST 5 s /api/metrics poll, so up to 5 s of events are missing from every number below"
    source = f"{when} — serve printed no counter block, so it was killed rather than stopped"
    missing.append("the final counter block (serve was killed, not SIGINTed): engine numbers are a metrics sample")
    rate, wall, mbs = e.get("events_per_sec"), e.get("elapsed_secs"), e.get("mb_per_sec")
    hw, cap = e.get("queue_high_water"), e.get("queue_capacity")
    blocks, batches = e.get("backpressure_blocks"), e.get("batches")
else:
    stages = dict.fromkeys(KEYS)
    source = "nothing — no counter block in serve.log and no /api/metrics sample"
    missing.append("every engine number: no counter block and no metrics.jsonl")
    rate = wall = mbs = hw = cap = blocks = batches = None

# out.jsonl is the only independent witness to `emitted` when the engine never said it
outp = f"{out}/out.jsonl"
out_bytes = os.path.getsize(outp) if os.path.exists(outp) else None
out_lines = None
if out_bytes is not None and stages["emitted"] is None:
    r = subprocess.run(["wc", "-l", outp], capture_output=True, text=True).stdout.split()
    out_lines = int(r[0]) if r else None

verify = read("verify.txt")
if not verify and BIN and os.path.isdir(f"{out}/store"):
    verify = subprocess.run([BIN, "verify", "--store", f"{out}/store"], capture_output=True, text=True).stdout
    open(f"{out}/verify.txt", "w").write(verify)
records = num(r"verified (\d+) records", verify)
corrupt = num(r"records, (\d+) corrupt", verify)
chain_ok = "chain ok" in verify
if records is None:
    missing.append("ulpf verify over the store (no verify.txt; re-run with SOAK_BIN=<ulpf> to have this report run it)")

sent_file = counts.get("file_events")
sent_udp = counts.get("udp_events", 0)
sent_tcp = counts.get("tcp_events", 0)
sent = None if sent_file is None else sent_file + sent_udp + sent_tcp
elapsed = counts.get("elapsed_secs") or 0.0

# per-source counts, so the syslog listeners reconcile separately from the file
srcs = (last or {}).get("sources") or []
def by_prefix(p):
    return sum(s.get("events", 0) for s in srcs if str(s.get("name", "")).startswith(p))
got_udp, got_tcp = by_prefix("udp/"), by_prefix("tcp/")
if (sent_udp or sent_tcp) and not srcs:
    missing.append("per-source events (no metrics sample): udp/tcp cannot be reconciled separately")

rss = [l.split("\t") for l in read("rss.tsv").splitlines()[1:] if "\t" in l]
rss = [(float(a), int(b), int(c)) for a, b, c in rss]
if not rss:
    missing.append("rss.tsv — no memory samples, so nothing is known about a leak")

def slope(rows):
    if len(rows) < 3:
        return None
    t0 = rows[0][0]
    xs = [r[0] - t0 for r in rows]
    ys = [r[1] / 1024.0 for r in rows]
    mx = sum(xs) / len(xs); my = sum(ys) / len(ys)
    den = sum((x - mx) ** 2 for x in xs)
    return None if den == 0 else sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den * 60.0

def sl(v):
    return "n/a" if v is None else f"{v:+.2f} MB/min"

burst = [m for m in metrics if (m.get("gen") or {}).get("phase") == 2]
pre = [m for m in metrics if (m.get("gen") or {}).get("phase") == 1]
def eng(m, k):
    return m["engine"].get(k, 0)
burst_blocks = (eng(burst[-1], "backpressure_blocks") - eng(burst[0], "backpressure_blocks")) if len(burst) > 1 else None
burst_hw = max((eng(m, "queue_high_water") for m in burst), default=None)
pre_blocks = eng(pre[-1], "backpressure_blocks") if pre else None
pre_hw = max((eng(m, "queue_high_water") for m in pre), default=None)

P = print
P("=" * 78)
P(f"ULPF soak report   {out}")
plan = read("plan.txt").strip()
if plan:
    P(plan)
P(f"engine numbers from: {source}")
P("=" * 78)
if sent_file is None:
    P("generator   counts.json missing — nothing known about what was sent")
else:
    P(f"generator   file {sent_file:>12,} events in {elapsed:8.1f} s = {sent_file/max(elapsed,1e-9):>9,.0f} ev/s"
      f"   ({counts.get('file_bytes',0)/1048576:.0f} MB, {counts.get('behind_chunks',0)} chunks behind schedule)")
if sent_udp or sent_tcp:
    P(f"            udp  {sent_udp:>12,} sent ({counts.get('udp_errors',0)} errors)   "
      f"tcp {sent_tcp:>12,} sent ({counts.get('tcp_connects',0)} connections, {counts.get('tcp_errors',0)} errors)")
P(f"engine      {n(stages['framed'], 12)} events in {wall or 0:8.1f} s = {rate or 0:>9,.0f} ev/s   "
  f"({mbs or 0:.1f} MB/s, {batches or 0:,} batches)")
P(f"queue       high-water {hw}/{cap}   backpressure blocks {blocks}   "
  f"engaged: {'YES' if (blocks or 0) > 0 else 'no' if blocks is not None else 'unknown'}")
if burst_blocks is not None:
    P(f"  before    phase 1 (1x): {pre_blocks:,} blocks, queue high-water {pre_hw}/{cap}")
    P(f"  burst     phase 2 (3x): +{burst_blocks:,} blocks, queue high-water {burst_hw}/{cap}")
else:
    P("  burst     no phase-2 metrics samples in this run")
if rss:
    mb = [r[1] / 1024.0 for r in rss]
    span = rss[-1][0] - rss[0][0]
    half = [r for r in rss if r[0] >= rss[0][0] + span / 2]
    last5 = [r for r in rss if r[0] >= rss[-1][0] - 300]
    P(f"RSS         min {min(mb):.1f} MB  max {max(mb):.1f} MB  last {mb[-1]:.1f} MB   over {span:.0f} s, {len(rss)} samples")
    P(f"            slope all {sl(slope(rss))}   2nd half {sl(slope(half))}   last 5 min {sl(slope(last5))}")
    P(f"threads     min {min(r[2] for r in rss)}  max {max(r[2] for r in rss)}")
if out_bytes is not None:
    P(f"output      out.jsonl {out_bytes/1048576:,.0f} MB" + (f", {out_lines:,} lines (counted here)" if out_lines is not None else ""))
P(f"SSE         {sse.get('frames',0):,} frames {sse.get('by_event',{})}   {sse.get('events',0):,} tail events   "
  f"skipped {sse.get('skipped',0):,}")
P(f"            max gap {sse.get('max_gap_secs',0):.2f} s   gaps > 5 s: {sse.get('gaps_over_5s',0)}   reconnects {sse.get('reconnects',0)}")
P(f"drain       {'unknown (run did not reach the drain)' if drain is None else f'{drain:.0f} s after the last append'}")
if verify.strip():
    for l in verify.strip().splitlines():
        P(f"store       {l}")
for k in ("signals: ", "parse_failed by reason: ", "inference: ", "drift: "):
    m = re.search(rf"^{k}.*$", serve, re.M)
    if m:
        P(m.group(0))
P("-" * 78)

def check(label, a, b):
    if a is None or b is None:
        unknown.append(label)
        P(f"  ??    {label:<34} {n(a)}  vs  {n(b)}")
        return
    if a != b:
        fail.append(f"{label}: {a:,} != {b:,} (delta {b-a:+,})")
    P(f"  {'ok  ' if a == b else 'FAIL'}  {label:<34} {a:>14,}  vs  {b:>14,}")

P("reconciliation")
# A run that never drained has the tailer legitimately behind the appender; calling that
# a lost event would be a lie, so it is reported as a lag and the verdict stays partial.
drained = bool(counts.get("done")) and "stages:" in serve
if sent is not None and stages["framed"] is not None and not drained:
    # counts.json and the metrics poll are written by different processes at different
    # instants; only the `gen` block recorded inside a metrics sample is a paired reading,
    # so the lag is measured from that when it is there.
    paired = (last or {}).get("gen") if last else None
    if paired and paired.get("file_events") is not None:
        a = paired["file_events"] + paired.get("udp_events", 0) + paired.get("tcp_events", 0)
        b = last["engine"]["framed"]
        how = "paired sample"
    else:
        a, b, how = sent, stages["framed"], "unpaired files (counts.json vs the metrics poll)"
    lag = a - b
    unknown.append("sent vs framed (never drained)")
    P(f"  ??    {'sent (file+udp+tcp)   vs framed':<34} {a:>14,}  vs  {b:>14,}")
    P(f"        the run never drained, so this is a lag, not a loss: {lag:+,} events "
      f"({lag/max(rate or 1,1):+.1f} s) from the {how}.")
else:
    check("sent (file+udp+tcp)   vs framed", sent, stages["framed"])
check("framed                vs stored", stages["framed"], stages["stored"])
check("stored                vs emitted", stages["stored"], stages["emitted"] if stages["emitted"] is not None else out_lines)
if records is not None and stages["stored"] is not None and not drained and records >= stages["stored"]:
    # The store kept writing after the metrics sample was taken, so more records than the
    # snapshot counted is a stale reading. Only a SHORTFALL here would be a lost record.
    ahead = records - stages["stored"]
    unknown.append("stored vs verify records (stale poll)")
    P(f"  ??    {'stored                vs verify records':<34} {stages['stored']:>14,}  vs  {records:>14,}")
    P(f"        verify counts {ahead:,} more ({ahead/max(rate or 1, 1):.1f} s of events): the store kept")
    P("        writing after the last metrics poll. Only a shortfall here would be a lost record.")
else:
    check("stored                vs verify records", stages["stored"], records)
if sent_udp or sent_tcp or got_udp or got_tcp:
    check("udp sent              vs udp source events", sent_udp, got_udp if srcs else None)
    check("tcp sent              vs tcp source events", sent_tcp, got_tcp if srcs else None)
    if srcs and got_udp < sent_udp and not counts.get("udp_errors"):
        P("  note  a UDP shortfall with 0 sender errors is a kernel drop, not a lost event:")
        P("        `netstat -s -p udp` names it (dropped due to full socket buffers).")
if corrupt:
    fail.append(f"{corrupt} corrupt records in the store")
if records is not None and not chain_ok:
    fail.append("the store's integrity chain did not verify")
P(f"  {'ok  ' if records is not None and not corrupt else '??  '}  {'corrupt records':<34} "
  f"{corrupt if corrupt is not None else 'unknown':>14}   chain {'ok' if chain_ok else 'not verified'}")

# The counter block is not noise: it says `time_error` and `parse_failed` on a clean run.
block = re.compile(r"^(stages:|signals:|queue:|inference:|drift:|parse_failed|pending |files |[\d,]+ events)")
noise = [l for l in serve.splitlines()
         if re.search(r"input problem|load problem|shrank|panic|error|reload:", l, re.I)
         and not block.match(l.strip())]
# The RSS sampler is a 1 s loop in the same process as the SSE client. If it stopped
# for the same window, the host suspended (a laptop sleeping) and the server never
# stalled: both clocks froze together. Only a gap the sampler did NOT see is a stall.
host_gaps = [b[0] - a[0] for a, b in zip(rss, rss[1:]) if b[0] - a[0] > 5]
if sse.get("gaps_over_5s"):
    covered = len(host_gaps) >= sse["gaps_over_5s"] and max(host_gaps, default=0) >= sse["max_gap_secs"] * 0.8
    if covered:
        P(f"  note  {sse['gaps_over_5s']} SSE gaps over 5 s (max {sse['max_gap_secs']:.0f} s) match "
          f"{len(host_gaps)} gaps in the 1 s RSS sampler (max {max(host_gaps):.0f} s):")
        P("        the host suspended, both clocks froze together. Not a server stall.")
    else:
        fail.append(f"{sse['gaps_over_5s']} SSE gaps over 5 s (max {sse['max_gap_secs']:.1f} s) "
                    f"the RSS sampler did not see: the server stalled")
if sse.get("errors"):
    noise += [f"sse: {e}" for e in sse["errors"][:5]]
P("-" * 78)
P(f"server log notes: {len(noise)}")
for l in noise[:10]:
    P(f"  {l.strip()[:110]}")
if missing:
    P("-" * 78)
    P(f"NOT MEASURED ({len(missing)}) — this report is partial:")
    for m in missing:
        P(f"  - {m}")
    if unknown:
        P(f"  therefore undecidable: {', '.join(unknown)}")
P("=" * 78)
P("SOAK FAIL" if fail else "SOAK PARTIAL" if (missing or unknown) else "SOAK PASS")
for f in fail:
    P(f"  {f}")
sys.exit(1 if fail else 3 if (missing or unknown) else 0)
PY
}

if [ -n "$REPORT_ONLY" ]; then
  [ -d "$REPORT_ONLY" ] || die "$REPORT_ONLY is not a directory"
  REPORT_ONLY=$(cd "$REPORT_ONLY" && pwd)
  report_run "$REPORT_ONLY" "" | tee "$REPORT_ONLY/report.txt"
  exit ${PIPESTATUS[0]}
fi

# The generator is an example of this workspace; build it if it is not already there.
if [ -z "$GEN" ]; then
  GEN=${CARGO_TARGET_DIR:-$ROOT/target}/release/examples/soak_gen
  if [ ! -x "$GEN" ]; then
    printf 'soak: building the generator (cargo build --release -p ulpf --example soak_gen)\n' >&2
    (cd "$ROOT" && cargo build --release -p ulpf --example soak_gen >&2) || die "generator build failed"
  fi
fi
[ -x "$GEN" ] || die "generator $GEN not found; pass --gen PATH"
[ -f "$UNKNOWN" ] || UNKNOWN=""

# ---------------------------------------------------------------- selftest
# Proves the generator and both socket senders before anyone soaks with them: every line
# distinct, the real parsers still claim them, and a python listener receives exactly what
# the sender counted.
if [ "$SELFTEST" = 1 ]; then
  TD=$(mktemp -d "${TMPDIR:-/tmp}/ulpf-soak-selftest.XXXXXX") || exit 2
  trap 'rm -rf "$TD"' EXIT
  printf '== generator: 200000 events ==\n'
  "$GEN" --samples "$ROOT/samples" --parsers "$ROOT/parsers" ${UNKNOWN:+--unknown "$UNKNOWN"} --selftest 200000 || die "generator selftest failed"
  RC=0
  for FRAMING in line octet; do
  printf '== sockets: udp 127.0.0.1:5599, tcp 127.0.0.1:5598 (%s framing), 5 s ==\n' "$FRAMING"
  python3 - "$TD/listener.json" "$FRAMING" >"$TD/listener-$FRAMING.log" 2>&1 <<'PY' &
import json, socket, sys, threading, time
framing = sys.argv[2]
udp_n = tcp_n = 0
udp_b = tcp_b = 0
tcp_conns = 0
stop = time.time() + 20
u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
u.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 8 << 20)
u.bind(("127.0.0.1", 5599)); u.settimeout(0.5)
t = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
t.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
t.bind(("127.0.0.1", 5598)); t.listen(4); t.settimeout(0.5)
def udp_loop():
    global udp_n, udp_b
    while time.time() < stop:
        try:
            d, _ = u.recvfrom(65535)
        except OSError:
            continue
        udp_n += 1; udp_b += len(d)
def tcp_conn(c):
    global tcp_n, tcp_b
    rest = b""
    c.settimeout(0.5)
    while time.time() < stop:
        try:
            d = c.recv(1 << 16)
        except OSError:
            continue
        if not d:
            break
        tcp_b += len(d); rest += d
        if framing == "line":
            *lines, rest = rest.split(b"\n")
            tcp_n += len(lines)
            continue
        # RFC 6587 octet counting: <len> SP <body>
        while True:
            sp = rest.find(b" ")
            if sp < 0 or not rest[:sp].isdigit():
                break
            want = int(rest[:sp])
            if len(rest) < sp + 1 + want:
                break
            rest = rest[sp + 1 + want:]
            tcp_n += 1
def tcp_loop():
    global tcp_conns
    while time.time() < stop:
        try:
            c, _ = t.accept()
        except OSError:
            continue
        tcp_conns += 1
        threading.Thread(target=tcp_conn, args=(c,), daemon=True).start()
th = [threading.Thread(target=f, daemon=True) for f in (udp_loop, tcp_loop)]
[x.start() for x in th]
[x.join() for x in th]
time.sleep(1)
json.dump({"udp_events": udp_n, "udp_bytes": udp_b, "tcp_events": tcp_n, "tcp_bytes": tcp_b, "tcp_connections": tcp_conns}, open(sys.argv[1], "w"))
PY
  LPID=$!
  sleep 1
  OCTET=""; [ "$FRAMING" = octet ] && OCTET=--tcp-octet-counting
  "$GEN" --samples "$ROOT/samples" --counts "$TD/counts.json" --socket-secs 5 \
    --udp 127.0.0.1:5599 --udp-rate 20000 --tcp 127.0.0.1:5598 --tcp-rate 20000 $OCTET >"$TD/gen.json" || die "socket send failed"
  wait $LPID
  python3 - "$TD/gen.json" "$TD/listener.json" <<'PY'
import json, sys
g = json.load(open(sys.argv[1])); l = json.load(open(sys.argv[2]))
ok = True
for k in ("udp_events", "tcp_events"):
    sent, got = g[k], l[k]
    # UDP over loopback may still drop under a full socket buffer; TCP may not.
    limit = 0.98 if k.startswith("udp") else 1.0
    good = got == sent if limit == 1.0 else got >= sent * limit
    ok &= good
    print(f"{k}: sent {sent}, received {got}  {'ok' if good else 'FAIL'}")
print(f"tcp connections {l['tcp_connections']}, udp errors {g['udp_errors']}, tcp errors {g['tcp_errors']}")
sys.exit(0 if ok else 1)
PY
  [ $? = 0 ] || RC=1
  done
  exit $RC
fi

# ---------------------------------------------------------------- setup
[ -n "$BIN" ] || die "--bin PATH is required"
[ -x "$BIN" ] || die "$BIN is not executable"
# The syslog listeners land after the senders; fail on that plainly rather than on a clap error.
if [ -n "$UDP$TCP" ] && ! "$BIN" serve --help 2>&1 | grep -q -- --syslog-udp; then
  die "$BIN serve has no --syslog-udp/--syslog-tcp yet: the listeners have not landed. Drop --udp/--tcp to soak the file path alone."
fi
[ -n "$OUT" ] || OUT=$ROOT/soak/run-$(date +%Y%m%d-%H%M%S)
mkdir -p "$OUT" || die "cannot create $OUT"
OUT=$(cd "$OUT" && pwd)
[ -n "$WATCH" ] || WATCH=$OUT/watch
mkdir -p "$WATCH" "$OUT/pending"
[ -e "$OUT/store" ] && die "$OUT/store exists; a soak wants a fresh store"

TOTAL=$((MINUTES * 60))
BURST_RATE=$((FILE_RATE * BURST_MULT))
[ "$BURST_SECS" -lt "$TOTAL" ] || BURST_SECS=0
PRE=$(((TOTAL - BURST_SECS) / 2)); POST=$((TOTAL - BURST_SECS - PRE))
PHASES=(--phase "$FILE_RATE:$PRE")
[ "$BURST_SECS" -gt 0 ] && PHASES+=(--phase "$BURST_RATE:$BURST_SECS")
[ "$POST" -gt 0 ] && PHASES+=(--phase "$FILE_RATE:$POST")

EXPECT_FILE=$((FILE_RATE * (PRE + POST) + BURST_RATE * BURST_SECS))
[ "$EVENTS_TARGET" -gt 0 ] && [ "$EXPECT_FILE" -gt "$EVENTS_TARGET" ] && EXPECT_FILE=$EVENTS_TARGET
FREE_MB=$(df -m "$OUT" | awk 'NR==2 {print $4}')
# measured on the 4.8M-event smoke run: 1494 MB input + 1814 MB store + 6353 MB jsonl
NEED_MB=$((EXPECT_FILE / 1000 * 2100 / 1024))
printf 'soak: %s minutes, file %s/s (burst %s/s for %ss), planned %s events, ~%s MB of disk, %s MB free\n' \
  "$MINUTES" "$FILE_RATE" "$BURST_RATE" "$BURST_SECS" "$EXPECT_FILE" "$NEED_MB" "$FREE_MB" | tee "$OUT/plan.txt"
[ "$NEED_MB" -lt $((FREE_MB / 2)) ] || die "refusing: the plan needs about ${NEED_MB} MB and only ${FREE_MB} MB are free"

SERVE_PID=""; GEN_PID=""; MON_PID=""
cleanup() {
  [ -n "$GEN_PID" ] && kill -TERM "$GEN_PID" 2>/dev/null
  [ -n "$MON_PID" ] && kill -TERM "$MON_PID" 2>/dev/null
  [ -n "$SERVE_PID" ] && kill -INT "$SERVE_PID" 2>/dev/null
}
trap 'cleanup; exit 130' INT TERM

"$GEN" --samples "$ROOT/samples" --parsers "$ROOT/parsers" ${UNKNOWN:+--unknown "$UNKNOWN"} --selftest 100000 >"$OUT/gen-selftest.json" || die "generator selftest failed"
printf 'soak: generator %s\n' "$(cat "$OUT/gen-selftest.json")"

# ---------------------------------------------------------------- server
"$BIN" serve "$WATCH" --store "$OUT/store" --output "$OUT/out.jsonl" \
  --parsers "$ROOT/parsers" --mappings "$ROOT/mappings" --pending "$OUT/pending" \
  --tail 1000 --listen "$LISTEN" ${UDP:+--syslog-udp "$UDP"} ${TCP:+--syslog-tcp "$TCP"} \
  >"$OUT/serve.log" 2>&1 &
SERVE_PID=$!
trap 'cleanup' EXIT

for _ in $(seq 40); do
  curl -fsS "http://$LISTEN/api/status" -o "$OUT/status.json" && break
  kill -0 "$SERVE_PID" 2>/dev/null || { cat "$OUT/serve.log" >&2; die "serve exited during startup"; }
  sleep 0.5
done
[ -s "$OUT/status.json" ] || { cat "$OUT/serve.log" >&2; die "no /api/status after 20 s"; }
printf 'soak: serve pid %s on http://%s (store %s)\n' "$SERVE_PID" "$LISTEN" "$OUT/store"

# ---------------------------------------------------------------- monitor
# One process, three threads: the SSE client that must stay live for the whole run, the
# 5 s metrics poll, and the 1 s RSS/thread sample of the serve process.
python3 - "$LISTEN" "$SERVE_PID" "$OUT" >"$OUT/monitor.log" 2>&1 <<'PY' &
import json, os, signal, subprocess, sys, threading, time, urllib.request
listen, pid, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
stop = threading.Event()
signal.signal(signal.SIGTERM, lambda *_: stop.set())
sse = {"frames": 0, "by_event": {}, "events": 0, "skipped": 0, "max_gap_secs": 0.0,
       "max_gap_at": None, "gaps_over_5s": 0, "reconnects": 0, "errors": [],
       "first_frame": None, "last_frame": None}

def sse_client():
    log = open(f"{out}/sse.jsonl", "w", buffering=1)
    last = None
    while not stop.is_set():
        try:
            r = urllib.request.urlopen(f"http://{listen}/api/stream?tail=100", timeout=30)
            if sse["frames"]:
                sse["reconnects"] += 1
            kind = None
            for raw in r:
                if stop.is_set():
                    break
                line = raw.decode("utf-8", "replace").rstrip("\n")
                if line.startswith("event:"):
                    kind = line[6:].strip()
                elif line.startswith("data:"):
                    now = time.time()
                    if last is not None:
                        gap = now - last
                        if gap > sse["max_gap_secs"]:
                            sse["max_gap_secs"], sse["max_gap_at"] = gap, now
                        if gap > 5.0:
                            sse["gaps_over_5s"] += 1
                    last = now
                    sse["frames"] += 1
                    sse["by_event"][kind] = sse["by_event"].get(kind, 0) + 1
                    sse["first_frame"] = sse["first_frame"] or now
                    sse["last_frame"] = now
                    n = 0
                    try:
                        d = json.loads(line[5:])
                        f = d.get("tail", d) if kind == "hello" else d
                        n = len(f.get("events", [])) if isinstance(f, dict) else 0
                        sse["events"] += n
                        sse["skipped"] += f.get("skipped", 0) if isinstance(f, dict) else 0
                    except Exception as e:
                        sse["errors"].append(f"{kind}: {e}")
                    log.write(json.dumps({"ts": now, "event": kind, "events": n}) + "\n")
        except Exception as e:
            sse["errors"].append(str(e))
            time.sleep(1)

def metrics_poll():
    f = open(f"{out}/metrics.jsonl", "w", buffering=1)
    while not stop.is_set():
        try:
            d = json.load(urllib.request.urlopen(f"http://{listen}/api/metrics", timeout=10))
            d["ts"] = time.time()
            try:
                d["gen"] = json.load(open(f"{out}/counts.json"))
            except Exception:
                d["gen"] = None
            f.write(json.dumps(d) + "\n")
        except Exception as e:
            f.write(json.dumps({"ts": time.time(), "error": str(e)}) + "\n")
        stop.wait(5)

def threads_of(pid):
    if sys.platform == "darwin":
        o = subprocess.run(["ps", "-M", "-p", str(pid)], capture_output=True, text=True).stdout
        return max(0, len(o.strip().splitlines()) - 1)
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("Threads:"):
                return int(line.split()[1])
    except OSError:
        pass
    return 0

def rss_sample():
    f = open(f"{out}/rss.tsv", "w", buffering=1)
    f.write("epoch\trss_kb\tthreads\n")
    while not stop.is_set():
        o = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
        if not o:
            break
        f.write(f"{time.time():.1f}\t{o}\t{threads_of(pid)}\n")
        stop.wait(1)

ts = [threading.Thread(target=t, daemon=True) for t in (sse_client, metrics_poll, rss_sample)]
[t.start() for t in ts]
while not stop.is_set():
    stop.wait(1)
    if not os.path.exists(f"/proc/{pid}") and os.name == "posix":
        try:
            os.kill(pid, 0)
        except OSError:
            stop.set()
time.sleep(0.3)
json.dump(sse, open(f"{out}/sse-summary.json", "w"), indent=1)
PY
MON_PID=$!
sleep 0.5

# ---------------------------------------------------------------- load
"$GEN" --samples "$ROOT/samples" ${UNKNOWN:+--unknown "$UNKNOWN"} \
  --file "$WATCH/soak.log" "${PHASES[@]}" --counts "$OUT/counts.json" \
  ${EVENTS_TARGET:+--events-target "$EVENTS_TARGET"} \
  ${UDP:+--udp "$UDP" --udp-rate "$UDP_RATE"} ${TCP:+--tcp "$TCP" --tcp-rate "$TCP_RATE"} $TCP_OCTET \
  >"$OUT/gen.json" 2>"$OUT/gen.log" &
GEN_PID=$!
printf 'soak: generator pid %s, %s phases, counts %s\n' "$GEN_PID" "$((${#PHASES[@]} / 2))" "$OUT/counts.json"

SERVE_DIED=0
while kill -0 "$GEN_PID" 2>/dev/null; do
  sleep 10
  if ! kill -0 "$SERVE_PID" 2>/dev/null; then
    wait "$SERVE_PID"; SERVE_STATUS=$?
    printf 'serve exited on its own after %s s, status %s\n' "$SECONDS" "$SERVE_STATUS" >"$OUT/serve-died.txt"
    printf 'soak: serve died mid-run (status %s)\n' "$SERVE_STATUS" >&2
    SERVE_DIED=1
    kill -TERM "$GEN_PID" 2>/dev/null
    break
  fi
  printf 'soak: %s | %s\n' "$(date +%H:%M:%S)" "$(cat "$OUT/counts.json" 2>/dev/null)"
done
wait "$GEN_PID"

# ---------------------------------------------------------------- drain
# The tailer is up to a poll behind the appender; wait for the engine to catch up with
# what the generator counted before asking it to stop.
SENT=$(python3 -c 'import json,sys; c=json.load(open(sys.argv[1])); print(c["file_events"]+c["udp_events"]+c["tcp_events"])' "$OUT/counts.json")
printf 'soak: generator done, %s events sent; draining\n' "$SENT"
DRAIN_START=$(date +%s); LAST=-1; STALL=0
while [ "$SERVE_DIED" = 0 ]; do
  FRAMED=$(curl -fsS "http://$LISTEN/api/metrics" | python3 -c 'import json,sys; print(json.load(sys.stdin)["engine"]["framed"])' 2>/dev/null || echo -1)
  [ "$FRAMED" -ge "$SENT" ] && break
  if [ "$FRAMED" = "$LAST" ]; then STALL=$((STALL + 1)); else STALL=0; LAST=$FRAMED; fi
  [ "$STALL" -ge 60 ] && { printf 'soak: DRAIN STALLED at framed %s of %s\n' "$FRAMED" "$SENT" >&2; break; }
  sleep 1
done
DRAIN=$(( $(date +%s) - DRAIN_START ))
printf 'soak: drained in %s s\n' "$DRAIN"

# ---------------------------------------------------------------- stop
curl -fsS "http://$LISTEN/api/metrics" -o "$OUT/final-metrics.json" 2>/dev/null
kill -TERM "$MON_PID" 2>/dev/null; wait "$MON_PID" 2>/dev/null
kill -INT "$SERVE_PID" 2>/dev/null
for _ in $(seq 240); do kill -0 "$SERVE_PID" 2>/dev/null || break; sleep 0.5; done
kill -0 "$SERVE_PID" 2>/dev/null && { kill -9 "$SERVE_PID"; printf 'soak: serve did not stop within 120 s of SIGINT\n' >&2; }
wait "$SERVE_PID" 2>/dev/null
trap - EXIT

"$BIN" verify --store "$OUT/store" >"$OUT/verify.txt" 2>&1
printf 'soak: %s\n' "$(head -1 "$OUT/verify.txt")"

export SOAK_BIN="$BIN"
report_run "$OUT" "$DRAIN" | tee "$OUT/report.txt"
exit ${PIPESTATUS[0]}
