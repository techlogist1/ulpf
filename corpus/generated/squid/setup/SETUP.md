# Squid corpus generation — setup & reproduction

Real Squid forward proxy + real nginx origin, both in Docker (OrbStack runtime),
driven by real curl/netcat traffic. `access.log` and `cache.log` are copied out
of the container filesystem with `docker cp` — never scraped from stdout.

## Tool / image versions actually used

- Docker: `Docker version 29.4.0, build 9d7ad9f` (`docker --version`)
- Compose: `Docker Compose version v5.1.2` (`docker compose version`)
- Host: Apple M1 Pro, OrbStack, `linux/aarch64` containers
- Squid: `Squid Cache: Version 6.13` (`docker exec ulpf-squid-proxy squid -v`),
  Ubuntu 24.04.3 LTS package build, from image
  `docker.io/ubuntu/squid:latest`
  digest `sha256:6a097f68bae708cedbabd6188d68c7e2e7a38cedd05a176e1cc0ba29e3bbe029`
  (Canonical's official multi-arch Squid image)
- nginx: `nginx version: nginx/1.31.5` (`docker exec ulpf-squid-origin nginx -v`),
  from image `docker.io/library/nginx:alpine`
  digest `sha256:72ba65eb42c10344912a84ff42408db7d34f2feb642204570ab8fc5ffd29f1d3`

## Files in this directory

- `docker-compose.yml` — two services: `origin` (nginx:alpine, bind-mounted
  config+content) and `proxy` (built from `squid/Dockerfile`), both on a
  private bridge network `squidnet`. `origin` carries network aliases
  `origin`, `www.example.test`, `static.example.test`, `api.example.test` so
  one nginx container answers as four distinct virtual hosts.
- `squid/Dockerfile` — `FROM ubuntu/squid:latest`, copies in `squid.conf`.
- `squid/squid.conf` — `http_port 3128`; `cache_mem 16 MB` +
  `cache_dir ufs /var/spool/squid 100 16 256` so repeat requests actually hit
  cache; `acl blocked_sites dstdomain .blocked.test .denied.test
  .adtracker.test` denied before any DNS lookup (these hostnames are never
  resolved — Squid denies on the ACL alone); `access_log
  /var/log/squid/access.log squid` — the built-in **squid** logformat
  (`%ts.%03tu %6tr %>a %Ss/%03>Hs %<st %rm %ru %[un %Sh/%<a %mt`), not
  combined/common.
- `nginx/nginx.conf` + `nginx/html/**` — static files (`index.html`,
  `style.css`, `app.js`, `logo.png`, `files/report.txt`, `files/data.json`,
  `files/bigfile.bin` — a 1 MiB filler object, bigger than Squid's
  `maximum_object_size_in_memory` so its repeats land on **disk** (plain
  `TCP_HIT`) instead of memory), `/error500` (`return 500`), `/error404`
  (`return 404`), `/api/` (`Cache-Control: no-store`, JSON).
- `traffic.sh` — builds a request plan (hot cacheable paths repeated across
  hosts, dynamic/error paths, unique long-tail 404s, denied-host paths) and
  fires it through the proxy with `xargs -P` for real concurrent traffic;
  then a batch of malformed raw requests via `nc` (→ `NONE_NONE`) and
  conditional `If-Modified-Since` GETs (→ `TCP_IMS_HIT`).

## Exact commands, in order

```bash
cd corpus/generated/squid/setup

# 1. pull/build images and start the stack
docker compose up -d --build

# 2. sanity check: one request end to end
curl -s -o /dev/null -w '%{http_code}\n' -x http://127.0.0.1:3128 http://origin/

# 3. drive traffic (thousands of real requests through the proxy)
#    args: TOTAL_REQUESTS PARALLELISM — ~5000/40 takes ~3.5 minutes wall clock
./traffic.sh 5000 40

# 4. (optional, for a meatier cache.log) toggle Squid's live debug flag,
#    fire a short extra burst, then toggle it back off — this is a real
#    Squid admin operation (SIGUSR2 under the hood), not a config hack:
docker exec ulpf-squid-proxy squid -k debug
./traffic.sh 500 20
docker exec ulpf-squid-proxy squid -k debug

# 5. copy the logs out byte-for-byte (not stdout scraping)
docker cp ulpf-squid-proxy:/var/log/squid/access.log ./access.log.out
docker cp ulpf-squid-proxy:/var/log/squid/cache.log ./cache.log.out

# 6. teardown — leaves nothing running
docker compose down -v
docker rmi ulpf-corpus-squid-proxy:latest ubuntu/squid:latest nginx:alpine
```

## What we actually ran for the committed corpus

Three traffic bursts against one long-lived stack (all through the same
`docker compose up` instance, no restarts in between so `logfile_rotate 0`
kept one contiguous `access.log`):

| burst | requests fired | wall time | purpose |
|---|---|---|---|
| 1 | 7,000 planned → 8,274 actual (incl. warmup + denied-host batch) | 2m 36s | bulk MISS/MEM_HIT/DENIED/NONE_NONE volume |
| 2 | 5,000 planned → 6,016 actual, incl. `bigfile.bin` warmup | 3m 24s | added plain `TCP_HIT` (disk, not memory) samples |
| 3 | 800 planned → 1,246 actual, run with `squid -k debug` on | 42s | inflated `cache.log` with real Squid debug trace output |

Total real requests fired at the proxy across the session: **~15,500**,
producing a **17,124-line** `access.log` and a **1,392,118-line / 145 MB**
`cache.log` (the debug burst alone produces roughly 1,100 debug lines per
HTTP request at `ALL,9`-equivalent verbosity — this is genuinely how verbose
Squid debug logging is, not an error).

The committed `../access.log` and `../cache.log` are the **first** 16,500 /
20,000 lines of those real captures respectively (both cut at a line
boundary, both under the 2 MB / 20,000-line corpus cap — 16,700 lines of
`access.log` was already 2,109,546 bytes, just over the 2 MB cap, hence the
16,500 cut point). The full, untruncated files are kept in the generating
session's scratch directory, not in this repo.

## Re-running live during the demo (under 5 minutes)

```bash
cd corpus/generated/squid/setup
docker compose up -d --build        # ~15s, images already cached after first pull
./traffic.sh 3000 40                # ~2 minutes, thousands of fresh log lines
docker cp ulpf-squid-proxy:/var/log/squid/access.log /tmp/access.log
docker cp ulpf-squid-proxy:/var/log/squid/cache.log /tmp/cache.log
docker compose down -v              # teardown
```

## Teardown (already done for the committed corpus — nothing is left running)

```bash
docker compose down -v
docker rmi ulpf-corpus-squid-proxy:latest ubuntu/squid:latest nginx:alpine
```
Verified after generation: `docker ps -a` and `docker images` show no
squid/nginx-alpine containers or images left from this generation.
