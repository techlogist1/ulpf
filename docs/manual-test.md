# Manual test plan: the CLI and the desktop app, on Windows 11 and on macOS

Two machines, one product. This is the list a person works through by hand, in order, with
the expected observation beside every step and the file that observation came from in
parentheses. It is not the demo script: the demo is step by step in `PROGRESS.md`'s demo
section, and running it is itself a test (it appears below as one).

Every expectation here was read out of the tree, not remembered. Where an expectation exists
only on a branch not yet merged it is marked **(with lane 7C)**; where nothing in the tree
says what will happen, it says so and asks you to record what you saw instead of matching it
against a claim.

---

## 0. Before you start

### What to install or build

**The CLI.** Either build it or download it.

- From source, on both machines: Rust 1.95 or newer, then `cargo build --release` at the
  repo root. About a minute; the binary is `target/release/ulpf` (`target\release\ulpf.exe`
  on Windows). This is the profile the README's quick start and every documented command
  use, and it deliberately has LTO off so a first build finishes fast (D88).
- From a release: the assets are `ulpf-<tag>-x86_64-pc-windows-msvc.exe` and
  `ulpf-<tag>-aarch64-apple-darwin`, beside a `SHA256SUMS`, on
  `https://github.com/techlogist1/ulpf/releases` (README, "Get it"). Numbers you measure on
  a downloaded binary are `--release` numbers too: the workflow's release assets are built
  with `cargo build --release` and override no profile (`.github/workflows/app.yml`, the
  `cli` job; README, "Which build"). **(with lane 7C)** that changes: both the `cli` and the
  `bundle` job move to `cargo build --profile dist`, which is `release` plus fat LTO, so a
  downloaded binary is then the profile the throughput numbers were measured on
  (`git show lane-7b-app:.github/workflows/app.yml`, both jobs; the `[profile.dist]` comment
  in `Cargo.toml` names the move). Record which of the two you tested on.

**The desktop app.** From the release page, not from a CI artifact.

| | asset |
|---|---|
| Windows | `ULPF_0.1.0_x64-setup.exe` (NSIS, the one to use) or `ULPF_0.1.0_x64_en-US.msi` |
| macOS | the `.dmg` (or the `.app` inside it) |

`tauri-action` creates that release as a **draft**, and a draft's files are invisible to
anyone not logged in with write access — an unauthenticated visit to the tag page answers
200 and shows no assets at all (app/README.md, "Get the installer"). So either publish the
release first (Releases > the tag > Edit > Publish release, with "Set as a pre-release"
ticked) or download while logged in. The tags are `v0.1.0-rc1`, `v0.1.0-rc2`, ….

**Nothing in this repository's `parsers/` may carry `origin = "inferred"` when a bundle is
built.** A generated parser inside the installer means the unseen format is recognised on
first run and no proposal is ever raised. **On main nothing catches this** — the two sidecar
scripts (`app/scripts/sidecar.sh`, sixteen lines, and `app/scripts/sidecar.ps1`, twelve) check
only that the engine binary exists — so on main the rule is yours to keep by hand. **(with lane
7C)** two gates appear. Both sidecar scripts test each `parsers/*.toml` for a line matching
`^\s*origin.*inferred` as their first command, name the offending files one per line and
refuse: `sidecar.sh: the bundle would carry a generated parser:` then `sidecar.sh: remove the
N of them with: ulpf demo --reset`, and on Windows `sidecar.ps1: the bundle would carry a
generated parser (listed above); remove it with: ulpf demo --reset`. And the app's first-run
copy skips such a file rather than copying it (`git show lane-7b-app:app/scripts/sidecar.sh`
lines 20-35, and `sidecar.ps1` lines 11-18; `prepare()` and `is_generated()` in
`git show lane-7b-app:app/src-tauri/src/lib.rs`). Either way `ulpf demo --reset` removes
them (`purge_generated` in `crates/ulpf/src/demo.rs`) — that is the command to run before a
bundle, on both branches. So: approve nothing from the CLI before the installers are built
or the video is recorded (PROGRESS, demo section, "Nothing is approved from the CLI").

**The binaries are unsigned.** There is no signing or notarization step anywhere in
`.github/workflows/app.yml` and no signing identity in `app/src-tauri/tauri.conf.json`. On
Windows that produces a known dialog (see the Windows caveats). On macOS the same absence
means Gatekeeper is expected to object to a downloaded `.dmg` or binary; nothing in the tree
records which dialog or which click, so treat the macOS case as unrecorded and write down
exactly what you see. Building from source sidesteps it on both machines.

### Where the data lives, and how to get back to a first run

| | macOS | Windows |
|---|---|---|
| App data directory | `~/Library/Application Support/dev.ulpf.desktop` | `%APPDATA%\dev.ulpf.desktop` |
| The override, if one was chosen | `<config dir>/data_dir`, one line | `%APPDATA%\dev.ulpf.desktop\data_dir` |
| The intensity setting | `<config dir>/intensity`, one word | `%APPDATA%\dev.ulpf.desktop\intensity` |
| The engine's own output | `<data>/engine.log` | same |

(app/README.md, "Where the data lives"; `configured_data_dir` in
`app/src-tauri/src/lib.rs`; `file()` in `app/src-tauri/src/intensity.rs`.)

Inside the data directory: `watch/` (what you added; `staging/` holds a copy in flight),
`store/`, `out.jsonl` with its `.pivot` and `.meta.json`, `pending/`, `parsers/` and
`mappings/` (copied from the bundle on first run, then yours to edit), `engine.log`, and
`server.url` while the engine is serving (app/README.md).

**Reset to a first run:** quit the app, then delete that one directory. It holds the data,
the `data_dir` override and the `intensity` word, so removing it puts the next launch back
to a fresh install: Balanced intensity (`load()` in `intensity.rs` returns Balanced for a
missing or unreadable file) and a re-seeded `parsers/`. `engine.log` is truncated at each
start (`fs::File::create` in `lib.rs`), so copy it out before you relaunch if a failure is
in it.

### What to have open

A terminal (zsh on the Mac) or PowerShell on Windows; a browser; on Windows, Task Manager.
Keep a scratch directory for stores and outputs so nothing lands in the repo.

### How to report a failure

Five things, in this order:

1. **The sentence on screen, verbatim.** The app never shows a blank page or an endless
   spinner: every failure is a sentence, the way out, and the file to read (app/README.md,
   "When it does not start"). Copy it exactly — the words are the diagnosis.
2. **`engine.log`** from the data directory above.
3. **`ulpf --version`** — it exists (`#[command(name = "ulpf", version, ...)]` in
   `crates/ulpf/src/cli.rs`) and prints the crate version.
4. **The OS build**: `winver` on Windows, `sw_vers` on macOS.
5. **Which step number** in this file, and whether you built from source or installed.

---

## 1. The CLI, macOS (zsh)

Run from the repo root. `$S` is a scratch directory; make one first (`mkdir -p /tmp/mt`,
then `S=/tmp/mt`). The commands are the README's quick start, which is not an example of the
install path but the install path itself — `eval/lib/extract_fence.py` reads that fenced
block out of the README and re-runs it in a fresh clone (README, "Quick start").

**Always name the log files, never the bare `samples` directory.** The engine has no include
filter, so `samples` ingests `samples/README.md` as a log: 16 files and 354 events instead
of 15 and 309, `no_parser` 41 instead of 2 (README, "Run it"; D91, the rule; D83 reserves the
post-demo decision on whether a directory-level filter should exist at all).

| # | Command | Expected |
|---|---|---|
| M1 | `cargo build --release` | About a minute; `target/release/ulpf` exists (README, "From source"). If the last line is a `Compiling` line the build is not finished — look for `Finished`; a gate once ran its checks against a stale binary for exactly this reason (PROGRESS v4, 05:11 IST). |
| M2 | `./target/release/ulpf --version` | The version prints (`cli.rs`). Record it. |
| M3 | `./target/release/ulpf check --pending pending` | One `parser  <name> <vendor> <product> (N subs, M timestamp candidates)` line per definition, one `mapping` line per schema, then `15 parsers, 2 mappings loaded; 0 problems`; exit 0 (`Cmd::Check` in `cli.rs`; PROGRESS, demo section). A non-zero exit means problems, and the `ERROR` lines above it name the path. |
| M4 | `./target/release/ulpf run samples/*.log --store $S/store --output $S/out.jsonl` | The counter block. The expected key counts for the fifteen samples: **framed 309, stored 309, detected 307, no_parser 2, parsed 305, parse_failed 2, normalized 309, emitted 309**, under a header line reading `15 files (0 failed), 0.10 MB, 309 events`. These are the numbers in the README's own Run-it block and they are the expected ones (README, "Run it"). |
| | | Also expected there: `parse_failed by reason: pattern_no_match 1, invalid_json 1`, and `pending: 0 proposals awaiting review`. The `events/s` on the header line is 309 events in five milliseconds — startup noise, not a throughput measurement, and the README says so in the same place. Do not report it as a number. |
| M5 | `./target/release/ulpf verify --store $S/store` | `store <hex> genesis <hex>`, then `verified 309 records, 0 corrupt`, then `chain ok (head <hex>)`; exit 0 (`Cmd::Verify` in `cli.rs`). |
| M6 | `./target/release/ulpf raw 0 --store $S/store` | On **stderr**: `raw id 0  source <file>  received <rfc3339>  <N> bytes  sha256 <hex>`. On **stdout**: the exact original bytes, line terminator included (`Cmd::Raw` in `cli.rs`). The header is on stderr so that redirecting stdout gives you the bytes alone. |
| M7 | `./target/release/ulpf attest --store $S/store --out $S/att.json`, then `./target/release/ulpf verify --store $S/store --attestation $S/att.json` | Both exit 0; the second checks the store against the document a stranger would re-verify offline (README, "Quick start"; PROGRESS step 9). |
| M8 | **A second writer is refused.** Terminal 1: `./target/release/ulpf serve $S/watch --store $S/store2 --output $S/o2.jsonl --parsers parsers --pending $S/pend --listen 127.0.0.1:7979`. Terminal 2, same store: `./target/release/ulpf run samples/cisco_asa.log --store $S/store2 --output $S/o3.jsonl` | Terminal 2 fails with `store <dir> is in use by another process` and writes nothing (`open_catalog` in `crates/ulpf-store/src/store.rs`, and its "One writer at a time" module note). This is the invariant, not a fault: the store takes one writer and the lock dies with the process. **Then ctrl-c terminal 1 before M9.** The `--listen` is not optional and neither is stopping it: `serve` without `--listen` binds `127.0.0.1:7878` (the clap default in `cli.rs`), and `demo --check` tests `port 127.0.0.1:7878 free` as one of its items and prints `demo --check: DRIFT` with exit 1 on any failed item (`check()` in `demo.rs`) — so a server left running here makes M9 fail for a reason that has nothing to do with drift, and M10 and M11 cannot run at all. |
| M9 | `./target/release/ulpf demo --check` | A list of `ok    <label>` lines — the samples, the two held-out files, `mappings/ocsf.toml`, `parsers/*.toml`, both ports, then every step title and every command — ending `demo --check: no drift`; exit 0. A `DRIFT` line means the runner and `PROGRESS.md` no longer read the same text (`check()` in `demo.rs`). The gate has been seeing 39 ok lines (PROGRESS v4, 05:34 IST). |
| M10 | **The busy-port refusal.** With something on the port (`python3 -m http.server 7878`), run `./target/release/ulpf demo --auto` | Exits 1 **at once** with `port 127.0.0.1:7878 is in use (a server from an earlier rehearsal?): stop whatever holds it (...) and run again`, naming `lsof -nP -i :7878` for macOS, and leaves no `demo/` (`refuse_busy_ports` in `demo.rs`; PROGRESS v4, 05:34 IST). Free the port before continuing. |
| M11 | `./target/release/ulpf demo` (interactive; 7878 and 5514 must be free) | Fourteen numbered steps, Enter between them, each printing the shell command before it runs it. Step 0 removes `demo/` and any generated parser; step 1 copies `parsers/` into `demo/parsers`, starts the server and prints the URL with the key map; step 2 copies the fifteen samples in one per second and sends `heldout/edgerouter.log` over UDP; step 3 drops `heldout/mikrotik.log` and prints `(proposal mikrotik after N s)`; step 4 waits for you to approve in the UI and otherwise POSTs the approve itself; step 5 re-drops the file so it parses; step 6 prints 40 lines of `/api/events/0`; step 7 breaks `demo/parsers/cisco_asa.toml`, feeds events under the bug, restores it and replays; step 8 writes `gw-drift.log` and waits for the update proposal; step 9 attests, verifies, tampers one byte and verifies again — **the non-zero exit and the record it names are the point**; steps 10-13 are printed, not played. At the end the server stays up for questions and one more Enter stops it and removes `demo/` (`play()` and `main()` in `demo.rs`; PROGRESS, demo section). |
| | | Reference timings from `--auto` passes on this Mac: 56 s end to end, the proposal 0.3-0.6 s after the drop, approve `now_detected 250/250, parsers_loaded 16`, replay v2 over about 1,089 events, the drift update 5.9-6.1 s after the new lines, attestation 2 of 2 checkpoints over 2,739 records, the tamper naming raw id 0 with exit 1, reset clean (PROGRESS v4, Verified state). |
| M12 | `./target/release/ulpf demo --reset` | `reset: demo removed, N generated parser(s) removed from parsers/`. A server left by an interrupted rehearsal is stopped first, and only if that pid is still a `ulpf serve` (`kill_leftover` in `demo.rs`). |
| M13 | `mkdir -p demo/watch && ./target/release/ulpf serve demo/watch --store demo/store --output demo/out.jsonl`, then open <http://127.0.0.1:7878> | The UI. `0` is Flow, `1`-`7` are Live, Review, Traceback, Pivot, Replay, Drift, Integrity, `?` opens the key map, `t` switches light and dark (docs/design.md, Keyboard map). Copy a log file into `demo/watch` and the screens move within 500 ms (README, "See it"). |

`ulpf demo` uses 7878 and 5514, and those ports belong to the demo; a server of your own
alongside it needs a different `--listen`.

---

## 2. The CLI, Windows 11 (PowerShell)

The same commands in PowerShell form: the executable takes `.exe`, paths take backslashes
and `$env:TEMP`, and the glob has to be expanded by the shell because the engine takes file
arguments, not patterns (README, "On Windows").

| # | Command | Expected |
|---|---|---|
| W1 | `cargo build --release` | The same command and the same minute (README, "On Windows"). It needs the **Microsoft C++ Build Tools**, workload *Desktop development with C++* — nothing links without it (app/README.md, "Building from source on Windows"). |
| W2 | `.\target\release\ulpf.exe --version` | The version prints. Record it. |
| W3 | `.\target\release\ulpf.exe check` | As M3: `15 parsers, 2 mappings loaded; 0 problems`, exit 0. CI asserts the words `parsers` and `0 problems` on this exact call (`app.yml`, the `smoke-windows` job). |
| W4 | `.\target\release\ulpf.exe run (Get-ChildItem samples\*.log).FullName --store $env:TEMP\ulpf-store --output $env:TEMP\out.jsonl --pivot on` | The counter block with the same fifteen-sample counts as M4 (framed 309, stored 309, detected 307, no_parser 2, parsed 305, normalized 309, emitted 309 — README's Run-it block). `--pivot on` builds the entity index beside the output and costs about an order of magnitude of throughput, by design (README, "Honest numbers"; D66). |
| W5 | `.\target\release\ulpf.exe verify --store $env:TEMP\ulpf-store` | As M5, exit 0. CI runs this on Windows and fails the job on a non-zero exit (`app.yml`, `smoke-windows`). |
| W6 | `.\target\release\ulpf.exe raw 0 --store $env:TEMP\ulpf-store` | As M6: header on stderr, exact bytes on stdout. |
| W7 | `.\target\release\ulpf.exe attest --store $env:TEMP\ulpf-store --out $env:TEMP\attest.json`, then the same `verify` with `--attestation $env:TEMP\attest.json` | Both exit 0 (README, "On Windows"). |
| W8 | **A second writer is refused.** `serve` in one PowerShell against a store **with `--listen 127.0.0.1:7979`**, `run` in another against the same store | `store <dir> is in use by another process` (`store.rs`). Same invariant, same message: the lock is SQLite's, and the OS releases it when the holder dies. Then ctrl-c the `serve`, for the reason M8 gives: without `--listen` it holds `127.0.0.1:7878` and W9 fails on the port item, not on drift (`cli.rs`; `check()` in `demo.rs`). |
| W9 | `.\target\release\ulpf.exe demo --check` | As M9. |
| W10 | `.\target\release\ulpf.exe demo` | As M11, from PowerShell with no shell involved — the runner is a subcommand precisely so the demo plays where no shell exists (D67; README, "On Windows"). **Two of its Windows branches have never been executed**: `taskkill` (`kill_pid`) and `tasklist` (`is_ulpf_serve`) are compiled by the `windows-latest` job and have not run (PROGRESS, Runner section). They fire only when an earlier rehearsal left a server behind, so provoke it: start `demo`, close the PowerShell window on it, start `demo` again, and record whether it prints `stopping the server left by an earlier run (pid N)`. |
| | | Bold is off on Windows (`BOLD` is empty under `cfg(windows)` in `demo.rs`), so the step titles are plain text. Expected, not a rendering fault. |
| W11 | `.\target\release\ulpf.exe demo --reset` | As M12. |

### The Windows caveats the tree documents

- **Do not send `--output` to `NUL`.** On the current release a run whose `--output` is the
  null device still writes a `NUL.v1.meta.json` beside it, with an `events` count of 0. Give
  it a path under `$env:TEMP` instead. The fix (`NUL`, `\\.\NUL` and `\\?\NUL` recognised as
  sinks, and the count written from what the run emitted) is on branch `lane-8-windows` and
  lands after the demo (README, "On Windows"). Until then, wherever a macOS command says
  `--output /dev/null`, write a temp file.
- **The shell scripts need Git Bash.** `scripts/isolation.sh` and `scripts/coverage.sh` are
  bash: run them from a Git Bash prompt, which ships with Git for Windows. `scripts/demo.sh`
  is only a wrapper that finds the binary, so skip it and call
  `.\target\release\ulpf.exe demo` directly (README, "On Windows"; D67).
- **Defender and SmartScreen on the installer.** It is unsigned, so Windows shows a blue
  dialog titled `Windows protected your PC`: click **More info**, then **Run anyway**. There
  is no certificate for this build; a signed one would skip the dialog (app/README.md,
  "SmartScreen"). A downloaded CLI `.exe` is unsigned for the same reason — no signing step
  exists in `.github/workflows/app.yml` — and may draw the same dialog or a Defender prompt;
  nothing in the tree records that case, so write down what you actually see.
- **The engine's one dependency, on a build from before 06 Sep 04:45.** `ulpf.exe` used to
  import `VCRUNTIME140.dll`, from the C in `rusqlite`'s bundled SQLite. Without the
  Microsoft Visual C++ 2015-2022 redistributable the loader fails before the program runs,
  and the app shows `The engine stopped (exit 3221225781)` — 0xC0000135,
  STATUS_DLL_NOT_FOUND — with `It printed nothing.` and an empty `engine.log`. That exact
  pair of symptoms is this missing DLL and nothing else. Either
  `winget install Microsoft.VCRedist.2015+.x64`, or take a build from after 06 Sep 04:45:
  the workflow's Windows engine step sets `RUSTFLAGS=-C target-feature=+crt-static` and
  links the runtime in (app/README.md, "One dependency the engine still has"; `app.yml`).
- **`(Get-ChildItem samples\*.log).FullName` is not optional.** A bare `samples` ingests
  `samples\README.md`; CI's own Windows job passes the bare directory and therefore
  compensates in its arithmetic (`app.yml`, `smoke-windows`: 355 non-empty lines over
  sixteen files, minus one folded continuation line). Do not copy that form into a manual
  run.

---

## 3. The desktop app

The steps are the same on both machines; where a machine differs, the "How to tell" column
says which. Do step A1 on a **fresh** data directory (section 0 says how) so the first-run
copy and the proposal flow are real.

The reference numbers below assume an eight-core machine, because that is what the Intensity
labels were written against (app/README.md, "Intensity"). On another core count the labels
carry that machine's own numbers: Low is two cores (one under four cores), Balanced is half,
Max is all but one (`threads()` in `app/src-tauri/src/intensity.rs`).

### First launch and the window

| # | Action | Expected | How to tell |
|---|---|---|---|
| A1 | Install and launch: Start menu > `ULPF` on Windows, the `.app` on macOS | The window opens on the bundled splash — the ULPF mark and one status line: `Starting the engine at Balanced: 4 of 8 cores, entity index on`, with a pulsing dot. The title bar reads `ULPF · starting engine…` while it starts (`start()` in `app/src-tauri/src/lib.rs`; the window's `title` in `app/src-tauri/tauri.conf.json`). | The line is on the page, not in a log. |
| A2 | Wait for the engine | The window navigates to the served UI and a notice at the bottom says `Engine ready at Balanced · 4 of 8 cores · entity index on` (`ready_notice` in `intensity.rs`). `<data>/server.url` now exists and holds the URL; it is written only after `/api/status` answered (D73). | macOS: `cat "$HOME/Library/Application Support/dev.ulpf.desktop/server.url"`. Windows: `Invoke-RestMethod "$(Get-Content $env:APPDATA\dev.ulpf.desktop\server.url)/api/status"` answers JSON (app/README.md, step 3). |
| A3 | Read the title | `ULPF · engine ok · N events · M pending · Balanced · 4 of 8 cores · index on`, refreshed once a second. The core count and `index on/off` come from the running engine's `/api/status` (`threads`, `pivot_index`), not from the setting file, so while a restart is in flight the title says `restarting` rather than quoting a number nothing is using (`intensity_part` in `title.rs`; D84). | Read the window title. |
| A4 | Check the first-run copy | `<data>/parsers/` holds the fifteen bundled definitions and `<data>/mappings/` holds two. Nothing named `*_inferred.toml` is there. They are seeded only when the directory holds no TOML, so an approved or edited parser is never overwritten on a later launch (`prepare()` in `lib.rs`; D73). | List the directory. **(with lane 7C)** `engine.log` also says `[shell] seeded 15 parsers definition(s) from the bundle` and names anything it skipped: `[shell] skipped N generated parsers definition(s) (origin = "inferred"): <files>` (`seed_dir` in `app/src-tauri/src/guard.rs`). |
| A5 | Resize the window | It opens at 1280x820 and will not go below 720x480 (`tauri.conf.json`). At the minimum nothing overlaps: wide content (the byte ruler, the diff, the tail) scrolls inside its own container, and long lists render only the rows in view (docs/design.md, "Under load"). | Drag the corner to the minimum and back. |
| A6 | Press `t` | The whole UI switches between dark and light. Every colour is a token with a light value and a contrast row (docs/design.md, the colour tables and the Keyboard map). | Check the four state colours (green, amber, red, blue) are still legible on the light ground. |
| A7 | Press `?` | The full key map in two columns; Esc or a click outside closes it (docs/design.md, the component inventory). Everything below is reachable from it. | Compare it against docs/design.md's Keyboard map. |

### The Intensity setting

| # | Action | Expected | How to tell |
|---|---|---|---|
| A8 | Open `File > Intensity` | Exactly three items — `Low · 2 of 8 cores · entity index off`, `Balanced · 4 of 8 cores · entity index on`, `Max · 7 of 8 cores · entity index on` — with the running one check-marked. It is in the **window** menu only; the tray does not repeat it (the `intensity` submenu in `menu.rs`; app/README.md, "Intensity"; D84). | Read the menu. On macOS `File` is in the menu bar; on Windows it is in the window's own menu bar. |
| A9 | Choose `Max` | A notice on the page that is up: `Restarting the engine at Max: 7 of 8 cores, entity index on`. Then the engine is killed and started again on a fresh free port against the same store, and a second notice says `Engine ready at Max · 7 of 8 cores · entity index on`. About 1.2 s end to end on the Mac (D84). Nothing in the store or the output is lost: the kill is the one Quit uses and the engine's kill recovery makes it safe (D59). | Watch the two notices and the title. The check mark moves to Max; all three items are set, not only the new one (`check_marks` in `intensity.rs`). |
| A10 | Quit and relaunch | It comes back at Max: the word is kept in `<config dir>/intensity` and the check mark follows it. A missing or unrecognised file means Balanced (`load()` in `intensity.rs`). | `cat` the `intensity` file, then read the menu. |
| A11 | Choose `Low`, then open Pivot (key `4`) | Low is the only choice that gives up the entity index, and Pivot is the screen that reads it, so Pivot has nothing to answer from. The index costs about an order of magnitude of bulk throughput, which is why it is dropped only there (app/README.md, "Intensity"; D66, D84). | The title says `index off`. Record what Pivot shows in that state, then set it back to Balanced. |

### Getting logs in

Four ways in, all landing in one function: each item is copied into `<data>/staging` and
renamed into `<data>/watch` under a unique name, so the engine's poller sees a complete file
or nothing (`ingest_paths` in `app/src-tauri/src/ingest.rs`; D73).

| # | Action | Expected | How to tell |
|---|---|---|---|
| A12 | `File > Add files…` (Cmd/Ctrl+O), pick `samples/cisco_asa.log` | A native file picker titled `Add log files to ULPF`, then a notice at the bottom of the window: `Added 1 file to the watch folder: cisco_asa.log` (`action("add_files")` in `menu.rs`; `ingest_paths`). | Press `1` for Live: the source appears with its counters and the tail moves (app/README.md, step 4). |
| A13 | `File > Add folder…` (Cmd/Ctrl+Shift+O), pick `samples` | A native folder picker titled `Add a folder of logs to ULPF`; the notice counts the regular files copied and names up to four of them, then `, …`. Folders keep their structure; only regular files are copied, and symlinks, sockets and devices are skipped (`copy_tree` in `ingest.rs`). | The notice's count, then Live's source list. |
| A14 | Drag `samples/openvpn.log` onto the window | The same notice. Tauri owns the drop wherever it lands on the window, on both platforms, so the served page never sees it (`dragDropEnabled` in `tauri.conf.json`; the `DragDrop` arm in `lib.rs`). | The notice, then Live. |
| A15 | Add the same file twice | The second copy is renamed, not overwritten: `cisco_asa (2).log` (`unique` in `ingest.rs`). | The notice names the new name. |
| A16 | Copy a file into `<data>/watch/` from the shell | The engine's own poller picks it up; the app is not involved. | Live's source list gains it. |
| A17 | Add a folder with no regular files in it | `Not copied: <name> (no regular files)` in the notice, rather than a silent nothing (`ingest_paths`). | Read the notice. |

### The proposal flow

| # | Action | Expected | How to tell |
|---|---|---|---|
| A18 | Add `heldout/mikrotik.log` | Within a few seconds the title says `1 pending`, and Live shows the source with `no_parser` climbing and its lines buffered. Clustering runs at the inference threshold (`serve`'s default is 64) (app/README.md, step 5; CLAUDE.md, Inference). | The title's `M pending`. |
| A19 | Press `2` (Review), then Enter on the row | The proposal: templates with `support` beside `verified`, and every slot with its name **and the reason that name was chosen** (`` key `src-mac` before the value ``, `` vocabulary: `{ip}:{port}->{ip}:{port}` names src/dst ``, or why it stayed generic), plus example lines, the decision log and the unmatched lines by reason (docs/api.md, "Evidence"; PROGRESS step 3). | Read the slot table's reason column. |
| A20 | Press `a`, then Enter | `a` opens a confirmation; the letter that opens it can never confirm it. Enter confirms, Esc backs out, Tab moves between the two buttons, and focus lands on the confirming button (docs/design.md, "The confirmation"). The box states the file path that will be written, the version it replaces and what will be re-detected. | Afterwards, a result card with the path, the parser count and the re-detected count (`.proof` in docs/design.md's inventory). Expect `mikrotik_inferred`, `parsers_loaded 16`, and 250 of 250 re-detected (PROGRESS step 4). |
| A21 | Look at what was written | `<data>/parsers/mikrotik_inferred.toml` exists and carries `origin = "inferred"` with `priority = -1`, so a hand-written parser always wins (CLAUDE.md, "Nothing is parsed on a proposal"; D45). | Open the file. |
| A22 | Add `heldout/mikrotik.log` again | It parses: 250 of 250 detected on the new source, on the fast path (PROGRESS step 5; app/README.md, step 7). | Live's row for the new file. |
| A23 | Press `4` (Pivot) and search `src_ip` `203.0.113.9` | One entity across every device in one lane-per-device timeline, with "seen with" values you can click to pivot again; Backspace steps back along the trail (PROGRESS step 5; docs/design.md, Keyboard map). Needs the entity index, so not on Low (A11). | Read the timeline. |
| A24 | Press `x` on a proposal instead | The reject confirmation, marked `danger` in red; Enter confirms, Esc backs out. There is no single-click path to approve or reject (docs/design.md, "The confirmation"). | Read the box, then Esc — do not reject the proposal the rest of the list needs. |

### Traceback, integrity and export

| # | Action | Expected | How to tell |
|---|---|---|---|
| A25 | Press `1`, select a tail row, Enter (or press `3` for Traceback) | One event's raw bytes with its stored and recomputed SHA-256, `prev_chain` and `chain` with `chain_match`, and every normalized field lit over the bytes it was read from (README, "See it"; docs/api.md, "Provenance"). | The two digests are equal and `chain_match` is true. |
| A26 | Press `j` / `k`, then Enter, then Esc, then `h` | `j`/`k` walk the normalized fields, lighting each field's bytes; Enter (or a click) pins the range; Esc releases it; `h` switches the ruler between text and hex (docs/design.md, Keyboard map). | Watch the lit range follow the selection. |
| A27 | Press `7` (Integrity), then `v`, then Enter | A confirmation naming the record count, then the verify runs on a snapshot of the store on its own thread while the engine keeps ingesting; the result gives the elapsed time, the corrupt count and whether it was checked against the attestation (`ui/src/Integrity.svelte`; docs/api.md, "Integrity chain"). Expect 0 corrupt and a head value. | Read the verdict line. |
| A28 | On Integrity, use `Export attestation` | It points at `/api/integrity/attestation`, which is what a stranger re-verifies offline with `ulpf verify --store DIR --attestation FILE` (`Integrity.svelte`). **What the Tauri webview does with that link is not recorded anywhere in the tree**: it is a plain `target="_blank"` anchor and the shell installs no download handler (`lib.rs`). Record what actually happens. | If nothing usable happens: `File > Open in browser` and do it there, or run `ulpf attest --store <data>/store --out FILE` from the CLI. |
| A29 | On Live, press `e` | The export choice under the tail's head: jsonl or csv, this view or everything, and a sentence naming the raw id range and the filter terms that will be written. Enter takes the file, Esc closes. It writes nothing itself, so it is a choice and not a confirmation (docs/design.md, the inventory; docs/api.md, "Export"). | Same caveat as A28: the download is an `<a download>` to `/api/export` and its behaviour inside the webview is unrecorded. Record it, and fall back to the browser. |
| A30 | `File > Open output folder` (Cmd/Ctrl+Shift+E) | Finder or Explorer opens with `out.jsonl` selected; before the engine has written a line it opens the directory instead (`action("open_output")` in `menu.rs`). | Watch the file manager. |
| A31 | `File > Open in browser` (Cmd/Ctrl+Shift+B) | The default browser opens the served URL. Before the engine is serving it says `The engine is not serving yet.` instead (`action("open_browser")` in `menu.rs`). | The browser, or the notice. |
| A32 | `File > Choose data directory…` | A native folder picker titled `Choose where ULPF keeps its data`; the absolute path is written to `<config dir>/data_dir` and the engine restarts against the new directory. The old one is left exactly as it is — its store, output and proposals stay where they were (`action("choose_data")` in `menu.rs`; app/README.md). | The `data_dir` file, then the new directory filling up. Point it back afterwards by deleting `data_dir`. |

### Quit, relaunch and what is left running

| # | Action | Expected | How to tell |
|---|---|---|---|
| A33 | Close the window (the red button, or the X) | The window hides and the engine keeps ingesting. The tray brings it back: menu bar on macOS (a template glyph that follows the bar's style), notification area on Windows (the app icon) (`CloseRequested` in `lib.rs`; `menu.rs`; D73). | Copy a file into `<data>/watch` while the window is hidden; on `Show ULPF` the counters have moved. |
| A34 | **Clean quit**: tray > `Quit ULPF`, or Cmd/Ctrl+Q | Both the window and the engine stop; `<data>/server.url` is removed (`stop()` in `lib.rs`, from `RunEvent::ExitRequested` and `Exit`). | macOS: `pgrep -l ulpf` prints nothing. Windows: `Get-Process ulpf, ulpf-app -ErrorAction SilentlyContinue` prints nothing (app/README.md, step 8). |
| A35 | Relaunch | The record count does not go backwards and `out.jsonl` keeps its earlier lines: a restart completes an interrupted output from the store before ingesting anything new (D59). The title's **event count restarts at 0** — it is this run's counter, like the engine's counter block — and that is not a loss (app/README.md, step 8). | `GET /api/integrity` `records` is not below what it was; `wc -l` the output before and after. |
| A36 | **Force kill.** Windows: Task Manager > `ulpf-app.exe` > End task. macOS: `kill -9` the app process | This skips the clean quit path on both platforms. **On main** the engine can survive it: CI's own smoke run found `ulpf.exe` outliving a `Stop-Process` of the window and records that as the expected result there, with the tray's Quit left as the human check (`app/scripts/smoke-windows.ps1`; D89). **(with lane 7C)** a Windows job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` holds the sidecar for the app's whole life, so the kernel terminates `ulpf.exe` when the app process ends by any means (`adopt()` in `guard.rs`). On macOS the tree makes **no** claim that anything reaps it: `guard.rs`'s own comment says a SIGKILL of the app skips the quit path exactly as it does on Windows, and the leftover is caught on the next launch instead. | Windows: `Get-Process ulpf`. macOS: `pgrep -l ulpf`. Record the answer on each machine rather than confirming a claim; then clean up by hand and relaunch, which is A38. |

### The failure paths (a sentence, then the way out, then the file)

Each of these must land on the splash as a sentence — never a blank page, never an endless
spinner (app/README.md, "When it does not start"). Copy the sentence verbatim.

| # | Provoke it | Expected sentence |
|---|---|---|
| A37 | **Engine missing.** Rename or remove the sidecar beside the installed executable (`ulpf.exe` on Windows, `ulpf` on macOS) | `ULPF could not start its engine: <error>.` then `The engine ships beside the app as ulpf.exe; reinstalling ULPF replaces it.` (`ulpf` on macOS). Title: `ULPF · engine down (engine missing)` (the `spawned` error arm and `SIDECAR` in `lib.rs`). macOS capture: `docs/screens/app-error-sidecar.png`. |
| A38 | **Store held by another writer.** Start `ulpf serve` by hand against `<data>/store`, then launch the app | **On main**: the sidecar exits at once, so you get `The engine stopped (exit N). Its last words: <the store-in-use line>` and `The whole of its output is in <data>/engine.log` (the `Terminated` arm in `lib.rs`). **(with lane 7C)**: the splash names the holder before anything is spawned — `The engine's store at <path> is held by ulpf (pid N, started <when>). Stop it and start again?`, a `Details: <data>/engine.log` line, and one button, `Stop it and start again`, that stops that pid and retries through the same start path (`find_holder` and `stop_holder`; the `#act` button in `app/dist/index.html`). Title: `ULPF · engine down (store in use)`. |
| A39 | **Port in use.** Set `ULPF_APP_PORT` to a port something else already holds, then launch | `ULPF could not take port <N> on 127.0.0.1: <error>.` then `Quit whatever is listening there, or start ULPF with ULPF_APP_PORT unset and it will pick a free port.` Title: `ULPF · engine down (port in use)` (the `pinned` branch in `lib.rs`). Capture: `docs/screens/app-error-port.png`. Unset the variable afterwards: unset, the app asks the kernel for a free port and never collides with the demo's 7878. |
| A40 | **The engine dies.** Point `data_dir` at a path the user cannot write, or corrupt the store directory | `The engine stopped (exit N). Its last words: <the engine's own last line>` and `The whole of its output is in <data>/engine.log`; or `Cannot prepare <path>: <error>` when the directory itself could not be made (`fail` and the `prepare` error arm in `lib.rs`). Capture: `docs/screens/app-error-engine.png`. |
| A41 | **No answer.** Hard to provoke deliberately; report it if you see it | `The engine did not answer within two minutes.` — the start timeout (`START_TIMEOUT` in `lib.rs`). |

### macOS only

- The tray glyph is a template image (alpha only) drawn at runtime, so it follows the menu
  bar's light or dark style (`glyph()` in `menu.rs`). Switch the system appearance and check
  it stays legible.
- A click on the dock icon while the window is hidden shows it again (`RunEvent::Reopen` in
  `lib.rs`).
- The installed pair is `ULPF.app/Contents/MacOS/{ulpf-app,ulpf}` (app/README.md, "Platform
  differences"), and the splash is served from `tauri://localhost` (`SPLASH` in `lib.rs`).

### Windows only

- The installed pair is `ulpf-app.exe` + `ulpf.exe` in one directory, and NSIS `installMode`
  defaults to `currentUser`: it installs under `%LOCALAPPDATA%` and asks for no administrator
  (app/README.md, "Platform differences"). Task Manager shows `ulpf-app.exe` for the window
  and `ulpf.exe` for the engine.
- The installer carries the full WebView2 runtime (`offlineInstaller`), so nothing is
  downloaded at install time — 258,614,480 bytes of it, measured — and it is skipped on
  Windows 10 1803+ and Windows 11, which already have the runtime (`tauri.conf.json`;
  app/README.md, "The WebView2 runtime"; D89).
- The splash is served from `http://tauri.localhost` (`SPLASH` in `lib.rs`).
- The silent install CI performs is reproducible by hand:
  `pwsh app\scripts\smoke-windows.ps1 -Installer <the .exe>` runs the same install (`/S`),
  launch, `server.url`, `/api/status`, both-processes and kill sequence, and prints
  `SMOKE PATH: app` or `SMOKE PATH: sidecar` for the path it achieved
  (`app/scripts/smoke-windows.ps1`; the `app-smoke-windows` job in `app.yml`). Useful as a
  cross-check when a manual step disagrees.
- The `.msi` is built as well, but `ULPF_0.1.0_x64-setup.exe` is the one to use
  (app/README.md).

---

## 4. Results

One table per machine. Fill in `Pass` / `Fail` / `N/A`, and put the verbatim sentence, or
the number you saw, in the note. Take a screenshot for every `Fail` and for every step this
file says is unrecorded (A28, A29, A36, and the macOS Gatekeeper case in section 0).

### macOS (MacBook Pro M1) — `sw_vers`: ______  `ulpf --version`: ______  built / installed: ______

| step | pass/fail | note (the verbatim sentence, or the number seen) | screenshot |
|---|---|---|---|
| M1 | | | |
| M2 | | | |
| M3 | | | |
| M4 | | | |
| M5 | | | |
| M6 | | | |
| M7 | | | |
| M8 | | | |
| M9 | | | |
| M10 | | | |
| M11 | | | |
| M12 | | | |
| M13 | | | |
| A1 | | | |
| A2 | | | |
| A3 | | | |
| A4 | | | |
| A5 | | | |
| A6 | | | |
| A7 | | | |
| A8 | | | |
| A9 | | | |
| A10 | | | |
| A11 | | | |
| A12 | | | |
| A13 | | | |
| A14 | | | |
| A15 | | | |
| A16 | | | |
| A17 | | | |
| A18 | | | |
| A19 | | | |
| A20 | | | |
| A21 | | | |
| A22 | | | |
| A23 | | | |
| A24 | | | |
| A25 | | | |
| A26 | | | |
| A27 | | | |
| A28 | | | |
| A29 | | | |
| A30 | | | |
| A31 | | | |
| A32 | | | |
| A33 | | | |
| A34 | | | |
| A35 | | | |
| A36 | | | |
| A37 | | | |
| A38 | | | |
| A39 | | | |
| A40 | | | |
| A41 | | | |

### Windows 11 (ROG G615) — `winver`: ______  `ulpf --version`: ______  built / installed: ______

| step | pass/fail | note (the verbatim sentence, or the number seen) | screenshot |
|---|---|---|---|
| W1 | | | |
| W2 | | | |
| W3 | | | |
| W4 | | | |
| W5 | | | |
| W6 | | | |
| W7 | | | |
| W8 | | | |
| W9 | | | |
| W10 | | | |
| W11 | | | |
| A1 | | | |
| A2 | | | |
| A3 | | | |
| A4 | | | |
| A5 | | | |
| A6 | | | |
| A7 | | | |
| A8 | | | |
| A9 | | | |
| A10 | | | |
| A11 | | | |
| A12 | | | |
| A13 | | | |
| A14 | | | |
| A15 | | | |
| A16 | | | |
| A17 | | | |
| A18 | | | |
| A19 | | | |
| A20 | | | |
| A21 | | | |
| A22 | | | |
| A23 | | | |
| A24 | | | |
| A25 | | | |
| A26 | | | |
| A27 | | | |
| A28 | | | |
| A29 | | | |
| A30 | | | |
| A31 | | | |
| A32 | | | |
| A33 | | | |
| A34 | | | |
| A35 | | | |
| A36 | | | |
| A37 | | | |
| A38 | | | |
| A39 | | | |
| A40 | | | |
| A41 | | | |

---

## 5. Known limits: do not report these as new

Every item here is already recorded in `PROGRESS.md`'s v4 section or in `docs/DECISIONS.md`.

- **`verify` does not cover the record header's receipt time.** The digest and the chain
  cover the record bytes, not the header, so a byte changed inside a record header — its
  receipt time — leaves `verify` clean with exit 0. Found when the demo's tamper at byte 200
  of `raw.seg` fell inside record 1's header; the tamper moved to byte 100, inside record 0's
  body whatever the first sample is (the segment and record headers end at byte 68). Whether
  the header's receipt time belongs under the chain is a store-format question recorded for
  after the demo, not built (PROGRESS v4, "Tried and abandoned").
- **One known test flake.** `crates/ulpf/tests/v4_api.rs`'s
  `pivot_pages_by_the_cursor_pair_and_reports_its_timings` failed once at machine load 37
  ("saw 31 of 32") and passed three times alone and in the full run. A page takes the first
  `limit*4` entries past the cursor in raw-id order and re-sorts by device time, so an event
  whose device time disagrees with arrival order by more than that window is skipped
  (PROGRESS v4, "Tried and abandoned").
- **The tail's `frames skipped` / `events skipped` counters go amber during fast drops**
  (150 during a demo pass, 1,400 after replay and drift). That is the tail's honesty about
  what it did not render; `framed`, `stored` and `emitted` stay equal, so nothing was lost
  (PROGRESS v4, Verified state, 05:29 IST).
- **After a server stops, every screen shows `TypeError: Failed to fetch` with `retry 8s`.**
  That is the designed disconnected state, not a crash (PROGRESS v4, Verified state).
- **Three branches are not merged, so their features are absent, not broken.**
  `lane-5-xml` (the `xml` strategy and the Windows Event definition), `lane-6-index` (the
  entity index cost) and `lane-8-windows` (the store reopen, the stop path's handles and the
  null output device). D75, D76, D82 and D83 are reserved for them and written on those
  branches, so `docs/DECISIONS.md` on main carries only the reservation (PROGRESS v4, "In
  flight"; the "D75, D76: reserved" and "D82, D83: reserved" entries).
- **The Windows syslog receive buffer is the system default.** `set_recv_buffer` is a
  `cfg(windows)` no-op returning 0 and the asked/granted line says so; the Winsock version
  is on `lane-8-windows` (D74; PROGRESS v4, "In flight").
- **The demo runner has been played on macOS only.** Its two Windows branches (`taskkill`,
  `tasklist`) are compiled by the `windows-latest` job and have not been executed (PROGRESS,
  Runner section). Step W10 is where that changes.
- **Nobody has hand-launched the Windows installers.** CI's `app-smoke-windows` installs,
  launches and quits the app on a runner and prints which path it achieved, but the human
  click path — SmartScreen, the Start menu, the tray, the native pickers — is what this
  document exists to cover (D74, D86, D89).
- **The headline throughput figure is 258,411 events/s and it is a floor.** A quiet re-run on
  the dist build measured 295,928 (median of three); it is recorded but not promoted, because
  the committed scorecard holds the older number (README, "Honest numbers"; D87). Six runs
  under load on the same input spread 153k-309k, so a throughput figure without its machine
  state is not a figure — do not treat a number from a busy laptop as a regression. Related:
  the pivot-cache "cut 4-8x" claim was measured only at load 28-36; the quiet-machine number
  is 2.6-3.3x (PROGRESS v4, "Tried and abandoned").
- **The entity index costs about an order of magnitude of bulk throughput.** `run` defaults it
  off and `serve` defaults it on, and the app's Low intensity turns it off — so a Pivot screen
  with nothing in it on Low is the design, not a fault (D66, D84).
- **The app title's event count restarts at 0 on every launch.** It is this run's counter, the
  same meaning the engine's counter block gives it (app/README.md, step 8).
