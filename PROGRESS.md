# ULPF progress

## v2 (2026-09-05 evening session, autonomous): product

Compared head to head against another project at 04:00, demonstrated at 10:00. Everything
below is verified by running, never by reading. Skills: `software-design-philosophy`
loaded (every interface decision); `example-skills:frontend-design` loaded before the UI
lane; `aposd-critique` reserved for the review pass (item 15); no `prompting-practices`
skill and no skills-audit `MANIFEST.md` exist on this machine (checked 2026-09-05), its
requirements are carried from the brief. Tiers: lead on Fable; design, spine review and
verification workers on Opus; corpus, harness, mappings and UI workers on Sonnet with the
source document in hand; Haiku banned (D30). Baseline at a9d0dd8: 71 tests, clippy clean,
260k events/s.

### Definition of done (each item checked only after running it)
- [x] 1. (D57; `crates/ulpf/src/engine.rs` traceback; smoke-tested: 35 of 38 Check Point fields with spans) Field-level provenance: `GET /api/events/{id}` returns, per normalized field, the
      source field and the byte range in the raw record; the traceback view highlights it.
      Offsets come from the borrowed `Cow` spans (D15), computed only on the traceback path.
- [x] 2. (D52; `crates/ulpf/tests/replay.rs`; rehearsed on the demo server: 260 corrected events, why names the file) Replay: `ulpf replay` (and `POST /api/replay`) re-runs the raw store through the
      current parsers and mappings into `out.v<N>.jsonl`, streams a diff against the previous
      version (events changed, fields added, fields lost) with a why line per change. Demo:
      parser bug -> run -> fix -> replay -> every past event corrected, store untouched.
- [x] 3. (D55; `crates/ulpf/tests/pivot.rs`; smoke-tested: 203.0.113.9 across 11 devices, 41/41 lines) Pivot: entity index (src ip, dst ip, user, dst port, device) over normalized
      events, declared per mapping; `GET /api/pivot` and a timeline view across devices.
- [x] 4. (D54; `crates/ulpf/tests/drift.rs` batch and serve modes; rehearsed: update v2 with diff in ~10 s) Drift: per-source rolling match rate; an established source whose window trips
      routes its misses into inference with the current parser as prior; a versioned update
      with a diff lands in `pending/`; a two-format source does not trip.
- [x] 5. (D56 and the store worker's tests; verify, attest, tamper at record N all exercised) Integrity chain: per-record chain value, `ulpf verify` names the first broken
      record, `ulpf attest` exports what a stranger needs to re-verify offline.
- [x] 6. (D60, D62; `crates/ulpf/tests/syslog.rs`; soak runs 10.0M and 15.0M PASS, burst saturates the queue at 300k/s with 537 blocks and zero loss; socket run reconciles TCP exactly and measures UDP kernel drops) Syslog UDP + TCP listeners in `serve`; soak >= 10M events over an appended file
      plus a live socket, UI open, RSS flat, counters reconciled; queue saturation recorded.
- [x] 7. (D53; `docs/slot-vocabulary.md`; reasons visible on the review screen) Slot naming: kv keys, preceding-constant names, curated vocabulary
      (`docs/slot-vocabulary.md`); every name marked suggested with its reason in evidence.
- [x] 8. (D58; branch diff = `mappings/ecs.toml` + one test file) ECS: `mappings/ecs.toml` only plus `--schema ecs`; the diff touches mappings and
      the selector only.
- [x] 9. Multi-core: parsing was already partitioned at event boundaries across N workers
      with one ingest thread, one writer and one sequencer (D19, D60); no engine change was
      needed and none was made. Measured 2026-09-05 18:31 on the M1 Pro (load 8 at start,
      other agents active), 5,000,000 events, 1526 MB, `--output /dev/null`, inference off:
      -j 1 66,827 events/s (74.8 s); -j 2 118,038 (42.4 s); -j 4 218,391 (22.9 s); -j 7
      265,752 / 212,427 / 250,674 (median 250,674, 76.5 MB/s). Backpressure engaged at
      every width (4,789 blocks at -j 1 down to 492 at -j 7): the ingest thread outruns the
      workers, so parallelism is the throughput. Against v1's 260k on a quiet machine this is
      within the stated ±10% variance. Costs of the optional sinks on the 497,607-event
      slice (load 12-17): entity index on 28-31k events/s against 196-249k off (D66: off by
      default in `run`, on in `serve`); `--parquet` 0.46x on the output thread when enabled
      (D64, the worker's measurement, before the row was switched to the entity arena).
- [x] 10. (D63; `corpus/README.md`; six parsers fixed from vendor docs; unseen: nginx, HAProxy, Zeek, OpenVPN 2.6) Corpus: real captures (web, licence read) and locally generated captures (tool
      version + exact setup) replace synthetic samples where obtained; three unseen formats;
      the twelve parsers fixed against the real data.
- [x] 11. (`docs/evaluation.md`, `eval/run.sh`; first scorecard committed; kill_recovery found D59; re-run on the final build pending) Harness: `docs/evaluation.md` scorecard + `eval/` scripts taking any tool's
      command template; ULPF's scorecard generated and committed.
- [x] 12. (D61; seven screens verified by the worker against a 400k-event live server and reviewed by the lead from captures) UI: live feed, review (merge/discard, naming reasons), traceback with provenance,
      pivot timeline, replay diff, drift alerts, integrity status; batched SSE; keyboard.
- [x] 13. (D64; `--parquet` on run and serve, rolled files, static image verified; 0.46x on the output thread when enabled, off by default) Parquet output, only after 1-12 are green; crate and static build verified first.
- [x] 14. (after f267496: 107 tests pass, 2 ignored, `clippy --all-targets -D warnings` clean; the alloc test and the store round-trip are in that suite; isolation run, serve and docker PASS on the final binary and image; harness throughput median 258,411 events/s over three runs on the final build against 251k at the multi-core measurement, inside the ±10% variance) Regression: full suite, clippy, alloc test, round-trip, isolation; bench within variance.
- [x] 15. (D65) `aposd-critique` pass: seven graders, every critical/high/medium finding fixed in one commit (37e41d6, 18 files), two closed with evidence, four deferred with triggers.
- [x] 16. (demo script and 04:00 procedure below; CLAUDE.md, DECISIONS D52-D66 with the D59 amendment, docs/api.md, README throughput paragraph current; final commit pushed to origin/main) This file's demo script rewritten (commands, expected output, reset, 04:00
      procedure); CLAUDE.md, DECISIONS.md, docs current; committed and pushed.
- Out of scope by the brief: segment rotation and retention (design note only, `docs/retention.md`).

### Spine order (lead, sequential, each lands green)
provenance offsets in the API -> integrity chain -> replay -> pivot index -> drift ->
syslog listeners + soak -> multi-core -> Parquet.

### Lanes opened before the spine (fan-out 1)
Split: (a) corpus acquisition, several Sonnet workers with web access, one per vendor
group plus one for unseen formats, each returning files under `corpus/` with a provenance
note and the licence it read; (b) local generation, one Sonnet worker per open-source
tool (Suricata, Squid, OpenVPN, nginx, one outside the twelve), each standing the tool up
in Docker, driving traffic, capturing logs and writing the exact setup; (c) harness, one
Sonnet worker writing `eval/` and `docs/evaluation.md` against the CLI as it is at a9d0dd8;
(d) API contract extension, the lead (interface design for features not yet built).
Why not fewer: disjoint file sets, no shared state, every lane is hours of wall-clock the
spine does not need to wait for. Each worker returns: files written, commands run with
their exit codes, licences read, uncertainties. A worker's claim is a claim until the lead
runs it.

### Fan-out 2 (13:58 IST, after the contract; the user lifted the agent budget)
Split: (e) slot naming, one Opus worker owning `crates/ulpf-infer` and
`docs/slot-vocabulary.md` after the lead's review of `shape()` (names only from
`key_before`, a 30-word list, else `kind+n`, no reason text); (f) integrity chain, Opus in a
worktree owning `ulpf-store` and the `verify`/`attest` subcommands (index entry becomes
offset + chain, store id, genesis, attestation with checkpoints); (g) pivot index and the
normalize half of provenance, Opus in a worktree owning `ulpf-normalize`, a new
`pivot.rs`, the `pivot` subcommand and `[entities]` in `ocsf.toml`; (h) ECS mapping, Opus,
`mappings/ecs.toml` + tests only; (i) soak harness, Opus, `scripts/soak.sh` plus an early
12-minute soak of the file-append path against the baseline binary; (j) retention design
note; (k) Parquet feasibility in scratch, report only; (l) UI, Opus in a worktree with
`frontend-design`, seven screens against the v2 contract, v2 routes mocked in scratch
until the server lands. The lead keeps engine.rs, pipeline.rs, server.rs, pending.rs,
inference.rs: replay, traceback wiring, drift, syslog, multi-core, and every merge.
Why not fewer: each worker owns a disjoint file set; the engine files are the only
shared surface and stay with one writer. Each returns a schema-validated report
(worktree, branch, commits, tests pass/fail, clippy, measurements, public API, decisions,
contract gaps, uncertainties, not done); the lead merges by running the full suite.

### Fan-out 3 (15:40 IST, after the session limit reset; low-priority mode on the weekly budget)
The limit cut the UI, ECS, soak and Parquet workers and two corpus generators mid-task;
integrity, pivot and retention had committed and are merged (2491ec0, e256e38, 023bbe7).
Relaunched on Opus, each in its own workflow: UI resumes in its worktree (merge main
first, seven screens, Chrome verification against main's binary); ECS resumes (its
workaround edits to five test files dropped: the real fix, default schema `ocsf` when
several mappings load, is 7fa1204 on main), adds `[entities]`; soak resumes (commit the
harness, `--report-only` for the cut run, a 10-minute file soak on the current build,
senders ready for the listeners); Parquet feasibility fresh in scratch; the OpenVPN and
nginx/HAProxy/Zeek generators finish from their partial setups; a new parser-fidelity
worker runs the real corpus through the twelve parsers and fixes what breaks from vendor
documentation (D30), promoting permissively licensed real lines into samples and fixtures.
The lead wires integrity and pivot into the engine and server, then fixes the harness's
finding that `run` re-ingests from byte zero after a kill (double counting on restart),
adds `--receipt` to `run`, then syslog listeners.

### Fan-out 4 (18:27 IST): the review pass
Seven Opus graders, one per crate group (store; parse+time; normalize+parquet; infer;
engine+syslog+inference+metrics; replay+pivot+tail; server+pending+cli+contract), each
running `aposd-critique` read-only and returning the graded table, ranked findings with
file:line and the fix's risk tonight, which invariants are enforced by types or tests, and
what to remove. Why seven: the crates are disjoint and a grader that reads 2,000 lines
finds what one that reads 12,000 skims past. The lead merges, ranks by what hurts at
04:00, fixes each real finding in its own commit with a DECISIONS entry, closes wrong ones
with evidence, and defers only with a written revisit trigger.

### Verified state (19:50 IST; every line was run, not read)
- Final build f267496: 107 tests (2 ignored), clippy clean, release binary and `ulpf:static`
  image (11.7 MB, built by the harness from this tree) current. Harness scorecard on it:
  `eval/results/ulpf-20260905T140426Z-33371/scorecard.md`: throughput 263,588 / 258,411 /
  258,398 events/s (median 258,411, about 79 MB/s, 19.0-19.4 s per 5M events), correctness
  264/264, raw preservation and chain ok, unknown format 1 proposal, 12 damaged inputs no
  crash no hang, isolation PASS, container build and run PASS, cold start PASS, kill recovery
  consistent (5,000,000 = 5,000,000); the memory criterion's peak RSS of 1.5 GB is the
  memory-mapped 1.6 GB input counted as resident (serve RSS under soak: 11-103 MB, D62).
  Isolation proofs re-run on this binary and image: run PASS (29 samples, 0 sockets), serve
  PASS (2 sockets, both the listener and its loopback client), docker `--network none` PASS;
  in-container `serve` on port 7879 lists the MikroTik proposal, approves it (13 parsers
  loaded, 250/250 detected) and reports the chain head.
- main b0f4117 (16:05): 102 tests, clippy clean, release build current. Replay (D52), naming (D53),
  drift (D54), integrity chain (D56, store worker), pivot index (D55, pivot worker), provenance
  spans (D57), ECS (D58), kill recovery + `--receipt` (D59), syslog listeners (D60) are on main,
  each with its test; every new route smoke-tested with curl against a real `serve`.
- Soak, file half (soak worker, run1 on the pre-listener build): 10,005,840 events, SOAK PASS,
  chain ok, RSS flat; report at the worker's scratch `soak/run1/report.txt` (numbers land in
  PROGRESS when the worker returns). Socket soak pending the listeners (now on main).
- Harness (`eval/`, `docs/evaluation.md`) generated ULPF's first scorecard; its kill_recovery
  criterion found the restart double count, fixed in D59.
- Corpus: 7 of 8 acquisition workers returned; real captures under `corpus/real/*` with
  PROVENANCE.md (licences read; Elastic-2.0 sources recorded, not copied), generated
  Suricata and Squid captures under `corpus/generated/`; OpenVPN and nginx/HAProxy/Zeek
  generators relaunched; a parser-fidelity worker is fixing what the real data breaks.
- Soak (soak worker, four runs, D62): run1 12 min 10,005,840 events PASS (RSS 11-84 MB,
  slope -0.26 MB/min, SSE max gap 0.52 s); run3 10 min 14,976,000 events PASS (RSS 11-103 MB
  flat over awake time, queue 13/64, 0 blocks); burst 100k/s base + 300k/s burst 8,220,000
  events PASS with the queue at 64/64 and 537 backpressure blocks, zero loss; socket run:
  TCP exact, UDP 47% kernel drops matched by `netstat -s -p udp` (fixed: the 8 MiB
  SO_RCVBUF request was refused by macOS and the default stayed; negotiated down now and
  reported). Same runs found `framed`/`stored` credited per file, not per batch (fixed).
- Corpus + parsers (D63): real captures under `corpus/real` and generated captures from
  real Suricata 7, Squid 6.13, OpenVPN 2.4/2.5/2.6 (file and syslog forms), nginx 1.27,
  HAProxy 2.9, Zeek 8.2 under `corpus/generated`, each with PROVENANCE.md and SETUP.md;
  index in `corpus/README.md`. Six parsers fixed from vendor documentation against the real
  data (ASA rfc5424/EMBLEM headers: 335 lines from 100% pattern_no_match to 100% parsed;
  PAN-OS empty serial; OpenVPN syslog and ISO 8601 forms; IOS SISF; SonicOS empty address
  parts; legacy FortiOS keys); real lines promoted into samples and fixtures.
- Parquet (D64): `--parquet FILE` on run and serve, rolled files, 107 tests, static image.
- Unseen formats for the live inference demo: nginx access/error, HAProxy httplog, Zeek
  conn/dns/http/ssl (TSV and JSON), OpenVPN 2.6 file and syslog forms; each graded by
  `ulpf infer` in its PROVENANCE.md (e.g. nginx access 1 template 1548/1548, Zeek dns 3
  templates 3400/3409, Zeek TSV http.log the honest failure at 40 templates/100 lines).
- UI (Opus worker, `frontend-design` loaded, worktree merged at 5fbcf05): seven screens
  verified in Chrome against the real server by the worker and reviewed by the lead from
  headless-Chrome captures of a populated server (samples + two pending proposals + a
  replay + a verify + both sockets fed): live counter grid with the funnel and named
  drop-offs, review with the naming reasons per slot, traceback with digest/chain/timestamp
  verdicts in plain English and byte-range highlighting, pivot with the device-lane
  timeline and the seen-with panel, replay with the why box above the counts. Found and
  fixed from the review: the "approved" badge came from a negative priority (OpenVPN and
  IOS are hand-written at -1); the engine now stamps `origin = "inferred"` (0aaca31).
  The Chrome extension was unreachable from this session, hence headless captures.
- Stop semantics (f267496, found by the serve isolation proof on the final binary): ctrl-c
  during a 5M-event drop had drained the whole file through the entity index before
  exiting (minutes on a loaded machine). A stop now ends the file at the next batch
  boundary; measured on the 5M file with the index on: ctrl-c to exit 13.0 s (the 64-batch
  queue draining), restart output contiguous 0..994303, no duplicate; tailer bytes credited
  per batch so MB/s moves during a large ingest. Serve isolation PASS on that binary, 41 s wall.

### Tried and abandoned (v2)
- Recovering the output whenever it is empty: a fresh output beside an existing store
  would have received the whole store; the live meta now names the store and recovery
  applies only to an output this store's engine already started (D59).
- Routing the tripping window's own lines to inference: they had already been offered as
  unknown lines and would shape the update twice (D54).
- Comparing a replay against the previous version's final parser set: the demo's own
  reopen-after-fix made v1 look unchanged; the oldest recorded set is the comparison (D52).

---


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

## Demo (10:00) and the 04:00 comparison: start here

Everything below was run on 2026-09-05 on the M1 Pro from a clean checkout. Terminal 1 is
the server, terminal 2 everything else; paths are relative to the repo root. A store
written before tonight is refused by name (the integrity chain changed the index): delete
it and start over.

```
cargo build --release                                      # ~1 min; binary target/release/ulpf
./target/release/ulpf check --pending pending              # 12 parsers, 2 mappings (ocsf, ecs), 0 problems

# 0. reset between rehearsals (approvals land in parsers/; every generated file goes)
rm -rf demo pending/*.toml pending/*.json pending/*.lines pending/approved pending/rejected parsers/*_inferred.toml

# 1. server + UI (terminal 1): watches demo/watch, listens for syslog on UDP and TCP 5514
mkdir -p demo/watch demo/parsers && cp parsers/*.toml demo/parsers/
./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl --pending pending \
    --parsers demo/parsers --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514 --infer-threshold 64
#    -> ulpf: serving http://127.0.0.1:7878 ; syslog udp 127.0.0.1:5514, tcp 127.0.0.1:5514 ; 12 parsers loaded
#    open http://127.0.0.1:7878  (1 Live, 2 Review, 3 Traceback, 4 Pivot, 5 Replay, 6 Drift, 7 Integrity; ? = keys)

# 2. known formats and a live device: counters, sources and the tail move within 500 ms
cp samples/*.log demo/watch/
python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);[s.sendto(l,('127.0.0.1',5514)) for l in open('heldout/edgerouter.log','rb').read().splitlines()]"
#    Live -> sources: udp/127.0.0.1 (250 events, no parser yet), 12 sample sources parsed; syslog row: udp datagrams 250

# 3. an unknown format from a file and from the socket: clustered at 64 lines, "Review (2)" appears
cp heldout/mikrotik.log demo/watch/
#    Review -> mikrotik: 14 templates; every slot has a name and the REASON it was chosen
#    (key `src-mac` before the value; vocabulary `{ip}:{port}->{ip}:{port}` names src/dst ...);
#    generic slots stay ip1/word2 and say why. Uncheck a template + Regenerate to drop it.

# 4. approve (UI button, or):
curl -s -X POST http://127.0.0.1:7878/api/pending/mikrotik/approve
#    -> {"name":"mikrotik_inferred","parsers_loaded":13,"now_detected":{"tested":250,"detected":250},"replaced_version":null}
#    demo/parsers/mikrotik_inferred.toml carries origin = "inferred"; Live -> parsers: origin approved

# 5. the same events take the fast path; the pivot sees them
cp heldout/mikrotik.log demo/watch/mikrotik-again.log
#    Live -> mikrotik-again.log detected 250. Pivot -> search src_ip -> pick 203.0.113.9:
#    one attacker across OpenVPN, pfSense, ASA, Check Point, SRX, SonicWall in one lane-per-device timeline,
#    "seen with" dst_ip 10.0.0.7, dst_port 22, user jdoe; click any related value to pivot again.

# 6. traceback with provenance: click a tail row, or open http://127.0.0.1:7878/#/trace/0
curl -s http://127.0.0.1:7878/api/events/0 | python3 -m json.tool | head -40
#    stored and recomputed SHA-256, chain and prev_chain with chain_match, every parsed field with its
#    byte range, every normalized path with the field and bytes it came from; hover a normalized field
#    in the UI and its bytes light up in the raw record.

# 7. replay: a parser bug, the fix, every past event corrected, the store untouched
sed -i '' 's/{dst_ip:ip}/{dst_addr:ip}/g' demo/parsers/cisco_asa.toml     # the bug (reloads within 250 ms)
cp samples/cisco_asa.log demo/watch/asa-under-the-bug.log                  # events written wrong
cp parsers/cisco_asa.toml demo/parsers/                                     # the fix
curl -s -X POST http://127.0.0.1:7878/api/replay                            # -> {"version":2,"started":true,"total":N}
#    Replay -> v2 report: changed = the ASA events written under the bug, by_field dst_endpoint.ip added /
#    unmapped.dst_addr lost, and the WHY box: "demo/parsers/cisco_asa.toml changed since v1 (sha256 .. -> ..)".
./target/release/ulpf verify --store demo/store                             # exit 0: chain ok, nothing rewritten

# 8. drift: a device changes its format mid-stream; the update proposal carries a diff
python3 - <<'EOF'
import time
lines=open('heldout/mikrotik.log','rb').read().splitlines()
hdr=b' '.join(lines[0].split()[:4])
with open('demo/watch/gw-drift.log','ab') as f:
    for _ in range(5):
        for l in lines: f.write(l+b'\n')                                     # 1250 known lines: established
    f.flush(); time.sleep(3)
    for i in range(400):                                                     # a new message type
        f.write(hdr+b' interface,info ether%d link up (speed %dG, full duplex)\n' % (1+i%8, [1,10,25][i%3]))
EOF
#    Drift -> gw-drift.log tripped (window rate vs baseline; a partial window is judged after 5 s of
#    quiet, D54); within ~10 s Review shows mikrotik_inferred v2 replacing the standalone proposal:
#    the diff adds one pattern, the decisions start with "prior: `mikrotik_inferred` v1".
#    Approve -> parsers/mikrotik_inferred.toml is v2, pending/approved/mikrotik_inferred.v1.toml kept.

# 9. integrity: verify from the UI (Integrity -> Verify) or offline, and hand a stranger the attestation
./target/release/ulpf attest --store demo/store --out demo/attestation.json
./target/release/ulpf verify --store demo/store --attestation demo/attestation.json   # exit 0
printf 'X' | dd of=demo/store/raw.seg bs=1 seek=200 conv=notrunc 2>/dev/null           # tamper one byte (rehearsal only!)
./target/release/ulpf verify --store demo/store                                        # names the record, exit 1

# 10. a second output schema with zero parser changes
./target/release/ulpf run samples --store demo/ecs-store --output demo/ecs.jsonl --schema ecs --infer-threshold 0
git log --oneline -1 -- mappings/ecs.toml ; git show --stat 5f7abd5 | tail -3         # mappings/ + one test file

# 11. throughput (terminal 2, quiet machine; the bench file is gitignored, generate once, ~25 s, 1.5 GB)
cargo run --release -p ulpf --example gen_bench -- 5000000 bench
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/ulpf-bench --output /dev/null --infer-threshold 0
#    numbers: see "Verified state" below; never quote one you did not just measure

# 12. kill recovery: kill -9 a run, restart it, same output id for id
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/kr --output /tmp/kr.jsonl --infer-threshold 0 & sleep 3; kill -9 $!
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/kr --output /tmp/kr.jsonl --infer-threshold 0
#    -> "recovered: N stored records an interrupted run had not written to the output"; wc -l equals a clean run

# 13. isolation and container
scripts/isolation.sh run bench/mixed-5000000.log ; scripts/isolation.sh serve demo/watch 20
docker build -t ulpf:static . && scripts/isolation.sh docker ulpf:static samples
```

### The 04:00 comparison (docs/evaluation.md, "The 04:00 procedure")
Both tools, same machine, quiet (no soak, no builds, no Docker workloads), same bench file.
`eval/tools/ulpf.toml` is ULPF's template; write `eval/tools/<other>.toml` for the other tool
(build command, run/verify/raw_of templates, container, key map). Then, for each tool:
`eval/run.sh eval/tools/<tool>.toml` (three runs of throughput inside; every raw output under
`eval/results/<tool>-<timestamp>-<pid>/`). Compare the two `scorecard.md` files criterion by
criterion; a criterion a tool cannot do reads "not measurable: reason", never pass. ULPF's own
committed scorecard is `eval/results/ulpf-<timestamp>/scorecard.md`.

### What to say when something looks wrong
A proposal that looks wrong: the evidence panel shows `support` beside `verified`, the slot
table with the reason for every name, `history`, `decisions` and `unmatched` by reason;
`ulpf infer FILE --decisions` prints the same offline. A replay diff nobody expected: the
WHY box names every parser or mapping file whose digest changed since the previous version,
and says so explicitly when none did. A source that stopped parsing: Drift shows the window
rate against the baseline and where its lines went. A record in doubt: Traceback shows the
stored and recomputed digest, the chain link, and the same bytes through today's parsers.

## v1 (2026-09-05 session, autonomous): the visible half

Brief: inference, review workflow, server, embedded UI, isolation check, container, demo
script, design review. Skills: `software-design-philosophy` loaded (interfaces),
`example-skills:frontend-design` for the UI worker, `aposd-critique` for the review phase;
`prompting-practices` does not exist on this machine (manifest at
`~/Documents/dev/skills-audit/MANIFEST.md`), its requirements are carried from the brief.
Toolchain verified: node 24.15, pnpm 11.9, docker 29.4, lsof; axum 0.8.9 (path syntax
`/{id}`, `Sse::new(stream).keep_alive(KeepAlive::default())`, `axum::serve(listener, app)`
verified from docs.rs), tokio 1.53, notify 9.0.0-rc.5 (evaluated, ruled out: D40).

### Definition of done (each item checked only after running it)
- [x] 1. Inference end to end. `crates/ulpf/tests/live.rs`
      `unknown_format_becomes_a_proposal_and_approval_moves_it_to_the_fast_path`: 250
      MikroTik lines -> no_parser 250 -> one proposal (14 templates, 0 problems) -> a
      second run parses nothing and skips the duplicate -> approve through `Live` reloads
      in place, `now_detected` 250/250 -> a third run: detected 250, parsed 250, no
      proposal. Held-out grades (`cargo run -p ulpf-infer --example infer -- heldout/X.log`):
      mikrotik 14 templates for 14 message types, 250/250 lines, 0 unmatched; edgerouter
      9 templates for 10 types, 250/250; nginx 1 template, 250/250; messy 19 templates,
      289/300 with 4 empty, 4 below_support (truncated), 3 no_template. Thresholds and
      the alternatives tried: D46.
- [x] 2. Review workflow. `review_edge_cases_are_errors_as_values`: invalid edit saved and
      listed with its line, approval refused with the same problem, edited proposal never
      replaced, name conflict 409, regenerate from a kept subset keeps the human's
      `[match]`, reject then resubmit skipped as rejected across a restart. UI: Review
      list, detail with editor/Save/Approve/Reject, per-template Keep + Regenerate,
      verified in Chrome against the real server 2026-09-05 (approval showed 13 parsers
      loaded, 250 of 250 buffered lines detected).
- [x] 3. Server. `crates/ulpf/tests/server.rs` walks the demo over real HTTP: status, UI
      assets, ingest by watch, proposal, tail with raw ids, invalid edit -> 422 with
      problems, approve -> now_detected 250/250, approve twice -> 404, parsers list shows
      origin approved, traceback with matching digests and emitted vs now, missing id ->
      404 with store_len, second file detected 250/0, SSE hello+metrics, client disconnect
      drops the count to 0. Store reads go through the writer's lock (D42); the server
      owns a frame cache and per-client positions only (D41, D48).
- [x] 4. UI embedded. `ui/dist/{index.html,app.js,app.css}` (63 KB JS) built by Svelte 5 +
      Vite in 13 minutes (timer did not fire), served from the binary and from `--ui-dir`.
      Screens checked in Chrome: Live counters/sources/parsers/tail, Review, Traceback.
      No console errors. Tokens at the top of `ui/src/app.css`.
- [x] 5. Isolation. `scripts/isolation.sh run bench/mixed-5000000.log`: 46 samples, no
      socket, PASS. `serve` mode: listener on 127.0.0.1 and one loopback client, PASS.
      `docker` mode with `--network none`: PASS. Commands in the hackathon section.
- [x] 6. Container rebuilt from the final code (47965c8; `docker build -t ulpf:static .`,
      9.57 MB): `serve` inside with `-p 7879:7878`; from the host `/api/status`, `/` and
      `/app.js` answered, the MikroTik proposal (14 templates) was generated in-container
      and approved through the API (13 parsers loaded, 250/250 now detected);
      `scripts/isolation.sh docker ulpf:static samples` PASS.
- [x] 7. Regression: `cargo test --workspace` 71 tests, 0 failed (v0.1's 50 plus 21);
      `cargo clippy --workspace --all-targets -- -D warnings` clean; counting-allocator
      test unchanged and passing; bench 231k then 260k events/s (inference off) and 258k
      then 266k (on) against v0.1's 214k-232k.
- [x] 8. `aposd-critique` review pass: two workers (engine modules; `ulpf-infer`), 18
      ranked findings, every one fixed in the "review fixes" commit with the suite re-run
      (71 tests, clippy clean, held-out grades unchanged, bench re-measured) or closed
      with a reason in D51. Table below.
- [x] 9. This section, CLAUDE.md, DECISIONS.md D38-D50 with anchors.

### Spine (sequential, lead), as run
- [x] skills check, four files read, toolchain and crate APIs verified
- [x] `docs/api.md` contract before server or UI
- [x] Template optional groups `{? ...}` + CLF timestamp shape (D39; D27 gap closed)
- [x] `ulpf-infer` crate, graded on the four held-out files after eleven iterations (D46)
- [x] engine restructure: `Live`, per-batch store lock, pipeline swap, tail, inference
      thread, pending module, `serve` poller, `infer` and `check --pending` (D40-D44)
- [x] server + UI merge, browser verification, isolation, container, demo script

### Fan-outs, as dispatched
- Fan-out A (three workers, after the contract): UI worker (strong, frontend-design)
  shipped Svelte in 13 min and reported two contract gaps (hello count, 422 problems),
  both adopted; held-out samples worker (Sonnet, web-verified) delivered four files with
  ground truth; isolation worker (Opus) delivered the script with run mode tested and a
  self-test. The first two attempts at the samples and isolation workers died on the
  session rate limit and were relaunched. Why not one worker: disjoint file sets, the UI
  the long pole. Server and inference were written by the lead (the server needs `Live`,
  which was being built; a cold worker would have re-read the engine).
- No stress-test worker: the lead graded the four held-out files directly with
  `examples/infer.rs` across eleven iterations; the graded table is in
  `docs/inference-prototype-report.md`.

### Tried and abandoned (v1)
- Cluster key on the first alphabetic word: a username became the key in nginx lines.
- Similarity 0.7: fragmented free-text tails into singletons; 0.6 + keyword split kept.
- Plain LCS alignment: a missing NAT block pulled a line's address pair into the block
  on a tie; gap-open penalty added. Gap penalty alone: two-word disagreements became one
  region; substitution state added.
- Joining a many-token run into one value when one column faced it: hid `connected` vs
  `disconnected, reason` from the keyword split; first-token substitution instead.
- Enum split on any identifier-like value: split per script name, per flag list;
  restricted to plain alphabetic words seen at least twice each.
- `notify` for directory watching: rc release, and no events across bind mounts (D40).
- A separate `ulpf-server` crate (D38).

### Review pass (done item 8), findings ranked as the workers ranked them
| # | finding (file) | verdict | what changed |
|---|---|---|---|
| E1 | source identity = basename, resume offsets summed by name (engine, store) | bug, fixed | `source_name(root, path)`: path relative to the input root |
| E2 | no mutual exclusion between inference `write` and reviewer ops (pending) | bug, fixed | `Pending.ops` lock around every mutating method |
| E3 | per-event lock + String on the unknown path (inference, engine) | smell, fixed | `offer_batch` once per batch |
| E4 | `finish` returns before stopping inference: a worker panic hangs the scope | smell, fixed | every thread joined before any error returns |
| E5 | `[parser] name` unvalidated as a file name (pending) | bug, fixed | `[A-Za-z0-9_-]+` or 422 |
| E6 | unreadable watched file retried every tick, counters climb (engine) | bug, fixed | reported once, retried when the file changes |
| E7 | `Pending::list` (file reads, regex compiles) every 200 ms for ids (server) | smell, fixed | `Pending::ids` directory scan |
| E8 | approve/reject ordering leaves ghosts on IO failure (pending) | smell, fixed | record first, parser file rolled back, missing toml is `Io` |
| E9 | inference disabled: 404 on GET, 500 on POST, uncounted (server, engine) | smell, fixed | one `NotFound` through `review_error` |
| E10 | `after + 1` overflow (tail) | bug, fixed | saturating add |
| E- | store `get` allocates from an unchecked header; `atomic_write` no fsync; `walk` follows symlinked dirs; mtime-only reload signature; `origin` by description text | fixed | bounds check; fsync; `symlink_metadata`; count+mtime+size; `priority < 0` |
| I1 | alignment tables unbounded in token count (align) | bug, fixed | lines over 2048 tokens -> `unmatched[too_long]` |
| I2 | optional constants weighed in ordering, general template first (lib) | bug, fixed | required constants only; `verified > support` explained |
| I3 | merge decision printed wrong counts (lib) | bug, fixed | counts bound before the take |
| I4 | deduped templates lost their presence decisions (lib) | smell, fixed | decisions passed through |
| I5 | verification included templates the definition drops (lib) | bug, fixed | eligibility = compiles and >= min_support |
| I6 | approved regex matcher claims a sibling unknown source's lines (lib, engine) | smell, fixed | parse failures under a `priority < 0` parser are offered to inference |
| I7 | `.lines` split by `\n`, member indices shift on blank lines (pending) | bug, fixed | re-framed with `Framer` |
| I8 | dedupe shape by brace counting (lib) | smell, fixed | shape from `Template.tokens` |
| I- | two `similarity` names; compile error dropped; unused params; doc comments on the wrong items; `ParamsUsed` mirror | fixed | renamed, kept, removed, moved, `Params` serialised |
| closed | axum plain-text 400 for malformed params; `infer_buffered` vs `buffered`; evidence integer widths | closed, documented | D51, api.md |
| deferred | `Live` public fields | argued in D51 | revisit on a second consumer |

### Adversarial pass (each handled, counted, tested)
| case | outcome | where |
|---|---|---|
| source never reaches the threshold | clustered on idle (serve) and at the end of the run (batch) | `a_source_below_the_threshold_is_still_clustered_at_the_end_of_a_run`; idle rule in `inference.rs` |
| proposal approved twice | 404 `not_found`, counted in `review_errors` | live.rs, server.rs tests |
| rejected definition resubmitted | skipped, `proposals_skipped[rejected]`, survives restart | live.rs |
| client disconnecting mid-stream | guard drops, `sse_clients` back to 0 | server.rs test |
| traceback for an id that does not exist | 404 with `store_len` | both tests |
| pending file edited by hand into invalid syntax | listed with `path:line`, approval 422 with problems, `ulpf check --pending` exit 1 | live.rs, server.rs |
| unknown source floods | buffer capped at 4096, `infer_buffer_full` counted (43,951 on the bench) | bench run |
| watched file shrinks | re-read from 0, reported as an input problem | `poll_loop` |

## Cold start (v0.1 record; v1 is above)

v0.1 closed at 4a74364; v1 (the sections above) was built on 2026-09-05 in one session
and is at `git log -1`. Working tree clean, `origin/main` in sync. The verified-claims
table below is v0.1's; v1's proofs are listed per done item above. Do not redo the review
passes listed under Fan-out 2 or the v1 inference iterations.

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
The `aposd-critique` review pass (v1 done item 8): two workers grade `ulpf-infer` and the
`ulpf` engine/server/pending modules; every real finding is fixed with its own commit and
a DECISIONS entry or amendment, every wrong one is closed with evidence here. Then the
hackathon: follow the first section of this file.
