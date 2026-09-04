# ULPF progress

Started 2026-09-04. Single autonomous session building v0.1 from nothing.

## Definition of done (each item is checked only after running it)
- [x] 1. CLI processes a directory of mixed-format logs end to end, writes JSON Lines,
      reports sustained events/sec measured ingest→output on this machine. Measured
      2026-09-05 on the M1 Pro (8 cores, 7 workers), release build of the final engine,
      5,000,000 line synthetic mix of all 12 families (1526 MB, `bench/mixed-5000000.log`),
      three consecutive runs on a quiet machine: 214k / 225k / 232k events/s
      (65–71 MB/s, 21.5–23.4 s wall); a fourth run under load gave 206k and an
      independent reviewer's clean run 265k, so the honest figure is 225k events/s
      with about ±10% run-to-run variance. SHA-256 + raw store (flushed per batch) +
      JSON Lines included; `ulpf verify` on the resulting store: 5,000,000 records, 0
      corrupt. Queue high-water 64/64 with about 1,650 measured backpressure blocks, so
      the workers (parse+normalize+serialize) are the bottleneck, not ingest. Signals on
      that run: detected 99.0%, no_parser 48,047 (generator-mutated lines), sub_no_match
      91,105, sub_uncovered 145,461, time_from_receipt 165,001, time_error 14 (the
      earlier 51,641 was the counter firing on resolved timestamps, D36),
      class_unknown 898,670.
- [x] 2. Every raw event reconstructs byte-identically from the append-only store;
      proven by a test reading back bytes and digests across all samples including
      multi-line events, non-UTF-8 input, and chunk boundaries mid-event.
- [x] 3. Parser definition format exists, loaded from a directory at runtime, supports
      delimiter / key-value / structured (JSON, CEF, LEEF) / pattern-with-named-slots,
      and round-trips: a definition emitted from a `Template` parses back and runs
      identically to a hand-written one.
- [x] 4. 12 parser definitions, each with a paired synthetic sample and a reviewed
      fixture: cisco_asa, cisco_ios, fortinet_fortigate, openvpn, palo_alto_panos,
      pfsense_filterlog, check_point, juniper_srx, sonicwall, sophos_xg, squid_access,
      suricata_eve (214 sample events, all asserted by `cargo test -p ulpf --test fixtures`).
- [x] 5. Dedicated timestamp module handles the sample formats (syslog no-year,
      no-timezone) with its own corpus; every policy decision explicit and recorded;
      original string retained on the event.
- [x] 6. `cargo test --workspace` passes (2026-09-05 at 97fee74: 50 tests across 11 test
      binaries, 0 failed, exit code checked), `cargo clippy --workspace --all-targets -- -D warnings` clean, Dockerfile
      static build built and run 2026-09-04 and rebuilt 2026-09-05 at cade196 with the
      final 12 definitions and the reviewed engine (ulpf:static, 7.37 MB, scratch base;
      in-container run over `samples/` processed 236 events, 0 failed files, time_error
      none, counters matching the fixtures).
- [x] 7. Throwaway inference prototype run on unseen samples; honest report in docs on
      whether prefix-tree clustering produced usable templates.
- [x] 8. CLAUDE.md, this file, and docs/DECISIONS.md (D1–D36, each with an anchor)
      current; every milestone committed and pushed to techlogist1/ulpf main.

## Cold start for v1 (read this first)

v0.1 is closed at the commit that added this section (parent 1fb4c41); `git log -1`
gives the hash. Working tree clean, `origin/main` in sync, the eight done items above
are all verified by runs recorded in this file. The next session starts with the
deferred `aposd-critique` review pass, then the server + UI session (CLAUDE.md, "Verify
before building the server"). Do not redo the review passes listed under Fan-out 2.

### What is verified, and by which test
| claim | proof |
|---|---|
| framing lossless, chunk-boundary safe, multi-line and non-UTF-8 kept | `crates/ulpf-store/tests/roundtrip.rs`: `framing_is_lossless_and_groups_continuations`, `framing_is_identical_across_every_chunk_boundary`, `framing_edge_cases` |
| store round trip, reopen, crash recovery both directions, single writer | same file: `store_round_trips_bytes_and_digests_and_survives_reopen`, `index_ahead_of_segment_recovers_to_the_last_complete_record`, `segment_ahead_of_index_reindexes_complete_records_and_drops_a_torn_tail`, `a_second_writer_is_refused_while_the_store_is_open` |
| whole samples corpus round-trips through the engine in raw-id order | `crates/ulpf/tests/e2e.rs`: `samples_directory_round_trips_through_store_and_output_in_order`, `single_thread_and_many_threads_produce_identical_output` |
| Template -> definition -> identical parse | `crates/ulpf-parse/tests/roundtrip.rs`: `generated_definition_parses_identically_to_hand_written` (plus bijection and machine-emittability tests) |
| every parser has a fixture and every fixture line matches | `crates/ulpf/tests/fixtures.rs`: `every_fixture_matches_its_sample` (214 events) |
| timestamp formats and policies | `crates/ulpf-time/tests/corpus.rs` over `tests/corpus.txt` (118 cases) + 4 unit tests |
| hostile inputs counted, broken parser files reported, output failure aborts, queue depth bounded | `crates/ulpf/tests/adversarial.rs` (6 tests) |
| zero allocations per event on span-valued families | `crates/ulpf-parse/tests/alloc.rs` (counting global allocator) |
| parser/mapping wall | `crates/ulpf-normalize/tests/normalize.rs`: `ocsf_mapping_loads_and_has_no_vendor_vocabulary`; `ulpf-parse` has no dependency on `ulpf-normalize` |

### Exact commands
```
cargo build --release                                   # binary at target/release/ulpf
cargo test --workspace                                  # 52 tests, check the exit code itself
cargo clippy --workspace --all-targets -- -D warnings
./target/release/ulpf check                             # 12 parsers, 1 mapping, 0 problems
cargo test -p ulpf --test fixtures                      # every sample event asserted
cargo run --release -p ulpf --example gen_bench -- 5000000 bench      # ~25 s, 1.5 GB, gitignored
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/ulpf-bench --output /dev/null
./target/release/ulpf verify --store /tmp/ulpf-bench    # 5000000 records, 0 corrupt
./target/release/ulpf fixture samples/<parser>.log > fixtures/<parser>.expected.jsonl   # then review the diff
docker build -t ulpf:static .
docker run --rm -v "$PWD/samples:/data/samples:ro" ulpf:static run /data/samples --store /tmp/s --output /dev/null
```
Bench numbers on the M1 Pro: 214k to 232k events/s over three quiet runs (median 225k),
one later run 265k; expect about ±10% between runs. Never quote a number you did not
just measure.

### Inference prototype verdict (docs/inference-prototype-report.md)
Prefix-tree clustering yields correct typed templates for fixed-layout lines (66 to 71%
of lines) but fragments every optional field into a separate template and merges
disposition words at loose thresholds, so it is usable only as a candidate generator for
a human to prune, not as an unattended parser generator.

### Warts and half-decisions not in docs/DECISIONS.md (with anchors)
- The e2e multi-line check (`multiline >= 2` in `crates/ulpf/tests/e2e.rs`) is met by
  the folded Fortinet line plus `samples/README.md`, which `run samples` ingests as a
  log file because directory scans take every non-hidden file with no extension filter
  (`walk` in `crates/ulpf/src/engine.rs`). A real multi-line perimeter event does not
  exist in the corpus; the store tests prove the framing with synthetic input.
- `RawReader` takes no lock (`RawReader::open` in `crates/ulpf-store/src/store.rs`):
  `ulpf verify` or `ulpf raw` while a writer runs reads a moving file, and the recovery
  truncation at open can shrink a file a reader mapped after a crash. Only the catalogue
  read (`source_names`) is refused with "in use". The server session, which will hold
  the writer open and serve reads in-process, should decide this properly.
- A worker thread panic aborts the whole process through `join().expect(...)` in `run`
  (`crates/ulpf/src/engine.rs`): no counter, no report. Input cannot reach a panic
  (adversarial tests), a bug can.
- OCSF `status` never receives a source field literally named `status`: that name is an
  `action` alias so Sophos `status="Allow"` canonicalises (`mappings/ocsf.toml`, `action`
  and `status` alias lists). Fortinet `status="success"` therefore lands in `action` as
  Allowed too.
- Check Point `origin` (a gateway IP) sits under `device.hostname` because Cisco IOS
  `origin` is a hostname; it wins only when no syslog host exists (`mappings/ocsf.toml`).
- Numeric severity scales differ per vendor (Check Point 0-4, syslog 0-7) and the
  mapping keys on field name only; a numeric Check Point `severity` would normalise on
  the syslog scale. The sample uses the text form (`parsers/check_point.toml`,
  `[[enum]] field = "severity"` in `mappings/ocsf.toml`).
- ASA teardown endpoints are `lower_*`/`higher_*` and deliberately unmapped; join on
  `connection_info.uid` with the build event for direction (`parsers/cisco_asa.toml`).
- OpenVPN is detected by its ctime prefix alone at priority -1 (`parsers/openvpn.toml`);
  any other ctime-prefixed file log would be claimed by it.
- Cisco IOS: the documented `<time>:%FAC-n-MNEM` form with no space before `%` does not
  match because a pattern space requires at least one byte (`parsers/cisco_ios.toml`,
  `crates/ulpf-parse/src/pattern.rs`); every real capture has the space.
- The timestamp slot swallows an all-caps token right after a syslog stamp if it equals a
  zone abbreviation (`timestamp_regex` in `crates/ulpf-parse/src/template.rs`; the
  `CET1`/`CET` cases in `crates/ulpf-parse/tests/strategies.rs` show the boundary).
- `gen_bench.rs` keeps the samples' timestamps (no time spread), weights families by
  sample line count rather than realistic volume, and has an unused time-offset parameter
  (`crates/ulpf/examples/gen_bench.rs`).
- Delimiter `quote` is one byte while kv `quote` accepts several (`Strategy` in
  `crates/ulpf-parse/src/def.rs`); nothing needed the asymmetry yet.
- pfSense CARP `advbase`/`advskew` follow the Netgate BNF, whose prose lists them the other
  way; unverified on a live box (`parsers/pfsense_filterlog.toml`). SonicWall `m=29` for
  "Administrator login allowed" is unverified (`samples/sonicwall.log`).
- Fixtures are full snapshots reviewed by hand (D30); the skeleton keeps a fixed subset of
  normalized paths (`skeleton` in `crates/ulpf/src/fixture.rs`), so a mapping change
  outside that subset does not change fixtures.
- Normalization builds a `serde_json::Map` per event and is the throughput ceiling
  (`Mapping::normalize` in `crates/ulpf-normalize/src/mapping.rs`); profile before the
  server session.

## Environment (Phase 0 findings, 2026-09-04)
- rustc/cargo 1.95.0, rustup 1.29; only `aarch64-apple-darwin` installed locally;
  `aarch64-unknown-linux-musl` available via rustup (static build done in Docker).
- git 2.54 configured as Lokavya Singh <lokavya12@gmail.com>.
- `gh` 2.92 authenticated as techlogist1 with `repo` scope → repo creation + push OK.
- Docker 29.4 CLI present; OrbStack daemon was not running at start — launched it.
- 8-core Apple M1 Pro, 16 GB RAM. Throughput numbers are for this machine.
- Python 3.14 available (used only for the throwaway inference prototype).
- No `ULPF-PRD.md` anywhere in the working directory; the brief is the source of truth.
- Skills (source of truth: ~/Documents/dev/skills-audit/MANIFEST.md, read 2026-09-04):
  loaded `software-design-philosophy` (every module/interface decision) and
  `andrej-karpathy-skills:karpathy-guidelines` (the installed Karpathy skill; no skill
  named `prompting-practices` exists on this machine — its gate is carried from the
  brief directly). `aposd-critique` is present and RESERVED for a separate review pass
  after v0.1; not run in this session. Manifest note: aposd-critique writes untracked
  `.aposd/critique/` into the repo — `.aposd/` is gitignored ahead of that pass.
- Discrepancy with brief: none material. Docker daemon needed a manual start.

## Spine (sequential, lead)
- [x] scaffold workspace (5 crates: time, store, parse, normalize, cli)
- [x] framing (lossless, multi-line by indentation, chunk-safe) — 3 tests
- [x] raw store + index + round-trip tests — reopen, crash recovery, digest verify
- [x] two hand-written parsers (Fortinet KV, Cisco ASA pattern) → format FROZEN (docs/parser-format.md)
- [x] parser format + runtime, four strategies (kv, delimiter, json/cef/leef, pattern) — 15 tests
- [x] Template → definition round trip test (crates/ulpf-parse/tests/roundtrip.rs)
- [x] signature detection (Registry::detect with per-source hint)
- [x] mapping stage + OCSF subset mapping (mappings/ocsf.toml, fragments merge) — 4 tests
- [x] JSON Lines output (ordered by raw id, unknown formats emitted as Base Event)
- [x] throughput measurement (printed every run; 5M-line bench measured, see done item 1)
- [x] end-to-end CLI: run / check / verify / raw / fixture — e2e + fixture harness tests
- [x] adversarial pass — crates/ulpf/tests/adversarial.rs (4 tests): empty file, 8 MiB single line, unknown format, binary garbage, BOM+CRLF, nested dirs, hidden files, truncated KV, broken/bad-regex/bad-format parser files, zero parsers, missing dirs, batch-boundary parity. Found and fixed: BOM defeats envelope; uncovered message ids invisible; queue depth off by one.
- [x] Dockerfile static build — rust:1.95-alpine → scratch, 7.3 MB image, `file` confirms statically linked aarch64; `docker run ulpf:static run /data/samples` processed 142 events and `verify` reported 0 corrupt (2026-09-04)

## Parallel work
### Fan-out 1 (independent of the format; starts right after scaffold)
Split: two workers. Timestamp module and inference prototype share nothing with each
other or with the spine (time crate has a fixed interface; prototype lives in
`scratch/`, outside the workspace). Fewer workers would serialise ~2h of independent
work behind the spine. Each returns: files written, test counts pass/fail, decisions
made, uncertainties.
- [x] ulpf-time worker — 118-case corpus + 4 unit tests, clippy clean, verified by lead's `cargo test --workspace`; policies D8–D12
- [x] inference prototype worker → docs/inference-prototype-report.md (verdict: correct typed templates for fixed-layout lines, 61–71% line coverage, but fragments optional fields; usable only as a candidate generator). Prototype code deleted as required.
- Tier note: both ran on the top tier; dispatched before the tier rule arrived. All later workers: haiku/low for mechanical work.

### Fan-out 2 (after format freeze + fixture harness)
Split: parser-definition workers by device family (3–4 each) plus one bench-file
generator. Each parser worker touches only `parsers/`, `samples/`, `fixtures/` for its
families; no shared state. The bench generator touches `crates/ulpf/examples/` only.
- [x] parser workers A/B/C (haiku/low, confirmed from transcripts) — returned 10
  families, every one "all fixtures pass". Lead review of the generated output found the
  fixtures had snapshotted wrong parsing (D30): Palo Alto written as key=value, Check
  Point/Juniper hacks around missing engine support, Cisco IOS and OpenVPN with invented
  message texts and no device time, pfSense misparsing every row, SonicWall's second sub
  never running. Seven families rewritten by the lead from vendor references; Sophos,
  Squid, Suricata kept. Engine gaps the review exposed were fixed: per-field sub groups
  (D24), delimiter `rest` (D25), RFC 5424 SD params as fields (D26), timestamp slot from
  the zone table (D27), empty captures (D28), absent values and the never-implemented
  class wildcard (D29), kv quote set (D32).
- [x] bench generator worker (haiku/low) — `crates/ulpf/examples/gen_bench.rs`, kept as
  delivered (D31); 5M lines in 25 s.
- [x] Opus reviewers (3, read-only, web) verified the seven rewritten definitions against
  vendor documentation. All confirmed findings applied 2026-09-05 (see D30): PAN-OS THREAT
  gained four documented columns after sig_flags and CONFIG's order/placeholder were
  fixed; Check Point's sample uses the exporter's default space-separated timestamp and
  trailing `;`; IOS origin-id precedes the sequence number, log-input on an SVI, login
  without a trailing time, CONFIG_I variants, IPACCESSLOGRP; Junos legacy positional form,
  trailing deny fields, `-->`; pfSense IPv6 rows capitalise the protocol and ICMPv6 has
  no payload; SonicWall ids 37/38 and double-quoted appName; OpenVPN VERIFY ERROR serial,
  `Learn sec`, `(Not enabled)`, daemon signals. Engine: an empty delimiter remainder emits
  no `rest` field (consistent with D28).
- [x] Opus reviewer on Sophos, Squid, Suricata (the three kept worker families): every
  sample was rewritten from the sources 2026-09-05. Sophos now uses the wire form
  (`<30> device="SFW" ...`, no syslog header), sent_bytes/recv_bytes, uppercase
  protocols, correct log_id subtype digits, empty values, and Content Filtering / ATP /
  IDP / Event records; Squid logs DIRECT/NONE hierarchy codes (HIER_ is the C enum
  prefix), real result codes and plausible code/method pairs, `%6tr` widths, an IPv6
  client and NONE_NONE/000, with a detector that no longer rejects IPv6 or hostnames;
  Suricata TLS/HTTP/DNS objects match output-json-*.c (TLS 1.2, colon-hex serial and
  fingerprint, no-offset validity dates, sni, http app_proto, dns v3 queries array,
  community_id, alert metadata arrays).
- [x] Opus reviewer on Fortinet and Cisco ASA (the two hand-written first-session
  families), applied 2026-09-05: Fortinet admin-login log id, IPS msg trailing comma,
  incoming direction, trandisp/appcat/dstintfrole, a config-change event carrying the
  escaped quote, the folded line relabelled as collector wrapping; ASA's sample RFC 5424
  frame replaced by the real `logging timestamp rfc5424` form at severity 7, no-NAT
  302013/302015 without mapped-address parentheses, `%ASA-auth-` and `%FTD-` headers, the
  documented comma form of 113004, 106100 without the hash pair, ICMP 106023 without
  parentheses, and teardown endpoints renamed lower_/higher_ because 302014 carries no
  direction (the old fixed guess inverted inbound connections).
- [x] Ultracode invariant review workflow (5 Opus finders, one adversarial Opus verifier
  per finding): 12 findings, 12 confirmed, 0 refuted; the zero-copy finder did not run
  (session limit). All twelve applied 2026-09-05 with regression tests (D23 rewritten,
  D33 to D36): store writer lock and two-direction crash recovery, ids flushed before they
  escape, output-failure abort instead of a hang, measured backpressure with a clamped
  high-water, subs on materialised values, repeated source fields kept, `time_error`
  only when unresolved, class uid range check, D3 anchor. The zero-copy dimension was
  then run (3 findings, 3 confirmed, 0 refuted) and applied with a counting-allocator
  test that pins the invariant (D37): multi-field timestamp join no longer cloned, CEF/LEEF
  position buffers in `Scratch`, JSON flattener moves values. Ten of twelve families
  measure zero allocations per event after warm-up; JSON and escaped quoted values are the
  documented exceptions.

## Tried and abandoned
- Internally-tagged `Strategy` enum with `#[serde(flatten)]` inside `[[sub]]`: serde cannot combine flatten with deny_unknown_fields; replaced by one flat validated struct (D13).
- Per-event SQLite rows for the raw index: ruled out on throughput math before writing it (D5).

## Known limits carried into the next session
- Throughput ceiling is the worker side: normalization builds a `serde_json::Map` per
  event. Profile before the server session; the parse path allocates nothing for
  span-valued families (`crates/ulpf-parse/tests/alloc.rs` proves it), JSON and escaped
  quoted values excepted.
- `class_unknown` on the bench mix comes from families with no OCSF class for their
  events (IOS config/interface messages, OpenVPN control-channel lines) and from
  generator-mutated lines; not a mapping bug.
- Check Point `origin` (gateway IP) maps to `device.hostname` only when no syslog host is
  present; Cisco IOS `origin` is a hostname. One alias, two meanings; acceptable for now.
- Fixtures are reviewed snapshots; a deliberate mapping change regenerates them with
  `ulpf fixture` and a diff review.

## Next action
Apply the three Opus vendor-documentation reviews when they return (fix, regenerate the
affected fixtures, re-run the suite, commit). Then the deferred `aposd-critique` review
pass, then the server session (see CLAUDE.md "Verify before building the server").
