# Zeek corpus generation — reproduce in under 5 minutes

**Host it was generated on**: Apple M1 Pro (arm64), OrbStack, Docker CLI 29.4.0.
**Image**: `zeek/zeek:latest`, digest
`sha256:703f0b22af150d9418739b2a012fbfb5d01ee004aded3bd43b0175010db05928`.
**Zeek version** (from the container, `zeek --version`): `zeek version 8.2.2`.
**Capture tool**: `tcpdump` from `nicolaka/netshoot:latest`, digest
`sha256:b09d9b21381f47a79b3cbcb30da25266dc17186ea00ae65e99fdc51396f48e70`.

Zeek here runs **offline** (`zeek -r`) over a pcap captured while the nginx/HAProxy traffic
from `../../haproxy/setup/SETUP.md` ran — that stack must run first (or you need any other
pcap of real HTTP/DNS/TLS traffic). This directory holds the offline-analysis half only.

## Files (this directory)
- `docker-compose.yml` — same shared stack (only `client` + `sniffer` matter for producing a
  pcap here; see `../../haproxy/setup/SETUP.md` for the full up-sequence with the backend-down
  test).
- `traffic.sh` — same traffic script (drives the DNS/HTTP/TLS mix that ends up in
  `dns.log`/`http.log`/`ssl.log`).
- `run_zeek.sh` — runs `zeek/zeek:latest` twice over `../../../<scratch>/pcap/capture.pcap`
  (TSV form, then `LogAscii::use_json=T` for the JSON form), each into its own bind-mounted
  output directory.

## Exact commands, in order

```sh
# 1. produce a pcap (full instructions: ../../haproxy/setup/SETUP.md steps 1-6);
#    minimally, from that directory:
cd ../../haproxy/setup
docker compose up -d nginx1 nginx2 sysloglistener haproxy client sniffer
docker compose exec -d sniffer tcpdump -i any -w /pcap/capture.pcap
docker compose exec -T client sh /traffic.sh
docker compose exec -T sniffer sh -c 'pkill -INT tcpdump; sleep 2'
docker compose down -v --remove-orphans

# 2. run zeek offline over the capture (this directory)
cd ../../zeek/setup
chmod +x run_zeek.sh
./run_zeek.sh
# -> ./zeek-out-tsv/{conn,dns,http,ssl,files,syslog,weird}.log  (default #fields-headered TSV)
# -> ./zeek-out-json/{conn,dns,http,ssl,files,syslog,weird}.log (LogAscii::use_json=T)
```

**Critical flag**: `run_zeek.sh` calls `zeek -C -r ...` (not plain `zeek -r`). Docker's
virtual `veth` network never computes real TCP/UDP checksums (offloaded to hardware that
never runs for container-internal traffic); without `-C` (Zeek's own `ignore_checksums`
option) Zeek silently discards every packet as checksum-invalid before running any
protocol analyzer, and you get only `conn.log`/`weird.log` with **no**
`dns.log`/`http.log`/`ssl.log` at all. This bit us on the first attempt — confirmed by
re-running with `-C` and seeing `dns.log`/`http.log`/`ssl.log` appear with real content.

Total wall time: capture+traffic per step 1 above (~4.5 min, shared with the haproxy leg —
skip re-running it if you already have a pcap from that step) + `zeek -r` itself, which is
near-instant (well under 10s for a ~4 MB pcap, twice) — **comfortably under 5 minutes** for
the Zeek analysis step alone; under 5 minutes total only if you already have a capture, or
close to it (~5 min) if generating traffic fresh too.

## What was captured / produced (this generation)

`tcpdump -i any -w capture.pcap`: **23,882 packets captured, 30,682 received by filter, 0
dropped** (`LINUX_SLL2` link type — no single physical interface to bind promiscuously inside
a container network namespace, which tcpdump itself flags as a harmless warning, not an
error). `zeek -C -r` over that capture produced, in TSV form: `conn.log` 5,129 lines,
`dns.log` 3,409, `http.log` 1,545, `ssl.log` 169, `files.log` 1,447, `syslog.log` 26,
`weird.log` 18 (JSON form: 5,120 / 3,400 / 1,536 / 160 / 1,438 / 17 / 9 respectively — Zeek's
JSON output drops the `#fields`/`#types`/`#open`/`#close` header/footer lines that count
toward the TSV line totals).

## Teardown

Zeek itself ran as a one-shot `docker run --rm ...` (see `run_zeek.sh`) — nothing is left
running after it exits; no explicit teardown step is needed for Zeek. For the traffic-side
stack, see `../../haproxy/setup/SETUP.md`'s teardown (`docker compose down -v
--remove-orphans`, verified empty with `docker ps -a --filter name=ulpf-corpus-gen`).

No image was built for this task. `zeek/zeek:latest` and `nicolaka/netshoot:latest` were left
in the local image cache — both are needed to re-run in under 5 minutes. Remove only if
reclaiming disk space matters more than fast re-runs:

```sh
docker rmi zeek/zeek:latest   # optional; netshoot's removal is covered by the nginx SETUP.md
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
