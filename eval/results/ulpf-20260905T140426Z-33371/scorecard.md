building: cargo build --release -p ulpf --target-dir "/Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T140426Z-33371/.cargo-target"
tool binary: /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T140426Z-33371/.cargo-target/release/ulpf
# scorecard: ulpf (20260905T140426Z)
tool config: eval/tools/ulpf.toml
quick mode: no
threads: 7

## throughput
input: bench/mixed-5000000.log (1600273885 bytes)
run 1: exit=0 wall=18.969s events=5000000 events/s=263588 MB/s=80.5 (cmd: raw/throughput-1.cmd)
run 2: exit=0 wall=19.349s events=5000000 events/s=258411 MB/s=78.9 (cmd: raw/throughput-2.cmd)
run 3: exit=0 wall=19.350s events=5000000 events/s=258398 MB/s=78.9 (cmd: raw/throughput-3.cmd)
median events/s across 3 runs: 258411

## correctness
check_point: total=13 matched=13 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-check_point.cmd, diff: raw/correctness-check_point.diff)
cisco_asa: total=30 matched=30 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-cisco_asa.cmd, diff: raw/correctness-cisco_asa.diff)
cisco_ios: total=32 matched=32 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-cisco_ios.cmd, diff: raw/correctness-cisco_ios.diff)
fortinet_fortigate: total=11 matched=11 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-fortinet_fortigate.cmd, diff: raw/correctness-fortinet_fortigate.diff)
juniper_srx: total=16 matched=16 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-juniper_srx.cmd, diff: raw/correctness-juniper_srx.diff)
openvpn: total=44 matched=44 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-openvpn.cmd, diff: raw/correctness-openvpn.diff)
palo_alto_panos: total=22 matched=22 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-palo_alto_panos.cmd, diff: raw/correctness-palo_alto_panos.diff)
pfsense_filterlog: total=18 matched=18 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-pfsense_filterlog.cmd, diff: raw/correctness-pfsense_filterlog.diff)
sonicwall: total=19 matched=19 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-sonicwall.cmd, diff: raw/correctness-sonicwall.diff)
sophos_xg: total=15 matched=15 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-sophos_xg.cmd, diff: raw/correctness-sophos_xg.diff)
squid_access: total=33 matched=33 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-squid_access.cmd, diff: raw/correctness-squid_access.diff)
suricata_eve: total=11 matched=11 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-suricata_eve.cmd, diff: raw/correctness-suricata_eve.diff)
TOTAL: events=264 matched=264 mismatched=0 missing=0 pct=100.0%
scope: only fixture keys observable in a tool's *output* are checked (normalized/time/time_policies/parser/status/sub); 'fields'/'absent' describe the vendor-native parse stage, out of scope for a black-box harness -- see fixtures/README.md and docs/evaluation.md.

## raw_preservation
verify: store 9571def6b959e5187d91d41919bc2e43 genesis 83e2c0cb3ebf8ebf2c711bb93b35aae4a0d496f15feed4877b86d4cef473cbaf
verified 30 records, 0 corrupt
chain ok (head 4a3f54d1b9a7d50764fd1123e2e2a4472ecb4629f8c4fad55f5b53b0761b6cce) (exit=0, cmd: raw/rawpres-verify.cmd)
records verified: 20/20 sampled ids (mismatches: 0); framing rule: docs/evaluation.md#raw_preservation

## unknown_format
exit=0 events_emitted=250 proposals_written=1 (in /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T140426Z-33371/raw/unknown.pending) (cmd: raw/unknown.cmd)

## damaged_inputs
binary_garbage.log: exit=0 events=11 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.001 s)'
crlf.log: exit=0 events=18 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
cut_mid_field_10pct.log: exit=0 events=18 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
empty.log: exit=0 events=0 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
zero_byte.log: exit=0 events=0 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
no_parser_format.log: exit=0 events=50 crashed=no hung=no stderr_last='  no_parser_format  source no_parser_format.log  50 lines  1 templates  0 unmatched'
nul_byte_mid.log: exit=0 events=18 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
only_newlines.log: exit=0 events=1 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
single_8mib_line.log: exit=0 events=1 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
symlink_loop: exit=2 events=0 crashed=no hung=no stderr_last='ulpf: input /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/damaged/symlink_loop: Too many levels of symbolic links (os error 62)'
truncated_no_newline.log: exit=0 events=5 crashed=no hung=no stderr_last='pending: 0 proposals awaiting review (final inference pass 0.000 s)'
utf16_bom.log: exit=0 events=19 crashed=no hung=no stderr_last='  utf16_bom  source utf16_bom.log  19 lines  1 templates  4 unmatched'

## isolation
sampled 0 socket observation(s) over the run (pid 35572, sampler lsof)
(no socket observed)
ISOLATION: PASS
(exit=0, cmd: raw/isolation.cmd; scripts/isolation.sh gives ULPF's own second opinion via ULPF_BIN=/Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T140426Z-33371/.cargo-target/release/ulpf)

## container
image=ulpf:static size_bytes=3965424 build_exit=0 run_exit=0 events_emitted=304 (cmds: raw/container-build.cmd, raw/container-run.cmd)

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
commands run: 9, total wall time: 105.3s
COLD START: PASS

## memory
peak RSS: 1528352 KB; slope over last minute (or full run if shorter): 66739.2 KB/s; series: raw/memory-rss.tsv (exit=0)

## kill_recovery
run-to-completion baseline: exit=0 events=5000000 (cmd: raw/kr-baseline.cmd)
killed after 5s: partial events=1160192 (note=killed)
verify after kill: store 0cc29d33dab54f5d4251bc5bb1fa6e75 genesis 60fdb098bc9316aca68e557b0a02cdb839a2d01ffb64d514e8a467f0f177233e
verified 1165312 records, 0 corrupt
chain ok (head bfc2b3fb2ae6840b9162b3e349ad5c2ede0e51411ad194373e6537e2b5f42da6) (exit=0)
restart: exit=0 final_events=5000000 vs baseline=5000000 -> consistent (cmd: raw/kr-resume.cmd)
restarts cleanly: yes

raw output and exact commands: /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T140426Z-33371/raw
