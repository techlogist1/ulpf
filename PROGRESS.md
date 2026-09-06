# ULPF progress

## Demo (10:00) and the 04:00 comparison: start here

Everything below was run on 2026-09-05 on the M1 Pro from a clean checkout. Terminal 1 is
the server, terminal 2 everything else; paths are relative to the repo root. A store
written before tonight is refused by name (the integrity chain changed the index): delete
it and start over.

### Runner (D67): `ulpf demo`
`ulpf demo` plays steps 0-9 below from the repo root: it prints each command before running it
and what to click next, Enter advances, and the server stays up for questions at the end (Enter
again stops it and resets `demo/`). `ulpf demo --auto` is the unattended rehearsal (fixed 3 s
pauses, then stop and reset); `--check` starts nothing and proves every step title and command
in the runner appears verbatim in this section (run it after editing either; `cargo test` asserts
the same); `--reset` stops a leftover server, removes `demo/` and removes any generated parser
from the repo's `parsers/`. `scripts/demo.sh` is now a wrapper that finds `./target/release/ulpf`
and hands the flags over, so anything that referenced it keeps working. The runner lives in the
binary rather than in a shell script so the demo can be played where no shell exists; it has
been played on macOS only, and the two Windows branches (`taskkill`, `tasklist`) are compiled by
the `windows-latest` job of `.github/workflows/app.yml` and have not been executed, the same
standing D74 records for the app. Lane D's verifier played it twice on the lane's binary at
9c2b946 (04:00 and 04:02 IST, once through the wrapper): the proposal for mikrotik 0.6 s after
the drop, approve `now_detected 250/250, parsers_loaded 13`, replay v2 over 1,044 events, verify
clean, the drift update proposal 6.1 s after the new lines, attestation 2 of 2 checkpoints over
2,694 records, the tamper named raw id 0 (digest) with exit 1, reset clean, 53 s end to end.
The lead's pass on the merged binary is recorded under Verified state. Ports 7878 and 5514 must
be free: the runner refuses to start while either is held, naming the port and the command that
finds the holder (a serve left from an earlier rehearsal answers `/api/status` with 200, so
without the refusal the runner would play against it and every drop would land in a directory
nobody watches); `--check` reports the same two ports.

**Nothing is approved from the CLI before the video is recorded.** A CLI approve writes the
generated parser (`origin = "inferred"`, priority -1) into the repo's `parsers/`, and a bundle
or a demo copy built after it knows mikrotik already, so the unseen-format demo cannot raise a
proposal; the demo's reset removes any generated parser from `parsers/` before the copy is made,
and the bundle step and the app's first-run copy exclude them (a Windows tester hit this against
14d3b0c). Every documented command names the log files (`samples/*.log`), never the bare
`samples` directory, which would ingest `samples/README.md` as a log.

```
cargo build --release                                      # ~1 min; binary target/release/ulpf
./target/release/ulpf check --pending pending              # 15 parsers, 2 mappings (ocsf, ecs), 0 problems

# 0. reset between rehearsals (the server uses demo/parsers and demo/pending, so nothing lands in the repo)
rm -rf demo
#    the runner also removes any generated parser (origin = "inferred") a CLI approve left in parsers/

# 1. server + UI (terminal 1): watches demo/watch, listens for syslog on UDP and TCP 5514
mkdir -p demo/watch demo/parsers demo/pending && cp parsers/*.toml demo/parsers/
./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl --pending demo/pending --parsers demo/parsers --syslog-udp 127.0.0.1:5514 --syslog-tcp 127.0.0.1:5514 --infer-threshold 64
#    -> ulpf: serving http://127.0.0.1:7878 ; watching demo/watch ; syslog udp 127.0.0.1:5514, syslog tcp 127.0.0.1:5514 ; 15 parsers loaded ; ctrl-c to stop
#    open http://127.0.0.1:7878  (0 Flow, 1 Live, 2 Review, 3 Traceback, 4 Pivot, 5 Replay, 6 Drift, 7 Integrity; ? = keys)

# 2. known formats and a live device: counters, sources and the tail move within 500 ms (one file a second, so the feed visibly moves)
for f in samples/*.log; do cp "$f" demo/watch/; sleep 1; done
python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);[s.sendto(l,('127.0.0.1',5514)) for l in open('heldout/edgerouter.log','rb').read().splitlines()]"
#    Live -> sources: udp/127.0.0.1 (250 events, no parser yet), 15 sample sources parsed; syslog row: udp datagrams 250

# 3. an unknown format from a file and from the socket: clustered at 64 lines, "Review (2)" appears
cp heldout/mikrotik.log demo/watch/
#    Review -> mikrotik: 14 templates; every slot has a name and the REASON it was chosen
#    (key `src-mac` before the value; vocabulary `{ip}:{port}->{ip}:{port}` names src/dst ...);
#    generic slots stay ip1/word2 and say why. Uncheck a template + Regenerate to drop it.

# 4. approve (UI: `a` opens the confirmation, Enter approves, Esc backs out; or:)
curl -s -X POST http://127.0.0.1:7878/api/pending/mikrotik/approve
#    -> {"name":"mikrotik_inferred","now_detected":{"detected":250,"tested":250},"parsers_loaded":16,"path":"demo/parsers/mikrotik_inferred.toml","problems":[],"replaced_version":null}
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
#    (j/k walk them, Enter pins one, h = hex, Esc releases)
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
#    Approve (`a`, Enter) -> demo/parsers/mikrotik_inferred.toml is v2, demo/pending/approved/mikrotik_inferred.v1.toml kept.

# 9. integrity: verify from the UI (Integrity -> Verify) or offline, and hand a stranger the attestation
./target/release/ulpf attest --store demo/store --out demo/attestation.json
./target/release/ulpf verify --store demo/store --attestation demo/attestation.json   # exit 0
printf 'X' | dd of=demo/store/raw.seg bs=1 seek=100 conv=notrunc 2>/dev/null           # tamper one byte of record 0 (rehearsal only!)
./target/release/ulpf verify --store demo/store                                        # names the record, exit 1

# 10. a second output schema with zero parser changes
./target/release/ulpf run samples --store demo/ecs-store --output demo/ecs.jsonl --schema ecs --infer-threshold 0
git log --oneline -1 -- mappings/ecs.toml ; git show --stat 5f7abd5 | tail -3         # mappings/ + one test file

# 11. throughput (terminal 2, quiet machine; the bench file is gitignored, generate once, ~25 s, 1.5 GB)
cargo run --release -p ulpf --example gen_bench -- 5000000 bench
./target/release/ulpf run bench/mixed-5000000.log --store /tmp/ulpf-bench --output /dev/null --infer-threshold 0
#    numbers: see "Verified state" below; never quote one you did not just measure
#    say the thread count with the number: -j 7 is the default here (cores minus one): 337k events/s
#    median with --output /dev/null, -j 1 68k on the same file (v3 A3); the harness figure with the
#    JSON Lines output written is 258k at 7 threads

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

---

## v4 (2026-09-06, 02:50-09:30 IST, autonomous): the demo morning

### Cold start (read this first; written 09:40 IST at the close of the session)

**Where main is.** The commit this block ships in sits on 2c955c7 (the lead's 08:58 record) over
06cd41a (lane R's records, D100). main == origin/main after every push this morning; the pushes go
over `ssh://git@ssh.github.com:443/techlogist1/ulpf.git` because the https token lacks the workflow
scope. The wake lock (caffeinate) is stopped. No agent, worktree or process of this session is
running; the caffeinate and the demo server are down. The owner's demo material lives outside the
repo in `~/Desktop/demo-logs` on the Mac (the HPC slices, the 15-device pivot folder, the 100k
Cisco file, the reset/tamper/benchmark `.command` files) and the installed app is lane R's bundle
at be8748b, its sidecar sha 55b52b87… equal to main's `target/dist/ulpf`.

**What is verified, by which test (all on the tree at 06cd41a; nothing in the two commits since
touches code).**
- Engine, server, UI contract: `cargo test --workspace` 125 passed 0 failed over 38 binaries
  (fixtures over all 15 samples in `crates/ulpf/tests/fixtures.rs`; the v4 API contract in
  `v4_api.rs`, 5 tests, the paging case deterministic since D96; pivot in `pivot.rs`; store,
  recovery, replay, drift, syslog, integrity, parquet, adversarial each in its file), `cargo clippy
  --workspace --all-targets -- -D warnings` clean, `ulpf check` 15 parsers 2 mappings 0 problems.
- The demo path: `scripts/demo.sh --check` 39 ok lines rc 0 (every step title and command in this
  file matches the binary), `ulpf demo --auto` at 4e0d71d 57 s exit 0 (proposal 0.6 s, approve 250
  of 250, drift 6.0 s, attestation 2 of 2 over 2,739, tamper named at raw id 0 with exit 1).
- Isolation: `scripts/isolation.sh run` PASS at 4e0d71d; serve and docker modes PASS at 217d0df
  (the scripts unchanged since). Cold start from a fresh clone 50.2 s over 9 commands at 217d0df.
- The desktop app: app crate `cargo test --lib` 12 passed, clippy clean; CI `app.yml` green on
  lane-7b-app (runs 34002152105, 34003825762): macOS and Windows bundles, the Windows smoke job
  installs the NSIS setup silently, force-kills the window and proves no ulpf.exe survives (500 ms
  against a 5 s ceiling), runs `ulpf.exe demo --check` on the installed engine. By hand on this Mac
  (08:43-08:46, the lead, one clean instance): 100,000 Cisco events through every stage, Traceback,
  both resets, an unknown 1,000-line slice to a proposal, approve, the fast path on a second slice.
  CI has NOT run on main since 5b27f68's push: run it (`gh run list --branch main`) and read it
  before trusting a fresh checkout on Windows.
- Throughput: the committed scorecard 258,411 events/s (D87); 295,928 and 320,369 measured quiet
  on dist and recorded beside it, not promoted; 237,643 at 08:52 with other work running.

**Every lane branch on origin, one line each.** Merged into main and safe to delete:
lane-4-releases (f62ef5c), lane-4b-readme (61233b3), lane-7-windows (26d0bbd), lane-7b-app
(02b4bef, tag v0.1.0-rc3 at fb7bda9 on it), lane-docs-fixes (42b19cd), lane-i-intensity (f68781d),
lane-mt-plan (e6fdde8), lane-pv-paging (da002ec), lane-r-reset (be8748b), lane-u-ui (eee630f),
lane-u2-review (1e9e2f3); lane-a-shell (a88a709) is one docs commit ahead whose content is on main
as a37af63, delete it. Not merged:
- lane-5-xml (1b8aa19, 7 commits over 14d3b0c): the xml strategy and Windows Event, D75 on the
  branch, 118 tests there. Verified by its lane; engine work, so it waited for the owner's go. Needs
  a rebase over the demo-morning merges (crates/ulpf-parse touched) and one full gate: READY after
  that rebase.
- lane-6-index (9389be7, 6 commits over d62a01c, merges main once): the entity-index page cache,
  5.1x back to back, D76 reserved. Engine + store-adjacent; rebase then gate and a quiet-window
  re-measure of D87's index-on figure: READY after that.
- lane-3b-cef-leef (c6e13ca, 4 commits over a9c8ac6): CEF string severity `Very-High` into the
  Critical bucket, LEEF `0xHH` delimiters, 117 tests. Parsers and mappings plus one test: the rebase
  is trivial, READY.
- lane-8-windows (80a5bfc, 6 commits over 14d3b0c): the store reopen under a live mapping on
  Windows, the Parquet teardown handle, the null device metadata, a Windows suite job (green,
  33998281457); D82 on the branch; reviewed by the lead 05:27 (118 tests on the Mac). Touches
  ulpf-store and ulpf-parquet: rebase, full gate, and one Windows run of the suite: READY after that.
- lane-7b-windows (fbda6a0, 2 commits over 14d3b0c): ABANDONED, superseded by lane-7b-app; delete.

**Open items, from the report's section 8 and the Windows tester's report.**
1. Nobody has clicked the Windows app since the tester's report against 14d3b0c: exports, the
   locked-store button, the job object under a real force-quit, Intensity's restart, the sidecar per
   target, `sidecar.ps1` (never executed anywhere: CI runs sidecar.sh under bash on Windows), lane
   A's click interceptor on WebView2, and File > Reset… on Windows (`remove_dir_all` over an open
   file is reported in engine.log and the notice, untested). `docs/manual-test.md` is the plan.
2. The tester's contributed throughput figure for the ROG G615 was never received; README says a
   later one would be contributed. The tester's machine has a hardware fault (random access
   violations there are not engine evidence).
3. D83, directory-level include/exclude, reserved and not built: a bare `samples` directory still
   ingests `samples/README.md` as a log (D91 documents `samples/*.log` everywhere; `PROGRESS.md`'s
   demo section line about `run samples` is the last place the glob is not spelled out).
4. Lane 8's three Windows store fixes are on the branch (above), not on main.
5. The rc3 draft release (tag fec0119 at fb7bda9) is unpublished and three commits behind that
   branch's tip; publishing or re-tagging on main is the owner's call. rc1 and rc2 drafts too.
6. `ulpf verify` covers record bytes, digest and chain link, not the index header: a tampered
   `raw.idx` header reads as a pre-chain store and verify refuses instead of naming the rewrite
   (adversarial review finding 19).
7. On macOS a force-quit (`kill -9`) of the app leaves the engine running; the next launch names
   the holder and offers Stop it and start again (D93, D98). A parent-death mitigation in the engine
   is not built.
8. The DragDrop handler in the app was never driven by a real drag with the tools (files went in
   through the watch directory and Add files); the owner dragged files by hand this morning and it
   worked.
9. Two timing assertions still flake under machine load (`server.rs:209`, `replay.rs:154`).
10. The footer's 200-row frame limit is prose in the UI, not read from the server (a
    `tail_per_tick` in `/api/status` would fix it).
11. Process lessons for the next session: create every lane worktree from main's head and print
    its base; a gate script checks the build's exit code; a headline number comes from a quiet
    window; a verifier that drives a GUI gets its own HOME from the start (lane R's had to be
    killed mid-demo); write the lead's own actions into PROGRESS before a report pass, because
    agents cannot source them.
12. Kabir's LogLens comparison has no numbers: the corpus was not reachable.

**GitHub secret-scanning alert 1** ("Amazon AWS Temporary Access Key ID", samples/cloudtrail.log:5
and fixtures/cloudtrail.expected.jsonl:5, first seen in 360faec): the string was the AWS
documentation's temporary-key example `ASIAIOSFODNN7EXAMPLE`; both files now carry the documented
permanent-key example `AKIAIOSFODNN7EXAMPLE` (same length, so no byte offset in any record moved),
the fixture test passes, and the alert can be closed as "used in tests" once this commit is on
origin. The AROA/AIDA example ids in the same file are AWS's documented examples and are not
flagged.


Started 02:50 IST at 14d3b0c (main == origin/main, 114 tests, clippy clean, CI green on both
runners). Two clocks: main freezes at 08:30 (nothing merges after); 08:30-09:30 is the final
sequence (rebuild with the final dist, re-bundle the app with the sidecar SHA checked,
isolation in run, serve and docker modes, cold start, two demo-runner passes, the verified
state with its timestamp, commit, push, main == origin/main); the report by 09:30. A lane not
merged by 08:30 stays on its branch, verified and described here; that is not a failure.
The wake lock from last night (`caffeinate`, pid 6054) is left running.
Skills: `software-design-philosophy` loaded by the lead for every interface decision; no
`prompting-practices` skill on this machine (its requirements carried from the brief: clean
lead context, five-line structured worker returns, kill timers, every claim run before it is
believed); `example-skills:frontend-design` is loaded by the UI workers, not the lead; the
skills-audit manifest is `~/Documents/dev/skills-audit/MANIFEST.md`; no `aposd` pass. Tiers:
lead and every builder that touches design, `parsers/`, `mappings/`, server state or an
engine crate on Fable; Opus for verifiers and for writing to a spec the lead holds (CI YAML,
README, the measurement script); Haiku banned (D30).
Kabir's LogLens repository with its labelled corpus is NOT reachable from this machine:
searched 02:55 IST with `mdfind -name loglens`, `find / -maxdepth 5 -iname "*loglens*"`,
`~/Documents/dev`, `~/Desktop`, `~/Downloads`, and the git remotes (origin only, plus one stale
lane branch). The comparison is blocked on the corpus location; the owner supplies it as an
addendum and `scripts/coverage.sh <dir>` (lane 4) then grades detection per file against the
directory-name vendor. Nothing is fabricated in its place.
The stale data directory `~/Library/Application Support/dev.ulpf.app` (first bundle
identifier, dead data) was deleted 03:05 IST; `dev.ulpf.desktop` is the live one.

### Fan-out (03:10 IST): five worker lanes, the lead on lane 2
Every lane is one Workflow: a builder in its own git worktree, an independent Opus verifier
that re-runs every claim, one fix round in the same worktree. Lanes start together; the only
sequencing is named below. Return format (schema-enforced, nothing else): worktree, branch,
commits, files, tests with the exact commands and pass/fail counts, clippy, decisions (each
with the alternative ruled out and its anchor, for DECISIONS), contract gaps, uncertainties
verified with their source, measurements with their commands, not done and why, and at most
twenty-five lines for this file. No worker command over about four minutes (backgrounded and
polled past that). Every worker claim is a claim until the lead has run it.

| lane | owns (in its worktree) | tier | kill timer (IST) | merges tonight |
|---|---|---|---|---|
| 1 Flow screen and motion | `ui/`, `docs/design.md`, `docs/screens/` | Fable + frontend-design | 05:50 committed and captured, hard stop 06:00 | yes, after the lead looks at the captures |
| 2 server truthfulness (lead) then UI plumbing | `crates/ulpf/src/{server,engine,pivot}.rs` additively, `docs/api.md`, tests; then `ui/` for badges, filter, export, bytes route | Fable (lead); Opus for the UI plumbing against the committed contract | API by 04:30; UI plumbing 04:30-06:15 | yes |
| 3 CEF, LEEF, CloudTrail definitions; the Zeek http class rule | `parsers/`, `samples/`, `fixtures/`, `mappings/` (class rules and field lists, additive), `samples/README.md`, `docs/parser-format.md` (one line), the alloc test's family list only | Fable | 05:30 | yes |
| 4 releases, README, measurement | `.github/workflows/`, `README.md`, `scripts/coverage.sh`, `docs/coverage.md` | Opus (the lead reviews every number) | 05:30 (the tag run may finish later) | yes |
| 5 xml strategy + Windows Event | branch `lane-5-xml` only: `crates/ulpf-parse`, `parsers/windows_event.toml`, sample, fixture, `docs/parser-format.md`, `docs/DECISIONS.md` on the branch | Fable | 06:20, then push and stop | never (owner's go after the demo) |
| 6 entity index cost | branch `lane-6-index` only: `crates/ulpf/src/pivot.rs`, `engine.rs` output thread, `docs/DECISIONS.md` on the branch | Fable | 05:20, then push and stop | never (owner's go after the demo) |

Why not fewer: the six lanes touch disjoint trees (`ui/`; the server routes; `parsers/` with
`mappings/`; CI and docs; a branch of `ulpf-parse`; a branch of the index), share no state,
and each is two to three hours of wall-clock the others need not wait for; a builder doing
two of them in sequence would put either the front door (lane 1) or the honest numbers
(lane 4) after the freeze. Why not more: PROGRESS, DECISIONS and CLAUDE.md are lead-owned
(workers return text, the lead writes it), the demo path is frozen, and a seventh lane would
contend for the eight cores the measurements in lanes 4 and 6 need.
Sequencing named: lane 2's UI plumbing starts only after the lead's API commit (it builds
against `docs/api.md` v4, committed 03:10 before any dispatch); lane 4's coverage table is
regenerated by the lead after lane 3 merges (the Zeek http class rule and three new families
change the numbers); lane 1 merges before lane 2's UI plumbing, which is rebased on it (both
edit `App.svelte` and append their own section to `app.css`). Ports: lane 1 7891-7895, lane 2
7896-7899, lane 3 7901, lane 4 7902-7905, lane 6 7906-7910, the demo 7878 untouched.
Clarifications folded in at 03:25 and 03:40 IST (no restart): every kill timer is a ceiling, not a
schedule, and the aim is every lane through the gate well before 06:00, then a second adversarial
pass over the merged tree and rehearsal; three tiers (Fable: design, merges, review of worker
output, server state, the API contract, engine crates, the store; Opus: most implementation;
Sonnet: mechanical work to a spec the lead holds; Haiku banned). The lanes already running keep
their tiers (a restart was ruled out); every new dispatch follows the three-tier rule. Added lanes:
lane 2 split into 2P (the pivot's 500 ms, Fable, ceiling 05:00), 2T (the v4 server tests against
the contract, Opus, ceiling 05:15) and 2U (UI plumbing, Opus, ceiling 05:45); lane D (the demo
runner as `ulpf demo`, Opus builder, Fable review, ceiling 05:30: orchestration of existing
subcommands and the watch mechanism, same steps in the same order as the demo script, `--check`
and `--reset`, `scripts/demo.sh` a thin wrapper); lane 7 (Windows as a first-class target, Opus
builder, Fable review, ceiling 06:00: the installer reachable from the pre-release page, SmartScreen
named, the webview runtime bundled offline where the framework offers it, sidecar and data
directory through the platform abstractions, designed failure states, verified prerequisites,
a Windows CI job that installs and launches the app or falls back to the sidecar and the demo's
check mode, a new pre-release tag). Addition folded in at 04:07 IST (a Windows tester's report
against 14d3b0c; the tester's machine has a hardware fault, so random access violations there are
not engine evidence; the items below reproduce deterministically): on main through the full gate,
the release profile without LTO and a `dist` profile with fat LTO for the shipped binaries,
installers, Docker image and the harness (lane P, Opus builder, Fable verifier, dispatched 04:09,
owns Cargo.toml, the Dockerfile and `eval/`; README and CI lines applied by lane 4B), the Windows
quick start and the tester's contributed throughput line in README (lane 4B, after lane 4 merges),
the sidecar in a Windows job object, the locked-store message naming the holder, the bundle and
first-run copy excluding generated parsers, the packaging script honouring `CARGO_TARGET_DIR`,
which installer the smoke job exercised (lane 7B, after lane 7 merges), the demo reset purging
generated parsers and every documented command naming `samples/*.log` (the lead at lane D's
merge; a directory-level include or exclude for the engine is a post-demo decision, D83
reserved); on a branch, lane 8 (`lane-8-windows`, Fable builder and verifier, dispatched 04:08,
ceiling 07:05): the store reopen that truncates a torn tail under a live mapping (Windows refuses
it), the parquet watch-mode teardown handle, the null output device's stray metadata file and
wrong count, a Windows CI test job running the whole suite on the branch; and lane 3b
(`lane-3b-cef-leef`, Opus builder, Fable verifier, dispatched 04:03) for the two CEF/LEEF engine
defects lane 3 found. Dependencies named: lane 7 edits `.github/workflows/app.yml`
only after lane 4's YAML is on main (it merges main first); the smoke job's `ulpf demo --check`
step is lane 7's after lane D merges; lane 2T's tests pass only once the lead's `v4:` commits land.
Merge gate for every lane: `cargo test --workspace`, `cargo clippy --workspace --all-targets
-- -D warnings`, `cargo build --release`, `ulpf check --pending pending`, `scripts/demo.sh
--check`, `scripts/isolation.sh run` on the merged binary, and for a UI lane the lead's own
look at the captures and a grep of `ui/dist` for external references.

### Definition of done (each item checked only after running it)
- [x] L1. (merged 04:04 IST as 8dc1651, seven commits b86a434..0d4f424, D79) Flow screen: home
      (`#/` and `#/flow`, key 0, Esc from any top-level screen), six stations, inference branch,
      pending tray, chain head, live counters and a rate-proportional pulse from the metrics
      frame (the server's `rate`/`queue` when the frame carries them, else the delta between
      frames, labelled), reduced-motion static diagram, empty/loading/error states as sentences,
      one key per station (i s d r p n e), motion pass over the seven screens, captures under
      `docs/screens/`, `docs/design.md` Motion section. The Opus verifier reproduced on 7893: the
      six station numbers equal `/api/metrics` exactly (514/514/262/260/514/514), the tray badge
      equals the length of `/api/pending`, reduced motion gives `document.getAnimations().length`
      0 with the numbers still on screen (6 animations without it), every station key and click
      lands on its documented screen, Esc and 0 return to Flow, the empty state names the watch
      path, all seven screens render with zero console errors and the longest record's traceback
      included, `pnpm build` gives three files with the committed dist byte-identical and the
      served `app.css`/`app.js` hashes equal to the files. Two findings closed in the fix round
      (7efc5d8: Drift's `seen` was not `$state`, so `pnpm build` warned; the station eased its
      border colour on hover, which design.md forbids: the transition is gone and the selection
      snaps everywhere). The lead viewed flow, flow-reduced, empty-flow and keys at 1280 and the
      reconnecting capture. Recorded, not done: the 14 `tool-*` and 17 `app-*` captures of the
      desktop app still show the pre-Flow top bar (one index row says so; re-shot if lane 7 or I
      re-captures); `crates/ulpf/tests/replay.rs:154` raced once under load 30 (the first replay
      finished before the second was asked; passes alone and in the full rerun).
- [~] L2a. (18fab3e, D77, D78; the lead) API: `queue` and `rate` in the frame, `emitted_from`
      with the output-file lookup (`crates/ulpf/src/outfile.rs`, a binary search on the raw id;
      the file cut to its last terminator is the snapshot), `?bytes=0`, `GET /api/events/{id}/bytes`,
      `GET /api/export` (jsonl verbatim, csv as the eleven Parquet columns, `from`/`to`/`q`),
      `pivot_index` in status. Smoke on a live server (03:35 IST, `--tail 5`, twelve samples and a
      4,000,001-byte line, load 40 from the lanes): id 0 evicted from the ring came back with
      `emitted_from: output` and its own `raw_id`; the bytes route's body equals the dropped file
      byte for byte with `Content-Length` = `bytes_len`; a never-issued id is the JSON 404 on both
      routes; the export equals the output file (`cmp`), `from=5&to=9` gives ids 5..9,
      `q=DENY+tcp` gives 6 lines against an independent count of 6, the csv header and RFC 4180
      quoting hold, `format=xml` is 422. The 24 MB finding measured on the 4 MB record: the full
      JSON is 28,001,835 bytes in 0.43 s (`emitted` now adds a fourth copy of the 4 MB value),
      `bytes=0` 16,001,835 bytes in 0.06 s (the parsed `message` value still appears in
      `fields`, `provenance`, `normalized` and `emitted`), the bytes route 4,000,001 bytes in
      0.011 s; so `?values=N` was added (a cut per long string with `value_len`, `values_cut`)
      and measured at 03:37 on the same record with the rebuilt binary: full JSON 28,001,884
      bytes in 0.24 s; `bytes=0` 16,001,884 in 0.044 s; `bytes=0&values=4096` 18,274 bytes in
      0.025 s (`values_cut` 4: the `raw_message` field, its provenance, `normalized.message`,
      `emitted.message`, each with `value_len` 4,000,000); the bytes route 4,000,001 bytes in
      0.0025 s; a small record with `values=4096` has `values_cut` 0 and every `value_len` null.
      116 tests, clippy clean. The pivot's 500 ms and `elapsed_ms` are lane 2P's; the server
      tests are lane 2T's (three of four green at 03:30 against 18fab3e).
- [x] L2b. (lane 2U, merged 05:00 IST as eb4540e, seven commits 28bb681..c1696f3 plus merges of
      main, D90) Trust flags per tail row as two-letter marks with `f` for flagged-only, the
      filter over every field (terms, the export route's rule) with `e` exporting the filtered
      view (a download of 181,096 bytes, every line carrying the term), the traceback over
      `?bytes=0&values=4096` plus the bytes route (the 4 MB record: 1,267 ms to the ruler before,
      62-75 ms after), `emitted_from` and the cut named, the windowed rates with the window in the
      label and the run average beside them, the queue depth now with the high-water mark as a
      rule, the seen-with wording, eight `v4-*` captures indexed, design.md rows and the overlay
      keys. The Opus verifier reproduced twelve keyboard and count checks and nine screens with a
      clean console; its two findings (the overlay lacked `f`/`e`; design.md and the screens index
      lacked the rows) were closed by the builder's docs commit c1696f3 before the session limit
      stopped the fix round. The lead viewed v4-live-flags-1280. Known, pre-existing: the tail's
      header labels drift from their columns (`--cols` in em against two font sizes).
- [x] L3. (merged 04:01 IST as a9c8ac6, 360faec + 59c9ea8, D80) `cef.toml`, `leef.toml`,
      `cloudtrail.toml` from the specifications (cited in each header) with samples and fixtures;
      class rules and field lists in both mappings, additive (the lead checked the 51 replaced
      lines: each old list is a subset of its replacement); Zeek http rows classify once a
      proposal names the columns (1,531 HTTP Activity, 5 Network Activity, `class_unknown` 9 =
      the header lines, both schemas; conn 5,120 Network Activity); no JSON catch-all; nginx and
      Apache the first post-demo addition; Postfix held. The Opus verifier reproduced the gates
      (cef 14/14 parse_failed 0, leef 16/16, cloudtrail 15/15, both schemas; 12 original samples
      byte-identical through old and new mappings) and found two mapping defects closed in the
      fix round (a LEEF `sev` behind a syslog `<pri>` lost to `syslog_severity`; CloudTrail
      writes without `readOnly` carried no `activity_id`, which the schema requires: now 0
      Unknown) and two engine defects outside the lane, confirmed and dispatched at 04:03 to
      branch `lane-3b-cef-leef` (Opus builder, Fable review): CEF's header severity is named
      `severity`, the syslog scale's name, so 10 -> Other and 1 -> Critical; a LEEF 2.0 delimiter
      written `0xHH` splits on the literal `0` with no counted failure. Lead's gate at a9c8ac6:
      116 tests, clippy clean, 15 parsers 0 problems, demo check 18/18.
- [x] L4. (merged 04:58 IST as 1a7f05e, eleven commits 0ff8e98..f62ef5c, D85-D87) `cli` builds
      the static CLI for x86_64 musl (static-pie, stripped: 8,904,048 bytes from 57,032,312),
      aarch64 macOS and x86_64 Windows beside the installers on the one draft release with
      SHA256SUMS; `smoke-windows` ran the engine on Windows for the first time (check, run over
      the samples with framed asserted against the non-empty line count, verify, serve answering
      `/api/status`, a drop emitted, stopped clean); tag `v0.1.0-rc1` green with all seven jobs
      (https://github.com/techlogist1/ulpf/actions/runs/33995222954), the draft holds eight
      assets and is not published; README rewritten as a front door with one headline (258,411
      events/s, harness median, `-j 7`, output written) and every other figure labelled;
      `scripts/coverage.sh` and `docs/coverage.md` (15 samples, 17 real, 29 generated corpus
      files, every number from `--report-json`). The Opus verifier's seven findings (numbers one
      format short of the merged tree, the format table, coverage rows, an unlabelled index-on
      figure) closed in the fix round on the merged tree (CI green at 33997822506 and
      33998224513). Known: the gh token lacks the `workflow` scope, so workflow files push only
      over SSH (`ssh://git@ssh.github.com:443/techlogist1/ulpf.git`); the rc1 binaries predate
      lanes D, I and 2T (7B tags rc3); `crates/ulpf/tests/server.rs:209` is a timing assertion
      that failed once at load 12 (saw 96 of 250) and passes alone.
- [x] L5. (pushed 03:59 IST, `origin/lane-5-xml` at 1b8aa19, D75 on the branch) Coherent, all
      of it: the seventh strategy `xml` on `xmlparser` 0.13.6 (MIT/Apache-2.0, zero deps; quick-xml
      measured at 23 allocations per parse and ruled out), values borrowed, entity-bearing values
      the one materialisation, dotted keys from a pool so a plain line allocates nothing after
      warm-up; `parsers/windows_event.toml` from the 4624/4625/4720 and Sysmon pages, a 14-line
      sample and reviewed fixture, mapping fragments for both schemas, 118 tests, clippy clean,
      every other sample byte-identical against main's binary. The Opus verifier found three
      defects, closed on the branch: the `parse_failed` counters were sized by hand at 6, so the
      seventh failure reason panicked a worker (now sized from `ParseFailure::ALL` in
      `crates/ulpf/src/metrics.rs`, an engine file outside the lane, named in D75 for the
      merge); a quadratic allocation on repeated unnamed elements (now on the stack, asserted by
      the alloc test); a hexadecimal `ProcessId` under an `int` field (`as_int` reads `0x`,
      `crates/ulpf-normalize/src/mapping.rs`). Not merged tonight by rule: endpoint telemetry
      outside the perimeter line, kept as evidence of extensibility.
- [x] L6. (pushed 04:49 IST, `origin/lane-6-index` at 9389be7, contains main at d62a01c; D76 on
      the branch; never merges tonight) The profile named the cost: SQLite's 2 MiB page cache
      spilling the three value-keyed B-trees to the WAL several times per transaction and the
      checkpoint copying them again (93% of the pivot thread in pwrite/pread). Fix: a 64 MiB page
      cache (`CACHE_KIB`, `PRAGMA cache_size`) and one transaction per drained group. On the
      468k-event slice, index on: main 31,118 events/s median, the branch 49,735 (at higher load),
      back to back at the same load 9,497 -> 48,424 (5.1x); the cache alone is the gain (58,611
      with the old group of 8), the group a few percent; answers identical (pivot on three
      entities over both indexes). The cliff: at 5M never-repeating events (index 1.57 GB) the
      branch runs at 7,142 events/s (10,005 with 256 MiB), sys-bound again, so the cache is a fix
      while the index fits and a knob past it; the two feeds that remove the cliff (sorted runs
      merged, or an index fed from the output file that lags instead of blocking) change D55 and
      are named, not built. One soak with the index on ran at unequal load to run 6, so the UDP
      comparison is not settled; the A2 demo rule (TCP or file, never UDP with the index on)
      stands. 122 tests, clippy clean, `crates/ulpf/tests/pivot.rs` unchanged. The Opus
      verifier's four findings (a misreported merge state, a gap that was not one, a stale
      sentence in D76, a bound the code does not enforce) closed in the fix round. A later merge
      touches pivot.rs and docs/DECISIONS.md only.
- [x] L2T. (merged 04:17 IST as 50b288f, `crates/ulpf/tests/v4_api.rs`) Four contract tests
      against the merged main: queue depth and the windowed rate in the frame; `emitted_from`
      tail then output on a five-event ring, `?bytes=0`, the bytes route byte-for-byte against
      the sample's first line, 404 on both routes; the export as ndjson equal to the flushed
      file, `from`/`to` inclusive, `q` case-insensitive against an independent count, the csv
      header as D64's eleven columns with RFC 4180 quoting; pivot paging by the cursor pair
      with `elapsed_ms` as five f64s. Not exercised: the trust-flags table, export's 404 on a
      device output, `after` paging, the filename shape, `?values=N`.
- [x] L2P. (merged 04:12 IST as 2027391, 986154b, D81; the lead's gate: 116 tests, clippy clean, demo check 18/18) `elapsed_ms` on every pivot page; the
      related scan on four connections without the SQLite mutex, through mmap, borrowed blobs, a
      bitset; pages byte-identical to before; the lead's gate on the merged tree.
- [x] LD. (merged 04:16 IST as 82378fc with the follow-up 97934a9 at 04:26 amending D67; the
      lead's two runner fixes d6d5a77 at 05:34 and 7c223e3 at 07:04 are on main since; the item
      was left unchecked at the merge and is checked here) `ulpf demo` plays the PROGRESS demo
      from the binary with `--auto`, `--check`, `--reset`; `scripts/demo.sh` the wrapper; the
      reset purges generated parsers from `parsers/`; a full `--auto` pass on the merged binary;
      the Windows smoke job runs `--check`, which landed with lane 7D (466d79b).
- [x] L7. (lane 7 merged 05:03 IST, nine commits c6400e3..26d0bbd, D89; 7C/7D merged 07:19 IST
      as 7dc8b9b, twelve commits 25388fd..02b4bef including its merge of main 5928e35, 15 files
      +587/-55, D92-D94)
      Lane 7's builder hit the session limit before returning, so no structured report and no
      verifier ran: the lead reviewed the diff (11 files, +367/-25: the offline WebView2 installer
      mode, the failure sentences on the splash with `engine.log` named, the pinned-port hook,
      the three Windows differences commented in ingest.rs, `sidecar.ps1`, `smoke-windows.ps1`,
      the README's Windows section from the installers, three captures) and its CI: the branch is
      green (https://github.com/techlogist1/ulpf/actions/runs/33998207341) and `app-smoke-windows`
      installed the NSIS build into AppData\Local\ULPF, saw `server.url`, and printed `orphan:
      ulpf.exe pid 1904 outlived a Stop-Process of the window`, which is the tester's finding
      reproduced and the reason for 7B's job object. Tag `v0.1.0-rc2` is on the lane 7 head (run
      33998466623; its draft held the two macOS assets when read at 05:00). Lane 7D (Opus builder,
      Opus verifier; the verifier's verdict was fix with four findings, all closed by the fix
      round) carried 7B's items on the right base and closed them: a Windows job object with
      `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` held for the app's life, so the kernel reaps the
      sidecar whatever the exit path (D92, `app/src-tauri/src/job.rs`); a store another writer
      holds is a splash sentence naming the store and the holder pid with one button that stops
      it and restarts through the ordinary path (D93, `app/src-tauri/src/holder.rs`); generated
      parsers excluded by their own `origin = "inferred"` at all three places they could leak --
      the first-run copy, which logs which file it skipped, and `sidecar.sh`/`.ps1`, which exit 1
      naming each refused file (D94, `lib.rs` and `app/scripts/sidecar.sh`); `--profile dist` in
      app.yml's `cli` and `bundle` jobs honouring `CARGO_TARGET_DIR`, with `smoke-windows`
      deliberately left on `--release`; `ulpf.exe demo --check` in the smoke job against the
      installed engine. Verified by the builder on the M1: `cargo test --workspace` 123 passed,
      app `cargo test --lib` 9 passed, clippy clean, `pnpm tauri build` producing ULPF.app and
      `ULPF_0.1.0_aarch64.dmg`, the bundle answering `server.url` inside 0.5 s with
      `samples/cisco_asa.log` through the watch directory at 30 each framed through emitted and
      `no_parser` 0, `osascript quit` leaving no `ulpf-app` and no engine, and the first-run
      exclusion proved on the built bundle (16 definitions in `Resources/parsers`, 15 in the data
      directory, `engine.log`: `shell: 1 generated definition(s) not copied into <data>/parsers:
      mikrotik_firewall.toml`). CI on the branch: run 34002152105 (fb7bda9) green over 8 jobs,
      `app-smoke-windows` reading `no ulpf.exe left 500 ms after the window process was
      force-killed (ceiling 5 s): the job object reaped it`; run 34003825762 (e2ce03b) green over
      all 8 jobs in 13m36s. The four verifier findings closed: not rebased -> `git merge main`
      (5928e35, merge-base 217d0df); D92-D94 dangling in `app/README.md` -> written into
      docs/DECISIONS.md at 1721/1741/1757 (e2ce03b); the orphan assertion claiming `5 s` where it
      measured a 500 ms poll -> the script now prints the measured elapsed against the named
      ceiling; the screens index at 24 rows for 28 app captures -> 28 rows (02b4bef), including
      `app-error-locked.png` for D93. Tag `v0.1.0-rc3` (annotated fec0119) points at fb7bda9,
      three commits behind the branch tip (one PowerShell log string and two markdown changes);
      its draft release run 34002380126 is green with 8 assets and unpublished. The tag was
      deliberately not moved: moving it deletes the remote tag and re-fires a ~14 min release
      job, so the owner decides after the demo. Run 34004510572 (02b4bef, docs-only) was still
      `in_progress` when this merge landed at 07:19 IST and was not waited on. Gaps, recorded not
      papered over: the DragDrop handler was never exercised by hand (every file was fed through
      the watch directory); `sidecar.ps1` has never been executed (no pwsh on this Mac, and CI
      runs `sidecar.sh` under bash on Windows too); and the branch's macOS bundle pass is the
      earlier build's, the fix round having touched no Rust, script, bundle config or `ui/`.
- [x] LI. (merged 04:17 IST as 20a66c2, eight commits 3e85c8a..f68781d, D84) Intensity: Low /
      Balanced / Max with the machine's core count and the index state, persisted in
      `app_config_dir/intensity`, applied at sidecar start, a clean restart on change with the
      notice on the live page, the choice and live thread count in the title from `/api/status`,
      seven captures indexed. The Fable verifier rebuilt the bundle and drove it: Low 2/off,
      Balanced 4/on, Max 7/on in `/api/status` and on the child's command line, one serve child
      after rapid Low-then-Balanced clicks, the title says `restarting` within 1.6 s when the
      settings file disagrees with the engine, Quit leaves no `ulpf serve`; two findings closed
      (app/README's Menus paragraph contradicted the new section; the captures were unindexed).
      Not verified on Windows (the config path and TerminateProcess are named in comments).
- [x] LP. (merged 04:59 IST as 6752d02, adfafcc, D88) `release` without LTO (11,778,856
      bytes), `dist` with fat LTO and one codegen unit (8,777,544 bytes, the pre-split release
      within 96 bytes); the Dockerfile builds `--profile dist` and proves the binary static, the
      harness builds and measures dist and prints the build it declared, docs/evaluation.md says
      which build the numbers come from; measured within noise on this M1 Pro (best-of-8 dist
      1.791 s against release 1.690 s). The Fable verifier confirmed the flags in `-v` output, a
      cold harness build through run.sh, and a `--no-cache` docker build (230 s, static, 304
      events emitted under `--network none`); its two findings (five `target/release` hits missing
      from the grep table, all "stay"; scripts/README.md:14 to follow CI's profile) were docs and
      are carried by 4B and 7B since the fix round hit the session limit. README's profile
      sentence and CI's `--profile dist` are lanes 4B and 7B. The harness re-run on dist in a
      quiet window is lane 4B's.
- [x] L4B. (merged 06:02 IST as 87877a4, nine commits d0af71d..61233b3, D91) Every documented
      command names its log files: README's Run-it command and both Quick start `run` lines, the
      docker isolation line, scripts/README's isolation and `ULPF_FEED` lines (`grep -n 'run
      samples' README.md scripts/README.md docs/evaluation.md` returns three hits, all
      `samples/*.log`). The Run-it counter block was regenerated verbatim from the glob and the
      verifier diffed it against its own run, identical: 15 files, 309 events, no_parser 2,
      inference runs 0, against 16 files, 354 events, no_parser 41, class_unknown 106 from the
      bare directory; the 45 extra events are `samples/README.md`. The correction the lane made
      to its own brief and measured twice: the bare directory produces NO junk proposal. At the
      default threshold and at `--infer-threshold 8` the 41 unknown lines give `runs 1  lines
      templated 0  unmatched 39  proposals written 0  skipped [no_templates 1]` and the pending
      directory holds only its empty `approved` and `rejected` - the clustering already refuses
      prose, so the cost is the counter block a reader is asked to trust, not the review queue
      (D91). `samples/README.md` alone is `no_parser` 39, not 41: 41 is the whole-directory
      total, 39 plus the 2 the real samples carry, and scripts/README said 41 and was fixed. A
      `### On Windows` second fence gives a Windows reader the same nine commands in PowerShell
      (`.exe`, an `$env:TEMP` path rather than `NUL`, `(Get-ChildItem samples\*.log).FullName`
      because the engine takes files and not patterns); a second fence deliberately, because
      `eval/run.sh`'s cold_start `eval`s every line of the FIRST fence under "## Quick start" in
      a fresh clone, so a PowerShell line inside it would fail the criterion - the extraction is
      1090 bytes on both sides of that commit (1078 before the lane opened; the two `/*.log`
      suffixes are the 12), and `docs/evaluation.md` holds no command list to diff
      against (0 bytes), the README fence being the single source `eval/tools/ulpf.toml`'s
      `[cold_start]` reads. "Which build" now claims only what it can grep: dist is the Docker
      image and `eval/tools/ulpf.toml`, not CI, which builds every asset with `cargo build
      --release` (app.yml:74, 134, 204, no profile override) as both sidecar scripts do; the
      committed scorecard's `--release` header is answered as pre-split, when release carried
      dist's fat LTO. The machine state beside each number is now only what a log holds: the
      quiet run's three pre-run gate samples (118, 147 and 130 percent of one core) and no
      during-run figure at all, because that sampler counted ulpf's own threads as everything
      else (`ps -o comm` splits on this repo's spaced path, so ulpf read 0 in all 36 samples);
      the loaded set carries its before-run loads 4.99 and 5.85 against 2.91 for the quiet one,
      the observed range 4.99-21.58, and the cold page cache that explains its 32.6 s first run.
      Re-measurement, recorded and not promoted: the quiet window on the dist build gives
      310,849 / 295,928 / 290,478 events/s, median 295,928, 14.5 percent above the committed
      258,411 and outside its 10 percent band (284,252); the verifier's independent run in its
      own quiet window read 341,018 / 298,936 / 320,369, median 320,369, 8 percent above the
      lane's and also above the band. The headline stays 258,411 because D87 pins the quoted
      figure to a committed scorecard, README now reads it as a floor, and no scorecard was
      committed (`eval/` is lane P's). No contributed Windows throughput line: the tester's
      report is in neither the repo nor `gh issue list` nor `gh pr list` (`grep -nic contributed
      README.md` is 0), so no figure was invented and the line shape was returned instead. The
      pivot line names the five entities `mappings/ocsf.toml`'s `[entities]` actually holds
      (src_ip, dst_ip, user, dst_port, device); there is no `hash`. Lane gate: 122 tests 0
      failed over 38 suites, clippy clean, `demo.sh --check` 39 ok and no drift, every
      executable Unix command in the Quick start run once at exit 0, `isolation.sh docker` PASS.
      Stated limits: the PowerShell block and the docker isolation were inspected, not executed
      here (no pwsh on this Mac; the docker run substituted lane P's verify image of the same
      Dockerfile), so the Windows block is reasoned from PowerShell semantics and lane 7D's smoke
      job now runs `demo --check` on the installed engine, though the README block itself is
      still unpasted; the container command in "Get it" still names the mounted
      `/data/samples` because the `scratch` image has no shell to expand a glob, and README says
      so and names the cost where the command appears (the real fix is D83); `docs/coverage.md`'s
      header still stamps `commit 0c197bc` and `2026-09-05 23:04 UTC`, three merges stale,
      unregenerated because `scripts/coverage.sh` already runs one file at a time so its numbers
      should not have moved. The three out-of-lane repeats of "what CI ships" (D88 in two
      places, `docs/evaluation.md`, Cargo.toml's `[profile.dist]` comment) were applied in that
      commit as "still use `cargo build --release` ... and move to `--profile dist` with lane
      7C", and all four were rewritten to the past tense at lane 7D's merge, once app.yml on
      main was confirmed to build `--profile dist` in the `cli` and `bundle` jobs (lines 80 and
      141) and to stay on `--release` only in `smoke-windows` (line 213).
- [x] L3b. (pushed 04:30 IST, `origin/lane-3b-cef-leef` at c6e13ca, a9c8ac6 + 4, never merges
      tonight) CEF's seventh header field is `cef_severity`, bucketed in both mappings on
      ArcSight's own ranges (0-3 Low, 4-6 Medium, 7-8 High, 9-10 Very-High to Critical; 14 of 14
      sample lines in the spec bucket, both schemas); LEEF 2.0's delimiter reads `xHH` and `0xHH`
      in either case, and a prefix whose digits are not hex (`0xZZ`, `0x+5`) is a counted
      `invalid_leef` instead of a fall back to tab (samples/leef.log line 17 carries `0x5E`,
      17/17 parsed). 117 tests, clippy clean, the alloc test prints per-family numbers under
      `--nocapture` (allocations equal materialised values in every family), the twelve original
      samples byte-identical through main's binary and the branch's on both schemas. The Fable
      verifier's one defect (the samples README row) closed in the fix round. Gaps recorded on the
      branch: the string form `Very-High` is not in the Critical bucket; a multi-byte delimiter
      without a prefix keeps its first byte (spec-undefined); a counted `invalid_leef` carries no
      header fields (the pipeline's Err shape for every strategy). Merge after the demo: the
      mapping additions are inside the severity enums, so the rebase onto main is trivial.
- [x] L8. (pushed 04:53 IST, `origin/lane-8-windows` at 80a5bfc, based on 14d3b0c, D82 on the
      branch; never merges tonight; ready for the owner's go after the demo) Three deterministic
      Windows failures fixed with a test each: (1) the store reopen truncated a torn tail under a
      live mapping, which Windows refuses (os error 1224): recovery now reclaims the tail in
      place (the bytes beyond the recovered end are zeroed, the writer resumes there, neither
      file is ever shrunk), the walk-back checks the last entry's digest and chain link and not
      only its shape, and `RawReader::open` finds its logical end by dropping trailing entries
      that are not a record while keeping a wrong-digest record for verify to name; test
      `a_reader_mapped_across_a_reopen_keeps_its_records_and_no_file_shrinks`
      (crates/ulpf-store/tests/roundtrip.rs; fails on the old code with "left: 3448 right:
      3464"); the on-disk format is unchanged and D52/D56 survive (the reader still bounds to the
      flushed count). (2) The parquet watch-mode teardown held files open after stop (os error
      32): `Live.store` is now `Mutex<Option<RawStore>>` behind `Live::store()` and
      `Live::close()` runs on every exit path of run and serve, dropping the store and the pivot
      read connection; test `stop_releases_every_file_the_engine_opened` (crates/ulpf/tests/
      stop.rs; with close neutered it names catalog.sqlite, its WAL, raw.seg and raw.idx). (3)
      The null device: `output_is_sink` knows `NUL` on Windows, a device output leaves no meta
      file, and the version meta counts what the file holds (the live count when the output
      started empty, else a line count); tests `a_null_device_output_leaves_nothing_beside_it_or_
      in_the_cwd` and `the_version_meta_counts_the_lines_the_output_holds` (crates/ulpf/tests/
      output_meta.rs). Needed for a green suite, outside the three: the UDP receive buffer is
      raised on Windows through Winsock (D74's no-op replaced). Windows job
      `.github/workflows/windows-tests.yml` on the branch: baseline run 33997160230 FAILED three
      targets (the three failures as the tester saw them), 33997927604 FAILED two, 33998281457
      SUCCESS with 39 `test result: ok`, 33998494780 SUCCESS (docs push). Mac: 26 suites green,
      clippy clean, the samples byte-identical through old and new binaries. The independent
      verifier did not run (the session limit); the lead read the store diff (digest and chain
      on the walk-back, zero-fill, the reader's logical end) and the run list. Gap the builder
      recorded: a reader in another process can observe the zero region between the writer's
      zero-fill and its next append (dropped by the walk-back; nothing in the format marks the
      logical end explicitly).
- [x] LU. (merged 07:58 IST in 06ab4fb, `lane-u-ui` head eee630f, nine commits plus three fix-round
      commits, D95) The tail frame's honest cut count: `TailFrame` carries an additive `cut` set by
      `since()` and published in the server's tail JSON, `skipped` keeps its published meaning (the
      total a caller did not receive) and `skipped - cut` is the eviction, the only part that is
      gone; the `docs/api.md` sentence that called `skipped` eviction -- the source of the
      misleading tooltip -- is corrected and a "Tail frame" subsection added; the footer says which
      is which. The new `the_tail_frame_separates_eviction_from_the_frames_own_limit`
      (crates/ulpf/tests/v4_api.rs) asserts it over real HTTP on a 64-event ring fed 309 events:
      `?limit=500` cut 0 and skipped 309-1-64, `?limit=8` cut 56 with skipped == evicted + cut, a
      caller two events behind 0 and 0, and a cut line still in the ring on the next full frame.
      Also `fmt.stamp` renders the zone the value carries rather than the viewer's local time, and
      the confirmation is kept on screen with scroll-margin plus `scrollIntoView({block:'nearest'})`
      rather than moved into an overlay. Its verifier confirmed 124 tests on the branch and the tail
      arithmetic over HTTP, and its fix round closed all three findings (the 12em stamp columns --
      the zone decision cost a millisecond digit at 11em -- the merge of main, and the footer's gaps
      at 900px so the cut note is not pushed off the edge). Gaps the lane recorded: the footer has to
      name the per-tick frame limit (200) in prose because `/api/status` reports `tail_capacity` and
      nothing reports `TAIL_PER_TICK` (server.rs:704), so the tooltip goes stale if that constant
      changes; `engine.files` counts inputs opened, not files in the watch directories, so Flow's
      note says "M files watched" with the caveat in its title; and nothing in the metrics frame
      separates "sources seen this run" from "sources this store knows", which is why the ingest note
      has to say "this run".
- [x] LU2. (merged 07:58 IST in 06ab4fb, `lane-u2-review` head 1e9e2f3, two commits, D99) The review
      screen's confirmations name the real directories: Approve/reject confirmations, notes and the
      proof row hardcoded `parsers/` and `pending/rejected/` while the demo server runs with
      `demo/parsers` and `demo/pending` and the desktop app points at its own data directory, so the
      confirmation named a path that held nothing; they now read `parsers_dir` and `pending_dir`
      from live.status. The same commit added the missing `m` row (load more diff entries on Replay)
      to the `?` overlay, which the documented keyboard map already listed, and 1e9e2f3 corrected two
      docs/design.md claims against the code (the foot inventory was missing the queue item; the tail
      row is ten flattened fields, not seven strings). Four files, +16/-10. Verdict pass. It has no
      test line of its own -- it is covered by the merged tree's 125 and by the dist rebuild; it
      deliberately did not commit `ui/dist`, which is why the integration's dist rebuild was not a
      no-op.
- [x] LA. (merged 07:58 IST in 06ab4fb, `lane-a-shell` head 81d1dbd, five commits, D97 and D98) The
      desktop shell's exports and recovery buttons, all in `app/` plus docs/screens/README.md, zero
      engine files. New `app/src-tauri/src/download.rs` (171 lines: `save()`, the loopback GET with
      Content-Disposition and de-chunking, `name_for`); `lib.rs` gains `SAVE_SCHEME`, `INTERCEPT`,
      `window()` building the window in code against `"create": false`, a `Retry` enum,
      `down`/`splash_with`/`stop_holder`, `focus_webview` and an orphan pre-flight in `start()`;
      `app/dist/index.html` labels the button per fragment flag and hides the drop hint when the
      engine is down; `app/README.md` gains the file-links section, the failure/button paragraph, the
      macOS force-quit sentence and 15 parsers in the Windows payload; docs/screens/README.md's four
      parser counts corrected to 15 seeded / 15 loaded / 16 after approve, each naming the capture's
      own older number, the captures themselves unchanged. Numbers (builder, reproduced here): in
      app/src-tauri `cargo test --lib` 11 passed 0 failed (was 10; download.rs adds two) and
      `cargo clippy --all-targets -- -D warnings` rc 0; six `pnpm tauri build` runs with every claim
      driven on the last bundle; saves of attestation.json (597 bytes) and out-first-last.jsonl
      (32,206) / .csv (38,969) into ~/Downloads byte-identical to curl. Its independent verifier had
      not returned when this merge was dispatched at 07:58 and its verdict is still pending: the
      builder's claims stand on the integration check and on this gate, which re-ran the app tests
      and clippy on the merged tree. Gaps: nothing is measured on Windows -- the interceptor is
      unexercised on WebView2 and `app-smoke-windows` does not touch the export; the v4 export
      contract has no `filename` parameter, so the shell takes the server's own
      Content-Disposition name and falls back to the URL's last path segment; `GET /api/export`
      answers `Transfer-Encoding: chunked` and docs/api.md does not say so, so a naive reader
      interleaves chunk-size lines into the file; the de-chunker's read timeout is 3 s per read, not
      total, verified only against a 38,969-byte loopback export; why WKWebView drops the anchor
      download without calling any delegate is unexplained (empirical -- wry 0.55.1 and
      tauri-runtime-wry 2.11.4 were read and everything is wired), and Tauri 2.11.5's own
      `on_download`/`on_new_window` are measured dead for these links on macOS and are not in the
      tree. The Mac force-quit truth, plainly: `kill -9` of the app leaves the engine running on
      macOS; the locked-store page with its button (D93) is the recovery, not the job object (D92),
      which is Windows only.
- [x] LPV. (merged 07:58 IST in 06ab4fb, `lane-pv-paging` head da002ec, one commit, D96) The pivot
      paging flake is fixed, and it was never a race. `PivotIndex::walk` kept only the first
      `limit*4` postings past the cursor as candidates before sorting them by device time, so an
      event whose device clock ran behind sorted onto a page it had already been dropped from and
      the `(time, raw id)` cursor then paged past it for good; the cap now only ends the scan and
      never drops an entry, and `query()` wraps the header and the timeline in one read transaction
      because the writer commits a batch's entity counts and its posting rows together. Scope check:
      crates/ulpf/src/pivot.rs (+18/-4) and crates/ulpf/tests/pivot.rs (+69) only, no engine crate
      touched, docs/api.md unedited because nothing in the contract changed -- it already promised
      events are "neither repeated nor skipped" and the code now does what that says. Numbers:
      the new deterministic test saw 20 of 200 events before the fix (= `limit*4` exactly) and 200
      of 200 after; the builder measured 2 failures in 50 loops before and 50 of 50 after under six
      `yes` load generators; its verifier paged the seven busiest entities on main's pre-fix binary
      over all 15 samples and found 6 of 7 skipping (src_ip 203.0.113.9 38 of 51, dst_port 443 33 of
      36, user jdoe 31 of 33) against 7 of 7 exhaustive on the branch, with the pre-fix skip
      repeating to the row across three runs; end to end on the release binary dst_port 443 pages 36
      of 36 distinct in 8 pages of limit 5; the million-posting page 0.009 s -> 0.017 s against its
      own 1.0 s bound, the index write unchanged. The integration tree reproduced it: 1 failure in
      45 isolated runs pre-merge, 45 of 45 post-merge at the same load. Gap, named in the code and
      not documented away: the ceiling is not lifted -- an entity with more than about 10,000 events
      can still in principle hide an event whose device clock is behind by more than the scan's
      window, and closing that needs a first_time/last_time column on `postings`, an index-format
      change. The pivot screen was driven by curl and the API test, not opened in a browser.
- [x] LMT. (merged 07:58 IST in 06ab4fb, `lane-mt-plan` head e6fdde8, five commits) The hand-test
      plan, one new file: `docs/manual-test.md`, 517 lines -- 13 macOS CLI steps, 11 PowerShell
      steps, 41 app steps, results tables and 11 known limits, every expectation cited to the file it
      was read out of rather than remembered, and an expectation with no source in the tree says so
      and asks the tester to record what they saw. Docs only, no tests. Its fix round closed all
      nine findings: the "(with lane 7C)" markers are gone, `git show lane-7b-app:` prefixes became
      real paths (holder.rs and job.rs, not a guard.rs that lived only on a dead branch), the
      pre-7C halves of the two contrast rows read "Before lane 7D" so both sentences stay true of
      the merged tree, the dist-profile sentence was rewritten against .github/workflows/app.yml
      (the `cli` and `bundle` jobs build `--profile dist`, the Windows smoke job is deliberately
      `--release`, so the plan says which build a tester holds), and A36/A38 were rewritten because
      lane A landed in the same tree. `README.md:354` carries its line under "Where things are".
- [x] LR. (merged 08:44 IST in 1ce7652, `lane-r-reset` head be8748b, two commits: 014d393 the reset,
      be8748b the app/README section) The desktop app empties its own data from inside the app.
      `File > Reset…` (`CmdOrCtrl+Shift+R`, placed after `Open output folder`) shows the splash under
      a third fragment flag `?` -- a question, not a failure -- naming the data directory, with three
      buttons: `Reset events, keep approved parsers` removes `store/`, `out.*` (by prefix, so the
      `.pivot` and every `out.vN.*` go too), `watch/`, `pending/` and `staging/` and keeps `parsers/`,
      `mappings/` and `engine.log`; `Reset to first launch` removes the whole data directory and the
      ordinary start re-seeds the 15 bundled parsers, generated ones excluded (D94); `Cancel` touches
      nothing and navigates back to the served UI or to the page that was showing. Either reset stops
      the engine, waits until `holder::find` says nothing holds the store, deletes, and re-enters
      `start()` exactly as a launch does; a path that will not go is a line in `engine.log` and a
      count in the notice (`. N item(s) could not be removed; see engine.log`), never a panic. The
      notice reads `Reset: events removed, N parsers kept` or `Reset to first launch: 15 parsers`.
      The engine is never asked to delete: D100 records why. Scope check: app/ only -- app/README.md,
      app/dist/index.html, app/src-tauri/src/{lib.rs,menu.rs,reset.rs} (+271/-19, reset.rs new),
      no crates/ and no ui/, so the 56 s demo pass was NOT required for this merge and was not run.
      Numbers: `cargo test --lib` in app/src-tauri 12 passed 0 failed (the 11 that were there plus
      `reset::tests::each_choice_removes_exactly_its_own_paths`), `cargo clippy --all-targets -D
      warnings` rc 0, `cargo test --workspace` at the root 125 passed 0 failed (unchanged -- app/ is
      its own workspace). The builder drove the bundled app under a private HOME on port 7931, never
      the owner's data directory: 30 events -> Reset events -> framed 0 with 15 parsers kept; Reset
      to first launch -> the data directory recreated, 15 parsers seeded, framed 0; the menu clicked
      through System Events (`osascript`), the buttons through the page. Verification, plainly: the
      lane's independent verifier was STOPPED by the lead at 08:37 because its test instances were
      opening windows on the owner's screen during the demo, so its verdict does not exist. What
      stands instead is the builder's own private-HOME drive above, the app crate's 12 tests and
      clippy on the merged tree, and the lead's hands-on test of the installed bundle -- the lead
      installed this lane's own bundle at 08:28 as `/Applications/ULPF.app`, sidecar sha256
      55b52b87931faf63b79e2af1cfe298686f79b72d07c0382dc50105ec85ea951c, the same binary as
      `target/dist/ulpf` at 4e0d71d, and is driving it. Gaps, not closed: `Cancel` while the engine
      is DOWN was set up (a second instance on an occupied port lands on the `port in use` page with
      its Start again button) but never driven end to end; Windows untested, where `remove_dir_all`
      can fail on an open file -- that path is the `engine.log` line and the notice count, not a
      crash; no capture in `docs/screens/`; no hidden CLI argument was added.
- [ ] Final sequence 08:30-09:30 in order, then the nine-section report plus the stage order.

### Verified state (v4, rolling; every line was run, not read)
- 08:58 IST, the lead's own record of what the agents could not see. PUSHES: this session's lead
  pushed main three times over the SSH URL `ssh://git@ssh.github.com:443/techlogist1/ulpf.git`
  (the https remote's token lacks the workflow scope, so workflow files push only that way): 5b27f68
  at 08:22, 20d1890 at 08:30, 06cd41a at 08:47; `git fetch origin main` after each showed main ==
  origin/main. The 08:24 In flight sentence that says the pushes came from outside this session is
  right about the agents and wrong about the session: they were the lead's. INSTALLS: the lead
  installed three bundles into /Applications/ULPF.app: lane 7D's (07:44, for the mentors, its data
  seeded with samples/*.log and heldout/mikrotik.log), the 4e0d71d bundle (08:10, by the final-half
  executor), and lane R's own bundle at be8748b (08:28, while R's verifier still ran); the data
  directory was reset by the lead at the owner's request at 08:09, 08:25, 08:28, 08:37, 08:42,
  08:50 and 08:57 (quit, remove, relaunch), which is the "something outside this session" the
  08:11 and 08:24 entries saw. HANDS-ON DRIVE of the installed lane R bundle, 08:43-08:46, with the
  computer-use tools on one clean instance from first launch: samples of 100,000 Cisco ASA lines
  (samples/cisco_asa.log repeated) through the watch directory: framed, stored, detected, parsed,
  normalized and emitted 100,000 each, the Flow screen reading 10,105 events per second over its
  own 9.9 s window, Live rows cisco_asa, Traceback lighting 15 byte ranges with the digest
  recomputed equal and the chain link shown; File > Reset... > Reset to first launch back in 5 s
  with the toast `Reset to first launch: 15 parsers`; a 1,000-line HPC slice: 1,000 no_parser, one
  proposal (1 template, 1,000 of 1,000 verified, six slots each with a reason), the approve
  confirmation scrolled into view, Enter: `parsers loaded 16, 1,000 of 1,000 buffered lines take
  the fast path`; Reset events keep approved parsers: toast `Reset: events removed, 16 parsers
  kept`; a second slice of a different message type was not claimed (detected 0, no_parser 1,000,
  a second proposal), correct engine behaviour and a demo-material finding. DEMO MATERIAL in
  ~/Desktop/demo-logs (not in the repo): hpc-test.log is every 433rd line of the owner's 433,489-
  line HPC.log (433,490 events, the last line unterminated); `ulpf infer` on it gives 25 templates
  with a matcher regex over the message families; hpc-test-2.log is 1,000 lines chosen so that
  parser parses every one (CLI: detected 1,000, parsed 1,000, failed 0; before the selection a
  sampled slice parsed 973 with 27 pattern_no_match, and the engine turned those 27 into a new
  proposal when the source went quiet, which is the product working and not a defect);
  samples-15-devices is samples/*.log with each file's most frequent address rewritten to
  203.0.113.9 (CLI pivot src_ip: 72 events across 14 sources); two double-click .command files
  reset the app from Finder, one runs the dist binary over bench/mixed-5000000.log (08:52, other
  work running: 5,000,000 events in 21.0 s, 237,643 events/s, 72.5 MB/s), one flips byte 100 of the
  app's raw.seg so Integrity's verify names raw id 0. PROCESS facts the records lacked: the
  session limit hit at about 04:55 and the owner resumed the session in low-priority mode at about
  05:15; 7B was dispatched at 05:05 and stopped at 05:19; lane 3b's worktree had also been created
  at a stale base and reset itself (nothing of the kind is recorded for 4B); the adversarial
  review was launched twice, the first launch at about 06:00 with every agent inheriting the lead's
  model, stopped within minutes at the owner's instruction, and relaunched at 06:04 with Opus
  finders and Sonnet skeptics, which is the run the 07:04 entry records. The owner's brief changed
  during the morning: no 10:00 slot; mentorship sessions from 07:45 on the installed app; a
  two-minute demo, a video and the upload before 11:00.
- 08:45 IST: the merge gate on main at 1ce7652, the merge of lane R (`lane-r-reset`, head be8748b)
  into 20d1890. The merge was clean -- no conflict in any file -- and `git diff 20d1890..HEAD --stat`
  is five files, all under app/ (`--numstat`): app/README.md (+25/-1), app/dist/index.html
  (+47/-12), app/src-tauri/src/lib.rs (+22/-6), app/src-tauri/src/menu.rs (+3/-0),
  app/src-tauri/src/reset.rs (+174/-0, new), 271 insertions and 19 deletions in all. crates/ and ui/ are untouched, so the 56 s
  `ulpf demo --auto` pass was NOT required for this merge and was not run; the gate, isolation and
  the app checks all were.
  GATE GREEN at 1ce7652, 08:43:54-08:44:29 (35 s): `cargo test --workspace` 125 passed 0 failed,
  `cargo clippy --workspace --all-targets -- -D warnings` rc 0, `cargo build --release` Finished
  rc 0 (up to date in 0.17 s -- app/ is its own workspace, so no engine code was rebuilt),
  `target/release/ulpf` 12,114,696 B, `ulpf check --pending pending` 15 parsers 2 mappings
  0 problems, `scripts/demo.sh --check` 39 ok lines rc 0, no fetchable external reference in
  `ui/dist`.
  ISOLATION, run mode, 08:44:49: `ULPF_BIN=./target/release/ulpf scripts/isolation.sh run
  samples/cisco_asa.log` -> counter block printed, "(no network socket observed in any sample)",
  1 live sample, 0 distinct sockets, ISOLATION PASS, rc 0.
  APP CHECKS, 08:44:11-08:44:18 (7 s), in app/src-tauri: `app/scripts/sidecar.sh` rc 0 and it
  printed `(profile dist)` -- it took the shipped binary, not the release fallback, and refused no
  generated parser; `cargo test --lib` 12 passed 0 failed; `cargo clippy --all-targets -- -D
  warnings` rc 0. `target/dist/ulpf` is unmoved at 9,035,832 B, sha256
  55b52b87931faf63b79e2af1cfe298686f79b72d07c0382dc50105ec85ea951c -- the same sha as the sidecar
  inside `/Applications/ULPF.app`, which is this lane's own bundle at be8748b, installed by the
  lead at 08:28 and being driven by hand right now. Nothing in this merge touched that bundle, that
  app or `~/Library/Application Support/dev.ulpf.desktop`, and no `ulpf` process was signalled.
  Lane R's independent verifier is not part of this record: the lead stopped it at 08:37 because its
  test instances opened windows on the owner's screen mid-demo. The three checks above, the
  builder's private-HOME drive and the lead's hands-on test are what the item stands on.
- 08:11 IST: the final sequence, second half, on frozen main at 4e0d71d. The engine DID change
  since the first half's head 217d0df (`git diff --stat 217d0df..4e0d71d -- crates ui Cargo.toml
  Cargo.lock`: 20 files, +242/-50 -- pivot.rs, tail.rs, server.rs, demo.rs, cli.rs, two test files,
  ui/src and ui/dist), so isolation run mode and one demo pass were re-run here and are quoted from
  this head; cold start, isolation serve and docker, and the two 56 s demo passes are quoted from
  the first half at 217d0df and are named as such. The three commits after the merge gate at 06ab4fb
  are records only -- 24c4ac5 (PROGRESS, DECISIONS D95-D99), a37af63 (one sentence in app/README.md),
  4e0d71d (PROGRESS, CLAUDE.md) -- so no engine code moved after the gated tree.
  GATE GREEN at 4e0d71d, 08:07:46-08:08:18 (32 s): `cargo test --workspace` 125 passed 0 failed,
  `cargo clippy --workspace --all-targets -- -D warnings` rc 0, `cargo build --release` Finished
  rc 0, `ulpf check --pending pending` 15 parsers 2 mappings 0 problems, `scripts/demo.sh --check`
  39 ok lines rc 0, no fetchable external reference in `ui/dist`.
  BINARIES. `cargo build --profile dist -p ulpf` 08:06:22-08:07:25 (1m 03s, fat LTO):
  `target/dist/ulpf` 9,035,832 B, sha256
  55b52b87931faf63b79e2af1cfe298686f79b72d07c0382dc50105ec85ea951c. `cargo build --release`
  finished up to date in 0.17 s: `target/release/ulpf` 12,114,696 B (the merge gate's figure), sha256
  6995b5c73f1e3cd9a4a633ef9feaa9fb9d5ccb09010908b46e7cbf99bcc55119.
  BUNDLE. `app/scripts/sidecar.sh` printed `(profile dist)` -- it took the shipped binary, not the
  release fallback -- and the sha is the same in all three places: `target/dist/ulpf`,
  `app/src-tauri/binaries/ulpf-aarch64-apple-darwin` and the `ulpf` inside the bundle
  (`app/src-tauri/target/release/bundle/macos/ULPF.app/Contents/MacOS/ulpf`, the only file `find`
  returns) are each 55b52b87931faf63b79e2af1cfe298686f79b72d07c0382dc50105ec85ea951c, 9,035,832 B.
  `pnpm install --frozen-lockfile` "Already up to date", `pnpm tauri build` 08:07:42-08:08:28 (46 s)
  rc 0, two bundles: `app/src-tauri/target/release/bundle/macos/ULPF.app` and
  `app/src-tauri/target/release/bundle/dmg/ULPF_0.1.0_aarch64.dmg` (8,134,338 B).
  ISOLATION, three modes. run, re-run here at 4e0d71d 08:08:48-08:08:49: `ULPF_BIN=./target/release/ulpf
  scripts/isolation.sh run samples/cisco_asa.log` -> counter block printed, "(no network socket
  observed in any sample)", "sampler lsof, 1 live samples every 0.5 s, 0 distinct socket(s)",
  ISOLATION PASS, rc 0. serve and docker are the first half's, at 217d0df: serve (20 s window on
  7878) PASS with three sockets, all loopback -- the listener and the two ends of the one curl
  client -- over 58 samples; docker `--network none` PASS, the container having no interface but lo
  and completing the run anyway. Both stand: `scripts/isolation.sh` and `Dockerfile` are unchanged
  between the two heads.
  COLD START (first half, 217d0df, run 2 of 2 -- the one to quote): `eval/run.sh eval/tools/ulpf.toml
  cold_start` from a fresh clone, 9 commands, every one exit 0, 50.2 s wall, COLD START: PASS. The
  README fence it executed hashes to
  c67e5740b32692b65c3636853e35d92d1db647f50634d989a485f3be278f1a92, byte for byte the fence in
  README.md. Its `ulpf run samples/*.log` framed 309 stored 309 detected 307 no_parser 2 parsed 305
  normalized 309 emitted 309; verify "309 records, 0 corrupt"; replay "unchanged 309 changed 0".
  DEMO. Here at 4e0d71d, one `./target/release/ulpf demo --auto` pass, 08:08:52-08:09:49 = 57 s,
  exit 0 on 7878/5514 (both confirmed free before): proposal mikrotik after 0.6 s; approve
  `{"name":"mikrotik_inferred","now_detected":{"detected":250,"tested":250},"parsers_loaded":16,...,"replaced_version":null}`;
  replay `{"started":true,"total":1089,"version":2}` with the mid-demo verify "1089 records, 0
  corrupt"; the drift update proposal for mikrotik_inferred after 6.0 s; "attested 2739 records"
  then "attestation: 2 of 2 checkpoints agree (2739 records attested)"; after the deliberate
  one-byte tamper "verified 2739 records, 1 corrupt" / "corrupt: raw id 0" / "chain broken at id 0
  (digest)" with exit 1 as the point; "done: stopped and reset (demo removed)". Afterwards no
  `ulpf serve demo` process, no `demo/` directory, `git status --short` empty. The first half's two
  passes at 217d0df were 56 s each with the same numbers (proposal 0.6 s, drift 5.9 s), so three
  passes across the two heads agree.
  THE APP, INSTALLED AND RUNNING FOR THE MENTORS. `/Applications/ULPF.app` is this bundle: quit,
  swap and relaunch took one second, 08:10:04-08:10:05, and the engine answered immediately.
  `shasum -a 256 /Applications/ULPF.app/Contents/MacOS/ulpf` is 55b52b87..., equal to
  `target/dist/ulpf`, and `ps` names that path as the running `ulpf serve` -- the review instance is
  the new build. `GET /api/status`: its own free localhost port (not 7878, so the terminal demo and
  the app never collide), threads 4, pivot_index true, schema ocsf 1.3.0, queue capacity 64,
  infer_threshold 64.
  ONE THING WENT WRONG, AND IT WAS NOT THIS SESSION'S DOING. Immediately after the swap the owner's
  data directory was intact and was recorded twice, at 08:10:12 and 08:10:28: 17 parsers (the 15
  shipped plus the owner's approved `hpc_inferred` and `mikrotik_inferred`), `/api/integrity`
  records 866,980 with head 1dea952e... and store id a7f3f36d..., `out.jsonl` 504,822,718 B,
  `/api/pending` 0, and the run before the swap had framed/stored/emitted 433,490 with detected
  325,374, no_parser 108,116, parsed 36,213. Between 08:10:28 and 08:10:45 something outside this
  session deleted `~/Library/Application Support/dev.ulpf.desktop` -- the app then recreated it from
  the bundle and restarted its engine at 08:10:45 onto an empty store with the 15 shipped parsers.
  Nothing in this session touched that directory before that point (no move, no delete; the
  instruction to move it aside was deliberately not followed), the app itself has no code path that
  removes it, and the data is not in `~/.Trash` and not anywhere on disk -- a `find` for a 500 MB
  `out.jsonl` newer than 07:00 returns nothing. WHAT IS GONE IS THE STORE, NOT THE DATA -- this
  sentence is the correction of a wrong one (see the fix round at the end of this entry). The events
  themselves were re-creatable from disk the whole time and were in fact re-created within minutes
  (below): the source sits in four places, `~/Downloads/HPC/HPC-1.log` and
  `~/Desktop/demo-logs/HPC-full.log`, both 33,553,503 B = 433,490 events, plus the two small
  extracts `hpc-test.log` and `hpc-test-2.log`; 2 x 433,490 = 866,980 is exactly the count that was
  lost. What no re-ingest can bring back is that store's identity and chain -- store id a7f3f36d...
  with head 1dea952e... over 866,980 records under one genesis, and the two approved parser files --
  because a new store gets a new random genesis and issues new ids. THE ONE UNVERIFIED CLAIM IN THIS
  ENTRY REMAINS: who deleted the directory is not known. The one candidate on this machine -- the
  other lane that runs a bundled ULPF.app against a redirected HOME under `/tmp/laneR` -- is ruled
  out: its worktree was created at 08:14:23 and `/tmp/laneR` at 08:14:40, both after the deletion.
  SEEDING THE EMPTY INSTANCE (08:13, this session). The guard that said "feed nothing" existed to
  protect that data, and the data was already gone, so the empty instance was seeded the way step 3
  describes rather than left blank in front of the mentors: `samples/*.log` copied into the watch
  directory one a second (08:13:21-08:13:36), then `heldout/mikrotik.log` (08:13:36);
  `heldout/edgerouter.log` was NOT fed, it needs UDP. Measured at 08:13:44: framed 559 stored 559
  detected 307 no_parser 252 parsed 305 normalized 559 emitted 559, `/api/integrity` records 559
  head e2979fb0..., 15 parsers, one proposal (`mikrotik`) and it not approved. The window was
  brought to the front and the app left running.
  WHAT THE REVIEWERS SEE NOW -- measured at 08:21:55 by the fix round, and it is a live instance, so
  these counts move whenever anything is fed to it. After this session finished, someone outside it
  put the corpus back: `HPC.log` into the watch directory at 08:15:48 and `HPC-1.log` at 08:16:37
  (both 33,553,503 B, 433,490 events each), and approved the hpc proposal at 08:16:23
  (`pending/approved/hpc-1788662783824868000.json`, `parsers/hpc_inferred.toml` written 08:16:23).
  So the instance now holds `/api/integrity` records 867,539 = 559 + 2 x 433,490, head 7f89822d...,
  store id c93f12ac..., genesis 66086db6..., running false (ingest idle: two reads 30 s apart in
  the fix round, 08:21:25 and 08:21:55, gave the same count); framed/stored/normalized/emitted 867,539, detected 59,034, no_parser 808,505, parsed
  31,138, parse_failed pattern_no_match 27,895 and invalid_json 1, files 18, elapsed 669 s; 16
  parsers, the sixteenth `hpc_inferred` (origin approved, priority -1, 58,727 detected); inference
  runs 5, proposals written 3 replaced 2, approved 1; drift tripped 1 with 401,746 lines routed and
  2 update proposals. `/api/pending` carries TWO: `mikrotik` (source mikrotik.log, 250 lines, 14
  templates, version 1) and `hpc_1` (an update to hpc_inferred, version 2, source HPC-1.log, 4096
  lines, 9 templates, 2 unmatched). NEITHER IS APPROVED: approving one is still the reviewers' step.
  The app was running from `/Applications/ULPF.app` on 127.0.0.1:52134, pids 39323 and 39329, and
  the fix round did not touch it (read-only GETs only).
  AND THEN IT HAPPENED AGAIN, SO TREAT THE NUMBERS ABOVE AS A SNAPSHOT, NOT A STATE. At 08:25:53 the
  installed app restarted a second time onto a fresh empty data directory: new pids 45563/45569, new
  port 127.0.0.1:56552, new store id da68516f... with genesis bdcbcd73... and records 0, `out.jsonl`
  empty, `server.url` rewritten. Nothing in this session did it (between 08:21:55 and 08:25:53 this
  session ran only git, cargo and `ulpf demo --check`), and the same thing happened at 08:10:45.
  Twice in fifteen minutes, so expect it again: something on this machine is resetting
  `~/Library/Application Support/dev.ulpf.desktop`, and it is not the app's own code and not the
  other lane (which is sandboxed under `/tmp/laneR`). Practical consequence for whoever demos: read
  the live port from `~/Library/Application Support/dev.ulpf.desktop/server.url` rather than from
  this entry, and if the window is empty, `cp samples/*.log heldout/mikrotik.log` into the `watch`
  directory that `/api/status` names -- the counters move immediately and the mikrotik proposal
  lands in about half a second. The terminal demo (`ulpf demo`) does not depend on the app at all.
  FIX ROUND, 08:16-08:26, PROGRESS.md only. The verifier's fifteen findings: thirteen pass (repo
  state, the four binary shas, the app running as this build, /api/status, the unapproved mikrotik
  proposal, isolation, check, demo --check, and every figure in this entry against the logs under
  /tmp/final-b and against final-a's quoted numbers), two record errors, both corrected above --
  the recoverability sentence, and the reviewers-see-now paragraph which was accurate as a
  measurement and stale as a present tense. Nothing else in the tree changed. Re-run first-hand
  here at 08:23 to make sure the correction did not rest on the verifier's word alone: `ulpf check
  --pending pending` rc 0 "15 parsers, 2 mappings loaded; 0 problems", `ulpf demo --check` rc 0
  "demo --check: no drift", and `ULPF_BIN=./target/release/ulpf scripts/isolation.sh run
  samples/cisco_asa.log` ISOLATION PASS "(no network socket observed in any sample)".
- 08:04 IST: lane FINAL5 merged as 06ab4fb -- five verified lanes (U, U2, A, PV, MT) integrated on
  branch `integration-final` at 6c56572 over main at 4804f30 and merged here in one `--no-ff`
  commit. Clean merge, no conflicts, no hand resolution; PROGRESS.md and docs/DECISIONS.md carry no
  change on the branch, so neither could conflict. 29 files, +1103/-97 (crates/ulpf pivot.rs,
  server.rs, tail.rs and two test files; ui/src plus ui/dist; app/src-tauri including the new
  download.rs; docs/api.md, design.md, screens/README.md and the new docs/manual-test.md; README.md
  and app/README.md). D95-D99 written from the five lanes. GATE GREEN on the merged tree:
  `cargo test --workspace` 125 passed 0 failed (124 on main plus lane PV's deterministic pivot
  case), `cargo clippy --workspace --all-targets -- -D warnings` rc 0, `cargo build --release`
  Finished rc 0 with `target/release/ulpf` at 12,114,696 bytes, `ulpf check --pending pending` 15
  parsers 2 mappings 0 problems, `scripts/demo.sh --check` 39 ok lines rc 0, no fetchable external
  reference in `ui/dist`, and `ULPF_BIN=./target/release/ulpf scripts/isolation.sh run
  samples/cisco_asa.log` ISOLATION PASS (no network socket observed in any sample). `crates/` and
  `ui/` are touched, so the demo pass ran: `./target/release/ulpf demo --auto` twice, exit 0 both
  times on 7878/5514, each pass showing the mikrotik proposal (0.6 s), approve giving
  `parsers_loaded` 16 with 250 of 250 re-detected at `demo/parsers/mikrotik_inferred.toml`, the
  drift update proposal for mikrotik_inferred (6.2 s and 6.0 s), attestation 2 of 2 checkpoints
  agreeing over 2,739 records, `chain broken at id 0 (digest)` with exit 1 as the point, and
  `done: stopped and reset (demo removed)`. `app/` is touched, so the app checks ran after
  `app/scripts/sidecar.sh` (sidecar at 9,035,672 bytes, profile dist): in app/src-tauri
  `cargo test --lib` 11 passed 0 failed and `cargo clippy --all-targets -- -D warnings` rc 0.
  The mentors' live review instance (/Applications/ULPF.app, pid 29713, its own store under
  ~/Library/Application Support/dev.ulpf.desktop) was not touched at any point.
- 07:39 IST: the records fix round on main (the verifier's two record findings; no lane branch --
  PROGRESS.md and the records commit's message only). GATE GREEN on the final tree: `cargo test
  --workspace` 124 passed 0 failed over 38 targets, `cargo clippy --workspace --all-targets --
  -D warnings` rc 0, `cargo build --release` Finished rc 0 with `target/release/ulpf` at
  12,097,752 bytes, `ulpf check --pending pending` 15 parsers 2 mappings 0 problems,
  `scripts/demo.sh --check` 39 ok lines rc 0 (re-run because PROGRESS.md is that check's own
  input), no fetchable external reference in `ui/dist`, and `ULPF_BIN=./target/release/ulpf
  scripts/isolation.sh run samples/cisco_asa.log` ISOLATION PASS. `crates/`, `ui/` and `app/` are
  all untouched by this round, so neither `demo --auto` nor the app checks were required; the app
  checks stand from the 07:19 entry and the verifier reproduced them at 07:25 (9 passed, clippy
  rc 0, the sidecar at 9,035,672 bytes). What changed: the records commit's message now says
  07:19 where it said 07:25, so it agrees with the entry it writes (message-only amend, 114a1a3
  -> 1e3c3a4, identical tree, never pushed, nothing citing it -- the DoD item and Verified state
  cite 7dc8b9b); CLAUDE.md:242 still reads `(D1-D91)` and is named in In flight as the owner's
  own one-word edit rather than applied here; and the three gate numbers in the 07:19 entry were
  re-measured and corrected there.
- 07:19 IST: lane 7D merged as 7dc8b9b (`lane-7b-app`; Opus builder, Opus verifier, verdict fix
  with four findings all closed; twelve commits 25388fd..02b4bef including its merge of main
  5928e35; 15 files, +587/-55; D92, D93, D94 already written on the branch and not rewritten
  here). Clean merge, no conflicts. GATE GREEN on the merged tree: `cargo test --workspace` 124
  passed 0 failed, `cargo clippy --workspace --all-targets -- -D warnings` rc 0, `cargo build
  --release` Finished with `target/release/ulpf` at 12,095,064 bytes, `ulpf check --pending
  pending` 15 parsers 2 mappings 0 problems, `scripts/demo.sh --check` 39 ok lines rc 0, no
  fetchable external reference in `ui/dist`, and `ULPF_BIN=./target/release/ulpf
  scripts/isolation.sh run samples/cisco_asa.log` ISOLATION PASS. `app/` is touched, so the
  app checks ran after
  `app/scripts/sidecar.sh` staged `ulpf-aarch64-apple-darwin` at 9,035,672 bytes and printed
  `profile dist`: in `app/src-tauri`, `cargo test --lib` 9 passed 0 failed and `cargo clippy
  --all-targets -- -D warnings` 0 warnings. `crates/` and `ui/` are untouched by this branch
  (`git diff` over the merge names only `.github/workflows/app.yml`, `app/`, `docs/DECISIONS.md`
  and `docs/screens/`), so the full `ulpf demo --auto` pass was not required by the gate and was
  not run; the last full pass stands in the entries below. docs/DECISIONS.md now holds 93
  headings: D1-D94 with D67 twice (the lane D amendment) and D76 and D83 reserved but never
  written, both pre-existing and left unrenumbered. Three gate numbers were re-measured at the
  07:27-07:39 fix round and corrected here. The suite is 124, not 123: `cargo test --workspace` prints
  38 `test result:` lines and the 123 dropped the last target printed,
  `crates/ulpf-time/tests/corpus.rs` (one test); nothing was added, the count was short. The
  release binary's size is not byte-reproducible on this host -- the same source relinked at
  07:27 and 07:34 gave 12,115,128 and 12,097,752 bytes -- so 12,095,064 is the 07:19 measurement,
  not an invariant to diff a later build against; `cargo build --release` Finished and rc 0 are
  the checkable parts. And the `ui/dist` grep does find sixteen `https://` strings (fourteen
  `svelte.dev/e/<code>` error codes Svelte compiles into its runtime, two XHTML namespace
  constants); none is fetchable -- no `src=`, `href=`, `fetch(` or `import ... from` names an
  external host, which is the sense every earlier entry's "no external reference" carries.
- 07:10 IST: lane DOCS merged as 8c90b0b (Sonnet builder and verifier, verdict pass; two commits
  896caf0, 42b19cd; five files, 12 lines): the adversarial review's seven documentation findings
  closed to the tree (api.md: GET /api/pending answers an empty list with inference disabled, the
  `[entities]` example is `user.name`; parser-format's `pattern` row back in its table; CLAUDE.md:
  the demo's flags, D1-D91, the v4 record, `FILE.pivot` only with the index on, a docs/coverage.md
  line; D67's fourteen headings; L4's fifteen samples). Docs only, so the gate for this merge is
  `demo --check` 39 ok no drift, `check` 15 parsers 0 problems, the D-numbering unchanged; the
  suite runs on the merged tree at the next lane's gate.
- 07:04 IST: the adversarial review's runner findings closed on main (lead; demo.rs and one clap
  attribute in cli.rs). The review (five Opus finders, three Sonnet skeptics per finding, 80 agents,
  06:04-06:55) confirmed 19 findings and refuted 6; the demo-breaking one was the lead's own
  06:59 port guard: `refuse_busy_ports` sat inside `play`, and `main` removed `<dir>` after any
  error, so `ulpf demo` typed while a hand-started server (PROGRESS step 1) held the port deleted
  `demo/` under that server (verify then said "No such file", replay 500). Now the refusal is the
  first act of both the run and the `--reset` paths, before the leftover kill and before any
  removal; `--reset` stops only a server this runner started (`serve.pid`) and refuses a port held
  by anyone else; the server is spawned with `--mappings <repo>/mappings` so `--repo` works away
  from the root (the preflight names `mappings/ocsf.toml`); `--check` compares the commands as
  PROGRESS documents them (default dir and ports) whatever the live flags; reset says `was not
  there` when there was nothing; `--pivot`'s help shows `<on|off>` instead of clap's true/false.
  Repro on scratch ports 7923-7928: a refused run and a refused reset both leave the directory and
  its marker, the server answers 200 afterwards, then reset removes it and a second reset says
  so; `--check --dir /tmp/pf/x --listen 127.0.0.1:7925 ...` no drift, 39 ok; a full `--auto` pass
  through `--repo` from /tmp (06:59:46-07:00:42, exit 0, proposal 0.6 s, drift 6.1 s); the default
  pass 07:01:38-07:02:34 exit 0. Gate: unit tests 3 of 3, clippy clean, release binary 12,095,064
  bytes (06:59), `check` 15 parsers 0 problems, `demo --check` 39 ok, no external reference. The
  suite at load 16 (four lanes building): 122 passed, 1 failed, the pivot paging test
  (`v4_api.rs:557`, "saw 29 of 32"), which alone failed once then passed twice; unrelated to this
  change and named below. Review findings routed elsewhere: the tail counter's `skipped` (lane U),
  the Review confirmation naming `parsers/` regardless of the server's directories and three
  design-doc mismatches (a UI lane after U), seven documentation sentences (lane DOCS), and one
  engine finding that stays post-demo: a tampered `raw.idx` header is reported as a pre-chain
  store and `verify` refuses to run instead of naming the rewrite (crates/ulpf-store/src/store.rs
  `pre_chain`; the segment's magic could tell the two apart).
- 06:18 IST: gate at the 4B fix round (main, records only: two markdown files, no Rust and no
  `ui/`, so the release binary is still the 05:31 one the 05:34 entry drove through `--auto`):
  `cargo test --workspace --release` 123 passed 0 failed rc 0 (the second run; the first hit the
  known `pivot_pages_by_the_cursor_pair_and_reports_its_timings` flake, re-measured and re-recorded
  under Tried and abandoned), clippy rc 0 with 0 warnings, `cargo build --release` Finished in
  0.13 s (up to date), binary 12,094,216 bytes Sep 6 05:31, `ulpf check --pending pending` 15
  parsers, 2 mappings, 0 problems, `ulpf demo --check` 39 ok (rc 0), and `ULPF_BIN=./target/
  release/ulpf scripts/isolation.sh run samples/cisco_asa.log` ISOLATION PASS (no network socket
  observed in any sample, sampler lsof, 0 distinct sockets). No fetchable external reference in
  `ui/dist`: the only external host in it is `svelte.dev`, and every occurrence is inside a
  `console.warn`/`Error()` message body (Svelte's error-code links), with no `src=`, `href=`,
  `url(` or `from"` naming an external host. GATE GREEN. Closed: D89's forward reference, which
  read "the fact 7B's job object (D91) answers" and pointed at the samples/*.log rule after the
  records commit gave D91 that number; it now names the lane instead of a number not yet
  assigned, so no entry was renumbered (90 `## D` headings before and after) and lane 7C is free
  to take D92. Also corrected: the L4B item's "1090 bytes before and after", which is true of the
  Windows-fence commit (5dd5236) but not of the merge - the fence is 1078 bytes at d6d5a77, 1090
  from d0af71d on, the two `/*.log` suffixes being the 12; `eval/run.sh`'s cold_start reads the
  first fence only, so the criterion is untouched either way.
- 06:02 IST: gate at the lane 4B merge (main 87877a4; the merge is two markdown files, so no Rust
  changed and the release binary is the 05:31 one the 05:34 entry verified): 123 tests 0 failed,
  clippy rc 0, `cargo build --release` Finished in 0.17 s (up to date), binary 12,094,216 bytes,
  `ulpf check --pending pending` 15 parsers, 2 mappings, 0 problems, `ulpf demo --check` 39 ok
  (rc 0), no external reference in `ui/dist`, and `ULPF_BIN=./target/release/ulpf
  scripts/isolation.sh run samples/cisco_asa.log` ISOLATION PASS (no network socket observed in
  any sample). GATE GREEN. No `demo --auto` pass at this merge and none required: it touched
  neither `crates/` nor `ui/` nor `app/`, so the binary is the one the 05:34 pass drove end to
  end. The records commit that follows touches PROGRESS.md, docs/DECISIONS.md,
  docs/evaluation.md and one comment in Cargo.toml.
- 05:34 IST: the runner's busy-port refusal (lead, on main): with `python3 -m http.server 7878`
  holding the port, `ulpf demo --auto` exits 1 at once with `port 127.0.0.1:7878 is in use (a
  server from an earlier rehearsal?): stop whatever holds it (...) and run again` and leaves no
  `demo/`; before the change it printed the server's `Address already in use (os error 48)` and
  waited 20 s for `/api/status`, and a stale `ulpf serve` on the port would have been played
  against. Gate at 12359c6 plus the change: 123 tests 0 failed, clippy clean, release binary
  12,094,216 bytes, `check` 15 parsers 2 mappings 0 problems, `demo --check` 39 ok, no external
  reference in `ui/dist`, isolation run PASS, then a full `demo --auto` pass 05:33:20-05:34:16 (56
  s, exit 0: proposal 0.6 s, approve 250/250 parsers_loaded 16, drift update 6.0 s, attestation 2
  of 2 over 2,739 records, tamper named raw id 0 with exit 1, reset clean).
- 05:29 IST: the lead's live look at the UI on the fresh binary, during a `demo --auto` pass
  (05:22:23-05:23:19, 56 s, exit 0) and then on a scratch serve over the fifteen samples plus
  `heldout/mikrotik.log`: Flow at 52 events/s with every station lit, the tray at 1 waiting and
  1 approved; Live's tail with the trust flags and the proposal banner; Review's row, the
  proposal (14 templates, every slot with its naming reason), the approve confirmation and the
  result card (written to the scratch `parsers/`, parsers loaded 16, 250 of 250 re-detected),
  pending empty afterwards; Pivot's 50 source addresses; Integrity's 559 records with genesis
  and head. The footer's amber `frames skipped` / `events skipped` counters rise during the fast
  drops (150 during the demo, 1,400 after replay and drift): the tail's honesty about what it
  did not render, while framed, stored and emitted stay equal (1,059 each). After the pass every
  screen shows `TypeError: Failed to fetch` with `retry 8s` in the footer: the designed
  disconnected state, the server having stopped. The repo's `parsers/` still holds 15 files;
  nothing was written outside the scratch directory.
- 05:27 IST: lane 8's independent review (the lead, in place of the verifier the limit killed):
  `cargo test --workspace` in its worktree at 80a5bfc, 39 result lines, 118 passed, 0 failed
  (main's gate counts 116); the diff read in full: store recovery zeroes the tail beyond the
  recovered end instead of shrinking the file and walks back on digest and chain, `Live.store`
  is `Mutex<Option<RawStore>>` behind `Live::store()` so a request racing shutdown gets an error
  value and not a panic, `close()` drops the store and the pivot connection after the threads
  join, `is_nul` is Windows-only, the version meta's event count follows the file when the
  output was not empty at start; the Windows suite job 33998281457 is green. Verdict: ready for
  the owner's go after the demo; it changes engine.rs and the store, so it never merges tonight.
- 05:11 IST: the 05:07 gate's release build had not finished (its last line was a `Compiling`
  line and the binary kept its 04:24 timestamp), so its `check` and `demo --check` ran on the
  binary from before lanes 2U and P; the gate script now prints `Finished` or `error` and the
  rc. Rebuilt: `cargo build --release` 52.7 s, 12,092,632 bytes (the no-LTO release profile,
  D88; the 2U overlay strings embedded). On that binary: `ulpf check --pending pending` 15
  parsers, 2 mappings, 0 problems; `ulpf demo --check` no drift; `scripts/isolation.sh run
  samples/cisco_asa.log` ISOLATION PASS; `ulpf demo --auto` end to end, exit 0: 15 parsers
  loaded, the mikrotik proposal 0.3 s after the drop, approve `now_detected 250/250,
  parsers_loaded 16`, replay v2 over 1,089 events, verify clean, the drift update proposal 5.9 s
  after the new lines, attestation over 2,739 records, the tamper named raw id 0 with exit 1,
  reset clean, no server left.
- 05:07 IST: gate at the merge of lanes 4, P, 2U and 7 (main 1a7f05e, 6752d02, eb4540e and the
  lane 7 merge): 122 tests 0 failed, clippy rc 0, release binary 9,019,000 bytes
  (Sep 6 04:24), `ulpf check --pending pending` 15 parsers, 2 mappings loaded; 0 problems, `ulpf demo --check`
  39 ok (rc 0), no external reference in `ui/dist`. GATE GREEN.
- 04:27 IST: gate at 97934a9 (the lane D follow-up): 122 tests 0 failed (116 + the four v4
  contract tests + the runner's two), clippy clean, `ulpf check --pending pending` 15 parsers, 2
  mappings, 0 problems, `ulpf demo --check` 39 ok, no external reference in `ui/dist`;
  `scripts/isolation.sh run samples/cisco_asa.log` ISOLATION PASS (no socket observed).
- 04:25 IST: the merged tree (lanes 3, 1, 2P, D, 2T, I on main, release binary 9,019,000
  bytes rebuilt after the runner's tamper moved to byte 100): `ulpf demo --check` no drift
  (39 ok), the runner's unit test green, `ulpf demo --auto` end to end in 56.6 s: 15 parsers
  loaded, the mikrotik proposal 0.6 s after the drop, approve `now_detected 250/250,
  parsers_loaded 16`, replay v2 over 1,089 events (the three new samples add 45 lines to the
  1,044), verify clean, the drift update proposal 5.9 s after the new lines, attestation 2 of 2
  checkpoints over 2,739 records, the tamper named raw id 0 with exit 1, reset clean (`demo/`
  gone, no server left). The first pass at 04:20 had the tamper land in a header byte (see Tried
  and abandoned). App crate: 7 tests green in `app/src-tauri` after clearing a stale tauri build
  output that pointed at a removed worktree.
- 04:02 IST: main at a9c8ac6 (lane 3 merged): gate 116 tests 0 failed, clippy clean, release
  binary 8,858,888 bytes (no Rust in the binary changed), `ulpf check --pending pending` 15
  parsers, 2 mappings, 0 problems, `scripts/demo.sh --check` 18 ok, no external reference in
  `ui/dist`.
- 03:05 IST: main at 14d3b0c, `cargo build --release` up to date (8,777,448 bytes), `ulpf
  check --pending pending` 12 parsers, 2 mappings, 0 problems (plus one uncommitted scratch
  proposal in `pending/` from last night's bench, not loaded by the registry); `cargo test
  --workspace` 114 passed, 0 failed.

### In flight
- 08:46 IST (lane R merged): nothing is in flight. Lane R (`lane-r-reset`, the in-app reset) is on
  main as the merge commit 1ce7652 and its records commit below it; its branch is merged, its
  worktree is finished with, and its independent verifier was stopped by the lead at 08:37 and will
  not return a verdict (why, and what stands in its place, is in the Definition-of-done item and the
  Verified state entry at the top). Main is frozen again at this records commit. Nothing of this
  session's is running: no worker, no build, no engine of ours. What IS running on this machine and
  is deliberately left alone: the owner's demo on `/Applications/ULPF.app` -- this lane's own bundle
  at be8748b, sidecar sha 55b52b87..., installed at 08:28 -- which the lead is driving by hand, plus
  its data directory `~/Library/Application Support/dev.ulpf.desktop`; and `caffeinate`. No `ulpf`
  process was signalled by this merge and the app was never launched by it. Ports 7878 and 5514 are
  free for the demo. Branches pushed and never merged before the demo, unchanged: `lane-5-xml`,
  `lane-6-index`, `lane-3b-cef-leef`, `lane-8-windows` (and 7B, stopped at 05:19, whose items lane 7C
  carries on the right base). This records commit is not pushed. Left for the owner, as before:
  whether tag `v0.1.0-rc3` is moved off fb7bda9 and its draft release republished, and whether
  branch run 34004510572 (02b4bef, docs-only) is read.
- 08:24 IST (fix round): nothing of this session's is in flight, but this machine is not idle --
  another lane is working in the worktree `.claude/worktrees/wf_babea0b7-cc8-1` (born 08:14:23, at
  this same commit 5b27f68): it bundled the app there (`bundle_dmg.sh`, pid 43664, seen running at
  08:21 and finished by 08:24) and is now running that bundle's own ULPF.app (pids 44556/45101,
  started 08:21:43) against a redirected HOME, `/tmp/laneR/home`, on port 7931. Left alone, and
  named here for two reasons: so the line below is not read as "nothing at all is running", and
  because it is the obvious suspect for the deleted data directory and it is not the culprit --
  its worktree and its `/tmp/laneR` were both created after 08:14, three and a half minutes after
  the 08:10:45 deletion, and it never writes outside `/tmp/laneR`. That question stays open.
  `caffeinate` 6054 also still alive and untouched. Ports 7878 and 5514 are free for the demo (7931
  is that lane's, not ours). And the remote moved: `origin/main` was 14d3b0c when the verifier
  checked at 08:19:52 and is `5b27f68` at 08:23:15, so main through the final-half commit has been
  pushed by someone outside this session (`git ls-remote origin main` agrees). This fix round's
  records commit is not pushed.
- 08:11 IST: nothing is in flight. Every lane dispatched this session has returned; no worker and
  no build is running. Main is frozen at the final-sequence commit (the entry at the top of Verified
  state), and the second half of the final sequence -- rebuild, bundle, install, gate, isolation,
  demo -- is done and recorded there. Branches pushed and never merged before the demo, unchanged:
  `lane-5-xml`, `lane-6-index`, `lane-3b-cef-leef`, `lane-8-windows` (and 7B, stopped at 05:19, whose
  items lane 7C carries on the right base). Left for the owner, as before: whether tag `v0.1.0-rc3`
  is moved off fb7bda9 and its draft release republished, and whether branch run 34004510572
  (02b4bef, docs-only) is read.
- 08:04 IST: nothing in flight. Main is frozen at this records commit for the final sequence
  (08:30-09:30). On main: lanes 3, 1, 2P, D, 2T, I, 4, P, 2U, 7, 4B, DOCS, 7C/7D and, last,
  FINAL5 -- lanes U, U2, A, PV and MT merged together as 06ab4fb through one gate (GATE GREEN at
  the top of Verified state). No worker is running; every lane dispatched this session has
  returned and is recorded above. Lane A's independent verifier returned at 08:02 with verdict
  fix on one sentence (app/README.md's Windows payload count, which had traded a stale 12 for an
  unmeasured 15: run 33990295166's artifact really carried 12); its fix round's docs-only commit
  a88a709 is cherry-picked onto main as a37af63 after the freeze commit, with CLAUDE.md's D-range
  brought to D1-D99 in the same pass. The builder's code claims stand on the integration check
  and on this gate, which re-ran the app tests (11 passed) and clippy on the merged tree.
  Branches pushed and never merged before the demo, still listed as such: lane-5-xml,
  lane-6-index, lane-3b-cef-leef, lane-8-windows. 7B stays stopped at 05:19 (its worktree was
  created at 14d3b0c, before lane 7's merge, so its job-object draft sat on the old `lib.rs`); the
  diff is kept in the lead's scratch and 7C carries the same items on the right base.
  Left for the owner, deliberately not applied here: whether tag `v0.1.0-rc3` is moved off fb7bda9
  to a head that carries the last commits and its draft release republished (moving it deletes the
  remote tag and re-fires a ~14 min release job); whether branch run 34004510572 (02b4bef,
  docs-only) is read; and CLAUDE.md:242, which reads `(D1-D91)` and is now eight short after this
  merge landed D95-D99 -- the fix is one word, `D1-D99`, and this round left CLAUDE.md alone rather
  than rewrite it on a relayed instruction.
  Still the lead's: the final sequence and the report. The harness re-run on the dist build is done
  twice over (4B's median 295,928 and its verifier's 320,369, both above the committed 258,411 and
  its 10 percent band); whether a scorecard is committed and the headline re-pinned off 258,411 is
  the final sequence's call, D87 unchanged.

### Tried and abandoned (v4)
- Lane 2P's headline "cut 4-8x": measured only at load 28-36. The controlled pair on a quiet
  machine (two serves over identical copies of one index, five alternating calls, load 4.3) is
  2.6-3.3x (jdoe 93 -> 29 ms, dst_port 443 89 -> 33, src_ip 74 -> 28), and that is the number
  recorded; 4-8x is the loaded end. The "500 ms" in the lane's name had no measurement behind
  it in the repo (docs/screens/README.md's pivot row carries no timing): the measured before was
  93 ms quiet, 239 ms at load 30.
- The demo's tamper at byte 200 of `raw.seg`: once `cef.log` sorted first among the samples, byte
  200 fell inside record 1's header (its receipt time), which the digest and the chain do not
  cover, so `verify` said clean with exit 0 on the merged tree's first `--auto` pass (04:20 IST).
  The tamper moved to byte 100, inside record 0's body whatever the first sample is (the segment
  and record headers end at byte 68). Post-demo question for the owner, recorded not built:
  whether the record header's receipt time belongs under the chain (a store-format change).
- A flake, closed by lane PV in this session's last merge (D96): `crates/ulpf/tests/v4_api.rs`
  `pivot_pages_by_the_cursor_pair_and_reports_its_timings` failed once at load 37 on lane 6's
  worktree ("saw 31 of 32"), then, re-measured on main at 06:12-06:14 IST (load 7.45-8.62), 3 of 11
  runs of the `v4_api` binary alone with the same "saw 31 of 32" while passing 3 of 3 when named by
  itself. The mechanism recorded here was right -- a page took the first `limit*4` entries past the
  cursor in raw-id order and re-sorted them by device time, so an event whose device time disagreed
  with arrival order by more than that window was skipped -- but the reading of it was wrong: this
  was never a race, and "a hand-run gate may need a second run" was the wrong remedy. Lane PV
  reproduced it deterministically on a fully settled hand-written index and on real data, so the
  test was right and the read path was wrong; the cap now only ends the scan and never drops an
  entry. Before: 2 failures in 50 loaded runs, and 20 of 200 events on the deterministic repro.
  After: 50 of 50 for the builder, 30 of 30 at load 19.24 for its verifier, 45 of 45 on the
  integration tree at load 8-12, 200 of 200 on the repro, and 5 of 5 twice for `cargo test -p ulpf
  --test v4_api`. What the load actually decided was which entity the test picked, not whether the
  read path was correct. The ceiling that remains -- an entity above about 10,000 events can still
  hide an event whose device clock is behind by more than the scan's window -- is named in D96 and
  in the code, and needs an index-format change to lift.

### Next action (if this session is cut off here)
The app is running from `/Applications/ULPF.app` for the mentors: it is this tree's bundle (the
engine inside it hashes to `target/dist/ulpf`), it serves on its own free localhost port named in
`~/Library/Application Support/dev.ulpf.desktop/server.url`, and its store, output, parsers and
pending directory are the owner's own -- leave them alone. If it is ever gone, relaunch with
`open /Applications/ULPF.app`. Its port changes on every restart, so take it from
`~/Library/Application Support/dev.ulpf.desktop/server.url`, not from any number written here; that
directory was reset from underneath the app twice this morning (08:10:45 and 08:25:53) by something
outside this session, so if the window is empty, copy `samples/*.log` and `heldout/mikrotik.log`
into the `watch` directory `/api/status` names and it fills in seconds.
The terminal demo is `./target/release/ulpf demo --check` and then `./target/release/ulpf demo`
(ports 7878 and 5514; the app never uses them). `--auto` plays it without waiting for a key.
Main is frozen at the records commit this entry was written in and `git status --short` is empty.
Pushing has changed since the entry above was first written: main through `5b27f68` IS on origin
now (someone outside this session pushed it; a fetch at 08:23:15 moved the ref up from 14d3b0c,
and `git ls-remote origin main` confirms it). The records commit sitting on top of it is not
pushed, and this session pushed nothing at any point.
Branches pushed and never merged before the demo: `lane-5-xml`, `lane-6-index`, `lane-3b-cef-leef`,
`lane-8-windows`; 7B stays stopped at 05:19 and 7C carries its items on the right base. Lane
worktrees, where any remain, are under `.claude/worktrees/`; `git worktree list` names them.

---

## v3 (2026-09-05 night session, autonomous): fixes, UI redesign, desktop app, demo runner

Started 20:12 IST at 9d39679 (107 tests, clippy clean). The owner is away; this file and the
committed captures under `docs/screens/` are what they review from a phone, so both are current
at every commit and main is never half-done. The brief names a 22:00 demo; the demo script below
still says 10:00 and the 04:00 comparison, which is the last thing the owner wrote; both stand.
Skills: `software-design-philosophy` loaded (interface decisions); `example-skills:frontend-design`
loaded by the UI and app workers, not the lead; no `prompting-practices` skill on this machine
(its requirements carried from the brief: clean lead context, structured worker returns, kill
timers); the skills-audit manifest is `~/Documents/dev/skills-audit/MANIFEST.md`; no `aposd` pass
this session. No "last session report section 8" exists in the tree (grep for weak spots and
section 8 found nothing), so Phase A is the brief's own list. Tiers: lead and the three lane
workers on Fable, verifiers on Opus; Haiku banned (D30).

### Definition of done (each item checked only after running it)
- [x] A1. (D68; merged 22:20 IST) Inference uses the names it already has: JSON object keys
      become slot names (reason `json key`), a `#fields` header names delimited columns (reason
      `header`); `[[timestamp]]` follows the slot's name. Re-graded with `ulpf infer FILE
      --pending SCRATCH --decisions` (before = main at 9d39679, after = the merged tree):

      | Zeek file | templates | slots before (suggested / typed only) | slots after (suggested / typed only) | covered |
      |---|---|---|---|---|
      | json/conn.log | 1 | 40 (1 / 39) | 19 (19 / 0) | 5,096 of 5,120 (24 `no_template`) |
      | json/dns.log | 3 | 99 (3 / 96) | 42 (42 / 0) | 3,400 of 3,400 |
      | conn.log (TSV) | 5 | 78 (16 / 62) | 78 (78 / 0) | 5,096 of 5,129 (9 header lines `below_support`, 24 `no_template`) |
      | http.log (TSV) | 40 | 541 (76 / 465) | 541 (540 / 1) | 100 of 1,545 (1,354 `template_cap`), unchanged |

      Names after: `ts uid id_orig_h id_orig_p id_resp_h id_resp_p proto service duration
      orig_bytes resp_bytes conn_state ...` (the device's own). Heldout grades byte-identical
      before and after. 110 tests, clippy clean. Then lane A1b (D72; merged 22:54 IST as
      4d031ec, two commits, two Opus verifications reproducing every number): a headed
      delimited file whose every row fits the header is ONE `kind = "delimiter"` definition:

      | Zeek file | before (pattern path) | after (D72) |
      |---|---|---|
      | http.log | 40 templates, 541 slots, 100 of 1,545 | 1 definition, 30 columns, 1,536 of 1,545 (9 `header`) |
      | conn.log | 5 templates, 78 slots, 5,096 of 5,129 | 1 definition, 22 columns, 5,120 of 5,129 |
      | dns.log | (not graded) | 1 definition, 26 columns, 3,400 of 3,409 |

      Approved and run, http.log parses 1,536 of 1,536 data lines, `parse_failed` 0, `ts` as
      the event time; `class_unknown` on Zeek http fields is mapping work, not done tonight.
      The lead accepted the ten-line `pending.rs` touch both verifiers flagged (D72 says why).
      114 tests, clippy clean, heldout byte-identical.
- [x] A4. The nine stale `worktree-wf_*` branches and worktrees removed 20:18 IST after
      confirming each: eight were ancestors of main (515dc9ab, 9e3d885f, bad47452-1..5,
      fe15bb9f, all `merge-base --is-ancestor` yes, 0 commits ahead); `worktree-wf_d4c9a934-b72-1`
      was 2 ahead (7c76587, 4577440: the Parquet worker's crate and `--parquet` flag, 19 files)
      and is the one re-applied on main as eb2e2c4 ("parquet: --parquet writes a columnar copy",
      18 files, same crate, same flag, the diff being the merge onto the then-current engine).
- [~] A2. Socket soak re-run after the `SO_RCVBUF` negotiation (D62 amendment): run 4, loaded
      machine (load 11-49, host suspended 16 min mid-run), 9,000,000 sent; rcvbuf granted
      8 MiB; UDP 911,692/2,400,000 (62% kernel drops, netstat delta 1,488,318 vs shortfall
      1,488,308), TCP exact, RSS 16.5-714 MB, PARTIAL. Run 5 (22:58 IST, from the quiet-window
      watcher: started at load 3.9, then the lanes loaded the host 54% of the run, peak 17):
      UDP 1,063,160/2,400,000 received, shortfall 1,336,840 against a netstat drop delta of
      1,336,894; file and TCP exact; framed = stored = emitted = verified 7,663,160, chain ok;
      engine 5,950 events/s with the index on beside two listeners; RSS max 930 MB (backlog).
      Run 6 (01:42-01:58 IST, 06 Sep, nothing else of the session running, load mean 4.1 peak
      8.6 from the run itself): UDP 1,067,333/2,400,000, shortfall 1,332,667 against a netstat
      drop delta of 1,332,731; file and TCP exact; 7,667,333 framed = stored = emitted =
      verified, chain ok; engine 7,736 events/s with the index on, 2,980 backpressure blocks;
      RSS max 986 MB. Three runs, one answer: the engine with the entity index on is the
      bottleneck at 26k/s aggregate, the listener blocks by design (D60) and the kernel drops
      UDP at 8k/s. DEMO RULE (firm): feed the demo device over TCP or the file path, or run
      `serve --pivot off` when a UDP device must keep up. No engine change (the brief: a
      warning, not a second fix).
- [x] A3. (23:05-23:25 IST, from the quiet-window watcher: each run started at a one-minute load
      under 4 with no rustc/cargo/ld alive, load sampled every 2 s during the run; `ulpf run
      bench/mixed-5000000.log --output /dev/null --infer-threshold 0 -j N`, fresh store each
      time, the merged binary of 22:54) Honest throughput on the 5M bench, 1,526 MB:

      | -j | events/s | MB/s | wall | load before / peak during |
      |---|---|---|---|---|
      | 1 | 68,330 | 20.9 | 73.2 s | 3.59 / 4.88 |
      | 2 | 121,092 | 37.0 | 41.3 s | 3.93 / 5.13 |
      | 4 | 200,797 | 61.3 | 24.9 s | 3.85 / 10.65 (lane V ingesting beside it) |
      | 7 | 314,691 / 337,471 / 345,153 (median 337,471) | 96-105 | 14.5-15.9 s | 3.4-3.9 / 5.0-9.5 |

      Against the 18:31 loaded run (66,827 / 118,038 / 218,391 / median 250,674) the single
      thread is within 2.3%, the seven-thread median is 35% higher with `--output /dev/null` on a
      quieter host. README's throughput paragraph now carries this table's numbers with the
      thread count and the `-j 1` figure; item 9 below and the demo's step 11 comment say the
      thread count with every number.
- [x] B. (D69-D71; merged 22:47 IST as b85c1c4, five commits 273d2d9..00062be, verified by an
      independent Opus pass: git surface ui/ + docs/design.md + docs/screens/ only, dist exactly
      three files with no runtime fetch, fonts real WOFF2 byte-identical to IBM's release with the
      OFL text, the contrast table reproduced, 44 captures all indexed both ways, the API paths
      called by the new src identical to the old, its own capture of #/live from its own server)
      UI redesigned in place: one token block, IBM Plex Sans + Mono embedded (78,656 bytes), dark
      default with light through tokens, AA everywhere (lowest 4.64:1), keyboard map with `?`
      overlay, one `VList` for every long list and the byte ruler (4 MB record: ruler in 1.3 s,
      24-30 DOM rows), one keyboard-reachable confirmation for approve/reject/replay/verify;
      `docs/design.md`; `ui/capture.mjs` (puppeteer-core over the installed Chrome's CDP, no bundled Chromium) re-takes every capture; 44 PNGs at
      1280 and 2560 under `docs/screens/` with README.md and index.json. Contract gaps the UI
      labels honestly rather than papering over (server unchanged tonight, follow-ups): no
      instantaneous queue depth in the metrics frame (high-water and blocks shown instead);
      `events_per_sec` is a decaying run average, not a windowed rate; `/api/pivot` paging by
      `before_id` is prose-only; `/api/events/{id}.emitted` is null once out of the tail ring.
      Not done: automated UI tests; the merge-mid-state capture; light theme captured on two
      screens only. Rendered in the Tauri webview by C's verifications. Lane P (merged 01:15
      IST as 8ccb8e5, one commit 61a6a4c, judged by a strongest-tier verifier that re-ran each
      fix over CDP and looked at all 17 changed captures): the nine minor findings from the
      Chrome-driven pass fixed in the UI alone (the approve result scrolls into view and takes
      focus, the written-to path wraps, axis ticks are distinct, seen-with bars are the share
      of the events they were computed over, both store sizes formatted, no negative zero,
      the tail note says click or Enter, the sources table fits at 1512 (stack breakpoint
      1650), the field legend clips to two rows with an `All N keys` button); 114 tests, clippy
      clean, dist rebuilt byte-identical, no external reference.
- [x] C. (D73; merged 23:12 IST as ce3826e, twelve commits acd42c7..cdb4d9b, verified by an
      independent Opus pass that launched the .app itself: clean data dir, `server.url` in
      2 s, `/api/status`, a new source in the feed within 1 s, quit leaves no app process and
      removes `server.url`, relaunch on a new port with the same store id, head and record
      count) Desktop app: Tauri 2 shell in `app/` (own workspace; root `cargo metadata` lists
      the seven engine crates only), ulpf as a sidecar on a free localhost port against
      `~/Library/Application Support/dev.ulpf.desktop` (parsers and mappings seeded once),
      splash then navigate to the served UI, Quit kills the child (D59 makes it safe). Each
      its own commit and each verified by the worker on screen: drop and Add files… through
      one `ingest_paths` with a visible notice (`app-drop.png` is a real Finder drag driven
      with the computer-use tools, `app-add-files.png` the native panel); Choose data
      directory… restarts the engine, Open output folder reveals `out.jsonl`, Open in browser;
      the title `ULPF · engine ok · N events · M pending` once a second; tray with Show, Open
      output folder, Open in browser, Quit, closing the window keeps the ingest (emitted 280
      to 291 with the window hidden). The five verifications: (a) launch, (b) drop shows in the
      feed, (c) `heldout/mikrotik.log` proposed and approved inside the app, (d) no orphan after
      tray Quit, plain Quit and the CI-built bundle, (e) relaunch resumes (291 records and
      lines before and after, 313 after a new drop). Captures `docs/screens/app-*.png`,
      indexed. Not done: `engine down (exit N)` never provoked; the tray icon's glyph sits
      under this Mac's notch overlay so only its menu is captured.
- [x] C-CI. (D74; `.github/workflows/app.yml`) macOS and Windows runners build the sidecar and
      the shell and bundle installers; the run on the lane branch's final commit cdb4d9b
      (`worktree-wf_b664b6d7-603-1`; main was not pushed until the end of the session, so CI
      had not yet built the merged tree; the run on main after the final push, 33990295166,
      is green on both and is in the verified state below),
      https://github.com/techlogist1/ulpf/actions/runs/33980779377, green on both (macOS
      6m02s, Windows 9m13s); artifacts `windows-x64-nsis` 5,351,146 B, `windows-x64-msi`
      7,794,850 B, `darwin-aarch64-app` 7,855,749 B, `darwin-aarch64-dmg` 7,606,904 B. First
      push 22:22 IST, both green by 22:34; one red Windows job on the feature commit (E0521 in
      `menu.rs`, an app-handle borrow in the Windows tray branch), fixed in cdb4d9b, green at
      23:02, inside the sixty-minute rule. Windows shims: `#[cfg(windows)]` `FileExt` in the
      store (additions only, one `read_exact_at` over `seek_read`) and a no-op
      `set_recv_buffer`; in `syslog.rs` the unix function now takes the socket instead of a raw
      fd and the caller's warning branch reads `cfg!(windows)` first, behaviour unchanged (D74).
      The Windows build has NOT been
      run on a Windows machine tonight; the five owner checks are in `app/README.md`: launch
      (window shows the live feed, `%APPDATA%\dev.ulpf.desktop\server.url`, `/api/status`
      answers), drop `samples/cisco_asa.log` (notice, events in the feed), drop
      `heldout/mikrotik.log` then Review and Approve, Quit from the tray (also after closing
      the window) leaves no `ulpf.exe`, relaunch keeps `/api/integrity` records and appends.
- [x] D. (D67; `scripts/demo.sh`; two `--auto` passes 21:53 and 21:55 IST; `--check` PASS) Demo
      runner: one command plays the demo script step by step (fresh `demo/`, parsers copy, paced
      known-format drops, one unseen-format drop on cue, what-to-click prompts, clean reset),
      existing subcommands and watch only; written while C was in flight, verified before C landed.
- [x] Final (01:15-01:58 IST, 06 Sep, in this order on the final tree): the release binary rebuilt
      with P's dist (`cargo build --release` up to date, 8,777,448 bytes, P's strings in it); the
      app re-bundled with that sidecar (SHA-256 39d5bec1... equal for `ULPF.app/Contents/MacOS/ulpf`
      and `target/release/ulpf`; `ULPF_0.1.0_aarch64.dmg` 7,971,800 B); `scripts/isolation.sh` PASS
      in all three modes (run over the 5M bench: 30 samples, 0 sockets; serve for 20 s: 58 samples,
      the one 127.0.0.1:7878 listener; docker `--network none` on the rebuilt `ulpf:static`);
      the cold-start criterion PASS (fresh clone of HEAD, README's nine Quick-start commands,
      94.8 s; `eval/results/ulpf-20260905T200729Z-11466/scorecard.md`); one `scripts/demo.sh
      --auto` pass (proposal 0.9 s, update 5.4 s, replay v2 verified 1,044, attestation 2 of 2
      over 2,694, tamper caught at raw id 0, reset clean); the quiet soak (run 6, D62: the same kernel-drop
      result with nothing else on the host, so the demo rule is firm); then this
      file's verified state, the commit and the push.
- Automated UI tests remain absent; tonight's captures are the UI's verification.

### Fan-out 1 (20:20 IST): three lanes, then the lead's measurements
Split: (A1) inference naming, one Fable worker in its own worktree owning `crates/ulpf-infer`
and its example, the pending fixture it re-grades and `docs/slot-vocabulary.md`; (B) the UI, one
Fable worker with `frontend-design` in its own worktree owning `ui/`, `docs/design.md`,
`docs/screens/`; (C) the app, one Fable worker with `frontend-design` for the shell chrome only,
in its own worktree owning `app/` and `.github/workflows/`, allowed `#[cfg(windows)]`-only
shims where the sidecar does not compile on Windows (the unix code stays byte-identical, the
lead diffs it at merge; at merge the store was additions only and `syslog.rs`'s unix
function signature and caller branch had changed with behaviour unchanged, D74). The lead runs A2 (socket soak, backgrounded) and A3 (throughput,
backgrounded, load recorded) in the main tree, merges each lane by running the full suite and
the build itself, and writes D after C. Why not fewer: the three lanes touch disjoint files, share
no state, and each is one to three hours of wall-clock that the others need not wait for; one
worker doing them in sequence would put the UI (the demo's face) after the app. Why not more: the
engine, the store and the server are frozen, so there is no fourth lane; verification of each
lane is a second, independent Opus agent inside the same workflow, not a fourth builder.
Return format (schema-enforced): worktree path, branch, commits (hash + message), files written,
tests run with pass/fail counts and the exact commands, clippy result, decisions made (each with
the alternative ruled out, for DECISIONS), contract gaps (UI: fields a screen needs the API lacks),
uncertainties verified against current documentation with the source, what is not done and why,
measurements with their commands. Nothing else: no logs, no transcripts. No worker command runs
longer than about four minutes (backgrounded and polled past that).

### Verified state (v3, rolling; every line was run, not read)
- 02:00 IST, 06 Sep, the final tree (the commit after 5e7dcd4): `cargo test --workspace`
  114 passed, 0 failed, 2 ignored and clippy `-D warnings` clean at the P merge (8ccb8e5;
  nothing under `crates/` changed after it); release binary 8,777,448 bytes with P's dist;
  `ULPF.app` bundled 01:16 with that binary as its sidecar (SHA-256 equal); `scripts/
  isolation.sh` PASS in run (0 sockets, 30 samples), serve (one 127.0.0.1:7878 listener, 58
  samples) and docker (`--network none`, image rebuilt 01:19) modes; cold start PASS (fresh
  clone, nine Quick-start commands, 94.8 s, scorecard committed); `scripts/demo.sh --auto`
  PASS (proposal 0.9 s, update 5.4 s, replay 1,044 verified, attestation 2/2 over 2,694,
  tamper at raw id 0 caught, reset clean); `scripts/demo.sh --check` 18 ok under zsh and
  under bash; the quiet soak run 6 as in A2 and D62; V2's nine computer-use captures and
  V's fifteen Chrome captures under `docs/screens/` with all 76 PNGs indexed both ways;
  `git worktree list` main only. Pushed 02:02 IST as a78f1e3 (main == origin/main); the push's
  CI run on main, https://github.com/techlogist1/ulpf/actions/runs/33990295166, green on both
  runners (macOS 6m55s, Windows 12m06s): artifacts `windows-x64-nsis` 5,447,145 B,
  `windows-x64-msi` 7,897,250 B, `darwin-aarch64-app` 7,961,464 B, `darwin-aarch64-dmg`
  7,711,297 B, built from the merged tree.
- 01:15 IST (06 Sep): P merged (8ccb8e5): `cargo test --workspace` 114 passed, 0 failed, 2
  ignored; clippy clean; release build 8,777,448 bytes embedding P's dist (the new strings are
  in the binary); the lead's grep of `ui/dist` finds no external reference.
- 23:12 IST: C merged (ce3826e): `cargo test --workspace` 114 passed, 0 failed, 2 ignored;
  clippy clean; release build 8,777,448 bytes (unchanged by the cfg shims, as intended);
  `cargo metadata --no-deps` at the root lists the seven engine crates only, so `app/` stays
  outside the engine build.
- 22:54 IST: A1b merged (4d031ec): `cargo test --workspace` 114 passed, 0 failed, 2 ignored;
  clippy clean; release build 8,777,448 bytes; the lead's own `ulpf infer corpus/generated/
  zeek/http.log` on it: 1,545 lines, 1,536 used, 1 template, 9 unmatched `{"header": 9}`,
  `kind = "delimiter"`, 30 fields from the header.
- 22:47 IST: B merged (b85c1c4): `cargo test --workspace` 110 passed, 0 failed, 2 ignored;
  clippy `-D warnings` clean (0 warnings); `cargo build --release` embeds the new dist (binary
  8,759,400 bytes); the lead's own grep of `ui/dist` for `https?://`, `@import` and `<link`
  finds only Svelte's error-message URLs inside thrown strings, the XHTML namespace constant,
  the same-origin stylesheet link and the inline `data:,` icon.
- 22:20 IST: A1 merged (42e5a1a): `cargo test --workspace` 110 passed, 0 failed, 2 ignored;
  `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo build --release` 1m20s;
  the merged binary's `ulpf infer corpus/generated/zeek/json/conn.log` gives 1 template, 19
  slots, 19 suggested, names `ts uid id_orig_h ...` (the lead's own re-run, scratch pending dir).
- 20:18 IST: A4 done as above; `git worktree list` shows main only; `cargo build --release` at
  9d39679 up to date; `ulpf check --pending pending` 12 parsers, 2 mappings, 0 problems.

### Fan-out 2 (21:00 IST): the same three lanes, resumed after the account limit
The account's session limit cut all three builders at about 20:35 IST, nine to eleven minutes
in (each had 35-60 tool calls done). Their worktrees kept the partial work uncommitted: A1 had
edited cluster.rs, lib.rs, token.rs and docs/slot-vocabulary.md; B had tokens, fonts, a
virtual list and the traceback under way with dist rebuilt; C had an `app/` scaffold and a
built sidecar. The owner switched the session to low-priority mode and asked for the agents
to be relaunched at the highest quality. Relaunched 21:00 IST with the same split, the same
return format and the same verifier stage, each builder told to resume in its existing
worktree: read every changed file first, keep what is coherent, revert what is not, say
which. New kill timers: A1 22:25, C shell 22:25 (features and CI to about 23:40), B 23:55.

### Fan-out 3 (22:12 IST): the limit is lifted; the same lanes, resumed a second time
The limit cut B and C again at about 22:00 IST, after 54 and 59 tool uses (about an hour in).
A1's builder landed on its branch (b637781, d3274dd; 110 tests, clippy clean; its measurements
are in the run record) and its verifier died before running anything. Each worktree kept more
work than after cut one: B has commit 273d2d9 (tokens, IBM Plex Sans and Mono as data URIs,
the traceback on a virtual byte ruler) plus six screens edited but uncommitted; C has commit
acd42c7 (the shell launches the sidecar on a free port and shows the served UI) plus the
Windows cfg shims, the CI workflow, ingest/menu/title sources and captures uncommitted. The
owner moved to a larger plan and asked for the agents to be relaunched with no quality cut.
Relaunched 22:12 IST: A1 resumed from its run record so the cached build report goes straight
to a fresh Opus verifier; B and C resumed in their worktrees with the resume notice rewritten
to name exactly what each commit and dirty file holds, told to judge the uncommitted work
first, commit each coherent piece, then finish. Same split, same return format, same reason
fewer workers would not do (the three lanes touch disjoint trees: ulpf-infer, ui/, app/).
New kill timers: B 01:15 IST (fix round 01:50); C CI pushed by 23:30, features and the five
verifications by 00:45, report by 01:15.
A fourth lane at 22:14 IST, A1b: A1's builder diagnosed the http.log explosion (40 templates at
`template_cap`) as structural and named the fix (a header-carrying delimited file becomes one
delimiter-strategy proposal, which `Strategy` already expresses) but did not start it inside
its timer. A1b builds that in a worktree branched from A1's d3274dd (`.claude/worktrees/a1b`,
branch `worktree-a1b`), forty minutes of building, hard stop 23:10, Opus verifier, one fix
round; same return format. It is a separate lane because it edits `lib.rs infer()` while the
A1 verifier reads A1's tree, and it merges only after A1 does.

### Crash and restart (00:57 IST, 2026-09-06)
The Mac crashed and rebooted at about 00:57 IST with lanes P, V2 and K in flight (host up
4 minutes at 01:01, load 42). Main was clean at 4f8f6ea. What survived: lane P's worktree
holds one clean commit, 61a6a4c ("the nine Chrome-driven polish findings fixed in the UI
alone", 29 files: Review/Pivot/Traceback/Live/api.js, app.css, the rebuilt dist, capture.mjs,
design.md, the screens index, 17 re-captured PNGs), unverified; lane V2's worktree holds only the launch capture (its own
message: the display slept and the session locked two minutes in); lane K returned nothing.
Wake lock for the rest of the session, started 01:04 IST: `caffeinate -d -i -m -s -u -t 28800`
(display, idle, disk, system and a user-activity assertion for eight hours; `pmset -g
assertions` shows all four); stop it with `pkill -x caffeinate`. Order on the owner's go:
verify 61a6a4c with a strongest-tier verifier against docs/design.md and the nine findings,
merge through the full gate; then V2 and K rerun in parallel on the merged build; then the
final sequence in order (rebuild, re-bundle with the sidecar SHA checked, isolation in run,
serve and docker modes, cold start, demo runner, quiet soak, verified state, commit, push).

### In flight
- Nothing. Every lane is merged (A1, A1b, B, C, V, P, V2, K's fixes) and every worktree is
  removed; `git worktree list` shows main only. The wake lock (`caffeinate`, pid in
  `pgrep -x caffeinate`) is the only process of the session left running; stop it with
  `pkill -x caffeinate`.
- Deferred, recorded where they belong: the three server-side UI findings (item B's contract
  gaps and the P bullet above); `class_unknown` on Zeek http fields (mapping work, D72); the
  Windows installers never launched on a Windows machine (D74, the five owner checks in
  `app/README.md`); the stale data directory `~/Library/Application Support/dev.ulpf.app` left
  by the first bundle identifier on this Mac (dead data, nothing reads it; delete by hand).

### Tried and abandoned (v3)
- (none yet)

### Next action (if this session is cut off here)
Nothing is half done. For the demo: `scripts/demo.sh` from a built tree (`cargo build --release`;
ports 7878, 5514 and 5515 free), or the desktop app from `app/src-tauri/target/release/bundle/
macos/ULPF.app` (rebuild with `app/scripts/sidecar.sh && cd app && pnpm tauri build`). For the
04:00 comparison: `docs/evaluation.md`. If a number here is questioned, the command that
produced it is beside it; re-run it.

---

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
      265,752 / 212,427 / 250,674 (median 250,674, 76.5 MB/s); re-measured on a quiet host
      23:05-23:25 (v3 A3): -j 1 68,330, -j 2 121,092, -j 4 200,797, -j 7 median 337,471, all
      with `--output /dev/null`, 7 threads being the default here. Backpressure engaged at
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
  258,398 events/s at 7 worker threads (median 258,411, about 79 MB/s, 19.0-19.4 s per 5M
  events; -j 1 is 68k on this file, v3 A3), correctness
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
