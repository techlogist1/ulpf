#!/bin/zsh
# --check uses zsh parameter indirection; under bash or sh the expansion error would skip the
# exit and fall through into the live demo, so any other shell re-executes this file with zsh.
[ -n "${ZSH_VERSION:-}" ] || exec /bin/zsh "$0" "$@"
# Plays the demo in PROGRESS.md ("Demo ... start here"), one numbered step at a time, printing the
# command it runs and what to click next. Existing subcommands and the watch directory only; the
# server uses demo/parsers and demo/pending, so the repo's parsers/ and pending/ are never written.
#   scripts/demo.sh            # interactive: Enter advances; the server stays up for questions at the end
#   scripts/demo.sh --auto     # unattended rehearsal: fixed pauses, then stop and reset
#   scripts/demo.sh --check    # every command below appears verbatim in PROGRESS.md (no drift)
#   scripts/demo.sh --reset    # stop a leftover server and remove demo/
set -u
cd "$(dirname "$0")/.."
MODE=${1:-}
BIN=./target/release/ulpf
PAUSE=3

say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
hint() { printf '   -> %s\n' "$*"; }
run()  { printf '   $ %s\n' "$1"; eval "$1"; }
next() {
  if [ "$MODE" = --auto ]; then sleep "$PAUSE"; else printf '\n   [Enter] '; read -r _; fi
}
wait_http() { for _ in $(seq 1 100); do curl -sf "$1" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
# wait_for LABEL PATTERN: poll /api/pending up to 30 s and say how long the proposal took, or that it is not there yet.
wait_for() {
  for i in $(seq 1 100); do
    if curl -s http://127.0.0.1:7878/api/pending | grep -q -- "$2"; then printf '   (%s after %d.%d s)\n' "$1" $((i*3/10)) $((i*3%10)); return 0; fi
    sleep 0.3
  done
  printf '   (%s not seen after 30 s: open Review and check by hand)\n' "$1"; return 1
}
stop_server() {
  pkill -f "ulpf serve demo/watch" 2>/dev/null && sleep 1
  pkill -9 -f "ulpf serve demo/watch" 2>/dev/null || true
}

# The commands, verbatim from PROGRESS.md; --check greps each one there.
C_RESET='rm -rf demo'
C_MK='mkdir -p demo/watch demo/parsers demo/pending && cp parsers/*.toml demo/parsers/'
C_SERVE='./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl --pending demo/pending --parsers demo/parsers --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514 --infer-threshold 64'
C_SAMPLES='for f in samples/*.log; do cp "$f" demo/watch/; sleep 1; done'
C_UDP=$(cat <<'X'
python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);[s.sendto(l,('127.0.0.1',5514)) for l in open('heldout/edgerouter.log','rb').read().splitlines()]"
X
)
C_UNKNOWN='cp heldout/mikrotik.log demo/watch/'
C_APPROVE='curl -s -X POST http://127.0.0.1:7878/api/pending/mikrotik/approve'
C_AGAIN='cp heldout/mikrotik.log demo/watch/mikrotik-again.log'
C_TRACE='curl -s http://127.0.0.1:7878/api/events/0 | python3 -m json.tool | head -40'
C_BUG="sed -i '' 's/{dst_ip:ip}/{dst_addr:ip}/g' demo/parsers/cisco_asa.toml"
C_UNDERBUG='cp samples/cisco_asa.log demo/watch/asa-under-the-bug.log'
C_FIX='cp parsers/cisco_asa.toml demo/parsers/'
C_REPLAY='curl -s -X POST http://127.0.0.1:7878/api/replay'
C_VERIFY='./target/release/ulpf verify --store demo/store'
C_DRIFT_HEAD="lines=open('heldout/mikrotik.log','rb').read().splitlines()"
C_ATTEST='./target/release/ulpf attest --store demo/store --out demo/attestation.json'
C_VERIFY_ATT='./target/release/ulpf verify --store demo/store --attestation demo/attestation.json'
C_TAMPER="printf 'X' | dd of=demo/store/raw.seg bs=1 seek=200 conv=notrunc 2>/dev/null"

if [ "$MODE" = --check ]; then
  rc=0
  for v in C_RESET C_MK C_SERVE C_SAMPLES C_UDP C_UNKNOWN C_APPROVE C_AGAIN C_TRACE C_BUG C_UNDERBUG C_FIX C_REPLAY C_VERIFY C_DRIFT_HEAD C_ATTEST C_VERIFY_ATT C_TAMPER; do
    if grep -qF -- "${(P)v}" PROGRESS.md; then printf 'ok    %s\n' "$v"; else printf 'DRIFT %s: %s\n' "$v" "${(P)v}"; rc=1; fi
  done
  exit $rc
fi
if [ "$MODE" = --reset ]; then stop_server; run "$C_RESET"; exit 0; fi
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 2; }

trap 'stop_server' EXIT
stop_server

say "0. reset"; run "$C_RESET"; run "$C_MK"
say "1. server + UI (this is terminal 1)"
printf '   $ %s\n' "$C_SERVE"; eval "$C_SERVE &"
wait_http http://127.0.0.1:7878/api/status || { echo "server did not come up"; exit 1; }
hint "open http://127.0.0.1:7878  (1 Live, 2 Review, 3 Traceback, 4 Pivot, 5 Replay, 6 Drift, 7 Integrity; ? = keys)"
next

say "2. known formats and a live device: the counters, sources and tail move"
hint "watch Live while the twelve samples land one per second"
run "$C_SAMPLES"; run "$C_UDP"
hint "Live -> sources: udp/127.0.0.1 (250 events, no parser yet), 12 sample sources parsed"
next

say "3. an unknown format: clustered at 64 lines, 'Review' appears"
run "$C_UNKNOWN"
wait_for "proposal mikrotik" '"mikrotik"'
hint "Review -> mikrotik: every slot has a name and the REASON it was chosen; uncheck a template + Regenerate to drop it"
next

say "4. approve: the Review screen's Approve, or"
if [ "$MODE" = --auto ]; then run "$C_APPROVE"; echo; else
  hint "click Approve in the UI, then Enter here (Enter without approving runs the curl)"; printf '   [Enter] '; read -r _
  curl -s http://127.0.0.1:7878/api/pending | grep -q '"mikrotik"' && { run "$C_APPROVE"; echo; }
fi
hint "Live -> parsers: mikrotik_inferred, origin approved (demo/parsers/mikrotik_inferred.toml)"
next

say "5. the same events take the fast path; the pivot sees them"
run "$C_AGAIN"
hint "Live -> mikrotik-again.log detected 250. Pivot -> src_ip 203.0.113.9: one attacker across every device; click a related value to pivot again"
next

say "6. traceback with provenance"
hint "click any tail row, or open http://127.0.0.1:7878/#/trace/0 ; hover a normalized field and its bytes light up"
run "$C_TRACE"
next

say "7. replay: a parser bug, the fix, every past event corrected, the store untouched"
run "$C_BUG"; sleep 1; run "$C_UNDERBUG"; sleep 2; run "$C_FIX"; sleep 1; run "$C_REPLAY"; echo
hint "Replay -> v2: changed = the ASA events written under the bug; WHY names demo/parsers/cisco_asa.toml"
run "$C_VERIFY"
next

say "8. drift: a device changes its format mid-stream; the update proposal carries a diff"
python3 - <<'PY'
import time
lines=open('heldout/mikrotik.log','rb').read().splitlines()
hdr=b' '.join(lines[0].split()[:4])
with open('demo/watch/gw-drift.log','ab') as f:
    for _ in range(5):
        for l in lines: f.write(l+b'\n')                                     # 1250 known lines: established
    f.flush(); time.sleep(3)
    for i in range(400):                                                     # a new message type
        f.write(hdr+b' interface,info ether%d link up (speed %dG, full duplex)\n' % (1+i%8, [1,10,25][i%3]))
PY
wait_for "update proposal for mikrotik_inferred" '"updates":"mikrotik_inferred"'
hint "Drift -> gw-drift.log tripped; Review -> mikrotik_inferred v2 with the diff (one pattern added); Approve makes it v2"
next

say "9. integrity: verify from the UI (Integrity -> Verify) or offline, and hand a stranger the attestation"
run "$C_ATTEST"; run "$C_VERIFY_ATT"
hint "the tamper below breaks the store on purpose (last step; reset follows)"
run "$C_TAMPER"; run "$C_VERIFY" || hint "exit 1 and the record named: that is the point"
next

say "10-13 (terminal 2, not played here): ECS run, throughput on bench/, kill recovery, isolation: see PROGRESS.md"
if [ "$MODE" = --auto ]; then stop_server; run "$C_RESET"; say "done: stopped and reset"; else
  hint "the server is still up for questions; Enter stops it and resets demo/"; printf '   [Enter] '; read -r _
  stop_server; run "$C_RESET"
fi
