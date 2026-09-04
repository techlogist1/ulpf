# ULPF progress

Started 2026-09-04. Single autonomous session building v0.1 from nothing.

## Definition of done (each item is checked only after running it)
- [ ] 1. CLI processes a directory of mixed-format logs end to end, writes JSON Lines,
      reports sustained events/sec measured ingest→output on this machine.
- [ ] 2. Every raw event reconstructs byte-identically from the append-only store;
      proven by a test reading back bytes and digests across all fixtures including
      multi-line events, non-UTF-8 input, and chunk boundaries mid-event.
- [ ] 3. Parser definition format exists, loaded from a directory at runtime, supports
      delimiter / key-value / structured (JSON, CEF, LEEF) / pattern-with-named-slots,
      and round-trips: a definition emitted from a `Template` parses back and runs
      identically to a hand-written one.
- [ ] 4. 10–12 parser definitions for common perimeter families, each with paired
      sample and fixture asserting parsed + normalized output.
- [ ] 5. Dedicated timestamp module handles the sample formats (syslog no-year,
      no-timezone) with its own corpus; every policy decision explicit and recorded;
      original string retained on the event.
- [ ] 6. Full test suite passes, clean build, Dockerfile producing a static build has
      been built successfully at least once.
- [ ] 7. Throwaway inference prototype run on unseen samples; honest report in docs on
      whether prefix-tree clustering produced usable templates.
- [ ] 8. CLAUDE.md, this file, and docs/DECISIONS.md current; every milestone
      committed and pushed.

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
- [ ] two hand-written parsers (Fortinet KV, Cisco ASA pattern) → format freeze
- [ ] parser format + runtime, four strategies
- [ ] Template → definition round trip test
- [ ] signature detection
- [ ] mapping stage + OCSF subset mapping
- [ ] JSON Lines output
- [ ] throughput measurement
- [ ] end-to-end CLI
- [ ] adversarial pass
- [ ] Dockerfile static build

## Parallel work
### Fan-out 1 (independent of the format; starts right after scaffold)
Split: two workers. Timestamp module and inference prototype share nothing with each
other or with the spine (time crate has a fixed interface; prototype lives in
`scratch/`, outside the workspace). Fewer workers would serialise ~2h of independent
work behind the spine. Each returns: files written, test counts pass/fail, decisions
made, uncertainties.
- [ ] ulpf-time worker
- [ ] inference prototype worker → docs/inference-prototype-report.md

### Fan-out 2 (after format freeze + fixture harness)
Split: parser-definition workers by device family (3–4 each) plus one bench-file
generator. Each parser worker touches only `parsers/`, `samples/`, `fixtures/` for its
families; no shared state. The bench generator touches `crates/ulpf/examples/` only.
- [ ] parser workers A/B/C
- [ ] bench generator worker

## Tried and abandoned
(none yet)

## Next action
Hand-write parsers/fortinet_fortigate.toml and parsers/cisco_asa.toml with samples, then
build ulpf-parse around what they need.
