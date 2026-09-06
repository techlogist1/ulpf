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

**From source.** Rust 1.95 or newer (edition 2024), no other build dependency:

```
cargo build --release        # about one minute; binary at target/release/ulpf
```

## Run it

One command, on the samples in this repository:

```
./target/release/ulpf run samples/*.log --store /tmp/s --output /tmp/out.jsonl
```

```
definitions: 15 parsers loaded, 0 file problems
ulpf: 15 files (0 failed), 0.10 MB, 309 events in 0.005 s -> 59345 events/s, 18.6 MB/s, 7 worker threads
stages: framed 309  stored 309  detected 307  no_parser 2  parsed 305  parse_failed 2  normalized 309  emitted 309 (410361 bytes)
parse_failed by reason: pattern_no_match 1, invalid_json 1
signals: sub_matched 202  sub_no_match 7  sub_uncovered 4  time_from_receipt 10  time_error [no_match 1]  class_unknown 62  enum_other 4  unmapped_fields 3025  utf8_lossy 13
queue: 15 batches, high-water 1/64, backpressure blocks 0 (engaged: no)
inference: buffered 2 (buffer full 0)  runs 0  lines templated 0 unmatched 0  proposals written 0 replaced 0 skipped [none]  approved 0  rejected 0  reloads 0
drift: tripped 0  lines routed 0  update proposals 0  cleared 0
syslog: udp datagrams 0 (0 bytes)  tcp connections 0 events 0 (0 bytes) partial 0 refused 0  errors 0
pending: 0 proposals awaiting review (final inference pass 0.000 s)
```

(The `events/s` on that line is 309 events in five milliseconds — startup noise, not a
throughput measurement. The measured figure is under "Honest numbers" below.)

The input is `samples/*.log`, never the bare `samples` directory: the engine has no
include filter yet, so a bare directory ingests `samples/README.md` as a log. That is 16
files and 354 events instead of 15 and 309 — 45 lines of documentation counted as events,
`no_parser` 41 instead of 2, `class_unknown` 106 instead of 62, and an inference run over
prose that ends `skipped [no_templates 1]` (the clustering does refuse it, so no proposal
is written; the counters are wrong, not the review queue). A directory-level include or
exclude is a post-demo decision (D83), so every documented command in this repository
names its log files. The container command above is the one exception a shell cannot fix:
the `scratch` image has no shell to expand a glob, so it takes the mounted directory and
its counters carry those same 45 lines.

That block is the contract: when the output looks plausible but wrong, read it first.
`no_parser` means the format was not recognised, `sub_uncovered` means a message id has no
pattern yet, `class_unknown` means no class rule matched. `ulpf raw <id> --store /tmp/s`
prints the exact input bytes behind any output line's `ulpf.raw_id`.

## See it

```
mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl
```

Then open <http://127.0.0.1:7878> (key `0` for Flow, `1`-`7` for the screens behind it,
`?` for the map). Copy a log file into `demo/watch` and the screens move within 500 ms:

- **Flow** — the front door: every station of the machine — ingest, preserve, detect,
  parse, normalize, emit — live, each one opening the screen behind it.
- **Live** — per-stage counters, throughput, every source with what claimed it, the event tail.
- **Review** — a format nothing claimed, clustered into a proposal: every slot with the
  reason its name was chosen, and the evidence behind every template. Approve activates it.
- **Traceback** — one event's raw bytes, its stored and recomputed digest and chain link,
  and every normalized field lit up over the bytes it was read from.
- **Pivot** — one entity across every device, in one timeline: a source or destination
  IP, a user, a device hostname or a destination port (the five kinds the mapping names).
- **Replay** — every stored event re-run through today's parsers, diffed against the last
  version, with the parser or mapping file whose digest changed named as the reason.
- **Drift** — a device that changed its format mid-stream, its window miss rate against
  its own baseline, and the versioned update proposal with a diff.
- **Integrity** — verify the chain from the UI, and export the attestation a stranger
  re-verifies offline.

## Formats it knows

Fifteen device families ship in `parsers/`. Every definition was written from the vendor's
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
| ArcSight CEF | cef | hand, from the vendor reference | synthetic from the specification (the CEF Implementation Standard's own example lines) |
| IBM LEEF | leef | hand, from the vendor reference | synthetic from the specification (the QRadar DSM guide's own example lines) |
| AWS CloudTrail | json | hand, from the vendor reference | synthetic from the specification (the CloudTrail User Guide's own records) |

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
    "framed": 559, "stored": 559, "detected": 307, "no_parser": 252,
    "parsed": 305, "parse_failed": [["pattern_no_match", 1], ["invalid_json", 1]],
    "normalized": 559, "emitted": 559, "output_bytes": 570063,
    "sub_matched": 202, "sub_no_match": 7, "sub_uncovered": 4,
    "infer_buffered": 252, "infer_runs": 1, "infer_lines_templated": 250,
    "proposals_written": 1, "queue_high_water": 1, "backpressure_blocks": 0,
    ...
  },
  "sources": [ { "name": "mikrotik.log", "events": 250, "detected": 0,
                 "no_parser": 250, "buffered": 250, "pending_id": "mikrotik" }, ... ],
  "parsers": [ { "name": "cisco_asa", "strategy": "pattern", "origin": "hand",
                 "detected": 30 }, ... ],
  "server": { "sse_clients": 0, "review_errors": 0, "uptime_secs": 23.7 }
}
```

**`GET /api/events/{raw_id}`** — one event's provenance: the exact bytes, both digests,
the chain link, and the same bytes through today's parsers (excerpt):

```
$ curl -s http://127.0.0.1:7902/api/events/28
{
  "raw_id": 28,
  "source": "cisco_asa.log",
  "receipt": "2026-09-05T23:00:38.975Z",
  "bytes_len": 157,
  "text": "<164>Sep  4 10:15:24 asa-edge-01 %ASA-4-106023: Deny tcp src outside:203.0.113.9/44321 dst inside:10.0.0.7/22 by access-group \"outside_in\" [0x8ed66b60, 0x0]\n",
  "stored_sha256":     "89e83c1c52a2618c004d9b341b85b913063b27b7362c836b8ab3ad3a3f54638c",
  "recomputed_sha256": "89e83c1c52a2618c004d9b341b85b913063b27b7362c836b8ab3ad3a3f54638c",
  "digest_match": true,
  "prev_chain": "b07ec3420536ad33f7ddce3522088f349dea2af5d9d7faa0b6559ebf9f21bdc6",
  "chain":      "c70a0f59e01f2b53f3a85860ccf5076e1dd9cb17fc957b83e9b8046e9ed4bdfc",
  "chain_match": true,
  "emitted": {
    "class_name": "Network Activity", "class_uid": 4001, "action": "Denied",
    "src_endpoint": { "ip": "203.0.113.9", "port": 44321, "interface_name": "outside" },
    "dst_endpoint": { "ip": "10.0.0.7", "port": 22, "interface_name": "inside" },
    "firewall_rule": { "name": "outside_in" },
    "metadata": { "event_code": "106023", "original_time": "Sep  4 10:15:24",
                  "product": { "vendor_name": "Cisco", "name": "ASA" } },
    "ulpf": { "raw_id": 28, "parser": "cisco_asa", "parse_status": "parsed",
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
{"name":"mikrotik_inferred","now_detected":{"detected":250,"tested":250},"parsers_loaded":16,"path":"/tmp/l4/srv/parsers/mikrotik_inferred.toml","problems":[],"replaced_version":null}
```

## Honest numbers

**258,411 events/s** end to end — ingest through JSON Lines on disk — on an Apple M1 Pro
at the default seven worker threads, with **264/264** fixture events correct. Median of
three runs over a 5,000,000-event, 1,526 MB mixed file (19.0-19.4 s each), raw store with
SHA-256 and the integrity chain flushed per batch included. That is the number to quote.
It was produced by the neutral harness every tool is run through, not by a hand-timed
loop: `eval/run.sh eval/tools/ulpf.toml`, scorecard committed at
`eval/results/ulpf-20260905T140426Z-33371/scorecard.md`, 2026-09-05. Run-to-run variance
is about ±10% on a quiet machine, and both halves of that sentence have now been measured
again on the merged v4 tree.

Quiet: `eval/run.sh eval/tools/ulpf.toml throughput` on the dist build, started
2026-09-06 05:28 IST only after three consecutive twenty-second samples put the one-minute
load under 4 (2.91 at the last) and everything else on the machine at 1.2-1.5 cores' worth
of CPU (118%, 147%, 130% of one core across those three samples), gave
**310,849 / 295,928 / 290,478 events/s (median 295,928)** in 16.1-17.2 s per run. That is
14% above the 258,411 the committed scorecard holds, on the same input, the same harness
and the same thread count. 258,411 stays the figure to quote because it is the one with a
committed scorecard behind it (D87) — the newer number is recorded here, not promoted,
until a scorecard re-pins it. Read the headline as a floor. Those three CPU samples are
pre-run ones: that run has no valid during-run competition figure, because the sampler
counted `ulpf`'s own threads as everything else (`ps -o comm` split on this repository's
spaced path), and a broken instrument is quoted as nothing at all.

Loaded: the identical command, run six times between 05:14 and 05:22 while five other
build and test lanes shared the laptop, gave 153,247 / 282,646 / 166,196 / 192,197 /
221,180 / 308,528 events/s — a 2x spread. Those six went as two sets of three, at
before-run one-minute loads of **4.99** and **5.85** against the quiet set's 2.91, and the
first set ran on a cold page cache as well: its first run took 32.6 s, the slowest of the
six, where the later sets read the input once before gating. Load observed across the six
spanned 4.99 to 21.58, but the top of that range is mostly `ulpf` itself — it alone takes
the load past 18 while it runs (seven worker threads plus store and output I/O), which is
why the load to gate on is the load *before* a run and the CPU everything else is using
*during* it; `docs/evaluation.md`'s 04:00 procedure says the same thing in one line. A
throughput figure without its machine state is not a figure.

**Which build.** Every number in a scorecard is a `--profile dist` number: fat LTO, one
codegen unit — what the Docker image holds and what `eval/tools/ulpf.toml` builds. (And,
since lane 7C, what CI ships: `.github/workflows/app.yml` builds every release asset and
the app's sidecar with `cargo build --profile dist`. Only its `smoke-windows` job stays on
`--release`, deliberately: that job proves the Windows code paths, not the shipped bits.)
The committed scorecard's own header line reads `cargo build --release -p ulpf` because it
predates the split, when `[profile.release]` still carried the fat LTO that
`[profile.dist]` carries now — the same settings under the older name, as Cargo.toml says
where dist is defined. `cargo build --release` is deliberately the other profile —
no LTO, so a stranger's first build finishes in about a minute — and it is what the quick
start above runs and what the harness's cold-start criterion therefore times. The two are
the same source, so only throughput and memory can differ between them at all, and on this
M1 Pro they do not differ measurably: lane P timed both on a 500,000-line slice and got
best-of-eight 1.791 s for dist against 1.690 s for release, medians 2.120 s and 2.164 s —
the two orderings disagree, which is what "inside the noise" looks like. Reproduce a
headline figure with `cargo build --profile dist -p ulpf`; reproduce the quick start with
`cargo build --release`.

Three other figures exist and each measures something different:

- **337,471 events/s** is the *discarded-output* figure: the same file with
  `--output /dev/null`, which measures the engine without the write. Median of three,
  2026-09-05 23:05-23:25 IST, `-j 7`.
- **68,330 events/s** is one worker thread (`-j 1`) on that same file with
  `--output /dev/null`: the per-thread engine rate, not a machine figure.
- **about 30,000 events/s** is the rate with the entity index *on*. Measured for this
  file on 2026-09-06 over a 497,607-line slice of the same bench file, `-j 7`, JSON Lines
  written to disk, machine at load 10-14: `--pivot on` 33,537 events/s against `--pivot
  off` 322,733 for the identical command. An order of magnitude, which is why `run`
  defaults it off and `serve`, whose UI pivots live, defaults it on (D66).
  `ulpf pivot --rebuild` builds the index afterwards from the output.

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
and `scripts/isolation.sh docker ulpf:static samples/cisco_asa.log` under `--network none`.

## Quick start

```
cargo build --release
./target/release/ulpf check
./target/release/ulpf run samples/*.log --store /tmp/ulpf-store --output /tmp/out.jsonl --pivot on
./target/release/ulpf verify --store /tmp/ulpf-store                      # every digest and chain link
./target/release/ulpf attest --store /tmp/ulpf-store --out /tmp/attest.json  # what a stranger re-verifies offline
./target/release/ulpf raw 3 --store /tmp/ulpf-store
./target/release/ulpf replay --store /tmp/ulpf-store --output /tmp/out.jsonl   # v2 beside v1, with a diff and why
./target/release/ulpf pivot src_ip 203.0.113.9 --output /tmp/out.jsonl --limit 5 # one entity across every device
./target/release/ulpf run samples/*.log --store /tmp/ulpf-ecs --output /tmp/ecs.jsonl --schema ecs --parquet /tmp/ecs.parquet

mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl \
    --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514
cp samples/*.log heldout/mikrotik.log demo/watch/      # then open http://127.0.0.1:7878: Live, Review, Traceback, Pivot, Replay, Drift, Integrity
```
Those nine commands are re-run from this file, in a fresh clone, by the harness's
cold-start criterion (`docs/evaluation.md`); they are the install path, not an example of
one — `eval/lib/extract_fence.py` reads the fenced block above out of this README, so it
is the only copy of that list and everything below here is prose the harness ignores.

### On Windows

`cargo build --release` is the same command. The rest, in PowerShell — the executable
takes `.exe`, paths take backslashes and `$env:TEMP`, and the glob has to be expanded by
the shell because the engine takes file arguments, not patterns:

```
cargo build --release
.\target\release\ulpf.exe check
.\target\release\ulpf.exe run (Get-ChildItem samples\*.log).FullName --store $env:TEMP\ulpf-store --output $env:TEMP\out.jsonl --pivot on
.\target\release\ulpf.exe verify --store $env:TEMP\ulpf-store
.\target\release\ulpf.exe attest --store $env:TEMP\ulpf-store --out $env:TEMP\attest.json
.\target\release\ulpf.exe raw 3 --store $env:TEMP\ulpf-store
.\target\release\ulpf.exe replay --store $env:TEMP\ulpf-store --output $env:TEMP\out.jsonl
.\target\release\ulpf.exe pivot src_ip 203.0.113.9 --output $env:TEMP\out.jsonl --limit 5
.\target\release\ulpf.exe run (Get-ChildItem samples\*.log).FullName --store $env:TEMP\ulpf-ecs --output $env:TEMP\ecs.jsonl --schema ecs --parquet $env:TEMP\ecs.parquet

New-Item -ItemType Directory -Force demo\watch | Out-Null
.\target\release\ulpf.exe serve demo\watch --store demo\store --output demo\out.jsonl --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514
Copy-Item samples\*.log,heldout\mikrotik.log demo\watch\
```

**Write the output to a real file, not `NUL`.** On the current release a run whose
`--output` is the null device still writes a `NUL.v1.meta.json` beside it, with an
`events` count of 0. Give it a path under `$env:TEMP` instead. The fix (NUL, `\\.\NUL`
and `\\?\NUL` recognised as sinks, and the count written from what the run emitted) is
on branch `lane-8-windows` and lands after the demo.

**The shell scripts need Git Bash.** `scripts/isolation.sh` and `scripts/coverage.sh` are
bash; run them from a Git Bash prompt (they ship with Git for Windows). `scripts/demo.sh`
is only a wrapper that finds the binary — the runner itself is a subcommand, so on Windows
skip the script and run `.\target\release\ulpf.exe demo` (or `demo --check`, `--auto`,
`--reset`) from PowerShell, no shell required (D67).

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
- `docs/manual-test.md` — the hand-test checklist for the CLI and the desktop app on Windows and macOS, with the expected observation and its source beside every step.
- `PROGRESS.md` — the demo script first, then the build record session by session.
