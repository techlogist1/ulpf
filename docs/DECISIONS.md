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
