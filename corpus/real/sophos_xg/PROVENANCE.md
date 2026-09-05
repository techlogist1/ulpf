# Provenance — corpus/real/sophos_xg/sophos_xg.log

1 line. Thin — see "Search effort" below for why bulk Sophos XG real captures under a permissive licence
could not be found in the time box, and what was checked and rejected.

| line | source | source URL | revision | path in source | licence | what the source anonymised | kind | fetched via |
|---|---|---|---|---|---|---|---|---|
| 1 | Azure Sentinel sample data | https://github.com/Azure/Azure-Sentinel | commit `a77ea8c2ec2b74820e98c3e34ee7ec0371207d91` (master, 2026-09-05) | `Sample Data/Custom/SophosXGFirewall.json`, the one record (of 1000) whose `SyslogMessage` field is a complete, unmangled line | MIT License — repo licence confirmed via `gh api repos/Azure/Azure-Sentinel --jq '.license.spdx_id'` → `MIT` | `dst_ip` is scrubbed to the placeholder `10.10.10.10`; `src_ip` (`10.1.1.2`) is untouched (private range, real). `TenantId`/`_ResourceId` are Microsoft's own demo-tenant/subscription GUIDs, not customer data. `src_mac` is genuinely corrupted in the device's own output (`00: 0:00: 0:00: 0` — not six hex pairs) — kept byte-exact per the no-fixing rule rather than repaired. Real 2019-03-06 firewall-rule "Allow" event from device `XG330`. | sanitized-real | File is 3.2 MB — too large for the GitHub Contents API's base64 field (which silently returns `encoding: "none"` and no content above ~1 MB). Fetched via `gh api repos/Azure/Azure-Sentinel/git/blobs/<blob-sha>` (git Blobs API, no size ceiling in this range) `--jq '.content' \| base64 -d`, then `json.load` and filtered for the one record whose `SyslogMessage` starts with `device="SFW"` (999 of the 1000 records have their `date=`/`time=` prefix consumed by Azure's own ingestion-time syslog-tag parsing and are missing those bytes entirely — see below). |

No `<PRI>` header was prepended: `samples/README.md` documents that Sophos XG lines are legitimately observed
with no syslog envelope at all (`the wire form <30> device="SFW" ... with no syslog header`), and the parser's
`[match] contains = ["device=\"SFW\""]` doesn't require one, so committing the bare `device="SFW" date=... `
line exactly as fetched — with no invented priority number — is the byte-exact, non-fabricating choice.

## Why only 1 line: the Azure dataset is mostly unusable without fabricating bytes
The same `SophosXGFirewall.json` file has 1000 records and 722 *distinct* `SyslogMessage` values — genuinely
varied real traffic, not a replay — but **999 of the 1000** have their `SyslogMessage` field starting mid-string
(e.g. `"51:10 timezone=\"BST\" device_name=..."`), because Azure's own Log Analytics ingestion mis-parsed the
BSD syslog TAG field on this vendor and swallowed `device="SFW" date=<D> time=<HH` into a `ProcessName` field
that itself only preserved the literal string `device="SFW"` — the date and the first two digits of the hour
are **not recoverable from any field in the record** (`"Date": ""`, `"Time": ""` on every one of these 999).
Reconstructing a "complete" line for any of them would mean inventing the missing date and hour — refused
per the no-fabrication rule. Only the 1 record above happened to have its `SyslogMessage` field intact.

## Search effort — checked and rejected (licence) or not found
- **elastic/integrations** `packages/sophos/_dev/deploy/docker/sample_logs/sophos-xg.log` and
  `packages/sophos/data_stream/xg/_dev/test/pipeline/test-*.log` — repo-default licence is
  **Elastic License 2.0** (`LICENSE.txt` at repo root; no package-level override found for `sophos`).
  Not copied — recorded here so the lead can pull it live at the demo if wanted.
- **elastic/beats** `x-pack/filebeat/module/sophos/xg/test/firewall.log` — `x-pack/` tree, **Elastic
  License** by definition per the task brief. Not copied.
- **CrowdStrike/logscale-community-content** — checked `Log-Sources/` listing; no Sophos parser exists
  in this repo (only Dell/SonicWall among perimeter-device firewalls).
- **DataDog/integrations-core** — has a `sophos_central_cloud` integration (Sophos Central cloud-console
  API events), but no `sophos_firewall`/XG syslog integration at all, so nothing in the expected wire
  format exists there.
- **splunk/splunk-connect-for-syslog** `tests/test_sophos_firewall_xg.py` (Apache-2.0) — has two fully
  concrete Sophos XG event bodies (`log_type="Firewall"`/`"Content Filtering"`, complete field lists), but
  both are Jinja2 templates with `{{ mark }}` (priority) **and** `{{ host }}` substituted into the
  `user_name` field's value at test-run time; `host` is a random test-run key with no literal value in the
  source. Filling it in would mean inventing a username — skipped per the no-fabrication rule (see the
  parallel sonicwall case in that vendor's PROVENANCE.md for the same reasoning).
- **community.sophos.com** forum thread "Sample of syslog messages for Sophos Firewall" — real
  admin-posted examples exist per the search-result snippet, but the page did not render through the
  available fetch tool (JS-rendered forum; returned empty content) inside the time box. Recorded as a
  lead for the lead to pull manually/live if wanted.
- **splunk/splunk-connect-for-syslog** issue #1665 (GitHub issue comment) — a community member posted a
  syslog-ng *parser config* referencing Sophos XG field names, not a raw captured line. No usable sample.

## Validation (baseline binary, `--infer-threshold 0`)
```
stages: framed 1  stored 1  detected 1  no_parser 0  parsed 1  parse_failed 0  normalized 1  emitted 1
signals: sub_matched 0  sub_no_match 0  sub_uncovered 0  time_from_receipt 0  class_unknown 0  unmapped_fields 20
```
The one line parses and normalizes cleanly end to end (`action: Allowed`, `src_endpoint.ip: 10.1.1.2`,
`dst_endpoint.ip: 10.10.10.10`, `firewall_rule.uid: 94`, timestamp correctly resolved from the split
`date`+`time`+`timezone` fields to `2019-03-07T04:04:00Z`). The corrupted `src_mac` value from the source
(`00: 0:00: 0:00: 0`) comes through verbatim as `"mac": "00:"` in the output — the kv parser stopped at the
first unquoted space, which is the correct, honest behaviour given genuinely malformed input; not a ulpf bug.
`unmapped_fields` (20) are the app-control/heartbeat/zone-type detail fields (`ips_policy_id`,
`appfilter_policy_id`, `hb_health`, `srczonetype`) the OCSF mapping doesn't carry in this subset.
