# PROVENANCE — corpus/real/cisco_ios (label: web-cisco-fortinet)

`parsers/cisco_ios.toml` expects `%FACILITY-SEVERITY-MNEMONIC: message` inside a syslog
envelope, with optional origin-id/sequence-number/timestamp-or-uptime prefixes. Real-format
volume for this exact device family was hard to obtain cleanly-licensed: the two big corpora
(elastic/integrations, Splunk attack_data) either don't have it or have almost none of it.
This directory is thin (9 lines total) and that is reported honestly rather than padded.

| file | source URL | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `cisco_ios_sisf_t1557.log` | https://github.com/splunk/attack_data/blob/master/datasets/attack_techniques/T1557.002/cisco_ios/cisco_ios.log | commit `main` HEAD at fetch time 2026-09-05 (LFS oid `sha256:94e4834c7da62847bd20a1672ebc110b14f9b4e2a91ab5689dd34233a757030b`) | `datasets/attack_techniques/T1557.002/cisco_ios/cisco_ios.log` | Apache-2.0 (root `LICENSE`, read via `gh api repos/splunk/attack_data/contents/LICENSE`) | the device's own IPv6/MAC values were already the RFC 3849/reserved documentation placeholders (`2001::DB8::1`, MAC `0000.0000.0000`) in the source file itself — not anonymised by us, and plausibly not by the original author either (could be a lab device that was never given real addresses); sibling `.yml`: "cisco ios logs", `environment: attack_range` | real-capture | 5 | LFS-batch-API fetch (pointer → `POST .../info/lfs/objects/batch` → signed `github-cloud.githubusercontent.com` URL → `curl`), same method as `cisco_asa` |
| `cisco_ios_siemlite_generated.log` | https://github.com/Arpitapaaul/SIEM-Lite/blob/main/samples/cisco_ios.log | commit `main` HEAD at fetch time 2026-09-05 | `samples/cisco_ios.log` | MIT (`gh api repos/Arpitapaaul/SIEM-Lite --jq '.license'`) | n/a — **flagged, not a real capture**: 2026-dated timestamps, generic hostnames (`RTR-CORE`, `SW-ACCESS`), and a repeated "attacker" IP (`45.83.122.7`) across an IPACCESSLOGP deny and a LOGIN_FAILED line are the signature of a hand-built demo fixture for the author's own SIEM tool, not device output. Included anyway, clearly labelled `generated`, because it is the only cleanly-licensed source found that exercises `IPACCESSLOGP`/`CONFIG_I`/`LINK-3-UPDOWN`/`SEC_LOGIN-4-LOGIN_FAILED` — message families the one real file above does not touch at all | generated | 4 | `gh api -H "Accept: application/vnd.github.raw" repos/Arpitapaaul/SIEM-Lite/contents/samples/cisco_ios.log`, verbatim |

## Licences checked and rejected / not obtained

| candidate URL | licence found | verdict |
|---|---|---|
| `elastic/beats` `filebeat/module/cisco*` (OSS tree) | n/a | no `cisco` module exists in OSS `filebeat/module` (see `cisco_asa/PROVENANCE.md` for the full checked list) |
| `elastic/beats` `x-pack/filebeat/module/cisco/ios/test/cisco-ios-syslog.log` | Elastic License (x-pack tree) | not copied |
| `elastic/integrations` `packages/cisco_ios/_dev/deploy/docker/sample_logs/cisco-ios.log`, `packages/cisco_ios/data_stream/log/_dev/test/pipeline/test-*.log` (`test-asr920.log`, `test-cisco-ios.log`, `test-syslog.log`) | Elastic License 2.0 (root `LICENSE.txt`; no override under `packages/cisco_ios`) | rejected — not copied |
| `cisco-ie/telemetry` `*/logs/*.log` (e.g. `4/logs/spine2.portflap.log`) | GitHub reports `NOASSERTION` (no SPDX-recognised licence identified) for the whole repo | rejected — unclear licence, not copied. Also wrong product: this is IOS-XR (`RP/0/RP0/CPU0:... %PKT_INFRA-LINK-3-UPDOWN :` — 4-part facility, space before the colon), not classic IOS/IOS-XE, so it would not have matched `parsers/cisco_ios.toml`'s `[match]` regex even if the licence had been clear |
| `hakkisagdic/loganalyzer` `catalog/parsers/cisco.asa/samples/*` (checked while looking for a Cisco fixture here) | MIT on the repo, but content strongly resembles copied Elastic `_dev/test/pipeline` fixture data (see `fortinet_fortigate/PROVENANCE.md` for the specific tell) | rejected — suspected licence-contamination, not copied |
| `pmuellrgitoff/integrations` (fork of `elastic/integrations`) | inherits Elastic License 2.0 | rejected |
| `patelhet2501/Network-Log-Analyzer` `samples/sample.log` | no licence (`gh api ... --jq '.license'` → `null`) | rejected — not copied |
| `community.cisco.com` — "%SEC-6-IPACCESSLOGP: list INTERNET-IN denied udp" thread (`t5/firewalls/.../td-p/2656396`) and "Access-list logs" thread (`t5/routing/.../td-p/1247384`) | n/a | both returned HTTP 403 to `WebFetch` (bot-protected); could not extract verbatim quoted lines. A human with a browser could likely retrieve real pasted `%SEC-6-IPACCESSLOGP`/`%SYS-5-CONFIG_I` lines from these threads at demo time |
