# Samples

Each file is a synthetic block written from the vendor's public log reference during
the v0.1 build and deliberately dirtied (truncated lines, a non-UTF-8 byte, CRLF
terminators, header variants, no-year timestamps), followed — where a permissively
licensed real capture exists in `corpus/` — by a REAL block appended at the end. The
synthetic lines stay: they cover shapes the real captures do not (deliberate damage,
alternate framings, message ids the capture never emitted). The `real lines` column
below names the line range and where those bytes came from; `corpus/README.md` has the
full index with licences.

Samples are one line per *shape*, not bulk volume: the corpus is where volume lives.
Every real line here was reviewed against the vendor's own format before its fixture
line was committed (D30).

One `samples/<parser>.log` per `parsers/<parser>.toml`, with expected output in
`fixtures/<parser>.expected.jsonl`.

| sample | real lines (appended at the end) |
|---|---|
| `cisco_asa.log` | 19-30 — `corpus/real/cisco_asa/*` (splunk/attack_data, Apache-2.0). Lines 19-24, 26-30 are the `logging emblem` + `logging timestamp rfc5424` form (`:<RFC3339>: %ASA-…`), line 25 the same timestamp option without EMBLEM (`<RFC3339> ciscoasa : %ASA-…`). Message ids 111008/111009/111010/113006/302013/302014/502101/502102/502103/609002/710005. |
| `cisco_ios.log` | 28-32 — `corpus/real/cisco_ios/cisco_ios_sisf_t1557.log` (splunk/attack_data, Apache-2.0): SISF device-tracking IP_THEFT / MAC_THEFT / MAC_AND_IP_THEFT and two PAK_DROP shapes. |
| `fortinet_fortigate.log` | 9-12 — line 9 `corpus/real/fortinet_fortigate/fortigate_sample_sva_s1.log` (sva-s1/sentinelone-syslog-toolkit, MIT), lines 10-12 `fortigate_forum_quotes.log` (poster-redacted mailing-list quotes): one FortiOS 7.x traffic event, one 7.x SSL-VPN login failure, two pre-5.0 events with the legacy `device_id=`/`log_id=` key spelling. |
| `openvpn.log` | 29-44 — lines 29-34 `corpus/real/openvpn/*` (Azure-Sentinel and DFIRArtifactMuseum, both MIT): the `--syslog` wire form, tag with and without `[pid]`, peer prefix in all three shapes plus a daemon line with none. Lines 35-41 `corpus/generated/openvpn/server.log` and 42-44 `server-2.5.log` (generated locally from real OpenVPN 2.6.14 and 2.5.1 containers): the ISO 8601 file-log stamp OpenVPN 2.5 replaced ctime with, both daemon banners, and the 2.6 `Data Channel:`/`Control Channel:` summary lines beside 2.5's `Outgoing Data Channel: Cipher … initialized`. |
| `palo_alto_panos.log` | 20-22 — `corpus/real/palo_alto_panos/palo_alto_panos.log` (chronosphereio/processing-templates, Apache-2.0): TRAFFIC start and end rows from a VM-Series with an empty Serial Number column. |
| `sonicwall.log` | 16-19 — `corpus/real/sonicwall/sonicwall.log` (CrowdStrike Unlicense / DataDog BSD-3-Clause blocks): the four `src`/`dst` shapes with an empty part — no interface, empty v4 address, empty port, empty address with a VLAN-suffixed interface. |
| `squid_access.log` | 28-33 — lines 28-29 `corpus/real/squid_access/usiem_squid_docker_native.log` (MIT), line 30 `azuresentinel_asim_websession.log` (MIT), lines 31-33 `corpus/generated/squid/access.log` (generated locally from a real Squid 6 container): `HIER_DIRECT`/`HIER_NONE` hierarchy codes, `TCP_TUNNEL_ABORTED`, `NONE/503`, `NONE_NONE/400` with a `-` method. |

No real block yet (synthetic only): `check_point.log`, `juniper_srx.log`,
`pfsense_filterlog.log`, `sophos_xg.log`, `suricata_eve.log` — see `corpus/README.md`
for what exists for each and why it was not promoted. `cef.log`, `leef.log` and
`cloudtrail.log` (added 2026-09-06) are built from the specifications' own example lines
and records, extended with the dictionary keys the specifications define; no capture.

| sample | written from | deliberately dirty |
|---|---|---|
| `cisco_asa.log` | Cisco Secure Firewall ASA Syslog Messages guide, reviewed against it 2026-09-05 | the `logging timestamp rfc5424` form, a header-less buffer line, a relay-rewritten BSD header, no-NAT build, `%ASA-auth-` and `%FTD-` headers, the documented comma form of 113004, 106100 without the hash pair, a truncated 302013, a non-UTF-8 byte in a user name, a message id with no sub |
| `cisco_ios.log` | Cisco IOS System Message Guide; `service timestamps`/`sequence-numbers`/`origin-id` docs | CRLF, uptime stamp (`1d03h:`), no sequence number, a stamp with the year before the time, truncated message, non-UTF-8 byte, a bare `%SEC-` line with no `<pri>` and no header, a relay-prefixed line, `RESTART` with no sub |
| `fortinet_fortigate.log` | FortiOS Log Reference (traffic, utm, event), reviewed 2026-09-05 | one line folded by a collector (FortiOS itself never wraps; the framing rule must still keep it with its event), a non-UTF-8 byte in an admin name (only plausible from a non-UTF-8 auth source; deliberate), an `action=acc` typo, CRLF, a relayed BSD header, a config-change event with an escaped quote |
| `openvpn.log` | OpenVPN 2.6 manual, `Changes.rst` (the 2.5 ISO 8601 log-format change) and source message strings (`init.c`, `multi.c`, `crypto.c`, `ssl_openssl.c`) | CRLF, non-UTF-8 bytes in a common name (two lines), truncated line, daemon lines with no peer, all three wire forms (ctime file log, ISO 8601 file log, `--syslog`) |
| `palo_alto_panos.log` | PAN-OS 10.2 Syslog Field Descriptions (Traffic, Threat, System, Config) | IETF (RFC 5424) header, CRLF, non-UTF-8 byte in `srcuser`, a row cut at column 30, a header-less row, a USERID row no sub covers, a quoted URL with a comma |
| `pfsense_filterlog.log` | Netgate "Raw Filter Log Format", captured IPv6 rows | RFC 3164 (with and without hostname) and RFC 5424 framings, CRLF, a row ending in a delimiter, a row cut inside the common columns, non-UTF-8 byte in the interface name, no-`<pri>` line, TCP/UDP/ICMP/ICMPv6/CARP/GRE tails |
| `check_point.log` | Check Point Log Exporter (sk122323), R81 log fields | the exporter's default space-separated timestamp on most lines and an RFC 3339 one, CRLF, non-UTF-8 byte in `src_machine_name`, a body missing its closing `]`, a trailing `;` before `]`, a `\]` escape inside a value |
| `juniper_srx.log` | Junos RT_FLOW session and RT_IDP attack log field references | structured and unstructured forms of the same events, CRLF, non-UTF-8 byte in `username`, truncated structured data |
| `sonicwall.log` | SonicOS Log Events Reference Guide, captured SonicOS 6/7 lines | no syslog header (as the device sends it) and a relayed header, four-part `src` with a VLAN interface suffix, a bare `src` on a drop, zone and NAT fields, `time` without a zone, CRLF, non-UTF-8 byte in `usr`, truncated line |
| `sophos_xg.log` | SFOS 21.5 syslog guide (Firewall, Content Filtering, ATP, IDP, Event) | the wire form `<30> device="SFW" ...` with no syslog header, two relayed lines with one, empty values (`key=` and `key=""`), an ambiguous zone name (`IST`), a truncated line, a non-UTF-8 byte in a user name |
| `squid_access.log` | Squid `access.log` native format (`%ts.%03tu %6tr %>a %Ss/%03>Hs %<st %rm %ru %[un %Sh/%<a %mt`), LogTags.cc and hier_code.h for the codes | two lines no parser claims, a truncated line, a tab in the spacing, a non-UTF-8 byte inside a result code, CRLF, an IPv6 client, a `NONE_NONE/000` reset |
| `suricata_eve.log` | Suricata EVE JSON format reference and output-json-*.c | alert (with metadata arrays and community_id), dns v3 request/response and a legacy v1 query, http, tls, flow; a minimal alert, an event with no `timestamp`, a non-UTF-8 byte in a user agent |
| `cef.log` | ArcSight "CEF Implementation Standard" (SmartConnectors 8.4), chapter 1 header and examples, chapter 2 extension dictionary, fetched 2026-09-06 | the spec's own `worm successfully stopped` line behind its syslog prefix (its `Sep 19` moved to `Sep 3` so the sample keeps one date), `rt` as epoch milliseconds and as `MMM dd yyyy HH:mm:ss`, escaped `\|` in the header, `\=` and `\\` in values, no extension (the one line that falls to the receipt time and Base Event), `<pri>` + RFC 3164 and RFC 5424 prefixes, an extension value carrying `LEEF:2.0|...`, CRLF, a non-UTF-8 byte in `suser`, a line cut mid-extension |
| `leef.log` | IBM QRadar DSM guide, "LEEF event components" and "Predefined LEEF event attributes", fetched 2026-09-06 | LEEF 1.0 with tabs, 2.0 with `^`, with the hex form in both spellings the spec allows (`x5E` and `0x5E`), with an empty delimiter field and with none, IBM's own attribute example and syslog prefixes (`<13>Jan 18 11:07:53 192.168.1.1`, the RFC 5424 one), devTime as epoch seconds and milliseconds and in five declared layouts plus one the candidates do not cover (falls to the receipt time), `isLoginEvent`/`isLogoutEvent`, NAT and MAC attributes, `\|` and `\=` escapes, a value carrying `CEF:0|...`, CRLF, a non-UTF-8 byte in `usrName` |
| `cloudtrail.log` | AWS CloudTrail User Guide record contents, userIdentity, log file examples and console sign-in pages; IAM User Guide CloudTrail integration; fetched 2026-09-06 | the guides' own records one per line: ConsoleLogin success and failure (IAM and root), IAM CreateUser, EC2 StartInstances with nested `instancesSet.items`, STS AssumeRole from both the caller's and the role owner's side (eventVersion 1.05, no `readOnly`), an `errorCode` record, a read-only `GetUserPolicy`, AssumeRoleWithSAML with `sourceIPAddress` "AWS Internal", root EnableMFADevice, a 1.03 S3 ListBuckets; composed after the reference: an S3 GetObject data event and an EC2 RunInstances; one record pretty-spaced with CRLF. No non-UTF-8 byte: a JSON record with one is `invalid_json` by design |
