#!/usr/bin/env bash
# ULPF corpus generation — drives real traffic through the Squid proxy
# started by docker-compose.yml. Run from this directory after
# `docker compose up -d --build`. Requires: curl, nc (BSD or GNU netcat),
# xargs with -P (GNU or macOS/BSD both support -P).
#
# Usage: ./traffic.sh [TOTAL_REQUESTS] [PARALLELISM]
set -uo pipefail

PROXY="http://127.0.0.1:3128"
HOSTS=(origin www.example.test static.example.test api.example.test)
DENIED_HOSTS=(blocked.test denied.test adtracker.test)
HOT_PATHS=(/ /style.css /app.js /logo.png /files/report.txt /files/data.json)
# bigfile.bin is bigger than squid's maximum_object_size_in_memory (512 KB),
# so repeats of it land on disk (plain TCP_HIT) instead of TCP_MEM_HIT.
BIG_PATH=/files/bigfile.bin
DYNAMIC_PATHS=(/api/ /api/status /error500 /error404)
METHODS_WEIGHTED=(GET GET GET GET GET GET GET HEAD HEAD POST PUT DELETE OPTIONS)

TOTAL="${1:-8000}"
PAR="${2:-40}"
PLAN="$(mktemp)"

echo "== building request plan ($TOTAL requests) =="
for h in "${HOSTS[@]}"; do
  for p in "${HOT_PATHS[@]}"; do
    echo "GET http://${h}${p}" >>"$PLAN"
  done
  # prime + repeat the big object a few times for plain TCP_HIT (disk, not memory)
  echo "GET http://${h}${BIG_PATH}" >>"$PLAN"
  echo "GET http://${h}${BIG_PATH}" >>"$PLAN"
  echo "GET http://${h}${BIG_PATH}" >>"$PLAN"
done

for ((i = 0; i < TOTAL; i++)); do
  h="${HOSTS[$((RANDOM % ${#HOSTS[@]}))]}"
  roll=$((RANDOM % 10))
  if (( roll < 5 )); then
    p="${HOT_PATHS[$((RANDOM % ${#HOT_PATHS[@]}))]}"
  elif (( roll < 8 )); then
    p="${DYNAMIC_PATHS[$((RANDOM % ${#DYNAMIC_PATHS[@]}))]}"
  else
    p="/notfound/item-${i}-$RANDOM"
  fi
  m="${METHODS_WEIGHTED[$((RANDOM % ${#METHODS_WEIGHTED[@]}))]}"
  echo "$m http://${h}${p}" >>"$PLAN"
  if (( i % 37 == 0 )); then
    for _ in 1 2 3 4 5; do
      echo "GET http://${h}/logo.png" >>"$PLAN"
    done
  fi
done

for i in $(seq 1 300); do
  h="${DENIED_HOSTS[$((RANDOM % ${#DENIED_HOSTS[@]}))]}"
  echo "GET http://${h}/ad-$RANDOM.js" >>"$PLAN"
done

echo "== plan built: $(wc -l <"$PLAN") requests, firing with $PAR parallel workers =="
cat "$PLAN" | xargs -P "$PAR" -n2 sh -c \
  'curl -s -o /dev/null -m 5 -x "'"$PROXY"'" -X "$0" "$1"' 2>/dev/null
rm -f "$PLAN"

echo "== malformed requests direct to the proxy port (NONE_NONE) =="
for i in $(seq 1 15); do
  printf 'NOTAMETHOD ??? GARBAGE\r\n\r\n' | nc -w1 127.0.0.1 3128 >/dev/null 2>&1
  printf 'GET\r\n\r\n' | nc -w1 127.0.0.1 3128 >/dev/null 2>&1
done

echo "== conditional GETs (If-Modified-Since) for IMS_HIT variety =="
FUTURE_DATE=$(date -u -v+1d '+%a, %d %b %Y %H:%M:%S GMT' 2>/dev/null || date -u -d '+1 day' '+%a, %d %b %Y %H:%M:%S GMT')
for i in $(seq 1 50); do
  h="${HOSTS[$((RANDOM % ${#HOSTS[@]}))]}"
  curl -s -o /dev/null -m 5 -x "$PROXY" -H "If-Modified-Since: ${FUTURE_DATE}" \
    "http://${h}/logo.png" >/dev/null 2>&1
done

echo "== done =="
