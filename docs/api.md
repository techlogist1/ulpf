# ULPF v1 HTTP API contract

The server is a window onto a running engine. It reads the engine's `Metrics`, the raw
store, the parser registry and the pending directory; it owns no state of its own. This
document is the contract the server and the UI are built against independently; a
change here is a change to both.

Base URL `http://127.0.0.1:7878` (`ulpf serve --listen`). Every response body is JSON
unless stated. Every error the server decides is `{"error": "<human text>", "reason":
"<code>"}` with the status: `404 not_found`, `409 conflict`, `422 invalid`, `500 io`.
A malformed query or path parameter (a non-numeric raw id, an unparseable `after`) is
rejected by the framework with a plain-text `400`. Every 4xx a review route decides
increments `review_errors`; with inference disabled the per-proposal review routes answer
`404 not_found` and `GET /api/pending` answers an empty list.

## Streaming

`GET /api/stream?tail=N` — server-sent events. `N` (default 100, max 500) is the size of
the initial tail snapshot. Keep-alive comment every 15 s. Event kinds, in `event:`:

| event | when | data |
|---|---|---|
| `hello` | once, first | `{ "latest_raw_id": u64\|null, "pending_generation": u64, "pending_count": u64, "tail": TailFrame }` |
| `metrics` | every 500 ms | `MetricsFrame` |
| `tail` | every 250 ms, only when new lines exist | `TailFrame` |
| `pending` | when the pending set changes | `{ "generation": u64, "count": u64 }` |

A client that disconnects is dropped at its next tick; `server.sse_clients` is the live
count. The tail is a bounded ring (`--tail`, default 1000 events); a client that falls
behind receives at most 200 events per tick and `skipped` counts every line newer than its
position that the frame did not carry, whether the ring evicted it or the frame's own limit
left it behind (`cut`, below, is that second part). Nothing blocks the engine on a slow client.

`TailFrame = { "events": [TailEvent], "skipped": u64, "latest_raw_id": u64|null }`
(v4 adds `"cut": u64` beside `skipped`; see "Tail frame" below)
`TailEvent = { "raw_id": u64, "line": <normalized event object as emitted> }`

## Read-only

`GET /api/status` → `{ "version", "started_at": rfc3339, "listen", "store", "parsers_dir",
"pending_dir", "output", "output_format": "jsonl", "parquet": path|null, "watch": [dir],
"threads", "queue_capacity", "tail_capacity", "infer_threshold" }`. `output_format` is
always `"jsonl"`: Parquet is an additional sink, not a replacement (see below).

`GET /api/metrics` → `MetricsFrame`:
```
{
  "engine":    Snapshot,                  // crates/ulpf/src/metrics.rs, verbatim
  "sources":   [ { "name", "events", "detected", "no_parser", "buffered",
                   "last_seen": rfc3339|null, "pending_id": string|null } ],
  "parsers":   [ ParserInfo ],
  "pending_generation": u64,
  "parquet":   { "rows": u64, "files": u64, "errors": u64 },
  "server":    { "sse_clients": u64, "review_errors": u64, "uptime_secs": f64 }
}
```
`Snapshot` includes the v1 counters: `infer_buffered` (lines ever buffered; a source's
`buffered` above is its current buffer), `infer_buffer_full`, `infer_runs`,
`infer_lines_templated`, `infer_lines_unmatched`, `proposals_written`,
`proposals_replaced`, `proposals_skipped` (list of `[reason, n]`: `edited`,
`duplicate`, `rejected`, `no_templates`), `approved`, `rejected`, `reloads`.

`ParserInfo = { "name", "vendor", "product", "priority": i32, "strategy": "kv|delimiter|json|cef|leef|pattern",
"subs": n, "origin": "hand|approved", "version": u64, "detected": u64 }` (`detected` counts
events this process routed to the parser; `origin` is `approved` when the definition carries
`[parser] origin = "inferred"`, which the inference engine writes; a hand-written parser with a
negative priority stays `hand`).

`GET /api/tail?after=<raw_id>&limit=N` → `TailFrame` with events whose id is greater
than `after` (omit for the newest `N`, max 500).

`GET /api/parsers` → `[ParserInfo]`

`GET /api/events/{raw_id}` → traceback:
```
{
  "raw_id": u64, "source": string, "receipt": rfc3339, "receipt_nanos": i64,
  "bytes_len": u64,
  "text": string,               // lossy UTF-8, control bytes escaped as \xNN
  "hex": string,                // lowercase hex of the exact bytes
  "stored_sha256": hex64, "recomputed_sha256": hex64, "digest_match": bool,
  "emitted": object|null,       // the normalized line as emitted, if still in the tail
  "now": {                      // the same bytes through the current parsers, right now
    "parser": string|null, "parse_status": string, "normalized": object
  }
}
```
`404 not_found` with `"store_len": u64` when the id was never issued. The bytes are read
through the writer's own lock, so an id that was emitted is always readable.

## Review workflow

The pending directory holds one proposal per source: `<id>.toml` (the parser
definition, editable), `<id>.json` (evidence and review state, read-only) and
`<id>.lines` (the unknown lines it was built from). Approval is the only path from
pending to active. Rejected proposals move to `pending/rejected/`; approved
evidence moves to `pending/approved/`; the approved definition is written to the parsers
directory and the registry is reloaded in place.

`GET /api/pending` → `[ { "id", "source", "name", "created": rfc3339, "lines": u64,
"templates": u64, "unmatched": u64, "edited": bool, "problems": u64 } ]`

`GET /api/pending/{id}` →
```
{ "id", "source", "definition": string /* TOML text */, "problems": [ "path:line: message" ],
  "evidence": Evidence, "edited": bool }
```
`problems` is the load report for the text as it is on disk: a file edited by hand into
invalid syntax lists its error here and refuses approval with the same list.

`PUT /api/pending/{id}` body `{ "definition": string }` → `{ "problems": [..] }`. Saved
even with problems (the human is mid-edit); marks the proposal `edited`, which stops the
engine from replacing it with a later proposal for the same source. A save that fails on
disk answers `500` with `"reason": "io"`, an `error` naming the file it failed on and the
operating system's own reason, and — whenever the failure is about one known file — a
`"path"` field holding that file (the definition `<id>.toml`, or `<id>.json` when it is the
record that could not be read or written). On Windows a failing save can take about half a
second: the create and the rename are retried through a scanner's transient lock.

`POST /api/pending/{id}/regenerate` body `{ "keep": [template id], "merge": [[template id]] }`
→ `{ "definition": string, "problems": [..] }`. Each `merge` group becomes one new
template built from the union of its members' lines (keyword splitting off) and is
appended to the evidence with a new id; the definition is then re-emitted from the kept
templates (merged groups replaced by their merged template), keeping the human's edits to
everything but `patterns`. Templates not kept stay in the evidence. This is the
merge-or-discard control; `merge` may be empty.

`POST /api/pending/{id}/approve` →
```
{ "name": string, "path": string, "parsers_loaded": u64, "problems": [..],
  "now_detected": { "tested": u64, "detected": u64 } }
```
`now_detected` re-runs detection over the source's buffered unknown lines with the new
registry: the proof that the same events now take the fast path. Errors: `404` no such
pending id (an approve after approve), `422` the definition does not load (the body
carries `"problems": [..]`), `409` an active parser already has that `[parser] name`
(edit the name first).

`POST /api/pending/{id}/reject` → `{ "id", "moved_to": path }`. The proposal's template
fingerprint is remembered; a later identical proposal for that source is skipped and
counted under `proposals_skipped[rejected]`.

`POST /api/parsers/reload` → `{ "parsers_loaded": u64, "problems": [..], "generation": u64 }`.
Reloads the parsers directory (a teammate dropped a file by hand). The engine also
reloads on its own when the directory's modification time changes.

## Evidence (what is on screen when a proposal looks wrong)

```
{
  "source": string, "generated": rfc3339, "lines_seen": u64, "lines_used": u64,
  "params": { "similarity": f64, "min_support": u64, "enum_max": u64, "rare_share": f64, "max_templates": u64 },
  "envelope": { "syslog": bool, "example_header": string|null },
  "templates": [ {
    "id": u64, "pattern": string, "support": u64, "verified": u64,
    "examples": [string],                       // up to 3 member lines, body only
    "slots": [ { "name", "kind", "suggested": bool, "preceded_by": string,
                 "distinct": u64, "examples": [string] } ],
    "members": [u32],                           // indices into <id>.lines
    "history": [string]                         // "cluster 3 (...)", "split on `input` (68 lines)", "not in the definition: ..."
  } ],
  "unmatched": { "count": u64, "examples": [string], "by_reason": { "empty": n, "too_long": n, "below_support": n, "no_template": n, "template_cap": n, "header": n } },   // header: a `#`-prefixed metadata line of a delimited file (D72)
  "decisions": [string],                        // every threshold decision the engine took, in order
  "fingerprint": string
}
```
`verified` is the number of lines the compiled pattern matched first in the second pass,
patterns tried in definition order; `support` is the cluster size that produced it. A
template with `verified` 0, or with `support` below `min_support` after a split, is kept
in the evidence but left out of the definition (its `history` says so); `regenerate`
puts it back. `generated` is the time the pending record was written.

## Output format (item 13): JSON Lines always, Parquet as well

`--parquet FILE` on `run` and `serve` adds a columnar copy of the normalized events. It
is an *additional* sink and never a replacement: a Parquet file carries its schema and
row-group index in a footer written at close, so an open file is unreadable, while the
JSON Lines file is complete after every line. `output_format` in `/api/status` therefore
stays `"jsonl"` and `parquet` names the file (or `null`).

One row per emitted line, in emitted order (`raw_id` ascending), eleven columns:

| column | type | null? | from |
|---|---|---|---|
| `raw_id` | `int64` | no | the raw store id, the same value as `ulpf.raw_id` |
| `time` | `int64` `TIMESTAMP_MILLIS` (UTC) | no | the event's `time` |
| `parser` | `utf8` | yes | `ulpf.parser`; null when no parser claimed the event |
| `source` | `utf8` | no | `metadata.log_name` |
| `class_uid` | `int32` | no | `class_uid` (0 = no class rule matched) |
| `normalized` | `utf8` | no | the emitted JSON line, verbatim, without its newline |
| `src_ip` | `utf8` | yes | `src_endpoint.ip` |
| `dst_ip` | `utf8` | yes | `dst_endpoint.ip` |
| `user` | `utf8` | yes | `user.name` |
| `device` | `utf8` | yes | `device.hostname` |
| `dst_port` | `int32` | yes | `dst_endpoint.port` when it is an integer |

The ten scalar columns are the ones a SIEM filters on; anything else is one
`json_extract` away in the `normalized` column, so no query needs the JSON Lines file.
SNAPPY, row groups of 8192 rows.

`run` writes exactly the file it was given, one file per run; a second run with the same
`--parquet` path replaces it, where the JSON Lines output is appended to. `serve` rolls: the file
being written is `<stem>.<seq>.parquet.part` and is renamed to `<stem>.<seq>.parquet`
the moment its footer lands, so a reader that lists `*.parquet` only ever sees complete
files. A file is closed after `--parquet-roll-rows` rows (default 1,000,000) or
`--parquet-roll-secs` seconds (default 300), whichever comes first; the check runs once
per batch, so a file can overshoot by up to one batch. `<stem>` is the `--parquet` path
with a trailing `.parquet` removed.

Counters, in `Snapshot` (so in the counter block, `GET /api/metrics` `engine`, and the
`metrics` SSE event) and repeated as the frame's `parquet` object: `parquet_rows`,
`parquet_files` (closed, therefore readable), `parquet_errors`. A write that fails is
counted, reported on stderr and stops the sink; the run continues and the JSON Lines
output is unaffected — the sink can never cost you an event.

## UI assets

`GET /` → `ui/dist/index.html`; `GET /app.js`, `GET /app.css` → the built assets. Fixed
names so the binary embeds them with `include_str!`. `--ui-dir DIR` serves the three
files from disk on every request instead (restyle without a rebuild).

---

# v2 additions (2026-09-05 evening): provenance, integrity, replay, pivot, drift, syslog

Everything above stays as written. Below is what the v2 server adds; the UI is built
against this text, so a change here is a change to both. Every new error follows the
same `{"error", "reason"}` shape and status set. New reasons: `409 conflict` for an
operation already running (replay, verify), `422 invalid` for an unknown pivot kind or
replay version. Every route below is backed by `Live` (D41); the server still owns
nothing.

## Provenance (traceback, item 1)

`GET /api/events/{raw_id}` gains, inside `now`:
```
"fields":     [ { "key": string, "value": string, "span": [u64, u64]|null } ],
"provenance": [ { "path": string, "source_key": string, "span": [u64, u64]|null,
                  "canonical": bool, "value": string } ],
"time":       { "text_span": [u64, u64]|null, "policies": [string] }
```
`fields` are the parser's own key/value pairs in parser order; `span` is the half-open
byte range `[start, end)` into the raw record's bytes (the same bytes `hex` shows) when
the value is a borrowed slice of the event (D15), `null` when it was materialised (a JSON
value, an unescaped quoted value, a joined multi-field timestamp, a `column_N` name) or
came from the definition (a constant). `provenance` has one entry per normalized schema
field that came from a source field: `path` is the dotted schema path in `normalized`
(`src_endpoint.ip`), `source_key` the parser field that fed it, `span` that field's span,
`canonical` true when the mapping rewrote the value (an enum: `deny` -> `Denied`),
`value` the normalized value as text. Fields the mapping synthesised (`class_uid`,
`metadata.*`, an enum `unknown`) have no entry. Offsets are computed only on this route,
never on the hot path.

Also added at the top level: `"chain": hex64, "prev_chain": hex64, "chain_match": bool`
(the record's stored chain value, the previous record's, and whether
`sha256(prev_chain || stored_sha256)` equals `chain`).

## Integrity chain (item 5)

Every raw record `i` has a chain value `chain_i = SHA-256(chain_{i-1} || sha256_i)` with
`chain_{-1} = genesis = SHA-256("ULPF chain genesis" || store id)`. The store id is a
random 16-byte value written when the store is created. Tampering with any byte of any
record, its header digest included, changes every chain value from that record on. The
chain is stored beside the offsets index, appended with the record, and cut with it on
recovery (D7, D33).

`GET /api/integrity` →
```
{ "records": u64, "store_id": hex32, "genesis": hex64, "head": hex64|null,
  "checkpoint_every": 4096,
  "last_verify": null | { "at": rfc3339, "records": u64, "ok": bool,
                          "corrupt": u64, "first_bad": u64|null, "reason": string|null,
                          "elapsed_secs": f64, "against_attestation": bool },
  "running": bool }
```
`first_bad` is the lowest raw id whose bytes do not hash to the stored digest or whose
chain value does not follow from its predecessor; `reason` is `digest` or `chain`.

`POST /api/integrity/verify` → `{ "started": true, "records": u64 }`; `409 conflict`
while one runs. Runs on its own thread over a snapshot of the store (ids below the
length at start), reading through the writer's files; the result lands in
`GET /api/integrity` and is pushed as an `integrity` SSE event.

`GET /api/integrity/attestation` → the attestation document, also written by
`ulpf attest --store DIR --out FILE`:
```
{ "format": "ulpf-attestation/1", "generated": rfc3339, "store_id": hex32,
  "records": u64, "genesis": hex64, "head": hex64,
  "checkpoints": [ { "id": u64, "chain": hex64 } ],      // every 4096th record and the last
  "record_digest": "sha256(bytes)",
  "chain": "sha256(prev_chain || record_digest)",
  "verify": "ulpf verify --store DIR --attestation FILE" }
```
A stranger with the store directory and this file runs the verify command offline; the
report names the first record whose recomputed chain disagrees with the store, or the
first checkpoint whose recorded chain disagrees with the recomputation (a store rewritten
consistently from record N onward passes the store-only check and fails the attestation
check at the first checkpoint at or after N).

## Replay (item 2)

Outputs are versioned. The path given as `--output out.jsonl` is version 1 (the live
output); a replay writes `out.v2.jsonl`, `out.v3.jsonl`, ... beside it, each with
`out.vN.meta.json` (when, parsers generation, the SHA-256 of every parser and mapping
file used, schema, events) and, when a previous version exists, `out.vN.diff.jsonl`.
The raw store is opened read-only by the replay (through the writer's snapshot in
`serve`, D42); it is never written.

`GET /api/replay` →
```
{ "versions": [ { "version": u64, "path": string, "created": rfc3339, "events": u64,
                  "schema": string, "parsers_generation": u64 } ],
  "running": null | { "version": u64, "done": u64, "total": u64, "started": rfc3339 },
  "last": null | ReplayReport }
```
`POST /api/replay` body `{}` or `{ "schema": string }` (an empty body is accepted only without a
`Content-Type: application/json` header; a client that sets the header sends `{}`) → `{ "version": u64, "started": true,
"total": u64 }`; `409 conflict` while a replay runs. The replay uses the parser pipeline
as it is at the start (the `Arc` it read); a parser approved mid-replay takes effect for
the live stream and the next replay, and the report says which generation it used.

```
ReplayReport = {
  "version": u64, "previous_version": u64|null, "output": string, "diff": string|null,
  "events": u64, "elapsed_secs": f64, "events_per_sec": f64, "parsers_generation": u64,
  "summary": { "unchanged": u64, "changed": u64, "only_in_new": u64, "only_in_old": u64,
               "fields_added": u64, "fields_lost": u64, "fields_changed": u64,
               "parser_changes": [ { "from": string|null, "to": string|null, "events": u64 } ],
               "by_field": [ { "path": string, "added": u64, "lost": u64, "changed": u64 } ] },
  "why": [ string ]     // "parsers/cisco_asa.toml changed since v1 (sha 3f2a.. -> 9c01..)", "mappings unchanged", "12 events only in v2: appended after v1 was written"
}
```
`why` is the 4am answer: the report compares the file digests recorded in the previous
version's meta with the ones it used, and states every difference it can see. A diff
with an empty `why` and a non-empty `changed` count says so explicitly
(`"no parser or mapping file changed; the difference comes from receipt time or engine
version"`).

`GET /api/replay/{version}/diff?after=<raw_id>&limit=N&kind=changed|only_in_new|only_in_old`
→ `{ "entries": [ DiffEntry ], "next_after": u64|null }`, `limit` max 500, ordered by raw id.
```
DiffEntry = { "raw_id": u64, "kind": "changed|only_in_new|only_in_old",
              "parser_before": string|null, "parser_after": string|null,
              "added": { path: value }, "lost": { path: value }, "changed": { path: [before, after] } }
```
`404 not_found` for a version that does not exist or has no diff.

SSE `replay` event: `{ "version": u64, "state": "started|progress|done|failed",
"done": u64, "total": u64, "report": ReplayReport|null, "error": string|null }`; a
progress frame at most every 500 ms.

CLI: `ulpf replay --store DIR --output out.jsonl [--schema ..] [--parsers ..] [--mappings ..]`
prints the same report and exits 0; exit 2 when the store is in use by a `serve`.

## Pivot (item 3)

The mapping file declares which schema paths are entities; the index and the routes
know only entity kinds, never vendor fields (the wall holds: `[entities]` lives in the
mapping, beside `[fields]`):
```
[entities]                       # mappings/ocsf.toml
src_ip   = "src_endpoint.ip"
dst_ip   = "dst_endpoint.ip"
user     = "user.name"
dst_port = "dst_endpoint.port"
device   = "device.hostname"
```
The five kinds are fixed: `src_ip`, `dst_ip`, `user`, `dst_port`, `device`. `device`
falls back to the ingest source name when the schema field is absent, so every event has
a device. The index lives beside the output (`out.jsonl.pivot`), is derived data (rebuilt
by `ulpf pivot --rebuild --output out.jsonl`), and is written by its own thread from the
entity spans the normalizer reports per event; the hot path gains no allocation. It is on
by default in `serve` and off in `run` (`--pivot on|off`, D66: its cost is per distinct
entity value and dominates a bulk run of high-cardinality data); a route on a server whose
index is off answers `404 not_found`. Paging: `before` plus `before_id` (both from the
previous page's `next_before` and `next_before_id`) so events sharing a millisecond are
neither repeated nor skipped.

`GET /api/pivot?kind=K&value=V&limit=N&before=<time_ms>&order=desc|asc` →
```
{ "kind": K, "value": V, "total": u64, "first_time": ms|null, "last_time": ms|null,
  "devices": [ { "device": string, "events": u64, "parsers": [string] } ],
  "related": { "src_ip": [ {"value", "events"} ], "dst_ip": [..], "user": [..], "dst_port": [..], "device": [..] },
  "events": [ { "raw_id": u64, "time": ms, "device": string, "parser": string|null, "line": object } ],
  "next_before": ms|null }
```
`events` is the timeline (default newest first, `limit` max 500, `before` pages older;
`order=asc` with `after` pages forward); `related` lists the ten most frequent
co-occurring values per other kind, computed over at most the newest 10,000 events of
the entity (`related_over: u64` says how many). `total` is exact. An entity with a
million events answers in bounded time: the posting list is read by range, never whole.
`422 invalid` for an unknown kind.

`GET /api/entities?kind=K&q=prefix&limit=N` → `{ "entities": [ { "kind", "value",
"events": u64, "devices": u64, "first_time": ms, "last_time": ms } ] }`, most events
first, `limit` max 100. Omit `kind` for all kinds.

CLI: `ulpf pivot KIND VALUE --output out.jsonl [--limit N]` prints the timeline as JSON Lines.

## Drift (item 4)

Per source, the engine keeps a rolling window (512 events) of misses (no parser claimed
the event, or the source's established parser claimed it and failed) beside the
long-run miss rate. A source is *established* after 1,024 events with a long-run miss
rate under 20%; it *trips* when the window's miss rate exceeds the long-run rate by
0.25 or more with at least 32 misses in the window. A source that has always mixed two
formats has a high long-run rate and never trips (its unknown half still feeds ordinary
inference). On a trip the window's misses are routed to inference with the established
parser as the prior: the proposal is that parser's definition plus the new templates,
`version` incremented, written to `pending/` as an *update*.

`GET /api/drift` → `[ DriftAlert ]`
```
DriftAlert = { "source": string, "parser": string, "state": "watching|tripped|proposed|cleared",
               "since": rfc3339, "window": { "events": u64, "misses": u64, "rate": f64 },
               "baseline_rate": f64, "lines_routed": u64, "pending_id": string|null,
               "proposed_version": u64|null }
```
`watching` is every established source (rate under threshold); the list is ordered
tripped/proposed first. The `sources` array in `MetricsFrame` gains `"parser": string|null,
"window_rate": f64, "baseline_rate": f64, "drift": "none|watching|tripped|proposed"`.

Pending records gain: `"updates": string|null` (the parser this proposal replaces),
`"version": u64` (proposed), `"current_version": u64|null`. `GET /api/pending/{id}` gains
`"update_kind": "patterns_added"|"matcher_widened"|null` (how the update was composed on
the prior), `"current_definition": string|null` and `"diff": string|null` (a unified diff
of the current file against the proposal, for the review screen's diff view). Approving an
update writes over `parsers/<name>.toml` atomically, keeps the replaced file as
`pending/approved/<name>.v<current>.toml`, and the approve response gains
`"replaced_version": u64|null`. A hand-written parser's `[parser]` table may carry
`version = N` (default 1).

SSE `drift` event: a `DriftAlert`, sent when a source's state changes. Counters in
`Snapshot`: `drift_tripped`, `drift_lines_routed`, `drift_proposals`, `drift_cleared`.

## Syslog listeners (item 6)

`ulpf serve ... --syslog-udp ADDR --syslog-tcp ADDR` (either, both, or neither). A UDP
datagram is one event, stored byte for byte with no terminator added. A TCP stream is
framed by RFC 6587 octet counting when a connection starts with `digits SP`, otherwise by
the same line rule as files (terminators kept inside the event); a connection that closes
mid-event stores the partial bytes as an event (nothing is dropped) and counts
`syslog_tcp_partial`. The source name is `udp/<peer ip>` or `tcp/<peer ip>`, so drift,
inference and the pivot's device fallback are per sending device. Listeners share the
engine's queue and its block-on-full policy: a burst past capacity blocks the listener
thread, and the kernel's socket buffer absorbs or drops behind it; what the kernel drops
is invisible to the process by design and is measured by the soak from the sender's
count. `GET /api/status` gains `"syslog": { "udp": addr|null, "tcp": addr|null }`.
`Snapshot` gains `syslog_udp_datagrams`, `syslog_udp_bytes`, `syslog_tcp_connections`,
`syslog_tcp_events`, `syslog_tcp_bytes`, `syslog_tcp_partial`, `syslog_tcp_refused` (connections
beyond the cap of 256) and `syslog_errors`. `GET /api/status` `syslog` carries the addresses
actually bound (port 0 resolved), `null` while a listener is not up.

## Metrics frame, status and evidence additions

`MetricsFrame` gains `"integrity": { "records", "head", "last_verify", "running" }` (as
in `GET /api/integrity`, without checkpoints), `"replay": { "running", "last_version" }`,
`"drift": [ DriftAlert ]` (tripped and proposed only), `"syslog": { "udp_datagrams",
"tcp_events", "tcp_connections" }`. `GET /api/status` gains `"schema": { "name",
"version", "entities": { kind: path } }`, `"output_format": "jsonl|parquet"`, `"syslog"`.

`Evidence.templates[].slots[]` gains `"reason": string`: why the name was suggested
(`"key \`src-mac\` before the value"`, `"vocabulary: \`{ip}:{port}->{ip}:{port}\` names
src/dst"`) or why it stayed generic. `suggested` keeps its meaning (true = a rule
produced the name).

## Output format (item 13, only after 1-12)

`--format jsonl|parquet` on `run` and `replay`; the tail, traceback `emitted`, pivot
and diff read JSON Lines, so with `parquet` the server keeps the tail in memory and the
pivot index carries the line text. Documented when it lands; absent until then.

## SSE client obligations

The server sends at most one `tail` frame per 250 ms with at most 200 events and one
`metrics` frame per 500 ms regardless of client count. A client renders frames on
`requestAnimationFrame`, keeps at most `--tail` rows in the DOM, and drops frames it
could not render rather than queueing them, so a full-rate run cannot lock the browser.

---

# v4 additions (2026-09-06, the morning of the demo): the frame tells the truth, flags per event, export

Everything above stays as written. Every addition below is a new field, a new route or a new
query parameter; nothing that exists changes shape. The UI is built against this text.

## Tail frame: what is gone, apart from what this frame did not carry

`TailFrame` gains `"cut": u64`, the part of `skipped` the frame left out because of its
limit (`?limit=`, or 200 per SSE tick): those lines are still in the ring, so another
request reaches them. `skipped` keeps its meaning, the total a caller at that position did
not receive, so `skipped - cut` is the part the ring evicted before it was read and is the
only part that is gone. A bulk drop larger than one frame therefore reports `cut` and no
eviction, and the UI shows eviction in `--warn` and the cut as a plain note.

## Metrics frame: the queue as it is, and a windowed rate

`MetricsFrame` gains:

- `"queue": { "depth": u64, "capacity": u64 }`: the batches in flight between the ingest
  threads and the output thread at the moment the frame was computed (the counter the
  high-water mark is taken from). `engine.queue_high_water` stays the high-water mark since
  start.
- `"rate": { "over_secs": f64, "framed_per_sec": f64, "emitted_per_sec": f64 }`: framed and
  emitted events per second over the frames the server computed in the last 10 s at most;
  `over_secs` is the span between the oldest sample kept and this frame; both rates are 0 with
  fewer than two samples. The server samples whenever it computes a frame (an SSE tick, `GET
  /api/metrics`), so the window covers the time a client was watching. `engine.events_per_sec`
  stays the run average since start, the number the counter block prints.

`GET /api/status` gains `"pivot_index": bool`, whether the entity index is running in this
process (`--pivot`), beside `threads`: the two numbers a person quotes about the machine.

## Traceback: the emitted line from the output, and the bytes on their own

`GET /api/events/{raw_id}`:
- `emitted` is looked up in the tail ring first and then in the JSON Lines output the sink
  wrote (a binary search over the file by raw id, bounded to the bytes flushed when the request
  began); `"emitted_from": "tail" | "output" | null` says which. It stays `null` only when the
  record has not reached the output (stored but not yet emitted, or an output that is a
  device such as `/dev/null`), never because it scrolled out of the ring.
- `?bytes=0` leaves `text` and `hex` `null` and everything else as before; `bytes_len` still
  says how long the record is. A client that reads the bytes from the route below asks for
  this and is spared a JSON body six times the record's size.

- `?values=N` cuts every string value longer than N bytes (at a character boundary) in
  `now.fields`, `now.provenance`, `now.normalized` and `emitted`; a cut entry in `fields` or
  `provenance` carries its full length in `"value_len": u64` (`null` when whole) and the
  top-level `"values_cut": u64` counts the cuts (0: nothing was cut). A 4 MB single-line record
  is one 4 MB `message` value repeated four times in the JSON; with `bytes=0&values=4096` the
  body is kilobytes and the bytes route carries the record once.

`GET /api/events/{raw_id}/bytes` → `application/octet-stream`, the record's exact bytes (what
`ulpf raw <id>` prints), read through the writer's own lock like the JSON route;
`Content-Length` is `bytes_len`. `404 not_found` with the JSON route's error body when the id
was never issued.

## Pivot: the paging cursor, spelled out

The response carries `"next_before_id": u64|null` beside `next_before` (both `null` on the
last page) and the query takes `before_id`, `after`, `after_id` as the prose above says: the
cursor is the pair `(time, raw id)`, so events sharing a millisecond are neither repeated nor
skipped. `related_over` is the number of the entity's newest events (at most 10,000) that
`related` was computed over, and each related value's `events` is how many of those events
the value appeared in, so `events / related_over` is a share and the panel says "in N of the
M newest events". The response also carries `"elapsed_ms": { "header", "timeline",
"related", "lines", "total" }` (each f64), the time each part of the query took, so a slow
pivot names its cause.

## Trust flags (per event): outcomes, not a score

Every emitted line already carries the outcome of every stage for that event; nothing is
computed on the hot path for this section and nothing is added to the line. A screen reads
its flags from these fields:

| flag | from | set when |
|---|---|---|
| `no_parser` | `ulpf.parse_status` | `"no_parser"`: no definition claimed the event |
| `parse_failed` | `ulpf.parse_status` | any value other than `"parsed"` and `"no_parser"`: the failure reason |
| `sub_uncovered` | `ulpf.sub_status` | `"uncovered"`: a message id no sub pattern covers yet |
| `sub_no_match` | `ulpf.sub_status` | `"no_match"`: a gated sub ran and failed |
| `time_from_receipt` | `ulpf.time_policies` | contains `"receipt_fallback"`: no device time was found and `time` is the receipt time |
| `time_error` | `ulpf.time_error` | present: the timestamp text was found but did not parse (its reason) |
| `class_unknown` | `class_uid` | `0`: no class rule matched the fields |
| `unmapped` | `unmapped` | present: the number of its keys is the count of source fields no mapping rule consumed |
| `utf8_lossy` | `ulpf.utf8_lossy` | `true`: the output text is not the exact bytes |

They are the per-event form of the counter block's `no_parser`, `parse_failed`,
`sub_uncovered`, `sub_no_match`, `time_from_receipt`, `time_error`, `class_unknown`,
`unmapped_fields` and `utf8_lossy`: summing a flag over the output equals the counter. They
are not a confidence score and are never shown as one: ULPF reports which stage did not reach
its outcome, never a probability.

## Export: the output file, streamed

`GET /api/export?format=jsonl|csv&from=<raw_id>&to=<raw_id>&q=<terms>` streams the live
output (`--output`, version 1) as it is on disk: from the first line whose raw id is at least
`from` (default: the first line) to the last whose raw id is at most `to` (default: the last
line flushed when the request began). The lines are copied as the sink wrote them, never
re-parsed and never held in memory: the server opens the file read-only, bounded to the length
flushed at the start of the request, so a line the writer is mid-way through is never sent,
and finds `from` by the same binary search the traceback uses. `q` is a space-separated list
of terms; a line is sent when every term occurs in its text, case-insensitive, which is the
rule the Live screen's filter applies to a row, so an export with the screen's terms is
exactly the filtered view.

`format=jsonl` (default) sends the lines verbatim as `application/x-ndjson`. `format=csv`
sends `text/csv` with a header row and the eleven columns the Parquet sink writes (D64:
`raw_id, time, parser, source, class_uid, normalized, src_ip, dst_ip, user, device, dst_port`;
`time` is epoch milliseconds, `normalized` the JSON line itself), RFC 4180 quoting, an empty
field for a value the line does not carry. `Content-Disposition: attachment; filename="<output
stem>-<from>-<to>.<ext>"`. `404 not_found` when the output is a device with nothing to read.
