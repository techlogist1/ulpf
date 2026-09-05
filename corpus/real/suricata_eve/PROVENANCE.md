# PROVENANCE — corpus/real/suricata_eve (label: web-squid-suricata-openvpn)

| file | source URL | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `ccdcoe_cdmcs_eve.json` | https://github.com/ccdcoe/CDMCS/blob/master/Suricata/data-exploration/eve.json | commit `master` HEAD at fetch time 2026-09-05 | `Suricata/data-exploration/eve.json` | MIT (`gh api repos/ccdcoe/CDMCS --jq '.license'` → `mit`; also root `LICENSE` file present) | none apparent — `host":"CDMCS"`, interface `enp0s3` and RFC 1918 addresses (10.0.2.15, 10.0.2.3) are simply what a NAT-networked training VM in the CCDCOE (NATO Cooperative Cyber Defence Centre of Excellence) "Cyber Defense Monitoring Course" produced when Suricata actually ran against real DNS/TLS/HTTP/SSH traffic (real destination `raw.githubusercontent.com`, real CNAME chain to `github.map.fastly.net`, real Fastly IPs) — a training-lab capture, not synthetic | real-capture | 314 | `gh api "repos/ccdcoe/CDMCS/contents/Suricata/data-exploration/eve.json" --jq '.content'` then `base64 -D`, verbatim, no line dropped or reordered |

Event-type mix (counted with a one-off Python script, not committed): `dns` 118,
`tls` 73, `flow` 79, `alert` 33, `http` 5, `fileinfo` 5, `ssh` 1 — genuinely messy,
matches the vocabulary spread the parser fixture expects (alert/dns/http/tls/flow).

## Licences checked and rejected / not obtained

| candidate URL | licence found | verdict |
|---|---|---|
| `elastic/integrations` `packages/suricata/data_stream/eve/_dev/test/pipeline/test-*.log` (test-eve-6-0.log, test-eve-alerts.log, test-eve-dns-4-1-4.log, test-eve-metadata.log, test-eve-small.log, test-eve-tls-empty-*.log) | Elastic License 2.0 (root `LICENSE.txt`; no override in `packages/suricata` subtree) | rejected — not copied, recorded in `not_obtained` (these look like the best-quality alternative if the lead has Elastic Agent licensing already in place for the demo) |
| `OISF/suricata-verify` (OISF's own regression-test repo, 1000 test dirs under `tests/`) | MIT-style permissive text (`LICENSE.txt`: "Copyright (C) 2017-2021 Open Information Security Foundation... Permission is hereby granted...") | checked but **not usable as-is**: none of the ~1000 test directories commit a generated `eve.json` — each ships only `suricata.yaml`/`test.yaml`/`test.rules`/the input `.pcap`; the expected `eve.json` is produced at test-run time by actually invoking the `suricata` binary, which is out of scope for a corpus fetch (would require installing/building Suricata) |
| `Azure/Azure-Sentinel` `Sample Data/Corelight/Corelight_v2_suricata_eve_CL.json` and `Sample Data/Custom/Corelight/Corelight_v3_suricata_eve_CL.json` | MIT (root `LICENSE`) | checked and **rejected as fabricated stub data**, not a real capture: v2 file is a single record with literal placeholder strings (`"_system_name": "sample string"`, `"raw_alert": "sample string"`); v3 file's `raw_alert` fields are literally `"raw_alert_0"`..`"raw_alert_4"` — schema-shape placeholders, not real Suricata output. Excluded per the no-fabrication rule even though the repo licence is fine |
| `splunk/attack_data` | Apache-2.0 (root `LICENSE`) | searched (`gh search code "event_type" --repo splunk/attack_data`) — no Suricata eve.json datasets found in the repo |
| `elastic/beats` `filebeat/module/*` | n/a | no `suricata` module in current `filebeat/module` tree (moved to elastic/integrations, see above) |
