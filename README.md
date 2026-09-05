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
./target/release/ulpf run samples --store /tmp/ulpf-store --output /tmp/out.jsonl --pivot on
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
Measured 2026-09-05 (evening, v2's final build f267496; the engine's hot path is unchanged since, and the thread-scaling table below is on the v3 tree) on an M1 Pro by the neutral harness
(`eval/run.sh eval/tools/ulpf.toml`, 7 worker threads): 5,000,000 mixed events, 1526 MB, in 19.0 to
19.4 s over three runs, 258k to 264k events/s (median 258k, about 79 MB/s), raw store with SHA-256
and the integrity chain (flushed per batch) and JSON Lines output included, entity index off (`run`
default, D66; about 30k events/s with it on); run-to-run variance is about ±10%.
Thread scaling, measured 2026-09-05 23:05-23:25 IST on the same machine and file with
`--output /dev/null` (no output write, so above the harness figure), every run started at a
one-minute load under 4 with no build running: `-j 1` 68,330 events/s (73.2 s); `-j 2` 121,092
(41.3 s); `-j 4` 200,797 (24.9 s); `-j 7` 314,691 / 337,471 / 345,153 (median 337,471, about
103 MB/s, 14.5-15.9 s). The default `-j` is the core count minus one (7 on this 8-core M1 Pro),
so every throughput figure in this repository is a seven-thread figure unless it says `-j 1`;
the single-thread engine does 68k events/s on this file.

Parser families (`parsers/`): Cisco ASA, Cisco IOS, Fortinet FortiGate, OpenVPN,
Palo Alto PAN-OS, pfSense filterlog, Check Point Log Exporter, Juniper SRX, SonicWall
SonicOS, Sophos Firewall, Squid access log, Suricata EVE; each checked against real
captures under `corpus/` (provenance and licence per file). Output schemas
(`mappings/`): OCSF and ECS. Unknown formats become reviewable parser proposals with a
named reason per slot; a device that changes format gets a versioned update with a diff.
Every raw byte is stored before parsing, digest-chained, and re-verifiable offline; a
restart after a kill yields the same output id for id. `docs/evaluation.md` is the
neutral scorecard any log tool can be run through (`eval/run.sh`).
