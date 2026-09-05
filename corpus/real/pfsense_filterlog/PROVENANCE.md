# Provenance — corpus/real/pfsense_filterlog/pfsense_filterlog.log

30 lines total. Every line below was fetched byte-exact via `gh api repos/<owner>/<repo>/git/blobs/<sha>`
(GitHub git-blob API, base64-decoded) or `contents/<path>` for small files — never retyped from memory.
Assembled with a short Python script that concatenated the fetched files in order; no line content was
altered except where noted.

| lines | file / source | source URL | revision | path in source | licence | what the source anonymised | kind | fetched via |
|---|---|---|---|---|---|---|---|---|
| 1-9 | `crowdsec_pf-logs.log` | https://github.com/crowdsecurity/hub | commit `31d852a3737c12ee095bee18ebc2942fb6707e8d` (master, 2026-09-05) | `.tests/pf-logs/pf-logs.log` | MIT — read at https://github.com/crowdsecurity/hub/blob/master/LICENSE | Nothing — private lab addresses (10.0.2.2/10.0.2.15, a VirtualBox-NAT-range pfSense test box) already appear in the clear; this is the CrowdSec parser test fixture for the community `firewallservices/pf-logs` parser. Mixes classic BSD-header pfSense lines with RFC 5424 OPNsense lines carrying `[meta sequenceId=...]` structured data. | real-capture | `gh api repos/crowdsecurity/hub/contents/.tests/pf-logs/pf-logs.log --jq '.content' \| base64 -d` |
| 10-25 | `crowdsec_pf-scan-multi-port.log` | https://github.com/crowdsecurity/hub | commit `31d852a3737c12ee095bee18ebc2942fb6707e8d` | `.tests/pf-scan-multi-port/pf-scan-multi-port.log` | MIT (same as above) | Nothing further anonymised beyond the lab addressing already in the capture (same 10.0.2.2 → 10.0.2.15 pair as pf-logs.log, a multi-port-scan burst against the same box). | real-capture | same method |
| 26-29 | `crowdsec_pfsense-rfc5424-filterlog.log` | https://github.com/crowdsecurity/hub | commit `31d852a3737c12ee095bee18ebc2942fb6707e8d` | `.tests/pfsense-rfc5424-filterlog/pfsense-rfc5424-filterlog.log` | MIT (same as above) | Nothing — real public IPv4/IPv6 addresses (e.g. `212.102.36.22`, `37.120.48.198`) and a real hostname `firewall.lan` appear in the clear. Two distinct captures concatenated by the source repo itself (2022-09-06 batch with real WAN traffic; 2025-11-06 batch, private `192.168.x` LAN, hostname `firewall.lan`). RFC 5424 framing, no structured data (`- -`), unlike the pf-logs.log batch. | real-capture | same method |
| 30 | SC4S pfsense test comment | https://github.com/splunk/splunk-connect-for-syslog | commit `d861082778c4ce35a1636dc83e19763338bf662e` (main, 2026-09-05) | `tests/test_pfsense.py`, line 18 (a `#`-comment directly above the templated test case) | Apache License 2.0 — read at https://github.com/splunk/splunk-connect-for-syslog/blob/main/License.md (the file's own header additionally claims a BSD-2-Clause-style licence; both are permissive) | Nothing — real IPv6 link-local addresses (`fe80::208:a2ff:fe0f:cb66` etc.) from the SC4S author's own pfSense box, left in the source as a worked example above the parameterised pytest case. Only the leading `# ` comment marker was stripped to turn it back into a bare log line — no other byte changed. | real-capture | `gh api repos/splunk/splunk-connect-for-syslog/contents/tests/test_pfsense.py --jq '.content' \| base64 -d`, then `grep`/extract the `# <27>...` line and drop `# ` |

## Rejected / checked but not copied (see `not_obtained` in the run summary for the full list)
- `elastic/integrations` `packages/pfsense_syslog/...` and beats `x-pack/filebeat/module/pfsense/...` — repo-default licence is **Elastic License 2.0** (confirmed via `LICENSE.txt` at the repo root; no package-level override found for pfsense). Not copied.
- `kurobeats/pfSense-filterlog-extractor` (GitHub) — **GPL-3.0**. Not copied.
- Assorted personal `logstash-pfsense` config repos (`threesquared/docker-logstash-pfsense`, `jtmpu/logstash-pfsense`, `vhondo/logstash-pfsense`, `alkivi-sas/logstash-pfsense`) — no LICENSE file / `NOASSERTION`. Not copied.
- `pfsense/docs` (Netgate's own "Raw Filter Log Format" page) — this is exactly the source the existing `samples/pfsense_filterlog.log` doc-example was already written from; re-fetching it would add no new real-capture value, so skipped in favour of the CrowdSec real captures above.

## Validation (baseline binary, `--infer-threshold 0`)
```
stages: framed 30  stored 30  detected 30  no_parser 0  parsed 30  parse_failed 0  normalized 30  emitted 30
signals: sub_matched 30  sub_no_match 0  sub_uncovered 0  time_from_receipt 0  class_unknown 0  unmapped_fields 535
```
100% detected/parsed/normalized, zero breakage. The RFC 5424 lines with `[meta sequenceId="..."]` structured
data (crowdsec_pf-logs.log lines 4-9) still parse cleanly because the pfsense_filterlog parser's `[strategy]`
falls back to a bare `{csv:rest}` pattern once the envelope layer has stripped the syslog header/structured-data
block — the CSV body after it is untouched by the structured-data block, so nothing broke here. No fields
lost; `unmapped_fields` is just the many TCP/ICMP detail columns (seq/ack/window/options, ICMP timestamps,
CARP advbase/advskew) that `mappings/ocsf.toml` intentionally leaves unmapped in this pragmatic OCSF subset.
