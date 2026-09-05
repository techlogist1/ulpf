# Inference prototype: prefix-tree clustering on unseen perimeter formats

**Question.** Does tokenize -> bucket by token count -> prefix tree -> candidate-only merge produce
templates and typed slots a human would accept, on messy logs from formats no ULPF parser covers?

**Method.** Throwaway Python (stdlib), ~250 lines, deleted after this report. Pipeline exactly as
specified: punctuation-aware tokenizer with typed atoms (timestamp, mac, ipv6, ipv4, 0x-hex, int,
word, punct); bucket by token count; prefix key = first N tokens with variable-typed tokens masked;
inside a leaf, compare token-by-token against existing clusters only; merge when the share of
agreeing constant tokens >= threshold; slot type from the values seen (port = int <= 65535 after
`:` or an SPT/DPT/port key; mixed v4/v6 = `ip`; mixed kinds = `text`). Variants: A thr 0.5;
B thr 0.7 (spec as written); D = B + variable-typed tokens start as slots ("pretype") + hostname
masked in the prefix key; F = D + syslog header (timestamp + hostname) stripped before bucketing.
Scoring: pairwise precision/recall of line grouping vs ground-truth message type; slot type
accuracy against the generator's per-line variable list; a template is "usable" if its cluster is
pure, has >= 2 lines, no keyword became a slot, no slot mistyped, and no IP/MAC/port/hex/timestamp
got frozen as a constant. Templates with one hostname or one proto hard-coded still count as usable.

**Formats (synthetic, 617 lines, 30 message types).** Bodies follow vendor docs or captured lines;
messiness added: optional fields dropped at random, IPv6 beside IPv4, multi-word free text, 3%
truncated lines, stray `\xff\xfe` bytes and a Latin-1 `é` hostname, double-space/tab jitter,
no-year BSD timestamps (`Sep  4 ...`).
- MikroTik RouterOS, 223 lines / 12 types: firewall input/forward+NAT/ICMP, dhcp (de)assigned,
  account login/logout/failure (empty user, IPv6 source), wireless connect/disconnect (free-text
  reason), script, OSPF. Verified: help.mikrotik.com Log and Wireless Troubleshooting pages,
  fail2ban issue #3458 (login-failure lines), ManageEngine MikroTik syslog page (forward+NAT line).
- WatchGuard Firebox, 187 lines / 8 types: BSD-syslog traffic tcp/udp (`msg_id="3000-0148"` +
  offset/win/geo), Fireware 12.10 layout tcp open/close and icmp (proc_id/rc/flags/duration),
  dns-proxy deny, blocked-site event 3000-002A, quota event 3000-0065. Verified: watchguard.com
  "Read a Log Message", WatchGuard community syslog thread, LogZilla WatchGuard doc. Assumed, not
  verified: the `dns-proxy:` tag and a syslog header in front of the 12.10-layout bodies.
- Ubiquiti EdgeRouter, 207 lines / 10 types: kernel iptables TCP/UDP/ICMP/IPv6 (`[RULE]IN= OUT=
  MAC= SRC= ...`, empty OUT=, optional DF, variable flag words), NAT masquerade line, dhcpd
  ACK/REQUEST/DISCOVER, sshd accept/fail. Verified: fail2ban issue #2865 (six kernel lines with
  header), UISP EdgeRouter syslog page, Ubiquiti community captures (NAT-MASQ, DHCPACK, sshd);
  ip6tables and ICMP field layout is netfilter's LOG target, not vendor-specific.

**Results** (P/R = pairwise grouping precision/recall; cov = share of lines under usable templates;
types = of the format's true types, how many have at least one usable template).

| format | variant | clusters | singletons | impure | P | R | slot acc | usable | cov | types |
|---|---|---|---|---|---|---|---|---|---|---|
| MikroTik 12 types | A thr .5 | 69 | 27 | 8 | .76 | .15 | .97 | 22 | .38 | 9 |
| | B thr .7 | 95 | 56 | 6 | .89 | .13 | 1.00 | 19 | .36 | 8 |
| | D pretype+mask | 76 | 32 | 7 | .86 | .14 | .99 | 37 | .66 | 10 |
| | F D+skip header | 49 | 19 | 9 | .85 | .33 | .99 | 20 | .66 | 9 |
| WatchGuard 8 types | A thr .5 | 80 | 38 | 4 | .89 | .09 | .97 | 28 | .49 | 7 |
| | B thr .7 | 121 | 98 | 2 | .92 | .06 | .97 | 18 | .36 | 7 |
| | D pretype+mask | 46 | 16 | 1 | .91 | .22 | .99 | 22 | .58 | 6 |
| | F D+skip header | 44 | 13 | 1 | 1.00 | .22 | 1.00 | 23 | .68 | 7 |
| EdgeRouter 10 types | A thr .5 | 71 | 23 | 11 | .71 | .09 | .90 | 17 | .24 | 7 |
| | B thr .7 | 100 | 52 | 4 | .87 | .07 | .99 | 23 | .34 | 7 |
| | D pretype+mask | 65 | 27 | 4 | .77 | .15 | .98 | 33 | .65 | 9 |
| | F D+skip header | 80 | 38 | 1 | .78 | .15 | 1.00 | 41 | .71 | 9 |

Read: 4-10 clusters per true type in every variant. Precision is fine, recall is 0.06-0.33.
Slot typing is essentially solved once the tokenizer knows the atoms. Truncated lines: 19 of 22
became singleton templates; the `é` hostname is harmless after header stripping; `\xff\xfe` tails
always produce a singleton.

**Emitted templates** (variant F unless noted; count = lines).
- OK 16 `firewall,info input: in:{w1:word} out:(none), src-mac {m1:mac}, proto {w2:word}, {ip1:ipv4}:{p1:port}->{ip2:ipv4}:{p2:port}, len {n1:int}`
- OK 15 `system,error,critical login failure for user {w1:word} from {ip1:ip} via {w2:word}` -- and a separate OK 5 `... for user from {ip1:ip} via {w1:word}` for the empty-user lines (optional field = second template)
- OK 13 `kernel: [NAT-5010-MASQ] IN= OUT=eth0 src={ip1:ipv4} DST={ip2:ipv4} LEN={n1:int} TOS={x1:hex} PREC={x2:hex} TTL={n2:int} ID={n3:int} DF PROTO=UDP SPT={p1:port} DPT={p2:port}`
- OK 10 `dhcpd: DHCPACK to {ip1:ipv4} ({m1:mac}) via {w1:word}`
- OK-ish 9 `dns-proxy: msg_id="1DFF-0003" Deny 1-Trusted 0-External udp {ip1:ipv4} {ip2:ipv4} {n1:int} {n2:int} msg="{w1:word}: DNS {w2:word} {w3:word}" (DNS-proxy-00)` -- positional ports typed int, free-text msg half frozen
- BAD 22 `sshd[{n1:int}]: {w1:word} {w2:word} for {w3:word} from {ip1:ip} port {p1:port} ssh2` -- "Accepted publickey" and "Failed password" merged at 0.7; the disposition is gone
- BAD 23 `system,info,account user {w1:word} logged {w2:word} from {ip1:ip} via {w3:word}` -- login/logout merged (a human might keep this one)
- BAD 5 `wireless,info {m1:mac}@{w1:word}: disconnected, {w2:word} key exchange timeout, signal strength -{n1:int}` -- free-text reason: two reasons share a template, the other three reasons are 9 singletons
- BAD (D) 11 `{t1:timestamp} gw.local {w1:word},info {w2:word} {w3:word} {s1:text} {w4:word} {s2:text} {w5:word}` -- dhcp and wireless lines merged: prefix key was entirely `<ts> gw . local`, then constants eroded merge by merge
- BAD (D) 7 `{t1:timestamp} ubnt-café kernel: [{w1:word}]IN={w2:word} OUT= MAC={m1:mac} ... ID={n3:int} {w3:word} {s1:text} ...` -- `OUT=` empty plus `DF` present gives equal token count with columns shifted by one; 0.7 still merged; 12 garbage slots

**Failure modes, ranked by damage.**
1. Token-count bucketing. Every optional field (NAT block, `len`, geo_src/geo_dst, DF, OUT=eth1,
   DHCP hostname, empty user, TCP flag words) is a new template. True types spanned 2-9 token
   counts; MikroTik forward and WatchGuard legacy tcp each spanned 8-9. This alone caps recall.
2. Syslog envelope. Hostname is a word so it is a constant: templates and prefix keys split per
   device; dotted hostnames (`gw.local` = 3 tokens) also shift the count, so masking token 1 does
   nothing for MikroTik (D == B there). Stripping the envelope (F) halved MikroTik clusters.
3. Prefix key eaten by non-discriminating tokens: `<ts> gw . local` at depth 4 has zero signal; after
   stripping, `kernel : [ WAN_IN-default-D` puts the rule name in the key, one template set per rule.
4. Erosion. Incremental merging removes constants, later lines merge more easily; order-dependent
   impure mega-clusters (dhcp+wireless, Accepted+Failed, in+out).
5. Same-count misalignment merges (example above). No threshold fixes 4 and 5 together: 0.5 gives
   8-11 impure clusters and 34-114 keyword slots per format, 0.7 gives 50-100 singletons.
6. Free-text tails and multi-word values (`Living Room TV`, `(Unhandled External Packet-00)`,
   disconnect reasons) fragment into per-value templates or half-frozen text.
7. Punctuation inside values: `HTTPS-proxy.1-00` -> `{w:word}{s:text}{w:word}`, `-67` -> `-{n:int}`.
   Positional formats (WatchGuard) get `int` not `port` because there is no key or `:` context.

**Recommendation for the inference session.**
Keep: the typed-atom tokenizer (slot accuracy .96-1.00 comes from it); pretyped slots (removed all
32 frozen IPs/MACs on EdgeRouter and doubled usable templates); candidate-only comparison; the
template syntax; `ip` for mixed v4/v6 slots.
Change: (1) strip the syslog envelope (timestamp, hostname, optional tag) before anything else and
carry it as a fixed prefix; (2) drop exact token-count buckets for count-insensitive alignment
(align on constant tokens / `KEY=` anchors, LCS-style) so an absent field becomes an optional slot
rather than a new template; (3) compare on constant tokens with slot types required to agree, and
make the first two body words a hard key so disposition words cannot merge; (4) two-pass: cluster,
then re-assign every line to the final templates and re-derive slots (kills erosion and order
dependence); (5) min support 3, with singletons, truncated and non-UTF-8 lines routed to an
"unmatched" list instead of templates; (6) collapse a trailing run of disagreeing tokens into one
`{text}` tail slot; (7) treat an int slot in a positional layout as `port` when all values fit.

**Verdict.** As specified it yields correct, typed templates for fixed-layout lines (66-71% of lines
under usable templates after header stripping) but fragments every optional field into another
template and merges disposition words at loose thresholds; usable as a candidate generator for a
human to prune, not as an unattended parser generator.

## v1 engine (2026-09-05): the same question, answered with the shipped `ulpf-infer`

The prototype's seven failure modes were addressed as D46 records: the slot regexes are
the tokenizer (plus bracket groups, hex chains and a no-address-after-colon rule),
alignment replaces token-count buckets (gap-open 2, substitution 1, earliest match on a
tie), presence per column makes optional groups, a keyword split keeps dispositions
constant, a minority rule ignores damaged lines, split siblings are merged back, and
every pattern is compiled through the real parser and re-tested. Inputs: `heldout/`
(four files with ground truth, written from vendor pages and captures by a worker;
guesses are listed in `heldout/README.md`).

| file | lines | true types | templates emitted | lines covered | unmatched (reason) | notes |
|---|---|---|---|---|---|---|
| mikrotik.log | 250 | 14 | 14 (+1 dead, kept in evidence) | 250 | 0 | input/forward and ICMP/UDP/TCP split; NAT block optional; `logged in`/`out` separate; empty user optional |
| edgerouter.log | 250 | 10 | 9 | 250 | 0 | TCP/UDP/ICMP/IPv6 split; `DF` optional; `MAC=` chain one slot; NAT-masq lines a template; sshd Accepted/Failed separate |
| nginx_access.log | 250 | 3 (by request content) | 1 | 250 | 0 | `{ip} - {user} [{timestamp}] {quoted} {int} {int} {quoted} {quoted}` with a `[[timestamp]]` candidate |
| messy.log | 300 | 14 + cron, systemd, truncated, empty | 19 | 285 | 15: 4 empty, 4 below_support (truncated), 7 no_template (re-measured 2026-09-05 at a9d0dd8; the 289/11 quoted earlier predated the review fixes) | cron and systemd got their own templates; truncated lines never became a template |

Reading: the prototype's 4-10 clusters per true type became 1 per type on the three
clean files (one over on MikroTik: a `disconnected, {text}{? is lost}` tail a human
trims). What a human still edits: generic slot names where the format has no key
(`word1`, `ip1`), the occasional `{? is lost}` tail, and the choice between per-flag
templates and a slot when a keyword has three values. The brief's kill criterion
(candidate generation only) did not fire.

Iterations, in order, each graded on all four files: similarity 0.7 -> 0.6; key removed;
weighted alignment; ipv6+port and chain tokens; letters-only keyword split; minority
rule; dead-template drop; head-6 similarity; gap penalty; substitution state; first-token
substitution both ways; messy-run collapse tightened; keyword-aware dedupe. Eleven
rounds; the rejected alternatives are in PROGRESS.md "Tried and abandoned (v1)".
