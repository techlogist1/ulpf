# Samples

Every file here is SYNTHETIC. It was written from public vendor documentation during
the v0.1 build, then deliberately dirtied (truncated lines, a non-UTF-8 byte, CRLF
terminators, header variants, no-year timestamps) because documentation examples
cluster beautifully and real logs do not. Real samples collected by the team replace
these file-for-file; keep the same file names so fixtures keep matching.

One `samples/<parser>.log` per `parsers/<parser>.toml`, with expected output in
`fixtures/<parser>.expected.jsonl`.

| sample | written from | deliberately dirty |
|---|---|---|
| `cisco_asa.log` | Cisco Secure Firewall ASA Syslog Messages guide | RFC 5424 and header-less lines, a truncated 302013, a non-UTF-8 byte in a user name, a message id with no sub |
| `cisco_ios.log` | Cisco IOS System Message Guide; `service timestamps`/`sequence-numbers`/`origin-id` docs | CRLF, uptime stamp (`1d03h:`), no sequence number, year-first stamp, truncated message, non-UTF-8 byte, bare `%SYS-` line with no header, `RESTART` with no sub |
| `fortinet_fortigate.log` | FortiOS Log Reference (traffic, utm, event) | a multi-line `msg`, a non-UTF-8 byte, an `action=acc` typo, header variants |
| `openvpn.log` | OpenVPN 2.6 manual and source message strings | CRLF, non-UTF-8 byte in a common name, truncated line, daemon lines with no peer |
| `palo_alto_panos.log` | PAN-OS 10.2 Syslog Field Descriptions (Traffic, Threat, System, Config) | IETF (RFC 5424) header, CRLF, non-UTF-8 byte in `srcuser`, a row cut at column 30, a header-less row, a USERID row no sub covers, a quoted URL with a comma |
| `pfsense_filterlog.log` | Netgate "Raw Filter Log Format", captured IPv6 rows | RFC 3164 (with and without hostname) and RFC 5424 framings, a row ending in a delimiter, a row cut inside the common columns, non-UTF-8 byte in the interface name, no-`<pri>` line, TCP/UDP/ICMP/ICMPv6/CARP/GRE tails |
| `check_point.log` | Check Point Log Exporter (sk122323), R81 log fields | older exporter timestamp with a space, CRLF, non-UTF-8 byte in `srcname`, a body missing its closing `]`, a `\]` escape inside a value |
| `juniper_srx.log` | Junos RT_FLOW session and RT_IDP attack log field references | structured and unstructured forms of the same events, CRLF, non-UTF-8 byte in `username`, truncated structured data |
| `sonicwall.log` | SonicOS Log Events Reference Guide, captured SonicOS 6/7 lines | no syslog header (as the device sends it) and a relayed header, four-part `src` with a VLAN interface suffix, a bare `src` on a drop, zone and NAT fields, `time` without a zone, non-UTF-8 byte in `usr`, truncated line |
| `sophos_xg.log` | Sophos Firewall syslog guide | a zone name that is ambiguous (`IST`), a truncated line, a backslash in a user name |
| `squid_access.log` | Squid `access.log` native format (`%ts.%03tu %6tr %>a %Ss/%03>Hs %<st %rm %ru %un %Sh/%<a %mt`) | two lines no parser claims, a truncated line, a tab in the spacing, a non-UTF-8 byte, CRLF |
| `suricata_eve.log` | Suricata EVE JSON format reference | alert, dns, http, tls, flow event types; an event with no `timestamp` |
