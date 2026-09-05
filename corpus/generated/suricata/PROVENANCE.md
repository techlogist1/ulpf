# PROVENANCE — corpus/generated/suricata (label: gen-suricata)

All files below are **generated**, not fetched from a third party: a real Suricata
8.0.6 IDS ran in a real Docker container on this machine, sniffing a real network
interface, while a real traffic generator made real DNS/HTTP/TLS/scan connections
out to the live internet. Nothing here is hand-written or synthetic. See
`setup/SETUP.md` for the exact reproduction steps.

- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack, Docker 29.4.0
- **Generation time**: 2026-09-05, 08:25:34 UTC (rules loaded) through 08:30:44 UTC
  (Suricata shutdown, flows flushed) — traffic-carrying window per the events
  themselves: `2026-09-05T08:26:17.732858+0000` to `2026-09-05T08:30:43.621787+0000`
  (4m26s), plus ~40s of setup (image already pulled, rule fetch, engine start) and
  teardown either side.
- **Tool**: Suricata `8.0.6 RELEASE` (`suricata --build-info`, run inside the
  container), `suricata-update 1.3.8`
- **Image**: `jasonish/suricata:latest`, digest
  `sha256:7ad56111569e3e477a871f318bae90eafa36451c44fb338ed8532cb0059a8970`, base OS
  AlmaLinux 9.8, arm64 (native — no QEMU emulation)
- **Rules**: Emerging Threats Open, fetched live via `suricata-update` at generation
  time — 68,625 rules parsed, 52,672 enabled after flowbit-dependency resolution
- **How fetched**: not fetched — generated in-place with `docker run` /
  `docker exec` per `setup/SETUP.md`; `eve.json` was copied out of the container with
  `docker cp` (byte-for-byte, not scraped from stdout)
- **Licence**: n/a (no third-party file copied; Suricata itself is GPLv2 but only
  its *binary*, from the upstream `jasonish/suricata` Docker Hub image, was
  executed — no Suricata source or config was copied into this repo. ET Open rules
  are used only transiently inside the running container to produce alert traffic;
  the rule files themselves are not committed here)
- **What was anonymised**: nothing — every IP, hostname, port and payload byte in
  `eve.json` is exactly what Suricata observed on the wire. The only non-organic
  artifact is the `User-Agent: ulpf-corpus/1.0` header the traffic generator sent
  on its own plaintext HTTP requests (so those lines are identifiable as generated
  traffic, same as any lab capture) and the container's private bridge IP
  (`192.168.215.3`, RFC 1918) as the sole source address throughout.

## Files

| file | source | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `eve.json` | generated locally: `jasonish/suricata:latest` container, real traffic (see below) | image digest `sha256:7ad561…8970`; Suricata 8.0.6; ET Open ruleset pulled 2026-09-05 08:25 UTC | `/var/log/suricata/eve.json` inside the container | n/a (own output, no third-party file copied) | none (see above) | generated | 2,924 (of 5,052 captured; trimmed for the 2 MB cap, see Trimming below) | `docker cp suricata-gen:/var/log/suricata/eve.json` |
| `setup/docker-compose.yml` | authored for this task | — | — | n/a (original) | n/a | generated | 15 | written directly |
| `setup/run.sh` | authored for this task | — | — | n/a (original) | n/a | generated | 47 | written directly |
| `setup/traffic-gen.py` | authored for this task | — | — | n/a (original) | n/a | generated | 116 | written directly |
| `setup/SETUP.md` | authored for this task | — | — | n/a (original) | n/a | generated | — | written directly |

## Traffic sources actually contacted (all real, all public)

DNS/HTTP/TLS: example.com, neverssl.com, httpforever.com, info.cern.ch,
captive.apple.com, detectportal.firefox.com, connectivitycheck.gstatic.com,
www.wikipedia.org, api.github.com, httpbin.org, www.cloudflare.com, 1.1.1.1,
www.mozilla.org, duckduckgo.com, www.python.org, get.docker.com, 1.1.1.1.nip.io,
scanme.nmap.org, secure.eicar.org, www.suricata.io, docs.suricata.io,
www.google.com, www.iana.org, www.rfc-editor.org, cdn.jsdelivr.net, plus two
deliberately-nonexistent names for NXDOMAIN traffic. Malware-signature trigger:
`https://secure.eicar.org/eicar.com.txt` (the standard, harmless EICAR AV test
file — not a real malicious payload). Port scan: `scanme.nmap.org` (Nmap
project's public scan-test target), 10 common ports, real TCP connect scan.
`testmynids.org` was attempted (classic IDS test string) but no longer resolves
as of this capture — recorded as a DNS NXDOMAIN in the log, not faked.

## Trimming (2 MB / 20,000-line cap)

The full capture was 5,052 events / 3.0 MB (kept in full, uncut, at
`/private/tmp/claude-501/-Users-lokavyasingh-Documents-dev-ssh-hackathon/acb32a14-b9e1-4fa5-a2e7-28f1a8daef1b/scratchpad/gen-suricata/logs/eve.json`
on this machine, plus `fast.log`, `stats.log`, `suricata.log`,
`traffic-gen-output.log` alongside it). To fit the 2 MB cap the committed copy:
- **drops** all 19 `stats` events (Suricata's internal engine-load telemetry, not
  a network event, not one of the required kinds, and by far the largest
  per-line payload — 167 KB for 19 lines)
- **keeps every** `alert` (28), `http` (103), `tls` (291), `fileinfo` (80) and
  `anomaly` (13) event, unchanged and in original order
- **keeps every** `flow` event (1,354) — flow records are what proves each
  connection actually terminated
- **subsamples** `dns` events to every 3rd one in original order (1,055 of
  3,164) — DNS was the highest-volume, most repetitive event type (the same ~27
  hostnames looked up every round)

No line was edited; every kept line is byte-identical to what Suricata wrote.
Final committed file: 2,924 lines, ~1.7 MB.
