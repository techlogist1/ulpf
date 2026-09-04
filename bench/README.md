# Benchmark Data

This directory contains synthetic log files generated for benchmarking the ULPF parser framework.

## Regenerating benchmark files

To regenerate the benchmark file, run:

```bash
cargo run --release -p ulpf --example gen_bench -- 5000000 bench
```

This generates a 5,000,000 line mixed-format log file (about 1.5 GB with the 12 v0.1 families) combining samples from all available log formats with synthetic mutations applied. Generation takes about 25 s on an M1 Pro.

Measure with:

```bash
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/ulpf-bench-store --output /dev/null
```

The number recorded in `PROGRESS.md` (226k events/s, 69.1 MB/s, 2026-09-05, M1 Pro) came from exactly that command.

## How the generator works

The `gen_bench` example:
- Reads all sample log files from `samples/` directory
- Frames multi-line events (lines with indented continuations)
- Generates the target number of output lines by randomly selecting and mutating sample events
- Mutations include:
  - Replacing IPv4 addresses with random valid IPs
  - Replacing port numbers (in fields like `srcport=`, `dstport=`, `port=`)
  - Replacing session/connection IDs
- Injects realistic corruption at ~0.1% rate:
  - Truncated lines
  - Non-UTF-8 bytes
  - Doubled spaces
  - Empty lines

The RNG is seeded deterministically, so all runs produce identical output.

## File commitment

These `.log` files are never committed to the repository and are generated as needed for testing.
