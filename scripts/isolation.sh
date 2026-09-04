#!/usr/bin/env bash
# Proves the running ulpf binary opens no outbound connection.
# Samples the process's own sockets while it works at full rate and classifies
# every socket ever seen. Loopback listen/connect is OK, anything else fails.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
ULPF_BIN=${ULPF_BIN:-$ROOT/target/release/ulpf}
LISTEN_ADDR=${ULPF_LISTEN:-127.0.0.1:7878}

TMP=$(mktemp -d "${TMPDIR:-/tmp}/ulpf-iso.XXXXXX") || exit 1
SOCKS=$TMP/sockets; : >"$SOCKS"
CHILD=""; N=0; LIVE=0; FAILED=""
cleanup() { [ -n "$CHILD" ] && kill -9 "$CHILD" 2>/dev/null; rm -rf "$TMP"; }
trap cleanup EXIT

fail() { printf '\nISOLATION FAIL: %s\n' "$*"; exit 1; }
alive() { kill -0 "$1" 2>/dev/null; }

case $(uname -s) in
  Darwin) command -v lsof >/dev/null || fail "lsof not found; cannot see sockets on macOS"; SAMPLER=lsof ;;
  Linux)  if command -v ss >/dev/null; then SAMPLER=ss
          elif [ -r /proc/net/tcp ]; then SAMPLER=proc
          else fail "no ss and no readable /proc/net/tcp; cannot see sockets"; fi ;;
  *) fail "unsupported OS $(uname -s)" ;;
esac

# ---- sampling -------------------------------------------------------------
# Each sampler prints zero or more "PROTO|local|remote|state" lines for the pid,
# and returns 1 only when it could not observe the process at all. Zero lines
# from a working sampler is the expected, passing result for `ulpf run`.

proc_sample() {
  local pid=$1 inodes
  [ -d "/proc/$pid/fd" ] || return 1
  inodes=$(ls -l "/proc/$pid/fd" 2>/dev/null | sed -n 's/.*socket:\[\([0-9]*\)\].*/ \1 /p' | tr -d '\n')
  [ -z "$inodes" ] && return 0
  awk -v want="$inodes" '
    function hv(c) { return index("0123456789abcdef", tolower(c)) - 1 }
    function hx2(s) { return hv(substr(s,1,1))*16 + hv(substr(s,2,1)) }
    function ip4(h) { return sprintf("%d.%d.%d.%d", hx2(substr(h,7,2)), hx2(substr(h,5,2)), hx2(substr(h,3,2)), hx2(substr(h,1,2))) }
    function ip6(h,   i,w,n,o,g) {
      n=""
      for (i=0;i<4;i++) { w=substr(h,i*8+1,8); n = n substr(w,7,2) substr(w,5,2) substr(w,3,2) substr(w,1,2) }
      n=tolower(n)
      if (n=="00000000000000000000000000000001") return "::1"
      if (n=="00000000000000000000000000000000") return "::"
      if (substr(n,1,24)=="00000000000000000000ffff")
        return sprintf("%d.%d.%d.%d", hx2(substr(n,25,2)), hx2(substr(n,27,2)), hx2(substr(n,29,2)), hx2(substr(n,31,2)))
      o=""
      for (i=0;i<8;i++) { g=substr(n,i*4+1,4); o = (i==0 ? g : o ":" g) }
      return o
    }
    function addr(a,   p,h,pt) {
      p=index(a,":"); h=substr(a,1,p-1); pt=substr(a,p+1)
      h = (length(h)>8 ? "[" ip6(h) "]" : ip4(h))
      return h ":" (hx2(substr(pt,1,2))*256 + hx2(substr(pt,3,2)))
    }
    BEGIN {
      st["01"]="ESTABLISHED"; st["02"]="SYN_SENT"; st["03"]="SYN_RECV"; st["04"]="FIN_WAIT1"
      st["05"]="FIN_WAIT2";  st["06"]="TIME_WAIT"; st["07"]="CLOSE";    st["08"]="CLOSE_WAIT"
      st["09"]="LAST_ACK";   st["0A"]="LISTEN";    st["0B"]="CLOSING"
    }
    FNR==1 { proto = (FILENAME ~ /udp/) ? "UDP" : "TCP"; next }
    index(want, " " $10 " ") {
      s = st[toupper($4)]; if (s == "") s = "?" $4
      if (proto == "UDP" && (s == "CLOSE" || s == "?07")) s = "UNCONN"
      r = addr($3)
      if (r ~ /:0$/ || s == "LISTEN" || s == "UNCONN") r = "-"
      print proto "|" addr($2) "|" r "|" s
    }' /proc/net/tcp /proc/net/tcp6 /proc/net/udp /proc/net/udp6 2>/dev/null
}

sample() {
  local pid=$1 out
  case $SAMPLER in
    lsof)
      # -a is required: without it lsof ORs -p and a type selector. Listing every
      # fd (not just -i) doubles as the liveness proof -- a live pid always has
      # fds, so empty output means the sampler itself is broken.
      out=$(lsof -nP -a -p "$pid" 2>/dev/null)
      [ -n "$out" ] || return 1
      printf '%s\n' "$out" | awk '
        $5=="IPv4" || $5=="IPv6" {
          proto=$8; name=$9; s=$10; gsub(/[()]/,"",s); if (s=="") s="-"
          i=index(name,"->")
          if (i>0) { loc=substr(name,1,i-1); rem=substr(name,i+2) } else { loc=name; rem="-" }
          print proto "|" loc "|" rem "|" s }'
      ;;
    ss)
      [ -d "/proc/$pid" ] || return 1
      out=$(ss -tunpa 2>/dev/null)
      [ -n "$out" ] || return 1
      printf '%s\n' "$out" | awk -v pid="$pid" '
        index($0, "pid=" pid ",") {
          proto=toupper($1); s=$2; loc=$5; rem=$6
          if (rem ~ /:\*$/ || rem ~ /:0$/) rem="-"
          print proto "|" loc "|" rem "|" s }'
      ;;
    proc) proc_sample "$pid" || return 1 ;;
  esac
}

# sample_window PID ITERATIONS INTERVAL   (ITERATIONS=inf -> until the pid exits)
sample_window() {
  local pid=$1 iters=$2 iv=$3 i=0 raw
  while [ "$iters" = inf ] || [ "$i" -lt "$iters" ]; do
    i=$((i+1))
    alive "$pid" || break
    N=$((N+1))
    if raw=$(sample "$pid"); then
      LIVE=$((LIVE+1))
      [ -n "$raw" ] && printf '%s\n' "$raw" | sed "s/^/$N|/" >>"$SOCKS"
    else
      alive "$pid" && fail "sampler '$SAMPLER' saw nothing for live pid $pid -- isolation is unproven, not proven"
      break
    fi
    alive "$pid" || break
    sleep "$iv"
  done
}

# ---- classification -------------------------------------------------------
host_of() { local a=${1%:*}; a=${a#"["}; a=${a%"]"}; printf '%s' "$a"; }
is_lo() {
  case "$(host_of "$1")" in
    127.*|::1|0:0:0:0:0:0:0:1|0000:0000:0000:0000:0000:0000:0000:0001|localhost) return 0 ;;
  esac
  return 1
}
# A remote decides it when there is one; otherwise the bind address must be
# loopback (so a UDP socket on *:53 -- a DNS lookup -- fails here).
classify() {
  if [ "$3" != "-" ] && [ "$3" != "*:*" ]; then
    is_lo "$3" && { echo OK; return; }; echo FAIL; return
  fi
  is_lo "$2" && { echo OK; return; }; echo FAIL
}

uniq_socks() {
  awk -F'|' '{ k=$2"|"$3"|"$4"|"$5; if (!(k in f)) f[k]=$1 } END { for (k in f) print f[k]"|"k }' \
    "$SOCKS" | sort -t'|' -k1,1n
}

report() {
  local nfail=0 n proto loc rem st v
  printf '\n%-5s %-30s %-30s %-12s %-6s %s\n' PROTO LOCAL REMOTE STATE SEEN@ VERDICT
  if [ ! -s "$SOCKS" ]; then
    printf '%s\n' "(no network socket observed in any sample)"
  else
    while IFS='|' read -r n proto loc rem st; do
      v=$(classify "$proto" "$loc" "$rem" "$st")
      [ "$v" = FAIL ] && { nfail=$((nfail+1)); FAILED="$FAILED$proto $loc->$rem ($st); "; }
      printf '%-5s %-30s %-30s %-12s %-6s %s\n' "$proto" "$loc" "$rem" "$st" "$n" "$v"
    done < <(uniq_socks)
  fi
  printf '\nsampler %s, %d live samples every %s s, %d distinct socket(s)\n' \
    "$SAMPLER" "$LIVE" "${1:-0.5}" "$(uniq_socks | wc -l | tr -d ' ')"
  [ "$nfail" -gt 0 ] && fail "$nfail non-loopback socket(s): $FAILED"
  printf 'ISOLATION PASS\n'
}

need_bin() {
  [ -x "$ULPF_BIN" ] || fail "no ulpf binary at $ULPF_BIN (set ULPF_BIN, or: CARGO_TARGET_DIR=target-iso cargo build --release)"
}

# ---- modes ----------------------------------------------------------------
mode_run() {
  local input=${1:?usage: isolation.sh run <file-or-dir>} log=$TMP/run.log rc
  need_bin; [ -e "$input" ] || fail "input not found: $input"
  printf 'run: %s %s --store %s --output /dev/null\n' "$ULPF_BIN" "$input" "$TMP/store"
  "$ULPF_BIN" run "$input" --store "$TMP/store" --output /dev/null >"$log" 2>&1 &
  CHILD=$!
  sample_window "$CHILD" inf 0.5
  wait "$CHILD"; rc=$?; CHILD=""
  printf '\n--- last 6 lines of ulpf run ---\n'; tail -6 "$log"; printf -- '--------------------------------\n'
  [ "$rc" -eq 0 ] || fail "ulpf run exited $rc"
  grep -q '^stages:' "$log" || fail "ulpf run printed no counter block"
  [ "$LIVE" -ge 1 ] || fail "the run finished before a single socket sample landed -- use a larger input or docker mode"
  report 0.5
}

mode_serve() {
  local watch=${1:?usage: isolation.sh serve <watch-dir> [seconds]} secs=${2:-20}
  local log=$TMP/serve.log curlout=$TMP/metrics.json feed dest rc i
  need_bin
  "$ULPF_BIN" serve --help >/dev/null 2>&1 \
    || fail "this binary has no 'serve' subcommand ('$ULPF_BIN serve --help' failed); build the server first"
  mkdir -p "$watch" || fail "cannot create watch dir $watch"
  mkdir -p "$TMP/pending"
  printf 'serve: %s serve %s --listen %s\n' "$ULPF_BIN" "$watch" "$LISTEN_ADDR"
  "$ULPF_BIN" serve "$watch" --listen "$LISTEN_ADDR" --store "$TMP/store" \
    --output "$TMP/out.jsonl" --pending "$TMP/pending" >"$log" 2>&1 &
  CHILD=$!

  for i in $(seq 1 40); do
    alive "$CHILD" || break
    sample "$CHILD" | awk -F'|' -v l="$LISTEN_ADDR" '$2==l && $4=="LISTEN" { f=1 } END { exit !f }' && break
    sleep 0.25
  done
  alive "$CHILD" || { tail -20 "$log"; fail "serve exited before it listened"; }
  sample "$CHILD" | awk -F'|' -v l="$LISTEN_ADDR" '$2==l && $4=="LISTEN" { f=1 } END { exit !f }' \
    || fail "serve never opened a listen socket on $LISTEN_ADDR within 10 s"
  printf 'listening on %s\n' "$LISTEN_ADDR"

  feed=${ULPF_FEED:-}
  if [ -z "$feed" ]; then
    if [ -f "$ROOT/bench/mixed-5000000.log" ]; then feed=$ROOT/bench/mixed-5000000.log; else feed=$ROOT/samples; fi
  fi
  [ -e "$feed" ] || fail "feed not found: $feed"
  dest=$watch/$(basename "$feed")
  # A 1.6 GB copy would dominate the window, so large feeds go in as a symlink.
  # Set ULPF_FEED_MODE=copy if the watcher turns out not to follow symlinks.
  local fmode=${ULPF_FEED_MODE:-auto}
  if [ "$fmode" = auto ]; then
    if [ -f "$feed" ] && [ "$(wc -c <"$feed")" -gt 268435456 ]; then fmode=symlink; else fmode=copy; fi
  fi
  case $fmode in
    symlink) ln -sfn "$feed" "$dest" || fail "cannot symlink feed into $watch" ;;
    copy)    cp -R "$feed" "$dest" || fail "cannot copy feed into $watch" ;;
    *) fail "ULPF_FEED_MODE must be auto, copy or symlink" ;;
  esac
  printf 'fed %s into %s (%s)\n' "$feed" "$watch" "$fmode"

  sample_window "$CHILD" 4 0.5
  # One loopback client. The burst is fine-grained because a metrics fetch over
  # loopback lives for a couple of milliseconds.
  curl -sS -m 10 -o "$curlout" -w '%{http_code}' "http://$LISTEN_ADDR/api/metrics" >"$TMP/code" 2>"$TMP/curlerr" &
  local cpid=$!
  sample_window "$CHILD" 20 0.05
  wait "$cpid"; rc=$?
  [ "$rc" -eq 0 ] || { cat "$TMP/curlerr"; fail "curl to http://$LISTEN_ADDR/api/metrics failed (exit $rc); the server path was never exercised"; }
  [ "$(cat "$TMP/code")" = 200 ] || fail "/api/metrics returned HTTP $(cat "$TMP/code")"
  printf 'GET /api/metrics -> 200, %s bytes\n' "$(wc -c <"$curlout" | tr -d ' ')"

  local left=$(( (secs - 3) * 2 )); [ "$left" -lt 1 ] && left=1
  sample_window "$CHILD" "$left" 0.5

  alive "$CHILD" && kill -INT "$CHILD" 2>/dev/null
  wait "$CHILD"; rc=$?; CHILD=""
  printf '\n--- last 6 lines of ulpf serve ---\n'; tail -6 "$log"; printf -- '----------------------------------\n'
  case $rc in 0|130|2) ;; *) fail "serve exited $rc on SIGINT" ;; esac
  [ -s "$SOCKS" ] || fail "not one socket was observed although the server was listening and answered a request -- the sampler is not seeing this process"
  report 0.5
}

mode_docker() {
  local image=${1:?usage: isolation.sh docker <image> <file-or-dir>} input=${2:?usage: isolation.sh docker <image> <file-or-dir>}
  local log=$TMP/docker.log abs rc
  command -v docker >/dev/null || fail "docker not found"
  [ -e "$input" ] || fail "input not found: $input"
  abs=$(cd "$(dirname "$input")" && pwd)/$(basename "$input")
  printf 'docker run --rm --network none -v %s:/data/input:ro %s run /data/input\n' "$abs" "$image"
  docker run --rm --network none -v "$abs:/data/input:ro" "$image" \
    run /data/input --store /tmp/s --output /dev/null >"$log" 2>&1
  rc=$?
  printf '\n--- last 6 lines of container output ---\n'; tail -6 "$log"; printf -- '----------------------------------------\n'
  [ "$rc" -eq 0 ] || fail "container exited $rc under --network none"
  grep -q '^stages:' "$log" || fail "no counter block printed under --network none"
  printf '\nnetwork namespace: none. The container had no interface but lo, so no\noutbound connection could have succeeded, and the run completed anyway.\n'
  printf 'ISOLATION PASS\n'
}

# Guards against a vacuous pass: the classifier must reject what it should, and
# the sampler must be able to see a real process's sockets on this machine.
mode_selftest() {
  local bad=0 got exp rec pid lines
  while IFS='!' read -r rec exp; do
    [ -z "$rec" ] && continue
    IFS='|' read -r _p _l _r _s <<<"$rec"
    got=$(classify "$_p" "$_l" "$_r" "$_s")
    if [ "$got" != "$exp" ]; then printf 'classify %-46s -> %s, want %s\n' "$rec" "$got" "$exp"; bad=$((bad+1)); fi
  done <<'CASES'
TCP|127.0.0.1:7878|-|LISTEN!OK
TCP|[::1]:7878|-|LISTEN!OK
TCP|127.0.0.1:7878|127.0.0.1:51234|ESTABLISHED!OK
TCP|[::1]:7878|[::1]:51234|ESTABLISHED!OK
TCP|10.0.0.5:51234|93.184.216.34:443|ESTABLISHED!FAIL
TCP|10.0.0.5:51234|1.1.1.1:53|SYN_SENT!FAIL
UDP|10.0.0.5:57840|8.8.8.8:53|-!FAIL
UDP|*:53|-|-!FAIL
UDP|0.0.0.0:5353|-|UNCONN!FAIL
TCP|*:7878|-|LISTEN!FAIL
TCP|[::]:7878|-|LISTEN!FAIL
UDP|127.0.0.1:323|-|UNCONN!OK
TCP|127.0.0.1:51234|93.184.216.34:443|ESTABLISHED!FAIL
CASES
  [ "$bad" -eq 0 ] && printf 'classify: 13/13 cases correct\n'

  case $SAMPLER in
    lsof) pid=$(lsof -nP -i -F p 2>/dev/null | sed -n 's/^p//p' | head -1) ;;
    ss)   pid=$(ss -tunpa 2>/dev/null | sed -n 's/.*pid=\([0-9]*\),.*/\1/p' | head -1) ;;
    proc) pid=$(for d in /proc/[0-9]*; do ls -l "$d/fd" 2>/dev/null | grep -q 'socket:\[' && { echo "${d#/proc/}"; break; }; done) ;;
  esac
  if [ -z "$pid" ]; then
    printf 'sampler: no process on this machine holds a socket; cannot self-check the sampler\n'; bad=$((bad+1))
  else
    lines=$(sample "$pid" | grep -c '^\(TCP\|UDP\)|[^|]*|[^|]*|.' )
    printf 'sampler %s: parsed %s socket line(s) from pid %s\n' "$SAMPLER" "$lines" "$pid"
    [ "${lines:-0}" -ge 1 ] || { printf 'sampler parsed nothing for a pid that holds sockets\n'; bad=$((bad+1)); }
  fi

  [ "$bad" -eq 0 ] || fail "$bad selftest problem(s); do not trust a PASS from this script until they are fixed"
  printf 'SELFTEST PASS\n'
}

case ${1:-} in
  run)    shift; mode_run "$@" ;;
  serve)  shift; mode_serve "$@" ;;
  docker) shift; mode_docker "$@" ;;
  selftest) mode_selftest ;;
  *) printf 'usage: %s run <file-or-dir>\n       %s serve <watch-dir> [seconds]\n       %s docker <image> <file-or-dir>\n       %s selftest\n' "$0" "$0" "$0" "$0" >&2; exit 2 ;;
esac
