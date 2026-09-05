# SETUP — corpus/generated/openvpn (label: gen-openvpn)

Five real OpenVPN log files, produced by running real OpenVPN 2.4 / 2.5 / 2.6
servers and real OpenVPN clients as Docker containers on this Mac and driving
real UDP/TLS sessions (successful and failing) between them over a private
Docker bridge. Nothing here is hand-written, templated or replayed; every byte
was written by the `openvpn` binary (or by `rsyslogd` framing `openvpn`'s
output) inside a container.

Provenance, versions, image digests and the ULPF validation numbers are in
`PROVENANCE.md`. Everything needed to reproduce is in `setup/`.

---

## What is here

| file | what it is | lines |
|---|---|---|
| `server.log` | OpenVPN **2.6.14** server, `--log-append`, `verb 3` — the modern `YYYY-MM-DD HH:MM:SS` prefix | 3,937 |
| `server-syslog.log` | the same server config, but `--syslog` with `rsyslogd` in the same container — real RFC 3164 `Mon DD HH:MM:SS host openvpn[pid]: …` | 1,044 |
| `client.log` | the four client containers' own logs (2.6.14), concatenated in start order: good, badcert, badtls, badport | 7,329 |
| `server-2.5.log` | OpenVPN **2.5.1** server, same config — proves 2.5 already uses the modern prefix | 565 |
| `server-2.4-ctime.log` | OpenVPN **2.4.12** server, same config — the `Sat Sep  5 10:14:21 2026` ctime prefix that `parsers/openvpn.toml` targets | 674 |

No file was truncated: the largest is 544 KB / 7,329 lines, well under the
2 MB / 20,000-line cap.

## The scenario the traffic script drives

One server container plus four client containers on a private bridge
(`ulpf-vpn-net`, 192.168.117.0/24). VPN subnet 10.8.0.0/24.

| client | what is wrong with it | what the logs show |
|---|---|---|
| `good` (CN `jdoe`) | nothing | full handshake, `VERIFY OK` → `Peer Connection Initiated` → push/pull → `Initialization Sequence Completed`; then repeatedly reconnected — `SIGHUP` soft restart on most cycles, a real `SIGTERM` stop/start every 7th |
| `badcert` (CN `mallory`) | certificate signed by a second, rogue CA the server does not trust | server: `VERIFY ERROR: depth=0, error=unable to get local issuer certificate: CN=mallory, serial=…` then `TLS Error: TLS handshake failed` |
| `badtls` (CN `jdoe`) | correct cert, **wrong** `tls-auth` static key | server: `Authenticate/Decrypt packet error: packet HMAC authentication failed` — the control channel never reaches certificate verification. Client: `TLS Error: TLS key negotiation failed to occur within 8 seconds` |
| `badport` (CN `olduser`) | valid identity, pointed at 11940/udp where nothing listens | client: `read UDPv4 [ECONNREFUSED]: Connection refused (fd=3,code=111)` and `TLS Error: TLS key negotiation failed…`, retried forever |

The server config forces churn on purpose so a short run yields a lot of
session lifecycle: `keepalive 5 15`, `reneg-sec 60`, `tls-timeout 5`,
`hand-window 8`, `explicit-exit-notify 1`, `crl-verify`, `status … 3`.

## Reproduce

Everything is driven by one script. It builds the three images, generates a
throwaway PKI with the real `easy-rsa 3` inside the image, runs all four
phases, copies the logs out with `docker cp`, and tears every container and
the network down through an `EXIT` trap.

```sh
cd corpus/generated/openvpn/setup
./run-demo.sh                       # full defaults = 30 30 20 20 12 12 12 12; ~4 min (from the script's own sleeps; not timed)
./run-demo.sh 12 8 8 6 6 4 6 4      # the demo profile, measured 2m07s cold
```

Arguments are `A_CYCLES A_DWELL B_CYCLES B_DWELL C_CYCLES C_DWELL D_CYCLES
D_DWELL` — reconnect cycles and failing-client dwell seconds for each of the
four phases. Output lands in `setup/out/`; the committed files at the top of
this directory came from a longer run (see `PROVENANCE.md`).

**Live-demo command, under 5 minutes** — measured end-to-end at **2m07s** on
this machine from a completely cold start (no PKI on disk, `debian:bullseye-slim`
and `ubuntu:20.04` not cached, so both were pulled):

```sh
cd corpus/generated/openvpn/setup && time ./run-demo.sh 12 8 8 6 6 4 6 4
```

Measured breakdown of that run (UTC): 12:22:57 start → 12:23:15 the three
image builds and the whole PKI (including 2048-bit `gen-dh`) are done → phase A
12:23:15-12:23:47 → phase B 12:23:53-12:24:17 → phase C 12:24:22-12:24:38 →
phase D 12:24:44-12:25:01 → 12:25:04 logs copied out and torn down. It
produced 236 + 179 + 422 + 124 + 112 = 1,073 real log lines with every
failure signature intact (4 `VERIFY ERROR`, 6 HMAC failures, 7 successful
`Peer Connection Initiated`, 6 `ECONNREFUSED`). Turn the numbers up for a
longer capture; they scale roughly linearly.

### Phases

1. **A** — 2.6.14 server with `--log-append /var/log/openvpn/server.log` → `out/server.log`
2. **B** — same server with `syslog` in the config plus `rsyslogd` as a
   co-process (`server-syslog-entry.sh` is PID 1: start `rsyslogd -n`, then
   `exec openvpn`) → `out/server-syslog.log`
3. **C** — 2.5.1 server → `out/server-2.5.log`
4. **D** — 2.4.12 server → `out/server-2.4-ctime.log`

Each phase runs `run-traffic.sh`, which starts the three failing clients,
then the good client, cycles it, and dwells. The client logs of phase A are
concatenated into `out/client.log`.

### `/dev/net/tun`

`--cap-add NET_ADMIN --device /dev/net/tun` was enough on this host — OrbStack's
Linux VM exposes `/dev/net/tun` and every container opened `tun0` on the first
attempt (`TUN/TAP device tun0 opened` in all five logs, no permission errors
anywhere). **`--privileged` was never needed**, and no TCP-mode/same-container
fallback was required.

## Teardown

`run-demo.sh` removes all five containers and the `ulpf-vpn-net` network from
its `EXIT` trap, so a Ctrl-C also cleans up. The three images survive on
purpose (they are the expensive part of a re-run); remove them with:

```sh
docker rmi ulpf-openvpn:local ulpf-openvpn25:local ulpf-openvpn24:local
docker rmi debian:bookworm-slim debian:bullseye-slim ubuntu:20.04   # if nothing else needs them
```

## Files under `setup/`

| file | what |
|---|---|
| `Dockerfile` | `debian:bookworm-slim` + openvpn, easy-rsa, openssl, rsyslog, iproute2 (OpenVPN 2.6.14) |
| `Dockerfile.openvpn25` | `debian:bullseye-slim` + openvpn (2.5.1) |
| `Dockerfile.openvpn24` | `ubuntu:20.04` + openvpn (2.4.12) |
| `gen-pki.sh` | real `easy-rsa 3` inside the image: main CA (`ULPF Test CA`), server cert, `jdoe`/`olduser` client certs, DH params, CRL, a `tls-auth` key **and a deliberately mismatched one**, plus a second rogue CA (`Rogue Test CA`) that mints `mallory` |
| `server.conf` | the server, logging natively with `log-append` |
| `server-syslog.conf` | identical except `syslog` replaces `log-append` |
| `server-syslog-entry.sh` | PID 1 for phase B: `rsyslogd -n` then `exec openvpn` |
| `rsyslog.conf` | `imuxsock` only (no `imklog` — there is no `/proc/kmsg` in a container), `RSYSLOG_TraditionalFileFormat` to `/var/log/openvpn/syslog` |
| `client-good.conf` / `client-badcert.conf` / `client-badtls.conf` / `client-badport.conf` | the four clients, each with a comment naming the failure it produces |
| `run-traffic.sh` | drives one phase's traffic |
| `run-demo.sh` | the whole thing, four phases, with teardown |

`setup/pki-out/` and `setup/run/` and `setup/out/` are generated working
directories and are **not** committed — `gen-pki.sh` mints a fresh throwaway CA
and private keys on every cold run, and no private key belongs in this repo.
