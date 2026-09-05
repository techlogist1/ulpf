# demo.sh — a wrapper; the runner is `ulpf demo`

```
scripts/demo.sh            # interactive: Enter advances, the server stays up at the end
scripts/demo.sh --auto     # unattended rehearsal: fixed 3 s pauses, then stop and reset
scripts/demo.sh --check    # inputs, ports, and no drift from the demo section of PROGRESS.md
scripts/demo.sh --reset    # stop a leftover server and remove demo/
```

The runner lives in the binary (`ulpf demo`, D67), so the demo plays on Windows and Linux as
well as here; this script only finds `./target/release/ulpf` and hands the flags over. The
subcommand takes `--dir`, `--listen`, `--syslog` and `--repo` for a second rehearsal beside a
live server. It plays steps 0-9 of the demo section of PROGRESS.md with the existing
subcommands, spawning its own `ulpf serve` on `demo/watch` with `--parsers demo/parsers
--pending demo/pending`, so nothing lands in the repo's `parsers/` or `pending/`; steps 10-13
are named, not played. `--check` starts nothing: it reports `ok`/`DRIFT` per item and exits 0
or 1, and `cargo test -p ulpf demo` asserts the same titles and commands.

# isolation.sh — proves the binary makes no outbound connection

```
scripts/isolation.sh run bench/mixed-5000000.log   # sample sockets for a whole run
scripts/isolation.sh serve watch-dir [seconds]     # same, with the server up and fed
scripts/isolation.sh docker ulpf:latest samples/   # --network none: needs no network at all
scripts/isolation.sh selftest                      # checks the classifier and the sampler
```

`ULPF_BIN` defaults to `./target/release/ulpf` (repo-root relative). To dodge the
lead's build lock: `CARGO_TARGET_DIR=target-iso cargo build --release` and
`ULPF_BIN=./target-iso/release/ulpf`.

Every socket the process holds is sampled every 0.5 s (`lsof -nP -a -p` on macOS,
`ss -tunpa` on Linux, `/proc/<pid>/fd` + `/proc/net/*` with no `ss`) and classified:
loopback listen OK, loopback peer OK, anything else FAIL. Ends with `ISOLATION PASS`
or `ISOLATION FAIL: <reason>`, exit 0/1.

It fails loudly rather than on nothing: missing binary, a sampler returning nothing
for a live pid, a run too short to sample once, a `serve` binary without the subcommand,
a `serve` that never listened or never answered. Sampling cannot catch a socket opened
and closed inside one 0.5 s gap; `docker` mode is the airtight complement.

`serve` env: `ULPF_FEED` (default `bench/mixed-5000000.log`, else `samples/`),
`ULPF_FEED_MODE=auto|copy|symlink`, `ULPF_LISTEN` (default `127.0.0.1:7878`).

# soak.sh — proves `serve` survives a long run and loses nothing

```
scripts/soak.sh --bin ./target/release/ulpf --minutes 12 --file-rate 150000 \
                [--udp 127.0.0.1:5514 --udp-rate 20000] \
                [--tcp 127.0.0.1:5515 --tcp-rate 20000] \
                [--events-target 10000000] [--watch DIR] [--out DIR]
scripts/soak.sh --selftest          # generator + both socket senders, ~45 s, no server
scripts/soak.sh --report-only DIR   # re-report a finished — or interrupted — run
```

Starts `serve` on a fresh store watching `--out DIR/watch` with `--tail 1000`, appends
generated events to one file in it at `--file-rate` per second, and (with `--udp`/`--tcp`)
sends generated syslog lines at those sockets. Three phases: the steady rate, then 60 s at
3x it, then the steady rate again (`--burst-secs`, `--burst-mult`), so the bounded queue's
saturation policy is exercised inside every run rather than hoped for. `--events-target`
stops every stream once that many events exist, whichever comes first; budget about
2.1 kB of disk per event (input + store + JSON Lines) and the script refuses a plan that
needs more than half the free space.

Throughout: one SSE client on `/api/stream` for the whole run (frames, tail events and any
gap over 5 s — the "UI stays live" proxy), `/api/metrics` every 5 s, and the serve process's
RSS and thread count every second. At the end it SIGINTs the server, waits for the counter
block, runs `ulpf verify`, and reconciles exactly:

```
lines appended + events sent per socket == framed == stored == emitted == records in the store
```

Everything lands under `--out`: `plan.txt`, `serve.log` (with the counter block),
`counts.json` (the generator's own totals), `metrics.jsonl`, `rss.tsv`, `sse.jsonl`,
`sse-summary.json`, `verify.txt` and `report.txt`. `report.txt` is the one screen:
achieved rates, RSS min/max/slope in MB/min over the last five minutes (a leak is a
positive slope that does not flatten), queue high-water and backpressure blocks with what
the burst phase did to them, SSE frames and max gap, and `SOAK PASS`/`SOAK FAIL` with
every number. Exit 0 on PASS, 1 on FAIL, 3 on PARTIAL.

`--report-only DIR` prints that same report over a directory that already exists, and is
the way to get numbers out of a run that was killed. Every input is optional: with no
counter block in `serve.log` it falls back to `final-metrics.json` (polled just before
shutdown) or the last `/api/metrics` sample and says so on the header line; with no
`verify.txt` it runs `ulpf verify` itself when `SOAK_BIN` points at a binary; with no
`emitted` anywhere it counts the lines of `out.jsonl`. A reconciliation line it cannot
decide prints `??` instead of `ok`/`FAIL`, and the report ends with a
`NOT MEASURED` list naming every input it did not have and `SOAK PARTIAL`. A missing
number is never reported as a passing one.

The load generator is `crates/ulpf/examples/soak_gen.rs`, built on demand into
`$CARGO_TARGET_DIR/release/examples/soak_gen` (pass `--gen PATH` to skip the build). It
mutates the real `samples/*.log` lines — every IPv4, port, time-of-day and fractional
second is rewritten, and one carrier field takes a global sequence number, so no two
generated events are ever byte-identical — and mixes in `--unknown` lines no parser claims
(0.5% by default, `heldout/mikrotik.log`) to keep the inference path warm. Events that go
on a socket are flattened to one line first: syslog is one message per datagram or per
line, so a multi-line sample would otherwise arrive as several events and no soak with
listeners could ever reconcile.

`--selftest` proves the generator before you trust a soak: 200k events, all distinct, and
the share the real parser registry still detects (99.5% on the shipped samples). It then
runs both socket senders at a Python UDP/TCP listener twice — newline framing and RFC 6587
octet counting — and fails unless TCP arrives exactly and UDP within 2%.

The listeners the senders aim at do not exist in the binary yet. `--udp ADDR --udp-rate N`
and `--tcp ADDR --tcp-rate N` also pass `--syslog-udp`/`--syslog-tcp` to `serve`, so the
moment that half lands the full soak runs unchanged: the report already reconciles the
sockets separately from the file, comparing `counts.json`'s `udp_events`/`tcp_events`
against the events `/api/metrics` attributes to the `udp/<peer>` and `tcp/<peer>` sources.
A UDP shortfall with zero sender errors is a kernel drop rather than a lost event, and the
report says so where you read it.

Deviating from the defaults: `--listen` (7878) to soak beside another server, `--gen`,
`--unknown`, `--burst-secs 0` for no burst, `--tcp-octet-counting` for RFC 6587 framing.
