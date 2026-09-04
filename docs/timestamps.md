# Timestamps in perimeter-device logs

Survey verified 2026-09-04 against vendor docs (Fortinet doc library, Cisco IOS/ASA config
guides, Check Point R81 CLI guide, PAN-OS 11.1 syslog field reference, Suricata EVE docs,
SonicOS 7.1 / Sophos SFOS syslog guides). `ulpf-time` handles every row below.

| Device | Sample | `Format` that parses it |
|---|---|---|
| Fortinet FortiGate | `date=2026-09-04 time=10:15:23 tz="+0530"` (compose `date time tz`), `eventtime=1788516923123456789` (epoch **nanos** since 6.2.1; secs before) | Rfc3339 (space sep, ` +HHMM` tolerated), EpochNanos / Auto |
| Cisco ASA | `Sep 04 2026 10:15:23`; via relay `Sep  4 10:15:23` (no year); optional trailing zone name | Syslog |
| Cisco IOS / IOS-XE | `*Sep  4 10:15:23.123`, `.Sep  4 10:15:23`, `Sep  4 2026 10:15:23.123`, `Dec 22 2020 18:17:08.097 CST` (`*` unsynced, `.` unsure) | Syslog (prefix skipped) |
| Palo Alto PAN-OS CSV | `2026/09/04 10:15:23` (receive_time); high-res `2026-09-04T10:15:23.123-07:00` | Strftime `%Y/%m/%d %H:%M:%S`, Rfc3339 |
| Check Point | `1788516923` (log exporter), `4Sep2026 10:15:23` / `12Jun2018 12:33:00` (fw log), `2021-02-19T17:03:00Z` | EpochSecs, Strftime `%d%b%Y %H:%M:%S`, Rfc3339 |
| Juniper SRX (RFC 5424) | `2026-09-04T10:15:23.123+05:30` | Rfc3339 |
| pfSense / FreeBSD | BSD `Sep  4 10:15:23`; RFC 5424 `2026-09-04T10:15:23.123456+00:00` (2.5+) | Syslog, Rfc3339 |
| SonicWall | `time="2026-09-04 10:15:23 UTC"` | Strftime `%Y-%m-%d %H:%M:%S %Z` |
| Sophos SFOS | `date=2026-09-04 time=10:15:23 timezone="IST"` (compose) | Strftime `%Y-%m-%d %H:%M:%S %Z` |
| Suricata EVE | `2026-09-04T10:15:23.123456+0000` (6 fraction digits, no colon) | Rfc3339 |
| Squid access.log | `1756980923.123` (secs.millis) | EpochSecs / Auto |
| OpenVPN | `Thu Sep  4 10:15:23 2026` (ctime) | Ctime |
| Apache / nginx | `[04/Sep/2026:10:15:23 +0000]` | Strftime `%d/%b/%Y:%H:%M:%S %z` (Auto strips `[ ]`) |

## `Format::Auto` order (cheapest, most specific first)
1. **epoch** — only digits and at most one `.`; unit by integer magnitude: `<1e11` secs, `<1e14` ms, `<1e17` us, else ns.
   Consequence: a compact `YYYYMMDD` is read as epoch seconds (not in the survey; use an explicit layout).
2. **rfc3339** — `YYYY-MM-DD` then `T`/`t`/whitespace, `HH:MM:SS[.frac]`, optional whitespace, `Z`/`±HH:MM`/`±HHMM`/`±HH`.
3. **syslog** — month name first (leading `*`/`.` skipped); year before or after the time; trailing zone name or offset.
4. **ctime** — weekday name first, remainder is syslog.
5. layouts in this order: `%Y/%m/%d %H:%M:%S`, `%d/%b/%Y:%H:%M:%S %z`, `%d%b%Y %H:%M:%S`, `%Y-%m-%d %H:%M:%S %Z`.
6. otherwise `no_match`. A syntactic match with an impossible date (Feb 30, hour 24) is `out_of_range`, never retried.

## Policies (each is a `Policies` flag on the result; see DECISIONS.md D8–D12)
- **year_assumed** — no year: take the year of the receipt time *in the resolved zone*; if the result lands more
  than 7 days after receipt, use the previous year (December logs read in January). A Feb 29 the receipt year
  lacks also falls back to the previous year.
- **tz_assumed** — no zone at all: `Context::default_offset_secs` applied.
- **zone_name_ambiguous** — abbreviation with several meanings; the pick below applied.
- **zone_name_unknown** — abbreviation not in the table: default offset applied.
- Fraction digits beyond nine are truncated. Second `60` is accepted and rolls into the next minute.
- Range: civil year 1970..=9999, but epoch nanos are `i64`, so anything after `2262-04-11T23:47:16Z` is `out_of_range`.

## Zone table (fixed offsets; DST is never inferred — a name means exactly its offset)
| Offset | Names |
|---|---|
| 0 | UTC GMT Z UT WET |
| +1 | WEST CET MET WAT, **BST** (British Summer; not Bangladesh +6) |
| +2 | CEST MEST EET SAST CAT |
| +3 | EEST MSK EAT |
| +4 | **GST** (Gulf; not South Georgia -2) |
| +5 / +5:30 | PKT; **IST** (India; not Israel +2 or Ireland +1) |
| +7 / +8 | ICT WIB; HKT SGT MYT PHT AWST |
| +9 / +9:30 / +10:30 | JST KST; ACST; ACDT |
| +10 / +11 / +12 / +13 | AEST; AEDT; NZST; NZDT |
| -2:30 / -3 / -3:30 | NDT; BRT ART ADT; NST |
| -4 | EDT, **AST** (Atlantic; not Arabia +3) |
| -5 / -6 | EST, **CDT** (US Central; not Cuba -4); **CST** (US Central; not China +8 or Cuba -5), MDT |
| -7 / -8 / -9 / -10 | MST PDT; PST AKDT; AKST; HST |

Bold names are ambiguous and set `zone_name_ambiguous`. Lookup is case-insensitive; names are 1–5 letters
(a longer alphabetic tail such as a hostname is a `no_match`, not an unknown zone).
