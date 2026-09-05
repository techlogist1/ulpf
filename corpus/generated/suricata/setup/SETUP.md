# Suricata IDS — generated corpus (real Docker capture)

Real Suricata, real network traffic, real ET Open detections. Captured on the host
below and copied out byte-for-byte; nothing in `eve.json` was hand-written.

## Tool / image

- **Suricata**: `8.0.6 RELEASE` (`suricata --build-info`, run inside the container)
- **suricata-update**: `1.3.8`
- **Image**: `jasonish/suricata:latest`
- **Image digest**: `jasonish/suricata@sha256:7ad56111569e3e477a871f318bae90eafa36451c44fb338ed8532cb0059a8970`
- **Image base OS**: AlmaLinux 9.8 ("Olive Jaguar"), arm64 (native, no emulation)
- **Rules**: Emerging Threats Open ruleset, pulled live via `suricata-update` —
  68,625 rules parsed, 52,672 enabled after flowbit-dependency resolution (run log
  timestamped 2026-09-05 08:25 UTC inside the container)
- **Host**: Apple M1 Pro MacBook Pro, macOS (Darwin 25.3.0), OrbStack, `docker 29.4.0`
- **Docker Compose**: v5.1.2 (declarative reference in `docker-compose.yml`; the
  actual run used the equivalent `docker run`/`docker exec` sequence in `run.sh`
  because Suricata is started *after* `suricata-update` populates the rules
  directory, not as the container's entrypoint)

## Exact commands, in order

```sh
# 1. Pull the image (arm64 native manifest)
docker pull jasonish/suricata:latest

# 2. Start a long-lived container, capabilities for AF_PACKET sniffing,
#    bind-mount ./logs so eve.json lands on the host filesystem directly
docker run -d --name suricata-gen \
  --cap-add=NET_ADMIN --cap-add=NET_RAW --cap-add=SYS_NICE \
  -v "$(pwd)/logs:/var/log/suricata" \
  --entrypoint sh jasonish/suricata:latest -c 'sleep 3600'

# 3. Pull Emerging Threats Open rules live (real internet fetch, ~69k rules)
docker exec suricata-gen suricata-update -v

# 4. Start Suricata sniffing the container's own eth0 (AF_PACKET), loaded with
#    the ET Open ruleset just fetched
docker exec -d suricata-gen sh -c \
  'suricata -c /etc/suricata/suricata.yaml \
            -S /var/lib/suricata/rules/suricata.rules \
            -i eth0 -l /var/log/suricata --set stats.interval=15'

# 5. Copy the traffic generator in and run it in the SAME network namespace as
#    Suricata, so every packet Suricata sees is real traffic this process made
docker cp traffic-gen.py suricata-gen:/root/traffic-gen.py
docker exec suricata-gen python3 /root/traffic-gen.py 12   # N rounds

# 6. Let in-flight flows time out, then stop Suricata cleanly (SIGTERM flushes
#    every still-open flow record instead of dropping it)
docker exec suricata-gen sh -c 'kill -TERM $(pgrep suricata)'

# 7. Copy the real eve.json out byte-for-byte (not stdout-scraped)
docker cp suricata-gen:/var/log/suricata/eve.json ./logs/eve.json

# 8. Teardown — see below
docker rm -f suricata-gen
```

`run.sh` in this directory runs steps 2-7 unattended (`./run.sh 12` for 12 rounds).

## What generated the traffic (`traffic-gen.py`)

Pure Python 3 stdlib (already present in the image — no extra install), executed
inside the Suricata container so packets cross the same `eth0` Suricata sniffs:

- **DNS** — `socket.getaddrinfo()` against ~27 real hostnames per round, including
  three deliberately-nonexistent names (NXDOMAIN traffic) and `scanme.nmap.org`.
- **HTTP** — `urllib.request` GETs against 7 plaintext HTTP endpoints per round
  (example.com, neverssl.com, Apple/Firefox/Google captive-portal probes, etc.)
  plus an EICAR antivirus test-file download from `secure.eicar.org` (this is the
  standard, harmless, industry test file made exactly to trigger AV/IDS malware
  signatures — not a real payload).
- **TLS** — a raw `ssl.wrap_socket` handshake (cert verification off, since we only
  need the handshake itself, not to trust the peer) against 10 real HTTPS hosts per
  round, each followed by a real `urllib` HTTPS GET through the same stack.
- **Port scan** — a real TCP connect-scan (`socket.connect_ex`, no raw sockets
  needed) of 10 common ports against `scanme.nmap.org`, the Nmap project's public
  target explicitly provided for exactly this kind of test traffic.
- **Alerts** — the EICAR download and the scan both reliably trip ET Open
  signatures (`ET POLICY EICAR...`, `ET SCAN...`) against the live ruleset loaded
  in step 3-4; no signature was hand-authored, all are stock ET Open.

## Run length / event volume (this capture)

- `traffic-gen.py` was launched for 40 rounds and left running for 12 completed
  rounds (~4m26s of traffic-carrying wall clock, 2026-09-05 08:26:17-08:30:43 UTC)
  before being stopped — already thousands of events by then, so the remaining
  rounds weren't needed. `docker exec suricata-gen pkill -f traffic-gen.py`, then
  a 30s drain, then `kill -TERM` on the suricata PID (clean shutdown flushes every
  still-open flow record). `run.sh` instead runs a fixed round count to
  completion, which is the reproducible form.
- Full capture: **5,052 events** (`dns` 3,164, `flow` 1,354, `tls` 291, `http` 103,
  `fileinfo` 80, `alert` 28, `stats` 19, `anomaly` 13) — kept in full in scratch
  (see PROVENANCE.md), 3.0 MB.
- Committed `../eve.json`: **2,924 events**, 1.7 MB — every `alert`/`http`/`tls`/
  `fileinfo`/`anomaly`/`flow` kept, `dns` subsampled 1-in-3, `stats` dropped
  (internal telemetry, not a network event). See PROVENANCE.md "Trimming".
- Event types present in the committed file: `dns`, `flow`, `tls`, `http`,
  `fileinfo`, `alert`, `anomaly` — all five kinds the brief asked for (`flow`,
  `dns`, `http`, `tls`, `alert`) plus incidental ones Suricata always emits.
- Validated against the baseline `ulpf` binary: all 2,924 lines detected, parsed,
  normalized and emitted — `no_parser 0`, `parse_failed 0` (full counter block in
  the task return).

## Re-run live at a demo (< 5 minutes)

```sh
cd corpus/generated/suricata/setup
./run.sh 8      # ~8 rounds ≈ 3-4 minutes wall clock, thousands of events
```
Then `tail -f logs/eve.json | jq .event_type` to show it live, or point
`ulpf run corpus/generated/suricata/setup/logs --store /tmp/s --output /tmp/o.jsonl`
at the freshly captured file.

## Teardown

```sh
docker rm -f suricata-gen
docker rmi jasonish/suricata:latest   # optional — only if the image itself isn't
                                        # needed again; re-pull takes ~30s
```
No containers or non-essential images were left running/present after this
capture (verified: `docker ps -a` and `docker images` show neither).
