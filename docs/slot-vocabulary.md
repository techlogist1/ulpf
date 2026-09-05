# Slot vocabulary

The names `ulpf-infer` suggests for the slots it discovers, and where each convention
comes from. Compiled into `crates/ulpf-infer/src/cluster.rs` as `VOCAB` plus the shape
rules in `positional`; nothing here is read from disk at runtime, and there is no model
anywhere — every name is produced by a rule you can print, and every slot in a proposal
carries `reason`, the one line that says which rule fired (or why none did).

**Names are device-side vocabulary, never output-schema field names.** `ulpf-infer`
depends on `ulpf-parse` only (D38), so it cannot see `src_endpoint.ip`; the parser/mapping
wall would forbid the name even if it could. The payoff is indirect: every name below is
already an alias in `mappings/ocsf.toml [fields]` where one exists, so an approved
proposal normalizes without a mapping edit. Where a convention has two spellings the
aliased one wins (`src_ip`, not `saddr`). Names with no alias (`len`, `ttl`, `pid`,
`icmp_type`, `icmp_code`, `via`, `request`) are the device's own word and stay as they are;
adding them to a mapping is a mapping decision, not a parser one.

## Rule order

0. **Names the input carries** (the first table) — a JSON object's key. It is the device's
   vocabulary verbatim, so it wins over everything below, including the slot's type: Zeek's
   `ts` stays `ts`, and the generated `[[timestamp]]` spec follows whatever the timestamp
   slot is called.
1. **Shape rules** (the table's second half) — they read the line's structure, so they win
   over a key: `kernel: [{rule}]` must not be named after the syslog tag.
2. **The slot's own type** — a `timestamp` slot is `timestamp`.
3. **Key–value keys** — `SRC=`, `in:`, `src-mac ` give a name for free. The key is first
   looked up in the table below; an unknown key is used verbatim, sanitized to `[A-Za-z0-9_]`.
4. **The preceding constant word** — `script {word}` is `script`, `CMD {rest}` is `cmd`.
   Blocked for stopwords: English connectives (`for`, `by`, `of`, `on`, `at`, `with`, …),
   syslog severity words that sit in a topic list (`wireless,info {mac}`), and the protocol
   keywords `tcp`/`udp`/`icmp`.
5. **Otherwise** the slot stays `kind+n` (`ip1`, `word2`), `suggested = false`, and the
   reason says why: no key at all, a connective, or a syslog tag.

A name that repeats inside one template gets a `_2` suffix. The generated definition's slot
names are exactly these names.

## Names the input carries

| Context | Name | Reason string | Notes |
|---|---|---|---|
| `{"key":{value}` — a line that opens with `{`; the tokenizer keeps `"key":` as a constant word, so only the value is a slot | the key, sanitized to `[A-Za-z0-9_]` (`id.orig_h` → `id_orig_h`) | ``json key `id.orig_h` (written `id_orig_h`)`` | A nested object names by its innermost key and the reason says so: ``json key `orig_h` (innermost key of a nested object)`` (the path is not tracked; Zeek's keys are flat, `id.orig_h` is one key). An array's elements take the array's key: ``json key `answers` (first array element)``; a second element gets `_2`. Case is kept (`AA`, `TTLs`). Example: Zeek `json/conn.log`, `{"ts":1788598139.6,"uid":"CDs4H0…","id.orig_h":"192.168.148.3"}`. |

The rule does not fire when its trigger is absent: a line that does not open with `{`
tokenizes exactly as before, and nothing else in the pipeline changes. Because a JSON key
replaces the slot's type as the name, the generated definition's `[[timestamp]]` follows the
timestamp slot's actual name (`ts`), one candidate per distinct name.

## Keys

| Context pattern | Slot kind | Name | Source of the convention | Example line |
|---|---|---|---|---|
| `from {ip}` | ip | `src_ip` | BSD syslog, OpenSSH sshd | `Failed password for root from 198.51.100.7 port 50002 ssh2` |
| `SRC={ip}` | ip | `src_ip` | Linux netfilter `LOG` target (`xt_LOG`, iptables-extensions(8)) | `IN=eth0 OUT= SRC=26.24.119.87 DST=10.0.0.5 LEN=60` |
| `saddr={ip}` | ip | `src_ip` | netfilter conntrack / nftables | `saddr=203.0.113.9 daddr=10.0.0.1` |
| `source-address={ip}` | ip | `src_ip` | Juniper SRX `RT_FLOW` | `RT_FLOW_SESSION_CREATE: source-address="203.0.113.9"` |
| `to {ip}` | ip | `dst_ip` | BSD syslog, ISC dhcpd | `DHCPACK to 10.0.0.5 (aa:bb:cc:dd:ee:ff) via eth1` |
| `DST={ip}` | ip | `dst_ip` | netfilter `LOG` target | `SRC=26.24.119.87 DST=10.0.0.5` |
| `daddr={ip}` | ip | `dst_ip` | netfilter conntrack / nftables | `saddr=203.0.113.9 daddr=10.0.0.1` |
| `destination-address={ip}` | ip | `dst_ip` | Juniper SRX `RT_FLOW` | `destination-address="10.0.0.1"` |
| `from {mac}` | mac | `src_mac` | ISC dhcpd | `DHCPDISCOVER from 22:c9:92:51:77:d9 via eth1` |
| `to {mac}` | mac | `dst_mac` | ISC dhcpd | `DHCPACK on 10.0.0.5 to 23:af:69:b3:6d:91 via eth1` |
| `SPT={port}` | int / port | `src_port` | netfilter `LOG` target | `PROTO=TCP SPT=39021 DPT=443` |
| `sport={port}` | int / port | `src_port` | OpenBSD pf, Suricata EVE | `sport=39021 dport=443` |
| `DPT={port}` | int / port | `dst_port` | netfilter `LOG` target | `PROTO=TCP SPT=39021 DPT=443` |
| `dport={port}` | int / port | `dst_port` | OpenBSD pf, Suricata EVE | `sport=39021 dport=443` |
| `in:{word}` / `IN={word}` | word | `in_interface` | MikroTik RouterOS firewall log, netfilter `LOG` | `firewall,info input: in:ether1 out:(none)` |
| `out:{word}` / `OUT={word}` | word | `out_interface` | MikroTik RouterOS firewall log, netfilter `LOG` | `forward: in:ether1 out:ether2` |
| `proto {word}` | word | `proto` | MikroTik RouterOS, netfilter `PROTO=` | `proto TCP (SYN), 203.0.113.9:44321->10.0.0.1:443` |
| `protocol {word}` | word | `proto` | Cisco ASA, pfSense `filterlog` | `protocol tcp` |
| `len {int}` | int | `len` | MikroTik RouterOS, netfilter `LEN=` | `203.0.113.9:44321->10.0.0.1:443, len 60` |
| `length {int}` | int | `len` | Cisco ASA, Squid | `length 1500` |
| `TTL={int}` | int | `ttl` | netfilter `LOG` target | `PREC=0x00 TTL=51 ID=10342` |
| `via {word}` | word | `via` | ISC dhcpd (relay interface), MikroTik login log | `login failure for user admin from 10.0.0.9 via ssh` |
| `user {word}` (also `for user {word}`) | word | `user` | MikroTik RouterOS account log, OpenSSH sshd | `system,info,account user bob logged in from 10.0.0.5` |
| `username={word}` | word | `user` | Cisco ASA, FortiGate | `user=bob` / `username=bob` |
| `login={word}` | word | `user` | Check Point, SonicWall | `login=bob` |

## Shapes

| Context pattern | Slot kind | Name | Source of the convention | Example line |
|---|---|---|---|---|
| `{ip}:{port}->{ip}:{port}`, `{ip}:{port} -> {ip}:{port}`, `{ip}/{port} -> {ip}/{port}`, `{ip}->{ip}` | ip, int / port | `src_ip`, `src_port`, `dst_ip`, `dst_port` | MikroTik RouterOS firewall log, Cisco ASA (`inside:10.0.0.1/2000 to outside:8.8.8.8/53`) — the arrow points from source to destination | `proto TCP (SYN), 203.0.113.9:44321->10.0.0.1:443, len 60` |
| a second address pair on the same line | ip, port | *(left generic)* | RouterOS logs the translated addresses after `NAT (`; which side is pre- and which post-translation is a reviewer's call | `..., NAT (10.0.1.2:1997->1.2.3.4:80)->5.6.7.8:80, len 60` |
| `from {ip} port {port}` (also `to {ip} … port`) | int / port | `src_port` (`dst_port`) | OpenSSH sshd auth log | `Accepted publickey for ubnt from 130.124.175.56 port 19276 ssh2` |
| `for {word} from` | word | `user` | OpenSSH sshd auth log | `Accepted publickey for ubnt from 130.124.175.56 port 19276 ssh2` |
| `{word}[{int}]:` | int | `pid` | RFC 3164 §4.1.3 TAG, RFC 5424 PROCID | `CRON[1234]: (root) CMD (run-parts /etc/cron.hourly)` |
| `type={int}` / `code={int}` on a line whose constants name ICMP | int | `icmp_type`, `icmp_code` | RFC 792, netfilter `LOG` `TYPE=`/`CODE=` | `PROTO=ICMP TYPE=8 CODE=0` |
| a word slot whose every value is `[label]` | word | `rule` | iptables `--log-prefix`; EdgeRouter and ufw write the rule name there | `kernel: [WAN_IN-default-D]IN=eth0 OUT= MAC=…` |
| a slot whose every value is TCP flag mnemonics (`SYN ACK FIN RST PSH URG ECE CWR NS`, in any punctuation) | word / text | `tcp_flags` | netfilter `LOG` target, MikroTik `proto TCP (SYN,ACK)` | `WINDOW=1221 RES=0x00 ACK PSH URGP=0` |
| `{ip} - {user} [{timestamp}] "{request}" {status} {bytes} "{referer}" "{user_agent}"` | ip, word, timestamp, quoted, int, int, quoted, quoted | `src_ip`, `user`, `timestamp`, `request`, `status_code`, `bytes`, `referer`, `user_agent` | Apache `LogFormat … combined`, nginx `log_format combined` — the field order *is* the format | `203.0.113.9 - - [04/Sep/2026:10:15:23 +0000] "GET / HTTP/1.1" 200 5124 "-" "curl/8.4.0"` |
| a `timestamp` slot | timestamp | `timestamp` | the slot's own type | `[04/Sep/2026:10:15:23 +0000]` |

The quoted request line stays one slot: splitting `"GET /a HTTP/1.1"` into method, path and
version is a `sub` a reviewer adds, not something inference guesses.

## Changing this table

Add a row only for a convention you can cite in the source column — a vendor's log
reference or a widely deployed tool's own format. Then re-grade all four held-out files
and check the numbers did not move:

```
cargo run -p ulpf-infer --example infer -- heldout/mikrotik.log --slots
```

Names never change what a template matches, so the grades (templates, lines covered,
unmatched) are invariant under a naming change; a diff in them means the rule touched
something else.
