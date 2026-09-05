# Coverage

Every sample and every corpus file through the built binary, one fresh store each.
Regenerate with `scripts/coverage.sh > docs/coverage.md`.

- binary: `./target/release/ulpf` at commit `0c197bc`
- generated: 2026-09-05 23:04 UTC
- per file: `ulpf run <file> --store <fresh> --output <scratch> --infer-threshold 0 --report-json <scratch>`; every number below is a field of that JSON report.
- `lines` is the file's own line count; `framed` is what the engine made of it, so the two differ where a collector folded one event over two lines.
- `PROVENANCE.md` and `setup/` are documentation and are not run.

The Zeek rows are the honest uncovered set: sixteen files, 23,434 lines, no parser claims one of them. Lane 3's CEF, LEEF and CloudTrail definitions have landed and did not move them; Zeek stays one of the unseen formats the live inference demo runs against (`corpus/README.md`) until a Zeek definition exists.

## samples/

| file | lines | framed | detected | parsed | parse_failed | sub_uncovered | sub_no_match | time_from_receipt | class_unknown | unmapped_fields |
|---|---|---|---|---|---|---|---|---|---|---|
| `samples/cef.log` | 14 | 14 | 14 | 14 | none | 0 | 0 | 1 | 1 | 60 |
| `samples/check_point.log` | 13 | 13 | 13 | 13 | none | 0 | 0 | 0 | 0 | 241 |
| `samples/cisco_asa.log` | 30 | 30 | 30 | 30 | none | 1 | 1 | 1 | 6 | 160 |
| `samples/cisco_ios.log` | 32 | 32 | 32 | 32 | none | 1 | 1 | 2 | 14 | 204 |
| `samples/cloudtrail.log` | 15 | 15 | 15 | 15 | none | 0 | 0 | 0 | 0 | 256 |
| `samples/fortinet_fortigate.log` | 12 | 11 | 11 | 11 | none | 0 | 0 | 0 | 2 | 148 |
| `samples/juniper_srx.log` | 16 | 16 | 16 | 16 | none | 1 | 0 | 0 | 1 | 210 |
| `samples/leef.log` | 16 | 16 | 16 | 16 | none | 0 | 0 | 1 | 0 | 73 |
| `samples/openvpn.log` | 44 | 44 | 44 | 44 | none | 0 | 4 | 0 | 30 | 112 |
| `samples/palo_alto_panos.log` | 22 | 22 | 22 | 22 | none | 1 | 0 | 0 | 2 | 563 |
| `samples/pfsense_filterlog.log` | 18 | 18 | 18 | 18 | none | 0 | 0 | 0 | 0 | 318 |
| `samples/sonicwall.log` | 19 | 19 | 19 | 19 | none | 0 | 0 | 0 | 1 | 208 |
| `samples/sophos_xg.log` | 15 | 15 | 15 | 15 | none | 0 | 0 | 0 | 1 | 271 |
| `samples/squid_access.log` | 33 | 33 | 31 | 30 | pattern_no_match 1 | 0 | 1 | 3 | 3 | 150 |
| `samples/suricata_eve.log` | 11 | 11 | 11 | 10 | invalid_json 1 | 0 | 0 | 2 | 1 | 51 |

## corpus/real/

| file | lines | framed | detected | parsed | parse_failed | sub_uncovered | sub_no_match | time_from_receipt | class_unknown | unmapped_fields |
|---|---|---|---|---|---|---|---|---|---|---|
| `corpus/real/cisco_asa/cisco_asa_arcane_door.log` | 287 | 287 | 287 | 287 | none | 0 | 0 | 0 | 36 | 1161 |
| `corpus/real/cisco_asa/cisco_asa_generic_logs.log` | 48 | 48 | 48 | 48 | none | 0 | 0 | 0 | 18 | 145 |
| `corpus/real/cisco_ios/cisco_ios_siemlite_generated.log` | 4 | 4 | 4 | 4 | none | 0 | 0 | 0 | 2 | 26 |
| `corpus/real/cisco_ios/cisco_ios_sisf_t1557.log` | 5 | 5 | 5 | 5 | none | 0 | 0 | 0 | 5 | 34 |
| `corpus/real/fortinet_fortigate/fortigate_forum_quotes.log` | 4 | 4 | 4 | 4 | none | 0 | 0 | 0 | 1 | 40 |
| `corpus/real/fortinet_fortigate/fortigate_sample_sva_s1.log` | 1 | 1 | 1 | 1 | none | 0 | 0 | 0 | 0 | 13 |
| `corpus/real/juniper_srx/juniper_srx.log` | 284 | 284 | 282 | 282 | none | 282 | 0 | 2 | 284 | 1128 |
| `corpus/real/openvpn/azuresentinel_openvpn_syslog.log` | 32 | 32 | 32 | 32 | none | 0 | 4 | 0 | 26 | 98 |
| `corpus/real/openvpn/dfir_gemini_openvpn_syslog.log` | 6 | 6 | 6 | 6 | none | 0 | 4 | 0 | 4 | 15 |
| `corpus/real/palo_alto_panos/palo_alto_panos.log` | 50 | 50 | 50 | 50 | none | 0 | 0 | 0 | 0 | 1942 |
| `corpus/real/pfsense_filterlog/pfsense_filterlog.log` | 30 | 30 | 30 | 30 | none | 0 | 0 | 0 | 0 | 535 |
| `corpus/real/sonicwall/sonicwall.log` | 15 | 15 | 15 | 15 | none | 0 | 0 | 0 | 0 | 211 |
| `corpus/real/sophos_xg/sophos_xg.log` | 1 | 1 | 1 | 1 | none | 0 | 0 | 0 | 0 | 20 |
| `corpus/real/squid_access/azuresentinel_asim_websession.log` | 12 | 12 | 12 | 12 | none | 0 | 0 | 0 | 0 | 60 |
| `corpus/real/squid_access/packt_sc300_generated.log` | 360 | 360 | 360 | 360 | none | 0 | 0 | 0 | 0 | 1800 |
| `corpus/real/squid_access/usiem_squid_docker_native.log` | 7 | 7 | 7 | 7 | none | 0 | 0 | 0 | 0 | 35 |
| `corpus/real/suricata_eve/ccdcoe_cdmcs_eve.json` | 314 | 314 | 314 | 314 | none | 0 | 0 | 0 | 0 | 3880 |

## corpus/generated/

| file | lines | framed | detected | parsed | parse_failed | sub_uncovered | sub_no_match | time_from_receipt | class_unknown | unmapped_fields |
|---|---|---|---|---|---|---|---|---|---|---|
| `corpus/generated/haproxy/haproxy.log` | 2356 | 1178 | 0 | 0 | none | 0 | 0 | 1178 | 1178 | 0 |
| `corpus/generated/nginx/nginx1-access.log` | 1548 | 1548 | 0 | 0 | none | 0 | 0 | 1548 | 1548 | 0 |
| `corpus/generated/nginx/nginx1-error.log` | 126 | 126 | 0 | 0 | none | 0 | 0 | 126 | 126 | 0 |
| `corpus/generated/nginx/nginx2-access.log` | 1407 | 1407 | 0 | 0 | none | 0 | 0 | 1407 | 1407 | 0 |
| `corpus/generated/nginx/nginx2-error.log` | 189 | 189 | 0 | 0 | none | 0 | 0 | 189 | 189 | 0 |
| `corpus/generated/openvpn/client.log` | 7329 | 7329 | 7329 | 7329 | none | 0 | 5389 | 0 | 7215 | 11138 |
| `corpus/generated/openvpn/server-2.4-ctime.log` | 674 | 674 | 674 | 674 | none | 0 | 61 | 0 | 559 | 1388 |
| `corpus/generated/openvpn/server-2.5.log` | 565 | 565 | 565 | 565 | none | 0 | 108 | 0 | 473 | 1114 |
| `corpus/generated/openvpn/server-syslog.log` | 1044 | 1044 | 1043 | 1043 | none | 0 | 218 | 1 | 874 | 3071 |
| `corpus/generated/openvpn/server.log` | 3937 | 3937 | 3937 | 3937 | none | 0 | 791 | 0 | 3319 | 7712 |
| `corpus/generated/squid/access.log` | 16500 | 16500 | 16500 | 16500 | none | 0 | 0 | 0 | 0 | 82500 |
| `corpus/generated/squid/cache.log` | 20000 | 19774 | 0 | 0 | none | 0 | 0 | 19774 | 19774 | 0 |
| `corpus/generated/suricata/eve.json` | 2924 | 2924 | 2924 | 2924 | none | 0 | 0 | 0 | 0 | 40741 |
| `corpus/generated/zeek/conn.log` | 5129 | 5129 | 0 | 0 | none | 0 | 0 | 5129 | 5129 | 0 |
| `corpus/generated/zeek/dns.log` | 3409 | 3409 | 0 | 0 | none | 0 | 0 | 3409 | 3409 | 0 |
| `corpus/generated/zeek/files.log` | 1447 | 1447 | 0 | 0 | none | 0 | 0 | 1447 | 1447 | 0 |
| `corpus/generated/zeek/http.log` | 1545 | 1545 | 0 | 0 | none | 0 | 0 | 1545 | 1545 | 0 |
| `corpus/generated/zeek/json/conn.log` | 5120 | 5120 | 0 | 0 | none | 0 | 0 | 5120 | 5120 | 0 |
| `corpus/generated/zeek/json/dns.log` | 3400 | 3400 | 0 | 0 | none | 0 | 0 | 3400 | 3400 | 0 |
| `corpus/generated/zeek/json/files.log` | 1438 | 1438 | 0 | 0 | none | 0 | 0 | 1438 | 1438 | 0 |
| `corpus/generated/zeek/json/http.log` | 1536 | 1536 | 0 | 0 | none | 0 | 0 | 1536 | 1536 | 0 |
| `corpus/generated/zeek/json/packet_filter.log` | 1 | 1 | 0 | 0 | none | 0 | 0 | 1 | 1 | 0 |
| `corpus/generated/zeek/json/ssl.log` | 160 | 160 | 0 | 0 | none | 0 | 0 | 160 | 160 | 0 |
| `corpus/generated/zeek/json/syslog.log` | 17 | 17 | 0 | 0 | none | 0 | 0 | 17 | 17 | 0 |
| `corpus/generated/zeek/json/weird.log` | 9 | 9 | 0 | 0 | none | 0 | 0 | 9 | 9 | 0 |
| `corpus/generated/zeek/packet_filter.log` | 10 | 10 | 0 | 0 | none | 0 | 0 | 10 | 10 | 0 |
| `corpus/generated/zeek/ssl.log` | 169 | 169 | 0 | 0 | none | 0 | 0 | 169 | 169 | 0 |
| `corpus/generated/zeek/syslog.log` | 26 | 26 | 0 | 0 | none | 0 | 0 | 26 | 26 | 0 |
| `corpus/generated/zeek/weird.log` | 18 | 18 | 0 | 0 | none | 0 | 0 | 18 | 18 | 0 |

