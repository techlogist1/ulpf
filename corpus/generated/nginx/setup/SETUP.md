# nginx corpus generation — reproduce in under 5 minutes

**Host it was generated on**: Apple M1 Pro (arm64), OrbStack, Docker CLI 29.4.0.
**Image**: `nginx:1.27-alpine`, digest
`sha256:65645c7bb6a0661892a8b03b89d0743208a18dd2f3f17a54ef4b76fb8e2f2a10`.
**nginx version** (from the container, `nginx -v`): `nginx/1.27.5`.

This directory is one leg of a single shared stack (nginx×2 + HAProxy + a syslog listener +
a traffic-driving client + a packet sniffer) — `docker-compose.yml`/`traffic.sh` here are
identical to the copies in `../../haproxy/setup/` and `../../zeek/setup/`. Run the compose
file once and all three tools' deliverables are produced in one pass; these steps show the
nginx-only subset if you just want fresh `access.log`/`error.log`.

## Files (this directory)
- `docker-compose.yml` — full stack: `nginx1`, `nginx2` (the two backends), `haproxy`,
  `sysloglistener` (tiny UDP:514 receiver for HAProxy's syslog output), `client`
  (traffic generator), `sniffer` (packet capture, shares `client`'s network namespace).
- `nginx-conf/default.conf` — locations returning 200/301/404/500/(fallthrough 404), an
  HTTPS server block on 443 with a self-signed cert, `client_max_body_size 512k` (exercises
  413s), `client_header_timeout 5s`.
- `traffic.sh` — curl/dig/nc loops run inside the `nicolaka/netshoot` `client` container:
  mixed methods/paths/UAs/referers through HAProxy and direct-to-backend, TLS requests,
  varied-size POST bodies, and deliberately malformed requests (plain HTTP on 443, oversized
  body/header, garbage request lines, raw non-UTF-8 bytes in the URI, bad `Host:` headers).

nginx's own self-signed cert (`nginx-certs/server.crt`/`server.key`, generated locally with
`openssl`, not third-party) is **not** committed here since it embeds a private key — the
command below regenerates it fresh in seconds.

## Exact commands, in order

```sh
cd corpus/generated/nginx/setup
mkdir -p nginx-certs logs/nginx/nginx1 logs/nginx/nginx2

# 1. self-signed cert for the 443 server block (fresh each run, not committed)
openssl req -x509 -newkey rsa:2048 -keyout nginx-certs/server.key \
  -out nginx-certs/server.crt -days 3 -nodes -subj "/CN=nginx.corpus.local"

# 2. bring up just the two nginx backends + traffic client (skip haproxy/sysloglistener/
#    sniffer if you only want nginx logs, not the full haproxy+zeek pass)
docker compose up -d nginx1 nginx2 client

# 3. drive traffic (this alone took ~4.5 minutes end to end in the full generation run,
#    including the HAProxy-routed loop this trimmed invocation skips — direct-to-backend-only
#    traffic against just nginx1/nginx2 finishes in well under 2 minutes)
docker compose exec -T client sh /traffic.sh

# 4. logs are already on the host via the bind mount (./logs/nginx/nginx1, ./logs/nginx/nginx2)
ls -la logs/nginx/nginx1 logs/nginx/nginx2

# 5. teardown
docker compose down -v --remove-orphans
```

For the **full** three-tool generation (what actually produced everything under
`corpus/generated/{nginx,haproxy,zeek}`), use `../../haproxy/setup/SETUP.md` instead — it
brings up the whole stack (HAProxy in front of both backends, the backend-down test, TLS
passthrough, the packet capture for Zeek) in one sequence.

## Traffic actually run (this generation)

`traffic.sh`, full sequence, against HAProxy + both backends directly: 500 mixed-method/path
requests through HAProxy, 300 more split direct-to-backend, 160 TLS requests, 60 varied-size
POSTs, ~30 malformed/edge-case requests (oversized body → 413, oversized header/garbage
request line/raw bytes via `nc`, bad `Host:` header, plain-HTTP-on-443), plus `dig` lookups —
**wall time ~4.5 minutes** for the script itself, plus HAProxy's own `GET /ok` health checks
every 2s running for the whole ~53-minute container lifetime (dominant source of low-entropy
access-log lines). Final counts: `nginx1-access.log` 1,548 lines, `nginx1-error.log` 126,
`nginx2-access.log` 1,407, `nginx2-error.log` 189.

## Teardown

```sh
docker compose down -v --remove-orphans
```

No image was built for this task — `nginx:1.27-alpine` and `nicolaka/netshoot:latest` are
left in the local image cache (needed to re-run in under 5 minutes; re-pulling `netshoot`
alone, at ~900 MB, would blow the time budget). Remove them only if reclaiming disk space
matters more than fast re-runs:

```sh
docker rmi nginx:1.27-alpine nicolaka/netshoot:latest   # optional
```

## Live demo re-run: measured, 2 min 09 s end to end

`traffic-quick.sh` is `traffic.sh` with the four loop counts cut (500/300/80/60 →
60/40/12/10) and nothing else changed. The whole three-tool cycle was actually re-run with it
on this host on **2026-09-05, 09:48:24–09:50:33 UTC**, so the timing below is measured, not
estimated. The three `setup/` directories each hold one leg's files, so assemble them into one
working directory first (that is how the real run was laid out too):

```sh
REPO=/path/to/ulpf                       # repo root
mkdir -p /tmp/ulpf-demo && cd /tmp/ulpf-demo
cp -R "$REPO"/corpus/generated/nginx/setup/nginx-conf .
cp -R "$REPO"/corpus/generated/haproxy/setup/haproxy-conf .
cp "$REPO"/corpus/generated/haproxy/setup/syslog_listener.py .
cp "$REPO"/corpus/generated/zeek/setup/run_zeek.sh .
cp "$REPO"/corpus/generated/nginx/setup/docker-compose.yml .
cp "$REPO"/corpus/generated/nginx/setup/traffic-quick.sh .
mkdir -p nginx-certs logs/nginx/nginx1 logs/nginx/nginx2 logs/haproxy pcap

# mount the quick script where the compose file expects traffic.sh, under its own project name
sed -i '' 's/^name: ulpf-corpus-gen$/name: ulpf-corpus-demo/' docker-compose.yml
sed -i '' 's|./traffic.sh:/traffic.sh:ro|./traffic-quick.sh:/traffic.sh:ro|' docker-compose.yml

openssl req -x509 -newkey rsa:2048 -keyout nginx-certs/server.key \
  -out nginx-certs/server.crt -days 3 -nodes -subj "/CN=nginx.corpus.local"

docker compose up -d                                                        #  9 s
docker compose exec -d sniffer sh -c 'tcpdump -i any -w /pcap/capture.pcap 2>/pcap/tcpdump.log'
docker compose exec -T client sh /traffic.sh &                              # 48 s
sleep 25; docker compose stop nginx2; sleep 20; docker compose start nginx2 # real outage
wait
docker compose exec -T sniffer sh -c 'pkill -INT tcpdump; sleep 2'
sh ./run_zeek.sh                                                            # 11 s
docker compose down -v --remove-orphans                                     # ~12 s
```

**Measured total: 129 s**, with the five images already in the local cache. What that run
produced (real output, kept in scratch, not committed): nginx access 130 + 94 lines, nginx
error 23 + 79; `haproxy.log` 200 lines including **8 real `is DOWN` / `is UP` / maintenance
transitions**; Zeek `conn.log` 508, `dns.log` 336, `http.log` 140, `ssl.log` 33, `files.log`
132, `weird.log` 19. tcpdump: 2,342 packets captured, 2,996 received by filter, 0 dropped.
Teardown verified with `docker ps -a --filter name=ulpf-corpus` (empty).

The committed corpus files come from the **full** `traffic.sh` run described above, not from
this quick one.
