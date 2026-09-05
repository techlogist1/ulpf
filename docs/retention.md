# Segment rotation and retention (design note)

Not implemented. Nothing in this note exists in the tree; it is the shape retention would
take if it were built, written now so tonight's integrity chain is not designed into a
corner. Read with `crates/ulpf-store/src/store.rs`, D5, D7, D33, D42, D43 and the
"Integrity chain" section of `docs/api.md`.

The five things retention may not weaken: append-only (the interface has no update and no
delete), one writer (D33), permanent monotonic raw ids, traceback (any emitted id is
answerable), and the chain `chain_i = SHA-256(chain_{i-1} || sha256_i)` from
`genesis = SHA-256("ULPF chain genesis" || store id)`.

## 1. Segments and the id -> segment map

Today the store is one `raw.seg`, one `raw.idx` (a u64 offset per id; the id *is* the
index position), one `catalog.sqlite`. Segmented, a store is:

```
raw.0000000000000000.seg   .idx   .chain   .att.json     # sealed
raw.0000000000a1b4c0.seg   .idx   .chain                 # live, no attestation yet
catalog.sqlite
```

The name carries the first raw id in the segment, zero-padded hex: the file states its
own range, name order is id order, and there is no counter to lose or renumber. `.idx`
and `.chain` are per segment and indexed by `id - first_id`, so they are unlinked with
the segment they describe and never outlive it.

The id -> segment map is a catalogue table, one row per segment (hundreds of rows for
years of data, so D5's "never per-event rows" holds):

```
CREATE TABLE segments (
  first_id INTEGER PRIMARY KEY, last_id INTEGER,          -- last_id NULL while live
  path TEXT NOT NULL, prev_chain BLOB NOT NULL, head BLOB,
  state TEXT NOT NULL,          -- 'live' | 'sealed' | 'moved' | 'deleted'
  sealed_nanos INTEGER, retired_nanos INTEGER, moved_to TEXT,
  attestation TEXT);            -- the sealed document, inline, kept after the bytes go
```

Lookup is the greatest `first_id <= id`, then a positional read at `(id - first_id) * 8`.
Rows are inserted and then only walked forward along `live -> sealed -> moved|deleted`;
a row is never deleted, which is what keeps a retired id explainable forever (section 6).

Rotation triggers on segment size (default 1 GiB) tested once per batch at a record
boundary, plus explicitly at `run` exit and on `ulpf seal`. Never mid-record, never per
event: the hot path gains one integer comparison per batch, no allocation (D43's rule).

*Ruled out:* one global `raw.idx` with `(segment, offset)` entries — it grows forever and
cannot be trimmed with the segment it describes, and after a retirement its entries point
at files that are gone. *Ruled out:* naming segments by sequence number or by wall-clock
time — a sequence number needs a second map to reach the first id and invites renumbering;
time names collide within a second and reorder when the clock steps back.

## 2. Where the chain crosses a boundary

Nowhere, by construction. The chain is over `(previous chain value, this record's
digest)`; it never mentions a file, an offset or a segment. The first record of segment
k+1 chains to the last record of segment k because it is the next record, not because
anything special happens at the boundary. Rotation chooses which file the bytes land in
and nothing else. This is the error defined out of existence: there is no boundary case
in the chain math to get wrong, so there is no boundary test that can be forgotten.

What the boundary does need is a way to verify a segment standalone. Each segment records
`prev_chain` — the chain value of the record before its first, `genesis` for segment 0 —
in its catalogue row and in the segment file header. Verifying a segment is: start from
`prev_chain`, recompute forward over the records present, end at `head`.

At seal time the segment stops changing forever, so its attestation is written then and
never recomputed: `raw.<first>.att.json`, the `ulpf-attestation/1` document restricted to
the segment, plus `first_id` and `prev_chain`, with checkpoints every 4096 records and the
segment's last record. Seal is write-temp, fsync, rename, then update the row to `sealed`:
if the process dies between the rename and the row update, reopening finds an attestation
for a segment the catalogue still calls live and re-seals to the same bytes.

The store-wide attestation (`GET /api/integrity/attestation`, `ulpf attest`) becomes a
concatenation: genesis, the per-segment `(first_id, prev_chain, head)` rows, the sealed
checkpoint lists read out of the sealed documents, and the live segment's checkpoints
computed now. Sealed history is never rehashed to answer that route.

*Ruled out:* a per-segment genesis (each segment restarting from a fresh seed) — cheap to
verify a segment alone, but it severs the store into unrelated chains and a whole segment
could be removed or reordered undetectably. *Ruled out:* computing the attestation lazily
on demand instead of at seal — after retirement the bytes are gone, so the one moment the
document can be produced is the moment before it is needed.

## 3. Retention: seal, then move or delete whole segments

Retention has exactly two verbs and both take a whole sealed segment:

```
enum Retire { Move { to: PathBuf }, Delete }
fn retire(&mut self, seg: SealedSegment, how: Retire) -> io::Result<()>
```

`SealedSegment` is produced only by `seal()`, so the live segment cannot be named by a
retirement call — a typestate, not a documented rule. There is no per-record verb at all:
no delete, no redact, no rewrite, so no code path exists that could change a stored byte
or renumber an id. Structural prevention over documentation.

The order is fixed and each step is durable before the next: seal -> export attestation ->
verify the segment against that attestation -> write the retirement row (`moved`/`deleted`,
timestamp, destination) -> unlink or rename the files. Verify precedes destruction so a
segment that was already corrupt is reported instead of silently discarded, and the row
lands before the unlink so a crash mid-retirement leaves a row describing files that may
still exist (idempotent to re-run) rather than files with no row.

Retirement runs through the single-writer lock (D33) like an append — it is the writer, or
a `ulpf retire --store DIR --before <date> [--to ARCHIVE]` that takes the same exclusive
catalogue lock and is refused while `serve` holds it. `serve` retires only when given
`--retain-days` / `--retain-bytes`, and a segment qualifies only when *every* record in it
is past the horizon, so a policy stated per record is enforced per segment and the
difference is a printed number, never a split inside a file.

*Ruled out:* compaction — copying the surviving records of a partly expired segment into a
new file. It rewrites stored bytes, changes offsets, and either breaks every chain value
after it or requires re-signing history; it is exactly the mutation the store's interface
exists to make impossible. *Ruled out:* tombstoning individual records in place — needs a
write path into an existing record, which is the one handle the API refuses to hand out
(D7's "the only mutation is to bytes that were never a record").

## 4. Reading a retained-away id

The read becomes three-valued. `Option<OwnedRecord>` cannot say the difference between
"never issued" and "was here, deliberately removed", so the type says it instead and the
compiler finds every call site the day the variant appears:

```
enum Lookup { Present(OwnedRecord), Retained(SegmentInfo), NeverIssued { store_len: u64 } }
```

`SegmentInfo` is the catalogue row: id range, `prev_chain`, `head`, sealed and retired
times, `moved_to` when it was archived rather than deleted. It survives the bytes.

`ulpf raw <id>` on a retained id writes nothing to stdout — never a partial or a
substitute — prints the segment line on stderr (`raw id 4210 retained: segment
0000000000001000..0000000000002000 sealed 2026-09-05T…, head 9c01…, moved to
/archive/raw.0000000000001000.seg`) and exits 3, distinct from 4 for an id that was never
issued. `GET /api/events/{id}` answers `410 Gone` with
`{"error", "reason": "retained", "segment": SegmentInfo}` — a status a UI renders as
history rather than as a bug, since 404 already means "no such id". Both increment
`traceback_retained` in `Snapshot`, so a store quietly answering nothing for half its ids
is a number in the counter block on every run, not a discovery made during a demo
(observability as a design input). The tail, the pivot index and an emitted output line
are derived data and unaffected; a pivot row whose record is retired links to the same 410
and shows the segment head, so the trail ends in a receipt rather than a dead end.

## 5. Verify across a gap

`ulpf verify` walks segments in id order and reports per segment. For a present segment it
recomputes digests and chain from `prev_chain` to `head` as today. For a retired one there
are no bytes, so it checks the join instead: segment k+1's `prev_chain` must equal segment
k's recorded `head`, for every k including retired ones. That test is what makes the gap
provable rather than assumed — the surviving records still demonstrably follow from the
history that was removed, and the recorded head of a retired range is exactly what its
exported attestation said before deletion.

The report is explicit about what it could not touch:

```
verified 812,304 records in 3 segments, 0 corrupt
skipped 2 retired segments (1,048,576 records): chain joins verified, bytes not present
chain continuous: yes    store head 9c01…
```

Exit 1 on corruption or a broken join; exit 0 otherwise, but the skipped line is always
printed, so "all good" never hides a hole. With `--attestation FILE` the checkpoints
inside a retired range cannot be recomputed and are compared against the segment
attestation's head instead of against bytes; the report says so per segment. An archived
segment is re-verifiable on its own at any time: point verify at the archive directory and
its exported `.att.json` and it is checked exactly as it would have been in place.

*Ruled out:* refusing to verify a store with a gap — routine retention would then make the
integrity command useless, and an operator with no working verify stops running it.
*Ruled out:* reporting a gap as corruption — it trains people to ignore corrupt counts,
which is worse than not counting at all.

## 6. What stays impossible

- **No in-place edit.** The store interface stays `append`, `get`, `seal`, `retire`. No
  method takes a record id and bytes; `retire` takes a `SealedSegment` and a destination.
  Adding record-level mutation means adding a method to one file, in one crate, in a diff
  a reviewer sees.
- **No id reuse.** The high-water id must move out of `raw.idx`'s length and into the
  catalogue the same day segment 0 can vanish: `next_id` is `max(last_id) + 1` over every
  segment row, retired rows included, cross-checked against the live segment's index by
  the existing `recover` (D7). Rows are never deleted, so a store whose every segment has
  been retired still opens at the right next id and hands out no id twice.
- **No silent loss.** Retention is a state transition recorded in a durable row before the
  unlink, counted in `Snapshot` (`segments_sealed`, `segments_retired`, `records_retired`,
  `traceback_retained`) and answerable per id forever.
- **One writer still.** Seal and retire are writer operations under the same exclusive
  catalogue lock; nothing about rotation introduces a second mutator of the store.
- **Recovery unchanged in shape.** `recover` runs on the live segment only. A sealed
  segment is never opened for append, so the torn-tail path can only ever touch bytes that
  were never a complete record.
