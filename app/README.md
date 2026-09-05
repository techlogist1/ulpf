# ULPF desktop shell

A Tauri 2 window around the unchanged `ulpf` binary. Double-click the app: it starts
`ulpf serve` on a free localhost port against a directory it owns, shows the served UI, and
takes files or folders dropped on the window. The engine, server and UI are not rebuilt
here; the app bundles `ulpf` as a sidecar.

## Build

```
cargo build --release -p ulpf         # at the repo root: the engine
app/scripts/sidecar.sh                # copies it to app/src-tauri/binaries/ulpf-<host triple>[.exe]
cd app && pnpm install && pnpm tauri build
```
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
bundled `parsers/*.toml` and `mappings/*.toml` on first run, then yours to edit), and
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

## Platform differences (each one is also a comment where it matters)

- Sidecar file name: `binaries/ulpf-<triple>` on macOS, `binaries/ulpf-<triple>.exe` on
  Windows (`scripts/sidecar.sh`); the bundler strips the triple beside the executable.
- App data path: see the table (`app.path().app_data_dir()` on both); the `intensity` file
  sits in `app_config_dir` beside `data_dir`, so it follows the same two paths
  (`src/intensity.rs`).
- Installer: `.app` + `.dmg` on macOS; NSIS `.exe` + `.msi` on Windows.
- Tray icon: a template image (alpha only) drawn at runtime on macOS so it follows the
  menu bar's style; the coloured app icon on Windows (`src/menu.rs`).
- Stopping the engine: std's `Child::kill`, SIGKILL on macOS and `TerminateProcess` on
  Windows; both are safe under the engine's kill recovery (D59). An intensity change uses
  the same kill, so the restart behaves the same way on both platforms.
- The splash page is served from `tauri://localhost` on macOS and `http://tauri.localhost`
  on Windows (`SPLASH` in `src/lib.rs`).
- Positional reads in the store and the UDP receive buffer in the syslog listener have
  `cfg(windows)` shims in the engine; the Windows receive buffer stays at the system default.

## The Windows build has not been run on a Windows machine

CI produced it; nobody has launched it yet. On the Windows rig, run these five checks:

1. Launch the installed app: the window shows the live feed; `%APPDATA%\dev.ulpf.desktop\server.url`
   exists and `curl` of `<that url>/api/status` answers JSON.
2. Drop `samples/cisco_asa.log` on the window (or File > Add files…): a notice names the
   file and the live feed shows its events within seconds.
3. Drop `heldout/mikrotik.log`: Review (key 2) shows a proposal; Approve it and the parser
   list gains it.
4. Quit from the tray (and once after closing the window first): Task Manager shows no
   `ulpf.exe`.
5. Relaunch: `GET /api/integrity` `records` is not below what it was before Quit, `out.jsonl`
   keeps its earlier lines, and a new drop appends to it. (The title's event count is this
   run's, like the engine's counter block; it restarts at 0.)
