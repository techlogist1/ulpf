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
scripts/soak.sh --selftest          # generator + both socket senders, ~20 s, no server
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
every number. Exit 0 only on PASS.

The load generator is `crates/ulpf/examples/soak_gen.rs`, built on demand into
`$CARGO_TARGET_DIR/release/examples/soak_gen` (pass `--gen PATH` to skip the build). It
mutates the real `samples/*.log` lines — every IPv4, port, time-of-day and fractional
second is rewritten, and one carrier field takes a global sequence number, so no two
generated events are ever byte-identical — and mixes in `--unknown` lines no parser claims
(0.5% by default, `heldout/mikrotik.log`) to keep the inference path warm. Events that go
on a socket are flattened to one line first: syslog is one message per datagram or per
line, so a multi-line sample would otherwise arrive as several events and no soak with
listeners could ever reconcile.

`--selftest N` proves the generator before you trust a soak: N events, all distinct, and
the share the real parser registry still detects (99.5% on the shipped samples). The same
`--selftest` run checks both socket senders against a Python UDP/TCP listener and fails
unless TCP arrives exactly and UDP within 2%. The listeners the senders aim at do not
exist in the binary yet — `--udp`/`--tcp` also pass `--syslog-udp`/`--syslog-tcp` to
`serve`, so a soak with them only runs once that half lands.

Deviating from the defaults: `--listen` (7878) to soak beside another server, `--gen`,
`--unknown`, `--burst-secs 0` for no burst, `--tcp-octet-counting` for RFC 6587 framing.
