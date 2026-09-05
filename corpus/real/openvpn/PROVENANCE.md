# PROVENANCE — corpus/real/openvpn (label: web-squid-suricata-openvpn)

Both files below are genuine OpenVPN server-process log messages, but **not** in the
`parsers/openvpn.toml` wire format. That parser matches only the raw `--log` /
`--log-append` file form (a bare ctime stamp: `Sun May 11 08:42:15 2026 ...`).
Every real OpenVPN capture found in this search — including both files here — was
produced by an OpenVPN daemon logging through the system logger (`--syslog`, or the
distro default), which hands the identical message body (`ip:port ...`,
`cn/ip:port ...`) to syslogd, which then prepends an RFC 3164 header
(`Mon DD HH:MM:SS host tag[pid]: `). This is arguably the more common real-world
wire form for a "syslog collector" per the task brief, but it is a header variant the
current parser's match regex does not accept (see Validation below). Kept as-is,
byte for byte — not rewritten to fit the parser.

| file | source URL | revision | path in source | licence | anonymised | kind | lines | how fetched |
|---|---|---|---|---|---|---|---|---|
| `azuresentinel_openvpn_syslog.log` | https://github.com/Azure/Azure-Sentinel/blob/main/Sample%20Data/Syslog/OpenVPN.txt | commit `main` HEAD at fetch time 2026-09-05 | `Sample Data/Syslog/OpenVPN.txt` | MIT (root `LICENSE`, Microsoft Corporation) | client IP replaced with `1.2.3.4`; hostname replaced with `demo-host.local`; VPN/CN name replaced with `demovpn`; everything else (message vocabulary, cipher negotiation strings, `IV_*` peer-info keys, MTU inconsistency warning, push-reply option list) is real OpenVPN 2.x message text as the Sentinel OpenVPN solution's sample data | sanitized-real | 32 | `gh api "repos/Azure/Azure-Sentinel/contents/Sample Data/Syslog/OpenVPN.txt" --jq '.content'` then `base64 -D`, verbatim |
| `dfir_gemini_openvpn_syslog.log` | https://github.com/AndrewRathbun/DFIRArtifactMuseum/blob/main/Linux/GeminiIntrusion/var/log/openvpn/openvpn.log | commit `main` HEAD at fetch time 2026-09-05 | `Linux/GeminiIntrusion/var/log/openvpn/openvpn.log` | MIT (root `LICENSE`) | none apparent — a DFIR (digital forensics) training-artifact repository's real captured `/var/log/openvpn/openvpn.log` from a simulated-intrusion Linux host ("GeminiIntrusion" case), including a real-looking public source IP (185.11.22.33) and a contractor username (`jsmith_contractor`) as observed on the artifact disk image | real-capture | 5 | `gh api "repos/AndrewRathbun/DFIRArtifactMuseum/contents/Linux/GeminiIntrusion/var/log/openvpn/openvpn.log" --jq '.content'` then `base64 -D`, verbatim |

## Licences checked and rejected / not obtained

| candidate URL | licence found | verdict |
|---|---|---|
| `elastic/integrations` `packages/openvpn` | n/a | package does not exist in `elastic/integrations` (`gh api repos/elastic/integrations/contents/packages/openvpn` → 404) |
| `simonheiniger/consultec` `OpenVPN/log/client.log`, `mokshraj/Mine_server` `openvpn.log`, `boredom1234/vpn-docker` `logs/openvpn.log`, `Mood5-SUT/VPN-Orchestrator` `output/openvpn/*.log`, `or0or1/docker-openvpn-rfw` `data/openvpn/access.log`, `diiageantonin/P5` `OpenVpn/openvpn.log` | none detected (`gh api repos/<owner>/<repo> --jq '.license'` returned null/empty for every one) | rejected — no licence, do not copy per instructions |
| `jupyterj0nes/masstin` `rules/vpn/samples/openvpn.sample.log` | AGPL-3.0 | rejected |
| DataDog/integrations-core `openvpn/` | BSD-3-Clause | checked, contains only a metrics-collection check (SNMP/status-file based), no log fixtures |
| `dinni3/cyberstudy` `tryhackme/artifacts/openvpn.log` | none detected | rejected — no licence |

## Validation finding (see also the structured `validation` field of this task's return)

Both files: **0/37 lines parsed** — every line comes back `ulpf.parse_status:
"no_parser"` and every line also fires `time_from_receipt` (the RFC 3164
`Mon DD HH:MM:SS host tag[pid]:` header is not the ctime format
`parsers/openvpn.toml`'s `[match].regex` expects at the start of the line, so no
parser claims it and the device's own timestamp is never extracted — the pipeline
falls back to receipt time). This is a real header-variant gap, not a corpus defect:
production OpenVPN deployments overwhelmingly ship logs via syslog rather than a bare
`--log` file, and this parser currently only covers the latter (the parser's own
top-of-file comment already anticipates this: "Detection is the ctime prefix alone
... this definition sits below every syslog-framed one" — but no syslog-framed
OpenVPN parser variant currently exists in `parsers/`).
