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
