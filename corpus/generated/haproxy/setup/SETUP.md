# HAProxy corpus generation — reproduce in under 5 minutes

**Host it was generated on**: Apple M1 Pro (arm64), OrbStack, Docker CLI 29.4.0.
**Image**: `haproxy:2.9-alpine`, digest
`sha256:3e29449a6beed63262e36104adf531b4e41b359f61937303f5ea8607987b3748`.
**HAProxy version** (from the container, `haproxy -v`): `HAProxy version 2.9.15-e872a3f
2025/03/21`.

This is the full stack: two nginx backends, HAProxy load-balancing across them, a tiny
`python3` UDP:514 syslog listener (HAProxy's Alpine image has no local syslog daemon, so
`option httplog` has to go somewhere), a traffic-driving client, and a packet sniffer (feeds
`../../zeek/setup/`). Running this once produces every deliverable under
`corpus/generated/{nginx,haproxy,zeek}`.

## Files (this directory)
- `docker-compose.yml` — the full 6-container stack (see above).
- `haproxy-conf/haproxy.cfg` — `fe_http`/`be_nginx` (HTTP, round-robin, `option httpchk GET
  /ok`, health check every 2s), `fe_tls`/`be_nginx_tls` (TCP passthrough on 443), a
  `listen stats` section on `:8404` (`/stats`), and `log sysloglistener:514 local0 info`.
- `syslog_listener.py` — 8-line UDP:514 receiver, appends every raw datagram to
  `/out/haproxy.log`.
- `traffic.sh` — same script as `../../nginx/setup/traffic.sh` (identical file).

## Exact commands, in order (what actually produced this corpus)

```sh
cd corpus/generated/haproxy/setup
mkdir -p ../../nginx/setup/nginx-certs logs/nginx/nginx1 logs/nginx/nginx2 logs/haproxy pcap

# 1. self-signed cert for nginx's 443 server block
openssl req -x509 -newkey rsa:2048 \
  -keyout ../../nginx/setup/nginx-certs/server.key \
  -out ../../nginx/setup/nginx-certs/server.crt \
  -days 3 -nodes -subj "/CN=nginx.corpus.local"

# 2. bring up the whole stack
docker compose up -d nginx1 nginx2 sysloglistener haproxy client sniffer

# 3. start the packet capture (feeds ../../zeek/setup/run_zeek.sh)
docker compose exec -d sniffer tcpdump -i any -w /pcap/capture.pcap

# 4. drive traffic (~4.5 minutes; run in the foreground so step 5 lands mid-run)
docker compose exec -T client sh /traffic.sh &
TRAFFIC_PID=$!

# 5. ~15-20s in, take nginx2 down for ~30s to force real HAProxy health-check failover
#    (this is what produced the "is DOWN"/"is UP" lines in haproxy.log)
sleep 20
docker compose stop nginx2
sleep 30
docker compose start nginx2

wait "$TRAFFIC_PID"

# 6. stop the capture cleanly
docker compose exec -T sniffer sh -c 'pkill -INT tcpdump; sleep 2'

# 7. logs are already on the host: ./logs/haproxy/haproxy.log, ./logs/nginx/*, ./pcap/capture.pcap
ls -la logs/haproxy logs/nginx/nginx1 logs/nginx/nginx2 pcap

# 8. teardown
docker compose down -v --remove-orphans
```

Then run `../../zeek/setup/run_zeek.sh` against `pcap/capture.pcap` to produce the Zeek logs
(see that directory's `SETUP.md`).

Total wall time for steps 1–7: cert generation ~1s, compose up (images already pulled)
~10–15s, traffic + backend-down window ~4.5 min, capture stop ~2s — **comfortably under
5 minutes** end to end, provided the five images are already in the local cache (they are,
after the first run — see Teardown below).

## Traffic actually run (this generation)

Same `traffic.sh` as the nginx leg (500 HAProxy-routed + 300 direct-to-backend requests, 160
TLS, 60 varied POSTs, ~30 malformed requests, DNS lookups), **plus** a real ~34-second
`nginx2` outage (`docker compose stop nginx2` / `start nginx2`) while HAProxy's health checker
(`inter 2s fall 2 rise 2`) and live traffic kept probing it — this is what produced the real
`Server be_nginx/nginx2 is DOWN, reason: Layer4 timeout...` / `...is UP/READY...` transitions
in `haproxy.log`. Final count: `haproxy.log` 2,356 lines.

## Teardown

```sh
docker compose down -v --remove-orphans
```

Verified after the real run with `docker ps -a --filter name=ulpf-corpus-gen` (empty) and
`docker images`. No image was built for this task; `nginx:1.27-alpine`, `haproxy:2.9-alpine`,
`python:3.12-alpine`, `nicolaka/netshoot:latest` and `zeek/zeek:latest` were left in the local
cache — every one is needed to re-run the full nginx+haproxy+zeek generation in under 5
minutes. Remove them only if reclaiming disk space matters more than fast re-runs:

```sh
docker rmi haproxy:2.9-alpine python:3.12-alpine   # optional; nginx/netshoot/zeek covered
                                                     # by the other two SETUP.md teardown notes
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
