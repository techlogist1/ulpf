# PROVENANCE — corpus/real/squid_access (label: web-squid-suricata-openvpn)

Squid native `access.log` format is `%ts.%03tu %6tr %>a %Ss/%03>Hs %<st %rm %ru %[un %Sh/%<a %mt`
per `parsers/squid_access.toml`. All three files below are that exact wire format,
one event per line, byte-for-byte as fetched (a CSV/JSON unwrap for the first two —
see "how fetched" — never a hand edit of the log bytes themselves).

| file | source URL | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `azuresentinel_asim_websession.log` | https://github.com/Azure/Azure-Sentinel/blob/main/Sample%20Data/ASIM/Squid_Squid%20Proxy_WebSession_IngestedLogs.csv | commit `main` HEAD at fetch time 2026-09-05 (repo has no per-file pinned SHA visible via API; fetched via GitHub Contents API) | `Sample Data/ASIM/Squid_Squid Proxy_WebSession_IngestedLogs.csv`, `RawData` column | MIT (root `LICENSE` read via `gh api repos/Azure/Azure-Sentinel/contents/LICENSE`, Microsoft Corporation) | client IPs replaced with RFC 1918 ranges (192.168.1.x, 10.1.1.4); destination domains (ipinfo.io, ocsp.swiss.com, google.com, westeurope.monitoring.azure.com) and one proxy username ("proxyuser") left as observed; this is Microsoft's Sentinel-solution sample data exported from a real Squid VM's access log | sanitized-real | 12 | `gh api "repos/Azure/Azure-Sentinel/contents/Sample Data/ASIM/Squid_Squid Proxy_WebSession_IngestedLogs.csv" --jq '.content'` then `base64 -D`, then the `RawData` field extracted per row with Python's `csv` module (one field per line, no other transform) |
| `usiem_squid_docker_native.log` | https://github.com/u-siem/usiem-squid/blob/main/docker/logs.log | commit `main` HEAD at fetch time 2026-09-05 | `docker/logs.log` (native-format lines only; the file also interleaves squidGuard redirector log lines in a different format, which were excluded, not altered) | MIT (`gh api repos/u-siem/usiem-squid --jq '.license'`) | none apparent — `172.17.0.1` is the Docker bridge gateway (not a sanitization, just what a container-to-host proxy request looks like); this is the maintainer's own captured output from a real Squid instance in their Docker test container, committed to test their Rust parser | real-capture | 7 | `gh api .../contents/docker/logs.log --jq '.content' \| base64 -D`, then `grep -E '^[0-9]{10}\.[0-9]{3}'` to keep only the native squid_access lines (the squidGuard-format lines above them in the same file were dropped, not edited) |
| `packt_sc300_generated.log` | https://github.com/PacktPublishing/Microsoft-Identity-and-Access-Administrator-SC-300-Exam-Guide/blob/main/squid_logs.log | commit `main` HEAD at fetch time 2026-09-05 | `squid_logs.log` | MIT (`gh api repos/PacktPublishing/... --jq '.license'`) | n/a | **generated** (flagged, not messy real: response_time is `200` and bytes is `1234` on every single line, a fixed 10-line cycle of hostnames repeated 36×; this is training-book demo data, not a real capture — kept only as bulk happy-path volume, task explicitly prefers messy real over this) | 360 | `gh api repos/PacktPublishing/.../contents/squid_logs.log --jq '.content' \| base64 -D`, verbatim, no line dropped |

## Licences checked and rejected

| candidate URL | licence found | verdict |
|---|---|---|
| `elastic/integrations` `packages/squid/_dev/deploy/docker/sample_logs/squid-log-access.log` and `squid-log-access-extensive.log` | Elastic License 2.0 (root `LICENSE.txt`: "licensed under the Elastic License Version 2.0, unless otherwise noted"; no override `LICENSE.txt` found in `packages/squid` subtree) | rejected — not copied, recorded in `not_obtained` |
| `elastic/beats` `filebeat/module/*` | no `squid` module exists in current `filebeat/module` tree (checked via Contents API; only apache/auditd/elasticsearch/haproxy/icinga/iis/kafka/kibana/logstash/mongodb/mysql/nats/nginx/osquery/pensando/postgresql/redis/santa/system/traefik) | n/a — moved to elastic/integrations (Elastic-2.0, see above) |
| `DataDog/integrations-core` `squid/` | BSD-3-Clause (root `LICENSE`) | checked but contains no log samples — it's an SNMP/HTTP metrics check (`squid/tests/`: only `.py` test files, no `.log` fixtures) |
| `jupyterj0nes/masstin` `rules/proxy/samples/squid.sample.log` | AGPL-3.0 (`gh api repos/jupyterj0nes/masstin --jq '.license'`) | rejected — copyleft, not on the permissive list |
| `linux8a/Docker-Cuba` `Squid/squid-4.11/squid/log/access.log` | GPL-3.0 | rejected |
| `RichMix/logAnalysisTools` `squid_access.log` | AGPL-3.0 | rejected |
| SecRepo.com | CC-BY-4.0 site-wide (per site's own statement, fetched via WebFetch) but its dataset index (`data/site-links.json`) lists no Squid-specific dataset | nothing to take |
