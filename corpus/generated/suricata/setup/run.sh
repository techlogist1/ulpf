#!/bin/sh
# Reproduce the ULPF suricata_eve corpus capture end to end.
# Requires: docker (OrbStack or Docker Desktop), internet egress from containers.
# Usage: ./run.sh [rounds]   (default 40; each round ~30-60s -> ~5-10 min wall clock)
set -eu
ROUNDS="${1:-40}"
HERE="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$HERE/logs"

docker rm -f suricata-gen >/dev/null 2>&1 || true

docker run -d --name suricata-gen \
  --cap-add=NET_ADMIN --cap-add=NET_RAW --cap-add=SYS_NICE \
  -v "$HERE/logs:/var/log/suricata" \
  --entrypoint sh jasonish/suricata:latest -c 'sleep 3600'

echo "waiting for container network..."
sleep 2

echo "fetching ET Open rules (suricata-update, needs internet)..."
docker exec suricata-gen suricata-update -v

echo "starting suricata on eth0..."
docker exec -d suricata-gen sh -c \
  'suricata -c /etc/suricata/suricata.yaml -S /var/lib/suricata/rules/suricata.rules -i eth0 -l /var/log/suricata --set stats.interval=15'

sleep 5

echo "copying traffic generator into the container..."
docker cp "$HERE/traffic-gen.py" suricata-gen:/root/traffic-gen.py

echo "generating real traffic ($ROUNDS rounds: DNS, HTTP, TLS, EICAR download, port scan)..."
docker exec suricata-gen python3 /root/traffic-gen.py "$ROUNDS"

echo "letting flows time out and flush (30s)..."
sleep 30

echo "stopping suricata cleanly (flushes remaining flow records)..."
docker exec suricata-gen sh -c 'kill -TERM $(pgrep suricata)' || true
sleep 5

echo "copying eve.json out byte-for-byte..."
docker cp suricata-gen:/var/log/suricata/eve.json "$HERE/logs/eve.json"
wc -l "$HERE/logs/eve.json"

echo "done. Teardown: docker rm -f suricata-gen"
