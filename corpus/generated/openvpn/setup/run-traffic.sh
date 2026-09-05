#!/bin/bash
# Drives real OpenVPN traffic against the running ulpf-ovpn-server container:
# - a "good" client repeatedly reconnected via SIGHUP (soft restart) and
#   every 7th cycle fully stopped/restarted (SIGTERM -> clean disconnect,
#   then a fresh process)
# - three long-lived failing clients (wrong cert, wrong tls-auth key,
#   unreachable port) left to retry continuously for a dwell window
# All are the real openvpn binary talking real UDP/TLS over a docker bridge.
# Usage: ./run-traffic.sh [RECONNECT_CYCLES] [BAD_CLIENT_DWELL_SECONDS] [LOGSUBDIR]
set -u
cd "$(dirname "$0")"
RUN="$(pwd)/run"
IMG=${ULPF_OVPN_IMG:-ulpf-openvpn:local}
NET=ulpf-vpn-net

RECONNECTS=${1:-30}
BAD_DWELL=${2:-30}
LOGSUB=${3:-native}

start_client() {
  local name="$1" dir="$2"
  docker rm -f "$name" >/dev/null 2>&1
  docker run -d --name "$name" \
    --network "$NET" \
    --cap-add NET_ADMIN --device /dev/net/tun \
    -v "$RUN/clients/$dir/certs:/etc/openvpn/certs:ro" \
    -v "$RUN/clients/$dir/client.conf:/etc/openvpn/client.conf:ro" \
    -v "$RUN/clients/$dir/logs/$LOGSUB:/var/log/openvpn" \
    "$IMG" openvpn --config /etc/openvpn/client.conf >/dev/null
}

for c in good badcert badtls badport; do mkdir -p "$RUN/clients/$c/logs/$LOGSUB"; done

echo "[$(date -u +%H:%M:%SZ)] starting failing clients (badcert, badtls, badport), ${BAD_DWELL}s dwell"
start_client ulpf-ovpn-client-badcert badcert
start_client ulpf-ovpn-client-badtls  badtls
start_client ulpf-ovpn-client-badport badport

echo "[$(date -u +%H:%M:%SZ)] starting good client, ${RECONNECTS} reconnect cycles"
start_client ulpf-ovpn-client-good good
for i in $(seq 1 "$RECONNECTS"); do
  sleep 1.5
  if [ $((i % 7)) -eq 0 ]; then
    # full clean stop/restart: real SIGTERM + explicit-exit-notify, fresh process
    docker stop -t 2 ulpf-ovpn-client-good >/dev/null 2>&1
    sleep 0.5
    docker start ulpf-ovpn-client-good >/dev/null 2>&1
  else
    # soft restart: real SIGHUP to the openvpn process (PID 1)
    docker kill --signal=HUP ulpf-ovpn-client-good >/dev/null 2>&1
  fi
done

echo "[$(date -u +%H:%M:%SZ)] final clean disconnect of good client"
docker stop -t 3 ulpf-ovpn-client-good >/dev/null 2>&1

echo "[$(date -u +%H:%M:%SZ)] dwelling ${BAD_DWELL}s on the failing clients"
sleep "$BAD_DWELL"
docker stop -t 2 ulpf-ovpn-client-badcert ulpf-ovpn-client-badtls ulpf-ovpn-client-badport >/dev/null 2>&1
docker rm -f ulpf-ovpn-client-good ulpf-ovpn-client-badcert ulpf-ovpn-client-badtls ulpf-ovpn-client-badport >/dev/null 2>&1
echo "[$(date -u +%H:%M:%SZ)] traffic done"
