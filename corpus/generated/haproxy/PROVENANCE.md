# PROVENANCE — corpus/generated/haproxy (label: gen-nginx-haproxy-zeek)

All files below are **generated**, not fetched from a third party: a real `haproxy:2.9-alpine`
container load-balanced real HTTP/TCP traffic across two real nginx backends on this machine,
logging in `option httplog`/`tcplog` format over UDP syslog to a tiny `python3` listener, and
the resulting stream was copied out byte-for-byte. Nothing here is hand-written or synthetic.
See `setup/SETUP.md` for the exact reproduction steps.

- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack, Docker 29.4.0
- **Generation time**: 2026-09-05, containers up 08:46 UTC, traffic 08:47–09:35 UTC (two takes,
  see nginx `PROVENANCE.md`), teardown 09:39 UTC. Log timestamps span `08:47:31` to
  `09:35:xx` UTC.
- **Tool**: HAProxy `2.9.15-e872a3f` 2025/03/21 (`haproxy -v` inside the container)
- **Image**: `haproxy:2.9-alpine`, digest
  `sha256:3e29449a6beed63262e36104adf531b4e41b359f61937303f5ea8607987b3748`, arm64 native
- **How fetched**: not fetched — generated in-place. HAProxy has no on-disk log file in the
  Alpine image (no local syslog daemon), so `global log sysloglistener:514 local0 info` in
  `setup/haproxy-conf/haproxy.cfg` points its syslog output at a second container running the
  4-line `setup/syslog_listener.py` (a UDP `socket.bind(("0.0.0.0", 514))` receiver), which
  appends each raw datagram, newline-terminated, to a bind-mounted file. That file was then
  copied byte-for-byte into this directory — the exact RFC 3164-framed syslog lines HAProxy
  emitted, not something scraped from container stdout.
- **Licence**: n/a — no third-party file copied; only the official `haproxy:2.9-alpine` and
  `python:3.12-alpine` Docker Hub images were executed, and `syslog_listener.py` is 4 lines
  written for this task.
- **What was anonymised**: nothing. Client IPs are the private bridge address of the
  `client` container (`192.168.148.0/24`, RFC 1918, Docker-assigned). `<NNN>` is HAProxy's own
  real syslog priority value (facility `local0` × severity), not redacted.

## Files

| file | source | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `haproxy.log` | generated locally: `haproxy:2.9-alpine` container, real traffic across two nginx backends (see below) | image digest `sha256:3e29449a…7b3748`; HAProxy 2.9.15 | HAProxy's `option httplog`/`tcplog` UDP syslog stream, received by `syslog_listener.py` and appended to `/out/haproxy.log` (bind-mounted) | n/a (own output) | none (see above) | generated | 2,356 | UDP syslog listener, bind mount, then `cp` |
| `setup/docker-compose.yml` | authored for this task | — | — | n/a (original) | n/a | generated | 59 | written directly |
| `setup/haproxy-conf/haproxy.cfg` | authored for this task | — | — | n/a (original) | n/a | generated | 53 | written directly |
| `setup/syslog_listener.py` | authored for this task | — | — | n/a (original) | n/a | generated | 8 | written directly |
| `setup/traffic.sh` | authored for this task (same script as `corpus/generated/nginx/setup/traffic.sh`) | — | — | n/a (original) | n/a | generated | 101 | written directly |
| `setup/traffic-quick.sh` | authored for this task: `traffic.sh` with the four loop counts cut (500/300/80/60 -> 60/40/12/10); actually run end to end to time the 2 min 09 s demo path | — | — | n/a (original) | n/a | generated | 102 | written directly (`sed` from `traffic.sh`) |
| `setup/SETUP.md` | authored for this task | — | — | n/a (original) | n/a | generated | — | written directly |

## What's in the log

- **httplog lines** (`fe_http`/`be_nginx`, `option httplog`): one per HTTP request routed
  through the `fe_http`/`be_nginx` frontend/backend — the 500-request curl loop from
  `traffic.sh` (mixed methods/paths/UAs/referers, round-robin across `nginx1`/`nginx2`) plus
  HAProxy's own `option httpchk GET /ok` health-check traffic against both backends every 2s
  for the whole run.
- **stats-frontend lines**: the `listen stats` section (`stats enable`, `/stats` URI) is
  reachable on `:8404` but was not polled by `traffic.sh` in this run — it is configured and
  live for the demo (`docker compose exec haproxy sh` → `curl localhost:8404/stats`), but no
  request for it appears in this specific capture.
- **health-check state-transition lines**: real `Server be_nginx/nginx2 is DOWN, reason:
  Layer4 timeout...` / `...was DOWN and now enters maintenance...` / `...is UP/READY...` —
  produced by actually running `docker compose stop nginx2` for ~34 seconds
  (`08:49:32`–`08:49:59` in this capture) while HAProxy's health checker (`inter 2s fall 2
  rise 2`) and real client traffic kept hitting it, then `docker compose start nginx2`. This
  is a real container stop/start, not a config toggle or synthetic log line.
- **admin/proxy lifecycle lines**: `Proxy fe_http stopped (cumulated conns: ...)` and similar —
  HAProxy's own startup/shutdown bookkeeping, captured because the syslog listener was up for
  the whole container lifetime.

## Baseline validation (`ulpf run ... --infer-threshold 0`)

Ran against the pinned baseline binary
(`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/ulpf-baseline`).

**One caveat, unrelated to these files**: at generation time, `mappings/ocsf.toml` at
repo HEAD (commit `bb8a10a`, made live by the actual Rust team during this session) carries a
new `[entities]` table that the pinned baseline binary predates — `ulpf check`/`ulpf run`
against `mappings/` as committed fails outright with `unknown field 'entities'` and loads
**zero** mappings, before any file-specific processing happens. This is a real, current
mismatch between the frozen validation binary and the live repo, not something introduced by
this corpus. To still get real per-stage counters (rather than an immediate load-time abort),
`run` below points `--mappings` at a scratch copy of `mappings/ocsf.toml` with the `[entities]`
table removed (`--parsers` still points at the real, unmodified `parsers/`) — nothing under
`mappings/` in the repo was touched. Since `[entities]` only feeds the pivot index at the
normalize stage, this has no effect on the numbers that matter here: `detected`/`no_parser`
come entirely from the parse stage against the real `parsers/`.

```
########## HAPROXY ##########
definitions: 12 parsers loaded, 0 file problems
ulpf: 1 files (0 failed), 0.19 MB, 1178 events in 0.006 s -> 201527 events/s, 31.8 MB/s, 7 worker threads
stages: framed 1178  stored 1178  detected 0  no_parser 1178  parsed 0  parse_failed 0  normalized 1178  emitted 1178 (789688 bytes)
parse_failed by reason: none
signals: sub_matched 0  sub_no_match 0  sub_uncovered 0  time_from_receipt 1178  time_error [none]  class_unknown 1178  enum_other 0  unmapped_fields 0  utf8_lossy 0

########## NGINX (4 files) ##########
ulpf: 4 files (0 failed), 0.32 MB, 3270 events in 0.013 s -> 257978 events/s, 25.2 MB/s, 7 worker threads
stages: framed 3270  stored 3270  detected 0  no_parser 3270  parsed 0  parse_failed 0  normalized 3270  emitted 3270 (2024140 bytes)
signals: ... time_from_receipt 3270  class_unknown 3270  (rest 0)

########## ZEEK, TSV (7 files) ##########
ulpf: 7 files (0 failed), 1.57 MB, 11743 events in 0.020 s -> 596989 events/s, 79.7 MB/s, 7 worker threads
stages: framed 11743  stored 11743  detected 0  no_parser 11743  parsed 0  parse_failed 0  normalized 11743  emitted 11743 (7816497 bytes)

########## ZEEK, JSON (conn/dns/http/ssl) ##########
ulpf: 4 files (0 failed), 3.65 MB, 10216 events in 0.026 s -> 396661 events/s, 141.8 MB/s, 7 worker threads
stages: framed 10216  stored 10216  detected 0  no_parser 10216  parsed 0  parse_failed 0  normalized 10216  emitted 10216 (9514151 bytes)
```

**Result across all three tools: 100% `no_parser`, 0 `detected`, 0 `parsed`, 0 `parse_failed`,
0 `sub_no_match`, 0 `sub_uncovered`** (there is nothing to sub-match with no parser claiming
the line). `time_from_receipt` = every event, `class_unknown` = every event — expected: no
timestamp was extracted from any vendor format and no class rule matched, because nothing
claimed the format. This is exactly the correct, expected outcome for three log formats
outside the twelve shipped parsers.

**3 output lines checked by hand** (all `ulpf.parse_status: "no_parser"`, message carried
verbatim, `time` fell back to receipt time):
1. `haproxy.log` line 1 — httplog line `192.168.148.3:33790 [05/Sep/2026:08:47:31.708]
   fe_http be_nginx/nginx1 0/0/4/14/18 200 139 - - ---- 1/1/0/0/0 0/0 "GET /ok HTTP/1.1"`,
   priority `<134>` preserved verbatim in `message`.
2. `haproxy.log` line 2 — a proxy-lifecycle line (`Proxy fe_http stopped ...`), no request at
   all — confirms `no_parser` handles non-httplog lines from the same source without special
   casing.
3. `nginx1-access.log` line 3 — combined-format line with a real `User-Agent: curl/8.21.0`,
   preserved byte-for-byte in `message`.

**3 `no_parser` lines inspected for what specifically breaks** (all three tools, same root
cause — the format is simply not one of the twelve):
1. **nginx combined access log** — `192.168.148.6 - - [05/Sep/2026:08:47:28 +0000] "GET /ok
   HTTP/1.0" 200 3 "-" "-"`: closest existing parser is `squid_access`, but Squid's native
   format is space-separated fields (`%ts.%03tu %6tr %>a %Ss/%03>Hs ...`), not
   `combined`'s `ip - user [date] "request" status bytes "referer" "ua"` bracket/quote
   layout — no shared literal anchor for the matcher to key on.
2. **HAProxy httplog** — `<134>Sep  5 08:47:31 haproxy[7]: 192.168.148.3:33790
   [05/Sep/2026:08:47:31.708] fe_http be_nginx/nginx1 0/0/4/14/18 200 139 - - ---- 1/1/0/0/0
   0/0 "GET /ok HTTP/1.1"`: has a BSD syslog header like several of the twelve
   (`cisco_ios`, `sonicwall`, `sophos_xg`), but none of those parsers' body grammars expect
   HAProxy's own field set (`client_ip:port [accept_date] frontend backend/server
   Tw/Tc/Tt/Tr/Ta status bytes term_flags conn_counters "http_request"`) — no field-name or
   delimiter overlap once past the syslog header.
3. **Zeek TSV** — `#separator \x09` / a tab-separated data row from `conn.log`: the `#fields`/
   `#types` comment-header lines and the packed positional TSV rows have no analog in any of
   the twelve (nearest structurally is `squid_access`'s space-delimited positional format, but
   Zeek's column count, `#`-prefixed metadata lines, and `\x09` literal separator token are
   unique to it).

## Teardown

`docker compose down -v --remove-orphans` (from `setup/`) — removed all 6 containers
(`nginx1`, `nginx2`, `haproxy`, `sysloglistener`, `client`, `sniffer`) and the compose network.
Verified with `docker ps -a --filter name=ulpf-corpus-gen` (empty) after teardown. No image was
built for this task, so nothing was pruned beyond the containers/network; `nginx:1.27-alpine`,
`haproxy:2.9-alpine`, `python:3.12-alpine`, `nicolaka/netshoot:latest` and `zeek/zeek:latest`
were left in the local image cache because every one of them is needed to reproduce the demo
in under 5 minutes (see each tool's `setup/SETUP.md`).

## Re-validation against the current release binary (`target/release/ulpf`)

Re-run 2026-09-05 after the Rust team's `mappings/ocsf.toml` change landed. **The
`[entities]` caveat recorded above no longer applies**: the current release binary loads the
live repo cleanly (`ulpf check` → `12 parsers, 1 mappings loaded; 0 problems`), so the
numbers below come from the unmodified repo `parsers/` and `mappings/`, with no scratch copy
of anything.

```
ulpf run corpus/generated/haproxy/haproxy.log --store <scratch> --output <scratch>.jsonl --infer-threshold 0
ulpf: 1 files (0 failed), 0.19 MB, 1178 events in 0.006 s -> 185326 events/s, 29.3 MB/s, 7 worker threads
stages: framed 1178  stored 1178  detected 0  no_parser 1178  parsed 0  parse_failed 0  normalized 1178  emitted 1178 (789688 bytes)
parse_failed by reason: none
signals: sub_matched 0  sub_no_match 0  sub_uncovered 0  time_from_receipt 1178  time_error [none]  class_unknown 1178  enum_other 0  unmapped_fields 0  utf8_lossy 0
```

Same result as the baseline run: **100% `no_parser`, 0 `parse_failed`** — correct for a
format outside the twelve. Every Zeek and nginx file was re-run the same way, per file; see
each tool's `PROVENANCE.md` for its table.

### Why 2,356 lines but 1,178 events

`haproxy.log` is 2,356 lines and ULPF frames exactly 1,178 events from it — one event per
two lines, not a framing bug. `setup/syslog_listener.py` writes `data.rstrip(b"\x00") +
b"\n"` for each UDP datagram, and HAProxy's syslog datagrams already end in `\n`, so every
record lands as `<record>\n\n`: 1,178 records, 1,178 blank separator lines. The bytes are
left exactly as the listener wrote them (nothing was stripped after the fact — that would be
editing captured output); ULPF's framing rule folds the trailing empty line into the record
it follows, which is why `framed` = the true record count. `grep -c '^$' haproxy.log` = 1,178
confirms it.

## Inference (`ulpf infer haproxy.log --decisions`) — the live demo material

```
# 1178 lines, 1175 used, 7 templates, 3 unmatched {"below_support": 3}
```

**7 templates covering 1,175 of 1,178 lines**, 3 unmatched and all three are singleton
clusters below `min_support 3` (left out of the proposal rather than guessed at). The two
dominant templates are the real `option httplog` and `option tcplog` grammars, recovered
field-for-field:

```
T3  support 1062 verified 1062  haproxy[{pid:int}]: {ip1:ipv4}:{port1:port} {rule:word} fe_http be_nginx/{word1:word} {int1:int}/{int2:int}/{int3:int}/{int4:int}/{int5:int} {int6:int} {int7:int} - - ---- {int8:int}/{int9:int}/{int10:int}/{int11:int}/{int12:int} {int13:int}/{int14:int} {quoted1:quoted}
T5  support   81 verified   81  haproxy[{pid:int}]: {ip1:ipv4}:{port1:port} {rule:word} fe_tls be_nginx_tls/{word1:word} {int1:int}/{int2:int}/{int3:int} {int4:int} -- {int5:int}/{int6:int}/{int7:int}/{int8:int}/{int9:int} {int10:int}/{int11:int}
```

T3 is `Tq/Tw/Tc/Tr/Tt` + status + bytes + termination flags + the five connection counters +
`"<request>"`; T5 is the shorter `tcplog` shape (three timers, no HTTP request) — the engine
split them because the constant tokens `fe_http be_nginx/` and `fe_tls be_nginx_tls/` differ,
which is exactly the right split. The remaining five templates are the operational lines:
`Proxy {proxy:word} stopped (cumulated conns: FE: {int1:int}, BE: {int2:int}).` (support 10),
the resolver lines (`administratively READY thanks to valid DNS answer.`, `changed its IP from
(none) to {dst_ip:ipv4} by {text1:text}.`), and two health-check state-transition shapes
including the real `Server {server:word}/nginx2 {nginx2:word} DOWN and {word1:word}
{text1:text}.` produced by the deliberate `nginx2` outage. That last one is a good review
exercise: the engine froze the literal `nginx2` into the pattern because it only ever saw one
server go down — a human reviewer should widen it to a slot before approving.
