tool binary: /private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/ulpf-baseline (EVAL_TOOL_BIN override, not built by this run)
# scorecard: ulpf (20260905T092307Z)
tool config: eval/tools/ulpf.toml
quick mode: yes
threads: 7

## throughput
input: QUICK MODE: 500,000-line slice of bench/mixed-5000000.log (159600178 bytes)
run 1: exit=0 wall=3.269s events=497607 events/s=152220 MB/s=46.6 (cmd: raw/throughput-1.cmd)
run 2: exit=0 wall=2.561s events=497607 events/s=194302 MB/s=59.4 (cmd: raw/throughput-2.cmd)
run 3: exit=0 wall=2.481s events=497607 events/s=200567 MB/s=61.3 (cmd: raw/throughput-3.cmd)
median events/s across 3 runs: 194302
NOTE: quick mode, under load; the lead re-runs on a quiet machine.

## correctness
check_point: total=13 matched=13 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-check_point.cmd, diff: raw/correctness-check_point.diff)
cisco_asa: total=18 matched=17 mismatched=1 missing=0 pct=94.4 (exit=0, cmd: raw/correctness-cisco_asa.cmd, diff: raw/correctness-cisco_asa.diff)
cisco_ios: total=27 matched=25 mismatched=2 missing=0 pct=92.6 (exit=0, cmd: raw/correctness-cisco_ios.cmd, diff: raw/correctness-cisco_ios.diff)
fortinet_fortigate: total=7 matched=7 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-fortinet_fortigate.cmd, diff: raw/correctness-fortinet_fortigate.diff)
juniper_srx: total=16 matched=16 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-juniper_srx.cmd, diff: raw/correctness-juniper_srx.diff)
openvpn: total=28 matched=28 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-openvpn.cmd, diff: raw/correctness-openvpn.diff)
palo_alto_panos: total=19 matched=19 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-palo_alto_panos.cmd, diff: raw/correctness-palo_alto_panos.diff)
pfsense_filterlog: total=18 matched=18 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-pfsense_filterlog.cmd, diff: raw/correctness-pfsense_filterlog.diff)
sonicwall: total=15 matched=15 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-sonicwall.cmd, diff: raw/correctness-sonicwall.diff)
sophos_xg: total=15 matched=15 mismatched=0 missing=0 pct=100.0 (exit=0, cmd: raw/correctness-sophos_xg.cmd, diff: raw/correctness-sophos_xg.diff)
squid_access: total=27 matched=24 mismatched=3 missing=0 pct=88.9 (exit=0, cmd: raw/correctness-squid_access.cmd, diff: raw/correctness-squid_access.diff)
suricata_eve: total=11 matched=9 mismatched=2 missing=0 pct=81.8 (exit=0, cmd: raw/correctness-suricata_eve.cmd, diff: raw/correctness-suricata_eve.diff)
TOTAL: events=214 matched=206 mismatched=8 missing=0 pct=96.3%
scope: only fixture keys observable in a tool's *output* are checked (normalized/time/time_policies/parser/status/sub); 'fields'/'absent' describe the vendor-native parse stage, out of scope for a black-box harness -- see fixtures/README.md and docs/evaluation.md.

## raw_preservation
verify: verified 18 records, 0 corrupt (exit=0, cmd: raw/rawpres-verify.cmd)
records verified: 18/18 sampled ids (mismatches: 0); framing rule: docs/evaluation.md#raw_preservation

## unknown_format
exit=0 events_emitted=250 proposals_written=1 (in /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T092307Z/raw/unknown.pending) (cmd: raw/unknown.cmd)

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
sampled 0 socket observation(s) over the run (pid 53652, sampler lsof)
(no socket observed)
ISOLATION: PASS
(exit=0, cmd: raw/isolation.cmd; scripts/isolation.sh gives ULPF's own second opinion via ULPF_BIN=/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/ulpf-baseline)

## container
image=ulpf:static size_bytes=3429456 build_exit=0 run_exit=0 events_emitted=236 (cmds: raw/container-build.cmd, raw/container-run.cmd)

## cold_start
$ cargo build --release
$ ./target/release/ulpf check
$ ./target/release/ulpf run samples --store /tmp/ulpf-store --output /tmp/out.jsonl
$ ./target/release/ulpf verify --store /tmp/ulpf-store
$ ./target/release/ulpf raw 3 --store /tmp/ulpf-store
stopping before long-running command: mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl
commands run: 5, total wall time: 142.5s
COLD START: PASS

## memory
peak RSS: 232352 KB; slope over last minute (or full run if shorter): 44692.0 KB/s; series: raw/memory-rss.tsv (exit=0)

## kill_recovery
run-to-completion baseline: exit=0 events=497607 (cmd: raw/kr-baseline.cmd)
killed after 5s: partial events=397312 (note=killed)
verify after kill: verified 471040 records, 0 corrupt (exit=0)
restart: exit=0 final_events=894919 vs baseline=497607 -> DOUBLE-COUNTED (894919 > 497607) (cmd: raw/kr-resume.cmd)
restarts cleanly: yes

raw output and exact commands: /Users/lokavyasingh/Documents/dev/ssh hackathon/eval/results/ulpf-20260905T092307Z/raw
