#!/bin/bash
# End-to-end reproduction, two phases against the same PKI and the same
# traffic script:
#   phase A  server logs natively (--log-append, ctime prefix)  -> server.log
#   phase B  server logs via --syslog, rsyslog in the same
#            container writes RFC 3164                          -> server-syslog.log
# Builds the image, generates the PKI, drives real clients (1 good in a
# reconnect loop + 3 failing variants), copies the logs out byte-for-byte,
# tears everything down. Defaults finish in under 5 minutes.
# Usage: ./run-demo.sh [A_CYCLES] [A_DWELL] [B_CYCLES] [B_DWELL]
set -euo pipefail
cd "$(dirname "$0")"
IMG=ulpf-openvpn:local
IMG25=ulpf-openvpn25:local
IMG24=ulpf-openvpn24:local
NET=ulpf-vpn-net
RUN="$(pwd)/run"
A_CYCLES=${1:-30}; A_DWELL=${2:-30}
B_CYCLES=${3:-20}; B_DWELL=${4:-20}
C_CYCLES=${5:-12}; C_DWELL=${6:-12}
D_CYCLES=${7:-12}; D_DWELL=${8:-12}

echo "== build images =="
docker build -t "$IMG" .
docker build -f Dockerfile.openvpn25 -t "$IMG25" .
docker build -f Dockerfile.openvpn24 -t "$IMG24" .

echo "== generate PKI (real easy-rsa 3 inside the image) =="
[ -d pki-out/server ] || ./gen-pki.sh

echo "== lay out run/ (bind-mount targets) =="
rm -rf "$RUN"
mkdir -p "$RUN/server/logs-native" "$RUN/server/logs-syslog" "$RUN/server/certs"
cp pki-out/server/* "$RUN/server/certs/"
cp server.conf server-syslog.conf rsyslog.conf server-syslog-entry.sh "$RUN/server/"
for c in good badcert badtls badport; do
  mkdir -p "$RUN/clients/$c/certs"
  cp pki-out/clients/$c/* "$RUN/clients/$c/certs/"
  cp "client-$c.conf" "$RUN/clients/$c/client.conf"
done

docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET"
teardown() {
  docker rm -f ulpf-ovpn-server ulpf-ovpn-client-good ulpf-ovpn-client-badcert \
                ulpf-ovpn-client-badtls ulpf-ovpn-client-badport >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
}
trap teardown EXIT

echo "== phase A: server with --log-append (native ctime format) =="
docker rm -f ulpf-ovpn-server >/dev/null 2>&1 || true
docker run -d --name ulpf-ovpn-server --network "$NET" \
  --cap-add NET_ADMIN --device /dev/net/tun \
  -v "$RUN/server/certs:/etc/openvpn/certs:ro" \
  -v "$RUN/server/server.conf:/etc/openvpn/server.conf:ro" \
  -v "$RUN/server/logs-native:/var/log/openvpn" \
  "$IMG" openvpn --config /etc/openvpn/server.conf >/dev/null
sleep 2
docker exec ulpf-ovpn-server openvpn --version 2>&1 | head -1 || true
./run-traffic.sh "$A_CYCLES" "$A_DWELL" native
docker stop -t 3 ulpf-ovpn-server >/dev/null
mkdir -p out
docker cp ulpf-ovpn-server:/var/log/openvpn/server.log out/server.log
docker rm -f ulpf-ovpn-server >/dev/null

echo "== phase B: server with --syslog + rsyslog (RFC 3164 framing) =="
docker run -d --name ulpf-ovpn-server --network "$NET" \
  --cap-add NET_ADMIN --device /dev/net/tun \
  -v "$RUN/server/certs:/etc/openvpn/certs:ro" \
  -v "$RUN/server/server-syslog.conf:/etc/openvpn/server.conf:ro" \
  -v "$RUN/server/rsyslog.conf:/etc/rsyslog.conf:ro" \
  -v "$RUN/server/server-syslog-entry.sh:/entry.sh:ro" \
  -v "$RUN/server/logs-syslog:/var/log/openvpn" \
  "$IMG" bash /entry.sh >/dev/null
sleep 3
./run-traffic.sh "$B_CYCLES" "$B_DWELL" syslog
docker stop -t 3 ulpf-ovpn-server >/dev/null
docker cp ulpf-ovpn-server:/var/log/openvpn/syslog out/server-syslog.log

echo "== phase C: OpenVPN 2.5.1 server (pre-2.6 ctime log prefix) =="
docker rm -f ulpf-ovpn-server >/dev/null 2>&1 || true
mkdir -p "$RUN/server/logs-25"
docker run -d --name ulpf-ovpn-server --network "$NET" \
  --cap-add NET_ADMIN --device /dev/net/tun \
  -v "$RUN/server/certs:/etc/openvpn/certs:ro" \
  -v "$RUN/server/server.conf:/etc/openvpn/server.conf:ro" \
  -v "$RUN/server/logs-25:/var/log/openvpn" \
  "$IMG25" openvpn --config /etc/openvpn/server.conf >/dev/null
sleep 2
docker exec ulpf-ovpn-server openvpn --version 2>&1 | head -1 || true
ULPF_OVPN_IMG="$IMG25" ./run-traffic.sh "$C_CYCLES" "$C_DWELL" ctime25
docker stop -t 3 ulpf-ovpn-server >/dev/null
docker cp ulpf-ovpn-server:/var/log/openvpn/server.log out/server-2.5.log
docker rm -f ulpf-ovpn-server >/dev/null

echo "== phase D: OpenVPN 2.4.12 server (the ctime log prefix parsers/openvpn.toml targets) =="
docker rm -f ulpf-ovpn-server >/dev/null 2>&1 || true
mkdir -p "$RUN/server/logs-24"
docker run -d --name ulpf-ovpn-server --network "$NET" \
  --cap-add NET_ADMIN --device /dev/net/tun \
  -v "$RUN/server/certs:/etc/openvpn/certs:ro" \
  -v "$RUN/server/server.conf:/etc/openvpn/server.conf:ro" \
  -v "$RUN/server/logs-24:/var/log/openvpn" \
  "$IMG24" openvpn --config /etc/openvpn/server.conf >/dev/null
sleep 3
docker exec ulpf-ovpn-server openvpn --version 2>&1 | head -1 || true
ULPF_OVPN_IMG="$IMG24" ./run-traffic.sh "$D_CYCLES" "$D_DWELL" ctime24
docker stop -t 3 ulpf-ovpn-server >/dev/null
docker cp ulpf-ovpn-server:/var/log/openvpn/server.log out/server-2.4-ctime.log
docker rm -f ulpf-ovpn-server >/dev/null

echo "== assemble the client log (phase A, all four variants in start order) =="
cat "$RUN/clients/good/logs/native/client.log" \
    "$RUN/clients/badcert/logs/native/client.log" \
    "$RUN/clients/badtls/logs/native/client.log" \
    "$RUN/clients/badport/logs/native/client.log" > out/client.log

wc -l out/*.log
echo "== teardown (trap) =="
