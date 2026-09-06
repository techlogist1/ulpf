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
  `cli` job; README, "Which build").

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
first run and no proposal is ever raised. Two gates catch it: the sidecar step refuses to
run and names the files (`app/scripts/sidecar.sh`, `app/scripts/sidecar.ps1`) and the app's
first-run copy skips them **(with lane 7C)** (`seed_dir` in `app/src-tauri/src/guard.rs`).
`ulpf demo --reset` removes them (`purge_generated` in `crates/ulpf/src/demo.rs`). So:
approve nothing from the CLI before the installers are built or the video is recorded
(PROGRESS, demo section, "Nothing is approved from the CLI").

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
of 15 and 309, `no_parser` 41 instead of 2 (README, "Run it"; D83).

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
| M8 | **A second writer is refused.** Terminal 1: `./target/release/ulpf serve $S/watch --store $S/store2 --output $S/o2.jsonl --parsers parsers --pending $S/pend`. Terminal 2, same store: `./target/release/ulpf run samples/cisco_asa.log --store $S/store2 --output $S/o3.jsonl` | Terminal 2 fails with `store <dir> is in use by another process` and writes nothing (`open_catalog` in `crates/ulpf-store/src/store.rs`, and its "One writer at a time" module note). This is the invariant, not a fault: the store takes one writer and the lock dies with the process. |
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
| W8 | **A second writer is refused.** `serve` in one PowerShell against a store, `run` in another against the same store | `store <dir> is in use by another process` (`store.rs`). Same invariant, same message: the lock is SQLite's, and the OS releases it when the holder dies. |
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
