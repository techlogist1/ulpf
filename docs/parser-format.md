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
kind = "pattern"            # kv | delimiter | json | cef | leef | pattern | xml
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
`syslog_msgid`, `syslog_sd`. Every RFC 5424 structured-data parameter also becomes a
field under its own name (`source-address="10.0.0.5"` is the field `source-address`), so
a device that puts the whole event there (Junos) needs no strategy beyond a catch-all
`{message:rest}`. Brackets that are not valid structured data (Check Point's
`[key:"value"; ...]`, a truncated element) stay in the message for the strategy; a 5424
timestamp written with a space (`2026-09-04 10:15:20`, older Check Point exporters) is
accepted. Fortinet-style bodies that start with `date=` keep their whole body because no
timestamp precedes them.

## Strategies

| kind | keys | notes |
|---|---|---|
| `kv` | `key_value_separator` (default `=`), `pair_separator` (default space; a space also means tab/CR/LF), `quote` (default `"`; may list several, `"'`, each closing itself) | Bare tokens without a separator are skipped. `\"` inside quotes is unescaped. Check Point: separator `:`, pair `"; "`. |
| `delimiter` | `delimiter` (one byte or `tab`), `quote` (optional), `fields` (column names in order, `_` skips), `rest` (optional name for everything after the last named column, unsplit) | Short rows emit what exists; extra columns become `column_N`. With `rest`, a `[[sub]]` gated on an earlier column splits the tail by the row's own type (pfSense, PAN-OS). |
| `json` | none | Nested keys flatten with `.`; arrays index from 0 (`tags.0`). Nulls are dropped. |
| `cef` | none | Header fields: `cef_version`, `device_vendor`, `device_product`, `device_version`, `signature_id`, `name`, `cef_severity`; extension pairs as-is. The seventh field is `cef_severity`, not `severity`, because CEF's scale is 0-10 while a device that writes a bare `severity` is on the syslog 0-7 scale; a vendor definition that speaks CEF and adds `[[sub]]`s sees the header value under `cef_severity` too. |
| `leef` | none | LEEF 1.0 (tab) and 2.0 (declared delimiter: one literal byte, or the hex value spelled `xHH`, `XHH`, `0xHH` or `0XHH`). A hex prefix whose digits do not parse is `invalid_leef`, not a silent fall back to tab. |
| `xml` | none | One document per event. Elements nest with `.` like `json`; attributes and text are fields; `<Data Name="X">v</Data>` is `EventData.X`. See "XML" below. |
| `pattern` | `pattern` or `patterns` (first match wins), `regex` (raw, `(?P<name>...)`), `anchor` (`start` default, `full`, `none`) | See slot syntax below. |

A generated definition (`origin = "inferred"`) is usually `kind = "pattern"`. When the unknown
file carries a `#fields` header and every data row has exactly the header's column count, the
engine writes a `kind = "delimiter"` definition instead: `fields` are the header's names
sanitised to `[A-Za-z0-9_]`, `[[timestamp]]` names the column whose every value is a timestamp,
and the `regex` matcher is the row's column count with that column's shape, anchored to the
whole line (D72). Zeek's TSV logs are the case it exists for.

A key that does not belong to the kind is an error (`key 'pattern' does not apply to
kind 'kv'`), so a typo cannot silently become a no-op.

### XML

`kind = "xml"` reads one XML document per event, the form Windows Event Forwarding,
`wevtutil` and the agents built on `EvtRender` hand on (Event Viewer's multi-line
rendering collapsed to one line). Rules, with the Windows Event shape as the example:

* The root element is the document, not a key: `<Event><System><EventID>4624` is the
  field `System.EventID`. Nested elements join with `.` the way `json` keys do.
* An attribute is a field under its element: `<Provider Name="P" Guid="G"/>` gives
  `System.Provider.Name` and `System.Provider.Guid`; `<TimeCreated SystemTime="..."/>`
  gives `System.TimeCreated.SystemTime`; `<Execution ProcessID="716" ThreadID="760"/>`
  gives `System.Execution.ProcessID` and `System.Execution.ThreadID`.
* An element whose only attribute is `Name` and which carries text is a named value:
  `<Data Name="LogonType">3</Data>` under `EventData` is `EventData.LogonType`, and the
  `Name` attribute itself is not a field. That is the `EventData` shape of every Windows
  provider, Sysmon included. An element with a `Name` and no text yields nothing.
* A repeated element without a name is numbered: `<Data>a</Data><Data>b</Data>` is
  `EventData.Data` then `EventData.Data2`, `EventData.Data3`, ... (any key that would
  repeat inside one event gets the counter, so nothing is silently overwritten).
* Namespace prefixes are stripped from element and attribute names (`<ns:Item>` is
  `Item`), and `xmlns` / `xmlns:*` declarations are not fields.
* Text is kept as written, including surrounding spaces (`User32 ` is what the device
  sent); whitespace-only text between elements (pretty-printed input) is not a field.
  CDATA is text without entity decoding. The XML declaration, comments and processing
  instructions are skipped; an empty element (`<Correlation/>`) yields nothing.
* Entity references in text and attribute values are decoded: `&amp;` `&lt;` `&gt;`
  `&quot;` `&apos;` `&#N;` `&#xN;`. Anything else after `&` (an unknown name, a
  reference cut off at the end of the line) is kept as written. A value with no `&` is a
  span of the event; a value with one is the strategy's single materialisation, counted
  like a JSON value or an unescaped quoted value.
* Failures are counted as `invalid_xml`, never a panic: input that is not UTF-8, no
  element at all, an unterminated tag (`<Event` or `</a` cut off), a stray `<` in text.
  A document whose closing tags were cut off still yields the fields it carried. A 1 MB
  attribute is one borrowed span.
* Field names are the XML's own, so `[[timestamp]]`, `[[sub]] when` and mappings use
  the dotted paths (`field = "System.TimeCreated.SystemTime"`,
  `when = { "System.EventID" = "4624" }`). A sub that only adds constants re-parses
  `System.EventID` with `pattern = "{_:int}"`, which emits nothing and matches.

Not handled: DTD entity declarations (the reference stays as written), UTF-16 input (a
counted failure; convert at the forwarder), and a document with more than one root (the
second root's fields are numbered like any repeat).

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
| `timestamp` | the shapes the time module reads: syslog/ctime (optional weekday, Cisco IOS `*`/`.` clock mark, fraction, year before or after the time, a known zone name or a numeric offset), ISO 8601, epoch |
| `quoted` | a double-quoted string; the field excludes the quotes |

A slot that captures nothing (`rest` with nothing left, `quoted` of `""`) emits no field:
the absence of text is the absence of a field. When the slot syntax cannot split a
message unambiguously (Junos writes `source rule nat-out N/A N/A`, two-word tokens next
to one-word ones), use `regex` with `(?P<name>...)` groups instead; the file stays the
same kind of file.

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
`when` lists field/value gates; a value may be a string or a list; a sub with no `when`
applies to every event. Subs run in file order, and each field is re-parsed by at most
one sub: the first eligible one whose strategy matches it, which then adds its
`constants`. Subs on different fields all run (SonicWall splits `src`, `dst` and `proto`
in one definition), and a later sub may gate on a field an earlier sub produced (pfSense
splits the CSV, then the IP-version tail, then the protocol tail). The event is always
emitted with its top-level fields. The status, per event and as counters, is the worst
outcome over the fields that have subs: `sub_no_match` (a gated sub ran and its strategy
failed: a pattern bug, a truncated line, or, for ungated subs, a message you have not
modelled) beats `sub_uncovered` (a field with subs is present but no sub is gated for it:
a message id you have not written yet) beats `matched`; when none of the sub fields is
present at all the status is `not_applicable`, the same as a definition without subs.
A sub on a field whose value was materialised rather than borrowed from the event (any
JSON value, a quoted value with escapes, an RFC 5424 parameter with escapes) runs on a
copy of that value; it behaves the same and costs one allocation for that event.

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
