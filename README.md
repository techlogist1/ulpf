# ULPF — Universal Log Pre-processing Framework

One static binary that takes perimeter-device logs in any vendor format — firewalls,
IDS/IPS, proxies, VPN concentrators, edge routers — stores every original byte before it
tries to understand any of them, parses each event in the device's own vocabulary,
normalizes it into a pragmatic OCSF (or ECS) subset, and writes JSON Lines. Every output
line points back to the exact bytes it came from, and those bytes are digest-chained so a
stranger can re-verify them offline. A format no parser claims is not dropped: it is
clustered into a candidate parser definition a human reviews and approves in the embedded
UI, and approval activates it without a restart.

## Get it

**A release binary.** Download from
[the releases page](https://github.com/techlogist1/ulpf/releases) — assets are named
`ulpf-<tag>-<target>[.exe]` beside a `SHA256SUMS`, plus the desktop-app installers:

| platform | asset |
|---|---|
| Linux x86-64 | `ulpf-<tag>-x86_64-unknown-linux-musl` (static: no loader, no libc on the host) |
| macOS Apple Silicon | `ulpf-<tag>-aarch64-apple-darwin` |
| Windows x86-64 | `ulpf-<tag>-x86_64-pc-windows-msvc.exe` |

`chmod +x` it and it runs; there is nothing else to install.

**A container.** `docker build -t ulpf:static .` then

```
docker run --rm -v "$PWD/samples:/data/samples:ro" -v "$PWD/out:/data/out" ulpf:static \
    run /data/samples --store /data/out/store --output /data/out/out.jsonl
```

The runtime image is `scratch`: the executable plus `parsers/` and `mappings/`, nothing else.

**From source.** Rust 1.95, no other build dependency:

```
cargo build --release        # about one minute; binary at target/release/ulpf
```

## Run it

One command, on the samples in this repository:

```
./target/release/ulpf run samples --store /tmp/s --output /tmp/out.jsonl
```

```
definitions: 12 parsers loaded, 0 file problems
ulpf: 13 files (0 failed), 0.08 MB, 304 events in 0.005 s -> 57242 events/s, 15.1 MB/s, 7 worker threads
stages: framed 304  stored 304  detected 268  no_parser 36  parsed 264  parse_failed 4  normalized 304  emitted 304 (373975 bytes)
parse_failed by reason: pattern_no_match 3, invalid_json 1
signals: sub_matched 204  sub_no_match 7  sub_uncovered 5  time_from_receipt 48  time_error [none]  class_unknown 100  enum_other 4  unmapped_fields 2647  utf8_lossy 11
queue: 13 batches, high-water 1/64, backpressure blocks 0 (engaged: no)
inference: buffered 36 (buffer full 0)  runs 1  lines templated 0 unmatched 34  proposals written 0 replaced 0 skipped [no_templates 1]  approved 0  rejected 0  reloads 0
drift: tripped 0  lines routed 0  update proposals 0  cleared 0
syslog: udp datagrams 0 (0 bytes)  tcp connections 0 events 0 (0 bytes) partial 0 refused 0  errors 0
pending: 0 proposals awaiting review (final inference pass 0.002 s)
```

That block is the contract: when the output looks plausible but wrong, read it first.
`no_parser` means the format was not recognised, `sub_uncovered` means a message id has no
pattern yet, `class_unknown` means no class rule matched. `ulpf raw <id> --store /tmp/s`
prints the exact input bytes behind any output line's `ulpf.raw_id`.

## See it

```
mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl
```

Then open <http://127.0.0.1:7878> (keys `1`-`7`, `?` for the map). Copy a log file into
`demo/watch` and the screens move within 500 ms:

- **Live** — per-stage counters, throughput, every source with what claimed it, the event tail.
- **Review** — a format nothing claimed, clustered into a proposal: every slot with the
  reason its name was chosen, and the evidence behind every template. Approve activates it.
- **Traceback** — one event's raw bytes, its stored and recomputed digest and chain link,
  and every normalized field lit up over the bytes it was read from.
- **Pivot** — one IP, user, host, hash or port across every device, in one timeline.
- **Replay** — every stored event re-run through today's parsers, diffed against the last
  version, with the parser or mapping file whose digest changed named as the reason.
- **Drift** — a device that changed its format mid-stream, its window miss rate against
  its own baseline, and the versioned update proposal with a diff.
- **Integrity** — verify the chain from the UI, and export the attestation a stranger
  re-verifies offline.

## Formats it knows

Twelve device families ship in `parsers/`. Every definition was written from the vendor's
own log reference, never from memory; where a permissively licensed real capture exists it
was appended to the sample and the definition fixed against it (D30, D63).

| family | strategy | written by | sample |
|---|---|---|---|
| Cisco ASA | pattern | hand, from the vendor reference | real capture (`corpus/real/cisco_asa/cisco_asa_arcane_door.log`) |
| Cisco IOS | pattern | hand, from the vendor reference | real capture (`corpus/real/cisco_ios/cisco_ios_sisf_t1557.log`) |
| Fortinet FortiGate | kv | hand, from the vendor reference | real capture (`corpus/real/fortinet_fortigate/fortigate_sample_sva_s1.log`) |
| OpenVPN | pattern | hand, from the vendor reference | generated by the real tool (`corpus/generated/openvpn/server.log`, OpenVPN 2.6.14) |
| Palo Alto PAN-OS | delimiter | hand, from the vendor reference | sanitized real (`corpus/real/palo_alto_panos/palo_alto_panos.log`) |
| pfSense filterlog | pattern | hand, from the vendor reference | synthetic from the specification (real capture in `corpus/real/pfsense_filterlog/pfsense_filterlog.log`) |
| Check Point Log Exporter | pattern | hand, from the vendor reference | synthetic from the specification (no permissively licensed capture found) |
| Juniper SRX | pattern | hand, from the vendor reference | synthetic from the specification (third-party generated data in `corpus/real/juniper_srx/juniper_srx.log`) |
| SonicWall SonicOS | kv | hand, from the vendor reference | real capture (`corpus/real/sonicwall/sonicwall.log`) |
| Sophos Firewall (SFOS) | kv | hand, from the vendor reference | synthetic from the specification (sanitized real in `corpus/real/sophos_xg/sophos_xg.log`) |
| Squid access log | pattern | hand, from the vendor reference | generated by the real tool (`corpus/generated/squid/access.log`, Squid 6.13) |
| Suricata EVE | json | hand, from the vendor reference | synthetic from the specification (real capture in `corpus/real/suricata_eve/ccdcoe_cdmcs_eve.json`) |
| ArcSight CEF | cef | *(landing in lane 3)* | *(synthetic from the specification)* |
| IBM LEEF | leef | *(landing in lane 3)* | *(synthetic from the specification)* |
| AWS CloudTrail | json | *(landing in lane 3)* | *(synthetic from the specification)* |

Output schemas live in `mappings/`: `ocsf.toml` and `ecs.toml` (`--schema ecs`). A parser
never names a schema field and a mapping never names a vendor; that wall is structural.
`docs/coverage.md` is what the engine does to every sample and every corpus file, counter
by counter.

## The API

The server is the whole product surface; the UI is a client of it. Full contract:
`docs/api.md`. Three calls, run against a server on port 7902 with a copy of `parsers/`,
fed `samples/*.log` and then `heldout/mikrotik.log`:

**`GET /api/metrics`** — the same counters the run block prints, live (excerpt):

```
$ curl -s http://127.0.0.1:7902/api/metrics
{
  "engine": {
    "framed": 514, "stored": 514, "detected": 262, "no_parser": 252,
    "parsed": 260, "parse_failed": [["pattern_no_match", 1], ["invalid_json", 1]],
    "normalized": 514, "emitted": 514, "output_bytes": 506145,
    "sub_matched": 202, "sub_no_match": 7, "sub_uncovered": 4,
    "infer_buffered": 252, "infer_runs": 1, "infer_lines_templated": 250,
    "proposals_written": 1, "queue_high_water": 1, "backpressure_blocks": 0,
    ...
  },
  "sources": [ { "name": "mikrotik.log", "events": 250, "detected": 0,
                 "no_parser": 250, "buffered": 250, "pending_id": "mikrotik" }, ... ],
  "parsers": [ { "name": "cisco_asa", "strategy": "pattern", "origin": "hand",
                 "detected": 27 }, ... ],
  "server": { "sse_clients": 0, "review_errors": 0, "uptime_secs": 22.4 }
}
```

**`GET /api/events/{raw_id}`** — one event's provenance: the exact bytes, both digests,
the chain link, and the same bytes through today's parsers (excerpt):

```
$ curl -s http://127.0.0.1:7902/api/events/14
{
  "raw_id": 14,
  "source": "cisco_asa.log",
  "receipt": "2026-09-05T21:43:39.863Z",
  "bytes_len": 157,
  "text": "<164>Sep  4 10:15:24 asa-edge-01 %ASA-4-106023: Deny tcp src outside:203.0.113.9/44321 dst inside:10.0.0.7/22 by access-group \"outside_in\" [0x8ed66b60, 0x0]\n",
  "stored_sha256":     "89e83c1c52a2618c004d9b341b85b913063b27b7362c836b8ab3ad3a3f54638c",
  "recomputed_sha256": "89e83c1c52a2618c004d9b341b85b913063b27b7362c836b8ab3ad3a3f54638c",
  "digest_match": true,
  "prev_chain": "dbf9c22feb7a2f851c5009ad2bc38e3328d3aeca83b79834ae1325fb1f332982",
  "chain":      "ffc61fa32bdc2edd119d23fe856ca8437fef01ac7399aa7f4bb801232a7a9482",
  "chain_match": true,
  "emitted": {
    "class_name": "Network Activity", "class_uid": 4001, "action": "Denied",
    "src_endpoint": { "ip": "203.0.113.9", "port": 44321, "interface_name": "outside" },
    "dst_endpoint": { "ip": "10.0.0.7", "port": 22, "interface_name": "inside" },
    "firewall_rule": { "name": "outside_in" },
    "metadata": { "event_code": "106023", "original_time": "Sep  4 10:15:24",
                  "product": { "vendor_name": "Cisco", "name": "ASA" } },
    "ulpf": { "raw_id": 14, "parser": "cisco_asa", "parse_status": "parsed",
              "sub_status": "matched", "time_policies": ["year_assumed", "tz_assumed"] },
    "unmapped": { "hash_1": "0x8ed66b60", "syslog_pri": "164", ... }
  },
  "now": { "parser": "cisco_asa", "parse_status": "parsed", "normalized": { ... } }
}
```

**`POST /api/pending/{id}/approve`** — the only path from a proposal to an active parser.
`now_detected` re-runs detection over the source's buffered unknown lines with the new
registry, so the response is the proof that those events now take the fast path:

```
$ curl -s -X POST http://127.0.0.1:7902/api/pending/mikrotik/approve
{"name":"mikrotik_inferred","now_detected":{"detected":250,"tested":250},"parsers_loaded":13,"path":"/tmp/l4/srv/parsers/mikrotik_inferred.toml","problems":[],"replaced_version":null}
```

## Honest numbers

**258,411 events/s** end to end — ingest through JSON Lines on disk — on an Apple M1 Pro
at the default seven worker threads, with **264/264** fixture events correct. Median of
three runs over a 5,000,000-event, 1,526 MB mixed file (19.0-19.4 s each), raw store with
SHA-256 and the integrity chain flushed per batch included. That is the number to quote.
It was produced by the neutral harness every tool is run through, not by a hand-timed
loop: `eval/run.sh eval/tools/ulpf.toml`, scorecard committed at
`eval/results/ulpf-20260905T140426Z-33371/scorecard.md`, 2026-09-05. Run-to-run variance
is about ±10%.

Three other figures exist and each measures something different:

- **337,471 events/s** is the *discarded-output* figure: the same file with
  `--output /dev/null`, which measures the engine without the write. Median of three,
  2026-09-05 23:05-23:25 IST, `-j 7`.
- **68,330 events/s** is one worker thread (`-j 1`) on that same file with
  `--output /dev/null`: the per-thread engine rate, not a machine figure.
- **about 30,000 events/s** is what the entity index costs when it is on. `run` defaults
  it off and `serve` defaults it on, because the pivot is a live-UI feature and bulk
  ingest should not pay for it (D66); `ulpf pivot --rebuild` builds it afterwards.

Generate the file the numbers are measured on with
`cargo run --release -p ulpf --example gen_bench -- 5000000 bench` (see `bench/README.md`).

**Kill recovery.** `kill -9` a run mid-file, start the same command again, and the output
is identical id for id — the restart completes the interrupted output from the store
before ingesting anything new, and says `recovered: N`:
`ulpf run bench/mixed-5000000.log --store /tmp/kr --output /tmp/kr.jsonl & sleep 3; kill -9 $!`
then re-run the same line.

**Isolation.** The binary makes no outbound connection: every socket the process holds is
sampled twice a second over a whole run and classified, in three modes —
`scripts/isolation.sh run bench/mixed-5000000.log`, `scripts/isolation.sh serve demo/watch 20`,
and `scripts/isolation.sh docker ulpf:static samples` under `--network none`.

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
Those nine commands are re-run from this file, in a fresh clone, by the harness's
cold-start criterion (`docs/evaluation.md`); they are the install path, not an example of one.

## Where things are

- `CLAUDE.md` — the architecture: the three shapes of an event, the parser/mapping wall,
  and every invariant the code is held to.
- `docs/api.md` — the HTTP and SSE contract the server and the UI are both built against.
- `docs/parser-format.md` — how to write a parser definition, for anyone who never opens
  a Rust file.
- `docs/evaluation.md` — the neutral scorecard any log tool can be run through, and the
  procedure for comparing two of them on one machine.
- `docs/coverage.md` — every sample and corpus file with its counters, regenerated by
  `scripts/coverage.sh`.
- `PROGRESS.md` — the demo script first, then the build record session by session.
