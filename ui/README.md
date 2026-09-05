# ULPF web UI

Svelte 5 + Vite single-page app: live counters, review of pending parser proposals,
traceback of one raw record to its bytes, entity pivot, replay diffs, drift alerts and
the integrity chain. Talks to the same-origin API described in `docs/api.md`. Every
control is backed by a documented route; a field the server does not send is reported
as a gap, never invented.

## Build

    pnpm install && pnpm build

Produces exactly three files, fixed names, no hashes:

    ui/dist/index.html   ui/dist/app.js   ui/dist/app.css

The Rust binary embeds them with `include_str!` and serves `/`, `/app.js`, `/app.css`.

## Restyle without touching Rust

    ulpf serve --ui-dir ui/dist

serves the three files from disk on every request, so edit, rebuild, reload.

All colours, fonts and spacing steps are CSS custom properties in the block at the
top of `src/app.css` (`:root { ... }`; the light theme redefines the colour tokens under
`:root[data-theme="light"]`). Nothing below that block hard-codes a value. Components
carry no `<style>` blocks; every class lives in that one stylesheet. `docs/design.md` is
the system: tokens, scales, colour semantics with the contrast table, the component
inventory and the keyboard map.

## Fonts

IBM Plex Sans (400, 600) and IBM Plex Mono (400, 500), Latin-1 subsets from IBM's own
release, OFL-1.1 (`fonts/LICENSE-OFL.txt`), 78,656 bytes in total. `vite.config.js` sets
`assetsInlineLimit` high enough that they are inlined into `dist/app.css` as `data:` URIs,
so the binary carries them and the page never fetches a font. After a build:

    grep -c 'url(data:font/woff2' dist/app.css     # 1
    ls dist                                        # exactly index.html app.js app.css

## Captures

    node capture.mjs --base http://127.0.0.1:7881 --out ../docs/screens [--big <raw id>] [--approve <pending id>]

shoots every screen at 1280x800 and 2560x1440 against a populated `ulpf serve`, plus the
stateful ones (hover, hex, the shortcut overlay, empty and error states, light theme, the
keyboard-only approve flow, one capture per key) and writes `docs/screens/README.md`, one
line per file. Needs Chrome at its usual path and `puppeteer-core` (a dev dependency).

## Develop

    pnpm dev

Vite proxies `/api` to `http://127.0.0.1:7878` (a running `ulpf serve`).

## Routes

| hash | screen |
|---|---|
| `#/live` | counters, sources, parsers, the tail; a tail row opens Traceback |
| `#/review`, `#/review/<id>` | pending proposals; the TOML editor, evidence, diff, approve |
| `#/trace/<raw_id>` | the record's bytes with every parsed field lit, digests and chain |
| `#/pivot`, `#/pivot/<kind>/<value>` | entity search, timeline across devices, related entities |
| `#/replay` | output versions, the replay report and its diff entries |
| `#/drift` | sources whose established parser started missing |
| `#/integrity` | store chain, verify, attestation |

Keys: digits 1-7 pick a screen, `?` shows the full map, `/` is the search box on the
screen you are on, `j`/`k` walk any list, Enter opens, Esc goes back.

## Under load

The stream is applied on `requestAnimationFrame`: a frame that arrives before the
previous one painted replaces it and is counted as *frames skipped* in the status bar,
so a full-rate engine cannot outrun the browser. The tail keeps at most 500 rows in
the DOM; *events skipped* counts what the server's ring evicted before this client
read it.
