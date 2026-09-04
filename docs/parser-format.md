# Parser definition format

One TOML file per device family in `parsers/`. Loaded at startup by directory scan; a
malformed file is reported with its path and line and every other file still loads.
Check your file without a rebuild: `ulpf check` (or `cargo run -- check`).

A definition says three things: how to **recognise** an event (`[match]`), how to **split**
it into the vendor's own fields (`[strategy]`, optional `[[sub]]`), and where the **event
time** is (`[[timestamp]]`). It never mentions an output schema; that is `mappings/`.

```toml
[parser]
name = "cisco_asa"          # unique; also the sample and fixture file stem
vendor = "Cisco"
product = "ASA"
description = "optional"

[match]                     # every listed test must pass; at least one is required
contains = ["%ASA-"]        # all substrings must be present (fast, use this)
starts_with = "<"           # optional
regex = "..."               # optional, bytes regex, slow path
priority = 0                # higher is tried first when several parsers could match

[envelope]
syslog = true               # strip <pri> and an RFC 3164 / RFC 5424 header first

[strategy]
kind = "pattern"            # kv | delimiter | json | cef | leef | pattern
pattern = '%ASA-{severity:int}-{msg_id:int}: {message:rest}'

[[timestamp]]               # candidates tried in order; omit the section to use the
field = "eventtime"         # syslog header time automatically
format = "auto"

[[sub]]                     # second-level parse of one field, gated by `when`
field = "message"
when = { msg_id = ["302013", "302015"] }
kind = "pattern"
pattern = 'Built outbound {protocol:word} connection {connection_id:int} ...'
constants = { direction = "outbound" }
```

## Envelope

`syslog = true` handles, each part optional: `<PRI>`, then either RFC 5424
(`1 TIMESTAMP HOST APP PROCID MSGID SD`) or RFC 3164 (`Mon dd HH:MM:SS` with optional
year, or an ISO timestamp, then a hostname). Cisco's ` : ` after the hostname is
skipped. Fields produced: `syslog_pri`, `syslog_facility`, `syslog_severity`,
`syslog_timestamp`, `syslog_host`, and for 5424 `syslog_app`, `syslog_procid`,
`syslog_msgid`, `syslog_sd`. Fortinet-style bodies that start with `date=` keep their
whole body because no timestamp precedes them.

## Strategies

| kind | keys | notes |
|---|---|---|
| `kv` | `key_value_separator` (default `=`), `pair_separator` (default space; a space also means tab/CR/LF), `quote` (default `"`) | Bare tokens without a separator are skipped. `\"` inside quotes is unescaped. Check Point: separator `:`, pair `"; "`. |
| `delimiter` | `delimiter` (one byte or `tab`), `quote` (optional), `fields` (column names in order, `_` skips) | Short rows emit what exists; extra columns become `column_N`. |
| `json` | none | Nested keys flatten with `.`; arrays index from 0 (`tags.0`). Nulls are dropped. |
| `cef` | none | Header fields: `cef_version`, `device_vendor`, `device_product`, `device_version`, `signature_id`, `name`, `severity`; extension pairs as-is. |
| `leef` | none | LEEF 1.0 (tab) and 2.0 (declared delimiter, `xHH` form allowed). |
| `pattern` | `pattern` or `patterns` (first match wins), `regex` (raw, `(?P<name>...)`), `anchor` (`start` default, `full`, `none`) | See slot syntax below. |

A key that does not belong to the kind is an error (`key 'pattern' does not apply to
kind 'kv'`), so a typo cannot silently become a no-op.

## Pattern slot syntax

`{name:type}`; `{name}` alone is `text`; `{_:type}` matches without emitting a field;
`{{` and `}}` are literal braces. A single space in the pattern matches any run of spaces
and tabs, because real devices jitter their spacing (`server =  10.0.0.2`).

| type | matches |
|---|---|
| `int` | optional sign, digits |
| `float` | digits with optional fraction |
| `word` | anything but whitespace |
| `text` | shortest run up to the next constant (greedy when last) |
| `rest` | everything to the end, may be empty |
| `ip` / `ipv4` / `ipv6` | addresses |
| `port` | 1–5 digits |
| `hex` | optional `0x`, hex digits |
| `mac` | `aa:bb:cc:dd:ee:ff` or dashes |
| `timestamp` | syslog, ISO 8601 or epoch shapes |
| `quoted` | a double-quoted string; the field excludes the quotes |

The same syntax is what the inference engine emits (`Template::to_pattern`), so a
generated file and a hand-written one are the same kind of file.

## Timestamps

`[[timestamp]]` candidates are tried in order. `field = "x"` reads one field; `fields =
["date", "time"]` joins several with a space. `format` is `auto`, `rfc3339`, `syslog`,
`ctime`, `epoch`, `epoch_ms`, `epoch_us`, `epoch_ns`, or a strftime layout (`%Y-%m-%d
%H:%M:%S %z`). If no candidate yields a value, `syslog_timestamp` is tried with `auto`.
If nothing yields a value the engine uses the receipt time and flags it. Every
assumption the time module makes (year, zone) is recorded on the event as a policy
flag, and the original text is kept. Policies: `docs/timestamps.md`.

## Sub-parsers

`[[sub]]` re-parses one already-extracted field (usually `message`) with any strategy.
`when` lists field/value gates; a value may be a string or a list. Subs are tried in
file order; the first whose strategy matches wins and adds its `constants`. The event is
always emitted with its top-level fields; two counters tell you when the definition is
behind the device: `sub_no_match` (a gate matched but no pattern did — a pattern bug or a
truncated line) and `sub_uncovered` (subs exist but none is gated for this event — a
message id you have not written yet).

## Naming fields

Use the vendor's own names where the format has names (`kv`, `json`, `cef`). Where the
format is free text (`pattern`), name slots in the vendor's documentation vocabulary
(`connection_id`, `xlate_type`) and use `src_ip`/`dst_ip`/`src_port`/`dst_port`,
`protocol`, `action`, `user` for the universal ones so one mapping covers every parser.
Never put an output-schema path (`src_endpoint.ip`) in a parser file: it will not load.

## Checklist for a new family

1. `parsers/<name>.toml`, verified against the vendor's log reference, not memory.
2. `samples/<name>.log`: synthetic if no real sample exists, and deliberately messy.
3. `fixtures/<name>.expected.jsonl`: one line per sample event (see `fixtures/README.md`).
4. `ulpf check` reports no errors; `cargo test -p ulpf --test fixtures` passes.
