# ULPF desktop shell

A Tauri 2 window around the unchanged `ulpf` binary. Double-click the app: it starts
`ulpf serve` on a free localhost port against a directory it owns, shows the served UI, and
takes files or folders dropped on the window. The engine, server and UI are not rebuilt
here; the app bundles `ulpf` as a sidecar.

## Build

```
cargo build --profile dist -p ulpf    # at the repo root: the engine that ships
app/scripts/sidecar.sh                # copies it to app/src-tauri/binaries/ulpf-<host triple>[.exe]
cd app && pnpm install && pnpm tauri build
```
`dist` is the shipped profile (fat LTO, one codegen unit; `[profile.dist]` at the repo root)
and `sidecar.sh` takes `<target>/dist/ulpf[.exe]` first. It falls back to
`<target>/release/` with a warning naming the profile it took, so a `--release` build still
bundles, it just says it is not the shipped one. `<target>` is `CARGO_TARGET_DIR` when that
is set and `<root>/target` otherwise — the same place cargo put the build.
On Windows without Git Bash, `app\scripts\sidecar.ps1` is the same copy in PowerShell.
The bundle lands under `app/src-tauri/target/release/bundle/` (`macos/ULPF.app` and a `.dmg`
on macOS; `nsis/*.exe` and `msi/*.msi` on Windows). `pnpm tauri dev` runs it unbundled.
`.github/workflows/app.yml` does the same on a macOS and a Windows runner for every push
and attaches the installers to a draft release on a `v*` tag.

## Where the data lives

| | macOS | Windows |
|---|---|---|
| Data directory (default) | `~/Library/Application Support/dev.ulpf.desktop` | `%APPDATA%\dev.ulpf.desktop` |
| The chosen override | `<config dir>/data_dir`, one line | same, `%APPDATA%\dev.ulpf.desktop\data_dir` |

Inside: `watch/` (what you dropped; `staging/` holds a copy in flight), `store/` (the raw store), `out.jsonl` (+ its `.pivot`
and `.meta.json`), `pending/` (proposals), `parsers/` and `mappings/` (copied from the
bundled `parsers/*.toml` and `mappings/*.toml` on first run, then yours to edit),
`engine.log` (everything the engine printed this run, truncated at each start) and
`server.url` while the engine is serving. File > Choose data directory… moves the engine to
another directory (the old one is left as it is).

## Menus

File: Add files… (Cmd/Ctrl+O), Add folder…, Open output folder, Open in browser, Intensity ▸
(Low, Balanced, Max — the running one check-marked), Choose data directory…. Tray (menu bar
on macOS, notification area on Windows): Show, Open output folder, Open in browser, Quit —
no Intensity there, it is a File-menu setting only. Closing the window hides it and the
engine keeps ingesting; Quit is what stops the engine. The title reads `ULPF · engine ok · N
events · M pending · Balanced · 4 of 8 cores · index on` once a second (the intensity part is
the next section), or `engine down (exit N)` if the engine stopped.

## Intensity: how hard the engine works

`File > Intensity` is one setting with three choices. Each label carries this machine's own
numbers, so nobody has to know the core count before choosing (an eight-core Mac):

| | Workers | Entity index |
|---|---|---|
| `Low · 2 of 8 cores · entity index off` | one core under four cores, else two | off |
| `Balanced · 4 of 8 cores · entity index on` (default) | half the cores | on |
| `Max · 7 of 8 cores · entity index on` | all but one (the engine's own default) | on |

The two are one control because they are one question: how much of the machine ULPF may
take. Only Low gives the entity index up — it costs an order of magnitude on bulk
throughput (D66) but it is what the Pivot screen reads, so it stays on everywhere else.

The choice becomes `-j N --pivot on|off` on the `ulpf serve` command line and is kept as one
word in `<config dir>/intensity`, beside the `data_dir` override (macOS
`~/Library/Application Support/dev.ulpf.desktop/intensity`, Windows
`%APPDATA%\dev.ulpf.desktop\intensity`); a missing or unreadable file means Balanced. The
engine fixes both its worker count and the index when it starts, so choosing a different
intensity restarts it: the window says `Restarting the engine at Max: 7 of 8 cores, entity
index on`, the child is killed the way Quit kills it (safe under the engine's kill recovery,
D59), a new one starts on a fresh free port against the same store, and a notice says
`Engine ready at Max · 7 of 8 cores · entity index on` when it answers. Nothing in the store
or the output is lost across the restart.

The title carries what the running engine reports, not what the file asks for:
`ULPF · engine ok · 1,250 events · 1 pending · Balanced · 4 of 8 cores · index on`, with the
core count and `index on/off` from `GET /api/status` (`threads`, `pivot_index`). While a
restart is in flight the two disagree and the title says `restarting` instead of quoting a
number nothing is using. The tray menu does not repeat the submenu; the window menu is the
one place the setting lives.

## When it does not start

The window never shows a blank page or an endless spinner: every failure lands on the splash
page as a sentence, the way out, and the file to read.

| What happened | What the window says |
|---|---|
| The engine binary is not beside the app | `ULPF could not start its engine: <error>.` / `The engine ships beside the app as ulpf.exe; reinstalling ULPF replaces it.` (`ulpf` on macOS) |
| The port is taken | `ULPF could not take port 7913 on 127.0.0.1: <error>.` / `Quit whatever is listening there, or start ULPF with ULPF_APP_PORT unset and it will pick a free port.` |
| The engine started and died | `The engine stopped (exit 2). Its last words: <the engine's own last line>` / `The whole of its output is in <data>/engine.log` |
| The store is held by another writer | `The engine's store at <path> is held by ulpf (pid N). Stop it and start again?` / `The whole of its output is in <data>/engine.log`, and one button, **Stop it and start again** |
| The engine never answered | `The engine did not answer within two minutes.` |

The title carries the same state (`ULPF · engine down (port in use)`). ULPF picks a free
port for itself; `ULPF_APP_PORT` pins one, which is how the port case above is provoked.
Captures, taken from the built app on macOS: `docs/screens/app-error-sidecar.png`,
`docs/screens/app-error-port.png`, `docs/screens/app-error-engine.png`,
`docs/screens/app-error-locked.png`.

The locked-store row is the only clickable thing on the splash page: the engine allows one
writer (the store's catalogue is opened in SQLite's exclusive locking mode), so a second
`ulpf serve` on the same directory is refused and the app would otherwise show that refusal
as the generic "the engine stopped". The holder is found by its command line (`ps` on macOS,
`Get-CimInstance Win32_Process` on Windows) because the lock is the process and the file
that records the writer's pid is the locked one; the button stops that pid and starts the
engine again through the ordinary start path. Provoke it by running a second
`ulpf serve --store "~/Library/Application Support/dev.ulpf.desktop/store"` before launching
the app (D93).

## Platform differences (each one is also a comment where it matters)

- Sidecar file name: `binaries/ulpf-<triple>` on macOS, `binaries/ulpf-<triple>.exe` on
  Windows (`scripts/sidecar.sh`, `scripts/sidecar.ps1`); the bundler strips the triple beside
  the executable, so the installed pair is `ULPF.app/Contents/MacOS/{ulpf-app,ulpf}` on macOS
  and `ulpf-app.exe` + `ulpf.exe` in one directory on Windows. The executable keeps the
  crate's name on both (`ULPF` is the bundle and the Start-menu shortcut), so Task Manager
  shows `ulpf-app.exe` for the window and `ulpf.exe` for the engine.
- App data path: see the table (`app.path().app_data_dir()` on both); the `intensity` file
  sits in `app_config_dir` beside `data_dir`, so it follows the same two paths
  (`src/intensity.rs`).
- Installer: `.app` + `.dmg` on macOS; NSIS `.exe` + `.msi` on Windows. Tauri's NSIS
  `installMode` default is `currentUser`: it installs under `%LOCALAPPDATA%` and asks for no
  administrator, which is why the CI smoke job looks for the installed executable there.
- The webview: macOS has WebKit in the OS; Windows needs the WebView2 runtime, and the
  installer carries it (see the Windows section).
- Tray icon: a template image (alpha only) drawn at runtime on macOS so it follows the
  menu bar's style; the coloured app icon on Windows (`src/menu.rs`).
- Stopping the engine: std's `Child::kill`, SIGKILL on macOS and `TerminateProcess` on
  Windows; both are safe under the engine's kill recovery (D59). An intensity change uses
  the same kill, so the restart behaves the same way on both platforms.
- Stopping the engine when the *app* is force-killed: on Windows the sidecar is put in a
  job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`src/job.rs`), whose handle the app
  holds for its whole life, so the kernel terminates the engine when the app's handles close
  — End task on the window, a crash, `Stop-Process -Force`, anything. Windows has no
  process group, so without it a force kill left `ulpf.exe` running with the store's SQLite
  lock and the next launch was refused (D92). macOS needs nothing: the sidecar is a direct
  child and dies with its parent. `app-smoke-windows` asserts it: no `ulpf.exe` within five
  seconds of the window being force-killed, or the job fails. Five seconds is the ceiling,
  not the measurement — the job polls every 500 ms, breaks on the first empty poll and
  prints the elapsed milliseconds, which has been 500 in every run so far.
- The splash page is served from `tauri://localhost` on macOS and `http://tauri.localhost`
  on Windows (`SPLASH` in `src/lib.rs`).
- Positional reads in the store and the UDP receive buffer in the syslog listener have
  `cfg(windows)` shims in the engine; the Windows receive buffer stays at the system default.

## Windows

Nobody in the session that built it has a Windows machine. What follows was read out of the
installers CI produced, not guessed; each line says how it was checked.

**Get the installer.** Not from a CI artifact — those need a GitHub login and expire. The
installers are attached to the pre-release:
`https://github.com/techlogist1/ulpf/releases/tag/<tag>` (the tags are `v0.1.0-rc1`,
`v0.1.0-rc2`, …), assets `ULPF_0.1.0_x64-setup.exe` (NSIS, the one to use) and
`ULPF_0.1.0_x64_en-US.msi`. **`tauri-action` creates that release as a draft, and a draft's
files are invisible to anyone not logged in with write access. Measured, not assumed: an
unauthenticated fetch of the `v0.1.0-rc1` page at 03:52 IST answers 200 and is titled
`Release v0.1.0-rc1` — because the tag exists — and `ULPF_0.1.0_x64-setup.exe` does not
appear on it at all. Publish the release (Releases > the tag > Edit > Publish release, with
"Set as a pre-release" ticked) or the teammate lands on a page with nothing to download.**

**SmartScreen.** The installer is unsigned, so Windows shows a blue dialog,
`Windows protected your PC`. Click **More info**, then **Run anyway**. (There is no
certificate for this hackathon build; a signed one would skip the dialog.)

**The WebView2 runtime.** Tauri 2 renders in WebView2. `bundle.windows.webviewInstallMode`
was absent, which means the default `downloadBootstrapper`: the installer *downloads* the
runtime from Microsoft during installation, so a machine that is offline or behind a proxy
gets an installer that fails or an app whose window never paints. It is now
`{"type": "offlineInstaller"}` in `src-tauri/tauri.conf.json` (JSON has no comments, so the
reason is here): the runtime's own installer rides inside the bundle and the machine needs
no network at install time. **It is not free, and the number is measured, not the docs':**
the build of 06 Sep 03:59 carries `MicrosoftEdgeWebView2RuntimeInstaller.exe`, 258,614,480 B,
and the installer is 267,447,747 B against 5,446,983 B for the same tree without it (Tauri's
page says ~127 MB; today's runtime is twice that). Tauri's prerequisites page says Windows 10
1803 and later already carry the runtime, in which case the embedded installer is skipped —
so on a current machine those 250 MB are insurance, not need. If the download hurts more
than the risk, `embedBootstrapper` costs ~1.8 MB and fetches the runtime at install time
instead; that is one key in the same object. Nothing is fetched at run time either way: the
app talks only to the engine on 127.0.0.1 (the ULPF invariant), and the served UI has no
external reference.

**The sidecar is found where it is installed.** Verified by unpacking the CI artifact of run
33990295166 (`7z x ULPF_0.1.0_x64-setup.exe`): the NSIS payload is `ulpf-app.exe`
(11,983,360 B), `ulpf.exe` (9,519,104 B) and the 12 `parsers/*.toml` + 2 `mappings/*.toml`
resources: 21,706,879 B of payload inside a 5,446,983 B installer; the MSI carries the same
files (`Bin_ulpf.exe`, in a 7,897,088 B installer). So `sidecar("ulpf")` — which resolves to
`ulpf.exe` beside the running executable — finds it in the installed directory, with no dev
path anywhere. The data directory is `app.path().app_data_dir()` = `%APPDATA%\dev.ulpf.desktop`,
and every argument the shell hands the engine is an absolute path built with `PathBuf::join`,
so `%APPDATA%` under a user name with spaces is passed as one argument by the OS, not by a
shell (`Command::args`, no quoting to get wrong). Drops, the native pickers and the tray go
through Tauri's own abstractions (`WindowEvent::DragDrop`, `tauri-plugin-dialog`,
`TrayIconBuilder`), and the staging rename is `<data>/staging` → `<data>/watch` inside the
one data directory, so it is never a cross-volume rename (Windows refuses those). If Windows
does refuse a rename anyway — a virus scanner still holding the copy — the notice says so
(`Not copied: <name> (…)`) instead of swallowing it.

**One dependency the engine still has.** `ulpf.exe` imports `VCRUNTIME140.dll`
(`strings ulpf.exe | grep -i vcruntime` on the artifact; `ulpf-app.exe` does not) — it comes
from the C in `rusqlite`'s bundled SQLite. On a machine without the Microsoft Visual C++
2015-2022 redistributable the engine cannot start. Windows fails that in the loader, before
the program runs, so the window shows `The engine stopped (exit 3221225781)` — that is
0xC0000135, STATUS_DLL_NOT_FOUND — with `It printed nothing.` and an empty `engine.log`; that
exact pair of symptoms is this missing DLL. Either install the
redistributable (`winget install Microsoft.VCRedist.2015+.x64`), or take a build from after
06 Sep 04:45: the workflow's Windows engine step now sets
`RUSTFLAGS=-C target-feature=+crt-static`, which links that runtime into `ulpf.exe` and
leaves nothing to install.

**What CI checks now.** `app-smoke-windows` in `.github/workflows/app.yml` runs after the
bundle job on `windows-latest`. **The installer it exercises is the NSIS
`ULPF_0.1.0_x64-setup.exe`, installed silently with `/S`; the MSI
(`ULPF_0.1.0_x64_en-US.msi`) is built by the same job and attached to the release, but
nothing in CI installs it.** After the install the job finds `ulpf-app.exe` under
`%LOCALAPPDATA%`, proves `ulpf.exe` sits beside it, runs
`ulpf.exe demo --check --repo <the checkout>` on the installed engine and fails on a
non-zero exit (the inputs, the two ports and every title and command in PROGRESS.md's demo
section), launches the app, waits for `%APPDATA%\dev.ulpf.desktop\server.url` and for
`/api/status` to answer JSON, checks that both processes are running, then force-kills the
window and **fails if any `ulpf.exe` is still alive after five seconds of polling** (every
500 ms, breaking on the first empty poll, whose elapsed time it prints: 500 ms in every run
so far) — the job object above is what makes that an assertion rather than a note. If the runner cannot host a
webview the job drives the installed engine instead (`check`, `run samples`, `serve` +
`/api/status`) and prints `SMOKE PATH: app` or `SMOKE PATH: sidecar` so the log says which
one it achieved. The same script runs by hand: `pwsh app\scripts\smoke-windows.ps1
-Installer <the .exe> [-Repo <the checkout>]`. The Windows engine in that workflow is built
with `RUSTFLAGS=-C target-feature=+crt-static`, so the shipped `ulpf.exe` no longer needs
the Visual C++ redistributable.

**Demo notes: never ship a generated parser.** Approving a proposal from the CLI writes the
generated parser (`origin = "inferred"`, priority -1) into the repo's `parsers/`, and a
bundle built after that carries it as a resource — so the app arrives already knowing the
"unseen" format and the inference demo has nothing to propose. Refused in three places:
`sidecar.sh` / `sidecar.ps1` exit 1 naming the file (they are the first command of the
bundle step), the app's first-run copy of the bundled definitions skips any such file and
says how many it skipped in `engine.log`, and this paragraph. `ulpf demo --reset` purges
them from `parsers/`; run it before building a bundle for the demo (D94).

### Building from source on Windows

Checked against Tauri 2's prerequisites page (v2.tauri.app/start/prerequisites), in this order:

1. **Microsoft C++ Build Tools** — the Visual Studio Build Tools installer, workload
   **Desktop development with C++**. (Nothing links without it.)
2. **WebView2 runtime** — already on Windows 10 1803+ and Windows 11; on anything older,
   Microsoft's *Evergreen Bootstrapper*.
3. **Rust** via rustup with the MSVC toolchain: `winget install --id Rustlang.Rustup`, then
   `rustup default stable-msvc` (target `x86_64-pc-windows-msvc`). Restart the terminal.
4. **Node 24** and **pnpm 11** (what CI uses): `winget install OpenJS.NodeJS.LTS`, then
   `npm install -g pnpm@11`.
5. Then, from the repo root:
   ```
   cargo build --release -p ulpf
   powershell -ExecutionPolicy Bypass -File app\scripts\sidecar.ps1
   cd app
   pnpm install
   pnpm tauri build
   ```
   The installers land in `app\src-tauri\target\release\bundle\nsis\` and `…\msi\`.

### The eight steps for the camera (the Windows rig, in this order)

1. **Install.** Open the release page, download `ULPF_0.1.0_x64-setup.exe`, run it.
2. **SmartScreen.** `Windows protected your PC` → **More info** → **Run anyway**. The NSIS
   wizard installs per user; no admin prompt.
3. **Launch** from the Start menu (`ULPF`). The splash reads *Starting the engine*, then the
   window becomes the live feed. `%APPDATA%\dev.ulpf.desktop\server.url` exists, and in
   PowerShell
   `Invoke-RestMethod "$(Get-Content $env:APPDATA\dev.ulpf.desktop\server.url)/api/status"`
   answers JSON.
4. **Drop** `samples\cisco_asa.log` on the window. A notice at the bottom names the file;
   Live (key 1) shows the source and the counters move within seconds.
5. **Drop** `heldout\mikrotik.log`. Within a few seconds the title says `1 pending`.
6. **Review** (key 2): the proposal, its templates and every slot's name with the reason it
   was chosen.
7. **Approve** (key `a`, Enter to confirm): the parser list gains `mikrotik_inferred`
   (`origin approved`), and dropping the same file again parses 250 of 250.
8. **Tray Quit**, then **relaunch**. Close the window first (the engine keeps running, the
   tray icon stays); Quit from the tray; Task Manager shows no `ulpf.exe` and no
   `ulpf-app.exe`. Relaunch: `GET /api/integrity` `records` is not below what it was, and
   `out.jsonl` keeps its earlier lines. (The title's event count is this run's, like the
   engine's counter block; it restarts at 0.)
