#!/bin/sh
# Runs zeek/zeek:latest offline over the captured pcap. TSV (default) and JSON both.
set -eu
SCR="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$SCR/zeek-out-tsv" "$SCR/zeek-out-json"

# -C: ignore checksums. Docker's veth pairs on OrbStack/macOS never compute real
# TCP/UDP checksums (offloaded to hardware that never runs for internal traffic),
# so zeek discards every packet as corrupt without this and only conn.log/weird.log
# come out; -C makes it actually reassemble streams and run the DNS/HTTP/SSL analyzers.
docker run --rm \
  -v "$SCR/pcap:/pcap:ro" \
  -v "$SCR/zeek-out-tsv:/out" \
  -w /out \
  zeek/zeek:latest \
  zeek -C -r /pcap/capture.pcap LogAscii::use_json=F

docker run --rm \
  -v "$SCR/pcap:/pcap:ro" \
  -v "$SCR/zeek-out-json:/out" \
  -w /out \
  zeek/zeek:latest \
  zeek -C -r /pcap/capture.pcap LogAscii::use_json=T

echo "TSV logs:"
ls -la "$SCR/zeek-out-tsv"
echo "JSON logs:"
ls -la "$SCR/zeek-out-json"
