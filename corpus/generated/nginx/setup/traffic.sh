#!/bin/sh
# Runs inside the "client" (nicolaka/netshoot) container.
# Drives varied HTTP/TLS/DNS/malformed traffic against haproxy + the two
# nginx backends so nginx access/error logs, haproxy httplog, and a zeek
# pcap all get real, non-trivial content.
set -u

PATHS="/ /ok /old /new /missing /boom /api/ /upload /slow /admin /nope/deep/path?x=1&y=2 /ok/../ok /%2e%2e/etc/passwd"
UAS="curl/8.4.0 Mozilla/5.0 (Windows NT 10.0; Win64; x64) Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15) python-requests/2.31 Go-http-client/1.1 sqlmap/1.7 - "
REFS="- https://example.com/ https://google.com/search https://internal.corp/dash -"
METHODS="GET GET GET GET POST PUT DELETE HEAD OPTIONS"

echo "== resolving names (dns.log material) =="
for h in nginx1 nginx2 haproxy sysloglistener nosuchhost.invalid; do
  dig +time=1 +tries=1 "$h" @127.0.0.11 >/dev/null 2>&1
done

echo "== main GET/mixed-method loop against haproxy =="
i=0
while [ $i -lt 500 ]; do
  p=$(echo $PATHS | tr ' ' '\n' | shuf -n1)
  ua=$(echo $UAS | tr ' ' '\n' | shuf -n1)
  ref=$(echo $REFS | tr ' ' '\n' | shuf -n1)
  m=$(echo $METHODS | tr ' ' '\n' | shuf -n1)
  curl -s -o /dev/null -m 3 -X "$m" -A "$ua" -e "$ref" "http://haproxy${p}" || true
  i=$((i+1))
done

echo "== direct-to-backend loop (nginx1/nginx2, varied) =="
i=0
while [ $i -lt 300 ]; do
  p=$(echo $PATHS | tr ' ' '\n' | shuf -n1)
  ua=$(echo $UAS | tr ' ' '\n' | shuf -n1)
  ref=$(echo $REFS | tr ' ' '\n' | shuf -n1)
  m=$(echo $METHODS | tr ' ' '\n' | shuf -n1)
  tgt=nginx1; [ $((i % 2)) -eq 0 ] && tgt=nginx2
  curl -s -o /dev/null -m 3 -X "$m" -A "$ua" -e "$ref" "http://${tgt}${p}" || true
  i=$((i+1))
done

echo "== TLS traffic direct + via haproxy tcp passthrough (ssl.log material) =="
i=0
while [ $i -lt 80 ]; do
  tgt=nginx1; [ $((i % 3)) -eq 0 ] && tgt=nginx2
  curl -sk -o /dev/null -m 3 "https://${tgt}/" || true
  curl -sk -o /dev/null -m 3 "https://haproxy/" || true
  i=$((i+1))
done

echo "== POST with bodies of varying size (client_max_body_size, upload path) =="
i=0
while [ $i -lt 60 ]; do
  sz=$((RANDOM % 4000 + 10))
  head -c "$sz" /dev/urandom | base64 | head -c "$sz" > /tmp/body.bin
  curl -s -o /dev/null -m 3 -X POST --data-binary @/tmp/body.bin -A "curl/8.4.0" "http://haproxy/upload" || true
  i=$((i+1))
done

echo "== malformed / edge-case requests (error.log material) =="
# 1) plain HTTP sent to the TLS port -> classic nginx error.log line
for tgt in nginx1 nginx2; do
  for n in 1 2 3 4 5; do
    curl -s -o /dev/null -m 2 "http://${tgt}:443/" || true
  done
done

# 2) request body over client_max_body_size (512k) -> 413 + error log entry
head -c 700000 /dev/urandom > /tmp/big.bin
for tgt in nginx1 nginx2; do
  curl -s -o /dev/null -m 5 -X POST --data-binary @/tmp/big.bin "http://${tgt}/upload" || true
done

# 3) oversized header line -> "client sent too long header line"
LONGVAL=$(head -c 20000 /dev/zero | tr '\0' 'A')
for tgt in nginx1 nginx2; do
  printf 'GET / HTTP/1.1\r\nHost: %s\r\nX-Long: %s\r\n\r\n' "$tgt" "$LONGVAL" | timeout 2 nc "$tgt" 80 >/dev/null 2>&1 || true
done

# 4) garbage/invalid request line -> "client sent invalid method"
for tgt in nginx1 nginx2 haproxy; do
  printf 'NOTAMETHOD ??? GARBAGE\r\n\r\n' | timeout 2 nc "$tgt" 80 >/dev/null 2>&1 || true
  printf '\x16\x03\x01\x00\xffGARBAGEBYTES\x00\x01\x02' | timeout 2 nc "$tgt" 80 >/dev/null 2>&1 || true
done

# 5) weird / oversized / bad Host header
for tgt in nginx1 nginx2; do
  curl -s -o /dev/null -m 2 -H "Host: [::badhost::%00%ff]" "http://${tgt}/" || true
  curl -s -o /dev/null -m 2 -H "Host: $(head -c 3000 /dev/zero | tr '\0' 'x')" "http://${tgt}/" || true
done

# 6) request with a raw non-UTF8 byte in the URI (weird bytes)
for tgt in nginx1 nginx2; do
  printf 'GET /\xfe\xff\x00bad HTTP/1.1\r\nHost: %s\r\n\r\n' "$tgt" | timeout 2 nc "$tgt" 80 >/dev/null 2>&1 || true
done

echo "== unresolvable-host DNS lookups (more dns.log variety) =="
for h in doesnotexist.invalid another.bad.invalid; do
  dig +time=1 +tries=1 "$h" @127.0.0.11 >/dev/null 2>&1 || true
done

echo "traffic.sh done"
