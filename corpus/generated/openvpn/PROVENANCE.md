# PROVENANCE — corpus/generated/openvpn (label: gen-openvpn)

All five log files are **generated**, not fetched from a third party: real
OpenVPN 2.6.14 / 2.5.1 / 2.4.12 servers and real OpenVPN clients ran as real
Docker containers on this machine and exchanged real UDP/TLS traffic over a
private Docker bridge — successful sessions, reconnect churn, and three
distinct authentication/reachability failures. Every committed byte was
written by the `openvpn` binary itself, or by `rsyslogd` framing `openvpn`'s
own output. Nothing is hand-written, templated or replayed.
`SETUP.md` has the reproduction steps; `setup/` has every file they need.

- **Kind**: generated
- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack,
  Docker 29.4.0 (client and server), `linux/arm64` containers — native, no
  QEMU emulation
- **Generation time**: 2026-09-05. The committed captures span
  **09:56:38 – 10:16:12 UTC** (19 min 34 s of wall clock across the four
  phases; per-file spans in the table below). An independent cold
  reproduction run was then timed at **12:22:57 – 12:25:04 UTC** (2 min 7 s)
  to verify the pipeline and measure the demo command.
- **Teardown**: all five containers and the `ulpf-vpn-net` bridge removed
  (`run-demo.sh`'s `EXIT` trap); verified with
  `docker ps -a --filter name=ulpf-ovpn` (empty) and
  `docker network ls --filter name=ulpf-vpn-net` (empty). The three built
  images were then removed with `docker rmi` — see **Teardown** at the end.
  Nothing from this task is left running or stored.
- **Licence**: n/a — no third-party file was copied into this repo. OpenVPN is
  GPL-2.0, rsyslog GPL-3.0/Apache-2.0, easy-rsa GPL-2.0; only their *binaries*
  from Debian/Ubuntu packages were executed inside throwaway containers. No
  OpenVPN source was vendored.

## Tool versions (recorded from the tools themselves, inside the containers)

```
$ docker run --rm ulpf-openvpn:local   openvpn --version | head -2
OpenVPN 2.6.14 aarch64-unknown-linux-gnu [SSL (OpenSSL)] [LZO] [LZ4] [EPOLL] [PKCS11] [MH/PKTINFO] [AEAD] [DCO]
library versions: OpenSSL 3.0.20 7 Apr 2026, LZO 2.10

$ docker run --rm ulpf-openvpn25:local openvpn --version | head -2
OpenVPN 2.5.1 aarch64-unknown-linux-gnu [SSL (OpenSSL)] [LZO] [LZ4] [EPOLL] [PKCS11] [MH/PKTINFO] [AEAD] built on Jul  3 2026
library versions: OpenSSL 1.1.1w  11 Sep 2023, LZO 2.10

$ docker run --rm ulpf-openvpn24:local openvpn --version | head -2
OpenVPN 2.4.12 aarch64-unknown-linux-gnu [SSL (OpenSSL)] [LZO] [LZ4] [EPOLL] [PKCS11] [MH/PKTINFO] [AEAD] built on Jun 27 2024
library versions: OpenSSL 1.1.1f  31 Mar 2020, LZO 2.10
```

Container OS, from `/etc/os-release` inside each image: Debian GNU/Linux 12
(bookworm), Debian GNU/Linux 11 (bullseye), Ubuntu 20.04.6 LTS.
`rsyslogd 8.2302.0` — recorded by rsyslog itself in the first line of
`server-syslog.log`. PKI built by the `easy-rsa` 3 package inside
`ulpf-openvpn:local` (`gen-pki.sh`), never on the host.

## Images

All three were **built locally** from the Dockerfiles in `setup/`; none was
pulled ready-made, so there is no upstream repo digest for them — the digest
below is each local image's own content digest (`docker image inspect .Id`),
recorded *before* teardown from the exact images that produced the committed
logs.

| tag | image digest (built 2026-09-05) | base | OpenVPN |
|---|---|---|---|
| `ulpf-openvpn:local` | `sha256:3a6d1d6267602c7fe5de80e6f576b9fb58c8db4e9cb6db38d00cbb16f6f7cc72` | `debian:bookworm-slim` | 2.6.14 |
| `ulpf-openvpn25:local` | `sha256:837f30aa1944844c5b65469a8fd619c889b66da27d45f06dc8c910644d4ddd74` | `debian:bullseye-slim` | 2.5.1 |
| `ulpf-openvpn24:local` | `sha256:054fb39ae6c8fd9d74b7e2b3b3573487659cde02429f5b975a813b5ffc97bf63` | `ubuntu:20.04` | 2.4.12 |

Base image digest, for the one that was still resolvable at recording time:
`debian:bookworm-slim` = `debian@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171`.
`debian:bullseye-slim` and `ubuntu:20.04` were no longer tagged locally when
provenance was recorded, so **their upstream digests are not captured** — an
honest gap (see *Not obtained*). A rebuild is therefore not bit-identical: the
cold reproduction run rebuilt the same three Dockerfiles and got new image ids
(`b2b7c56effdc…`, `d33393eb0724…`, `854599668dc7…`) because the apt indexes
moved, while `openvpn --version` still reported exactly 2.6.14 / 2.5.1 /
2.4.12 and the log formats were byte-shape-identical.

**How fetched**: not fetched — generated in place by
`setup/run-demo.sh`. Every log was copied out of the container filesystem with
`docker cp` (byte-for-byte), never scraped from `docker logs` stdout.

## Files

| file | source | revision | path in source | licence | anonymised | kind | lines | bytes | span (UTC) | how fetched |
|---|---|---|---|---|---|---|---|---|---|---|
| `server.log` | generated locally: `ulpf-openvpn:local`, phase A, `--log-append`, `verb 3` | OpenVPN 2.6.14; image `sha256:3a6d1d62…` | `/var/log/openvpn/server.log` in `ulpf-ovpn-server` | n/a (own output) | none | generated | 3,937 | 387,108 | 09:56:38 → 10:10:09 (13m31s) | `docker cp` |
| `server-syslog.log` | generated locally: same image and config, phase B, `syslog` + `rsyslogd` co-process | OpenVPN 2.6.14, rsyslogd 8.2302.0 | `/var/log/openvpn/syslog` in `ulpf-ovpn-server` | n/a (own output) | none | generated | 1,044 | 124,228 | 10:01:48 → 10:04:35 (2m47s) | `docker cp` |
| `client.log` | generated locally: the four client containers' own logs, concatenated in start order (good, badcert, badtls, badport) | OpenVPN 2.6.14 | `/var/log/openvpn/client.log` in each of `ulpf-ovpn-client-{good,badcert,badtls,badport}` | n/a (own output) | none | generated | 7,329 | 543,947 | 09:56:41 → 10:10:07 | bind mount + `cat` (`run-demo.sh`) |
| `server-2.5.log` | generated locally: `ulpf-openvpn25:local`, phase C, same server config | OpenVPN 2.5.1; image `sha256:837f30aa…` | `/var/log/openvpn/server.log` | n/a (own output) | none | generated | 565 | 54,449 | 10:10:10 → 10:11:36 (1m26s) | `docker cp` |
| `server-2.4-ctime.log` | generated locally: `ulpf-openvpn24:local`, phase D, same server config | OpenVPN 2.4.12; image `sha256:054fb39a…` | `/var/log/openvpn/server.log` | n/a (own output) | none | generated | 674 | 65,911 | 10:14:21 → 10:16:12 (1m51s) | `docker cp` |
| `setup/**` | authored for this task | — | — | n/a (original) | n/a | generated | small | — | — | written directly |

**No truncation.** The largest committed file is 544 KB / 7,329 lines — under
both the 2 MB and the 20,000-line cap — so every file is the complete capture,
not a slice. The cold reproduction run's own output (1,073 lines across five
files) is kept separately in scratch at
`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/gen-openvpn/rerun-out/`
and is **not** committed — it would only duplicate the shapes already here.

## What was anonymised

Nothing — every field is exactly what OpenVPN wrote. Nothing needed redacting:
all addresses are private (`192.168.117.0/24`, the OrbStack bridge for
`ulpf-vpn-net`; `10.8.0.0/24`, the VPN's own pool), the syslog hostname is the
container id (`253acea0e8fa`), and every identity is a lab invention minted by
`gen-pki.sh` on the spot — CNs `server`, `jdoe`, `olduser`, `mallory`, and two
throwaway CAs, `ULPF Test CA` and `Rogue Test CA`. Certificate serials in the
`VERIFY ERROR` lines belong to those throwaway certs. No real person,
organization or network appears anywhere.

## Traffic actually driven (all real, all local — no internet egress)

Four phases, each one server container plus four client containers on the
private `ulpf-vpn-net` bridge, all running the real `openvpn` binary speaking
real UDP/TLS. `--cap-add NET_ADMIN --device /dev/net/tun` was sufficient on
this host: every container opened `tun0` on the first attempt
(`TUN/TAP device tun0 opened` in all five logs, no permission error anywhere).
**`--privileged` was never needed and no TCP-mode / same-container fallback was
required.**

| client | fault injected | signature it produced (counts from `server.log` / `client.log`) |
|---|---|---|
| `good` (CN `jdoe`) | none — reconnected in a loop: real `SIGHUP` soft restart on most cycles, a real `SIGTERM` stop/start every 7th | 132 × `Peer Connection Initiated`, 132 × `Initialization Sequence Completed`, 382 × `VERIFY OK`, 100 × `SIGHUP`, 38 × `SIGTERM[soft,exit-with-notification]` |
| `badcert` (CN `mallory`, signed by `Rogue Test CA`) | certificate from a CA the server does not trust | 59 × `VERIFY ERROR: depth=0, error=unable to get local issuer certificate: CN=mallory, serial=…` followed by `TLS Error: TLS handshake failed` (57 on the client side) |
| `badtls` (CN `jdoe`, mismatched `tls-auth` key) | wrong control-channel HMAC key | 114 × `Authenticate/Decrypt packet error: packet HMAC authentication failed` — the handshake never reaches certificate verification |
| `badport` (CN `olduser`, remote port 11940) | nothing listening on that port | 114 × `read UDPv4 [ECONNREFUSED]: Connection refused (fd=3,code=111)` plus `TLS Error: TLS key negotiation failed to occur within 8 seconds` |

The server config forces lifecycle churn deliberately (`keepalive 5 15`,
`reneg-sec 60`, `tls-timeout 5`, `hand-window 8`, `explicit-exit-notify 1`,
`crl-verify`), which is why 19 minutes of traffic yields ~13,500 lines of real
session state across the five files rather than a few hundred.

**Honest note on the exact invocation.** The committed captures were produced
by an earlier session of this same task that was cut off by a session limit
before it wrote provenance. `setup/` (the Dockerfiles, PKI script, configs,
`run-traffic.sh`, `run-demo.sh`) is that session's tooling and is what produced
them; the per-phase wall-clock spans above are read off the log timestamps
themselves. The one thing not recoverable is whether that session drove the
four phases as a single `run-demo.sh` invocation or as separate manual phase
invocations — `server.log`'s span (09:56:38 → 10:10:09) overlaps
`server-syslog.log`'s (10:01:48 → 10:04:35), which a single `run-demo.sh`
cannot produce, so at least the first two phases were driven separately, and
the phase-A server was left up longer than one `run-traffic.sh` pass. To close
that gap this session re-ran `run-demo.sh` end-to-end from cold and confirmed
it reproduces all five formats and all four signatures (see *Reproduction
check*), so nothing about the file contents rests on the unrecoverable
command history.

## The version story (why there are three server logs)

The shipped `parsers/openvpn.toml` matches on the ctime prefix
(`^(?:Mon|Tue|…) [A-Z][a-z]{2} +\d{1,2} \d{2}:\d{2}:\d{2} \d{4} `). Measured
here rather than assumed: **OpenVPN 2.4.12 writes that prefix; 2.5.1 and 2.6.14
both write `YYYY-MM-DD HH:MM:SS`.** So the shipped parser covers 2.4-and-earlier
file logs only, and every modern OpenVPN deployment is an unknown format to
ULPF today. That is the point of this corpus: `server-2.4-ctime.log` proves the
parser works, and the other four files are honest, real, unseen-format
inference material.

## Baseline validation (`ulpf run … --infer-threshold 0`, release binary, read-only)

```
$ ./target/release/ulpf run corpus/generated/openvpn/<file> \
    --store <scratch>/store-N --output <scratch>/out-N.jsonl --infer-threshold 0
```

| file | framed | detected | no_parser | parse_failed | throughput |
|---|---|---|---|---|---|
| `server.log` | 3,937 | 0 | **3,937** | 0 | 354,198 ev/s, 33.2 MB/s |
| `server-syslog.log` | 1,044 | 0 | **1,044** | 0 | 171,926 ev/s, 19.5 MB/s |
| `client.log` | 7,329 | 0 | **7,329** | 0 | 245,384 ev/s, 17.4 MB/s |
| `server-2.5.log` | 565 | 0 | **565** | 0 | 50,007 ev/s, 4.6 MB/s |
| `server-2.4-ctime.log` | 674 | **674** | 0 | **0** | 46,241 ev/s, 4.3 MB/s |

**What breaks: nothing.** `framed` equals the exact line count of every file,
`parse_failed` is 0 everywhere, and no run produced a warning or a dropped
event.

The four `no_parser` results are the **expected, correct** answer, confirmed
deliberately: the modern `YYYY-MM-DD HH:MM:SS` OpenVPN file format, the client
log and the RFC 3164 syslog-framed form are all outside the twelve shipped
parsers (only the 2.4 ctime form is covered), so ULPF stores every raw byte,
counts the events as `no_parser`, and still emits all of them — 3,937 / 1,044 /
7,329 / 565 lines in, the same numbers out. On those runs
`time_from_receipt` and `class_unknown` equal the event count, which is exactly
right for an unparsed event: no device time was extracted and no class rule
could match, so ULPF falls back to receipt time and `Base Event` rather than
guessing.

`server-2.4-ctime.log` is the positive control: 674/674 detected, 674 parsed,
0 `parse_failed`, `time_from_receipt 0` (every ctime stamp parsed, policy
`tz_assumed` as documented), `sub_matched 549`, `sub_no_match 125`,
`sub_uncovered 0`. The 125 are real, benign, and worth naming: they are daemon
startup and platform lines the parser's `sub` patterns do not model —
`WARNING: file '…/server.key' is group or others accessible`,
`library versions: OpenSSL …`, `Diffie-Hellman initialized with 2048 bit key`,
`CRL: loaded 1 CRLs from file …`, `TUN/TAP TX queue length set to 100`,
`/sbin/ip addr add dev tun0 …`, `IFCONFIG POOL LIST`,
`Socket Buffers: R=[…] S=[…]`, `Control Channel: TLSv1.3, cipher …`,
`OpenSSL: error:1417C086:SSL routines:…`,
`TLS_ERROR: BIO read tls_read_plaintext error`, and
`Authenticate/Decrypt packet error: packet HMAC authentication failed`. The
line's fields and timestamp are all extracted; only the message-level
enrichment is missing. Two of those — the `OpenSSL: error:…` /
`Authenticate/Decrypt` pair — are genuinely worth adding to
`parsers/openvpn.toml` as new `sub` patterns, since they are the observable
signature of a `tls-auth` key mismatch. `class_unknown 559` follows from the
same gap.

## Inference (`ulpf infer <file> --pending … --decisions`) — the demo material

Run on all four unseen-format files. Every one produced a written proposal.

| file | lines | templates emitted | lines covered (verified) | unmatched | why unmatched | matcher the engine chose |
|---|---|---|---|---|---|---|
| `server.log` | 3,937 | 29 | 3,876 (98.4%) | 61 | 51 `below_support`, 10 `no_template` | regex over 17 template leads |
| `server-syslog.log` | 1,044 | 26 | 1,012 (96.9%) | 32 | 29 `below_support`, 3 `no_template` | `contains = ["openvpn"]` |
| `client.log` | 7,329 | 40 | 7,064 (96.4%) | 265 | 264 `template_cap`, 1 `below_support` | regex over 32 template leads |
| `server-2.5.log` | 565 | 24 | 537 (95.0%) | 28 | 25 `below_support`, 3 `no_template` | regex over 16 template leads |

Two results are worth putting on screen during the demo:

1. **The syslog envelope is detected, not configured.** On
   `server-syslog.log` the engine logged
   `envelope: syslog header on 1044 of 1044 lines -> syslog = true` and set
   `[envelope] syslog = true`, then picked `contains = ["openvpn"]` as the
   matcher (`openvpn` appears in 1043/1044 lines — the one exception is
   rsyslog's own startup banner). Templates come out already shaped like
   `openvpn[{pid:int}]: {ip1:ipv4}:{port1:port} VERIFY OK: depth={depth:int}, CN={cn:word}{? Test CA}`.
2. **The slot names are the device's own vocabulary, never OCSF.** Exactly as
   the parser/mapping wall requires: `{depth:int}`, `{cn:word}`, `{pid:int}`,
   `{mtu:int}`, `{ping:int}`, `{ping_restart:int}`, `{push_status:int}` — plus
   `kind+n` fallbacks (`{ip1:ipv4}`, `{word1:word}`, `{int1:int}`) where the
   line gave the engine no key to name a slot after.

The decision log is legible on the demo screen too: keyword splits
(`slot after 'tls' has 2 distinct keyword values 'handshake' (17), 'object'
(17): split so the words stay constant`), optional groups derived from presence
(`'Test' optional: present in 34/68 lines` → the `{? Test CA}` group above),
junk-token drops, cross-cluster merges of cipher names
(`'AES-128-GCM' optional`, `'CHACHA20-POLY1305' optional`), and per-template
verification against the real parser
(`template 2 verified 33/34 of its own lines`).

`client.log` is the one file that hits a ceiling: it saturates
`max_templates 40` and sends 264 lines to unmatched with reason
`template_cap`. That is the correct, visible behaviour — a client log is
chattier than a server log — and it is a good live demo of the reason codes
doing their job rather than a silent drop.

## Reproduction check (this session, independent of the committed capture)

`setup/run-demo.sh 12 8 8 6 6 4 6 4` was run end-to-end from a completely cold
start — no PKI on disk, `debian:bullseye-slim` and `ubuntu:20.04` not cached —
and took **2 min 7 s** (12:22:57 → 12:25:04 UTC, `2:06.96 total` from
`time`). It rebuilt all three images, minted a fresh PKI with real `easy-rsa`,
ran all four phases and tore everything down. It produced 236 + 179 + 422 +
124 + 112 = 1,073 real lines with every signature intact (4 `VERIFY ERROR`,
6 `packet HMAC authentication failed`, 7 `Peer Connection Initiated`,
6 `ECONNREFUSED`), the 2.4 image still emitting the ctime prefix
(`Sat Sep  5 12:24:41 2026 …`) and phase B still emitting RFC 3164
(`Sep  5 12:23:50 009f568edd2d rsyslogd: …`). That output is archived in
scratch, not committed. This is the command to use for a live demo.

## Teardown

`run-demo.sh`'s `EXIT` trap removed `ulpf-ovpn-server` and all four
`ulpf-ovpn-client-*` containers and the `ulpf-vpn-net` network — verified
empty afterwards with `docker ps -a --filter name=ulpf-ovpn` and
`docker network ls --filter name=ulpf-vpn-net`. The three built images
(`ulpf-openvpn:local`, `ulpf-openvpn25:local`, `ulpf-openvpn24:local`) were
then removed with `docker rmi`. `setup/pki-out/`, `setup/run/` and `setup/out/`
— the generated PKI (which contains real, if throwaway, private keys), the
bind-mount scaffolding and the run output — were deleted and are not
committed; `gen-pki.sh` mints a fresh CA on every cold run.

Verified after teardown: `docker images | grep -i openvpn` lists none of the
three, no `ulpf-ovpn-*` container exists, no `ulpf-vpn-net` network exists, and
`docker images -f dangling=true` shows only three images from two to three
months ago that predate this task. One image, `kylemanna/openvpn:latest`, is
still on the host; it is **not** used by anything in `setup/` and was most
likely pulled during an earlier exploration of this task, but its origin could
not be confirmed from this session, so it was deliberately left in place rather
than deleted on a guess. Unrelated containers and images belonging to other
work were left untouched.
