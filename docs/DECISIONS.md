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

## D23. Queue depth counts batches resident in the channel; the BOM is stripped by the envelope
**Decision.** The in-flight counter increments after a successful `send`, is signed, and
the high-water clamps at zero, so the printed depth never exceeds capacity and
"backpressure engaged" means the channel really filled. A leading UTF-8 byte-order mark is
removed before `<pri>` detection. Both found by feeding hostile inputs through the real
engine (`crates/ulpf/tests/adversarial.rs`). **Anchor.** `send_batch` in
`crates/ulpf/src/engine.rs`; `strip_syslog` in `crates/ulpf-parse/src/envelope.rs`.
**Principle.** Verification means exercising the binary, not reading the source; a
plausible-looking counter (`2/1`) is exactly the kind of wrong output only a run reveals.
**Ruled out.** Counting before send (reports capacity+1 under load); treating a BOM as
message bytes (a Windows-exported ASA log parses as `pattern_no_match` for every line).

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
