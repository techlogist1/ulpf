#!/usr/bin/env bash
# Generic per-pid socket sampler + loopback classifier, for the isolation criterion.
# Sampling/classification logic is the same shape as scripts/isolation.sh (lsof on
# macOS, ss on Linux) but stripped of its ULPF-specific run/serve/docker orchestration
# so run.sh can point it at any tool's already-running pid. Source, don't execute.

case $(uname -s) in
  Darwin) SOCKETS_SAMPLER=lsof ;;
  Linux)  if command -v ss >/dev/null 2>&1; then SOCKETS_SAMPLER=ss; else SOCKETS_SAMPLER=proc; fi ;;
  *) SOCKETS_SAMPLER=none ;;
esac

# sockets_sample PID -> zero or more "PROTO|LOCAL|REMOTE|STATE" lines, or nothing
# if the pid held no sockets (a passing, not a failing, result).
sockets_sample() {
  local pid=$1 out
  case $SOCKETS_SAMPLER in
    lsof)
      out=$(lsof -nP -a -p "$pid" 2>/dev/null) || return 0
      printf '%s\n' "$out" | awk '
        $5=="IPv4" || $5=="IPv6" {
          proto=$8; name=$9; s=$10; gsub(/[()]/,"",s); if (s=="") s="-"
          i=index(name,"->")
          if (i>0) { loc=substr(name,1,i-1); rem=substr(name,i+2) } else { loc=name; rem="-" }
          print proto "|" loc "|" rem "|" s }'
      ;;
    ss)
      out=$(ss -tunpa 2>/dev/null) || return 0
      printf '%s\n' "$out" | awk -v pid="$pid" '
        index($0, "pid=" pid ",") {
          proto=toupper($1); s=$2; loc=$5; rem=$6
          if (rem ~ /:\*$/ || rem ~ /:0$/) rem="-"
          print proto "|" loc "|" rem "|" s }'
      ;;
    proc)
      # No ss on this Linux box: not implemented (rare in practice). Reported by
      # the caller as not-measurable rather than silently sampling nothing.
      return 1
      ;;
    *) return 1 ;;
  esac
}

host_of() { local a=${1%:*}; a=${a#"["}; a=${a%"]"}; printf '%s' "$a"; }
is_loopback_host() {
  case "$(host_of "$1")" in
    127.*|::1|0:0:0:0:0:0:0:1|localhost) return 0 ;;
  esac
  return 1
}

# sockets_classify PROTO LOCAL REMOTE STATE -> prints OK or FAIL
sockets_classify() {
  local remote=$3 local_=$2
  if [ "$remote" != "-" ] && [ "$remote" != "*:*" ]; then
    is_loopback_host "$remote" && { echo OK; return; }
    echo FAIL; return
  fi
  is_loopback_host "$local_" && { echo OK; return; }
  echo FAIL
}

# sockets_watch PID ITERATIONS INTERVAL OUTFILE -- appends samples until the
# pid exits or ITERATIONS samples have been taken. Counted in iterations, not
# wall time, so no float arithmetic is needed for bash 3.2.
sockets_watch() {
  local pid=$1 iters=$2 interval=$3 outfile=$4 i
  for i in $(seq 1 "$iters"); do
    kill -0 "$pid" 2>/dev/null || break
    sockets_sample "$pid" >>"$outfile"
    kill -0 "$pid" 2>/dev/null || break
    sleep "$interval"
  done
}

# sockets_report INFILE -- prints a table, returns 1 if any non-loopback socket
# was seen. An empty INFILE (no socket ever observed) is a pass, printed plainly.
sockets_report() {
  local infile=$1 nfail=0 proto loc rem st v
  if [ ! -s "$infile" ]; then
    echo "(no socket observed)"
    return 0
  fi
  printf '%-5s %-28s %-28s %-12s %s\n' PROTO LOCAL REMOTE STATE VERDICT
  while IFS='|' read -r proto loc rem st; do
    [ -z "$proto" ] && continue
    v=$(sockets_classify "$proto" "$loc" "$rem" "$st")
    printf '%-5s %-28s %-28s %-12s %s\n' "$proto" "$loc" "$rem" "$st" "$v"
    [ "$v" = FAIL ] && nfail=$((nfail + 1))
  done < <(sort -u "$infile")
  [ "$nfail" -eq 0 ]
}
