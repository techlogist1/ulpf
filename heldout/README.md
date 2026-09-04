# heldout/ — inference-engine held-out test logs

No ULPF parser covers any format here (checked against `parsers/*.toml`). Sep 4 2026,
one day, timestamps increasing per file (messy.log is generated increasing then
shuffled deterministically, `random.seed(99)`). Generated with a throwaway Python
script, deleted after writing these files.

## mikrotik.log (250 lines, 14 types)
- fw_input (30): `input: in:ether1 out:(none), src-mac 11:2a:..., proto TCP (SYN), ip:port->ip:port, len 60`
- fw_input_nolen (20): same, `len` field dropped (optional per RouterOS config)
- fw_forward (24): `forward: in:X out:Y, ...` no NAT block
- fw_forward_nat (24): forward + `NAT (src:port->natip:natport)->dst:port`
- fw_icmp (18): `proto ICMP (type=8, code=0), ip->ip, len 56` (no ports)
- dhcp_assigned (23) / dhcp_deassigned (22): `dhcp,info dhcp1 assigned 10.0.x.x to mac`
- login_success (19): `system,info,account user alice logged in from ip via winbox`
- logout (13): same shape, `logged out`
- login_failure (10) / login_failure_emptyuser (9): `system,error,critical login failure for user  from ip via ssh` (double space = empty user)
- wireless_connect (11): `wireless,info mac@wlan1: connected`
- wireless_disconnect (14): `... disconnected, extensive data loss` (5 free-text reasons)
- script_run (13): `script,info script check-uptime started by scheduler`

## edgerouter.log (250 lines, 10 types)
- kernel_tcp (78): netfilter LOG, `IN=eth0 OUT=... MAC=... SRC=... DST=... LEN=... TOS=0x00 PREC=0x00 TTL=... ID=... [DF] PROTO=TCP SPT=... DPT=... WINDOW=... RES=0x00 <flags> URGP=0`
- kernel_udp (39): same header, UDP has no WINDOW/RES/flags, trailing `LEN=` repeated (payload length)
- kernel_icmp (37): `PROTO=ICMP TYPE=8 CODE=0`
- kernel_ipv6_tcp (26): ip6tables — `TC=0 HOPLIMIT=... FLOWLBL=0` replace TOS/TTL/ID
- nat_masquerade (16): `[WAN_LOCAL-1000-A]IN= OUT=eth0 SRC=... DST=... ... DF PROTO=TCP SPT=... DPT=...`
- dhcpd_discover (19) / dhcpd_request (9) / dhcpd_ack (12): isc-dhcpd, `DHCPDISCOVER from mac via eth1`
- sshd_accepted (6) / sshd_failed (8): `sshd[1234]: Accepted publickey for admin from ip port 51234 ssh2` / `Failed password ...`

## nginx_access.log (250 lines, 3 types)
- access_line (227): exact combined format, varied method/status/path/referer/UA, IPv6 clients mixed in
- access_dash_request (13): request field literally `"-"` (broken/empty request line, status 400)
- access_malformed (10): garbage inside the quotes — control bytes, `GET HTTP/0.9`, literal `\x16\x03\x01`

## messy.log (300 lines) — type labels reuse the tables above, plus:
- cron (21): `<78>Sep  4 ... gw CRON[1234]: (root) CMD (run-parts /etc/cron.hourly)`
- systemd (9): `<30>Sep  4 ... gw systemd[1]: Started Daily apt download activities.`
- truncated (9): any line above, cut at a random byte
- empty (3): zero-length lines
- MikroTik share is 258/300 (86%), a bit above "roughly 80%" — traded off against hitting the fixed truncated/cron/systemd/empty counts exactly.
- 2 lines carry raw `0xff 0xfe` inside a script_run/wireless_disconnect body; 1 line has a Latin-1 `é` (0xE9) byte in the hostname (`café-gw`); 5 lines have a doubled space or a tab substituted for one space. All keep their original type label — corruption doesn't change message type.

## Sources consulted
- MikroTik: [Log](https://help.mikrotik.com/docs/spaces/ROS/pages/328094/Log) (topics list, BSD-syslog note), [Wireless Troubleshooting](https://help.mikrotik.com/docs/spaces/ROS/pages/122388523/Wireless+Troubleshooting) (disconnect reasons), [fail2ban #3458](https://github.com/fail2ban/fail2ban/issues/3458) (login-failure line incl. empty-user/IPv6 capture), MikroTik forum threads on `dhcp,info assigned/deassigned` syntax.
- EdgeRouter: [fail2ban #2865](https://github.com/fail2ban/fail2ban/issues/2865) (six real captured kernel LOG lines — field order/DF-optional confirmed verbatim from these).
- netfilter: general LOG-target field reference (IN/OUT/MAC/SRC/DST/LEN/TOS/PREC/TTL/ID/DF/PROTO/SPT/DPT/WINDOW/RES/URGP), cross-checked against the fail2ban capture above.
- nginx: [ngx_http_log_module](https://nginx.org/en/docs/http/ngx_http_log_module.html) (`combined` format string, verbatim).

## Could not independently verify this session (patterned on the prior prototype / general knowledge, not re-fetched)
- MikroTik NAT-block display inside `forward:` lines, and the `proto ICMP (type=, code=)` display string.
- `logout` carrying `from ip via method` like `login`, and the `script,info` scheduler line wording.
- ip6tables `TC=/HOPLIMIT=/FLOWLBL=` field names (standard netfilter ipv6 LOG output, not fetched fresh this session).
- EdgeRouter NAT-masquerade rule-name bracket text and sshd line format (standard OpenSSH, not vendor-specific).
