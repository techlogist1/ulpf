# PROVENANCE — corpus/generated/zeek (label: gen-nginx-haproxy-zeek)

All files below are **generated**, not fetched from a third party: the official
`zeek/zeek:latest` image ran `zeek -r` offline over a real `tcpdump` packet capture of the
nginx/HAProxy/DNS traffic from `corpus/generated/nginx` and `corpus/generated/haproxy`, taken
on this machine while that traffic ran, and Zeek's own log files were copied out byte-for-byte.
Nothing here is hand-written or synthetic. See `setup/SETUP.md` for the exact reproduction
steps.

- **Host**: Apple M1 Pro MacBook Pro (Darwin 25.3.0, arm64), OrbStack, Docker 29.4.0
- **Generation time**: capture 2026-09-05 08:47–09:36 UTC (`tcpdump -i any -w capture.pcap`
  inside a `nicolaka/netshoot` container sharing the traffic-generating client's network
  namespace, so it saw every packet client↔haproxy↔nginx1↔nginx2 plus DNS to Docker's embedded
  resolver); `zeek -r` offline analysis run 2026-09-05 09:37 UTC, both TSV and JSON output
  forms in the same invocation window (`#open 2026-09-05-09-37-22`/`-23` in every log).
- **Tool**: Zeek `8.2.2` (`zeek --version` inside the container)
- **Image**: `zeek/zeek:latest`, digest
  `sha256:703f0b22af150d9418739b2a012fbfb5d01ee004aded3bd43b0175010db05928`, arm64 native
- **Capture**: `tcpdump` (from `nicolaka/netshoot:latest`, digest
  `sha256:b09d9b21381f47a79b3cbcb30da25266dc17186ea00ae65e99fdc51396f48e70`) — `tcpdump -i any
  -w /pcap/capture.pcap`, stopped with `SIGINT`. tcpdump's own summary: **23,882 packets
  captured, 30,682 packets received by filter, 0 dropped by kernel** (link type
  `LINUX_SLL2`, since Docker/OrbStack's network namespace has no single physical interface to
  bind promiscuously — `any` was used, and tcpdump itself warned promiscuous mode isn't
  supported on `any`, which is expected and harmless for capturing the container's own
  traffic).
- **How fetched**: not fetched — generated in-place. `zeek -r` was run with output volumes
  bind-mounted straight to the host (`-v $SCR/zeek-out-tsv:/out` / `-v $SCR/zeek-out-json:/out`
  from `setup/run_zeek.sh`), so the committed files are Zeek's own log-writer output, not
  anything scraped from stdout.
- **Licence**: n/a — no third-party file copied; only the official `zeek/zeek:latest` and
  `nicolaka/netshoot:latest` Docker Hub images were executed.
- **What was anonymised**: nothing. All addresses are the compose network's private bridge
  range (`192.168.148.0/24` and `172.x`/Docker's embedded `127.0.0.11` resolver, RFC 1918/
  loopback), assigned by Docker at `docker compose up` time.
- **One real, documented adjustment**: the pcap was captured over Docker's virtual `veth`
  interfaces, which — like most virtual/loopback network paths — never compute real TCP/UDP
  checksums (checksum calculation is offloaded to hardware that never runs for
  container-internal traffic), so every packet's checksum field is invalid junk. Zeek's
  default behavior is to silently discard any packet with an invalid checksum before protocol
  analysis, which produced only `conn.log`/`weird.log`/`packet_filter.log`/`reporter.log` on
  the first run (no `dns.log`/`http.log`/`ssl.log` at all). `setup/run_zeek.sh` runs `zeek -C`
  (Zeek's own documented `-C`/`ignore_checksums` flag for exactly this situation) so the real
  DNS/HTTP/TLS analyzers actually reassemble the streams. This is a standard Zeek flag for
  virtual-interface captures, not a modification to any log content — every log line below is
  Zeek's real analyzer output over the real capture.

## Files (TSV, default Zeek `#fields`-headered ASCII form — required four plus what Zeek
produced on its own)

| file | source | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `conn.log` | generated locally: `zeek/zeek:latest` `zeek -C -r` over the real capture | image digest `sha256:703f0b22…5928`; Zeek 8.2.2 | `/out/conn.log` inside the container (bind-mounted) | n/a (own output) | none | generated | 5,129 | bind mount, then `cp` |
| `dns.log` | same | same | `/out/dns.log` | n/a | none | generated | 3,409 | bind mount, then `cp` |
| `http.log` | same | same | `/out/http.log` | n/a | none | generated | 1,545 | bind mount, then `cp` |
| `ssl.log` | same | same | `/out/ssl.log` | n/a | none | generated | 169 | bind mount, then `cp` |
| `files.log` | same (Zeek's own file-extraction summary for HTTP bodies) | same | `/out/files.log` | n/a | none | generated | 1,447 | bind mount, then `cp` |
| `syslog.log` | same (Zeek's syslog analyzer picked up the HAProxy→listener UDP:514 traffic on the wire) | same | `/out/syslog.log` | n/a | none | generated | 26 | bind mount, then `cp` |
| `weird.log` | same (Zeek's protocol-anomaly log — triggered by the deliberately malformed/raw-byte requests in `traffic.sh`) | same | `/out/weird.log` | n/a | none | generated | 18 | bind mount, then `cp` |
| `json/conn.log`, `json/dns.log`, `json/http.log`, `json/ssl.log`, `json/files.log`, `json/syslog.log`, `json/weird.log` | same capture, second `zeek -C -r` invocation with `LogAscii::use_json=T` | same | `/out/*.log` (JSON form) | n/a | none | generated | 5,120 / 3,400 / 1,536 / 160 / 1,438 / 17 / 9 | bind mount, then `cp` |
| `setup/docker-compose.yml` | authored for this task (shared stack; also drives `corpus/generated/nginx` and `corpus/generated/haproxy`) | — | — | n/a (original) | n/a | generated | 59 | written directly |
| `setup/traffic.sh` | authored for this task (same script) | — | — | n/a (original) | n/a | generated | 101 | written directly |
| `setup/traffic-quick.sh` | authored for this task: `traffic.sh` with the four loop counts cut (500/300/80/60 -> 60/40/12/10); actually run end to end to time the 2 min 09 s demo path | — | — | n/a (original) | n/a | generated | 102 | written directly (`sed` from `traffic.sh`) |
| `setup/run_zeek.sh` | authored for this task | — | — | n/a (original) | n/a | generated | 28 | written directly |
| `setup/SETUP.md` | authored for this task | — | — | n/a (original) | n/a | generated | — | written directly |

All committed files are well inside the 20,000-line / 2 MB cap (largest: TSV `conn.log`,
624 KB / 5,129 lines; largest JSON, `conn.log`, 1.93 MB / 5,120 lines) — nothing was trimmed.

## What's in the logs

- **`conn.log`**: every TCP/UDP flow the capture saw — HTTP and TLS connections
  client↔haproxy↔nginx1/nginx2, HAProxy's own health-check connections, and DNS lookups
  (UDP/53) to Docker's embedded resolver (`127.0.0.11`).
- **`dns.log`**: real resolutions for `nginx1`, `nginx2`, `haproxy`, `sysloglistener` (Docker's
  internal DNS) plus real `NXDOMAIN` answers for the deliberately-invalid names
  (`nosuchhost.invalid`, `doesnotexist.invalid`, `another.bad.invalid`) in `traffic.sh`.
- **`http.log`**: every plaintext HTTP request/response from the 500+300-request curl loops —
  varied methods, status codes (200/301/404/500/413), user agents and referers, including the
  malformed-Host and oversized-header attempts that nginx/HAProxy accepted far enough to log
  at the HTTP layer.
- **`ssl.log`**: the TLS handshakes from the 80×2 `curl -k https://...` requests against the
  self-signed cert on 443 (both direct-to-backend and via HAProxy's TCP passthrough frontend).
- **`weird.log`**: Zeek's own anomaly detections — triggered by the raw non-UTF-8 byte
  sequences and garbage request lines `traffic.sh` sent via `nc` (e.g.
  `unescaped_%_in_URI`/`above_hole_data_without_any_acks`-style entries; see the file for the
  exact `name` values Zeek assigned).
- **`syslog.log`**: a real bonus catch — Zeek's syslog analyzer parsed the same
  HAProxy→`sysloglistener` UDP:514 traffic that produced `corpus/generated/haproxy/haproxy.log`,
  seen independently on the wire.

## Baseline validation

See `corpus/generated/haproxy/PROVENANCE.md` for the full validation write-up (nginx, haproxy
and zeek were validated together against the pinned baseline binary in one session, including
the `[entities]`/scratch-mapping caveat that applies identically to all three).

Result for Zeek (7 TSV files + 4 JSON files, run separately): **100% `no_parser`** — 11,743
framed/emitted events across the 7 TSV files, 10,216 across the 4 JSON files (conn/dns/http/
ssl), 0 `detected`, 0 `parsed`, 0 `parse_failed`, 0 `sub_no_match`, 0 `sub_uncovered` in both
runs — exactly the expected/correct result: Zeek's `#fields`-headered TSV and its JSON form
are both outside the twelve shipped parsers (the closest structural relative,
`squid_access`'s positional space-delimited format, shares no field layout, delimiter, or
header convention with either Zeek form).

## Re-validation against the current release binary (`target/release/ulpf`, per file)

Re-run 2026-09-05 after the Rust team's `mappings/ocsf.toml` change landed. The current
release binary loads the live repo cleanly (`ulpf check` → `12 parsers, 1 mappings loaded;
0 problems`), so the `[entities]` caveat recorded in `../haproxy/PROVENANCE.md` against the
older pinned baseline **no longer applies**; these numbers come from the unmodified repo
`parsers/` and `mappings/`, one `ulpf run ... --infer-threshold 0` per file.

| file | framed | detected | no_parser | parsed | parse_failed |
|---|---|---|---|---|---|
| `conn.log` | 5129 | 0 | 5129 | 0 | 0 |
| `dns.log` | 3409 | 0 | 3409 | 0 | 0 |
| `http.log` | 1545 | 0 | 1545 | 0 | 0 |
| `ssl.log` | 169 | 0 | 169 | 0 | 0 |
| `files.log` | 1447 | 0 | 1447 | 0 | 0 |
| `syslog.log` | 26 | 0 | 26 | 0 | 0 |
| `weird.log` | 18 | 0 | 18 | 0 | 0 |
| `packet_filter.log` | 10 | 0 | 10 | 0 | 0 |
| `json/conn.log` | 5120 | 0 | 5120 | 0 | 0 |
| `json/dns.log` | 3400 | 0 | 3400 | 0 | 0 |
| `json/http.log` | 1536 | 0 | 1536 | 0 | 0 |
| `json/ssl.log` | 160 | 0 | 160 | 0 | 0 |
| `json/files.log` | 1438 | 0 | 1438 | 0 | 0 |
| `json/syslog.log` | 17 | 0 | 17 | 0 | 0 |
| `json/weird.log` | 9 | 0 | 9 | 0 | 0 |
| `json/packet_filter.log` | 1 | 0 | 1 | 0 | 0 |

**100% `no_parser`, 0 `parse_failed` on every file** — the correct result for a format
outside the twelve. `framed` equals the raw line count in every case (no folding, no CRLF).

## Inference (`ulpf infer <file> --decisions`) — the live demo material

| file | lines | lines covered | templates | unmatched (by reason) |
|---|---|---|---|---|
| `conn.log` | 5129 | 5096 | 5 | 33 (`below_support` 9, `no_template` 24) |
| `dns.log` | 3409 | 3400 | 3 | 9 (`below_support` 9) |
| `ssl.log` | 169 | 160 | 1 | 9 (`below_support` 9) |
| `files.log` | 1447 | 1433 | 2 | 14 (`below_support` 9, `no_template` 5) |
| `syslog.log` | 26 | 17 | 1 | 9 (`below_support` 9) |
| `weird.log` | 18 | 7 | 2 | 11 (`below_support` 11) |
| `packet_filter.log` | 10 | 0 | 0 | 10 (`below_support` 10) |
| `http.log` | 1545 | 100 | 40 | 1445 (`below_support` 47, `no_template` 44, `template_cap` 1354) |
| `json/conn.log` | 5120 | 5096 | 1 | 24 (`no_template` 24) |
| `json/dns.log` | 3400 | 3400 | 3 | 0 |
| `json/http.log` | 1536 | 1448 | 1 | 88 (`no_template` 88) |
| `json/ssl.log` | 160 | 160 | 1 | 0 |

**The recurring `below_support 9` is not noise — it is Zeek's own metadata header.** Every
TSV file carries exactly 9 `#`-prefixed lines (`#separator \x09`, `#set_separator`,
`#empty_field`, `#unset_field`, `#path`, `#open`, `#fields`, `#types`, `#close`), each a
one-off with no sibling to cluster with, so all 9 land in `below_support` and stay out of the
proposal. `grep -c '^#' ssl.log` = 9 and 169 − 160 = 9 confirm the accounting exactly. That
is the right outcome — a header is not an event — and it is the clearest single thing to show
a reviewer: the engine templated 160/160 of the real `ssl.log` events with one pattern and
quarantined the file's metadata by itself.

Highlights:
- **`ssl.log`**: one template, support 160, verified 160, every real event covered —
  `... TLSv13 TLS_AES_256_GCM_SHA384 x25519 {x25519:word} F - - T CsiI - - -`.
- **`dns.log`**: 3 templates, 3400/3409 covered — split correctly on the response outcome
  (`NXDOMAIN` support 4, `NOERROR` with no answer support 1694, `NOERROR` with an answer
  address support 1702), because the disposition token is constant inside each cluster.
- **`conn.log`**: 5 templates split on `proto`/`service`/`conn_state` (`tcp http … RSTO`,
  `udp dns … SHR`, `tcp - … S0`, `udp … S0`, `tcp … SF`) — again a split on the constant
  disposition token, which is the behaviour `docs/DECISIONS.md` D46 describes.
- **`http.log` is the honest failure case and the best review-screen demo**: Zeek's TSV
  `http.log` puts the URI, the referrer and the User-Agent inline as unquoted, space-free-ish
  positional columns, so the clusterer keys on the literal path and referrer text and
  fragments into per-URL templates — it hit the template cap with 40 templates covering only
  100 lines and 1,354 lines rejected as `template_cap`. This is exactly the case a human is
  supposed to catch on the review screen (the proposal is not approved, so nothing is ever
  parsed by it). The **JSON form of the same file is the fix**: `json/http.log` collapses to
  **1 template covering 1,448 of 1,536 lines**, because JSON quotes the variable text and the
  slot regexes see `{quoted:quoted}` instead of a bare path.
- **JSON form generally**: `json/conn.log` 1 template / 5,096 lines, `json/dns.log` 3
  templates / 3,400 lines with **0 unmatched**, `json/ssl.log` 1 template / 160 lines with
  **0 unmatched** — the JSON logs have no `#` header, which is why their `below_support`
  count is 0 across the board.
- **`packet_filter.log`** is 9 header lines and 1 data line: 0 templates, 10 unmatched. A
  10-line file has nothing to cluster, and the engine says so rather than inventing a
  pattern from a single sample.
