# ULPF web UI

Svelte 5 + Vite single-page app: Live counters, Review of pending parser proposals,
Traceback of one raw record. Talks to the same-origin API described in `docs/api.md`.

## Build

    pnpm install && pnpm build

Produces exactly three files, fixed names, no hashes:

    ui/dist/index.html   ui/dist/app.js   ui/dist/app.css

The Rust binary embeds them with `include_str!` and serves `/`, `/app.js`, `/app.css`.

## Restyle without touching Rust

    ulpf serve --ui-dir ui/dist

serves the three files from disk on every request, so edit, rebuild, reload.

All colours, fonts and spacing steps are CSS custom properties in the block at the
top of `src/app.css` (`:root { ... }`). Nothing below that block hard-codes a value.
Components carry no `<style>` blocks; every class lives in that one stylesheet.

## Develop

    pnpm dev

Vite proxies `/api` to `http://127.0.0.1:7878` (a running `ulpf serve`).

## Routes

`#/live`, `#/review`, `#/review/<id>`, `#/trace/<raw_id>`. Clicking a tail row opens
that record in Traceback.
