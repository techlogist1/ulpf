# Evaluation harness

`eval/run.sh` scores any log-processing tool against ten fixed criteria, using
nothing but the tool's own command-line invocation. It knows nothing about
ULPF specifically -- ULPF is scored by pointing it at `eval/tools/ulpf.toml`;
a second tool is scored by writing a second `.toml` and pointing it there
instead. Every number in the scorecard is produced by a command printed next
to it, and every raw output is kept under `eval/results/<tool>-<timestamp>/`,
so a stranger can re-run any single line of the report.

## How to add a tool

Write `eval/tools/<name>.toml` (see `eval/tools/ulpf.toml` for the
reference). Placeholders are substituted literally, not through a shell, so
**every placeholder must sit inside double quotes in the template string**
(`"{input}"`, not `{input}`) or a path containing a space breaks.

```toml
name = "yourtool"

[build]
cmd = "..."                 # {target_dir}: a scratch build dir, never the repo's own target/
binary = "{target_dir}/..."

[templates]
run      = "\"{bin}\" ... \"{input}\" ... \"{output}\" ... \"{store}\" ... {threads}"
verify   = "..."            # optional: recompute/verify the raw store's integrity
raw_of   = "..."            # optional: print event {id}'s exact raw bytes to stdout
check    = "..."            # optional: validate config without a run

[container]                 # optional; omit both keys if the tool has no image
build = "docker build -t {image} -f \"{dockerfile}\" \"{context}\""
image = "yourtool:latest"
run   = "docker run --rm --network none -v \"{input_dir}:/data/input:ro\" -v \"{output_dir}:/data/out\" {image} ..."

[correctness]
key_map   = "eval/tools/yourtool.keymap.json"   # fixture key -> your output's dotted key
id_field  = "..."            # dotted path identifying which input event an output line is

[cold_start]
readme  = "README.md"        # or wherever install instructions live
heading = "## Quick start"   # the exact heading text above the fenced command block
stop_at_substring = " serve "  # optional: stop before a long-running/server command
```

**What a tool must provide to be measurable**, per criterion:
- Every criterion needs the `run` template.
- **correctness** needs `[correctness].key_map` and output events in the same
  order as the input (or an `id_field` a comparator could use to re-sort --
  this harness assumes order, see below).
- **raw_preservation** needs `raw_of`; `verify` is used if present, else that
  half is reported not-measurable.
- **container** needs both `[container]` keys and a working `docker`.
- **cold_start** needs `[cold_start].readme` to exist and contain a fenced
  code block under `heading`; a missing README or heading is a FAIL, not a
  skip -- that is itself a finding.
- **kill_recovery** needs the tool's `store`/`run` to be safely re-invocable
  against a store a previous run of the same tool left mid-write.
- Everything else (throughput, unknown_format, damaged_inputs, isolation,
  memory) only needs `run` and reads its output file's line count and the
  process's own exit code/signal/RSS -- no tool-specific knowledge required.

A tool missing an optional piece is scored **"not measurable: `<reason>`"** on
that line, never silently skipped and never counted as a pass or a fail.

## Running it

```
eval/run.sh eval/tools/ulpf.toml                    # every criterion, full bench file
eval/run.sh eval/tools/ulpf.toml --quick             # 500k-line slice for throughput/memory/isolation/kill_recovery
eval/run.sh eval/tools/ulpf.toml correctness damaged_inputs   # just these
```
`EVAL_TOOL_BIN=/path/to/binary` skips `[build]` entirely (use a prebuilt
binary instead of triggering a build that would race a concurrent build in
the repo's own `target/`). `EVAL_TARGET_DIR` overrides where a build (if one
happens) puts its output. `ULPF_EVAL_QUICK_BENCH` overrides where the
`--quick` slice is cached (default `$TMPDIR/ulpf-eval-quick-bench.log`).

Every run writes `eval/results/<tool>-<timestamp>/scorecard.md` (also printed
to stdout) and `eval/results/<tool>-<timestamp>/raw/` holding, per
measurement: the rendered command (`*.cmd`), stdout/stderr, the exit
code/wall-time/note (`*.exit`, as `rc|seconds|note`), and any derived data
(diffs, RSS series, socket samples). `run.sh` exits non-zero only if the
**harness itself** broke (a bug in run.sh, an unwritable results dir); a
tool's non-zero exit, timeout, or empty output is a normal, fully-reported
result and never aborts the run -- see `nonzero_exit_behaviour` below.

## Criteria

### throughput
**Measures:** events/s and MB/s processing `bench/mixed-5000000.log` (or, under
`--quick`, its first 500,000 lines).
**How:** the harness starts the tool's `run` template, waits for it to exit
(a wall clock it times itself around the subprocess, in
`eval/lib/run_async.py` -- never a number the tool prints), then counts lines
in the declared output file. Three runs, fresh store each time; each run and
the median of the three are reported.
**Pass rule:** no pass/fail -- a reported number, for the head-to-head to compare.
**Needs:** `run` template; an output file whose line count equals events processed.

### correctness
**Measures:** parsed-event accuracy against `fixtures/*.expected.jsonl`.
**How:** each `samples/<x>.log` is run alone (so output line *i* corresponds
to fixture line *i* by input order -- the measurability requirement named in
the brief: **an output line must carry the input's identity, or output order
must equal input order**), then `eval/lib/jsoncmp.py` compares each fixture
event's `normalized` subset (plus `time`, `time_policies`, `parser`, `status`,
`sub` where the fixture asserts them) against the same event in the tool's
output, with fixture keys translated through `[correctness].key_map`. A
fixture value of `"none"` matches an absent output key (fixtures/README.md's
convention for "no parser").
**Scope:** only fields observable in a tool's *emitted output* are checked.
Fixtures also carry a `fields` key (the vendor-native parsed fields before
normalization) and `absent`; those describe an internal pipeline stage no
external harness can see in arbitrary JSONL output, so they are out of scope
here by design -- ULPF's own `cargo test -p ulpf --test fixtures` already
covers that stage against the same fixtures.
**Known gap (recorded, not hidden):** fixtures were generated with the
receipt clock frozen at a fixed instant (fixtures/README.md); the public CLI
has no flag to pin receipt time, so any event whose time falls back to
receipt time (no timestamp found in the message) will legitimately mismatch
on `time` alone. Affects a small, fixed set of events per sample file.
**Pass rule:** reported as matched/mismatched/missing count and percentage,
per sample file and in total. No fixed bar; a stranger reads the diff files.
**Needs:** `[correctness].key_map`; input-order-preserving output.

### raw_preservation
**Measures:** every input byte is recoverable from the tool's own store.
**How:** run `samples/cisco_asa.log` alone into a fresh store; run `verify`
(if present); then, for up to the first 20 events, compare `raw_of {id}`'s
stdout byte-for-byte (`cmp`) against `eval/lib/frame.py`'s independent framing
of the same input file. **Framing rule** (tool-agnostic, defined once here):
line-oriented; an event is one line plus every following line starting with
space, tab, CR or LF, terminators kept verbatim. This assumes a tool assigns
ids 0..N-1 to a single input file's events in that file's line order --
documented as the contract a tool's id scheme must satisfy to be measured
this way.
**Pass rule:** records verified / mismatches out of the sampled ids; `verify`'s
own corrupt count reported alongside.
**Needs:** `raw_of`; `verify` is optional (reported not-measurable alone if absent).

### unknown_format
**Measures:** behavior on `heldout/mikrotik.log`, a format no shipped parser covers.
**How:** one `run`; report the output line count (events still emitted despite
no match), files written under the run's `--pending`-equivalent directory
(proposal count), and exit code.
**Pass rule:** descriptive only -- a tool that emits 0 events and proposes
nothing has a real gap; a tool that emits every line with no crash and offers
a candidate parser is doing the harder, better thing. No numeric bar.

### damaged_inputs
**Measures:** behavior on the fixed corpus in `eval/damaged/` (12 files,
generated deterministically by `eval/make_damaged.sh` from `samples/cisco_asa.log`
as seed material -- samples/ itself is never modified): empty file, newlines-only,
a single 8 MiB line, binary garbage, UTF-16 with BOM, CRLF, a truncated final
line with no trailing newline, a NUL byte spliced into a valid log, a 0-byte
file nested three directories deep, a self-referential symlink loop, a valid
log with every 10th line cut mid-field, and a format no parser covers.
**How:** each file run alone, 60 s timeout per file.
**Pass rule, per file:** exit code, events emitted, crashed (killed by
signal), hung (hit the 60 s timeout), and stderr's last line. A tool that
never crashes or hangs and always reports *some* outcome (even "0 parsed")
passes the bar this harness holds; silent hangs or signal deaths are named,
not scored on a curve.

### isolation
**Measures:** the running process opens no non-loopback socket.
**How:** `eval/lib/sockets.sh` samples the live pid's open sockets (`lsof` on
macOS, `ss` on Linux) every 0.1 s while it processes the throughput input, and
classifies every distinct one seen: loopback listen/connect is OK, anything
else is FAIL. Same classification rule as `scripts/isolation.sh`; ULPF ships
that script itself as a second, tool-specific opinion (`ULPF_BIN=<binary>
scripts/isolation.sh run <file>`), reported alongside for cross-check, not in
place of the generic sampler.
**Pass rule:** PASS iff zero non-loopback sockets were ever observed; a run
too short to sample even once is reported, not silently passed.
**Needs:** nothing beyond `run`; a real PID the harness can watch (i.e. the
tool must not double-fork away from the process the harness started).

### container
**Measures:** the tool builds and runs fully offline in a container.
**How:** `docker build` from `[container].build`; `docker run --network none`
over `samples/`, output captured to a mounted directory.
**Pass rule:** reports image size (bytes), build exit code, run exit code,
events emitted. A non-zero run exit under `--network none` after a successful
build is the interesting failure to read closely -- it usually means the
tool tried to reach the network.
**Needs:** `[container]` build+run templates and a local `docker`.

### cold_start
**Measures:** whether a stranger can go from `git clone` to a working run
using *only* the tool's own documented instructions.
**How:** `git clone` the repo's HEAD into a fresh temp directory (a local,
read-only clone of this repo -- no push, no change to the working tree);
extract the fenced code block under `[cold_start].heading` in
`[cold_start].readme` (`eval/lib/extract_fence.py`); run each non-comment line
in the clone, in order, stopping before a line containing
`stop_at_substring` (a long-running server command, which cold-start
automation does not drive interactively).
**Pass rule:** PASS iff every executed command exited 0; otherwise FAIL naming
the first failing command. A missing README or missing heading is a FAIL
naming the missing step -- reported under `contract_gaps`, not swallowed as
"not measurable", because an undocumented install path is itself the finding.
**Reports:** the exact commands run and total wall time.

### memory
**Measures:** peak RSS and its trend during the throughput run.
**How:** `ps -o rss=` on the live pid every 0.5 s (same on macOS and Linux);
the full series is written to `raw/memory-rss.tsv`. Peak is the max sample;
"slope" is the endpoint-to-endpoint rate over the last 120 samples (60 s, or
the whole run if shorter) -- a cheap, deliberately simple flat-vs-growing
signal, not a regression fit (`ponytail:` ceiling in `eval/run.sh`; upgrade to
a real least-squares slope if a borderline case needs it).
**Pass rule:** reported, not scored -- a flat line and a climbing line read
differently to a human without needing a threshold.

### kill_recovery
**Measures:** whether killing the tool mid-run loses or double-counts events on restart.
**How:** (1) run the throughput input to completion once, fresh store, for a
baseline event count; (2) start it again into a second fresh store, `kill -9`
after 5 s (aborted as not-measurable if the run already finished by then --
notably possible under `--quick`, which is why kill_recovery is one of the
criteria `--quick` also shrinks the input for); (3) run `verify` against the
half-written store if the tool has one; (4) re-invoke `run` with the *same*
store and input to let the tool finish the job; (5) compare the final output
line count against the step-1 baseline.
**Pass rule:** "consistent" if the final count equals the baseline;
"DOUBLE-COUNTED" if higher, "LOST INPUT" if lower. Restart cleanliness is the
resume run's own exit code.
**Needs:** `run` safely re-invocable against a store left mid-write by a
killed run of the same tool; `verify` optional.

## The 04:00 procedure

1. Both tools' `eval/tools/*.toml` finalized and each already run at least
   once solo (so a build/config bug surfaces before the head-to-head).
2. Same machine, quiet (no other heavy process -- check `ps`/Activity
   Monitor first; a loaded machine invalidates throughput and memory numbers).
3. Same bench file for both: `bench/mixed-5000000.log`, regenerated fresh if
   either tool could have influenced it (it can't -- `bench/README.md`'s
   generator only reads `samples/`).
4. For each tool: `eval/run.sh eval/tools/<tool>.toml` (no `--quick` -- that
   flag exists only for development on a busy machine, and every quick-mode
   line in a scorecard says so) three times; keep all three
   `eval/results/<tool>-<timestamp>/` directories; report the median of the
   three per numeric criterion, plus all three raw numbers.
5. Do not average across tools' *different* runs into one number -- report
   each tool's own three-run median side by side.
6. Commit nothing from either tool's run automatically; a human reviews
   `eval/results/` and commits what's worth keeping.

## Adversarial behavior (nonzero_exit_behaviour)

A tool's command exiting non-zero, hanging, or writing nothing is a normal
result, reported under its criterion, never a harness crash: `run_async.py`
runs every tool subprocess with no shell (so path spaces are safe) and an
explicit timeout that SIGKILLs a hung process and records `note=timeout`;
`run.sh` wraps every criterion function in a subshell and only turns
non-zero into `HARNESS ERROR` (the one case that flips the script's own exit
code) if the *function itself* broke -- a bad tool result never does.
Verified by pointing a throwaway tool config's `run` template directly at
`false` (recorded exit 1, `note=ok`) and at `sleep 1000` with a 2 s timeout
(recorded exit -9, `note=timeout`, process actually killed) -- both surfaced
as ordinary reported results, `run.sh`'s own exit code stayed 0.
