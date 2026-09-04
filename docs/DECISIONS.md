# ULPF decisions record

Format per entry: **Decision** · **Anchor** (the file that embodies it) · **Principle** ·
**Ruled out** (the specific alternative and why). A principle with no decision is noise; a decision with no alternative was not
decided.

## D1. Crate boundaries: time / store / parse / normalize / cli
**Decision.** Five crates: `ulpf-time` (calendar + format parsing, zero deps),
`ulpf-store` (framing, append-only raw store, offsets index, SQLite catalogue),
`ulpf-parse` (definitions, four strategies, detection, Template), `ulpf-normalize`
(mappings, OCSF subset), `ulpf` (engine, metrics, CLI). Dependency direction:
cli → {store, parse, normalize, time}; normalize → parse (for the `ParsedEvent` type
only); parse → time. `parse` never depends on `normalize`.
**Anchor.** `Cargo.toml` (workspace members) and each `crates/*/Cargo.toml` `[dependencies]`.
**Principle.** Information hiding; the parser/mapping wall enforced by the dependency
graph rather than by review. Sub-agent isolation: a worker owning `ulpf-time` cannot
touch the parser runtime.
**Ruled out.** A `ulpf-core` crate for shared types (would hold three structs and no
behaviour — a crate for a stub). Inference and server crates now (nothing exists to put
in them yet).

## D2. Repository root is the working directory
**Decision.** The repo is initialised in `…/ssh hackathon` itself and named `ulpf` on
GitHub. **Anchor.** `.git/` at `…/ssh hackathon`, `CLAUDE.md` at the same level.
**Principle.** Fresh Claude Code sessions in this directory auto-load
`CLAUDE.md`. **Ruled out.** A nested `ulpf/` folder (extra `cd` in every session, no
benefit).

## D3. Queue: `std::sync::mpsc::sync_channel`, block-on-full
**Decision.** Ingest→process is a bounded `sync_channel` of batches; when full the
ingest thread blocks. **Anchor.** `crates/ulpf/src/engine.rs` (`QUEUE_CAPACITY`, `sync_channel`).
**Principle.** Pull complexity downward: the one policy that never
loses data is chosen by the engine, not configured by the operator. **Ruled out.**
`crossbeam-channel` (a dependency for a feature stdlib has); drop-on-full (violates raw
completeness); unbounded (grows until the process dies under load).

## D4. Repo hygiene: lead commits, workers never commit
**Decision.** Only the lead runs git. Workers use a private `CARGO_TARGET_DIR` to avoid
build-lock contention. **Anchor.** `PROGRESS.md` (fan-out sections), `.gitignore` (`/target-*`).
**Principle.** One writer per shared resource. **Ruled out.**
Worker commits (interleaved half-states on `main`).

## D5. Raw store layout: segment + offsets file + SQLite catalogue (never per-event rows)
**Decision.** `raw.seg` holds self-describing records (magic, id, receipt ns, source, len,
sha256, bytes); `raw.idx` holds one u64 offset per id; `catalog.sqlite` holds `sources`,
`ingests` (one row per file/stream) and `runs` (one row per CLI run with its counter report).
**Anchor.** `crates/ulpf-store/src/store.rs`.
**Principle.** Observability as a design input (the catalogue is what the server queries),
information hiding (callers see `append`/`get`, never files), pull complexity downward
(record framing and recovery live in one place).
**Ruled out.** A SQLite row per event: ~1 µs per insert even batched caps the ingest thread
near 1M events/s, below the parser stage; the offsets file gives O(1) id lookup at 8 bytes
per event and zero SQL on the hot path.

## D6. Framing rule: a line plus following lines that start with space/tab/CR/LF
**Decision.** Format-agnostic continuation by indentation or blankness; terminators are kept
inside the event so concatenated records reproduce the file. Chunked input is supported by
withholding any event whose end is undecidable until the next line's first byte arrives.
**Anchor.** `crates/ulpf-store/src/frame.rs`; proof in `crates/ulpf-store/tests/roundtrip.rs`.
**Principle.** Raw before understanding: framing runs before any parser exists, so it may
not depend on vendor knowledge. Define errors out of existence: nothing is "unframeable" —
an empty file yields zero events, a final unterminated line is an event, a 5 MiB line is
an event.
**Ruled out.** Parser-driven multi-line rules (would require detection before storage);
dropping blank lines (breaks lossless concatenation).

## D7. Crash recovery: the index is authoritative; unindexed tail bytes are reclaimed
**Decision.** On open the writer resumes at the end of the last indexed record; a torn
index entry is ignored. No record that was ever handed out as a `RawId` is touched.
**Anchor.** `RawStore::open` in `crates/ulpf-store/src/store.rs`.
**Principle.** Immutability as a property of the interface: the only mutation is to bytes
that were never a record. **Ruled out.** Write-ahead journaling (a second file format for a
case a fixed-width index already resolves); refusing to open a torn store (operator
intervention at 4am for a routine crash).

## D8. No-year timestamps take the receipt year in the resolved zone, with a 7-day rollover
**Decision.** A timestamp with no year (BSD syslog, Cisco IOS default, Squid-relayed ASA)
takes the year of the receipt time *after converting receipt to the timestamp's resolved
offset*; if that instant lands more than 7 days after receipt, the previous year is used.
A Feb 29 the receipt year lacks also falls back to the previous year. Result flagged
`year_assumed`. **Anchor.** `crates/ulpf-time/src/lib.rs` (`resolve`, `YEAR_ROLLOVER_SLACK`).
**Principle.** Define the error out of existence with a deterministic rule the engine owns,
and expose the assumption as data (a flag), not a log line; receipt time (not wall clock)
keeps re-runs reproducible. **Ruled out.** Receipt year unconditionally (December logs
replayed in January land a year late). Rejecting no-year input (rejects the single most
common perimeter format). Wall-clock "now" (non-reproducible across replays).

## D9. No zone means `Context::default_offset_secs`, a fixed offset, never a region
**Decision.** A timestamp with no zone information gets the caller's fixed default offset
and the flag `tz_assumed`. The context carries an offset in seconds, not a zone name.
**Anchor.** `crates/ulpf-time/src/lib.rs` (`resolve`, `Zone::None`, `Context`).
**Principle.** A fixed offset is arithmetic; a region is a database (tzdata) and therefore a
dependency plus DST rules that change. The operator sets one offset per source. **Ruled
out.** Silently treating no-zone as UTC (wrong for every on-prem device and invisible
afterwards). Shipping tzdata/`chrono-tz` (a dependency for a static binary, and DST
guessing produces confidently wrong instants around transitions).

## D10. Zone abbreviations: fixed table, documented picks for ambiguous names, flags for both
**Decision.** ~50 abbreviations map to fixed offsets (table in `docs/timestamps.md`).
Unambiguous names apply with no flag. IST, CST, CDT, BST, AST, GST apply the documented
pick (India, US Central, US Central, British Summer, Atlantic, Gulf) and flag
`zone_name_ambiguous`. Unknown names apply the default offset and flag `zone_name_unknown`.
Names are 1–5 letters; a longer alphabetic tail (a hostname) is `no_match`, not a zone.
**Anchor.** `crates/ulpf-time/src/lib.rs` (`ZONES`, `Cur::zone_name`, `resolve`).
**Principle.** Abbreviations are not identifiers; the parser records what it guessed rather
than pretending certainty, so a later stage can weight or override. **Ruled out.**
Rejecting ambiguous names (drops the timestamp for the Sophos/Cisco sites most likely to
use IST/CST). Guessing by proximity to the default offset (unexplainable results).
A per-source override table (belongs in the caller's Context if a real sample needs it).

## D11. Fractional seconds beyond nine digits are truncated, not rejected
**Decision.** Any number of fraction digits is accepted; digit ten onward is dropped. Epoch
inputs likewise truncate below the unit's nanosecond resolution.
**Anchor.** `crates/ulpf-time/src/lib.rs` (`Cur::fraction`, `epoch`).
**Principle.** Precision beyond the output type is not an error in the input. **Ruled out.**
Rejecting (a log tool that drops an event over a tenth decimal). Rounding (can carry into
the next second and needs a renormalisation pass for nothing).

## D12. Range: civil year 1970..=9999, epoch nanos in i64, impossible dates are `out_of_range`
**Decision.** Year < 1970 or > 9999, invalid day-of-month (Feb 30, Feb 29 in a non-leap
year), hour 24, month 13, and any instant past 2262-04-11T23:47:16Z (i64 nanos) return
`TimeError::OutOfRange`, distinct from `NoMatch`. Second 60 is accepted and rolls over.
A syntactic match with an impossible date is never retried under another format.
**Anchor.** `crates/ulpf-time/src/lib.rs` (`epoch_from_civil`, `resolve`, `TimeError`).
**Principle.** Errors as values with a distinct reason: `out_of_range` counts separately in
Metrics, so a device with a broken clock shows up as its own number. **Ruled out.**
Clamping Feb 30 to Mar 1 like C `mktime` (fabricates an instant and hides the device bug).
i128 or a (secs, nanos) pair for post-2262 (no perimeter log carries such a year; i64 is
what the rest of the pipeline stores and what `epoch_millis` needs).

## D13. Parser definitions are TOML with one flat `Strategy` struct for `[strategy]` and `[[sub]]`
**Decision.** TOML, `deny_unknown_fields` on every table. A single flat `Strategy` struct
(`kind` plus every strategy's optional keys) serves both the top-level strategy and each
sub-parser; keys that do not belong to the `kind`, and sub-only keys at top level, are
rejected when the definition compiles. **Anchor.** `crates/ulpf-parse/src/def.rs`
(`Strategy`, `Strategy::validate`). **Principle.** Structural prevention over
documentation: a typo or an output-schema key cannot load. Consistency: a sub is written
exactly like a strategy. **Ruled out.** An internally-tagged enum with `#[serde(flatten)]`
in the sub table — serde cannot combine `flatten` with `deny_unknown_fields`, so either
typos pass silently or subs need a nested `[sub.strategy]` table humans would hate. YAML
or JSON definitions (JSON has no comments; YAML's indentation is a 3am foot-gun and
machine emission needs a second serializer).

## D14. Pattern strategy: `{name:type}` slots compiled to one bytes regex; `Template` is the single syntax authority
**Decision.** Patterns are constant text with typed slots; `Template::from_pattern` /
`to_pattern` is a bijection, and the compiler goes through `Template`, so anything the
inference engine can represent is loadable by construction. A raw `regex` key remains as
an escape hatch. Spaces in constants match runs of spaces/tabs. **Anchor.**
`crates/ulpf-parse/src/template.rs`, `crates/ulpf-parse/src/pattern.rs`; proof in
`crates/ulpf-parse/tests/roundtrip.rs`. **Principle.** Deep module: one syntax, one
compiler, human- and machine-writable. **Ruled out.** Raw regex as the primary format
(humans misplace escapes; inference would need a regex-to-template parser for the UI);
exact whitespace matching (Cisco ASA's documented `server =  10.0.0.2` double space breaks
it).

## D15. Parsed fields are `Cow<'a, [u8]>` borrowed from the event; per-thread `Scratch` holds regex capture buffers
**Decision.** `Parsed` holds `Cow` key/value pairs referencing the memory-mapped event
(or the definition, for constants). Only unescaping and JSON flattening allocate. Regex
`CaptureLocations` live in a per-thread `Scratch`, grown on first use. **Anchor.**
`crates/ulpf-parse/src/lib.rs` (`Field`, `Parsed`), `crates/ulpf-parse/src/compile.rs`
(`Scratch`). **Principle.** Zero-copy hot path as a design constraint, measured by the
CLI's throughput number. **Ruled out.** Owned `String` fields (an allocation per field
per event); a custom span arena with three source tags (more code for the same borrow
the compiler already checks).

## D16. Syslog envelope is a lenient, format-agnostic pre-step, not a strategy
**Decision.** `[envelope] syslog = true` strips `<pri>` and a 3164/5424 header with every
part optional, emitting `syslog_*` fields; the hostname is consumed only after a
timestamp was found, so Fortinet's `date=` body is left whole. **Anchor.**
`crates/ulpf-parse/src/envelope.rs`. **Principle.** Pull complexity downward: relays add
and strip headers unpredictably; every definition would otherwise re-encode the same
optional header regex. **Ruled out.** Requiring authors to include the header in their
pattern (twelve copies of one regex); strict RFC parsing (rejects the ASA `host : %ASA`
form and headerless lines that real relays produce).

## D17. Sub-parsers gated by `when`, first match wins, non-match is a counter not an error
**Decision.** `[[sub]]` runs a strategy on one field when the listed gate fields match;
the first matching sub adds its `constants`. Eligible-but-unmatched sets `SubStatus::NoMatch`,
counted by the engine; the event is still emitted with its top-level fields. **Anchor.**
`Parser::run_subs` in `crates/ulpf-parse/src/compile.rs`. **Principle.** Define errors out
of existence: an unseen message shape is data about the device, not a failure of the
event. Observability: `sub_no_match` on screen is the prompt to write the next pattern.
**Ruled out.** One definition file per message id (Cisco ASA alone has hundreds; the
matcher would run hundreds of times per event); slot types that recurse into strategies
(`{body:kv}`), which is the same power with a less explicit gate.

## D18. Timestamp extraction belongs to the parser definition, not the mapping
**Decision.** `[[timestamp]]` candidates (field or joined fields plus a format) live in
the parser file; `Parsed` carries one typed timestamp with policy flags and the original
text; the syslog header time is the automatic fallback. **Anchor.**
`Parser::resolve_timestamp` in `crates/ulpf-parse/src/compile.rs`. **Principle.** Knowing
that FortiGate writes `date`+`time` with a separate `tz` is vendor knowledge; the mapping
stage receives an instant and stays vendor-free. **Ruled out.** Mapping-side time
extraction (every vendor's layout would leak into the schema file); receipt time only
(loses the device clock, which is the whole point of a timestamp module).

## D19. Engine: one ingest thread, N workers, one ordered output thread, batches of byte ranges
**Decision.** Ingest frames the memory-mapped file, appends each event to the raw store, and
sends batches of `(first_raw_id, ranges)` over a bounded `sync_channel`; workers run the
shared `Pipeline::process`; the output thread reorders by batch sequence so JSON Lines order
equals raw id order. Receipt time is taken once per batch. **Anchor.**
`crates/ulpf/src/engine.rs`, `crates/ulpf/src/pipeline.rs`. **Principle.** Raw before
understanding (append happens on the ingest thread before any worker sees the event);
deep module (`Pipeline::process` is the single per-event path, so the fixture harness tests
production code). **Ruled out.** Per-event channel messages (contention at ~1M/s); per-file
worker parallelism (a single 2 GB file would use one core); unordered output (raw id N no
longer sits on output line N, which is the 4am debugging primitive).

## D20. Unknown-format events are emitted as Base Event with the text under `message`
**Decision.** When no parser matches, the engine pushes one synthetic field `raw_message`
and normalizes as usual; the line carries `parse_status: no_parser`, class 0. **Anchor.**
`Pipeline::process` in `crates/ulpf/src/pipeline.rs`; counter `no_parser` in
`crates/ulpf/src/metrics.rs`. **Principle.** Errors as values: the event is preserved,
counted, traceable and visible in the same output stream. **Ruled out.** Dropping with a
counter (invisible in the output); a separate rejects file (a second output to correlate).

## D21. Fixtures are subset assertions with a fixed receipt time, generated then reviewed
**Decision.** `fixtures/<parser>.expected.jsonl` asserts only the keys it lists; the
harness runs the production pipeline with a fixed receipt time and reports every mismatch
as file:line. `ulpf fixture` emits a skeleton to review, never to commit blind. **Anchor.**
`crates/ulpf/src/fixture.rs`, `crates/ulpf/tests/fixtures.rs`, `fixtures/README.md`.
**Principle.** Teamability without touching Rust; observability (all mismatches at once).
**Ruled out.** Full-snapshot comparison (every mapping improvement breaks every fixture);
Rust unit tests per parser (teammates cannot write them).
