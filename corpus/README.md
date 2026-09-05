# Corpus — real and locally generated captures

Index of every log file under `corpus/`. This directory is the *volume* input: bulk
bytes to run the pipeline against, not the reviewed one-line-per-shape material in
`samples/`. It is not loaded by `run`, `serve` or any test; nothing here is committed
to the fixture harness unless a line was promoted into `samples/<parser>.log` and its
fixture line reviewed against the vendor format first (D30).

Each directory has its own `PROVENANCE.md` with the source URL, revision, path, licence
text location, what the source anonymised, and the exact fetch or generation command.
That file is authoritative; this index summarises it.

**kind** — `real-capture` (a device or daemon wrote these bytes), `sanitized-real`
(the same, with addresses/names scrubbed by the source), `generated` (a real tool ran
and produced them locally, or a third party built representative data by hand),
`doc-example` (copied out of a vendor manual). `covered by` names the parser that
claims the file today, `none` means no parser family exists for that format.

Reproduce the counters with:

```
ulpf run corpus/<path> --store <scratch> --output <scratch>/x.jsonl --infer-threshold 0
```

## Real captures (`corpus/real/`)

| file | vendor / product | kind | lines | licence | covered by | counters |
|---|---|---|---|---|---|---|
| `cisco_asa/cisco_asa_arcane_door.log` | Cisco ASA | real-capture | 287 | Apache-2.0 (splunk/attack_data) | `cisco_asa` | 287 parsed, 287 sub_matched, 0 no_parser |
| `cisco_asa/cisco_asa_generic_logs.log` | Cisco ASA | real-capture | 48 | Apache-2.0 (splunk/attack_data) | `cisco_asa` | 48 parsed, 48 sub_matched, 0 no_parser |
| `cisco_ios/cisco_ios_sisf_t1557.log` | Cisco IOS (SISF) | real-capture | 5 | Apache-2.0 (splunk/attack_data) | `cisco_ios` | 5 parsed, 5 sub_matched |
| `cisco_ios/cisco_ios_siemlite_generated.log` | Cisco IOS | generated | 4 | MIT (Arpitapaaul/SIEM-Lite) | `cisco_ios` | 4 parsed, 4 sub_matched |
| `check_point/` | Check Point | — | 0 | — | `check_point` | no file: no permissively licensed Log Exporter "syslog"-format capture found |
| `fortinet_fortigate/fortigate_sample_sva_s1.log` | Fortinet FortiGate 7.x | real-capture | 1 | MIT (sva-s1/sentinelone-syslog-toolkit) | `fortinet_fortigate` | 1 parsed |
| `fortinet_fortigate/fortigate_forum_quotes.log` | Fortinet FortiGate (1 × 7.x, 3 × pre-5.0) | sanitized-real | 4 | mailing-list / forum quotes, redacted by their posters | `fortinet_fortigate` | 4 parsed (was 3 no_parser before the legacy `log_id=` matcher fix) |
| `juniper_srx/juniper_srx.log` | Juniper SRX (Junos) | generated | 284 | MIT (Azure/Azure-Sentinel sample data) | `juniper_srx` (282), none (2 sshd) | 282 parsed but **282 sub_uncovered** — see note below |
| `openvpn/azuresentinel_openvpn_syslog.log` | OpenVPN 2.x via syslog | sanitized-real | 32 | MIT (Azure/Azure-Sentinel) | `openvpn` | 32 parsed, 28 sub_matched, 4 sub_no_match |
| `openvpn/dfir_gemini_openvpn_syslog.log` | OpenVPN 2.x via syslog | real-capture | 5 | MIT (AndrewRathbun/DFIRArtifactMuseum) | `openvpn` | 6 parsed, 2 sub_matched, 4 sub_no_match |
| `palo_alto_panos/palo_alto_panos.log` | Palo Alto PAN-OS (VM-Series) | sanitized-real | 50 | Apache-2.0 (chronosphereio/processing-templates) | `palo_alto_panos` | 50 parsed, 50 sub_matched |
| `pfsense_filterlog/pfsense_filterlog.log` | Netgate pfSense / OPNsense | real-capture | 30 | MIT (crowdsecurity/hub), Apache-2.0 (splunk/splunk-connect-for-syslog) | `pfsense_filterlog` | 30 parsed, 30 sub_matched |
| `sonicwall/sonicwall.log` | SonicWall SonicOS | real-capture (2), sanitized-real (10), generated (3) | 15 | Unlicense (CrowdStrike), Apache-2.0 (aicers), MIT (u-siem), BSD-3-Clause (DataDog) | `sonicwall` | 15 parsed, 15 sub_matched |
| `sophos_xg/sophos_xg.log` | Sophos XG (SFOS) | sanitized-real | 1 | MIT (Azure/Azure-Sentinel) | `sophos_xg` | 1 parsed |
| `squid_access/usiem_squid_docker_native.log` | Squid 4/5 | real-capture | 7 | MIT (u-siem/usiem-squid) | `squid_access` | 7 parsed, 7 sub_matched |
| `squid_access/azuresentinel_asim_websession.log` | Squid | sanitized-real | 12 | MIT (Azure/Azure-Sentinel) | `squid_access` | 12 parsed, 12 sub_matched |
| `squid_access/packt_sc300_generated.log` | Squid | generated | 360 | MIT (PacktPublishing) | `squid_access` | 360 parsed, 360 sub_matched |
| `suricata_eve/ccdcoe_cdmcs_eve.json` | Suricata EVE JSON | real-capture | 314 | MIT (ccdcoe/CDMCS) | `suricata_eve` | 314 parsed |

## Generated locally (`corpus/generated/`)

Real tools, real containers, real traffic on this machine on 2026-09-05; each
directory's `setup/` reproduces it. No third-party licence applies.

| file | tool / product | kind | lines | covered by | counters |
|---|---|---|---|---|---|
| `openvpn/server.log` | OpenVPN 2.6.14 server, `--log-append` | generated | 3937 | `openvpn` | 3937 parsed, 3146 sub_matched, 791 sub_no_match (was **3937 no_parser** before the ISO 8601 file-log fix) |
| `openvpn/client.log` | OpenVPN 2.6.14 clients (4 variants concatenated) | generated | 7329 | `openvpn` | 7329 parsed, 1940 sub_matched, 5389 sub_no_match (was **7329 no_parser**) |
| `openvpn/server-2.5.log` | OpenVPN 2.5.1 server, `--log-append` | generated | 565 | `openvpn` | 565 parsed, 457 sub_matched, 108 sub_no_match (was **565 no_parser**) |
| `openvpn/server-2.4-ctime.log` | OpenVPN 2.4.12 server, `--log-append` (ctime prefix) | generated | 674 | `openvpn` | 674 parsed, 613 sub_matched, 61 sub_no_match |
| `openvpn/server-syslog.log` | OpenVPN 2.6.14 server, `--syslog` + rsyslog RFC 3164 | generated | 1044 | `openvpn` (1043), none (1 rsyslogd) | 1043 parsed, 825 sub_matched, 218 sub_no_match |
| `squid/access.log` | Squid 6.13 (`docker.io/ubuntu/squid`) | generated | 16500 | `squid_access` | 16500 parsed, 16500 sub_matched |
| `squid/cache.log` | Squid 6.13 cache log | generated | 19774 | none | 19774 no_parser (a different format from `access.log`; no parser family) |
| `suricata/eve.json` | Suricata 8.0.6 (`jasonish/suricata`) | generated | 2924 | `suricata_eve` | 2924 parsed |
| `nginx/nginx1-access.log` | nginx 1.27.5 combined access log | generated | 1548 | none | 1548 no_parser |
| `nginx/nginx2-access.log` | nginx 1.27.5 combined access log | generated | 1407 | none | 1407 no_parser |
| `nginx/nginx1-error.log` | nginx 1.27.5 error log | generated | 126 | none | 126 no_parser |
| `nginx/nginx2-error.log` | nginx 1.27.5 error log | generated | 189 | none | 189 no_parser |
| `haproxy/haproxy.log` | HAProxy 2.9.15 `option httplog`/`tcplog` over RFC 3164 syslog | generated | 2356 (1178 events) | none | 1178 no_parser |
| `zeek/{conn,dns,files,http,ssl,syslog,weird,packet_filter}.log` | Zeek 8.2.2 TSV logs | generated | 11748 | none | all no_parser |
| `zeek/json/{same}.log` | Zeek 8.2.2 JSON logs | generated | 11681 | none | all no_parser |

The nginx, HAProxy and Zeek files are the honest uncovered set: ULPF ships twelve
perimeter-device families and none of them is a web server, a load balancer or a
network-security monitor. They are the natural input for `ulpf infer` / the review
workflow rather than for a hand-written parser, and they are the reason the
inference demo has real unknown material to cluster.

## Notes on the two files that still show a signal

**`juniper_srx/juniper_srx.log` — 282 sub_uncovered, deliberately not "fixed".**
Every real Junos device writes a security log as `RT_<CATEGORY>: RT_<EVENT_NAME>:
<body>` (e.g. `RT_IDS: RT_SCREEN_TCP: TCP port scan! source: …`, `RT_FLOW:
RT_FLOW_SESSION_CREATE: session created …`). Microsoft's Azure-Sentinel sample data
drops both colons and the event name, leaving `RT_IDS IP spoofing! source: …`, so the
parser extracts no `event` and no per-event sub is gated. `RT_IDS` was added to the
matcher and a documented `RT_SCREEN_*` sub written (both correct for a real device),
but the strategy was **not** loosened to accept the colon-less rendering: that would
be fitting the parser to one vendor's demo file rather than to Junos. The two `sshd`
lines are correctly `no_parser` — they are a Unix daemon's messages that happen to
share the SRX's syslog stream, exactly the mixed-source mess the review workflow exists
for.

**`openvpn/*` — sub_no_match is the designed signal, not a defect.** OpenVPN's `[[sub]]`
list is ungated (there is no message id to gate on), so a message body no sub models is
reported as `sub_no_match` rather than `sub_uncovered` — the prompt to write the next
pattern. Every line still parses: the timestamp, the peer prefix (`ip:port`,
`cn/ip:port`, `ip:port [cn]`) and the message body are extracted whatever the body says.

Four subs were added from the OpenVPN source rather than from these files — 2.6's
`Data Channel: cipher '%s'[, auth '%s'][, peer-id: %d]` summary line (`init.c`, the
replacement for 2.5's `Outgoing Data Channel: Cipher … initialized`), `print_details()`'s
`Control Channel: …` line (`ssl_openssl.c`), the duplicate-CN warning (`multi.c`) and the
HMAC failure (`crypto.c`) — which is what took `server.log` from 1299 to 791.

What is left unmodelled is mostly the *client* vocabulary (`client.log` is 7329 lines of
one) and verb-3 diagnostics: `UDPv4 link remote:`, `Socket Buffers:`, `Timers:`,
`Protocol options:`, `OPTIONS IMPORT:`, `net_iface_up:`, `TLS: move_session:`,
`VERIFY KU OK`, `Restart pause`, `event_wait :`. Modelling a client role is a separate
piece of work from the perimeter-device families this repo ships, and each string still
needs checking against the source before a sub is committed.
