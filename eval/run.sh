#!/usr/bin/env bash
# Neutral eval harness. Tool-agnostic: everything it knows about a tool comes
# from the tool.toml passed as $1 (see docs/evaluation.md "how to add a tool").
# bash 3.2 safe (macOS ships no other bash) -- no associative arrays, no
# mapfile, no `timeout`(1); templating/timeouts/pid-tracking live in
# eval/lib/run_async.py instead, which also sidesteps the space in this
# repo's own path ("ssh hackathon").
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
LIBDIR="$ROOT/eval/lib"
# shellcheck source=eval/lib/sockets.sh
source "$LIBDIR/sockets.sh"

usage() { echo "usage: $0 <tool.toml> [--quick] [criterion ...]" >&2; exit 2; }

QUICK=0
TOOLFILE=""
CRITERIA=()
for a in "$@"; do
  case "$a" in
    --quick) QUICK=1 ;;
    -h|--help) usage ;;
    *) if [ -z "$TOOLFILE" ]; then TOOLFILE=$a; else CRITERIA+=("$a"); fi ;;
  esac
done
[ -n "$TOOLFILE" ] || usage
[ -f "$TOOLFILE" ] || { echo "no such tool file: $TOOLFILE" >&2; exit 2; }

ALL_CRITERIA="throughput correctness raw_preservation unknown_format damaged_inputs isolation container cold_start memory kill_recovery"
if [ "${#CRITERIA[@]}" -eq 0 ]; then
  # shellcheck disable=SC2206
  CRITERIA=($ALL_CRITERIA)
fi

eval "$(python3 "$LIBDIR/cfg.py" "$TOOLFILE")"
TOOLNAME=$CFG_name

TS=$(date -u +%Y%m%dT%H%M%SZ)
# $$ guards against two invocations landing in the same results dir when they
# start in the same wall-clock second (date has only 1s resolution) -- hit
# during development by running two criteria sets concurrently; without it
# their scorecard.md writes interleave into one file.
RESULTS="$ROOT/eval/results/${TOOLNAME}-${TS}-$$"
RAW="$RESULTS/raw"
mkdir -p "$RAW"
SCORECARD="$RESULTS/scorecard.md"
: > "$SCORECARD"

log() { printf '%s\n' "$*" | tee -a "$SCORECARD"; }
render_cmd() { python3 "$LIBDIR/render_cmd.py" "$@"; }

THREADS=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
[ "$THREADS" -gt 1 ] 2>/dev/null && THREADS=$((THREADS - 1))

BENCH_FILE="$ROOT/bench/mixed-5000000.log"
QUICK_BENCH_FILE=${ULPF_EVAL_QUICK_BENCH:-${TMPDIR:-/tmp}/ulpf-eval-quick-bench.log}

ensure_bench_file() {
  [ -f "$BENCH_FILE" ] && return 0
  log "bench file missing, regenerating per bench/README.md (~25s)..."
  local td=${EVAL_TARGET_DIR:-$RESULTS/.cargo-target}
  local rc=0
  ( cd "$ROOT" && CARGO_TARGET_DIR="$td" cargo run --profile dist -p ulpf --example gen_bench -- 5000000 bench \
      >"$RAW/gen_bench.stdout" 2>"$RAW/gen_bench.stderr" ) || rc=$?
  if [ "$rc" -ne 0 ] || [ ! -f "$BENCH_FILE" ]; then
    log "not measurable: bench file absent and regeneration failed (exit $rc); see raw/gen_bench.stderr"
    return 1
  fi
  return 0
}

ensure_quick_bench() {
  [ -f "$QUICK_BENCH_FILE" ] && return 0
  ensure_bench_file || return 1
  head -n 500000 "$BENCH_FILE" > "$QUICK_BENCH_FILE"
  return 0
}

# Resolve the tool binary: EVAL_TOOL_BIN overrides (e.g. a prebuilt baseline the
# lead's own build must not collide with); otherwise build once into a scratch
# CARGO_TARGET_DIR, never into the repo's own target/.
resolve_bin() {
  if [ -n "${EVAL_TOOL_BIN:-}" ]; then
    BIN=$EVAL_TOOL_BIN
    log "tool binary: $BIN (EVAL_TOOL_BIN override, not built by this run)"
    return
  fi
  local td=${EVAL_TARGET_DIR:-$RESULTS/.cargo-target}
  BIN=$(render_cmd "$CFG_build_binary" target_dir="$td")
  if [ ! -x "$BIN" ]; then
    local cmd; cmd=$(render_cmd "$CFG_build_cmd" target_dir="$td")
    log "building: $cmd"
    local rc=0
    ( cd "$ROOT" && eval "$cmd" >"$RAW/build.stdout" 2>"$RAW/build.stderr" ) || rc=$?
    [ "$rc" -eq 0 ] || { log "BUILD FAILED (exit $rc); see raw/build.stderr"; exit 1; }
  fi
  log "tool binary: $BIN"
}

# ---- shared subprocess helpers --------------------------------------------
# tool_run LABEL TEMPLATE_VAR TIMEOUT k=v... -- synchronous; sets RC WALL NOTE EVENTS.
# The template is looked up from the named CFG_* variable so callers can point
# it at run/verify/raw_of/check interchangeably.
tool_run() {
  local label=$1 tmplvar=$2 timeout=$3; shift 3
  local tmpl; eval "tmpl=\$$tmplvar"
  local cmd; cmd=$(render_cmd "$tmpl" "$@")
  echo "$cmd" > "$RAW/$label.cmd"
  local setargs=(); local kv
  for kv in "$@"; do setargs+=(--set "$kv"); done
  python3 "$LIBDIR/run_async.py" --template "$tmpl" --timeout "$timeout" \
    --out "$RAW/$label.stdout" --err "$RAW/$label.stderr" --exitfile "$RAW/$label.exit" "${setargs[@]}"
  IFS='|' read -r RC WALL NOTE < "$RAW/$label.exit"
}

# events_in FILE -> line count, 0 if the file is missing/empty.
events_in() { [ -f "$1" ] && wc -l < "$1" | tr -d ' ' || echo 0; }
last_err_line() { tail -n1 "$RAW/$1.stderr" 2>/dev/null || true; }

# tool_run_start LABEL TEMPLATE_VAR k=v... -- backgrounds the tool, waits for
# its real pid (not this wrapper's) to appear. Sets ASYNC_PID, ASYNC_JOB.
tool_run_start() {
  local label=$1 tmplvar=$2; shift 2
  local tmpl; eval "tmpl=\$$tmplvar"
  local cmd; cmd=$(render_cmd "$tmpl" "$@")
  echo "$cmd" > "$RAW/$label.cmd"
  rm -f "$RAW/$label.pid" "$RAW/$label.exit"
  local setargs=(); local kv
  for kv in "$@"; do setargs+=(--set "$kv"); done
  python3 "$LIBDIR/run_async.py" --template "$tmpl" \
    --out "$RAW/$label.stdout" --err "$RAW/$label.stderr" --exitfile "$RAW/$label.exit" \
    --pidfile "$RAW/$label.pid" "${setargs[@]}" &
  ASYNC_JOB=$!
  local tries=0
  while [ ! -s "$RAW/$label.pid" ] && [ "$tries" -lt 200 ]; do sleep 0.05; tries=$((tries + 1)); done
  ASYNC_PID=$(cat "$RAW/$label.pid" 2>/dev/null || echo "")
}
tool_run_wait() {
  local label=$1
  wait "$ASYNC_JOB" 2>/dev/null || true
  IFS='|' read -r RC WALL NOTE < "$RAW/$label.exit"
}

have_tmpl() { local v; eval "v=\${$1:-}"; [ -n "$v" ]; }

# =========================== criteria =======================================

crit_throughput() {
  log ""
  log "## throughput"
  local file label
  if [ "$QUICK" -eq 1 ]; then
    ensure_quick_bench || { log "not measurable: quick bench slice unavailable"; return 0; }
    file=$QUICK_BENCH_FILE; label="QUICK MODE: 500,000-line slice of bench/mixed-5000000.log"
  else
    ensure_bench_file || { log "not measurable: bench/mixed-5000000.log unavailable"; return 0; }
    file=$BENCH_FILE; label="bench/mixed-5000000.log"
  fi
  local bytes; bytes=$(wc -c < "$file" | tr -d ' ')
  log "input: $label ($bytes bytes)"
  local i eps_list=""
  for i in 1 2 3; do
    local store="$RAW/throughput-$i.store" out="$RAW/throughput-$i.out.jsonl"
    rm -rf "$store"; : > "$out"
    tool_run "throughput-$i" CFG_templates_run 600 \
      bin="$BIN" input="$file" store="$store" output="$out" \
      parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/throughput-$i.pending" threads="$THREADS"
    local events; events=$(events_in "$out")
    local eps mbs
    eps=$(awk -v e="$events" -v w="$WALL" 'BEGIN{ if (w>0) printf "%.0f", e/w; else print 0 }')
    mbs=$(awk -v b="$bytes" -v w="$WALL" 'BEGIN{ if (w>0) printf "%.1f", (b/1048576)/w; else print 0 }')
    log "run $i: exit=$RC wall=${WALL}s events=$events events/s=$eps MB/s=$mbs (cmd: raw/throughput-$i.cmd)"
    eps_list="$eps_list$eps "
  done
  local median; median=$(printf '%s\n' $eps_list | sort -n | awk '{a[NR]=$1} END{ if (NR==0) print 0; else if (NR%2==1) print a[(NR+1)/2]; else print int((a[int(NR/2)]+a[int(NR/2)+1])/2) }')
  log "median events/s across 3 runs: $median"
  if [ "$QUICK" -eq 1 ]; then
    log "NOTE: quick mode, under load; the lead re-runs on a quiet machine."
  fi
}

crit_correctness() {
  log ""
  log "## correctness"
  if ! have_tmpl CFG_correctness_key_map; then
    log "not measurable: tool config has no [correctness].key_map"
    return 0
  fi
  local keymap="$ROOT/${CFG_correctness_key_map#$ROOT/}"
  [ -f "$keymap" ] || keymap="$CFG_correctness_key_map"
  [ -f "$keymap" ] || { log "not measurable: key_map file not found: $CFG_correctness_key_map"; return 0; }
  local tot_total=0 tot_matched=0 tot_mismatched=0 tot_missing=0
  local f base fixture store out
  for f in "$ROOT"/samples/*.log; do
    base=$(basename "$f" .log)
    fixture="$ROOT/fixtures/$base.expected.jsonl"
    [ -f "$fixture" ] || continue
    store="$RAW/correctness-$base.store"; out="$RAW/correctness-$base.out.jsonl"
    rm -rf "$store"; : > "$out"
    tool_run "correctness-$base" CFG_templates_run 120 \
      bin="$BIN" input="$f" store="$store" output="$out" \
      parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/correctness-$base.pending" threads="$THREADS"
    local summary; summary=$(python3 "$LIBDIR/jsoncmp.py" "$fixture" "$out" "$keymap" 2>"$RAW/correctness-$base.diff")
    log "$base: $summary (exit=$RC, cmd: raw/correctness-$base.cmd, diff: raw/correctness-$base.diff)"
    local t m mm mi
    t=$(echo "$summary" | sed -n 's/.*total=\([0-9]*\).*/\1/p')
    m=$(echo "$summary" | sed -n 's/.* matched=\([0-9]*\).*/\1/p')
    mm=$(echo "$summary" | sed -n 's/.*mismatched=\([0-9]*\).*/\1/p')
    mi=$(echo "$summary" | sed -n 's/.*missing=\([0-9]*\).*/\1/p')
    tot_total=$((tot_total + ${t:-0})); tot_matched=$((tot_matched + ${m:-0}))
    tot_mismatched=$((tot_mismatched + ${mm:-0})); tot_missing=$((tot_missing + ${mi:-0}))
  done
  local pct; pct=$(awk -v m="$tot_matched" -v t="$tot_total" 'BEGIN{ if (t>0) printf "%.1f", m*100/t; else print "0.0" }')
  log "TOTAL: events=$tot_total matched=$tot_matched mismatched=$tot_mismatched missing=$tot_missing pct=$pct%"
  log "scope: only fixture keys observable in a tool's *output* are checked (normalized/time/time_policies/parser/status/sub); 'fields'/'absent' describe the vendor-native parse stage, out of scope for a black-box harness -- see fixtures/README.md and docs/evaluation.md."
}

crit_raw_preservation() {
  log ""
  log "## raw_preservation"
  if ! have_tmpl CFG_templates_raw_of; then
    log "not measurable: tool config has no [templates].raw_of"
    return 0
  fi
  local f="$ROOT/samples/cisco_asa.log" store="$RAW/rawpres.store" out="$RAW/rawpres.out.jsonl"
  rm -rf "$store"; : > "$out"
  tool_run rawpres CFG_templates_run 60 \
    bin="$BIN" input="$f" store="$store" output="$out" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/rawpres.pending" threads="$THREADS"
  if [ "$RC" != "0" ]; then
    log "not measurable: setup run exited $RC (cmd: raw/rawpres.cmd)"
    return 0
  fi
  if have_tmpl CFG_templates_verify; then
    tool_run rawpres-verify CFG_templates_verify 60 bin="$BIN" store="$store"
    log "verify: $(cat "$RAW/rawpres-verify.stdout" 2>/dev/null) (exit=$RC, cmd: raw/rawpres-verify.cmd)"
  else
    log "verify: not measurable (no [templates].verify)"
  fi
  local n; n=$(python3 "$LIBDIR/frame.py" "$f" --count)
  local sample=$((n < 20 ? n : 20))
  local i verified=0 mismatches=0
  for i in $(seq 0 $((sample - 1))); do
    tool_run "rawpres-raw-$i" CFG_templates_raw_of 20 bin="$BIN" store="$store" id="$i"
    python3 "$LIBDIR/frame.py" "$f" --get "$i" > "$RAW/rawpres-frame-$i.bin" 2>/dev/null || true
    if [ "$RC" = "0" ] && cmp -s "$RAW/rawpres-raw-$i.stdout" "$RAW/rawpres-frame-$i.bin"; then
      verified=$((verified + 1))
    else
      mismatches=$((mismatches + 1))
    fi
  done
  log "records verified: $verified/$sample sampled ids (mismatches: $mismatches); framing rule: docs/evaluation.md#raw_preservation"
}

crit_unknown_format() {
  log ""
  log "## unknown_format"
  local f="$ROOT/heldout/mikrotik.log"
  if [ ! -f "$f" ]; then log "not measurable: $f not found"; return 0; fi
  local store="$RAW/unknown.store" out="$RAW/unknown.out.jsonl" pending="$RAW/unknown.pending"
  rm -rf "$store" "$pending"; : > "$out"
  tool_run unknown CFG_templates_run 60 \
    bin="$BIN" input="$f" store="$store" output="$out" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$pending" threads="$THREADS"
  local events; events=$(events_in "$out")
  local proposals=0
  [ -d "$pending" ] && proposals=$(find "$pending" -maxdepth 1 -type f -name '*.toml' 2>/dev/null | wc -l | tr -d ' ')
  log "exit=$RC events_emitted=$events proposals_written=$proposals (in $pending) (cmd: raw/unknown.cmd)"
}

crit_damaged_inputs() {
  log ""
  log "## damaged_inputs"
  if [ ! -d "$ROOT/eval/damaged" ] || [ -z "$(find "$ROOT/eval/damaged" \( -type f -o -type l \) 2>/dev/null)" ]; then
    log "generating damaged inputs: bash eval/make_damaged.sh"
    bash "$ROOT/eval/make_damaged.sh" >"$RAW/make_damaged.stdout" 2>"$RAW/make_damaged.stderr" || true
  fi
  local f name store out
  while IFS= read -r f; do
    name=$(basename "$f")
    store="$RAW/damaged-$name.store"; out="$RAW/damaged-$name.out.jsonl"
    rm -rf "$store"; : > "$out"
    tool_run "damaged-$name" CFG_templates_run 60 \
      bin="$BIN" input="$f" store="$store" output="$out" \
      parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/damaged-$name.pending" threads="$THREADS"
    local events crashed hung
    events=$(events_in "$out")
    hung="no"; [ "$NOTE" = "timeout" ] && hung="yes (60s)"
    crashed="no"; case "$RC" in -*) crashed="yes (signal ${RC#-})" ;; esac
    log "$name: exit=$RC events=$events crashed=$crashed hung=$hung stderr_last='$(last_err_line "damaged-$name")'"
  done < <(find "$ROOT/eval/damaged" \( -type f -o -type l \) | sort)
}

crit_isolation() {
  log ""
  log "## isolation"
  if [ "$SOCKETS_SAMPLER" = "none" ]; then
    log "not measurable: no socket sampler for $(uname -s)"
    return 0
  fi
  local file
  if [ "$QUICK" -eq 1 ]; then ensure_quick_bench && file=$QUICK_BENCH_FILE || file="$ROOT/samples"; else file="$ROOT/samples"; fi
  local store="$RAW/isolation.store" out="$RAW/isolation.out.jsonl"
  rm -rf "$store"; : > "$out"
  tool_run_start isolation CFG_templates_run \
    bin="$BIN" input="$file" store="$store" output="$out" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/isolation.pending" threads="$THREADS"
  if [ -z "$ASYNC_PID" ]; then
    log "not measurable: process never reached a sampleable pid"
    wait "$ASYNC_JOB" 2>/dev/null || true
    return 0
  fi
  local socksfile="$RAW/isolation.sockets"
  : > "$socksfile"
  sockets_watch "$ASYNC_PID" 60 0.1 "$socksfile"
  tool_run_wait isolation
  log "sampled $(wc -l < "$socksfile" | tr -d ' ') socket observation(s) over the run (pid $ASYNC_PID, sampler $SOCKETS_SAMPLER)"
  if sockets_report "$socksfile" | tee -a "$SCORECARD"; then
    log "ISOLATION: PASS"
  else
    log "ISOLATION: FAIL"
  fi
  log "(exit=$RC, cmd: raw/isolation.cmd; scripts/isolation.sh gives ULPF's own second opinion via ULPF_BIN=$BIN)"
}

crit_container() {
  log ""
  log "## container"
  if ! have_tmpl CFG_container_build || ! have_tmpl CFG_container_run; then
    log "not measurable: tool config has no [container] build/run templates"
    return 0
  fi
  if ! command -v docker >/dev/null 2>&1; then
    log "not measurable: docker not found"
    return 0
  fi
  local image=$CFG_container_image
  tool_run container-build CFG_container_build 600 \
    image="$image" dockerfile="$ROOT/Dockerfile" context="$ROOT"
  if [ "$RC" != "0" ]; then
    log "not measurable: image build failed/timed out (exit=$RC, note=$NOTE); see raw/container-build.stderr"
    return 0
  fi
  local size; size=$(docker image inspect --format '{{.Size}}' "$image" 2>/dev/null || echo unknown)
  local outdir="$RAW/container-out"; mkdir -p "$outdir"; rm -f "$outdir/out.jsonl"
  tool_run container-run CFG_container_run 120 \
    image="$image" input_dir="$ROOT/samples" output_dir="$outdir"
  local events; events=$(events_in "$outdir/out.jsonl")
  log "image=$image size_bytes=$size build_exit=0 run_exit=$RC events_emitted=$events (cmds: raw/container-build.cmd, raw/container-run.cmd)"
}

crit_cold_start() {
  log ""
  log "## cold_start"
  if ! have_tmpl CFG_cold_start_readme; then
    log "not measurable: tool config has no [cold_start].readme"
    return 0
  fi
  local readme="$ROOT/$CFG_cold_start_readme"
  [ -f "$readme" ] || { log "FAIL: $CFG_cold_start_readme not found in repo -- missing install doc"; return 0; }
  local clone="$RAW/cold_start.clone"
  rm -rf "$clone"
  local t0 t1
  t0=$(python3 -c 'import time;print(time.time())')
  git clone --quiet "$ROOT" "$clone" >"$RAW/cold_start.clone.log" 2>&1 || { log "FAIL: git clone of repo HEAD failed"; return 0; }
  local commands="$RAW/cold_start.commands.txt"
  python3 "$LIBDIR/extract_fence.py" "$readme" "$CFG_cold_start_heading" > "$commands"
  if [ ! -s "$commands" ]; then
    log "FAIL: no fenced code block found under '$CFG_cold_start_heading' in $CFG_cold_start_readme (missing step: the quick-start command block itself)"
    return 0
  fi
  local ok=1 line n=0 first_fail=""
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    case "$line" in \#*) continue ;; esac
    if [ -n "$CFG_cold_start_stop_at_substring" ]; then
      case "$line" in *"$CFG_cold_start_stop_at_substring"*) log "stopping before long-running command: $line"; break ;; esac
    fi
    n=$((n + 1))
    log "\$ $line"
    local rc=0
    ( cd "$clone" && eval "$line" >"$RAW/cold_start.$n.stdout" 2>"$RAW/cold_start.$n.stderr" ) || rc=$?
    if [ "$rc" -ne 0 ]; then
      ok=0; first_fail=${first_fail:-"$line (exit $rc)"}
      log "  -> exit $rc (see raw/cold_start.$n.stderr)"
    fi
  done < "$commands"
  t1=$(python3 -c 'import time;print(time.time())')
  local wall; wall=$(awk -v a="$t0" -v b="$t1" 'BEGIN{printf "%.1f", b-a}')
  log "commands run: $n, total wall time: ${wall}s"
  if [ "$ok" -eq 1 ]; then
    log "COLD START: PASS"
  else
    log "COLD START: FAIL at: $first_fail"
  fi
  rm -rf "$clone/target"  # build artifacts are reproducible from the logged commands; no need to keep GBs of them
}

crit_memory() {
  log ""
  log "## memory"
  local file
  if [ "$QUICK" -eq 1 ]; then ensure_quick_bench && file=$QUICK_BENCH_FILE || { log "not measurable: no bench input"; return 0; }
  else ensure_bench_file && file=$BENCH_FILE || { log "not measurable: no bench input"; return 0; }
  fi
  local store="$RAW/memory.store" out="$RAW/memory.out.jsonl" series="$RAW/memory-rss.tsv"
  rm -rf "$store"; : > "$out"; : > "$series"
  tool_run_start memory CFG_templates_run \
    bin="$BIN" input="$file" store="$store" output="$out" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/memory.pending" threads="$THREADS"
  if [ -z "$ASYNC_PID" ]; then log "not measurable: process never reached a sampleable pid"; wait "$ASYNC_JOB" 2>/dev/null || true; return 0; fi
  local i=0 rss
  while kill -0 "$ASYNC_PID" 2>/dev/null; do
    rss=$(ps -o rss= -p "$ASYNC_PID" 2>/dev/null | tr -d ' ')
    [ -n "$rss" ] && printf '%s\t%s\n' "$i" "$rss" >> "$series"
    i=$((i + 1))
    sleep 0.5
  done
  tool_run_wait memory
  local peak; peak=$(awk -F'\t' 'BEGIN{p=0} {if ($2>p) p=$2} END{print p}' "$series")
  local slope="n/a"
  local nlines; nlines=$(wc -l < "$series" | tr -d ' ')
  if [ "${nlines:-0}" -ge 2 ]; then
    local win=120  # 120 samples * 0.5s = last minute
    slope=$(tail -n "$win" "$series" | awk -F'\t' '
      NR==1{t0=$1; r0=$2} {t1=$1; r1=$2}
      END{ dt=(t1-t0)*0.5; if (dt>0) printf "%.1f KB/s", (r1-r0)/dt; else print "n/a (too short)" }')
  fi
  log "peak RSS: ${peak} KB; slope over last minute (or full run if shorter): $slope; series: raw/memory-rss.tsv (exit=$RC)"
}

crit_kill_recovery() {
  log ""
  log "## kill_recovery"
  local file
  if [ "$QUICK" -eq 1 ]; then ensure_quick_bench && file=$QUICK_BENCH_FILE || { log "not measurable: no bench input"; return 0; }
  else ensure_bench_file && file=$BENCH_FILE || { log "not measurable: no bench input"; return 0; }
  fi

  local bstore="$RAW/kr-baseline.store" bout="$RAW/kr-baseline.out.jsonl"
  rm -rf "$bstore"; : > "$bout"
  tool_run kr-baseline CFG_templates_run 600 \
    bin="$BIN" input="$file" store="$bstore" output="$bout" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/kr-baseline.pending" threads="$THREADS"
  local baseline_events; baseline_events=$(events_in "$bout")
  log "run-to-completion baseline: exit=$RC events=$baseline_events (cmd: raw/kr-baseline.cmd)"

  local kstore="$RAW/kr-kill.store" kout="$RAW/kr-kill.out.jsonl"
  rm -rf "$kstore"; : > "$kout"
  tool_run_start kr-kill CFG_templates_run \
    bin="$BIN" input="$file" store="$kstore" output="$kout" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/kr-kill.pending" threads="$THREADS"
  if [ -z "$ASYNC_PID" ]; then log "not measurable: process never reached a sampleable pid"; wait "$ASYNC_JOB" 2>/dev/null || true; return 0; fi
  sleep 5
  if ! kill -0 "$ASYNC_PID" 2>/dev/null; then
    tool_run_wait kr-kill
    log "not measurable: the run finished before the 5s kill point (input too small/fast under --quick; rerun on the full bench file)"
    return 0
  fi
  kill -9 "$ASYNC_PID" 2>/dev/null || true
  tool_run_wait kr-kill
  local partial_events; partial_events=$(events_in "$kout")
  log "killed after 5s: partial events=$partial_events (note=$NOTE)"

  if have_tmpl CFG_templates_verify; then
    tool_run kr-verify CFG_templates_verify 60 bin="$BIN" store="$kstore"
    log "verify after kill: $(cat "$RAW/kr-verify.stdout" 2>/dev/null) (exit=$RC)"
  else
    log "verify after kill: not measurable (no [templates].verify)"
  fi

  tool_run kr-resume CFG_templates_run 600 \
    bin="$BIN" input="$file" store="$kstore" output="$kout" \
    parsers="$ROOT/parsers" mappings="$ROOT/mappings" pending="$RAW/kr-kill.pending" threads="$THREADS"
  local final_events; final_events=$(events_in "$kout")
  local verdict="consistent"
  if [ "$final_events" -gt "$baseline_events" ]; then verdict="DOUBLE-COUNTED ($final_events > $baseline_events)"; fi
  if [ "$final_events" -lt "$baseline_events" ]; then verdict="LOST INPUT ($final_events < $baseline_events)"; fi
  log "restart: exit=$RC final_events=$final_events vs baseline=$baseline_events -> $verdict (cmd: raw/kr-resume.cmd)"
  [ "$RC" = "0" ] && log "restarts cleanly: yes" || log "restarts cleanly: no (exit $RC)"
}

# =========================== main ===========================================

resolve_bin
log "# scorecard: $TOOLNAME ($TS)"
log "tool config: $TOOLFILE"
log "quick mode: $([ "$QUICK" -eq 1 ] && echo yes || echo no)"
log "build declared by $TOOLFILE: $CFG_build_cmd"
log "threads: $THREADS"

HARNESS_BROKE=0
for c in "${CRITERIA[@]}"; do
  case " $ALL_CRITERIA " in
    *" $c "*) ;;
    *) log "unknown criterion: $c"; HARNESS_BROKE=1; continue ;;
  esac
  rc=0
  ( set +e; "crit_$c" ) || rc=$?
  if [ "$rc" -ne 0 ]; then
    log "HARNESS ERROR while running criterion '$c' (exit $rc) -- this is a bug in run.sh, not a tool result"
    HARNESS_BROKE=1
  fi
done

# A quiet-machine 04:00 run over the full 5M-line bench file produces one
# *.store dir and one *.out.jsonl per run; at 1.5 GB of input that is several
# GB of scratch per criterion, and every measurement it holds (counts,
# verify/raw_of results, diffs) is already in scorecard.md/raw/*.{cmd,exit}.
# Keep a head sample for spot-checking and the reproduction command; drop the
# rest. ponytail: a fixed 20 MB/500-line cutoff, not a config knob -- raise it
# if a future criterion needs to keep a full large artifact around.
prune_large_raw() {
  local f size
  find "$RAW" -maxdepth 1 -type d -name '*.store' 2>/dev/null | while IFS= read -r f; do
    size=$(du -sk "$f" 2>/dev/null | cut -f1)
    if [ "${size:-0}" -gt 20000 ]; then
      rm -rf "$f"
      echo "pruned (was ${size}KB); reproduce with: $(cat "${f%.store}.cmd" 2>/dev/null)" > "$f.pruned"
    fi
  done
  find "$RAW" -maxdepth 1 -type f -name '*.out.jsonl' 2>/dev/null | while IFS= read -r f; do
    size=$(du -sk "$f" 2>/dev/null | cut -f1)
    if [ "${size:-0}" -gt 20000 ]; then
      head -n 500 "$f" > "$f.sample"
      echo "$f pruned from ${size}KB to a 500-line .sample; reproduce with: $(cat "${f%.out.jsonl}.cmd" 2>/dev/null)" >> "$f.sample"
      rm -f "$f"
    fi
  done
}
prune_large_raw

log ""
log "raw output and exact commands: $RAW"
echo "scorecard: $SCORECARD" >&2
exit "$HARNESS_BROKE"
