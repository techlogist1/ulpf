# ULPF — Universal Log Pre-processing Framework

Single static binary. Ingests perimeter-device logs in any vendor format, preserves
every raw byte immutably, parses with the vendor's own vocabulary, normalizes to a
pragmatic OCSF subset, emits JSON Lines, and prints measured throughput.

See `CLAUDE.md` for architecture and the plain-text folder contract,
`docs/parser-format.md` for writing parser definitions, `PROGRESS.md` for state.

## Quick start

```
cargo build --release
./target/release/ulpf check
./target/release/ulpf run samples --store /tmp/ulpf-store --output /tmp/out.jsonl
./target/release/ulpf verify --store /tmp/ulpf-store
./target/release/ulpf raw 3 --store /tmp/ulpf-store
```

Static container build: `docker build -t ulpf:static .` then
`docker run --rm -v "$PWD/samples:/data/samples:ro" -v "$PWD/out:/data/out" ulpf:static run /data/samples --store /data/out/store --output /data/out/out.jsonl`.

Throughput file: `cargo run --release -p ulpf --example gen_bench -- 5000000 bench` (see `bench/README.md`).
Measured 2026-09-05 on an M1 Pro (7 worker threads): 5,000,000 mixed events, 1526 MB, in 22.1 s,
226k events/s, 69.1 MB/s, raw store with SHA-256 (flushed per batch) and JSON Lines output included.

Parser families in v0.1 (`parsers/`): Cisco ASA, Cisco IOS, Fortinet FortiGate, OpenVPN,
Palo Alto PAN-OS, pfSense filterlog, Check Point Log Exporter, Juniper SRX, SonicWall
SonicOS, Sophos Firewall, Squid access log, Suricata EVE.
