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
