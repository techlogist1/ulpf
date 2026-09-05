building: cargo build --release -p ulpf --target-dir "/Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T200729Z-11466/.cargo-target"
tool binary: /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T200729Z-11466/.cargo-target/release/ulpf
# scorecard: ulpf (20260905T200729Z)
tool config: eval/tools/ulpf.toml
quick mode: no
threads: 7

## cold_start
$ cargo build --release
$ ./target/release/ulpf check
$ ./target/release/ulpf run samples --store /tmp/ulpf-store --output /tmp/out.jsonl --pivot on
$ ./target/release/ulpf verify --store /tmp/ulpf-store                      # every digest and chain link
$ ./target/release/ulpf attest --store /tmp/ulpf-store --out /tmp/attest.json  # what a stranger re-verifies offline
$ ./target/release/ulpf raw 3 --store /tmp/ulpf-store
$ ./target/release/ulpf replay --store /tmp/ulpf-store --output /tmp/out.jsonl   # v2 beside v1, with a diff and why
$ ./target/release/ulpf pivot src_ip 203.0.113.9 --output /tmp/out.jsonl --limit 5 # one entity across every device
$ ./target/release/ulpf run samples --store /tmp/ulpf-ecs --output /tmp/ecs.jsonl --schema ecs --parquet /tmp/ecs.parquet
stopping before long-running command: mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl \
commands run: 9, total wall time: 94.8s
COLD START: PASS

raw output and exact commands: /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T200729Z-11466/raw
