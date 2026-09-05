# Provenance — corpus/real/sonicwall/sonicwall.log

15 lines total. Every value below is either a single-line Rust/YAML string literal extracted with a real
YAML parser (`yaml.safe_load`, to correctly un-fold YAML block-scalar line wrapping — a mechanical parse,
not a rewrite) or a Rust string-literal regex-extracted from the `.rs` source, from files fetched byte-exact
via `gh api .../git/blobs/<sha>` or `contents/<path>`.

| line(s) | source | source URL | revision | path in source | licence | what the source anonymised / how it was authored | kind | fetched via |
|---|---|---|---|---|---|---|---|---|
| 1-2 | CrowdStrike LogScale community parser tests | https://github.com/CrowdStrike/logscale-community-content | commit `970f85669d8ec0fe07e65f13f7120fbbc26a1bb5` (main, 2026-09-05) | `Log-Sources/Sonicwall/src/parsers/sonicwall.yaml`, the `tests:` list | Unlicense (public domain) — read at https://github.com/CrowdStrike/logscale-community-content/blob/main/LICENSE | Nothing — real WAN IP `85.118.26.198`, real internal `10.2.x.x` addressing, 2024-dated (`2024-04-18`, `2024-02-27`), from CrowdStrike's own SonicOS parser test corpus. YAML plain-scalar line folding un-wrapped with `yaml.safe_load` (folded newlines become single spaces per YAML spec — a parse, not a content change). | real-capture | `gh api repos/CrowdStrike/logscale-community-content/contents/Log-Sources/Sonicwall/src/parsers/sonicwall.yaml --jq '.content' \| base64 -d`, then `yaml.safe_load` |
| 3 | aicers/reproduce test | https://github.com/aicers/reproduce | commit `d6c25887ea38f318bac6108d11a1b251779a9ad1` (main, 2026-09-05) | `src/parser/security_log/sonic_wall.rs`, line 125 (`let log = r#"..."#;`) | Apache License 2.0 — read at https://github.com/aicers/reproduce/blob/main/LICENSE | Nothing — a real 2020-03-16 port-scan alert (`Possible port scan detected`), real public source/dest IPs (`139.199.19.227` → `220.83.254.2`), real MACs, used as a Rust unit-test fixture. | real-capture | `gh api repos/aicers/reproduce/contents/src/parser/security_log/sonic_wall.rs --jq '.content' \| base64 -d`, regex-extracted the `r#"..."#` literal |
| 4 | u-siem/usiem-sonicwall benchmark | https://github.com/u-siem/usiem-sonicwall | commit `fd3460f5063dd6c99745719d666743f73de4b1bc` (main, 2026-09-05) | `src/bin/benchmark.rs` | MIT License — read at https://github.com/u-siem/usiem-sonicwall/blob/main/LICENSE | **Not a real capture** — this is the crate author's own hand-written representative line (obviously placeholder values: hostname `FWSonicWall`, serial `SERIALNUMBER111`, user `test@usiem.com`) used purely to benchmark parser throughput. Kept for its correctly-formed `X6-V80` VLAN-tagged interface suffix and `sslvpnc` session type, both of which are new shapes versus the existing `samples/sonicwall.log`. Labelled honestly below. | **generated** | `gh api repos/u-siem/usiem-sonicwall/contents/src/bin/benchmark.rs --jq '.content' \| base64 -d`, regex-extracted the `let log = "...";` literal |
| 5-8, 11 | Datadog sonicwall_firewall pipeline tests (recent/business events) | https://github.com/DataDog/integrations-core | commit `c545ca5b168e1ccbffba82121cda46fa09459a46` (master, 2026-09-05) | `sonicwall_firewall/assets/logs/sonicwall-firewall_tests.yaml`, `tests[].sample` (5 of 11 entries used here) | BSD-3-Clause — repo licence confirmed via `gh api repos/DataDog/integrations-core --jq '.license.spdx_id'` → `BSD-3-Clause` | **sanitized-real**: every `src=`/`dst=`/`fw=` IP is scrubbed to the placeholder `10.10.10.10` throughout Datadog's fixture, but `srcMac`/`dstMac`, session/audit metadata (`uuid=`, `auditId=`, `grpName=`), user agents, category names and message text are untouched — consistent with a real captured-then-anonymised corpus, not hand-invented text. 2022-2024 dated events (admin audit, web-proxy hit, IPS/Log4Shell alert, GUI logout). | sanitized-real | same method, `yaml.safe_load` on the file, took `tests[i]["sample"]` |
| 9-10, 12 | Datadog sonicwall_firewall tests, OSSEC-lineage lines | https://github.com/DataDog/integrations-core (same commit/path as above) | same | BSD-3-Clause (same) | **sanitized-real**, but note: these 3 entries (`Jan 3 13:45:5x ... c=262144 m=98`, `Jan 3 13:45:43 ... m=38`, `Jan 3 13:45:39 ... m=537`) reproduce, field-for-field (same `sn=000SERIAL`, same `n=`/`m=`/`c=` values), the classic 2007 SonicWall reference capture that also circulates via the OSSEC documentation project — Datadog re-anonymised the IPs to `10.10.10.10` and re-committed it under their own BSD-3-Clause licence, which is what makes it usable here (the OSSEC copy itself has no LICENSE file — see rejected list). | sanitized-real | same method |
| 13 | Datadog sonicwall_firewall tests, extra-fields torture case | https://github.com/DataDog/integrations-core (same commit/path) | same | BSD-3-Clause (same) | Same OSSEC-lineage line as line 12, but with a long tail of extra `key=0`/`key="NA"` pairs (`af_policy=`, `bcastRx=`, `bid=`, `vpnpolicyDst=`, etc.) that are **not documented SonicOS fields** — this looks like Datadog's own test author appended synthetic junk fields to fuzz-test their grok pattern's tolerance for unexpected trailing k=v pairs, not a real device emission. Flagged so the lead doesn't mistake `af_policy`/`bcastRx` for real SonicOS vocabulary. | **generated** (real line + synthetic tail) | same method |

## Rejected / checked but not copied
- `elastic/integrations packages/sonicwall_firewall/...` and `x-pack/filebeat/module/sonicwall/firewall/test/general.log` (elastic/beats) — repo-default / x-pack licence is **Elastic License 2.0**. Confirmed no package-level `LICENSE.txt` override for `sonicwall_firewall`. Not copied.
- `adriansr/nwdevice2filebeat samples/sonicwall/general.log` — root `LICENSE.txt` is the **Elastic License Agreement** verbatim. Not copied.
- `ossec/ossec-docs docs/log_samples/firewalls/sonicwall.rst` (and its mirrors `ossec/ossec-hids`, `ossec/ossec-rules`, `ossec/ossec.github.io`, `dcid/rootcheck`) — **no LICENSE file anywhere in the repo** (`gh api repos/ossec/ossec-docs --jq '.license'` → `null`; checked root listing, no `LICENSE*`). Per the rule "no licence: do not copy," rejected even though the content (real 2004-2007 SonicWall captures) is excellent and is the ultimate source lineage for the Datadog lines above.
- `santiago-bassett/Alienvault-Demo_scripts sonicwall/sonicwall.log` (+ `.log.orig`) — real 2003-2004 AlienVault/OSSIM demo captures with real public IPs, but **no LICENSE** (`license: null`). Not copied.
- `CrowdStrike/logscale-community-content` — has no dedicated Sophos or pfSense parser directory (checked; only `Log-Sources/Sonicwall` exists among our three vendors).
- SC4S's own `tests/test_dell_sonicwall.py` templated case — skipped entirely: the only concrete value in the template is the priority mark (`<166>`, itself literal in the file), but `{{ host }}` and `{{ delldt }}` are filled by the test framework at run time and are not present as literal bytes anywhere in the source, so reconstructing a "complete" line would mean inventing a timestamp and username — refused per the no-fabrication rule.
- `u-siem/usiem-sonicwall src/parsers/*.rs` (the actual decoder code, not the benchmark) — no additional log-line literals found beyond the one used above.

## Validation (baseline binary, `--infer-threshold 0`)
```
stages: framed 15  stored 15  detected 15  no_parser 0  parsed 15  parse_failed 0  normalized 15  emitted 15
signals: sub_matched 11  sub_no_match 4  sub_uncovered 0  time_from_receipt 0  class_unknown 0  unmapped_fields 209
```
100% of lines are detected and reach `parsed`/`normalized` output (the `sonicwall.toml` top-level kv strategy
never fails on any of these 15 lines). But 4 of the 15 fail their **src/dst sub-pattern** (`sub_no_match`),
all on the `src`/`dst` field's inner `ip:port:interface[:host]` grammar — three genuine shapes the parser's
three patterns (`ip:port:if:host`, `ip:port:if`, bare `ip`) don't cover, all present in real SonicOS output:

1. **Line 5** (`src=10.10.10.10:51692`) — a bare **`ip:port`, no interface segment**, on an admin
   configuration-audit event (`m=1382`). None of the three patterns has a 2-segment form.
2. **Line 9** (`src=:28503:WAN:SOURCEHOST`, and its `dst=::LAN:DSTHOST`) — the **IP segment is empty** but
   port/interface/hostname are present. The `{src_ip:ip}` typed slot's regex requires a real IP token, so
   an empty leading segment fails the whole anchored pattern; there's no fallback for "IP omitted, rest
   present" (this is the IPv4-record-with-only-an-IPv6-peer case: `srcV6=` carries the real address instead).
3. **Line 10** (`src=10.10.10.10::X0`) — IP and interface present but the **port segment is empty**
   (a GUI/administration session-end event with no port). The `{src_port:int}` typed slot rejects the
   empty string, so both 3- and 4-field patterns fail; only the bare-IP fallback would match, but that
   fallback is anchored to *just* an IP with no trailing colons, so it doesn't match either.
4. **Line 12** (`dst=:8080:X20-V68`) — same empty-leading-IP-with-populated-port/interface shape as #2,
   this time on `dst` (its `src` on the same line, `10.10.10.10:54192:X20-V60`, matches fine).

All 4 still parse and normalize (top-level kv fields are unaffected), they just lose the structured
`src_ip`/`src_port`/`src_interface` breakdown for that one endpoint. `unmapped_fields` count (209) is
otherwise the app-layer/audit detail fields (`gcat`, `auditId`, `grpName`, `dpi`, `spkt`/`rpkt`, IPS
signature ids) that `mappings/ocsf.toml` doesn't carry in this pragmatic subset — expected, not a bug.
