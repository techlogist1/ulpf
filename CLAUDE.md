# ULPF — Universal Log Pre-processing Framework

## What this is
A single statically-linked Rust binary (`ulpf`) that ingests logs from perimeter
network devices (firewalls, IDS/IPS, proxies, VPN concentrators, edge routers) in any
vendor format, preserves every original byte immutably and provably, parses each event
using the vendor's own vocabulary, normalizes it into a pragmatic OCSF subset, and
emits JSON Lines. End-to-end throughput (events/sec, ingest through output) is a
first-class output printed at the end of every run. v1 adds the visible half: events no
parser claims are clustered into a candidate parser definition a human reviews and
approves (never parsed until approved), a localhost HTTP API and embedded UI (live
counters, review screen, traceback from any output line to its raw bytes), and a
watch mode (`ulpf serve`) that tails directories and hot-reloads parsers.

Users: a six-person hackathon team. Two people touch Rust. Four work only in the
plain-text folders (`parsers/`, `mappings/`, `samples/`, `fixtures/`, `pending/`,
`ui/src/app.css`) and can never break the build. v0.1 is the invisible foundation; v1
(this repo state) is the inference engine, review workflow, server and UI on top of it.
`PROGRESS.md` opens with the hackathon demo script.

## Stack (each choice verified against crates.io on 2026-09-04)
| Piece | Choice | Why |
|---|---|---|
| Language | Rust 2024 edition, rustc 1.95 | single static binary, zero-copy byte handling without GC |
| Memory mapping | `memmap2` 0.9 | maintained fork of memmap; whole-file map is the zero-copy input path |
| Digest | `sha2` 0.10 | stable `Digest` API; aarch64/x86 SHA intrinsics auto-detected |
| Definition files | `toml` 1.x + `serde` | human-writable, machine-generatable, error spans give path:line |
| Index | `rusqlite` 0.40 `bundled` | bundled SQLite compiles into the static binary; no system lib |
| Regex | `regex` 1.x (`bytes` module) | non-UTF-8 safe; `CaptureLocations` reuse = no per-match allocation |
| Substring search | `memchr` 2.x | SIMD line/delimiter scanning |
| CLI | `clap` 4 derive | standard |
| Queue | `std::sync::mpsc::sync_channel` | bounded, stdlib; no extra crate needed |
| Timestamps | `ulpf-time` (zero deps) | fixed-layout formats only; civil-date math is 30 lines |
| Directory watching | polling, stdlib (D40) | `notify` 9.0.0-rc is a release candidate and misses events across Docker bind mounts |
| HTTP + SSE | `axum` 0.8 + `tokio` 1 (2 worker threads) + `futures-util` | verified against docs.rs 2026-09-05: `/{id}` routes, `Sse::new(stream).keep_alive(..)`, `axum::serve` |
| UI | Svelte 5 + Vite, prebuilt to three fixed files in `ui/dist`, `include_str!` | no node at Rust build or in the container; `--ui-dir` serves edits from disk |
| Sockets | `libc` 0.2 for `SO_RCVBUF` only | std has no receive-buffer setter; the kernel's grant is reported in `/api/status` |
| Parquet | `parquet` 59.3.0, default features off, `snap` | 27 deps, no thrift or arrow crate, static build unchanged, +320 KiB; an additional sink (D64) |

## The three shapes (data model)
Every event exists in exactly three forms:
1. **RawEvent** — original bytes untouched (including line terminators), receipt
   timestamp, source id, SHA-256, permanent monotonic id. Written to the append-only
   store *before* format detection runs. Crate: `ulpf-store`.
2. **ParsedEvent** — key/value pairs using the device's own field names, plus one
   typed timestamp (vendor-format → epoch nanos + explicit policy flags + original
   string). Carries the raw id. Knows nothing about any output schema. Crate: `ulpf-parse`.
3. **NormalizedEvent** — the same information under schema field names with
   canonicalised values (deny/denied/drop → one value). Carries the raw id.
   Crate: `ulpf-normalize`.

Supporting entities: `ParserDefinition` (matcher + strategy + fields; one per device
family; identical format hand-written or generated; `[parser] version` and
`origin = "inferred"` when the engine wrote it), `FieldMapping` (source field →
schema field + value canonicalisation; one per output schema, shared by all parsers;
`[entities]` names the five pivot kinds in schema paths),
`Template` (constant tokens + typed slots + optional groups `{? ...}`; the inference
engine's product; converts to a `ParserDefinition` losslessly), `Proposal` (a
`ParserDefinition` plus `Evidence`: templates with support/verified counts, slots with
examples, unmatched lines by reason, the decision log; crate `ulpf-infer`, written to
`pending/` by the engine, never loaded by the registry).

## The parser/mapping wall (non-negotiable)
Parsing (1→2) and normalization (2→3) are separate stages with a hard boundary.
- A parser definition cannot reference the output schema. A mapping cannot reference
  any vendor or parser.
- Enforced structurally: `ulpf-parse` has no dependency on `ulpf-normalize`; the
  definition and mapping TOML types use `deny_unknown_fields`, and neither type has a
  field where the other side's vocabulary could go. Mappings receive only
  `(field name, value)` pairs. `ulpf-infer` depends on `ulpf-parse` only, so an inferred
  slot cannot be named after a schema field by construction; inferred names are the
  device's own keys (`src-mac`, `IN`) or `kind+n`.
- Why: parser knowledge is per-device and grows with every vendor; schema knowledge is
  per-standard and changes rarely but globally. Fusing them means a second output
  schema rewrites every parser.
- If you want a parser to know an OCSF field name to make something easier, the
  mapping stage is incomplete. Fix the mapping stage.

## Other invariants
- **Raw before understanding.** The store API is append + read. No update, no delete,
  no handle to an existing record. Immutability is a property of the interface, and one
  writer at a time is enforced (a second `ulpf run` on the same store is refused). The
  server reads records through the writer's own handle (D42); it never opens the store.
  Every record carries a chain value in the index (`chain_i = sha256(chain_{i-1} ||
  digest_i)`, genesis from a random store id); `ulpf verify` names the first record a
  rewrite touched and `ulpf attest` exports checkpoints a stranger re-verifies offline
  (D56). Replay and verify read a bounded snapshot the writer flushed (`RawStore::reader`);
  the store is never written by them (D52).
- **One sequencer.** Files, UDP and TCP are three producers on one bounded queue; the
  batch sequence is taken inside the store lock where the ids are issued, so the output
  thread's order is the id order whatever the producer mix (D60). A restart completes an
  interrupted output from the store before ingesting anything new (D59).
- **Nothing is parsed on a proposal.** The registry loads `parsers/` only. A proposal is
  three files in `pending/`; `Pending::approve` is the only code that writes to
  `parsers/`, and it validates, refuses name collisions, writes atomically and reloads.
  Generated parsers carry `priority = -1`, so a hand-written parser always wins (D45).
- **The server owns no state.** `Live` (`crates/ulpf/src/engine.rs`) is the one shared
  object; the server holds an `Arc<Live>`, a 200 ms frame cache and per-client stream
  positions. If a feature needs server-side state, it belongs in `Live` (D41).
- **Zero-copy hot path.** Inputs are memory-mapped. Parsed fields are byte ranges into
  the map (`Cow::Borrowed`); text is materialised only at output. Digests are computed
  from the mapped bytes. Throughput is measured from the first CLI run so any
  allocation creeping in is visible as a regression. v1 adds work per batch (one store
  lock, one pipeline read, one source-stats lock), never per event; the unknown path
  copies the event into the inference buffer (bounded, counted), the serving path
  allocates in the server (D43, D49).
- **Bounded backpressure.** The ingest→process queue is a `sync_channel` of fixed
  capacity. Saturation policy: the producer blocks. Dropping is never acceptable
  because the raw store must be complete.
- **Errors as values.** Unparseable event, unmappable field, unknown format: each is
  a counted pipeline outcome that still reaches the output. Never a panic, never a
  silent drop.
- **Counters on screen.** Per-stage counts (framed, stored, detected, parsed,
  normalized, emitted), error counts by reason, queue high-water mark and throughput
  are printed at the end of every run. The server session exposes the same struct.
- **Zero outbound network calls at runtime.** Build-time crates.io fetches only. `serve`
  listens on `127.0.0.1` unless told otherwise and never connects out;
  `scripts/isolation.sh` proves it by sampling the process's sockets (run, serve and
  `--network none` docker modes).

## Plain-text folder contract (for teammates)
- `parsers/*.toml` — one parser definition per device family. Loaded by directory scan
  at startup. A malformed file is reported with path and line; the others still load.
  v0.1 ships 12: cisco_asa, cisco_ios, fortinet_fortigate, openvpn, palo_alto_panos,
  pfsense_filterlog, check_point, juniper_srx, sonicwall, sophos_xg, squid_access,
  suricata_eve. Every one was written from the vendor's log reference, not from a
  worker's memory; a fixture that passes only proves the code is self-consistent.
- `mappings/*.toml` — one per output schema (`ocsf.toml`). Same loading rules.
- `samples/<parser>.log` — paired sample for each parser. Synthetic until real samples
  arrive (see `samples/README.md`).
- `fixtures/<parser>.expected.jsonl` — expected parsed fields and normalized subset per
  sample event. The fixture test in `crates/ulpf/tests/` runs every pair.
- `pending/<id>.toml` + `.json` + `.lines` — one proposal per unknown source, written by
  the engine, edited by anyone, activated only by approval (UI, API or moving the file by
  hand into `parsers/`). `ulpf check --pending pending` validates them. Not committed.
- `heldout/*.log` + `.truth.tsv` — inference test inputs with ground truth; never loaded
  by `run`. Grade a change with `cargo run -p ulpf-infer --example infer -- heldout/X.log`.
- `corpus/real/<vendor>/`, `corpus/generated/<tool>/` — real captures (public sources,
  licence read and named in PROVENANCE.md) and captures generated locally from real
  tools in Docker (SETUP.md re-runs them in under five minutes); `corpus/README.md` is
  the index. nginx, HAProxy, Zeek and OpenVPN 2.6 are the unseen formats for the live
  inference demo.
- `eval/` + `docs/evaluation.md` — the neutral harness any tool runs through
  (`eval/run.sh eval/tools/<tool>.toml`); ULPF's generated scorecard is committed under
  `eval/results/`, raw result trees are not.
- `mappings/ecs.toml` — the second output schema (`--schema ecs`); it exists to prove the
  wall: the branch that added it touched mappings and one test file.
- `ui/src/app.css` — every colour and spacing token at the top; `ulpf serve --ui-dir
  ui/dist` after `pnpm build` in `ui/`, no Rust rebuild.
- Validate without a rebuild: `cargo run -- check` (or the built binary `ulpf check`).
- Full format reference: `docs/parser-format.md`; HTTP contract: `docs/api.md`.

## Coding standards
- Bytes, not strings, on the hot path (`&[u8]`, `regex::bytes`). UTF-8 is a property
  of the output, not the input.
- No allocation inside per-event parse/detect code paths. Materialise at output. The
  documented exceptions: JSON values, a quoted value that needs unescaping, a sub on such a
  materialised value, and `column_N` names for columns beyond the named ones.
  `crates/ulpf-parse/tests/alloc.rs` counts allocations and fails on any other.
- Errors that can be defined out of existence are. What remains is an enum reason
  counted in `Metrics`, never a `panic!`/`unwrap()` on input data.
- Rust warnings are errors in CI mindset: the build must be clean.
- No doc-comment essays. Interface comments say what and why; implementation comments
  only where the reason is non-obvious.
- Commit messages say what now works. Every commit builds and passes `cargo test`.
- `pnpm` is irrelevant here; this is pure Rust. No Python at runtime.

## Drift, replay, pivot (what v2 added around the engine)
A source with an established parser (1,024 baseline events, under 20% misses) trips when
a 512-event window, or a partial window after 5 s of quiet, misses 0.25 above its baseline
with at least 32 misses; its misses then feed inference with the parser as prior and the
update lands in `pending/` as a versioned proposal with a diff (D54). Replay re-runs the
store through the current parsers into `out.vN.jsonl` and diffs against the previous
version, naming every parser or mapping file whose digest changed (D52). The pivot index
beside the output (`out.jsonl.pivot`) answers one entity across every device (D55).

## Inference (how the engine proposes a parser)
Unknown events are copied into a per-source buffer (bounded at 4096, ordered by raw id).
At the threshold (`--infer-threshold`, 64), at each doubling, and when a source goes
quiet, `ulpf_infer::infer` tokenizes with the slot regexes, clusters by word similarity,
aligns members with gap and substitution costs, derives optional groups from presence,
types slots by value family, splits clusters on keyword slots so dispositions stay
constant, and compiles every pattern through the real parser to verify it. The
proposal's `Evidence` records every decision. Thresholds and their alternatives: D46.
The kill criterion in the brief did not fire: the four held-out files grade at 14/14,
9/10 and 1 template(s) per format with every line covered, and a messy file isolates its
junk by reason. Tune against `heldout/` and record the grades before changing a threshold.
A name the input carries wins over the type (D68): in a line that opens with `{` the JSON keys
are constant tokens and each value slot is named by its key (reason `json key`); a `#fields`
header among the buffered lines names delimited columns by position (reason `header`); the
generated `[[timestamp]]` follows the timestamp slot's own name (`ts`).

## CLI (what exists)
```
ulpf run <files|dirs>... --store DIR --output FILE.jsonl [--parsers parsers] [--mappings mappings]
         [--schema ocsf] [--tz +05:30] [-j THREADS] [--batch 1024] [--queue 64]
         [--pending pending] [--infer-threshold 64] [--report-json report.json]
ulpf serve <dirs>... --store DIR --output FILE.jsonl [--listen 127.0.0.1:7878] [--pending pending]
         [--infer-threshold 64] [--tail 1000] [--poll-ms 250] [--ui-dir ui/dist]
         [--syslog-udp 127.0.0.1:5514] [--syslog-tcp 127.0.0.1:5514] + the run options
ulpf replay --store DIR --output FILE.jsonl [--schema ecs] [--report-json r.json]
         # every stored record through the current parsers into FILE.vN.jsonl, diff against v(N-1)
ulpf pivot KIND VALUE --output FILE.jsonl [--limit N]   # one entity's timeline from the index beside the output
ulpf pivot --rebuild --output FILE.jsonl --mappings mappings --schema ocsf
ulpf infer FILE [--pending pending] [--parsers parsers] [--decisions]   # offline proposal for one file
ulpf check [--pending pending]  # load every parser, mapping and pending file, report path:line problems
ulpf verify --store DIR [--attestation FILE]   # every digest and chain link; names the first bad record
ulpf attest --store DIR [--out FILE]           # the attestation a stranger re-verifies offline
ulpf raw <ID> --store DIR       # exact bytes of one raw record (header on stderr)
ulpf fixture samples/x.log      # fixture skeleton for review (never commit blind)
```
`run` and `serve` take `--receipt <RFC3339>` to pin the receipt time (reproducible output),
`--schema ocsf|ecs`, `--pivot on|off` (the entity index: on by default in `serve`, off in `run`,
D66) and `--parquet FILE` (an additional sink, D64). A restart over the same input and store resumes where the store ends
and completes the output from the store first, and ctrl-c during a large drop returns at
the next batch boundary rather than draining the file (D59); a store written before the integrity
chain is refused by name (delete it). Every `run`/`replay` output has `FILE.vN.meta.json`
beside it and an entity index `FILE.pivot`.
Every `run` ends with the counter block: files, bytes, events/s, MB/s; per-stage counts
(framed, stored, detected, no_parser, parsed, parse_failed by reason, normalized, emitted);
signals (sub_matched, sub_no_match, sub_uncovered, time_from_receipt, time_error by reason,
class_unknown, enum_other, unmapped_fields, utf8_lossy); queue batches, high-water and
backpressure blocks (times the ingest thread found the queue full); inference (buffered,
buffer full, runs, lines templated/unmatched, proposals written/replaced/skipped by
reason, approved, rejected, reloads); drift (tripped, lines routed, update proposals,
cleared); syslog (udp datagrams/bytes, tcp connections/events/bytes/partial/refused,
errors); a `recovered: N` line when a restart completed an interrupted output; then the
pending proposals awaiting review. The same numbers are `engine` in `GET /api/metrics`
and the `metrics` SSE event.
When output looks plausible but wrong, read that block first: `no_parser` means the format
was not recognised, `sub_uncovered` means a message id has no pattern yet, `sub_no_match`
means a gated sub ran and failed (or an ungated sub met a message you have not modelled),
`time_from_receipt` means the device time was not found,
`class_unknown` means no class rule matched the fields. `ulpf raw <id>` shows the exact
input for any output line (`ulpf.raw_id`).

## Working files
- `PROGRESS.md` — hackathon demo script first, then the v3, v2, v1 and v0.1 records (definition of
  done, fan-outs, verified state, tried and abandoned, next action).
- `docs/DECISIONS.md` — every structural decision with anchor file and the alternative it ruled out (D1-D74).
- `docs/api.md` — the HTTP and SSE contract the server and UI are built against.
- `docs/parser-format.md` — the definition format reference for teammates.
- `docs/timestamps.md` — timestamp survey, auto-detection order, zone table, policies.
- `docs/inference-prototype-report.md` — the prefix-tree trial, and the v1 engine's graded results on `heldout/`.
- `docs/slot-vocabulary.md` — the curated naming vocabulary the inference engine compiles in (D53).
- `docs/retention.md` — how segment rotation and retention would work without weakening
  append-only, the single writer, permanent ids or the chain; a design note, not built.
- `docs/evaluation.md` — the scorecard and the 04:00 procedure.
- `docs/design.md` — the UI's tokens, type and spacing scales, colour semantics with the contrast
  table, component inventory and keyboard map (D69-D71); `docs/screens/` the captures.
- `app/` — the Tauri 2 desktop shell around the unchanged binary (D73), its own workspace; CI in
  `.github/workflows/app.yml` bundles macOS and Windows installers (D74).
