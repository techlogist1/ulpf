# ULPF — Universal Log Pre-processing Framework

## What this is
A single statically-linked Rust binary (`ulpf`) that ingests logs from perimeter
network devices (firewalls, IDS/IPS, proxies, VPN concentrators, edge routers) in any
vendor format, preserves every original byte immutably and provably, parses each event
using the vendor's own vocabulary, normalizes it into a pragmatic OCSF subset, and
emits JSON Lines. End-to-end throughput (events/sec, ingest through output) is a
first-class output printed at the end of every run.

Users: a six-person hackathon team. Two people touch Rust. Four work only in the
plain-text folders (`parsers/`, `mappings/`, `samples/`, `fixtures/`) and can never
break the build. v0.1 (this repo state) is the invisible foundation; the parser
inference engine and web UI are built on top of it in later sessions.

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
| Directory watching | none in v0.1 | `notify` 9.0.0-rc verified as candidate for the server session |

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
family; identical format hand-written or generated), `FieldMapping` (source field →
schema field + value canonicalisation; one per output schema, shared by all parsers),
`Template` (constant tokens + typed slots; the inference engine's product; converts
to a `ParserDefinition` losslessly).

## The parser/mapping wall (non-negotiable)
Parsing (1→2) and normalization (2→3) are separate stages with a hard boundary.
- A parser definition cannot reference the output schema. A mapping cannot reference
  any vendor or parser.
- Enforced structurally: `ulpf-parse` has no dependency on `ulpf-normalize`; the
  definition and mapping TOML types use `deny_unknown_fields`, and neither type has a
  field where the other side's vocabulary could go. Mappings receive only
  `(field name, value)` pairs.
- Why: parser knowledge is per-device and grows with every vendor; schema knowledge is
  per-standard and changes rarely but globally. Fusing them means a second output
  schema rewrites every parser.
- If you want a parser to know an OCSF field name to make something easier, the
  mapping stage is incomplete. Fix the mapping stage.

## Other invariants
- **Raw before understanding.** The store API is append + read. No update, no delete,
  no handle to an existing record. Immutability is a property of the interface, and one
  writer at a time is enforced (a second `ulpf run` on the same store is refused).
- **Zero-copy hot path.** Inputs are memory-mapped. Parsed fields are byte ranges into
  the map (`Cow::Borrowed`); text is materialised only at output. Digests are computed
  from the mapped bytes. Throughput is measured from the first CLI run so any
  allocation creeping in is visible as a regression.
- **Bounded backpressure.** The ingest→process queue is a `sync_channel` of fixed
  capacity. Saturation policy: the producer blocks. Dropping is never acceptable
  because the raw store must be complete.
- **Errors as values.** Unparseable event, unmappable field, unknown format: each is
  a counted pipeline outcome that still reaches the output. Never a panic, never a
  silent drop.
- **Counters on screen.** Per-stage counts (framed, stored, detected, parsed,
  normalized, emitted), error counts by reason, queue high-water mark and throughput
  are printed at the end of every run. The server session exposes the same struct.
- **Zero outbound network calls at runtime.** Build-time crates.io fetches only.

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
- Validate without a rebuild: `cargo run -- check` (or the built binary `ulpf check`).
- Full format reference: `docs/parser-format.md`.

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

## Verify before building the server (next session)
- The current `axum` API and its server-sent events support must be verified from the
  docs before writing any handler; do not trust recalled signatures.
- Svelte 5 + Vite embedded into the binary has a 45-minute abandon timer: if the embed
  is not serving in 45 minutes, ship plain HTML from `include_str!`.
- The `frontend-design` skill was not needed in v0.1; load it for the UI session.
- `notify` 9.x for directory watching: verify the API; v0.1 scans directories once.
- `Metrics` in `crates/ulpf/src/metrics.rs` is the struct to expose over the wire.

## CLI (what exists)
```
ulpf run <files|dirs>... --store DIR --output FILE.jsonl [--parsers parsers] [--mappings mappings]
         [--schema ocsf] [--tz +05:30] [-j THREADS] [--batch 1024] [--queue 64] [--report-json report.json]
ulpf check                 # load every parser and mapping file, report path:line problems
ulpf verify --store DIR    # recompute every SHA-256 in the raw store
ulpf raw <ID> --store DIR  # exact bytes of one raw record (header on stderr)
ulpf fixture samples/x.log # fixture skeleton for review (never commit blind)
```
Every `run` ends with the counter block: files, bytes, events/s, MB/s; per-stage counts
(framed, stored, detected, no_parser, parsed, parse_failed by reason, normalized, emitted);
signals (sub_matched, sub_no_match, sub_uncovered, time_from_receipt, time_error by reason,
class_unknown, enum_other, unmapped_fields, utf8_lossy); queue batches, high-water and
backpressure blocks (times the ingest thread found the queue full).
When output looks plausible but wrong, read that block first: `no_parser` means the format
was not recognised, `sub_uncovered` means a message id has no pattern yet, `sub_no_match`
means a gated sub ran and failed (or an ungated sub met a message you have not modelled),
`time_from_receipt` means the device time was not found,
`class_unknown` means no class rule matched the fields. `ulpf raw <id>` shows the exact
input for any output line (`ulpf.raw_id`).

## Working files
- `PROGRESS.md` — checklist, verified state, in-flight work, next action.
- `docs/DECISIONS.md` — every structural decision with anchor file and the alternative it ruled out.
- `docs/parser-format.md` — the definition format reference for teammates.
- `docs/timestamps.md` — timestamp survey, auto-detection order, zone table, policies.
- `docs/inference-prototype-report.md` — honest result of the prefix-tree clustering trial.
