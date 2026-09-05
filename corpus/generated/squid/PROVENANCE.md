# PROVENANCE — corpus/generated/squid (label: gen-squid)

All files below are **generated**, not fetched from a third party: a real Squid
6.13 forward proxy and a real nginx origin ran as real Docker containers on this
machine, and real curl/netcat traffic was driven through the proxy (`-x`).
Nothing here is hand-written or synthetic. See `setup/SETUP.md` for the exact
reproduction steps and full command history.

- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack, Docker
  29.4.0, Docker Compose v5.1.2, `linux/aarch64` containers (native, no QEMU
  emulation)
- **Generation time**: 2026-09-05, 08:27:22 UTC (stack up) through 08:48:59 UTC
  (final `docker cp`); teardown (`docker compose down -v` + `docker rmi`)
  completed 08:50 UTC, verified nothing left running.
- **Tool**: Squid Cache `6.13` (Ubuntu 24.04.3 LTS package build; `squid -v`
  inside the container), nginx `1.31.5` (`nginx -v` inside the container)
- **Images**:
  - `docker.io/ubuntu/squid:latest`, digest
    `sha256:6a097f68bae708cedbabd6188d68c7e2e7a38cedd05a176e1cc0ba29e3bbe029`
    (Canonical's official multi-arch Squid image; used as the base for a
    2-line Dockerfile that only copies in `squid.conf`)
  - `docker.io/library/nginx:alpine`, digest
    `sha256:72ba65eb42c10344912a84ff42408db7d34f2feb642204570ab8fc5ffd29f1d3`
- **How fetched**: not fetched — generated in-place with `docker compose up`
  + `traffic.sh` per `setup/SETUP.md`. `access.log` and `cache.log` were
  copied out of the container filesystem with `docker cp` (byte-for-byte,
  never scraped from stdout — the entrypoint's `tail -F` to stdout was
  ignored).
- **Licence**: n/a — no third-party file was copied into this repo. Squid
  itself is GPL-2.0-or-later and nginx is BSD-2-Clause, but only their
  *binaries*, from upstream Docker Hub images, were executed; no Squid or
  nginx source was vendored.
- **What was anonymised**: nothing — every field is exactly what Squid wrote.
  The client IP is the Docker bridge gateway address (`192.168.117.1`,
  RFC 1918) throughout since all traffic came from one curl client on the
  host; origin/server IPs are the nginx container's private bridge address
  (`192.168.117.2`). Hostnames (`origin`, `www.example.test`,
  `static.example.test`, `api.example.test`, `blocked.test`, `denied.test`,
  `adtracker.test`) are lab-invented `.test`-TLD names (RFC 2606 reserved),
  not real organizations.

## Files

| file | source | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `access.log` | generated locally: `ubuntu/squid:latest` container, real proxied traffic (see below) | image digest `sha256:6a09…be029`; Squid 6.13 | `/var/log/squid/access.log` inside `ulpf-squid-proxy` | n/a (own output, no third-party file copied) | none (see above) | generated | 16,500 (of 17,124 captured; trimmed for the 2 MB cap, see Trimming below) | `docker cp ulpf-squid-proxy:/var/log/squid/access.log` |
| `cache.log` | generated locally: same container, Squid's own debug/trace log | image digest `sha256:6a09…be029`; Squid 6.13, `squid -k debug` toggled on for one traffic burst | `/var/log/squid/cache.log` inside `ulpf-squid-proxy` | n/a (own output) | none | generated | 20,000 (of 1,392,118 captured; trimmed hard for the cap, see Trimming below) | `docker cp ulpf-squid-proxy:/var/log/squid/cache.log` |
| `setup/docker-compose.yml` | authored for this task | — | — | n/a (original) | n/a | generated | 27 | written directly |
| `setup/squid/Dockerfile` | authored for this task | — | — | n/a (original) | n/a | generated | 4 | written directly |
| `setup/squid/squid.conf` | authored for this task (references Squid's own built-in ACL/logformat vocabulary) | — | — | n/a (original) | n/a | generated | 55 | written directly |
| `setup/nginx/nginx.conf` | authored for this task | — | — | n/a (original) | n/a | generated | 27 | written directly |
| `setup/nginx/html/**` | authored for this task (placeholder static content: html/css/js/png/txt/json + a 1 MiB `/dev/urandom` filler binary) | — | — | n/a (original) | n/a | generated | small | written directly / `dd if=/dev/urandom` |
| `setup/traffic.sh` | authored for this task | — | — | n/a (original) | n/a | generated | 66 | written directly |
| `setup/SETUP.md` | authored for this task | — | — | n/a (original) | n/a | generated | — | written directly |

## Traffic actually driven (all real, all local — no internet egress)

curl through the proxy (`-x http://127.0.0.1:3128`) to four virtual hosts
(`origin`, `www.example.test`, `static.example.test`, `api.example.test`, all
aliases of the one nginx container) across GET/HEAD/POST/PUT/DELETE/OPTIONS,
mixing: a small "hot" cacheable set (`/`, `/style.css`, `/app.js`,
`/logo.png`, `/files/report.txt`, `/files/data.json`, `/files/bigfile.bin`)
repeated thousands of times to force real cache behaviour, dynamic/error
paths (`/api/`, `/api/status`, `/error500`, `/error404`), and per-request
unique `/notfound/item-N-<rand>` paths for guaranteed fresh 404 MISSes.
ACL-denied hosts (`blocked.test`, `denied.test`, `adtracker.test`, never
resolved — Squid denies on `dstdomain` before any DNS lookup) for
`TCP_DENIED`. Malformed raw requests sent directly to port 3128 with `nc`
(not through curl) for `NONE_NONE`/400. Conditional GETs with a future
`If-Modified-Since` for `TCP_IMS_HIT`/304. Three bursts total (~15,500 real
requests fired; full command/timing history in `setup/SETUP.md`) against one
long-lived stack, so `access.log` is one contiguous file (`logfile_rotate 0`,
no restarts in between).

Confirmed cache-result tags present in the full capture (and in the
committed, trimmed file): `TCP_MISS`, `TCP_MEM_HIT`, `TCP_HIT` (plain disk
hit — forced via `bigfile.bin`, which is bigger than
`maximum_object_size_in_memory` so its repeats can't be served from RAM),
`TCP_DENIED`, `TCP_IMS_HIT`, `NONE_NONE` — every tag the brief asked for, plus
one bonus (`TCP_IMS_HIT`).

## Trimming (2 MB / 20,000-line cap)

**`access.log`**: full real capture was 17,124 lines / 2.1 MB (kept in full at
`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/gen-squid/access.log.full`
on this machine). 16,700 lines was already 2,109,546 bytes (over the 2 MB
cap), so the committed file is the **first 16,500 lines**, byte-identical,
no line edited, 2,084,033 bytes. Re-validated after the cut: every one of the
6 cache-result tags above still appears (`TCP_MISS` 8,761 / `TCP_MEM_HIT`
6,579 / `TCP_DENIED` 901 / `TCP_IMS_HIT` 150 / `NONE_NONE` 91 / `TCP_HIT` 18).

**`cache.log`**: at Squid's default verbosity, a full ~15,500-request session
produced only ~76 lines of cache.log (checkpointed separately at
`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/gen-squid/checkpoint/cache.log.checkpoint`)
— that is genuinely how quiet Squid's operational log is when nothing is
misconfigured, not a mistake. To produce a substantial `cache.log` for the
corpus, `squid -k debug` was toggled live (a real, documented Squid admin
signal, not a config edit) for the third traffic burst, which alone produced
1,392,118 lines / 145 MB of real `ALL,9`-equivalent debug trace (kept in full
at
`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/gen-squid/cache.log.full`).
The committed file is the **first 20,000 lines** of that real capture,
byte-identical, 2,065,392 bytes — under both caps. This slice starts with the
container's own real startup/config-reload lines (default verbosity) and
transitions into real per-request debug trace (connection state machine,
async call queue, UFS swap-dir maintenance, etc.) once debug mode engaged;
it does not span the whole debug burst (only its first ~18 requests' worth),
since that burst alone was ~1,100 debug lines per HTTP request.

## Baseline validation (`ulpf run ... --infer-threshold 0`)

**`access.log`** (parser: `squid_access`, one of the twelve):
```
ulpf: 1 files (0 failed), 1.99 MB, 16500 events in 0.102 s -> 161177 events/s, 19.4 MB/s, 7 worker threads
stages: framed 16500  stored 16500  detected 16500  no_parser 0  parsed 16500  parse_failed 0  normalized 16500  emitted 16500 (16909751 bytes)
parse_failed by reason: none
signals: sub_matched 16500  sub_no_match 0  sub_uncovered 0  time_from_receipt 0  time_error [none]  class_unknown 0  enum_other 0  unmapped_fields 82500  utf8_lossy 0
```
100% parsed, 0 `no_parser`, 0 `parse_failed`, every `sub` (the packed
`Ss/Hs` cache-result and `Sh/<a` hierarchy fields) matched on every line —
nothing broke. `unmapped_fields` (82,500 = 5 fields × 16,500 events) is
expected: `squid_access`'s own fields (`response_time`, `cache_result` raw
string, `hierarchy` raw string, `hierarchy_code`, `timestamp` as parsed)
aren't all consumed by the OCSF mapping, which is a mapping-stage decision,
not a parser defect. Checked 3 output lines by hand (`GET http://origin/` →
`TCP_MISS/200`; a repeat `GET .../logo.png` → `TCP_MEM_HIT/200`; a unique
`HEAD .../notfound/item-146-16003` → `TCP_MISS/404`) — fields, hierarchy
sub-fields and OCSF `http_request`/`http_response`/`traffic.bytes` all
correct. No `no_parser`/`parse_failed` lines exist to inspect.

**`cache.log`** (no parser covers Squid's debug/trace log — expected and
confirmed):
```
ulpf: 1 files (0 failed), 1.97 MB, 19774 events in 0.064 s -> 310705 events/s, 30.9 MB/s, 7 worker threads
stages: framed 19774  stored 19774  detected 0  no_parser 19774  parsed 0  parse_failed 0  normalized 19774  emitted 19774 (12019414 bytes)
```
100% `no_parser`, exactly as expected for a log format outside the twelve —
`cache.log` is Squid's internal debug stream, not an access/security event
log, and no `ulpf` parser targets it. (19,774 framed events from 20,000
raw lines: the file mixes LF and CRLF terminators — confirmed by `file`,
inherited from the container's line endings — and a handful of continuation
lines fold together under ULPF's framing rule.)

## Teardown

`docker compose down -v` removed both containers and the private network;
`docker rmi ulpf-corpus-squid-proxy:latest ubuntu/squid:latest nginx:alpine`
removed every image pulled/built for this task. Verified with `docker ps -a`
/ `docker images` after teardown: no squid or nginx:alpine containers or
images remain from this generation (other unrelated containers on the host,
belonging to other work, were left untouched).
