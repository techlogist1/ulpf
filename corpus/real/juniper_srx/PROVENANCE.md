# Provenance — juniper_srx

| file | source URL | revision | path in source | licence | anonymised by source | kind | lines | fetch method |
|---|---|---|---|---|---|---|---|---|
| `juniper_srx.log` | https://github.com/Azure/Azure-Sentinel/blob/master/Sample%20Data/Syslog/JuniperSRX.json | Azure/Azure-Sentinel @ master, fetched 2026-09-05 (GitHub API blob returned for that ref at fetch time) | `Sample Data/Syslog/JuniperSRX.json` | MIT (root `LICENSE`, read at `https://github.com/Azure/Azure-Sentinel/blob/master/LICENSE` — "MIT License, Copyright (c) Microsoft Corporation") | Hostnames are lab names (`Daejeon_FW1`, `JuniperSRX_FW4`, `SRX-GW1`-style); source/dest IPs are RFC1918 or documentation ranges (`172.20.x.x`, `192.168.x.x`, `239.255.255.250`); no real usernames present (`N/A` throughout) | generated | 284 | `gh api "repos/Azure/Azure-Sentinel/contents/Sample Data/Syslog/JuniperSRX.json" --jq .content \| base64 -d`, then extracted the `SyslogMessage` value from every JSON record in file order with `python3 -c "import json; ..."` (script in scratch dir) — no line invented, every byte is the value Microsoft shipped in that field |

## Why "generated", not "real-capture" or "sanitized-real"

This is Microsoft's own sample-data set for exercising the Sentinel Syslog data
connector's Juniper SRX workbook/parser (`Sample Data/Syslog/ReadMe.md`: "tracks
sample data of Syslog format and can be pushed to Azure Log Analytics Syslog
table"). It is not a documentation code example (it is far too repetitive and
"live-looking" for that — sequential decrementing timestamps, a handful of fixed
hostnames, obvious placeholder externals), but comparing it against Juniper's own
documented wire format shows it is not byte-faithful to a real SRX either — see
validation below. Treated conservatively as `generated`.

## What this file actually contains

284 `SyslogMessage` values, unfiltered, in the order Microsoft's JSON has them:
- 265 `RT_IDS` lines (`IP spoofing! source: ..., destination: ..., protocol-id: ...`)
- 17 `RT_FLOW` lines (`RT_FLOW session created ...` / `session closed ...`), including
  one line with the 17.3+ two-word NAT rule type (`source rule source-nat-rule`)
- 2 `sshd` lines (unrelated to Junos RT_FLOW/RT_IDP/RT_UTM)

This is real "collector mess" in the sense the brief wants (mixed event types, one
source arriving alongside noise the parser was never meant to cover) even though the
RT_FLOW payload itself is a simplified rendering of the real format (below).
