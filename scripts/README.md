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
