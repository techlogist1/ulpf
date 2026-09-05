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
ingest thread blocks. **Anchor.** `crates/ulpf/src/engine.rs` (`Config::queue_batches`, `queue_cap`, `sync_channel`; the
operator sets the size with `--queue`, the engine owns the policy that a full queue blocks).
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

## D22. Sub-parser outcomes are four-valued; `uncovered` is its own counter
**Decision.** `SubStatus` distinguishes `not_applicable` (definition has no subs),
`matched`, `no_match` (a gate matched, no pattern did) and `uncovered` (subs exist, none
gated for this event). The engine counts the last two separately. Found by the adversarial
pass: an ASA message id with no sub read as "not applicable", indistinguishable from a
parser that never had subs. **Anchor.** `SubStatus` in `crates/ulpf-parse/src/compile.rs`;
`sub_uncovered` in `crates/ulpf/src/metrics.rs`. **Principle.** Observability as a design
input: the two causes need two different fixes (a pattern bug versus a missing message id),
so they must be two numbers on screen. **Ruled out.** Folding both into `no_match` (the
operator cannot tell which definition work to do).

## D23. Backpressure is measured at the channel, the depth is clamped, and the BOM is stripped by the envelope
**Decision.** The ingest thread counts a batch in flight before offering it, records the
high-water clamped to the queue capacity, and measures backpressure directly: a
`try_send` that reports the queue full increments `backpressure_blocks` before the
blocking `send`. "Engaged" on screen means that counter is non-zero. A leading UTF-8
byte-order mark is removed before `<pri>` detection. The first version of this decision
counted after the send and claimed the depth "never exceeds capacity"; the 2026-09-05
invariant review reproduced `high-water 2/1` under `--queue 1 -j 8`, because a worker's
decrement lands after its receive frees the slot. **Anchor.** `send_batch` in
`crates/ulpf/src/engine.rs`; `backpressure_blocks` in `crates/ulpf/src/metrics.rs`;
`queue_high_water_never_exceeds_capacity` in `crates/ulpf/tests/adversarial.rs`;
`strip_syslog` in `crates/ulpf-parse/src/envelope.rs`. **Principle.** Measure the thing
you report; a counter derived from a race is a guess with a number on it. **Ruled out.**
Inferring "engaged" from the high-water (wrong whenever the race over-counts); treating a
BOM as message bytes (a Windows-exported ASA log parses as `pattern_no_match` for every line).

## D24. Subs run per field: file order, one winner per field, later subs may gate on earlier subs' output
**Decision.** Every `[[sub]]` whose gate passes runs, in file order, except that a field
already re-parsed by an earlier matching sub is not re-parsed again. A later sub may gate
on a field an earlier sub produced. The status is the worst outcome over the fields that
have subs: `no_match` beats `uncovered` beats `matched`; when none of the sub fields is
present the status is `not_applicable`. **Anchor.** `Parser::run_subs` and `SubStatus` in
`crates/ulpf-parse/src/compile.rs`; `subs_on_different_fields_all_run_and_same_field_subs_are_alternatives`
and `delimiter_rest_feeds_subs_gated_on_earlier_columns` in
`crates/ulpf-parse/tests/strategies.rs`. **Principle.** General-purpose over
special-purpose: the same rule serves ASA-style alternatives on one `message`, SonicWall's
three packed fields, and pfSense's three-level CSV tail, with no new syntax. **Ruled out.**
First-match-wins across all subs (SonicWall's `dst` never ran; found in the worker's
fixture); running every eligible sub (ASA's outbound and inbound alternatives on one field
would report `no_match` for every event).

## D25. Delimiter strategy keeps the unsplit tail under `rest` for a gated sub to split
**Decision.** `rest = "name"` on a delimiter strategy emits everything after the last named
column as one span; a `[[sub]]` gated on an earlier column splits it with the layout that
row type uses. **Anchor.** `DelimConfig` in `crates/ulpf-parse/src/delimiter.rs`;
`Strategy::rest` in `crates/ulpf-parse/src/def.rs`; `parsers/pfsense_filterlog.toml`,
`parsers/palo_alto_panos.toml`. **Principle.** Pull complexity downward: PAN-OS's four
log types share seven columns and diverge after; pfSense's tail depends on IP version,
then protocol, then ICMP type. One definition per device family, not one per shape.
**Ruled out.** One parser file per PAN-OS log type (the worker's two files; detection by
`contains = ["TRAFFIC"]` claims any line with that word); a pattern with `{interface:word}`
over CSV (`word` swallows commas: the worker's pfSense fixture had the interface column
holding twenty fields).

## D26. The envelope exposes RFC 5424 structured-data parameters as fields; brackets that are not SD stay in the message
**Decision.** `[SD-ID name="value" ...]` elements push each parameter under its own name
(`source-address`) and keep `syslog_sd` as the raw text; a bracket run whose first
element is not RFC 5424 structured data (Check Point's `[key:"value"; ...]`, a truncated
element) is treated as message text so the strategy sees it. A 5424 timestamp written
`YYYY-MM-DD HH:MM:SS` (older Check Point exporters) is accepted. **Anchor.** `rfc5424`,
`sd_params` in `crates/ulpf-parse/src/envelope.rs`;
`rfc5424_structured_data_params_become_fields_and_odd_brackets_stay_message` in
`crates/ulpf-parse/tests/strategies.rs`; `parsers/juniper_srx.toml`, `parsers/check_point.toml`.
**Principle.** Information hiding: RFC 5424's escaping and grammar are the envelope's
knowledge; a definition should not re-parse `syslog_sd` with a key=value strategy (the
worker's Juniper file did, and would have broken on `\"`). **Ruled out.** A new strategy
kind for "the envelope is the whole event" (a catch-all `{message:rest}` pattern is
standard syntax and also covers the unstructured form of the same device); rejecting
Check Point's header as not-5424 (loses host, app and procid on every line).

## D27. The `timestamp` slot is generated from the time module's own shapes and zone table
**Decision.** The pattern slot regex is built once from `ulpf_time::zone_names()` and
accepts the ctime weekday, the Cisco IOS `*`/`.` clock mark, a year before or after the
time, a fraction, and a known zone name or numeric offset. **Anchor.** `timestamp_regex`
in `crates/ulpf-parse/src/template.rs`; `zone_names` in `crates/ulpf-time/src/lib.rs`;
`timestamp_slot_accepts_ctime_and_cisco_ios_shapes_without_eating_hostnames` in
`crates/ulpf-parse/tests/strategies.rs`. **Principle.** One source of truth: the slot
recognises exactly what `ulpf_time::parse` can read, so a slot that matches always yields
a time. **Ruled out.** `[A-Z]{1,5}` for the zone (a pattern `{ts:timestamp} {host:word}`
would swallow an all-caps hostname as a zone); leaving the weekday and mark to `{text}`
slots (the worker's IOS and OpenVPN definitions did, and never parsed a device time).

## D28. A pattern slot that captures nothing emits no field
**Decision.** An empty capture (`rest` with nothing left, `quoted` of `""`) is the absence
of a field. **Anchor.** `CompiledPattern::apply` in `crates/ulpf-parse/src/pattern.rs`.
**Principle.** Define errors out of existence: Junos structured lines have an empty
message part; with an empty `message` field present, every such line would read as
`sub_uncovered`. kv and delimiter keep empty values because there the device wrote the
key or the column. **Ruled out.** Special-casing `rest` only (a `quoted` empty string
would then behave differently for no reason a definition author could predict).

## D29. Mapping: `[values] absent` list, `*` class conditions mean present, explicit `Unknown` enum value
**Decision.** The mapping file declares source values that carry no information (`-`,
`N/A`, empty, `0.0.0.0`); they are neither mapped nor reported as unmapped and do not
satisfy a class condition. A class condition value of `*` means "field present"; this was
documented from the start but never implemented, so every wildcard rule was dead until the
fixture review of this session. The action enum has an explicit `Unknown` value for
`NA`/`none`, so a SonicWall `fw_action="NA"` is neither `Allowed` nor `Other`. **Anchor.**
`Mapping::absent`, `select_class` in `crates/ulpf-normalize/src/mapping.rs`; `[values]`
and the `Unknown` action value in `mappings/ocsf.toml`;
`absent_values_are_neither_mapped_nor_unmapped_nor_class_evidence` and
`class_rule_wildcard_means_present` in `crates/ulpf-normalize/tests/normalize.rs`.
**Principle.** Canonicalisation belongs to the mapping stage; a parser must not drop
vendor values. **Ruled out.** Dropping `-` in the Squid parser (parser knows output
semantics); listing `na` under `Allowed` (the worker-era fixture showed a denied admin
login normalised as Allowed).

## D30. Worker-produced parser files were reviewed against their generated output and mostly rewritten
**Decision.** Fan-out 2 (haiku, low effort) returned ten families. The review compared each
fixture line with the sample and the vendor format: Palo Alto was written as key=value
(PAN-OS is positional CSV), Check Point and Juniper worked around engine gaps with
`{_:rest}` hacks, Cisco IOS and OpenVPN used invented message texts and never parsed the
device time, pfSense misparsed every row, SonicWall's second sub never ran, and fixtures
had snapshotted all of it as expected. Seven families were rewritten by the lead from the
vendor references; Sophos, Squid and Suricata were kept with mapping fixes. Fixtures are
generated by `ulpf fixture` and reviewed line by line before commit; they stay full
snapshots as regression tests. The rewritten families were then checked against vendor
documentation by three Opus reviewers with web access (read-only), and their confirmed
findings applied: PAN-OS THREAT/CONFIG column corrections, Check Point's default header
timestamp, IOS origin-id order and message variants, Junos legacy and trailing-field
forms, pfSense IPv6 protocol casing, SonicWall message ids and quoting, OpenVPN's
VERIFY ERROR serial. Format fidelity is verified against sources, never inferred from a
passing fixture. **Anchor.** `parsers/*.toml`, `samples/*.log`,
`fixtures/*.expected.jsonl`; `skeleton` in `crates/ulpf/src/fixture.rs`. **Principle.**
Never report a test as passing that only proves the code does what the code does: a
fixture generated from wrong output passes by construction. Cheap workers are for work
whose correctness the lead can verify mechanically; format fidelity is not that.
**Ruled out.** Trusting the workers' "all fixtures pass" (true and meaningless).

## D31. Throughput file: deterministic mutation of the committed samples, regenerated on demand, never committed
**Decision.** `cargo run --release -p ulpf --example gen_bench -- 5000000 bench` reads
`samples/`, frames multi-line events, and writes N lines by picking events with a fixed-seed
xorshift generator, rewriting IPv4 addresses, ports and session ids, and injecting ~0.1%
mess (truncation, a non-UTF-8 byte, doubled spaces, empty lines). `bench/*.log` is
gitignored. The measured number lives in `PROGRESS.md` with the machine it was measured on.
**Anchor.** `crates/ulpf/examples/gen_bench.rs`, `bench/README.md`, `.gitignore`.
**Principle.** A throughput claim is reproducible or it is not a claim. **Ruled out.**
Committing a large file (repo weight, and it goes stale as samples change); timing the
samples directory (142 events cannot show sustained throughput or backpressure).

## D32. kv strategy accepts several quote characters
**Decision.** `quote` may list more than one byte; a value closes with the byte that
opened it. **Anchor.** `KvConfig::quotes` in `crates/ulpf-parse/src/kv.rs`;
the `appName='General HTTPS'` assertion in
`subs_on_different_fields_all_run_and_same_field_subs_are_alternatives`,
`crates/ulpf-parse/tests/strategies.rs`.
**Principle.** Pull complexity downward: a device that quotes one field differently should
cost its definition one character, not a split value. Written for SonicOS's reported
`appName='...'`; the 2026-09-05 documentation review found every captured SonicOS line
double-quoting it, so no shipped definition uses the capability yet. Kept: tested, one
line, and the next vendor to do this costs nothing. **Ruled out.** A second `quote2` key
(two keys for one concept).

## D33. One writer per store, enforced by the catalogue's exclusive lock; recovery is consistent in both directions
**Decision.** `RawStore::open` opens `catalog.sqlite` in SQLite's exclusive locking mode
and takes the lock with a first write, so a second writer, or a catalogue reader, gets
"store is in use" and the OS releases the lock if the process dies. On open, `recover`
walks the index back to the last entry whose record is fully present, reindexes complete
records the segment holds beyond it, and cuts both files there. The engine flushes both
buffers before a batch's ids can appear in the output. Found by the invariant review:
two concurrent runs on the default store path each exited 0 and left 37,041 corrupt
records with 170,001 ids issued twice; a kill with the index buffer ahead of the segment
buffer left the store unopenable. **Anchor.** `open_catalog`, `recover`, `record_end` in
`crates/ulpf-store/src/store.rs`; the `flush(false)` before `send_batch` in
`crates/ulpf/src/engine.rs`; `a_second_writer_is_refused_while_the_store_is_open`,
`index_ahead_of_segment_recovers_to_the_last_complete_record`,
`segment_ahead_of_index_reindexes_complete_records_and_drops_a_torn_tail` in
`crates/ulpf-store/tests/roundtrip.rs`. **Principle.** Immutability is a property of the
interface only if the interface is the only writer; a permanent id must be permanent
across a crash. **Ruled out.** A lock file (goes stale after a crash and wedges the
store); trusting the index unconditionally (the documented "index is authoritative" only
covered the segment-ahead direction).

## D34. A failed output stage aborts the run; ingest never blocks on a dead consumer
**Decision.** Only the workers hold the batch receiver, so when the output thread fails
(unwritable path, disk full, closed pipe) the workers exit, the channel disconnects, the
ingest thread's next send fails, and `run` returns the output error after flushing the
store. Found by the invariant review: `ulpf run ... | head` hung forever with no counter
block. **Anchor.** the `drop(batch_rx)` and the result match in `run`, `send_batch`
returning `Result`, in `crates/ulpf/src/engine.rs`; `output_failure_aborts_instead_of_hanging`
in `crates/ulpf/tests/adversarial.rs`. **Principle.** Errors as values applies to the
engine's own failures too: a hang is the one outcome that produces no counter.
**Ruled out.** A watchdog timeout (guesses at a number; a slow disk is not a failure).

## D35. Subs run on materialised values by copying them once
**Decision.** A `[[sub]]` whose input field was materialised (a JSON value, an unescaped
quoted value, an RFC 5424 parameter with escapes) runs on a copy and pushes owned
sub-fields; the status semantics are identical to the borrowed case. Before this the sub
was skipped and the event reported `not_applicable`, so a Suricata `http.url` could never
be split and nothing said so. **Anchor.** `Parser::run_subs` in
`crates/ulpf-parse/src/compile.rs`; `subs_run_on_materialised_json_values` in
`crates/ulpf-parse/tests/strategies.rs`. **Principle.** The one allocation is paid only
where the value already allocated; the hot path for borrowed spans is unchanged.
**Ruled out.** Counting the skip as `sub_no_match` (visible but still useless).

## D36. Repeated source fields keep every value; `time_error` means present-but-unreadable; class uids are range-checked
**Decision.** A source field name that repeats within one event lands in `unmapped` as
`name`, `name#2`, `name#3`, and `unmapped_fields` counts what was emitted. The parse
stage sets `timestamp_error` only when no candidate resolved, so the counter is disjoint
from `time_from_receipt`. `Mapping::compile` rejects a class uid outside 0..=99,999,999
so `ulpf check` reports it with the file, and `type_uid` arithmetic saturates. All three
from the invariant review. **Anchor.** `unmapped_insert` and the uid check in
`crates/ulpf-normalize/src/mapping.rs`; `resolve_timestamp` in
`crates/ulpf-parse/src/compile.rs`; `a_repeated_source_field_keeps_every_value`,
`absurd_class_uid_is_rejected_at_load` in `crates/ulpf-normalize/tests/normalize.rs`;
`timestamp_error_is_reported_only_when_no_candidate_resolves` in
`crates/ulpf-parse/tests/strategies.rs`. **Principle.** Never a silent drop; a counter
that fires on correct events is worse than no counter. **Ruled out.** Arrays for repeated
keys (changes the value type of a field depending on the event, which every consumer then
has to special-case).

## D37. The zero-copy claim is measured by a counting allocator, and the three per-event allocations it found are gone
**Decision.** `crates/ulpf-parse/tests/alloc.rs` installs a counting global allocator and
asserts that, after warm-up, detection and parsing allocate nothing over every sample of
the ten families whose values are borrowed spans, and nothing for CEF and LEEF. The
zero-copy dimension of the invariant review found three per-event allocations: the
multi-field `[[timestamp]]` join buffer was cloned into `timestamp_text` (one Vec per
Sophos event); CEF and LEEF built fresh position vectors per event; the JSON flattener
cloned strings it already owned. The join buffer now travels with `Parsed` and comes back
on `clear`, the CEF/LEEF buffers live in `Scratch`, and the flattener moves values.
The documented exceptions stand: JSON values, a quoted value with escapes, a sub on a
materialised value, `column_N` names. **Anchor.** `Parsed::take_spare`/`give_back` in
`crates/ulpf-parse/src/lib.rs`; `StructuredScratch` in `crates/ulpf-parse/src/structured.rs`;
`resolve_timestamp` in `crates/ulpf-parse/src/compile.rs`; `crates/ulpf-parse/tests/alloc.rs`.
**Principle.** A performance invariant that is not measured by a test is a hope; the
throughput number stays honest only if the hot path stays what the docs say it is.
**Ruled out.** A streaming JSON flattener (the JSON exception is documented and JSON is a
minority of the throughput file; revisit if it dominates).

## D38. v1 layout: an `ulpf-infer` crate that depends on `ulpf-parse` only; server, pending, tail and inference buffers are modules of `ulpf`
**Decision.** Inference is its own crate with one dependency, `ulpf-parse` (for `Template`,
`SlotKind`, the envelope strip and `Parser::from_definition` to verify its own output).
The HTTP server, the pending directory, the tail ring and the per-source buffers are
modules of the `ulpf` crate beside the engine. **Anchor.** `Cargo.toml` (members),
`crates/ulpf-infer/Cargo.toml`, `crates/ulpf/src/lib.rs`. **Principle.** The
parser/mapping wall extends to inference by the dependency graph: `ulpf-infer` cannot
name an output-schema field because `ulpf-normalize` is not in its tree. Sub-agent
isolation: the inference crate was built and graded without touching the engine.
**Ruled out.** A `ulpf-server` crate (the binary is `ulpf`; a server crate would need the
engine, and the binary would need the server, a cycle, or the documented binary path moves).

## D39. Optional groups `{? ...}` in the template syntax
**Decision.** A run of constants and slots may be wrapped in `{? ...}` and is matched as
one optional unit; groups do not nest; `to_pattern` re-emits them and the bijection test
covers them. **Anchor.** `parse_tokens`/`write_tokens` in `crates/ulpf-parse/src/template.rs`,
`emit_tokens` in `crates/ulpf-parse/src/pattern.rs`,
`optional_groups_round_trip_and_match_with_or_without_the_segment` in
`crates/ulpf-parse/tests/roundtrip.rs`. **Principle.** When inference discovers a
structure the format cannot express, fix the format: the prototype's worst failure was
one template per optional-field combination. **Ruled out.** Emitting a `patterns` list
with one entry per combination (eight patterns for three optional fields, and a human
cannot see that they are one message).

## D40. Directory watching by polling with a stability rule, not `notify`
**Decision.** `serve` scans the watch directories every `--poll-ms` (250 ms). A file that
kept its size for two ticks is ingested to its end; a file still growing after four ticks
is ingested up to its last complete line; offsets resume from the catalogue's ingest rows
so a restart does not store the same bytes twice; a file that shrank is re-read from the
start and reported at once. A source is named by its path relative to the watch root it
was found under (review finding: basenames summed in the catalogue, so two `syslog.log`
files shared one resume offset). An unreadable file is counted and reported once and
retried only when it changes. The tailer counts a file once when it first sees it.
**Anchor.** `poll_loop`, `source_name` in `crates/ulpf/src/engine.rs`;
`RawStore::ingested_bytes` in `crates/ulpf-store/src/store.rs`. **Principle.** Pull complexity downward: the two ways a
file arrives (a `cp` that lands in chunks, a stream that never stops) are both handled
by one rule the operator never sees. Zero new dependencies. **Ruled out.** `notify`
9.0.0-rc.5 (verified on crates.io 2026-09-05): a release-candidate dependency in a static
binary, and inotify/FSEvents events do not cross Docker bind mounts on macOS, which is
exactly how the container will be fed tonight.

## D41. `Live` is the one shared object; the pipeline is swapped, never mutated; the server owns nothing
**Decision.** Counters, the parser pipeline behind an `RwLock<Arc<Pipeline>>` read once
per batch, the raw store behind a `Mutex` taken once per batch, per-source counts, the
tail, the inference buffers and the pending directory live in `Live`. A reload builds a
new `Pipeline` and swaps the `Arc`; a worker notices at its next batch and drops its
detection hint. The HTTP server holds an `Arc<Live>`, a 200 ms frame cache and each
client's stream position, nothing else. **Anchor.** `Live`, `reload_parsers`,
`worker_thread` in `crates/ulpf/src/engine.rs`; `App` in `crates/ulpf/src/server.rs`.
**Principle.** One writer per shared resource, and the per-event path never touches a
lock: the store lock, the pipeline read, the source-stats lock and the inference offer
are per batch (1024 events); the review found the first version offering unknown events
one at a time and it now hands over the batch's unknown events in one call. **Ruled out.**
A request channel to the ingest thread for traceback reads (a second protocol for one
call); a per-event store lock (measurable at 250k events/s); a server-side copy of any
engine state (two truths).

## D42. Traceback reads through the writer
**Decision.** `RawStore::get` flushes and reads one record positionally from the open
files, so the server reads the store through the same handle that holds the single-writer
lock; the bytes are copied out and the lock is held for one pread. **Anchor.**
`RawStore::get`, `source_names` in `crates/ulpf-store/src/store.rs`;
`the_writer_reads_its_own_records_back_by_id` in `crates/ulpf-store/tests/roundtrip.rs`;
`Live::traceback` in `crates/ulpf/src/engine.rs`. **Principle.** Reuse the lock the store
already has; a second mapping of a file that is being appended to was the v0.1 wart.
**Ruled out.** `RawReader` inside the server (maps the files as they were at open time,
so the newest ids are invisible and recovery can shrink a mapped file).

## D43. Inference buffers: bounded copies of unknown events, ordered by raw id, clustered by threshold in `serve` and once after `run`
**Decision.** A worker copies an event into a per-source buffer only when no parser
claimed it; the buffer holds at most 4096 lines and overflow is a counter
(`infer_buffer_full`), never a dropped event (the store and the output already have it).
Lines are keyed by raw id and sorted before clustering, so the same input yields the same
proposal regardless of worker scheduling. In `serve` a source is clustered at the
threshold (64), then at each doubling, and again when it goes quiet for 5 s with at least
`min_support` new lines; in `run` the buffers are clustered once after the output thread
finishes, so the throughput number stays ingest to output. **Anchor.**
`crates/ulpf/src/inference.rs`; the `offer` call in `worker_thread`. **Principle.**
Zero-copy hot path: the allocation is on the unknown path and the counting-allocator test
still passes; observability: every buffered, dropped, clustered and skipped line is a
counter on screen. Found by the in-process test: without the raw-id order, two runs over
one file produced two fingerprints. **Ruled out.** Clustering inside the worker (blocks
the hot path for the whole batch); unbounded buffers (a device that never matches would
grow until the process dies).

## D44. The pending directory is three plain files per source; approval is the only path to `parsers/`
**Decision.** `<id>.toml` (the definition, editable), `<id>.json` (evidence, review state,
fingerprint) and `<id>.lines` (the unknown lines it was built from). One proposal per
source; a newer proposal replaces it unless a human edited it; a rejected proposal's
fingerprint is remembered on disk and never resubmitted; approval validates the text,
requires a name of `[A-Za-z0-9_-]+` (it becomes a file name: the review found `../x`
would have written outside `parsers/`), refuses a name an active parser already has,
writes `parsers/<name>.toml` atomically (temp file, fsync, rename), moves the record
first and takes the parser file back if that fails, and reloads the registry; approving
twice is `not_found`. Every mutating operation holds one lock, so the inference thread's
next proposal cannot overwrite a human's edit mid-review or race an approval (review
finding). `.lines` is re-framed with the engine's framer so template member indices
line up with events, not physical lines. **Anchor.** `crates/ulpf/src/pending.rs`;
`Live::approve` in `crates/ulpf/src/engine.rs`;
`review_edge_cases_are_errors_as_values` in `crates/ulpf/tests/live.rs`. **Principle.**
Structural prevention over documentation: nothing in the engine can write to `parsers/`
except `Pending::approve`, and the file that a proposal parses nothing with is not in a
directory the registry loads. Teamability: a proposal can be reviewed with `cat` and edited
with any editor, and `ulpf check --pending` reports its problems by line. **Ruled out.**
Proposals as rows in the catalogue (not editable by the four non-Rust teammates);
proposals inside `parsers/` with a disabled flag (a malformed one would sit in the active
load report).

## D45. Generated parsers carry priority -1 and a signature the human can read
**Decision.** Every emitted definition has `priority = -1`; its `[match]` is `contains`
of a word present in 98% of the source's lines, else a `regex` alternation of the
templates' leading constants, else `regex = "."`. **Anchor.** `matcher` in
`crates/ulpf-infer/src/lib.rs`; `Registry::new` ordering in `crates/ulpf-parse/src/detect.rs`.
**Principle.** A generated parser can never take an event from a hand-written one: the
registry tries all priority-0 parsers first. The cost of a generated regex matcher is paid
only by events no hand parser claimed. The review found the gap between two unknown
sources: an approved `kernel:|sshd\[` matcher claims a sibling device's `kernel:` lines,
which then fail to parse and would never reach inference; the engine therefore offers a
line a generated parser claimed but could not parse to the inference buffer as well
(`worker_thread`). **Ruled out.** Priority 0 (name order would decide
between `cisco_asa` and `mikrotik_inferred`); a matcher over the syslog body only (the
matcher sees the raw event by design, D16).

## D46. The inference algorithm, and the thresholds it was tuned to
**Decision.** Tokenize with the slot regexes (D14 made them the one syntax authority) plus
three shape rules the prototype lacked: a bracketed value with no spaces is one word
(`(SYN,ACK)`, `[WAN_IN-default-D]`), a run of seven or more two-digit hex groups is one
opaque chain (netfilter `MAC=`), an address never starts after a colon. Cluster by
word-level LCS similarity against a fixed seed (no erosion), taking the better of the
whole line and its first six words so a free-text tail cannot outvote the message type.
Align every member onto the pivot with a gap-open penalty (2) and a substitution state
(1): one contiguous absent field beats two split gaps, and `Accepted publickey` against
`Failed password` is two word substitutions, not a region. Presence per column decides
optional groups (absent in fewer than max(2, 5%) members is damage: required; present in
fewer is junk: dropped). Slots type by the family of their values, widening compatible
atoms (`ip`, `float`) and ignoring a dissenting minority below the same bound. A word
slot with at most three distinct plain-alphabetic values, each seen twice, not after an
identity key (`user`, `from`, `via`), splits the cluster so dispositions stay constant;
variable-length word runs (TCP flags) collapse to one text slot before that check so they
are not mistaken for keywords. Split siblings with identical shapes, or shapes differing
in one identifier, are merged back. Every pattern is compiled through the real parser
and every line re-tested; templates that match nothing first, or fall below `min_support`
after a split, stay in the evidence but not in the definition. **Anchor.**
`crates/ulpf-infer/src/{token,align,cluster,lib}.rs`; `Params`; the tests in each file;
the graded runs in `docs/inference-prototype-report.md` (v1 section). **Principle.**
Observability as a design input: every one of these decisions writes a line into the
evidence (`decisions`, per-template `history`, `verified` beside `support`, `unmatched`
by reason) so a proposal that looks wrong at 4am says why. **Thresholds, with the
alternatives tried on the four held-out files:** similarity 0.7 (the report's setting)
fragmented every free-text tail into singletons, 0.5 merged dispositions in the prototype;
0.6 with the keyword split as the guard covers every MikroTik and EdgeRouter line.
`enum_max` 3 (2 missed `TCP/UDP/ICMP`). `min_support` 3 (the report's recommendation;
2 promoted truncated pairs). `rare_share` 0.05 with a floor of 2. Gap 2 and substitution
1 (a gap penalty alone made `Accepted publickey` a region; no penalty let a missing NAT
block pull an address pair into the block). Head-6 similarity (without it wireless
disconnect reasons fragmented by word count). **Ruled out.** A cluster key on the first
word (a username became the key in nginx lines); token-count buckets (the report's first
failure mode); depth-limited splitting (MikroTik needs input/forward then ICMP/UDP/TCP).

## D47. A proposal parses nothing; a template earns its place in the definition by matching first
**Decision.** Nothing is parsed from a pending file: the registry loads `parsers/` only.
Within a proposal, a template whose compiled pattern matched no line first (an earlier,
more specific template took them all) or whose support fell below `min_support` after a
split is left out of `patterns` but kept in the evidence with a history line saying why;
the reviewer can put it back with `regenerate`. **Anchor.** `in_definition` in
`crates/ulpf-infer/src/lib.rs`; `Pending::regenerate`. **Principle.** The definition is
what the engine will run; the evidence is what the human reads. **Ruled out.** Deleting
dead templates (the reviewer loses the evidence that a shape existed).

## D48. Server: axum 0.8 on a two-thread runtime, SSE by per-client position, exact client counts, embedded UI with a disk override
**Decision.** The server runs on its own small tokio runtime beside the engine's threads.
`/api/stream` is an `unfold` stream per client: hello, then every 250 ms the tail lines
newer than that client's last id (at most 200, the rest counted as `skipped`), a metrics
frame every other tick from a 200 ms cache shared by all clients, and a `pending` event
when the generation changes; a guard in the stream state decrements `sse_clients` when
the connection drops. `/` serves `ui/dist` embedded with `include_str!`, or from
`--ui-dir` on every request. A 422 on approve carries the load problems; `hello` carries
the pending count. **Anchor.** `crates/ulpf/src/server.rs`; `docs/api.md`;
`the_server_is_a_window_onto_a_live_engine` in `crates/ulpf/tests/server.rs`.
**Principle.** Bounded work per client, per tick, however many clients: the hundredth
SSE client costs one frame clone. **Ruled out.** A broadcast channel fed by the output
thread (work on the engine thread proportional to clients); WebSockets (two-way for a
one-way feed); a JS bundler in the Docker build (three prebuilt files are committed).

## D49. The tail is a ring of emitted lines with ranges into the batch buffers
**Decision.** The output thread hands each batch's serialised buffer to the ring after
writing it (moved, not copied); the ring keeps `(raw id, range)` per line, evicts the
oldest beyond `--tail` events, and reports to each reader how many lines newer than its
position were evicted or cut. **Anchor.** `crates/ulpf/src/tail.rs`. **Principle.** The
allocation for serving lives in the server's buffer, not on the engine's path: the ring
adds one `Arc` clone per line to work the output thread already did. **Ruled out.**
Copying each line into the ring (a second allocation per event at full rate).

## D50. UI: Svelte 5 + Vite compiled to three fixed-name files, committed, no external requests
**Decision.** `ui/` builds with `pnpm build` to `ui/dist/{index.html,app.js,app.css}`
(no hashes, one chunk, assets inlined), committed so the Rust build and the container
need no node. Hash routing so the binary serves one page. All colours and spacing are CSS
custom properties at the top of `app.css`; `--ui-dir ui/dist` serves edits without a
rebuild. Verified in Chrome against the real server: live counters, review with approve,
traceback. **Anchor.** `ui/vite.config.js`, `ui/README.md`, `ui/dist/`. **Principle.**
The UI is a window, not the product: every control calls a route in `docs/api.md` and
nothing is faked in the browser. The 45-minute embed timer did not fire (first working
build at 6 minutes). **Ruled out.** Hashed filenames (the binary would need a manifest);
a CDN font (a runtime network call).

## D51. The review pass: what changed, what was closed, what is deferred
**Decision.** Two `aposd-critique` workers graded `ulpf-infer` and the `ulpf` engine,
pending, tail and server modules at the end of the v1 session (18 ranked findings, both
tables in `PROGRESS.md`). Every finding that input, a client or timing could trigger was
fixed in one commit with the suite re-run (amendments to D40, D41, D44, D45, D46 above;
`Pending::ids`, `Emitted`, `Params` serialised directly, saturating tail arithmetic, the
store bounds check, symlink-safe walk, parsers-directory change signature). Closed with a
reason rather than changed: axum's plain-text 400 for a malformed query or path parameter
(documented in `docs/api.md`; a custom rejection type would be forty lines to rename a
message nobody scripts against); `infer_buffered` counts lines ever buffered while a
source's `buffered` is the current buffer (documented; renaming a contract field the UI
already maps is not worth the churn tonight); integer widths in the evidence structs.
Deferred with an argument: `Live`'s public fields. The server is the only consumer and
reads them; hiding them behind twenty getters the night before the demo would touch every
handler for no behavioural change, and the invariant that matters (nobody mutates engine
state from outside) already holds because the mutable pieces are private or behind
methods. Revisit when a second consumer of `Live` appears. **Anchor.** the commit named
"review fixes" and this entry; `PROGRESS.md` "Review pass". **Principle.** A finding is
closed by a fix or by written evidence, never by a known-issues list.

## D52. Replay is a second engine over a store snapshot, not a mode of the live one
**Decision.** `ulpf replay` and `POST /api/replay` read every record below the store's
length at start through a `RawReader` the writer flushed first (in `serve`, through the
same files the writer holds; the reader is bounded to that length so appends after the
snapshot are invisible), and drive the shared `process_batch` with their own workers,
counters and output thread. Batches carry per-event receipts and source ids (empty
vectors on the live path, so the hot path pays one branch), so a replayed event sees the
receipt it was stored with and `processed_time` and every no-year timestamp resolve as
they did. Outputs are versioned beside the base path (`out.v2.jsonl`, `.meta.json`,
`.diff.jsonl`); the diff is a streaming merge by raw id with a byte-identical fast path;
`why` compares the SHA-256 of every parser and mapping file against the set the previous
version's first events were written with (the live meta keeps every earlier set in
`history`, so a reload or a reopen after a fix is visible, not hidden). A replay holds the
`Arc<Pipeline>` it started with: a parser approved mid-replay changes the live stream and
the next replay, and the report names the generation it used. **Anchor.**
`crates/ulpf/src/replay.rs`; `process_batch`, `Batch::receipt/source`, `Live::start_replay`
in `crates/ulpf/src/engine.rs`; `RawReader::segment`; `crates/ulpf/tests/replay.rs`.
**Principle.** Deep module: one per-batch path for live and replay (a second copy would
drift); information hiding: the diff and the versions know nothing of the engine; the
store's append-only interface is untouched (the replay never holds a writer). An output that is a sink (`-` or a device such as `/dev/null`, `output_is_sink`) gets no
version meta, no entity index and no recovery: the first cut wrote `/dev/null.v1.meta.json`
and broke the documented bench command, found when the multi-core measurement produced no
numbers.
**Ruled out.** Replaying through the live worker pool with a flag (every counter, the tail,
inference and per-source stats would need a "not this one" branch per event); a separate
process for the server's replay (cannot read source names while the writer holds the
catalogue, and no progress); comparing against the previous version's *final* file set
(the demo's own reopen-after-fix made v1 look unchanged; found by the test).

## D53. Slot names come from stated rules, each with its reason in the evidence
**Decision.** `SlotEvidence.reason` says which rule named a slot: (a) a `key=`/`key:` or
compound key before the value; (b) a plain constant word before the slot, gated by a
27-word stopword list (connectives, syslog severities in RouterOS topic lists, tcp/udp/icmp)
and by "`:` then space is a syslog tag, not a key"; (c) a curated vocabulary
(`docs/slot-vocabulary.md`, compiled in): keyed rows (`SRC=`/`from`/`saddr` -> `src_ip`,
`SPT=` -> `src_port`, `IN=` -> `in_interface`, ...), the first `{ip}:{port}->{ip}:{port}`
pair on a line, `from {ip} port {port}`, `for {word} from`, `{word}[{int}]:` -> `pid`,
ICMP `type=`/`code=`, a bracketed rule label, a TCP-flag run, the NCSA combined layout;
(d) the timestamp kind; (e) otherwise `kind+n`, `suggested = false`, with the reason it
stayed generic. Names are device-side words (`src_ip`, `user`, `proto`), never schema
paths; where two spellings are conventional the one already an alias in `mappings/ocsf.toml`
wins, so an approved proposal normalizes without a mapping edit. The old `in`/`out` names
were aliases of `traffic.bytes_in/out`; the vocabulary fixes that. Held-out grades are
unchanged (14/250, 9/250, 1/250, 19/285) with, for example, nginx going from one named
slot of eight to eight. **Anchor.** `name_slot`, `VOCAB`, `STOPWORDS` in
`crates/ulpf-infer/src/cluster.rs`; `SlotEvidence` in `crates/ulpf-infer/src/lib.rs`;
`docs/slot-vocabulary.md`; the naming tests in `cluster.rs`. **Principle.** No model
anywhere: every name has a reason a reviewer can read and a source a judge can check.
**Ruled out.** Naming from any preceding word (`info`, `on`, `for`, `tcp` became names);
naming the second address pair (RouterOS logs the NAT pair after the first; which is
pre-translation is the reviewer's call); value-inspection rules beyond TCP flags (no
held-out grade justified them).

## D54. Drift is a per-source window judged against a frozen baseline, healed through the same pending queue
**Decision.** Every source folds each batch into a 512-event window of misses (no parser
claimed the event, or a hand parser claimed it and failed). A completed window is judged
against the source's baseline (the miss rate over its earlier completed windows, which
stops accumulating once the source trips, so a format that changed and stayed changed
cannot drag the baseline up to meet it). The source is *established* after 1,024
baseline events with a baseline rate under 0.2; it *trips* when a window has at least 32
misses and a rate at least 0.25 above the baseline. The batch that completed the tripping
window is judged, not routed; from the next batch on, every miss of the source (unknown
lines and failures under the established parser alike) is offered to the inference buffer,
which keeps the unknown lines it already held (they are the misses that tripped the source)
and is given the established parser's definition as its prior. A window that has not filled is
judged once its source has been quiet for 5 s with at least 32 misses in it (the poller checks
every tick), so a low-volume device gets a verdict within seconds, not after 512 more events. The next
inference run composes an update: the prior's own parser is run over the lines first and
what it covers is excluded; a pattern prior gets the new patterns appended after its own
(first match wins, so old lines parse as before); a prior whose strategy still parses at
least 90% of the lines with the signature bypassed gets only its `[match]` widened to the
union; anything else stands alone and the decisions say why. The proposal keeps the
parser's name, bumps `[parser] version`, and is written under the source's pending id
with `updates`; approval writes over `parsers/<name>.toml` atomically after keeping the
replaced text as `pending/approved/<name>.v<N>.toml`, and clears the source's drift state
and baseline. A source that mixed two formats from the start has a baseline over 0.2,
never establishes, and never trips; its unknown half still reaches ordinary inference.
Four counters (`drift_tripped`, `drift_lines_routed`, `drift_proposals`, `drift_cleared`),
`GET /api/drift`, the `drift` SSE event and the unified diff on the review screen make
every step visible. **Anchor.** `SourceStats::observe`, `DriftState`, the `DRIFT_*`
constants, `Live::drift_alerts`, `clear_drift` in `crates/ulpf/src/engine.rs`;
`Inference::set_prior` in `crates/ulpf/src/inference.rs`; `infer_with_prior`,
`union_matcher`, `Update` in `crates/ulpf-infer/src/lib.rs`; `Pending::approve` (update
branch), `current_and_diff`, `unified_diff` in `crates/ulpf/src/pending.rs`;
`Meta::version` in `crates/ulpf-parse/src/def.rs`; `crates/ulpf/tests/drift.rs`.
**Principle.** Structural prevention over documentation: an update can only replace the
parser it was composed on (`updates.name` must equal the definition's name, checked at
approval), and it passes through the same validate-then-atomic-write path as a fresh
proposal, so drift-healing cannot bypass review or cross the parser/mapping wall (the
inference crate still cannot name a schema field). Define errors out of existence: a
tumbling window needs no timers and no per-event state beyond two counters; the baseline
freezing on trip means a permanent change is one alert, not a flapping one. Observability
as a design input: the window and baseline rates are on every source row. **Ruled out.**
Emptying the inference buffer on a trip (the first version did; a source that trips on a
quiet judgement has no later lines, so the update never came: found by the serve-mode test); a rolling window with a ring of
outcomes (512 bytes per source for a number the tumbling window gives within one window
of latency); comparing against a fixed absolute miss rate (a device that always had 15%
unmodelled messages would alert forever); `sub_uncovered` as a miss (a new message id
under an existing family is the existing "write the next sub" workflow, not drift).

## D55. The entity index is derived data beside the output, fed per batch by copied values, written by its own thread
**Decision.** `mappings/<schema>.toml` declares `[entities]` (five fixed kinds: `src_ip`,
`dst_ip`, `user`, `dst_port`, `device`, each a schema path), so the index and its routes
know entity kinds and never vendor fields. `normalize` reports, per event and without
allocating, which parsed field fed each kind (`NormalizeStats.entities`, an index into
`parsed.fields`) and the emitted `time`; the worker copies those values, the parser name
and the device (schema field, else the source name) into one per-batch arena, and the
output thread, which alone knows the file offset of every line, turns them into postings
and hands the batch to the index writer's bounded channel (block on full, never drop).
The index is SQLite beside the output (`out.jsonl.pivot`): one row per (kind, value,
commit) holding packed posting entries, a dictionary for devices and parsers, an
entities summary table; WAL, `synchronous = OFF` because it is rebuildable from the
output (`ulpf pivot --rebuild`). Queries read postings by range, so an entity with a
million events answers a page in bounded time; `related` is computed over the newest
10,000. **Anchor.** `Entities`, `EntityKind` in `crates/ulpf-normalize/src/def.rs`;
`NormalizeStats::entities`, `Mapping::provenance` in `crates/ulpf-normalize/src/mapping.rs`;
`crates/ulpf/src/pivot.rs`; `EntityBatch`, `process_batch`, `output_thread` in
`crates/ulpf/src/engine.rs`; `mappings/ocsf.toml`, `mappings/ecs.toml` `[entities]`.
**Principle.** The parser/mapping wall extended to the index: entity kinds are schema
knowledge, declared where schema knowledge lives, so ECS gets a pivot for free. Zero-copy
hot path kept where it was measured (parse and detect allocate nothing; the alloc test is
unchanged); the one copy per entity value is amortised into one arena per batch, and
everything heavier runs on the index thread. **Ruled out.** A row per event (D5's
arithmetic); parsing the emitted JSON back on the output thread to find entities (2-5 µs
per event on the ordered path); the index inside the raw store's catalogue (a store is
raw bytes; entities are a property of one output schema and change with a replay).

## D56. Integrity is exposed through the writer's snapshot, never through a second store handle
**Decision.** `GET /api/integrity`, `POST /api/integrity/verify` and the attestation route
read the store id, genesis and head under the store lock, and verify on a background
thread over `RawStore::reader()` (flush, then a `RawReader` bounded to the flushed record
count) so ingest continues while a million records are checked; the result and the
running flag live in `Live` and reach clients as the `integrity` SSE event. The traceback
carries `chain`, `prev_chain` (genesis for id 0) and `chain_match`, recomputed from the
stored digest on every request, beside the digest pair it already had. **Anchor.**
`Live::start_verify`, `integrity_summary`, `attestation`, `Live::traceback` in
`crates/ulpf/src/engine.rs`; the three routes in `crates/ulpf/src/server.rs`. **Principle.**
D41/D42 kept: the server owns nothing, and no code path opens the store twice.
**Ruled out.** Verifying under the store lock (blocks ingest for the length of the scan);
a cached chain in the server (two truths).

## D57. Provenance spans are pointer arithmetic on the borrowed values, computed only on the traceback
**Decision.** A parsed field's span is `[start, end)` of its `Cow::Borrowed` slice inside
the record's bytes, found by comparing pointers; a materialised value (JSON, unescaped,
joined timestamp, constant) has no span and the API says so with `null`. `Mapping::provenance`
runs the same alias-ranking routine as `normalize` over the same `Parsed`, so the two
cannot disagree, and returns the field index; the traceback joins it to the span.
**Anchor.** `span_in`, `TraceField`, `TraceProvenance`, `TraceTime` in
`crates/ulpf/src/engine.rs`; `Mapping::provenance` in `crates/ulpf-normalize/src/mapping.rs`.
**Principle.** The offsets already existed because the hot path is zero-copy (D15); the
feature surfaces them without adding a byte to the per-event path. **Ruled out.**
Recording offsets during parsing (a field per slot per event on the hot path, for a
value only the traceback reads).

## D58. ECS is one mapping file; the default schema stays `ocsf` when several mappings load
**Decision.** `mappings/ecs.toml` (ECS 9.5.0) maps the same source vocabulary under ECS
paths with its own `[entities]`; `--schema ecs` selects it; no parser changed (the branch
diff touches `mappings/` and the normalize tests only). With two mapping files the
pipeline's "first loaded" default became `ecs` by file name and broke every test that
relied on the default, which the first worker patched around in five test files; the
fix is one line in `Pipeline::load`: prefer the mapping named `ocsf`, else the first.
**Anchor.** `mappings/ecs.toml`; `Pipeline::load` in `crates/ulpf/src/pipeline.rs`;
`ecs_mapping_loads_and_has_no_vendor_vocabulary` (and siblings) in
`crates/ulpf-normalize/tests/normalize.rs`. **Principle.** The wall proven by a diff: a
second output schema is additive. Pull complexity downward: the default is the engine's,
not five test files' knowledge. **Ruled out.** Passing `--schema ocsf` everywhere (every
teammate command and every test carries a fact the loader should own); erroring when
several mappings load and no schema is given (breaks `ulpf run samples` on a fresh clone).

## D59. A killed run restarts to the same output and store as an uninterrupted one
**Decision.** `run` and `serve` both start by reconciling output and store. The store is
flushed per batch before ids escape (D33), the output's `BufWriter` is not, so after a
kill the output ends in a torn line and lacks the last stored records. Startup truncates
the torn line, reads the last emitted raw id (none for an empty or missing output), and
sends every later stored record through the pipeline into the output first, with the
receipt each record was stored with, through the same workers and the same ordered output
thread (a `Backing::Store` snapshot, D52). Ingest then resumes per source at the byte
offset the store already holds: the catalogue's completed ingests plus the torn tail,
summed from the records themselves. The counter block prints `recovered: N`. Found by the
evaluation harness (kill_recovery: a restart appended `partial + baseline` events, so the
score was "DOUBLE-COUNTED"); `crates/ulpf/tests/recovery.rs` kills the real binary
mid-run and asserts the restart equals the clean run id for id. `--receipt` pins the
receipt time so two runs over the same input are byte-identical (the fixture harness's
own knob, now on the CLI, so the black-box scorecard's 8 time-fallback mismatches are
gone). **Anchor.** `recover_output`, the `resume` map in `run`, `Live::receipt_nanos` in
`crates/ulpf/src/engine.rs`; `RawStore::ingested_bytes` in `crates/ulpf-store/src/store.rs`;
`--receipt` in `crates/ulpf/src/cli.rs`; `crates/ulpf/tests/recovery.rs`. **Principle.**
Define the error out of existence: there is no "resume" verb and no flag; a restart is a
start. The store stays the single truth and the output is derived from it, never the
other way round. **Ruled out.** A per-batch ingest row in the catalogue (a SQLite write
per 1024 events for a fact the records already carry); refusing to start on a torn output
(operator intervention for a routine crash); an output cursor file (a third thing to keep
consistent after a crash). **Amended (19:30 IST).** The serve isolation proof fed the 5M-event
bench file and sent ctrl-c after 20 s; the server printed "stopping" and kept ingesting for
minutes, because the stop flag was read only between directory polls and a file was framed
and stored to its end once started. A stop request now ends the file at the next batch
boundary (`ingest_file` checks `Live::stopped` after each send); everything stored is
still emitted, the ingest record holds the partial byte count, and the next start resumes
from it through this same path. The tailer credits `bytes` per batch for the same reason:
the live MB/s stood at zero until a large file finished.

## D60. Syslog listeners are producers on the same queue, sequenced under the store lock
**Decision.** `serve --syslog-udp ADDR --syslog-tcp ADDR` adds a UDP thread and a TCP
acceptor with one thread per connection (capped at 256; refusals counted). A datagram is
one event; a TCP connection is framed by RFC 6587 octet counting when it opens with
`digits SP`, else by the file line rule, and what is left when it closes is final input
(complete lines the continuation rule was holding become events; an unterminated tail is
an event counted `syslog_tcp_partial`). Events are batched per peer (`udp/<ip>`,
`tcp/<ip>`) by count, bytes or 50 ms, appended to the store under one lock, and the batch
sequence is taken inside that lock, so with three producers the output thread still
receives batches in raw id order; `send_batch` therefore takes a sequence rather than
owning one, and `Live.seq` replaced the ingest thread's private counter. A batch's bytes
are owned (`Backing::Owned`) rather than mapped. Listeners share the queue's block-on-full
policy; what the kernel drops before `recv` is invisible to the process by construction
and is measured by the soak from the sender's count, with an 8 MiB receive buffer asked
for. **Anchor.** `crates/ulpf/src/syslog.rs`; `Live::seq`, `Backing::Owned`, the listener
spawn in `serve` in `crates/ulpf/src/engine.rs`; `crates/ulpf/tests/syslog.rs`.
**Principle.** Raw before understanding for sockets too: the bytes reach the store before
any parser sees them, byte for byte (a datagram gets no terminator, a framed line keeps
its own). One sequencer, one writer: the invariant that made the output order equal the
id order for one producer is now enforced by taking the sequence where the ids are issued.
**Ruled out.** One batch mixing peers (per-source stats, drift and inference key on the
batch's source; per-peer batches keep every existing path correct with no worker change);
a thread pool for TCP (a connection is a device; devices are few and long-lived);
tokio for the listeners (the engine's threads and a blocking `recv` are the whole design).

## D61. The UI is seven windows onto `Live`, judged on two of them
**Decision.** Svelte 5 screens at hash routes `#/live`, `#/review[/:id]`, `#/trace/:id`,
`#/pivot[/:kind/:value]`, `#/replay`, `#/drift`, `#/integrity`, every control bound to a
route in `docs/api.md`, nothing computed from anything the API does not say (a missing
field is a contract gap raised to the lead, not a placeholder). The live tail renders on
`requestAnimationFrame`, caps rows at the tail size and drops frames it could not render,
showing the count. Traceback highlights provenance by byte range, not field name (two
fields sharing a key stay distinct), and a field whose span strictly contains other
reported fields yields to its parts, so a Check Point record lights as its thirty pairs,
not one block; hover on a normalized field lights its bytes and vice versa, `h` toggles
hex, click pins. Pivot's hero is a device-lane timeline with a time scale, kinds in
investigator order from `status.schema.entities`, related entities one click from
re-pivoting with a breadcrumb trail. Replay sets the `why` lines larger than the counters
they explain. Keyboard everywhere: `j`/`k`, `Enter`, `Esc`, `/`, `?`, digits for screens.
Built to three fixed files (`app.js` 127 KB), system font stacks, tokens at the top of
`ui/src/app.css`, no external request. **Anchor.** `ui/src/*.svelte`, `ui/src/keys.js`,
`ui/README.md`; D48, D50. **Principle.** The UI is a window, not the product (D50):
every number on screen is one the engine printed; two screens (traceback, pivot) got the
design budget, the rest stayed quiet and dense. **Ruled out.** Highlighting by field name
(collides on repeated keys); a mock server shipped with the UI (the worker's throwaway mock
died the moment the real routes existed); decorative motion (an operations tool at 3am).

## D62. The soak is a reconciliation identity, and the burst rate is whatever fills the queue
**Decision.** `scripts/soak.sh` starts `serve` on a fresh store, appends generated events
(the twelve families mutated, never identical lines) to one watched file at a paced rate
with a burst phase, optionally sends UDP and TCP syslog at their own rates, keeps one SSE
client connected, samples RSS every second, and at the end asserts the identity
`sent == framed == stored == emitted == verify records` with `corrupt == 0` and the
chain intact. A run whose serve died before its counter block is reported from the last
metrics poll with the arithmetic shown and the verdict PARTIAL, never PASS (`--report-only`).
Measured on the M1 Pro (2026-09-05, machine shared with other agents): run1 12 min at
12k/s, 10,005,840 events, RSS 11-84 MB, slope -0.26 MB/min, SSE max gap 0.52 s, PASS;
run3 10 min at 20.8k/s with a 3x burst, 14,976,000 events, RSS 11-103 MB flat over awake
time, PASS, queue high-water 13/64 and zero backpressure blocks because the pipeline
keeps up; burst run at 100k/s base with a 300k/s burst, 8,220,000 events, queue 64/64,
537 backpressure blocks, RSS peak 325 MB (the in-flight backlog), zero loss, PASS. The
socket run (10k/s file + 8k/s UDP + 8k/s TCP) reconciled TCP exactly and lost 47% of UDP
to the kernel (`netstat -s -p udp` dropped-due-to-full-socket-buffers 471,233 against a
461,363 shortfall): the listener's 8 MiB `SO_RCVBUF` request equalled `kern.ipc.maxsockbuf`
and was refused silently, leaving the 786 KB default; the listener now negotiates down and
reports the granted size. The same runs exposed `framed`/`stored` being credited once per
file after the batch loop, so a live reader saw `emitted > stored` under backpressure;
both are credited per batch now. **Anchor.** `scripts/soak.sh`,
`crates/ulpf/examples/soak_gen.rs`, `scripts/README.md`; `set_recv_buffer` in
`crates/ulpf/src/syslog.rs`; the per-batch `fetch_add` in `ingest_file`. **Principle.**
Measure the thing you report (D23): "the queue's saturation policy is exercised" is a
number of blocks at a stated rate, not a sentence; a partial run is labelled partial.
**Ruled out.** A fixed 3x burst as the saturation proof (it never filled the queue at
sustainable base rates); counting the kernel's UDP drops as engine loss (invisible to the
process by construction; measured from the sender and `netstat`).
**Amendment (2026-09-05 21:55, run 4, after the `SO_RCVBUF` negotiation).** Socket mode,
5 min at 10k/s file + 8k/s UDP + 8k/s TCP with the 60 s x3 burst, 9,000,000 sent
(`scripts/soak.sh --minutes 5 --file-rate 10000 --udp 127.0.0.1:5514 --udp-rate 8000 --tcp
127.0.0.1:5515 --tcp-rate 8000`). Not the quiet re-run the brief asked for: three release
builds ran beside it (load 11-49) and the report's own clock check shows the host suspended
for 16 minutes mid-run (11 SSE gaps matched by 11 RSS-sampler gaps, max 961 s), so the file
generator fell 12,608 chunks behind and the run was stopped before it drained (verdict
PARTIAL by the harness's rule; stored vs emitted was a lag, not a loss). What it measured:
`GET /api/status` reports `udp_rcvbuf: 8388608`, the full 8 MiB granted (the earlier run had
the 786 KB default after a silent refusal); UDP 911,692 of 2,400,000 received (62% shortfall)
against `netstat -s -p udp` dropped-due-to-full-socket-buffers rising by 1,488,318 for a
1,488,308 shortfall, so every missing datagram is a kernel drop, none an engine loss; TCP
exact, 2,400,000 = 2,400,000; RSS 16.5 to 714 MB (the in-flight backlog), 75 MB at the end.
Under this load the listener spent its time blocked on the full queue (64/64, 2,362
backpressure blocks), which is the designed policy (D60): the buffer only absorbs a burst as
long as the listener drains it. Demo warning until a quiet run says otherwise: UDP at 8k/s
on a loaded laptop loses datagrams in the kernel; TCP or the file path loses nothing.
`caffeinate -i` now runs for the rest of the session so the host cannot suspend a measurement
again.

**Amendment (2026-09-05 22:58, run 5, the same command from a watcher that waited for a quiet
machine).** Started at load 3.90 with no build running; two lanes then loaded the host for 54%
of the run (peak load 17.07, sampled every 2 s), so this is a second loaded point, not the
quiet run, and the host did not suspend (max SSE gap 0.55 s). 9,000,000 sent; `udp_rcvbuf`
8,388,608 granted again; UDP 1,063,160 of 2,400,000 received (56% shortfall, 1,336,840)
against `netstat -s -p udp` full-socket-buffer drops rising by 1,336,894: kernel drops, none an
engine loss. File 4,200,000 and TCP 2,400,000 exact; framed = stored = emitted = verified
7,663,160, chain ok. Queue 64/64 with 2,309 backpressure blocks: the engine ran at 5,950
events/s over the run with the entity index on (`serve` default, D66) beside 16k/s of syslog
(8k/s UDP and 8k/s TCP) and a 10k/s file feed, so the listener spent the run blocked on the queue and the kernel
buffer filled at 8k/s in. RSS 16.6 to 930 MB, the in-flight backlog while the output thread
was behind, 402 MB at the end. The demo warning stands until a run with nothing else on the
host says otherwise: at 8k/s UDP on a loaded laptop the kernel drops; TCP and the file path
lose nothing. The lesson for the soak itself: 26k/s aggregate with the index on is at the
measured index-on rate (28-31k/s, D66), so the harness should either turn the index off or
halve its rates when the question is socket loss rather than engine throughput.

**Amendment (2026-09-06 01:42-01:58, run 6, the quiet run the brief asked for).** Same
command, nothing else of the session running (no build, no lane, no browser; load 4.5 at
start, mean 4.1 and peak 8.6 over 501 samples, all of it the run's own processes; 2.4 at the
end; `caffeinate` holding the host awake). The result is the same as the two loaded runs:
9,000,000 sent; `udp_rcvbuf` 8,388,608 granted; UDP 1,067,333 of 2,400,000 received, shortfall
1,332,667 against `netstat -s -p udp` full-socket-buffer drops rising by 1,332,731; file
4,200,000 and TCP 2,400,000 exact; framed = stored = emitted = verified 7,667,333, chain ok,
0 corrupt; queue 64/64 with 2,980 backpressure blocks; the engine at 7,736 events/s over the
run with the entity index on; RSS 16.8 to 986 MB (the in-flight backlog), 483 MB at the end;
drain 254 s. So the loss is not the host's load: at 26k/s aggregate with the index on, the
engine is the bottleneck, the listener blocks on the full queue (D60, by design) and the
kernel drops UDP at 8k/s whatever the buffer. The demo rule is now firm, not a warning: feed
the demo device over TCP or the file path, or run `serve` with `--pivot off` when a UDP
device must keep up. No engine change tonight (the brief: a warning, not a second fix); the
fix that would change the number is the index cost (D66), not the socket.

## D63. Real captures fix parsers from vendor documentation, and stay in the samples
**Decision.** Real captures (public sources with permissive licences, and captures
generated locally from real Suricata, Squid, OpenVPN 2.4/2.5/2.6, nginx, HAProxy and Zeek
runs) live under `corpus/` with a PROVENANCE.md per directory and an index in
`corpus/README.md`; Elastic-2.0 and unlicensed sources are recorded, not copied. Every
parser the real data broke was fixed from the vendor's own documentation of that form and
the fix's source is cited inline in the definition: Cisco ASA `logging emblem` and
`logging timestamp rfc5424` header forms plus nine message ids; PAN-OS's empty serial
column on an unlicensed VM-Series (a matcher fix, verified not to shift columns); OpenVPN
over syslog (RFC 3164 `openvpn[pid]:`) and the 2.5+ ISO 8601 file-log stamp (Changes.rst:
2.4 is the last ctime release; the 2.4.12 capture is the positive control) with four subs
from the message strings in the source; IOS SISF; SonicOS empty address parts as optional
groups; legacy FortiOS `device_id=`/`log_id=` keys. Permissively licensed real lines were
appended to the samples with their source named in `samples/README.md`, and the fixtures
regenerated and reviewed line by line (D30). **Anchor.** `corpus/`, `parsers/*.toml`,
`samples/README.md`, `fixtures/*.expected.jsonl`. **Principle.** D30 again: a passing
fixture on synthetic data proved nothing; 100% `pattern_no_match` on 335 real ASA lines
was the first thing the real data said. **Ruled out.** Pattern-matching the sample into
submission (every change cites the vendor form it implements); copying Elastic-licensed
fixtures (the best messy captures, and not ours to redistribute).

## D64. Parquet is an additional sink behind `--parquet`, never the primary output
**Decision.** `crates/ulpf-parquet` is a leaf crate over `parquet` 59.3.0 with default
features off and `snap` on (27 dependencies, no thrift or arrow crate; the static musl
build is unchanged, the binary grows 320 KiB). Schema `ulpf_event`: raw_id, time
(TIMESTAMP_MILLIS), parser, source, class_uid, the emitted JSON line verbatim as
`normalized`, and the five entity columns; SNAPPY, row groups of 8192 (peak RSS ~42 MB
versus ~150 MB at 65,536 for a 3% larger file). The sink lives on the output thread only,
is not constructed when the flag is absent, and in `serve` rolls to a new file every
`--parquet-roll-rows` or `--parquet-roll-secs`, writing `.part` and renaming on close so
no reader ever sees a file without its footer. Counters `parquet_rows`, `parquet_files`,
`parquet_errors`; a failed write stops the sink and says so, the JSON Lines output is
unaffected. Measured on a 497,607-event slice under load: 292k events/s without the
flag, 135k with it (0.46x), of which ~2.6 µs per row is re-parsing the emitted line for
the scalar columns and ~1.5 µs the copy plus SNAPPY; the file is 3.6x smaller than the
JSON Lines with the whole line kept. **Anchor.** `crates/ulpf-parquet/src/lib.rs`,
`crates/ulpf/src/sink.rs`, the sink calls in `output_thread`, `crates/ulpf/tests/parquet.rs`.
**Principle.** Raw completeness first: a Parquet file truncated by a kill is worth zero
bytes (verified: pyarrow refuses it) while the JSON Lines truncated at the same point still
yields every complete line, so the sink that can lose everything cannot be the one the
pipeline's promise rests on. **Ruled out.** Parquet as the output format (the tail,
traceback, pivot and diff read lines by offset); `arrow` (the whole point of the plain
column writer is the dependency count); zero-copy column staging (the scalar columns are
available from the entity arena and would halve the cost; deferred: the flag is optional
and off in every measured number).

## D65. The v2 review pass: what changed, what was closed, what is deferred
**Decision.** Seven Opus graders ran `aposd-critique` read-only, one per crate group,
returning graded tables and findings ranked by what hurts at 04:00. Fixed, in one commit
(37e41d6: store/engine/syslog/sink and parse/time/infer/server/pending/pivot/replay/cli): the
attestation check now compares the head and refuses an empty checkpoint list (a store
rewritten from record 0 with a stripped attestation verified clean before); the reader
maps the index before the segment (a verify beside a live serve could name phantom
corruption); checked offset arithmetic on bytes read from disk; resume offsets summed
from the records rather than the catalogue's ingest rows (rows written per file after the
batch loop are not a watermark once sockets interleave; the store test that asserted the
rows' claimed 400 bytes now asserts the records' 396); recovery takes its sequence under
the store lock; per-source stats use the arrival clock and the batch's first source
(socket sources looked permanently quiet to the drift judge, recovered batches formed a
"recovered" source); drift clears itself after four clean windows (the field existed, the
transition did not); the output offset for postings is taken at the first write, after
recovery may have truncated, and a stale entity index is dropped after recovery; UDP peer
buffers grow on demand and idle peers are evicted; a listener flush failure stops the run
loudly; the Parquet row is built from the entity arena (schema-agnostic, no JSON re-parse,
and the OCSF-only columns under `--schema ecs` are gone) and an existing target is refused;
a failed RFC 5424 parse truncates its half-pushed fields; the per-source hint cannot
outrank a higher-priority parser (a generated parser could otherwise keep a line a hand
parser owns); `%s` on non-digits is no match and a bare number is an instant only at nine
digits (the corpus case that read `20260904` as August 1970 was the wrong answer encoded
as expected); drift-update members index the pending lines file; no widening of a prior's
signature to the catch-all; the `[[timestamp]]` spec follows emitted patterns; an update
composed on a kv or delimiter prior is written; pending ids are slug-only and the review
diff is capped at 4,000 lines; the pending list carries `updates`/`version`; four runtime
threads; the pivot cursor is `(time, raw id)` in the API and the UI; a partial replay is
never the comparison base; rebuild maps the output; zone offsets are range-checked.
**Closed with evidence.** "`drift_lines_routed` counts lines whether or not drift tripped":
the increment is inside `if routing`, which is true only in `Tripped`/`Proposed`; unknown
lines of a tripped source are drift evidence by design (D54). "A damaged record aborts the
replay": the snapshot is the writer's flushed files, so an unreadable record is corruption,
which `ulpf verify` names; a replay that silently skips it would hide that. **Deferred,
with the trigger.** Generating the signature from in-definition templates only (changes
generated matchers; re-grade `heldout/` when touched; today's looser matcher only claims
lines that then reach inference again under D45). `NEXT_SLOT` growing per reload (each
reload adds one capture-locations slot per pattern per worker; measured negligible at
fifty approvals; revisit if a deployment reloads thousands of times). Persisting the diff
index the replay already built (rebuilt once per version on first page request). Decoding
the record header in one place (four copies agree today; a change to the layout is the
trigger). **Anchor.** commit 37e41d6; `PROGRESS.md` "Fan-out 4" and item 15.
**Principle.** As D51: closed by a fix or by written evidence, never by a known-issues list.

## D66. The entity index is on for `serve` and off for `run`
**Decision.** `--pivot on|off` on both subcommands; `serve` defaults to on (the UI pivots
live, at device rates), `run` to off (bulk throughput; `ulpf pivot --rebuild` builds the
index afterwards from the output). Measured 2026-09-05 on the 497,607-event bench slice
(machine at load 12-17): 27,995-30,963 events/s with the index, 196,160-249,409 without,
and 158,791-196,461 to `/dev/null`, so the index thread, not the file write, is the cost.
The bench file is the worst case by construction: `gen_bench` rewrites every address and
port, so nearly every event carries entity values seen once, and the writer's cost is per
distinct (kind, value) per commit group; real device logs repeat their entities. The first
5M-event measurement with the index ran for forty minutes at 2% CPU before it was found
to be two bench processes sharing one SQLite file, and was discarded. **Anchor.**
`--pivot` in `crates/ulpf/src/cli.rs`; `Config::pivot_index`, the `index_entities` gate in
`output_thread`; the numbers in `PROGRESS.md` item 9. **Principle.** Measure the thing you
report; a default that costs an order of magnitude on the throughput criterion of the
harness every other tool is judged on is not a default, it is a feature the operator turns
on. **Ruled out.** Tuning the commit group (the writer already merges queued batches; the
cost is cardinality, not commits); building the index in `run` on a second thread pool
(the SQLite writer is single-threaded by nature); dropping the index (the pivot is the
payoff of normalisation and serve keeps it).

## D67. The demo runner plays the demo script's own commands, and a check proves they are the same text
**Decision.** `scripts/demo.sh` plays steps 0-9 of the PROGRESS.md demo (Enter advances;
`--auto` rehearses unattended with fixed pauses and resets at the end; `--reset` stops a
leftover server and removes `demo/`). Every command it runs is held as one string and printed
before it is evaluated, and `scripts/demo.sh --check` greps each string verbatim in
PROGRESS.md, so the runner and the script cannot drift without the check failing. The
runner uses only existing subcommands and the watch directory, waits for the proposal and the
drift update through `GET /api/pending` and says how long each took (or that it is not there
yet), and its server takes `--parsers demo/parsers --pending demo/pending`, so nothing lands in
the repo's `parsers/` or `pending/`; the demo script's own commands were moved to the same
directories and the reset line became `rm -rf demo`. Steps 10-13 (ECS run, throughput, kill
recovery, isolation) are terminal-two material and are named, not played. **Anchor.**
`scripts/demo.sh`, the "Demo" section of `PROGRESS.md`. **Principle.** One source of truth:
the text a presenter reads and the text the machine runs are the same bytes, checked, not
promised. **Ruled out.** Generating the PROGRESS section from the runner (the section carries
expected output and what to say, which the owner edits by hand; a generated block would be
overwritten); a runner with its own simplified commands (the first draft's UDP one-liner
already differed in quoting from the script's, which is exactly the drift the check exists
for); adding engine behaviour for the demo (a `--demo` mode would be code nobody runs in
production).

## D68. A name the input already carries outranks every other naming rule
**Decision.** The inference engine names a slot from the input's own vocabulary before it
looks at the value's type: in a line that opens with `{`, a quoted string followed by `:` is a
constant token (`Kind::Word`), not a quoted slot, so the naming rule reads `"key" :` before
the slot and the slot is `key` with reason `json key` (a nested object gives its innermost key
and the reason says so; a sanitised name says `written id_orig_h`); in a delimited file whose
buffered lines include a `#fields<sep>...` header, a template every member of which has exactly
the header's column count names each slot by the column its position falls in, reason
`header` with the column number, and a slot whose value spans columns stays generic with that
reason. Both rules are rule 0 of `docs/slot-vocabulary.md`. Consequence carried through: the
generated definition's `[[timestamp]]` follows the timestamp slot's name (`ts` for Zeek) instead
of assuming `timestamp`; `ulpf-parse` tries the candidates in order. Measured on the Zeek
files: json/conn 40 slots (1 suggested) became 19 slots (19 suggested) because keys are no
longer slots; json/dns 99 (3) became 42 (42); TSV conn 78 (16) became 78 (78); TSV http 541 (76)
became 541 (540) with its 40 templates and 1,354 `template_cap` lines unchanged, which is a
structural failure of clustering on tabular data, not a naming one. The four `heldout/` grades
are byte-identical before and after. **Anchor.** `crates/ulpf-infer/src/token.rs` (`tokenize`,
the `json` flag), `crates/ulpf-infer/src/cluster.rs` (`positional`, `json_keys`, `headers`,
`header_for`, `header_columns`), `crates/ulpf-infer/src/lib.rs` (`ts_fields`),
`docs/slot-vocabulary.md` rule 0. **Principle.** The device's vocabulary verbatim is the
contract (the parser/mapping wall): a key the device wrote is better evidence than a type the
engine guessed, and the mapping stage canonicalises. **Ruled out.** Naming the quoted key slot
after itself and the value after the previous slot (keys stayed slots with `distinct=1` and
cluttered every template and evidence file); joining nested key paths (`id_orig_h` from
`{"id":{"orig_h":..}}` needs a key stack across aligned columns with optional groups; every
Zeek log is flat); mapping header columns onto aligned columns (constants fold into pattern
text so slot index is not column index; separators are counted in the raw seed line instead);
lower-casing keys (case is the device's); raising `max_templates` for http.log (40 templates at
cap is the shape of the data: every combination of method, status and mime type is a cluster;
the fix is a delimiter-strategy proposal built from the header, which `Strategy` already
expresses losslessly, a second proposal path, not a threshold).

## D69. One token block, two embedded faces, colour only for state
**Decision.** The redesigned UI is built on a single `:root` block at the top of
`ui/src/app.css`: surfaces (`--bg`, `--bg-1`, `--bg-2`, `--sel`), lines, three ink levels,
four state colours each with a wash (`ok` = proved or streaming, `warn` = look at this,
`bad` = broken, `pend` = waiting for a human), eight provenance tints whose hue is an index
(which field owns which bytes in the traceback, which device owns which lane in the pivot),
a six-step type scale (11 to 28 px), an eight-step spacing scale (2 to 48 px), one 22 px row
height. Dark is the default; the light theme redefines only the colour tokens under
`:root[data-theme=light]`. Neutrals carry everything that is not state, including links and
the primary button (inverted ink, no hue). Text is IBM Plex Sans 400/600 and every number,
id, address, raw byte and code is IBM Plex Mono 400/500, both with tabular figures by default,
Latin-1 subsets taken byte-identical from IBM's own releases (`@ibm/plex-sans@1.1.0`,
`@ibm/plex-mono@2.5.0`, OFL-1.1, licence committed under `ui/fonts/`) and inlined into
`app.css` as `data:font/woff2` by `build.assetsInlineLimit` in `ui/vite.config.js`, 78,656
bytes, so the binary serves them air-gapped; the served page requests `/app.js`, `/app.css`
and `/api/*` and nothing else. Every text pair meets WCAG AA in both themes (lowest 4.64:1;
the table is in `docs/design.md`). **Anchor.** `ui/src/app.css` lines 1-60 and the
`@font-face` block, `ui/vite.config.js`, `ui/fonts/`, `docs/design.md`. **Principle.**
Data-ink: the counters, the timeline and the raw bytes are the content; chrome recedes, and
colour that does not mean state is noise a 3am operator has to read past. **Ruled out.**
Inter + JetBrains Mono (two unrelated designs, and Inter's figures are proportional unless
every counter asks for the feature); a font CDN (a runtime request the isolation script
would catch); system fonts only (no guarantee of tabular figures anywhere); a tokens file
each component imports (Svelte scoped styles would duplicate the cascade per component); a
second light stylesheet; a brand accent; cards, gradients, glass and glow.

## D70. Every write from the UI passes one keyboard-reachable confirmation
**Decision.** Approve, reject, replay and verify open the same `Confirm` component: a letter
opens it (`a`, `x`, `v`), focus lands on the confirming button, Enter confirms, Esc cancels,
Tab moves between the two, and the opening letter can never confirm; the box states the
exact file path, version and re-detection that will follow, and reject is marked as danger.
Mouse users get the same box, so there is no single-click write path. The approve flow is
captured one frame per key (`docs/screens/approve-1..5-1280.png`). **Anchor.**
`ui/src/Confirm.svelte`, the `asking` state in `ui/src/Review.svelte`, `docs/design.md`
"The confirmation". **Principle.** Approve is the one action that changes what the engine
parses (D45); it is deliberate by construction, not by care. **Ruled out.** A single-key
approve (an accidental key would write a parser); the browser's `confirm()` (unstyled,
inconsistent with the keyboard map, blocked in some webviews); a hold-to-confirm gesture
(mouse only).

## D71. Long lists and the raw record are virtualised on one component
**Decision.** One `VList.svelte` (fixed-height rows, the visible window plus a six-row margin,
the selection kept in view) renders the tail, the pivot timeline and entity search, the
replay diff entries, both traceback field lists and the byte ruler. The ruler is a virtual
list of row start offsets computed once per record (text rows never split a UTF-8 sequence,
hex rows are sixteen bytes), so a 4 MB single-line record shows its ruler 1.3 s after
navigation with 24 to 30 rows in the DOM and scrolls to any offset; the SSE client batches
frames so a 400,000-event drop at the queue's high-water mark kept the tail and counters live
with zero skipped frames. Navigation is in-app throughout (hash routes, breadcrumb trail,
Backspace along the pivot trail, Esc to the list), nothing depends on browser back or a
visible URL bar, so the same build runs in the desktop webview (D73). **Anchor.**
`ui/src/VList.svelte`, `ui/src/Traceback.svelte` (`starts`, `segments`),
`ui/src/state.svelte.js` (`pushTrail`), `docs/design.md` "Under load". **Principle.**
Performance is part of the design: a screen that freezes on the record a judge asks for is
not designed. **Ruled out.** Rendering the whole record as one text node (a 4 MB node froze
the page); a virtual-list dependency for thirty lines; `history.back()` for back actions.

## D72. A delimited file with a fitting header is one delimiter definition, not a cluster of patterns
**Decision.** When the buffered lines hold exactly one `#fields` header with an ASCII separator
and every non-`#`, non-empty body has exactly the header's column count (separators counted in
the raw bytes, at least `min_support` rows), `ulpf_infer::infer` skips clustering and writes a
`kind = "delimiter"` definition: `fields` are the header names sanitised (duplicates suffixed),
`[[timestamp]]` is the column whose every value is a timestamp atom, column types come from the
pattern path's own `slot_kind` over each column's values (a column of only `-` is text), and the
matcher is a regex anchored to the whole line with the header's column count and the timestamp
column's shape (`[0-9]{9,19}(?:\.[0-9]+)?` for epoch, the ISO form for ISO), ending in
`[\r\n]*$` so the stored terminator does not change the count. The definition is verified as the
runtime will run it: `Parser::from_definition`, then `matches` and `parse` on every stored line,
a rejected line being a decision line. Evidence is one template (the `#fields` line verbatim,
support = data rows, verified = rows parsed) and one slot per column with reason `header \`x\`
(column i)`. `#`-prefixed lines are unmatched under the new reason `header`. The rule is fit all
or fall back: a second header, one row of another width, too few rows, a non-ASCII separator,
or a syslog envelope (the anchored signature would reject its own rows) sends the input down the
pattern path unchanged, where D68 still names the slots from the header. Two lines of
`crates/ulpf/src/pending.rs` changed with it, both gated on `StrategyKind::Pattern`:
`Pending::write` no longer skips a proposal for having no `patterns` unless it is a pattern
definition, and `regenerate` no longer rewrites `patterns` from the kept templates on a
non-pattern strategy (which would have put the header line into a delimiter definition and
failed validation, and was already wrong for a kv or delimiter drift update). The lead accepted
that touch: it is the review workflow, not the hot path, the store or the API, and no working
version of the feature exists without it. Measured: http.log 40 templates / 100 of 1,545 lines
became 1 definition / 30 columns / 1,536 of 1,545; conn.log 5 / 5,096 of 5,129 became 1 / 22 /
5,120; dns.log 1 / 26 / 3,400 of 3,409; approved, http.log parses 1,536 of 1,536 data lines
with `ts` as the event time; `heldout/` byte-identical; 114 tests. **Anchor.**
`crates/ulpf-infer/src/lib.rs` (`header_fitting`, `infer_delimited`, the `if !syslog` guard),
`crates/ulpf-infer/src/cluster.rs` (`column`, `column_kind`), `crates/ulpf/src/pending.rs`
(`write`, `regenerate`), `crates/ulpf/tests/live.rs` (a delimiter proposal is written and
approved). **Principle.** Use the structure the input declares; clustering is for inputs
that declare none. **Ruled out.** Column count alone as the matcher (any 22-column TSV would
claim conn.log's parser; the shipped delimiter parsers key on a leading time layout); the
header text as the matcher (data rows do not carry it); a per-column shape for every typed
column (a column with rare `-` dissenters would reject those rows); a majority-fit threshold
(a heuristic where a rule was asked for); prefixing the signature with the syslog envelope
regex (a second matcher shape for an input that exists nowhere in the corpus); raising
`max_templates` (40 templates at cap was the shape of the data, D68); a dummy pattern to slip
past `Pending::write` (a wrong file that fails validation).

## D73. The desktop app is a shell around the unchanged binary: sidecar, free port, splash then navigate
**Decision.** `app/` is a Tauri 2 application with its own Cargo workspace (an empty
`[workspace]` table keeps it out of the root workspace, so `cargo test --workspace` and the
engine build are unaffected; `cargo metadata` at the root lists the seven engine crates only)
and its own pnpm package. The engine, server and UI are the ulpf binary bundled as a sidecar
(`bundle.externalBin`, `binaries/ulpf-<host triple>[.exe]` copied by `app/scripts/sidecar.sh`,
which CI runs too). On launch the shell binds `127.0.0.1:0`, reads the port, releases it, and
starts `ulpf serve` with every path absolute against an app-owned data directory
(`app_data_dir`: `~/Library/Application Support/dev.ulpf.desktop` on macOS,
`%APPDATA%\dev.ulpf.desktop` on Windows; `watch/`, `store/`, `out.jsonl`, `pending/`,
`parsers/` and `mappings/` seeded from bundled resources only when they hold no TOML, so an
approved or edited parser is never overwritten; one absolute path in `app_config_dir/data_dir`
overrides it). The window opens on a bundled splash and `WebviewWindow::navigate` moves it to
the served URL once `/api/status` answers; the URL is written to `<data>/server.url` only then
and removed on stop. Files and folders dropped on the window, or picked through File > Add
files… / Add folder…, go through one `ingest_paths`: copied into `<data>/staging` on the same
volume and renamed into `watch/` under a unique name (folders keep their structure, regular
files only), so the engine's poller never reads a half-written file; the confirmation is one
element injected into whatever page the window shows with `WebviewWindow::eval`, replaced by
the next notice, so the served UI is not restyled and does not know the shell exists. The
title is `ULPF · engine ok · N events · M pending` once a second from `/api/metrics`
(`engine.emitted`, this run's counter, the counter block's meaning) and `/api/pending`;
`engine down (exit N)` when the child dies. Closing the window hides it on both platforms
and the engine keeps ingesting; the tray (menu bar on macOS, a runtime-drawn template glyph;
notification area on Windows, an owned copy of the app icon) offers Show, Open output folder,
Open in browser, Quit; Quit kills the child outright from `RunEvent::ExitRequested` and
`Exit` (`Child::kill`: SIGKILL on macOS, TerminateProcess on Windows), which the engine's kill
recovery makes safe (D59), and a generation counter keeps an earlier child's exit from
touching the current one. **Anchor.** `app/src-tauri/src/lib.rs` (`start`, `splash`,
`navigate`, `toast`, `stop`, the run-event handler), `ingest.rs`, `menu.rs`, `title.rs`,
`app/src-tauri/tauri.conf.json`, `app/scripts/sidecar.sh`, `app/README.md`. **Principle.**
The engine is frozen; the app owns launch, paths, drop and quit, and nothing else. **Ruled
out.** A Tauri crate inside the root workspace (tauri, wry and tao in every engine build);
creating the webview with `WebviewUrl::External` only once the server is up (no window for
the first seconds, and every restart needs the splash anyway); a fixed port (collides with a
demo server on 7878); parsing the engine's `serving http://` stderr line (couples the shell to
a log line and races the first request); copying straight into `watch/`; pointing the engine
at the dropped path (the drop must survive the original moving); a native dialog per drop; a
notification area in the served UI (a UI change for the shell's sake); re-copying parsers on
every launch; a graceful stop signal (std has none cross-platform, and D59 makes it
unnecessary); macOS-only hide-on-close (on Windows the last window closing would end the
ingest the tray exists to keep); a bundle identifier ending in `.app` (Tauri warns it collides
with the bundle extension; it is `dev.ulpf.desktop`).

## D74. Windows is built by CI behind two cfg shims, and has not been run on a Windows machine
**Decision.** `.github/workflows/app.yml` runs on every push and on `v*` tags: a matrix of
`macos-latest` and `windows-latest` builds the engine (`cargo build --release -p ulpf`),
names the sidecar per host triple through `app/scripts/sidecar.sh` (Git Bash on Windows;
`.gitattributes` keeps `*.sh` at LF), and `tauri-apps/tauri-action@v1` bundles `.app` + `.dmg`
and NSIS `.exe` + `.msi`, uploads them as run artifacts on every push and attaches them to a
draft release on a tag; a concurrency group cancels a superseded run; `Swatinem/rust-cache`
covers both workspaces. The engine compiles on Windows behind exactly the two shims the brief
allowed: `crates/ulpf-store/src/store.rs` gains a `#[cfg(windows)]` local `FileExt` whose one
method, `read_exact_at`, loops over `seek_read` and restores the cursor (the store appends
through a `BufWriter`, so there is no positional write to shim), with the unix import now
`#[cfg(unix)]` and no unix line of the store changed; `crates/ulpf/src/syslog.rs`'s
`set_recv_buffer` now takes the socket instead of a raw fd (the `AsRawFd` import moved inside
its `#[cfg(unix)]` body) and is `#[cfg(windows)]` a no-op returning 0 with the asked/granted
line saying so, the caller's warning branch reading `cfg!(windows)` first; the unix behaviour
is unchanged (the lane's verifier read the diff; the suite and soak run 5 ran on it). First green run on both runners 22:34 IST, twelve minutes after the first push; the
feature commit's Windows job then failed once (`E0521` in `menu.rs`: the Windows-only tray
branch borrowed the app handle through `default_window_icon().cloned()`), fixed by building
an owned `Image`; the final run on cdb4d9b (`actions/runs/33980779377`) is green on both,
artifacts `windows-x64-nsis` (5,351,146 bytes), `windows-x64-msi` (7,794,850),
`darwin-aarch64-app` (7,855,749), `darwin-aarch64-dmg` (7,606,904). The CI-built macOS bundle
was launched on this Mac and behaved as the local build. Nobody has launched the Windows
installers: `app/README.md` lists the five checks for the Windows rig (launch with
`server.url` and `/api/status`; drop or Add files shows the events; `heldout/mikrotik.log`
proposes and approves; Quit from the tray, also after closing the window, leaves no
`ulpf.exe`; relaunch keeps the record count and appends to `out.jsonl`). **Anchor.**
`.github/workflows/app.yml`, `.gitattributes`, `crates/ulpf-store/src/store.rs` (`FileExt`),
`crates/ulpf/src/syslog.rs` (`set_recv_buffer`), `app/README.md`. **Principle.** A build
nobody ran is a build, not a verification; say which is which. **Ruled out.** Positional
writes for the store on every platform (changes unix behaviour); making `libc` unix-only
(it compiles unused on Windows); one workflow per OS or a hand-written bundling step
(tauri-action already names the bundles per target and handles the release).

## D75: reserved for lane-5-xml
D75 is the xml strategy and the Windows Event definition (branch `lane-5-xml`, written on that
branch); its entry arrives with the branch.

## D76. The entity index's cost was SQLite's page cache, not the per-value upsert: 64 MiB of cache and one transaction per queue-full
**Decision.** The pivot writer opens its connection with `PRAGMA cache_size = -65536` (64 MiB,
was SQLite's default 2 MiB) and joins everything queued at the moment it wakes into one
transaction (the channel's capacity bounds the group: 64 batches in the engine and in
`rebuild`, was 8, `COMMIT_BATCHES`). Nothing else changed: same tables, same indexes, same
routes, same `ulpf pivot --rebuild`, same single writer, and the proof that the contract
held is byte equality: main's binary and this branch's ran the same slice with `--receipt`
pinned (outputs identical, 643,727,851 bytes), then `ulpf pivot KIND VALUE --output out
--limit 20` for `src_ip fe80::1` (2,225 events), `user jdoe` (52,569 events, 6 devices) and
`dst_port 443` (52,398 events, 9 devices) answered the same header and the same 20 lines from
each binary over its own index, and this branch's `--rebuild` over main's output (1,339,677
postings, 6.78 s) answered the same three again.
**What the profile named (2026-09-06 03:12, `sample <pid> 15` on main's binary at 28,311
events/s, load 13).** The `pivot` thread was busy for every one of its 8,491 samples: 2,332
in the postings `INSERT`, 1,904 and 1,661 in the `entities` and `entity_devices` upserts,
2,117 in `commit`. Inside the inserts the frames were `sqlite3BtreeInsert` →
`sqlite3BtreeIndexMoveto` → `getPageNormal` → `pagerStress` (a dirty page evicted from the
cache is written to the WAL, `pwrite`) and `readDbPage` (the page evicted a moment ago is
read back, `pread`); inside commit, `sqlite3WalCheckpoint` (921 `pwrite`, 374 `pread`: the
auto-checkpoint copying the WAL into the main file after every commit) and
`CommitPhaseOne` (430 `pwrite`, 224 `pread`). About 7,900 of the 8,491 samples, 93%, were
in `pwrite`/`pread`; record comparison, cursor moves, page balancing, parameter binding
and the group `HashMap` together were under 7%. The dictionary lookups the brief suspected
are already `HashMap`s (`Writer::devices`, `Writer::parsers`); the entity upsert is already
one per distinct `(kind, value)` per group; the blob is one `Vec` per group. The arithmetic
behind the profile: the slice holds 713,921 distinct `(kind, value)` pairs over 1,339,677
postings (1.5 new values per event, the worst case `gen_bench` builds, D66), three of the
index's B-trees are keyed by value (`postings_kv`, `entities`, `entity_devices`), and a
group of 8 batches lands ~70,000 inserts spread over trees whose leaves run to tens of
thousands of 4 KiB pages, against a 500-page cache: every page is spilled and re-read
several times per transaction, then copied once more by the checkpoint. The sys time said
the same from outside: 8.6 s of a 15 s run.
**Numbers (this Mac, five lanes building alongside; the one-minute load is beside every
number; slice = the first 150,000,000 bytes of `bench/mixed-5000000.log`, 467,982 events;
`ulpf run slice --store S --output out.jsonl --pivot on|off --infer-threshold 0`, fresh store
and output each run, three runs, medians).** Main's binary, index on, load 6.8-13.8:
19,367 / 31,118 / 31,455 events/s (median 31,118; wall 24.2 / 15.1 / 14.9 s; sys 10.5 / 8.7 /
8.6 s; RSS 512-542 MB; index 149 MB). Index off, load 12.6-13.0: 205,591 / 199,118 / 143,040
(median 199,118; wall 2.3-3.4 s): on/off 6.4x, D66's number reproduced. This branch,
index on, load 16.6-18.0 (256 MiB cache, the first cut): 65,981 / 60,491 / 62,802 (median
62,802, 2.0x; wall 8.5 / 7.8 / 7.6 s, the extra second past the counter is the writer
draining its last, larger group; sys 1.2 s; RSS 660-691 MB, the cache). 64 MiB cache, load
18.8-20.9: 49,500 / 60,768 / 49,735 (median 49,735; RSS 664-682 MB, the same, so 64 MiB is
what the slice's dirty set needs and 256 MiB bought nothing here). Back to back at the same
load so the loaded host is not the difference: main 9,497 events/s (load 33-36, sys 15.9 s),
this branch 48,424 (load 29, sys 1.4 s), 5.1x. The realistic case, `corpus/generated/squid/
access.log` x30 (495,000 events, entities repeat): main on 325,109 / 371,248 / 381,554
(median 371,248, load 18.8), main off 392,330 / 396,199 / 411,624 (median 396,199), this
branch on 336,208 / 334,223 / 329,235 (median 334,223, load 15.9): the index costs 6-16%
of a run whose entities repeat, before and after, inside the noise of a host at load 16-19.
**Attribution.** The commit group alone, with the default cache (load 39.6-44.0): 5,519 /
5,032 / 6,317 events/s, sys 15-18 s, worse than main at any load measured: a larger
transaction against a 2 MiB cache spills each page more often, not less. That is why D66's
commit-group tuning found nothing and concluded the cost was cardinality; the cache is
the enabling change and the larger group multiplies it (the cache alone, with the group
of 8, was not measured: a gap). **What remains (profile of this branch, 4 s from 2 s in,
load 17 with a `cargo test` beside it).** The pivot thread is still the bottleneck and still
100% busy, now in CPU: 884 samples in the postings insert (`sqlite3BtreeIndexMoveto`,
record compares, page fetches that hit the cache), 549 and 525 in the two upserts, 351 in
commit, `pwrite`/`pread` gone from the tree. That is three value-keyed B-tree inserts per
distinct `(kind, value)` per group, and the per-distinct-value upsert is the contract (D55:
the entity summary and its device breakdown are queryable while the run is in flight). The
next cut is a schema change, folding `entities` into `entity_devices` and summing at read
time (~17% of the thread by this sample); not taken tonight.
**Anchor.** `CACHE_KIB`, `open_writer` and the drain loop in `PivotWriter::start`,
`crates/ulpf/src/pivot.rs`; `rebuild` passes 64. **Principle.** Profile, then remove what is
unearned (D23, D66): the cost the design earns is one upsert per distinct value; the cost
it did not earn was rewriting every page of three trees several times per transaction.
**Ruled out.** `journal_mode = MEMORY|OFF` (readers need WAL to page while the writer
appends, D55); `mmap_size` (the reads vanished with the cache; nothing left for it to
serve); dropping `postings_k` (append-ordered, cheap, and `related` needs it); a
second writer thread (one SQLite writer per file, D66); the schema change above (a
contract-preserving 17%, deferred so this branch stays two knobs and a proof).
**Amendment (2026-09-06 03:47-04:35, the large case and the soak; the branch merged main at
04:20 so its reader is D81's, its writer unchanged).** The cache is a fix for an index that
fits and a knob for one that does not. The 5M bench (`bench/mixed-5000000.log`, 1,526 MB,
never-repeating values, `ulpf run --pivot on --infer-threshold 0`, fresh store, one run each
because a run is 8-12 minutes; index 1.57 GB at the end) with the 64 MiB cache: 5,000,000
events in 700.1 s, 7,142 events/s, sys 181 s, RSS 1,389 MB, load before 17.3, peak 25.5; with
256 MiB: 499.8 s, 10,005 events/s, sys 127 s, RSS 1,676 MB, load before 6.4, peak 15.0; main's
binary (2 MiB): 1,187,840 events in 228 s, 5,210 events/s, when it was stopped at load 24 (started at
12.2) with this branch's merge build beside it; the run would not have ended before the timer. The output's growth during the 256 MiB run names the shape: 16 MB/s
(~11,800 events/s) in the first minutes, 6 MB/s (~4,500) in the last. The arithmetic: a
group of 64 batches lands ~100,000 inserts into each of three value-keyed trees; while a
tree has fewer leaves than that, the group's dirty set is the tree and fits the cache (the
slice: 149 MB index, 0.6 s of sys); once each tree runs to tens of thousands of leaves,
every random insert dirties its own page, the group's dirty set is the index, and each
commit spills it to the WAL and the checkpoint copies it again: one `pread` and two
`pwrite`s per insert per tree, whatever the cache, until the cache holds the whole index.
**The merged tree (04:20, main's D81 reader over this writer), proof and numbers.** The
merged binary answers `pivot src_ip fe80::1`, `user jdoe` and `dst_port 443` (`--limit 20`)
with the same 20 lines and the same header over main's 03:18 index, this writer's index,
this branch's rebuild of main's output, and the 03:19 answers from each binary (`cmp`). On
the slice at load 29-37 (the workspace tests running beside it): index on 24,345 / 29,637 /
68,602 events/s (median 29,637), off 148,135 / 132,213 / 138,373 (median 138,373); the
ratio at that load is the host's, the best run (68,602, 2.0x) is the branch's.
**The soak, run 6's shape, on this branch (03:55-04:11, `scripts/soak.sh --bin
<branch> --minutes 5 --file-rate 10000 --udp 127.0.0.1:7907 --udp-rate 8000 --tcp
127.0.0.1:7908 --tcp-rate 8000 --listen 127.0.0.1:7906 --out /tmp/l6/soak1`, five lanes
building beside it: load 7.2 at the start, mean 8.7, peak 19.6; run 6 was a quiet host, mean
4.1 peak 8.6).** 9,000,000 sent; UDP 1,431,220 of 2,400,000 received, shortfall 968,780
against a `netstat -s -p udp` full-socket-buffer delta of 968,825 (1,333,308 -> 2,302,133):
kernel drops, none an engine loss; file and TCP exact; framed = stored = emitted = verified
8,031,220, chain ok; engine 8,791 events/s over 913.6 s with 2,454 backpressure blocks (run
6: 7,736 and 2,980); RSS max 899 MB (run 6: 986); index 2.58 GB at the end. Against run 6's
shortfall of 1,332,667 the loss fell by 363,887 datagrams, 27%, on a busier host, and no
further: the metrics frames say why. The engine emitted 25,289 events/s in the first minute
(the whole 26k/s aggregate, queue high-water 3), 19,419 in the second (high-water 64 from
here on), 13,450, 10,166, 9,010, 6,767 in the third to sixth, and 3,300-5,500 through the
403 s drain: the cliff above, reached at about 1.5 million events, a few hundred MB of
index. Every datagram lost was lost after the second minute. **What this settles.** D66's
"cardinality, not commits" was right about the large case for the wrong reason: the cost
is not the per-distinct-value upsert's CPU (the slice's profile above, once the cache holds
the pages, is three B-tree inserts on cached pages) but the page traffic that random keys
into disk-resident B-trees cost, which no cache below the index's size removes. A writer that
never blocks the pipeline is a different feed, not a knob: stage postings in memory as
sorted runs and merge them (an LSM under the same tables and routes), or feed the index
from the output file the way `rebuild` does so that it lags, reports its lag on
`/api/status`, and the pipeline runs at index-off speed (a UDP listener behind it then
keeps up, D62). Either changes D55's "fed per batch by copied values"; neither is tonight's.
The demo rule in PROGRESS A2 stands (TCP or the file path for a device that must keep up,
`serve --pivot off` when it is UDP). **Also ruled out.** `wal_autocheckpoint` raised or off
(saves the checkpoint's copy, at most half the writes in the spill regime, and moves the
index into a WAL of GBs that the D81 reader cannot `mmap`); a cache the size of the index
(2 GB of RSS on a laptop for a cliff moved, not removed); `page_size` 64 KiB (fewer pages,
the same bytes spilled). **The gap left.** The cache alone with the old group of 8 was not
measured, and one run per 5M point is one run; the load is beside every number for that
reason.

## D77. Trust flags are the per-event form of the counters, never a score
**Decision.** A tail row shows, as a compact list of outlined marks, the stages that did not
reach their outcome for that event, read from fields the emitted line already carries:
`ulpf.parse_status` (`no_parser`, or a failure reason), `ulpf.sub_status` (`uncovered`,
`no_match`), `ulpf.time_policies` containing `receipt_fallback`, `ulpf.time_error`,
`class_uid == 0`, the key count of `unmapped`, `ulpf.utf8_lossy`. Nothing is computed on the
hot path and nothing is added to the line: the normalizer has written every one of these since
v0.1, so summing a flag over an output file equals the counter block's number for it
(`no_parser`, `parse_failed`, `sub_uncovered`, `sub_no_match`, `time_from_receipt`,
`time_error`, `class_unknown`, `unmapped_fields`, `utf8_lossy`), which is the test of the
table in `docs/api.md`. One key (`f`) filters the tail to flagged rows. **Anchor.**
`docs/api.md` "Trust flags", `ui/src/state.svelte.js` (`row`), `ui/src/Flags.svelte`,
`crates/ulpf-normalize/src/mapping.rs` (the `ulpf` object). **Principle.** Errors as values
(CLAUDE.md): each outcome is a counted fact with a reason, and a fact is shown as itself.
**Ruled out.** A confidence score (a number ULPF cannot justify: the engine knows which stage
failed, not a probability that the output is right; a score would invite the reader to trust
0.8 more than 0.7 with no basis); a server-side `flags` array on the line (a hot-path
allocation for a derivation the client does in constant time per row); colouring rows by
flag count (a row is tinted because it is in a state, not because it is interesting, D69).

## D78. The emitted line and the export are read from the output file, by raw id, through a snapshot
**Decision.** `GET /api/events/{id}` finds `emitted` in the tail ring first and then in the
JSON Lines output the sink wrote, by a binary search over line starts on the raw id in each
line's `ulpf` object (`crates/ulpf/src/outfile.rs`); `emitted_from` says which. `GET
/api/export` streams the same file from the first line at or after `from` to the last at or
before `to`, filtered by terms with the rule the Live screen's filter uses, as JSON Lines
verbatim or as the eleven Parquet columns in CSV (D64; the five entity columns come from the
schema's entity paths, so the CSV means the same thing under ocsf and ecs). Both open the
file read-only and bound every read to the bytes on disk when they opened it, cut to the
last line terminator, so a line the writer is mid-way through is never returned; the store
is never opened (D42) and the tail is never the source of an export (its ring holds the
newest thousand lines). The output is in raw id order by construction (D60), which is what
turns a lookup into a binary search. The line's raw id is read from `"ulpf":{...,"raw_id":N}`
with two substring searches rather than a JSON parse, because a search reads about twenty
lines and one of them may be 4 MB. `?bytes=0` on the traceback leaves `text` and `hex` null
and `GET /api/events/{id}/bytes` serves the record's exact bytes as an octet stream: a client
that renders the raw record from bytes (the byte ruler already does) fetches the record's own
size instead of a JSON body six times larger. **Anchor.** `crates/ulpf/src/outfile.rs`,
`Live::traceback_with`, `Live::raw_bytes`, `Live::emitted_from_output`, the `export` and
`traceback_bytes` handlers in `crates/ulpf/src/server.rs`, `docs/api.md` v4. **Principle.**
Raw before understanding and one writer: a read never touches the writer's handle or the
store; the file the sink wrote is the record of what was emitted, and reading it is cheaper
and more honest than re-parsing. **Ruled out.** Re-parsing the stored record through the
current parsers to reconstruct `emitted` (that is `now`, a different answer whenever a parser
changed); a line-offset index beside the output (a second derived file to keep in step, for a
lookup that is already logarithmic); serving the export from the tail ring (bounded to a
thousand lines) or from a re-run of the pipeline (re-parsing, and a second pass over the
store); a per-event `flags` field for the export filter (the filter is a substring rule over
the line text, the same on both sides).

## D80. Three definitions on existing strategies, no JSON catch-all, the containers at priority 0
**Decision.** `parsers/cef.toml`, `parsers/leef.toml` and `parsers/cloudtrail.toml` are written
from their specifications (ArcSight CEF, IBM LEEF v2, AWS CloudTrail record contents; each
header cites the page and the fetch date) on the cef, leef and json strategies the engine
already had; `mappings/ocsf.toml` and `mappings/ecs.toml` gain their source names, an `API
Activity` class (6003, activity Read when the record says `readOnly = true`, else 0 Unknown,
because the record does not say whether a write is Create, Update or Delete and the schema
marks `activity_id` required), and the alternatives that classify Zeek's http and conn rows
once a proposal names their columns. The samples are the specifications' own examples, and
`samples/README.md` says so. **Anchor.** The three definitions, the two mappings' `[fields]`
and `[[class]]` additions, `fixtures/{cef,leef,cloudtrail}.expected.jsonl`,
`crates/ulpf-parse/tests/alloc.rs` (cef and leef in the zero-allocation list). **Principle.**
A parser is written from the format's own document; a mapping is additive; the wall holds
(no schema name in a definition, no vendor in a mapping). **Ruled out.** A generic JSON
catch-all (`json_generic` at priority -2 claiming any `{` line): it would detect the Zeek
JSON files that are the live inference demo instead of proposing them, and a parsed but
unnamed JSON event teaches the mapping nothing; only cloudtrail (matched on its five
never-null top-level keys) and suricata_eve claim JSON lines. Priority -1 for cef and leef
(where D45 puts generated parsers): a generated parser named before `cef` with the `.`
fallback matcher would take CEF lines, and no CEF line could take the hinted fast path; the
containers sit at 0 with matchers that require the format's own header before any pipe,
which no vendor sample carries (the fixture test proves every existing sample still detects
as itself), and a vendor definition for a CEF-speaking device declares `priority = 1`.
Inferring the CloudTrail activity from the `eventName` verb (an open-ended per-service
vocabulary in the mapping). nginx and Apache definitions: the first post-demo addition,
because nginx is named as an unseen format for the live inference demo and a hand-written
parser hours before it would remove the demo's unknown input. Postfix: held, no mail
vocabulary in either schema and not a perimeter device. `sev` heads the `severity` and
`log.level` source lists so a LEEF line behind a syslog `<pri>` keeps the device's own
severity (first-present wins; no existing parser emits `sev`, and the twelve original samples
are byte-identical through the old and new mappings). Two engine defects the lane found and
could not fix in a definition are on branch `lane-3b-cef-leef` (CEF's header severity is
named `severity`, the syslog scale's name, so its 0-10 scale is canonicalised backwards; a
LEEF 2.0 delimiter written `0xHH` splits on the literal `0` with no counted failure); they
are named in the two headers and merge after the demo.

## D79. Motion shows the truth of the system or it does not exist
**Decision.** Motion is allowed exactly where it reports a state change or the movement of
data through the machine, and forbidden as decoration. Flow (`ui/src/Flow.svelte`, `#/` and
`#/flow`, key `0`, Esc from any top-level screen) draws the six stations on one line with
the inference branch and the pending tray under detect and the chain under preserve; every
number is a value the API returned and every motion is driven by one. The pulse on a link is
one element (a repeating 6 px dash every 32 px) moved by one Web Animations translate, looped;
each 500 ms frame sets its playback rate from that link's own rate as `px/s = 16·log10(1 +
events/s)` (1/s crawls at 5 px/s, 100/s at 32, 10,000/s at 64, 400,000/s at 90), so six moving
elements cover any rate and a speed change never jumps; at rate 0 the dashes fade over `--d2`
and the track stays. The rate is that stage's counter delta between the last two frames over
their interval, or the server's `rate`/`queue` window when the frame carries one, and the
label under the number says which. Tokens: `--d1` 120 ms (a value or badge changed), `--d2`
240 ms (a screen arrived, a result replaced a confirmation, the queue bar, a chain mark, the
branch lighting), `--ease cubic-bezier(.2,0,0,1)`, `--pulse` 6 px, `--pitch` 32 px. A
screen fades in over `--d2` only after the first hash change; a count badge pops over `--d1`
by a keyed re-mount, gated so the first frame's counts and a screen's opening state appear
still; the approve result, a verify verdict that lands after the screen opened and a drift
state that changes get the same one change. A selection moving (h/l on Flow, j/k on a list)
is the reader's own action and reports nothing, so it snaps everywhere. `prefers-reduced-motion`
turns every transition and animation off in one stylesheet rule and stops the script
animations from being created: the diagram stands with the same numbers. Station to screen:
ingest opens Live, preserve opens Integrity, detect opens Drift (detection is per source),
the branch and the tray open Review (nothing is parsed until a human approves), parse opens
the newest record's Traceback, normalize opens Pivot (the entity index is built from
normalized paths), emit opens Replay (emit writes v1, Replay the next version). **Anchor.**
`ui/src/Flow.svelte`, `ui/src/keys.js` (`stations()`, `reduced()`), the `/* ---- flow ---- */`
section of `ui/src/app.css`, `docs/design.md` Motion section, `docs/screens/flow-*`.
**Principle.** The counters are the product; a screen that moves when nothing changed is
lying about the machine (D69: no decoration). **Ruled out.** One DOM element per event
(400,000 events in 25 s is 16,000 nodes a second; a visible tab drops frames and a hidden one
queues them); a canvas particle system (a second rendering model beside the DOM, script per
frame on the main thread, no tokens, invisible to reduced-motion and the theme); motion on
hover (reports nothing that changed; the station's border-colour transition was removed for
this reason); a selection ease (the same property as hover, and every other selection in the
app snaps); a hero animation on load (decoration by definition, and the first frame's counts
would move without having changed).

## D81. A pivot names the cost of each part of its answer, and `related` reads the index the way the input is read
**Decision.** Every pivot page carries `elapsed_ms { header, timeline, related, lines, total }`
(`crates/ulpf/src/pivot.rs`, `Elapsed`), so a slow pivot says which part was slow instead of
being "about 500 ms" (a figure the record had never measured: on a 233,854-event slice the
busiest user answered in 93 ms quiet and 239 ms at load 30, and `related` was 78-98% of it).
The read side opens its connections with `SQLITE_OPEN_NO_MUTEX` and `mmap_size` 1 GiB
(`open_reader`): rusqlite's `Connection` is not `Sync`, so SQLite's serialized mode bought
nothing and cost a fifth of a scan in mutex calls, and the 2 MB page cache re-read every page
of a 30 MB scan through `pread` and a copy. `related` scans the four other kinds on four
read-only connections opened once, under `std::thread::scope` (a panicked scan is an error
value); blobs and values are borrowed from the row and a value is copied only on a hit;
membership in the window is one bit per id over the window's span. Pages are byte-identical
to before (eleven pages over six entities, every second page through the cursor, `cmp`), and
the controlled pair on a quiet machine reads 2.6-3.3x (jdoe 93 -> 29 ms, dst_port 443 89 ->
33, src_ip 74 -> 28); the loaded end (load 28-36) read 4-8x because the old path's mutex and
`pread` lose more under contention. **Anchor.** `open_reader`, `PivotIndex::open`,
`related`, `scan_related` in `crates/ulpf/src/pivot.rs`; `docs/api.md` v4 (`elapsed_ms`).
**Principle.** Inputs are memory-mapped and read without copies (CLAUDE.md); the index is an
input on the read side. A number on screen is measured, and a slow answer names its cause.
**Ruled out.** A larger `cache_size` (heap per connection, still a copy per page); a thread
pool or async for the four scans (the query already runs under `Live`'s index mutex; four
scoped threads cost about 100 µs); `HashSet<u64>` membership (SipHash and two allocations per
row, 15% of the profile) or a sorted `Vec` with binary search (fourteen compares against one
shift); a covering index `(kind, first_id, last_id, value, blob)` or a `WITHOUT ROWID` table
clustered by `(kind, first_id)`, which would remove the table seek per posting row that is
now 55% of the scan's CPU but doubles the writer's work (D66: the index thread is `serve`'s
throughput cap) and changes the layout of a file older serves still open; a
`first_id..last_id` pre-check before decoding (98% of rows hold one posting on the bench,
so the extra column read costs more than the decodes it saves); RELATED_WINDOW and
RELATED_ROW_BUDGET unchanged, because changing them changes `related_over`.

## D82. Windows correctness: the store never shrinks a file, stop closes every handle, a device output leaves nothing behind
**Decision.** Three faults a Windows tester reproduced against 14d3b0c, each fixed where
every caller routes through, on branch `lane-8-windows` (merged after the demo on the owner's
go; a store written by main opens here unchanged and vice versa, the on-disk format is
untouched). (1) *Recovery reclaims a torn tail in place.* `RawStore::open` used to
`set_len` both files to the recovered end; Windows refuses that while any process has the
file mapped (`SetEndOfFile` → `ERROR_USER_MAPPED_FILE`, os error 1224), which is what the
round-trip test does by holding a `RawReader` across the reopen, and on POSIX a reader
mapped across the shrink faults on its next access past the new end (a latent bug the Mac
never showed). `recover` now overwrites the bytes between the recovered end and the file
end with zeros and the writer resumes at the recovered end, so the next append overwrites
them; no file is ever shrunk. The writer's walk-back now also checks the last entry's digest
and chain link, not only its shape, so a torn write (a complete header over a half-written
body, a complete offset beside a half-written chain) can never become the head.
`RawReader::open` no longer takes the file length as the record count: it drops trailing
entries that are not a record (the zero region, an entry pointing past the segment) and
keeps a record whose digest or chain is wrong for `verify` to name, so a tamper of the last
record is still reported and never silently dropped. `RawStore::reader` still bounds the
snapshot to the writer's flushed count (D52, D56 survive unchanged: the snapshot is a
prefix of validated entries, appends after it are invisible, nothing is written). (2)
*Stop closes what it opened.* `Live` owned the store for its whole life, so after `serve`
returned the segment, the index, the catalogue and its WAL were still open in a process
that kept the `Arc<Live>` (the Parquet watch-mode test, the desktop shell if it ever
embedded the engine); Unix let the temp directory go, Windows returned os error 32. `Live`
now holds `Mutex<Option<RawStore>>` behind `Live::store()` (an error once closed, never a
panic) and `Live::close()` runs on every exit path of `run` and `serve`, dropping the store
and the pivot index's read connection; a request racing shutdown gets "the store is
closed". (3) *A device output leaves nothing behind and the meta counts the file.*
`output_is_sink` knew `-` and `/dev/*` only, so `--output NUL` on Windows wrote
`NUL.v1.meta.json` into the working directory; it now recognises `NUL`, `\\.\NUL` and
`\\?\NUL` on Windows only. The live meta's `events` was written as 0 at open and never
updated while the counter block said a million; `report` now writes it as what the run
emitted when the output started empty (`Live::output_start`, the file length at the first
write, is 0) and as the file's line count otherwise (a restart appended to an earlier
run's lines), and `Versions::list` counts v1 from the file always, since the live meta is
only right at the last clean stop. **Anchor.** `recover`, `zero_tail`, `RawReader::open`,
`RawReader::get` in `crates/ulpf-store/src/store.rs`;
`a_reader_mapped_across_a_reopen_keeps_its_records_and_no_file_shrinks` in
`crates/ulpf-store/tests/roundtrip.rs`; `StoreGuard`, `Live::store`, `Live::close`,
`output_is_sink`, `is_nul`, `report`, `output_start` in `crates/ulpf/src/engine.rs`;
`Versions::list` in `crates/ulpf/src/replay.rs`;
`stop_releases_every_file_the_engine_opened` in `crates/ulpf/tests/stop.rs` (counts this
process's descriptors under the temp directory through `F_GETPATH` on macOS and
`/proc/self/fd` on Linux, and removes the directory, which is the check Windows makes);
`a_null_device_output_leaves_nothing_beside_it_or_in_the_cwd` and
`the_version_meta_counts_the_lines_the_output_holds` in `crates/ulpf/tests/output_meta.rs`;
`.github/workflows/windows-tests.yml` (the whole suite on `windows-latest` on every push to
the branch). **Principle.** Immutability as a property of the interface (D7): the only
bytes recovery touches are bytes that were never a record, and the logical end of the store
is what the entries prove, never what the filesystem reports; stop means stopped on every
platform, and a counter the meta disagrees with is a bug wherever it appears. **Ruled out.**
Dropping and remapping every reader across a reopen (the store cannot see readers in other
processes, `ulpf verify` beside a starting `run` being the real case; and it would leave the
POSIX fault in place); a reader walk-back that also drops a record with a wrong digest or
chain (a tampered last record would vanish from `verify` instead of being named); a
running line count kept in the meta across runs (a killed run leaves it stale, and the
scan runs only when an earlier run's lines are already in the file); `Option` inside
`RawStore` instead of inside `Live` (every store method would grow a closed branch for a
state only the engine creates). **Also on the branch.** The Windows run after the three fixes
named a fourth failure outside the tester's report: the syslog test received 2010 of 3000
loopback datagrams because the Windows `set_recv_buffer` was the no-op D74 allowed (the
socket stayed at the 64 KiB default). It now makes the same two calls as unix through
Winsock's `setsockopt`/`getsockopt` (`ws2_32`, which std already links; no new dependency,
the unix path untouched), and the test's timeout prints the counters as they stood. Runs:
33997160230 (baseline: roundtrip 1224, parquet teardown 32, syslog timeout), 33997927604
(after the three fixes: the stop test's "open while serving" check on a platform that cannot
list descriptors, and the syslog loss with its counters), 33998281457 (green, every
target). **Not done.** A verify or replay thread still running when `serve` returns is not
joined (both are bounded and finish on their own; a cancel-and-join at stop is a separate
change). Nobody has run the branch on a Windows machine by hand; the CI runner is the
evidence.

## D83: reserved
D83 is directory-level include/exclude for `run` and `serve` (not built; open item 3 in PROGRESS v4).

## D67, amended: the runner is a subcommand
`ulpf demo [--auto] [--check] [--reset] [--dir demo] [--listen 127.0.0.1:7878] [--syslog
127.0.0.1:5514] [--repo .]` (`crates/ulpf/src/demo.rs`) plays the demo section of PROGRESS.md
from the binary, and `scripts/demo.sh` is a wrapper that finds the binary and hands the flags
over, because the team records on a Windows machine where no shell runs the script and a demo
that plays on one laptop is one laptop away from not happening. It adds no engine behaviour:
it spawns `current_exe() serve` with `--parsers demo/parsers --pending demo/pending`, copies
files into the watch directory, speaks HTTP/1.1 to localhost over a `TcpStream` (the shape of
`crates/ulpf/tests/server.rs`, so the binary gains no HTTP client), runs verify and attest as
children, and kills the server at the end because a killed run restarts to the same output
and store as an uninterrupted one (D59). `--check` keeps and widens the old grep: the fourteen step
headings and the seventeen command strings are constants the runner prints, and each must
appear verbatim in the demo section (the inputs and both ports are checked too); `cargo test`
asserts the same, so drift fails the suite. Requests the demo makes on stage (approve,
traceback, replay) print their failure where the answer would go instead of aborting; setup
failures stop with a reason and exit 1. The reset also removes any generated parser (`origin =
"inferred"`) from the repo's `parsers/`: a CLI approve writes it there, and a demo copy or a
bundle made after it knows the unseen format already, so the demo cannot raise a proposal (a
Windows tester hit exactly this). **Ruled out.** A PowerShell twin of the script (two runners
drift, which is the failure D67 exists to prevent); a `--demo` flag on `serve` (engine
behaviour for the demo's sake); shelling out to `sh -c` for the copies (the same portability
hole one level down); `?` on every request (correct for a script, wrong for a stage);
loosening the check to a fuzzy match (a match that tolerates an edit proves nothing).

## D84. One Intensity setting with three named choices, applied by restarting the engine
**Decision.** The desktop app exposes the engine's `-j` and `--pivot` as one setting with
three choices whose labels carry this machine's own numbers from `available_parallelism`:
`Low · 2 of 8 cores · entity index off` (one core under four cores, else two), `Balanced ·
4 of 8 cores · entity index on` (half; the default a fresh install and an unreadable settings
file both get), `Max · 7 of 8 cores · entity index on` (all but one, the engine's own
default). The choice becomes `-j N --pivot on|off` on the serve line and is kept as one word
in `app_config_dir/intensity` beside the `data_dir` override. Changing it restarts the
sidecar: the child is killed as Quit kills it (D59 makes it safe), a fresh free port, the
same store; the page that is up says `Restarting the engine at Low: 2 of 8 cores, entity
index off` and then `Engine ready at ...` (1.18 s end to end on this Mac). The title quotes
the running engine, not the file: `ULPF · engine ok · N events · M pending · Balanced · 4 of
8 cores · index on` from `/api/status` (`threads`, `pivot_index`), and `restarting` when the
two disagree. **Anchor.** `app/src-tauri/src/intensity.rs`, the `Intensity` submenu in
`app/src-tauri/src/menu.rs`, `intensity_part` in `app/src-tauri/src/title.rs`, the Intensity
section of `app/README.md`, `docs/screens/app-intensity-*.png`. **Principle.** A number a
person quotes is the number the machine is running; a setting names what it costs on this
machine. **Ruled out.** Two independent controls (a typed thread count with the index left
on is a machine at full tilt at a tenth of its throughput, D66, and a number typed by someone
who does not know the core count); Max as the default (right for a headless run, wrong for a
demo machine also driving a browser and a recording); a live change (the engine fixes its
worker count when it builds the pool and takes the index switch at start by design, D40,
D60, D66, so a live control would lie or the engine would grow the reconfiguration path the
one-sequencer design exists to avoid); printing the setting in the title (it would claim 7
cores during the second the old 2-core engine still answers); a tray copy of the submenu
(two sets of check marks for one choice).

## D85. One draft release carries the CLI and the installers, and the CLI job waits for the bundle job
**Decision.** A `v*` tag produces exactly one draft release. `tauri-action` creates it (D74);
the `cli` matrix job builds the bare binary for `x86_64-unknown-linux-musl`,
`aarch64-apple-darwin` and `x86_64-pc-windows-msvc`, strips it in the naming step (the musl
binary was 57,032,312 bytes with debug info, 8,904,048 stripped) and proves it static with
`file` and `ldd` on the file that ships; a `release` job with `needs: [bundle, cli]` on tags
downloads them, writes `SHA256SUMS` and runs `gh release upload <tag> dist/* --clobber`. The
`needs` is what guarantees the draft exists before the upload. The draft is not published by
CI; a draft is invisible without a login, so the README links the release page and says the
owner publishes it. **Anchor.** `.github/workflows/app.yml` jobs `cli` and `release`;
`v0.1.0-rc1` (run 33995222954, eight assets). **Principle.** One place a stranger downloads
from, and every artifact on it is the one the job proved. **Ruled out.** A separate
`release.yml` (no cross-workflow `needs`, so two workflows would race to create two
releases); `tauri-action`'s own plain-binary upload (it ships the app's binary, not a CLI
matrix); `softprops/action-gh-release` (a second action for what `gh` does in one line).

## D86. The Windows smoke job is the proof the binary runs where no human has launched it (D74 amended)
**Decision.** `smoke-windows` runs on `windows-latest` on every push: `cargo build --release
-p ulpf`, then in PowerShell `ulpf.exe check` (asserts `parsers` and `0 problems`), `ulpf.exe
run samples ...` (asserts `framed` equals the non-empty line count of `samples/*` minus the
one collector-folded FortiGate continuation, so the invariant moves with the tree instead of
a hard-coded 304), `ulpf.exe verify`, then `Start-Process` on `ulpf.exe serve`, `/api/status`
polled up to 30 s for a `version`, a sample copied into the watch directory, `/api/metrics`
polled until `engine.emitted` covers it, `Stop-Process`, the pid asserted gone. Every
assertion prints `::error::` and exits 1, and each script sets `$ErrorActionPreference`
itself so pwsh's default cannot turn a native command's stderr into a thrown error. The
first run of the engine on Windows: identical stage counts to macOS. D74 said a build nobody
ran is a build, not a verification; this converts the Windows CLI build into the second (the
installers are launched by lane 7's `app-smoke-windows`, which found the orphaned sidecar
that 7B's job object answers). **Anchor.** `.github/workflows/app.yml` job `smoke-windows`.
**Ruled out.** A hard-coded framed count (stale on the next sample); shelling out through
bash on Windows (the point is the path a Windows user takes); a Linux or macOS smoke job
(every local run and the harness already exercise both).

## D87. One headline number; every other figure says what it measures (D62 and D66 amended)
**Decision.** ULPF quotes one throughput figure: 258,411 events/s, ingest through JSON Lines
on disk, seven worker threads, M1 Pro, median of three, from the committed neutral-harness
scorecard `eval/results/ulpf-20260905T140426Z-33371/scorecard.md` (2026-09-05), beside
264/264 correctness. Other figures appear once each, labelled with their thread count and
output mode: the discarded-output figure (`--output /dev/null`, `-j 7`), the `-j 1` figure
on the same file, the rate with the entity index on (measured with its command, `-j 7`,
output written, `--pivot on` 33,537 against `--pivot off` 322,733 on the 497,607-line
slice). README's "Honest numbers" is the one place they live; the paragraph that printed the
harness figure and the `/dev/null` figure two lines apart, where they read as competing, is
gone. A sample run's events/s is labelled startup noise. **Anchor.** README "Honest
numbers"; the step 11 comment of the PROGRESS demo. **Ruled out.** The higher `/dev/null`
number as the headline (it is the one no other tool's harness figure is comparable to); a
range instead of a median (a range invites the top of it); dropping the `/dev/null` figure
(it is the honest engine measurement the thread-scaling table is built from).

## D88. Two build profiles: `release` builds anywhere, `dist` ships
**Decision.** `[profile.release]` drops LTO (`lto = false`, default codegen units, `debug =
1` unchanged); `[profile.dist]` inherits release with `lto = "fat"` and `codegen-units = 1`,
exactly what release carried, and is what the Docker image contains and what
the harness measures (`eval/tools/ulpf.toml` builds it, `eval/run.sh` prints the build it
declared, `docs/evaluation.md` says which build the numbers come from). Evidence they are the
same build: the pre-split release binary was 8,777,448 bytes, the dist binary 8,777,544 on a
tree that has moved, the new default 11,778,856 (+34%, no LTO); `-v` shows `-C lto=fat -C
codegen-units=1` on dist and neither flag on release. Measured on this M1 Pro in a quiet
window the two are within noise (best-of-8 on a 500k slice at `-j 7`: dist 1.791 s, release
1.690 s; medians 2.120 against 2.164). The demo stays on `target/release`: it is the build a
viewer reproduces in a minute and the gap is noise. **Anchor.** `Cargo.toml`, `Dockerfile`
(`--profile dist`, `target/dist/ulpf`), `eval/tools/ulpf.toml`, `eval/run.sh`,
`docs/evaluation.md`. **Principle.** A build a tester can reproduce on any machine is worth
more than any throughput the default path buys; the shipped number is labelled with the
profile that made it. **Ruled out.** One profile with fat LTO for everyone (a Windows tester
on a 16 GB box got a successful release build one time in four, and a minute every time with
LTO off: a first build that fails three times in four is a broken front door); thin LTO on
the default (nothing measured for it to recover); `dist` as the default with `release` opt-in
(the default is what a stranger, README, `scripts/demo.sh` and the cold-start criterion
type, and every `target/release` path stays correct). Cargo offers no alias for a custom
profile and no stable output path across profiles, so every shipping caller spells
`--profile dist` and `target/dist/` (Dockerfile, `eval/run.sh`, `eval/tools/ulpf.toml`, and
since lane 7C the `cli` and `bundle` jobs of `.github/workflows/app.yml`; that workflow's
`smoke-windows` job stays on `--release` deliberately, because it proves the Windows code
paths and not the shipped bits).

## D89. Windows is a first-class target: the installer carries its runtime, a failed start is a sentence, the log has a name
**Decision.** `bundle.windows.webviewInstallMode` is `offlineInstaller`: the NSIS installer
carries the full WebView2 runtime (about 127 MB in the docs, 267 MB measured) so a machine
needs no network at install time and nothing is fetched at runtime; the sidecar is found
beside the installed executable as `ulpf.exe` (the bundler strips the triple), the data
directory under `%APPDATA%` through `app_data_dir()`, and the three Windows differences in
the ingest path (separators joined by `PathBuf`, a rename that never crosses a volume, a
path with spaces passed as one argument because the engine is spawned with `args`, not a
shell) are named where they matter. A failed start is a sentence on the splash, then the way
out, then where the output is: `engine missing` (reinstalling replaces it), `port in use`
(the port, and `ULPF_APP_PORT` to move it), `The engine stopped (exit N). Its last words: ...
The whole of its output is in <data>/engine.log` (the sidecar's stderr, truncated per start;
the store and output are the durable record). The README's Windows section answers from the
installers, not from memory: the release page (a draft until the owner publishes it), the
SmartScreen dialog text and clicks, the prerequisites checked against Tauri's page, a
`sidecar.ps1`, and the five human checks in the click order the video follows. CI's
`app-smoke-windows` installs the NSIS build silently, launches it from the installed
location, waits for `server.url` and `/api/status`, and prints which path it achieved; its
first run found `ulpf.exe` outliving a `Stop-Process` of the window, the fact 7B's job
object answers (the decision lane 7C records at its merge). **Anchor.**
`app/src-tauri/tauri.conf.json`, `start` and `SPLASH` in `app/src-tauri/src/lib.rs`,
`app/src-tauri/src/ingest.rs`, `app/scripts/sidecar.ps1`,
`app/scripts/smoke-windows.ps1`, `app/README.md`, `docs/screens/app-error-*.png`. **Ruled
out.** `downloadBootstrapper` (the default; fetches at install time, and a demo machine may
be offline); a blank window or a spinner on a failed start (the tester saw one); a CI
artifact as the download (invisible without a login and gone in ninety days).

## D90. The UI reads the v4 contract: flags as marks, one filter rule shared with the export, the record's bytes from the bytes route
**Decision.** Trust flags (D77) are derived once at flatten time in `row()` from the fields
`docs/api.md` names and rendered by `Flags.svelte` as two-letter outlined mono marks in a 7em
tail column (`np pf su sn tr te cu um`N `u8`), every mark the same `--warn` colour because
every mark is the same kind of fact; `f` shows only flagged rows and the head counts them.
The filter takes space-separated terms, each a case-insensitive substring of the whole line
held as one lowercased string per row (the first 64 KiB: a 4 MB record kept whole would put
2 GB in a full tail), which is the export route's own rule, so the export of a filtered view
(`e`: jsonl or csv, this view's raw id range or everything) is the view; it writes nothing,
so it is a choice, not a confirmation. The traceback asks `?bytes=0&values=4096` and reads
`/api/events/{id}/bytes` as an `ArrayBuffer`: on the 4,000,001-byte record the old client
pulled one 28,001,884-byte JSON and painted the ruler in 1,267 ms (median of three), this one
pulls 18,283 bytes of JSON plus the 4,000,001 bytes and paints in 62-75 ms, names where the
emitted line came from and what was cut; an older server without the route still answers
with `hex` and is decoded as before. Live's two large numbers are the windowed rates with the
window in the label and the run average beside them; the queue bar is the depth now with the
high-water mark as a rule across it; the status line carries the depth on every screen;
Pivot's seen-with reads "in N of the M newest events". **Anchor.** `ui/src/Flags.svelte`,
`ui/src/state.svelte.js` (`flagsOf`, `row`), `ui/src/Live.svelte`, `ui/src/Traceback.svelte`,
the `/* ---- v4: flags, filter, export ---- */` section of `ui/src/app.css`,
`docs/screens/v4-*.png`. **Ruled out.** Spelling the flag names in the row (thirty em the tail
has not got) or any per-row colour or score (D77); deriving flags per render (nine fields per
visible row per frame); the old field-by-field filter (cannot promise the export is the
view); `Confirm.svelte` for the export (reserved for actions that write); a third large rate
in the hero (it squeezed the funnel's loss labels at 1280).

## D91. A documented command names its input files, not a directory
**Decision.** Every command in README.md and scripts/README.md names its log files
(`samples/*.log`), never the bare `samples` directory. The engine reads every file in a
directory it is given, so a bare `samples` ingests `samples/README.md` as a log: 16 files and
354 events instead of 15 and 309, `no_parser` 41 instead of 2, `class_unknown` 106 instead of
62, `unmapped_fields` 3036 instead of 3025, `time_from_receipt` 55 instead of 10. Measured
also, and against the brief's premise: the inference engine already refuses that input — at
the default threshold and at `--infer-threshold 8` the 41 unknown lines give `runs 1  lines
templated 0  unmatched 39  proposals written 0  skipped [no_templates 1]` and `pending/` stays
empty. The cost is the counter block a reader is asked to trust, not the review queue. Two
places a shell cannot fix keep the directory and name the consequence where it is read: the
container command (the `scratch` runtime image has no shell to expand a glob, and a host-side
glob resolves against host paths) and `scripts/isolation.sh`'s `ULPF_FEED` fallback (it copies
a directory whole; isolation is a socket verdict, so the extra events change no verdict).
**Anchor.** README "Run it", "Quick start" and its Windows block, the Isolation paragraph;
`scripts/README.md`. `scripts/coverage.sh` already iterates `samples/*.log` and `find
corpus/... ! -name '*.md'`, so `docs/coverage.md` never carried a README row and was not
regenerated. **Principle.** A documented command names its inputs. A counter block in a README
is a claim about the tool, so the command that produced it may not quietly include a file that
is not a log. **Ruled out.** Teaching the engine to skip non-log files tonight — an extension
test or content sniff in the ingest path is an engine change hours before the demo, and it
would make the counter block depend on a filename heuristic instead of on what was fed in; the
directory-level include/exclude option stays D83, post-demo. Moving `samples/README.md` out of
`samples/` — it is the file a teammate reads before adding a sample, and relocating it hides
the sharp edge instead of deciding about it.

## D92. On Windows the kernel owns the sidecar's lifetime, not the app's exit path
**Decision.** The engine is spawned into a job object created with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, whose handle the app holds for its whole life
(`app/src-tauri/src/job.rs`, `windows-sys` under `cfg(windows)`; the assignment happens
immediately after the spawn, before the app waits for `server.url`). When the last handle
closes — a clean quit, a panic, End task on the window, `Stop-Process -Force` — the kernel
terminates every process in the job. Windows has no process group, so before this a force
kill left `ulpf.exe` alive holding the store's exclusive SQLite lock and the next launch was
refused with the message D93 now explains. `app-smoke-windows` force-kills the window
process and fails with `::error::` if any `ulpf.exe` survives; the log line prints the
elapsed milliseconds (500 in both green runs) against a 5 s ceiling. **Anchor.**
`app/src-tauri/src/job.rs`, the spawn in `app/src-tauri/src/lib.rs`,
`app/scripts/smoke-windows.ps1`, the `app-smoke-windows` job in
`.github/workflows/app.yml`. **Ruled out.** `taskkill /T` on quit — it runs in the exit path,
which is exactly what a force kill or a crash skips, so it answers every case except the ones
that produced the orphan. The sidecar polling its parent's pid and exiting when it changes —
a second lifetime mechanism inside the engine, added for the app's sake, on the binary whose
hot path is the product; and pid reuse makes the poll wrong rather than merely late. macOS
needs neither: the sidecar is a direct child and dies with its parent.

## D93. A store another writer holds is a sentence with a button, not a dead window
**Decision.** The engine allows one writer, so a second `ulpf serve` on the same store exits
with `is in use by another process`. The app matches that message, finds the holder by
command line (`ps` on macOS, `Get-CimInstance Win32_Process` on Windows —
`app/src-tauri/src/holder.rs`; by command line because the lock is the process and the file
that would record its pid is the locked one), and renders one row on the splash naming the
pid, with a single button that stops that process and starts the engine again through the
ordinary start path. Captured at `docs/screens/app-error-locked.png`. **Anchor.**
`app/src-tauri/src/holder.rs`, the error branch in `app/src-tauri/src/lib.rs`, the
locked-store paragraph in `app/README.md`. **Ruled out.** Letting it fall into the generic
"the engine stopped" — the operator sees a dead app and no cause, and the one thing they must
do is invisible. Killing the holder automatically on launch — the holder may be a deliberate
`ulpf run` mid-ingest, and an app that terminates another writer without being asked breaks
the single-writer invariant's whole point: that the second writer is refused, not that the
first one is expendable.

## D94. A generated parser is excluded by what it declares, at all three places it could leak
**Decision.** An approved proposal writes a parser with `origin = "inferred"` into `parsers/`,
and a bundle built after that ships it as a resource — the app then arrives already knowing
the "unseen" format and the inference demo has nothing to propose. Excluded at three points:
`app/scripts/sidecar.sh` and `sidecar.ps1` refuse before the bundle, naming the offending file
and `ulpf demo --reset`; the app's first-run copy of the bundled definitions skips any such
file and logs how many it skipped to `engine.log`; and `app/README.md`'s demo note says so for
the human. The test is a line that starts with `origin` and contains `inferred`, not the word
anywhere in the file, so a hand-written parser whose description mentions inference is not a
refusal. **Anchor.** `app/scripts/sidecar.sh`, `app/scripts/sidecar.ps1`, the first-run copy in
`app/src-tauri/src/lib.rs`, the "never ship a generated parser" note in `app/README.md`.
**Ruled out.** Matching the name pattern `*_inferred` — the engine names a proposal after the
source, not with a suffix, and any rename defeats the check; the file already declares what it
is, so read the declaration. Trusting the operator to run `ulpf demo --reset` before building —
the bundle step is where the leak becomes an artifact somebody installs, so that is where the
refusal belongs.

## D95. The tail frame's `cut` is additive: `skipped` keeps meaning "did not receive"
**Decision.** `TailFrame` carries a new `cut` beside `skipped`, set by `since()` and published
as `"cut"` in the server's tail JSON. `skipped` keeps the meaning every consumer built against
`docs/api.md` already has — the total a caller did not receive — and `cut` names the part of it
the frame's own 200-row per-tick limit left behind, which is still in the ring and arrives on the
next frame. `skipped - cut` is therefore the eviction: the only part that is gone. The footer
says so in prose, and the streaming sentence in `docs/api.md` that described `skipped` as
eviction (the source of the misleading tooltip) is corrected. Measured over real HTTP on a
64-event ring fed 309 events: `?limit=500` gives cut 0 and skipped 309-1-64, `?limit=8` gives
cut 56 with skipped == evicted + cut, and a caller two events behind gets 0 and 0. **Anchor.**
`crates/ulpf/src/tail.rs` (`TailFrame.cut`, `since`), the `tail_json` line in
`crates/ulpf/src/server.rs`, the "Tail frame" subsection of `docs/api.md`,
`the_tail_frame_separates_eviction_from_the_frames_own_limit` in `crates/ulpf/tests/v4_api.rs`,
the footer in `ui/src/App.svelte`. **Ruled out.** Redefining `skipped` as eviction-only — it
silently changes an existing published field under every consumer written against the contract.
Publishing `evicted` and `cut` and leaving `skipped` as their sum — a third field for arithmetic
the client can do. Dropping the counter — it hides real eviction, which is the one number an
operator must see.

## D96. A pivot page is ordered by device time over every posting row the scan reads, in one snapshot
**Decision.** `PivotIndex::walk` capped its candidate set at `limit*4` postings past the cursor
and only then sorted them by device time, so an event whose device clock ran behind sorted onto a
page it had already been dropped from, and the `(time, raw id)` cursor paged past it for good.
The cap now ends the scan and never drops an entry, and `query()` reads the header and the
timeline inside one read transaction, because the writer commits a batch's entity counts and its
posting rows together. The page's ordering is the cursor's ordering. The remaining ceiling is
named in the comment above `candidates` rather than documented away: a page is exact whenever the
entity's posting rows fit the scan. Measured: 20 of 200 events before the fix (= `limit*4`
exactly) and 200 of 200 after on a deterministic hand-written index; on real data with main's
binary 33 of 36 before and 36 of 36 after; the million-posting page 0.009 s -> 0.017 s against
its own 1.0 s bound, the index write unchanged. **Anchor.** `walk` and `query` in
`crates/ulpf/src/pivot.rs`, `crates/ulpf/tests/pivot.rs`. **Ruled out.** Retrying a page when the
header and the page disagree — a retry hides a moving answer instead of naming one. Making the
test wait for the index to settle — the deterministic repro skips events on a fully settled
hand-written index, so the test was right and the read path was wrong. A time-ordered posting
list — the index file format is frozen for the demo.

## D97. The desktop shell downloads the file, not the page
**Decision.** WKWebView drops an anchor download without calling any delegate, so a file link in
the served UI did nothing in the app. The shell injects a capturing click handler that turns a
click on an anchor the page means as a file into a navigation to `ulpf-save:<url>`; the
navigation handler cancels that navigation and `download.rs` fetches the URL over loopback,
takes the name from the server's own `Content-Disposition` (falling back to the URL's last path
segment), de-chunks a `Transfer-Encoding: chunked` body and writes it into `~/Downloads`. The
window is built in code with `WebviewWindowBuilder::from_config` and `"create": false` in
`tauri.conf.json`, so the config stays the single source of every window value while the two
handlers a config cannot carry get attached. Measured: `attestation.json` (597 bytes) and
`out-first-last.jsonl` (32,206) / `.csv` (38,969) saved byte-identical to `curl`. **Anchor.**
`app/src-tauri/src/download.rs`, `SAVE_SCHEME`/`INTERCEPT`/`window()` in
`app/src-tauri/src/lib.rs`, `app/src-tauri/tauri.conf.json`. **Ruled out.** A Tauri command
called from the served page — the ACL gives a remote origin no command at all. Doing it in `ui/`
— it forks the web UI from the app UI for a problem only the shell has. Tauri 2.11.5's own
`on_download` and `on_new_window` hooks — measured dead for these links on macOS, and keeping
them beside the interceptor would risk a double save.

## D98. Every splash failure carries a recovery button, and a stale `server.url` is a state, not a start
**Decision.** Each way the engine can fail to come up ends on the splash with a sentence and a
button that acts: `splash_with` takes a `Retry` and the fragment flag (`+` / `*`) selects the
button's label, so the label varies and the button's presence does not — the port row and the
never-answered row are no longer dead ends. The retry stops the current child first and treats a
failed kill as a notice rather than a refusal. And a launch that finds a stale `server.url`
together with a live `ulpf` holding the store goes straight to the store-held page instead of
starting a second engine only to have it refused; `server.url` is the signal because it is
written when the engine answers and removed by every stop and every start. **Anchor.** `Retry`,
`down`, `splash_with`, `stop_holder` and the orphan pre-flight in `start()` in
`app/src-tauri/src/lib.rs`; `app/dist/index.html`. **Ruled out.** Restarting automatically — it
loops forever against a store a live writer holds. Buttons on only the two cases the brief named
— that is what left the other rows dead. `holder::find` alone as the trigger — a reader holding
no lock would block a good start.

## D99. The UI states what the running server reports, never a compiled-in literal
**Decision.** The Review screen's approve and reject confirmations, its notes and its proof row
read `parsers_dir` and `pending_dir` from `/api/status` and interpolate them, instead of naming
the literals `parsers/` and `pending/rejected/`. Those literals were wrong for both instances
that matter: the demo server runs with `demo/parsers` and `demo/pending`, and the desktop app
points at its own data directory, so the confirmation told the operator a file had been written
to a path that holds nothing. By the same rule `fmt.stamp` renders the zone the value carries
rather than converting to the viewer's local time. **Anchor.** `pdir`/`pendir` in
`ui/src/Review.svelte`, `fmt.stamp` in `ui/src/api.js`. **Ruled out.** Keeping the literals and
documenting that they are only true of the default layout — the screen is read by an operator
who is about to look for the file. Converting stamps to the viewer's local time — it makes two
stamps on one screen incomparable when one is a device time in its own zone, and makes a
screenshot unreadable without knowing the machine that took it.

## D100. A reset is the shell removing files, never a call into the engine
**Decision.** `File > Reset…` (`CmdOrCtrl+Shift+R`, after `Open output folder`) empties the app's
data from inside the app, and it does it entirely on the shell's side of the sidecar boundary: the
engine is never asked to delete anything, because its raw store is append-only by contract (D42,
D56) and its API has no verb that removes a record. The menu item removes nothing by itself — it
shows the splash under a third fragment flag `?`, a question rather than a failure, naming the data
directory and offering three buttons. `Reset events, keep approved parsers` removes `store/`,
`out.*` (the output, its `.pivot` and every `out.vN.*`, matched by prefix because the version is not
known here), `watch/`, `pending/` and `staging/`, and keeps `parsers/`, `mappings/` and
`engine.log`; `Reset to first launch` removes the whole data directory and the ordinary start
re-seeds the 15 bundled parsers with the generated ones excluded (D94); `Cancel` touches nothing and
navigates back to the served UI, or to the page that was showing. Either reset stops the engine and
waits until `holder::find` reports nothing holds the store, so the deletion cannot race the writer,
then deletes and re-enters `start()` exactly as a launch does — one start path, not a second one for
resets. A path that will not go is a line in `engine.log` and a count in the notice, never a panic
and never a refusal: the app comes back up either way. **Anchor.** `reset_paths`, `ask`,
`stop_and_wait`, `reset` and `reset_cancel` in `app/src-tauri/src/reset.rs`; the `reset` item and
its `action` arm in `app/src-tauri/src/menu.rs`; the `?` flag and the three buttons in
`app/dist/index.html`. **Ruled out.** A `POST /api/reset` in the engine — it breaks the append-only
contract the store is defined by and would make the server own state, against D41. A one-click
silent reset straight off the menu item — it destroys an approved parser by accident, and the one
thing the operator needs before a destructive act is the directory's name and a choice. Deleting
while the engine still runs — the store lock is held by the writer, so the removal would half
succeed and leave the app pointing at a store that no longer matches its index.
