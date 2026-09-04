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
increments `review_errors`; with inference disabled every review route is `404 not_found`.

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
behind receives at most 200 events per tick and `skipped` counts what the ring evicted
before it was read. Nothing blocks the engine on a slow client.

`TailFrame = { "events": [TailEvent], "skipped": u64, "latest_raw_id": u64|null }`
`TailEvent = { "raw_id": u64, "line": <normalized event object as emitted> }`

## Read-only

`GET /api/status` → `{ "version", "started_at": rfc3339, "listen", "store", "parsers_dir",
"pending_dir", "output", "watch": [dir], "threads", "queue_capacity", "tail_capacity",
"infer_threshold" }`

`GET /api/metrics` → `MetricsFrame`:
```
{
  "engine":    Snapshot,                  // crates/ulpf/src/metrics.rs, verbatim
  "sources":   [ { "name", "events", "detected", "no_parser", "buffered",
                   "last_seen": rfc3339|null, "pending_id": string|null } ],
  "parsers":   [ ParserInfo ],
  "pending_generation": u64,
  "server":    { "sse_clients": u64, "review_errors": u64, "uptime_secs": f64 }
}
```
`Snapshot` includes the v1 counters: `infer_buffered` (lines ever buffered; a source's
`buffered` above is its current buffer), `infer_buffer_full`, `infer_runs`,
`infer_lines_templated`, `infer_lines_unmatched`, `proposals_written`,
`proposals_replaced`, `proposals_skipped` (list of `[reason, n]`: `edited`,
`duplicate`, `rejected`, `no_templates`), `approved`, `rejected`, `reloads`.

`ParserInfo = { "name", "vendor", "product", "priority": i32, "strategy": "kv|delimiter|json|cef|leef|pattern",
"subs": n, "origin": "hand|approved", "detected": u64 }` (`detected` counts events this
process routed to the parser).

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
engine from replacing it with a later proposal for the same source.

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
  "unmatched": { "count": u64, "examples": [string], "by_reason": { "empty": n, "too_long": n, "below_support": n, "no_template": n, "template_cap": n } },
  "decisions": [string],                        // every threshold decision the engine took, in order
  "fingerprint": string
}
```
`verified` is the number of lines the compiled pattern matched first in the second pass,
patterns tried in definition order; `support` is the cluster size that produced it. A
template with `verified` 0, or with `support` below `min_support` after a split, is kept
in the evidence but left out of the definition (its `history` says so); `regenerate`
puts it back. `generated` is the time the pending record was written.

## UI assets

`GET /` → `ui/dist/index.html`; `GET /app.js`, `GET /app.css` → the built assets. Fixed
names so the binary embeds them with `include_str!`. `--ui-dir DIR` serves the three
files from disk on every request instead (restyle without a rebuild).
