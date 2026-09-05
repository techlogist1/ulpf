# PROVENANCE — corpus/generated/nginx (label: gen-nginx-haproxy-zeek)

All files below are **generated**, not fetched from a third party: a real `nginx:1.27-alpine`
(nginx 1.27.5) container ran on this machine in front of a real HAProxy load balancer, driven
by real curl/dig/nc traffic from a second container, and its own `access.log` / `error.log`
were copied out byte-for-byte from the bind-mounted log directory. Nothing here is hand-written
or synthetic. See `setup/SETUP.md` for the exact reproduction steps.

- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack, Docker 29.4.0
- **Generation time**: 2026-09-05, containers up 08:46 UTC, traffic ran (two takes — the first
  was interrupted mid-run when the driving shell process was killed, so it was re-run cleanly
  start to finish) roughly 08:47–09:35 UTC, teardown 09:39 UTC. Log timestamps span
  `08:47:28` to `09:38:06` UTC (nginx1); the tail of that window is HAProxy's own health-check
  probes (`GET /ok` every 2s to each backend), which keep both nginx access.logs growing for as
  long as the stack is up, independent of the curl-driven traffic.
- **Tool**: nginx `1.27.5` (`nginx -v` inside the container)
- **Image**: `nginx:1.27-alpine`, digest
  `sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10`, arm64 native (no
  QEMU emulation)
- **How fetched**: not fetched — generated in-place; `access.log`/`error.log` were written by
  the container straight into a bind-mounted host directory (`./logs/nginx/<name>` in
  `setup/docker-compose.yml`), then copied byte-for-byte from there into this directory. No
  `docker cp` scraping through stdout was involved.
- **Licence**: n/a — no third-party file copied; only the official `nginx:1.27-alpine` Docker
  Hub image was executed.
- **What was anonymised**: nothing. Every client IP is the private bridge address of the
  `client`/`sniffer`/HAProxy containers on the compose network (`192.168.148.0/24`, RFC 1918,
  assigned by Docker's bridge driver at `docker compose up` time — not a real client and not
  scrubbed after the fact). User-Agents, referers and paths are exactly what `setup/traffic.sh`
  sent.

## Files

| file | source | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `nginx1-access.log` | generated locally: `nginx:1.27-alpine` container (host `nginx1`), real HTTP/TLS traffic (see below) | image digest `sha256:65645c…f2a10`; nginx 1.27.5 | `/var/log/nginx/access.log` inside the container (bind-mounted) | n/a (own output) | none (private bridge IPs only, see above) | generated | 1,548 | bind mount, then `cp` |
| `nginx1-error.log` | generated locally: same container | same | `/var/log/nginx/error.log` inside the container (bind-mounted) | n/a | none | generated | 126 | bind mount, then `cp` |
| `nginx2-access.log` | generated locally: `nginx:1.27-alpine` container (host `nginx2`, second HAProxy backend) | same | `/var/log/nginx/access.log` inside the container (bind-mounted) | n/a | none | generated | 1,407 | bind mount, then `cp` |
| `nginx2-error.log` | generated locally: same container | same | `/var/log/nginx/error.log` inside the container (bind-mounted) | n/a | none | generated | 189 | bind mount, then `cp` |
| `setup/docker-compose.yml` | authored for this task | — | — | n/a (original) | n/a | generated | 59 | written directly |
| `setup/nginx-conf/default.conf` | authored for this task | — | — | n/a (original) | n/a | generated | 54 | written directly |
| `setup/traffic.sh` | authored for this task | — | — | n/a (original) | n/a | generated | 101 | written directly |
| `setup/traffic-quick.sh` | authored for this task: `traffic.sh` with the four loop counts cut (500/300/80/60 -> 60/40/12/10), nothing else changed; actually run end to end to time the 2 min 09 s demo path | — | — | n/a (original) | n/a | generated | 102 | written directly (`sed` from `traffic.sh`) |
| `setup/SETUP.md` | authored for this task | — | — | n/a (original) | n/a | generated | — | written directly |

## Traffic driving the logs

`setup/traffic.sh`, run inside a `nicolaka/netshoot` client container against both HAProxy
(`http://haproxy/...`) and each nginx backend directly (`http://nginx1/...`,
`http://nginx2/...`):
- 500 requests mixing `GET/POST/PUT/DELETE/HEAD/OPTIONS`, a dozen paths (`/`, `/ok`, `/old`
  → 301, `/missing` → 404, `/boom` → 500, `/api/`, `/upload`, `/slow`, `/admin` → 404 via
  fall-through, `/nope/deep/path?x=1&y=2`, `/ok/../ok`, `/%2e%2e/etc/passwd`), 7 user agents
  (curl, two browser UAs, `python-requests`, `Go-http-client`, `sqlmap`, none) and 5 referers,
  routed through HAProxy
- 300 more of the same, split evenly direct-to-backend (bypassing HAProxy)
- 80×2 TLS requests (`https://nginx1/`, `https://nginx2/`, `https://haproxy/`) against the
  self-signed cert on 443
- 60 POSTs with random-sized bodies (10–4000 bytes) to `/upload`
- malformed/edge-case requests: plain HTTP sent to the TLS port on both backends (5× each);
  a 700 KB POST body against the 512 KB `client_max_body_size` limit (→ 413, both backends);
  a 20 KB header value via raw `nc` (oversized-header attempt); a garbage request line and raw
  TLS-looking bytes via raw `nc`; an oversized/malformed `Host:` header; a raw non-UTF-8 byte
  sequence in the URI via raw `nc`
- `dig` lookups (both resolvable service names and two deliberately-unresolvable hostnames)
  for DNS variety in the paired Zeek capture

Plus, independently of `traffic.sh`, HAProxy's own health checks (`GET /ok` every 2 seconds to
each backend, `option httpchk`) run for the whole lifetime of the stack — the dominant source
of low-entropy access-log lines (`HTTP/1.0`, UA `-`, referer `-`) — and one deliberate backend
outage: `nginx2` was stopped for ~34s (`docker compose stop nginx2` / `start nginx2`) while
health checks and live traffic continued, producing HAProxy `is DOWN` / `is UP` transitions
(see `corpus/generated/haproxy/PROVENANCE.md`) and, in `nginx2-error.log`/`nginx2-access.log`,
a gap in nginx2's own log during the outage window.

## Baseline validation

See `corpus/generated/haproxy/PROVENANCE.md` for the full validation write-up (ran once,
covering nginx + haproxy + zeek together, against the pinned baseline binary). Result for
all four nginx files: **100% `no_parser`** (3,270 framed/emitted events across the 4 files,
0 `detected`, 0 `parsed`, 0 `parse_failed`) — exactly the expected/correct result, since
nginx's combined access-log and its error-log format are outside the twelve shipped parsers.

## Re-validation against the current release binary (`target/release/ulpf`, per file)

Re-run 2026-09-05 after the Rust team's `mappings/ocsf.toml` change landed. The current
release binary loads the live repo cleanly (`ulpf check` → `12 parsers, 1 mappings loaded;
0 problems`), so the `[entities]` caveat recorded in `../haproxy/PROVENANCE.md` against the
older pinned baseline **no longer applies**; the numbers below come from the unmodified
repo `parsers/` and `mappings/`.

```
ulpf run <file> --store <scratch> --output <scratch>.jsonl --infer-threshold 0
```

| file | framed | detected | no_parser | parsed | parse_failed |
|---|---|---|---|---|---|
| `nginx1-access.log` | 1548 | 0 | 1548 | 0 | 0 |
| `nginx1-error.log` | 126 | 0 | 126 | 0 | 0 |
| `nginx2-access.log` | 1407 | 0 | 1407 | 0 | 0 |
| `nginx2-error.log` | 189 | 0 | 189 | 0 | 0 |

Identical to the earlier baseline run: **100% `no_parser`, 0 `parse_failed`** — the correct
result for a format outside the twelve. `time_from_receipt` and `class_unknown` equal the
event count in every file (no vendor timestamp extracted, no class rule matched, because no
parser claimed the line); `sub_matched`/`sub_no_match`/`sub_uncovered`/`unmapped_fields`/
`utf8_lossy` are all 0.

## Inference (`ulpf infer <file> --decisions`) — the live demo material

This is the half that matters for these files: no parser claims them, so the inference engine
is what turns them into a reviewable proposal.

| file | lines | lines covered | templates | unmatched (by reason) |
|---|---|---|---|---|
| `nginx1-access.log` | 1548 | 1548 | 1 | 0 |
| `nginx2-access.log` | 1407 | 1407 | 1 | 0 |
| `nginx1-error.log` | 126 | 118 | 2 | 8 (`below_support` 7, `no_template` 1) |
| `nginx2-error.log` | 189 | 175 | 8 | 14 (`below_support` 12, `no_template` 2) |

Both access logs collapse to **one template covering every line, verified 1548/1548 and
1407/1407 through the real parser** — the combined log format recovered exactly, including
the optional trailing field:

```
{ip1:ipv4} - - [{timestamp:timestamp}] {quoted1:quoted} {int1:int} {int2:int} {quoted2:quoted} {quoted3:quoted}{? {quoted4:quoted}}
```

with `contains = ["HTTP"]` picked as the matcher signature (`HTTP` appears in 1546/1548
lines) and `syslog = false` (0/1548 lines carry a syslog header). The `{? ...}` optional group
is real: the trailing `"-"` referer/UA pair is present on 99 of 1548 lines only.

The error logs are the harder, more honest case: `nginx2-error.log` yields 8 templates — one
for the dominant `open() ... failed (2: No such file or directory)` line (support 117,
verified 117) and seven worker-lifecycle variants (`start worker process {process:int}`,
`signal {signal:int} {word1:word} received{? from {from:int}}`, `worker process
{process:int} exited with code {code:int}`, `... shutting down`, `exiting`, `exit`). 12 of the
14 unmatched lines are singleton clusters below `min_support 3` — correctly left out of the
proposal rather than guessed at, which is exactly the behaviour a reviewer should see.
