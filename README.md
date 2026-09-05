# ULPF — Universal Log Pre-processing Framework

Single static binary. Ingests perimeter-device logs in any vendor format, preserves
every raw byte immutably, parses with the vendor's own vocabulary, normalizes to a
pragmatic OCSF subset, emits JSON Lines, and prints measured throughput. Formats it does
not know are clustered into a candidate parser a human reviews and approves in the
embedded UI; approval activates the parser without a restart.

See `CLAUDE.md` for architecture and the plain-text folder contract,
`docs/parser-format.md` for writing parser definitions, `PROGRESS.md` for state.

## Quick start

```
cargo build --release
./target/release/ulpf check
./target/release/ulpf run samples --store /tmp/ulpf-store --output /tmp/out.jsonl
./target/release/ulpf verify --store /tmp/ulpf-store                      # every digest and chain link
./target/release/ulpf attest --store /tmp/ulpf-store --out /tmp/attest.json  # what a stranger re-verifies offline
./target/release/ulpf raw 3 --store /tmp/ulpf-store
./target/release/ulpf replay --store /tmp/ulpf-store --output /tmp/out.jsonl   # v2 beside v1, with a diff and why
./target/release/ulpf pivot src_ip 203.0.113.9 --output /tmp/out.jsonl --limit 5 # one entity across every device
./target/release/ulpf run samples --store /tmp/ulpf-ecs --output /tmp/ecs.jsonl --schema ecs --parquet /tmp/ecs.parquet

mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl \
    --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514
cp samples/*.log heldout/mikrotik.log demo/watch/      # then open http://127.0.0.1:7878: Live, Review, Traceback, Pivot, Replay, Drift, Integrity
```
The full demo, with the numbers measured on 2026-09-05, is the first section of `PROGRESS.md`.

Static container build: `docker build -t ulpf:static .` then
`docker run --rm -v "$PWD/samples:/data/samples:ro" -v "$PWD/out:/data/out" ulpf:static run /data/samples --store /data/out/store --output /data/out/out.jsonl`.

Throughput file: `cargo run --release -p ulpf --example gen_bench -- 5000000 bench` (see `bench/README.md`).
Measured 2026-09-05 on an M1 Pro (7 worker threads): 5,000,000 mixed events, 1526 MB, in 21.5 to 23.4 s
over three runs, 214k to 232k events/s (median 225k, about 69 MB/s), raw store with SHA-256 (flushed
per batch) and JSON Lines output included; run-to-run variance is about ±10%.

Parser families (`parsers/`): Cisco ASA, Cisco IOS, Fortinet FortiGate, OpenVPN,
Palo Alto PAN-OS, pfSense filterlog, Check Point Log Exporter, Juniper SRX, SonicWall
SonicOS, Sophos Firewall, Squid access log, Suricata EVE; each checked against real
captures under `corpus/` (provenance and licence per file). Output schemas
(`mappings/`): OCSF and ECS. Unknown formats become reviewable parser proposals with a
named reason per slot; a device that changes format gets a versioned update with a diff.
Every raw byte is stored before parsing, digest-chained, and re-verifiable offline; a
restart after a kill yields the same output id for id. `docs/evaluation.md` is the
neutral scorecard any log tool can be run through (`eval/run.sh`).
