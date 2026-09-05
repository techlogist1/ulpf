# ULPF console: the design system

The console is read by a security engineer at 3am who has to trust what is on the screen and
act in two keystrokes. Everything below serves density, legibility and honesty; anything that
only decorates was removed. This document is the reason the next change makes it better rather
than worse: every value on screen comes from a token named here, every component is listed
here, every key is in the map here.

The whole system lives in two files: `ui/src/app.css` (tokens at the top, then every class; no
component carries a `<style>` block) and `ui/src/keys.js` (one window listener, one handler per
screen). `ui/README.md` says how to build and where the routes are.

## Principles

- Less but better. An element earns its pixels by carrying a fact or an action. No hero copy,
  no decorative icons, no cards for the sake of grouping, no emoji.
- Data-ink. Counters, the tail, the byte ruler, the timeline and the diff are the content;
  chrome is one bar of screens on top and one status line at the bottom.
- Alignment does the work borders would. Tables and virtual lists share one row height
  (`--row`, 22 px), one header style and one hairline; numbers are right-aligned in a
  monospaced face with tabular figures so columns line up and counters never jitter.
- Colour means state and nothing else. Four state hues (proved, look, broken, waiting for a
  human); neutrals carry everything else, links and buttons included. The one exception is
  the eight provenance tints in the traceback, where hue is an index (which field owns which
  bytes); they are never used for anything else.
- Motion only where it reports a state change: the loading spinner and the busy meter of a
  replay or verify in flight. Nothing animates on load, nothing eases on hover.
  `prefers-reduced-motion` turns both off.
- Keyboard first: every primary action has a key, focus is always visible, the letter that
  opens a confirmation is never the key that confirms it, and `?` shows the map.
- Dark by default (an operations screen), light through the same tokens (`t` flips, the
  choice is kept per browser in `localStorage`), never a second stylesheet.
- Nothing is fetched at runtime: fonts, script and stylesheet are three files the binary
  embeds; the page requests `/app.js`, `/app.css` and `/api/*` on the same origin and
  nothing else (verified with `performance.getEntriesByType('resource')` in Chrome and with
  `scripts/isolation.sh` on the process).

## Typefaces

| role | face | weights | file | bytes |
|---|---|---|---|---|
| text | IBM Plex Sans | 400, 600 | `ui/fonts/IBMPlexSans-{Regular,SemiBold}-Latin1.woff2` | 20,984 + 22,260 |
| data | IBM Plex Mono | 400, 500 | `ui/fonts/IBMPlexMono-{Regular,Medium}-Latin1.woff2` | 17,544 + 17,868 |

78,656 bytes of font in total, inlined into `ui/dist/app.css` as `data:font/woff2;base64` URIs
(`build.assetsInlineLimit: 1e9` in `ui/vite.config.js`; the default 4 KiB limit would have
emitted them as hashed files the server never serves). Licence: SIL Open Font License 1.1,
the text is `ui/fonts/LICENSE-OFL.txt`. Source: IBM's own Latin-1 split subsets from the
`IBM/plex` GitHub releases, `fonts/split/woff2/` inside `ibm-plex-sans.zip` of
`@ibm/plex-sans@1.1.0` and `ibm-plex-mono.zip` of `@ibm/plex-mono@2.5.0`, fetched at build
time with curl, never at runtime. The committed files are byte-identical to the release
(SHA-256 compared on 2026-09-05: Sans Regular `b5ad7bd3…51f6b1`, Sans SemiBold
`fff0ab3a…de8ad5`, Mono Regular `e8993d94…c18e56`, Mono Medium `41201b65…6365d`). The
subset covers ASCII and Latin-1 supplement, which is every character the console prints
(raw bytes outside it are shown as `\xNN`).

Why this pair: Plex Sans and Plex Mono share one skeleton, so a mono value beside a sans
label reads as one voice rather than two fonts; both carry tabular figures (measured in
Chrome: `1111` and `0000` are the same width in both faces at every size used); the
Mono has a slashed zero and a distinct `l`/`1`/`I`, which is what a hex dump and an
address column need; and both are OFL-1.1, so they can live in the binary. Inter and
JetBrains Mono were the other candidate; they are two designs rather than one family and
Inter's default figures are proportional, so every counter would need an explicit feature.

The fallback stacks (`system-ui`, `ui-monospace`) only ever apply while the base64 face
decodes, which is under one frame from an embedded file; `font-display: block` stops a flash
of the fallback.

## Tokens

All tokens are custom properties on `:root` in `ui/src/app.css`; the light theme redefines the
colour tokens under `:root[data-theme="light"]` and nothing else.

### Surfaces and lines

| token | dark | light | use |
|---|---|---|---|
| `--bg` | `#141619` | `#f4f5f7` | the ground; the page and inputs |
| `--bg-1` | `#1b1e22` | `#ffffff` | raised: top bar, status line, panels, verdicts, the byte ruler |
| `--bg-2` | `#22262b` | `#e9ebee` | hover, diff hunk headers |
| `--sel` | `#2a3038` | `#dde2e8` | the selected row |
| `--line` | `#2c3137` | `#d9dde2` | hairline between rows and sections |
| `--line-2` | `#3f474f` | `#aeb6bf` | a stronger rule: table heads, the axis, key caps |
| `--line-3` | `#5f6873` | `#7d8690` | control borders (3:1 against the ground, the non-text minimum) |
| `--shade` | `rgba(0,0,0,.6)` | `rgba(20,22,25,.45)` | behind the keyboard map |

### Ink

| token | dark | light | use |
|---|---|---|---|
| `--fg` | `#e4e7ea` | `#181b1e` | primary text, numbers, the primary button's fill |
| `--fg-1` | `#b1b8c0` | `#40474f` | secondary text: values in the raw record, screen names |
| `--fg-2` | `#8a929b` | `#5f676f` | muted: labels, notes, column heads; still 4.5:1 and above |

### State

| token | dark | light | wash (dark / light) | meaning |
|---|---|---|---|---|
| `--ok` | `#63c383` | `#1e7a3e` | `#16281c` / `#e2f3e7` | proved or streaming: digest matches, chain follows, verify clean, approved parser, the stream dot |
| `--warn` | `#e2a83f` | `#8a5a00` | `#2b2210` / `#f6ead2` | look at this: a loss in the funnel, a denied action, drift tripped, a policy applied to a timestamp, backpressure |
| `--bad` | `#f07566` | `#b3261e` | `#2f1a17` / `#f8dedb` | broken: digest mismatch, chain broken, a failed request, a problem in a definition, the reject action |
| `--pend` | `#77b0ee` | `#1a5fb4` | `#172433` / `#dde9f8` | waiting for a human: the pending count, a proposal, a drift update proposed |

A state colour appears as text (`.is-ok` etc.), as an outlined mark (`.tag.ok`), as a
left rule on a notice, alert or verdict, and as the fill of a count badge. It never
appears as a large fill and never as decoration; a row is not tinted because it is
interesting, a value is tinted because it is in one of the four states.

### Provenance tints

`--p0` to `--p7`: amber, cyan, violet, green, coral, blue, pink, olive (light theme: the
same hues darkened to 4.6:1 and better on the ground). Assigned in the traceback to source
keys in parser order and to devices in the pivot in event order; `--tint` (20% dark, 16%
light) is how much of the tint sits behind lit bytes, and a hot range inverts to solid
tint with ground-coloured ink. Eight is enough because ranges are also underlined and
labelled; the ninth key wraps and the label disambiguates.

### Type scale

| token | px | use |
|---|---|---|
| `--t-1` | 11 | column heads, key caps, the status line, byte offsets |
| `--t0` | 12 | table cells, list rows, notes, controls, the raw record |
| `--t1` | 13 | body text, section titles, the base size |
| `--t2` | 15 | funnel numbers, the replay's why lines |
| `--t3` | 20 | the entity being pivoted on |
| `--t4` | 28 | the two rates on Live |

Line height 1.5 everywhere except the two display sizes (1 and 1.2). Weights: 400 and 600
in the sans, 400 and 500 in the mono. No italics, no caps, no letterspacing beyond the
brand mark. Line length is bounded by columns, and prose blocks (`.empty`, notes) by
`max-width: 70ch`.

### Spacing scale

`--s1` 2, `--s2` 4, `--s3` 8, `--s4` 12, `--s5` 16, `--s6` 24, `--s7` 32, `--s8` 48 px.
Rows are `--row` 22 px, the top bar `--top` 36 px, the status line `--foot` 22 px. The page
gutter is `--s5` and the content stops at `--page-max` 2400 px so a 27-inch monitor at
2560 px gets a wider tail, a wider byte ruler (256 bytes per row instead of 159) and
sources beside parsers, not a strip in the middle. Two breakpoints: under 1400 px the
sources and parsers tables stack (the sources table needs 800 px of columns); under
1100 px every two-column split stacks.

## Contrast

WCAG 2 contrast ratios computed from the token values (the script that produced this
table is in the commit that added it; rerun it after changing any colour). Text needs 4.5:1,
non-text (borders, the count badge outline) 3:1. Every pair passes in both themes; the
lowest text ratio is 4.64 (`--p1` on the light ground).

| pair | use | dark | light |
|---|---|---|---|
| `--fg` on `--bg` | body text | 14.60 | 15.85 |
| `--fg` on `--bg-1` | text on a panel | 13.48 | 17.29 |
| `--fg-1` on `--bg` | secondary text | 9.05 | 8.62 |
| `--fg-1` on `--bg-1` | secondary on a panel | 8.35 | 9.41 |
| `--fg-2` on `--bg` | muted text, labels | 5.75 | 5.27 |
| `--fg-2` on `--bg-1` | muted on a panel | 5.31 | 5.74 |
| `--fg-2` on `--bg-2` | muted on a hovered row | 4.83 | 4.81 |
| `--fg` on `--sel` | text on the selected row | 10.72 | 13.27 |
| `--ok` on `--bg` | ok text | 8.34 | 4.93 |
| `--warn` on `--bg` | warn text | 8.55 | 5.43 |
| `--bad` on `--bg` | bad text | 6.42 | 5.99 |
| `--pend` on `--bg` | pending text | 7.97 | 5.76 |
| `--ok` on `--ok-bg` | ok text on its wash | 7.14 | 4.66 |
| `--warn` on `--warn-bg` | warn text on its wash | 7.40 | 4.97 |
| `--bad` on `--bad-bg` | bad text on its wash | 5.81 | 5.12 |
| `--pend` on `--pend-bg` | pending text on its wash | 6.91 | 5.12 |
| `--bg` on `--pend` | count badge (ink on pending) | 7.97 | 5.76 |
| `--bg` on `--warn` | count badge (ink on warn) | 8.55 | 5.43 |
| `--bg` on `--fg` | primary button ink | 14.60 | 15.85 |
| `--line-3` on `--bg` | control border (non-text, 3:1) | 3.21 | 3.39 |
| `--p0` on `--bg` | provenance tint 0 as text | 8.55 | 5.43 |
| `--p1` on `--bg` | provenance tint 1 | 9.03 | 4.64 |
| `--p2` on `--bg` | provenance tint 2 | 7.88 | 6.60 |
| `--p3` on `--bg` | provenance tint 3 | 8.34 | 4.93 |
| `--p4` on `--bg` | provenance tint 4 | 7.97 | 5.02 |
| `--p5` on `--bg` | provenance tint 5 | 7.97 | 5.76 |
| `--p6` on `--bg` | provenance tint 6 | 7.71 | 5.79 |
| `--p7` on `--bg` | provenance tint 7 | 9.98 | 5.28 |
| `--bg` on `--p0` | bytes lit hot: ink on tint 0 | 8.55 | 5.43 |
| `--bg` on `--p1` | ink on tint 1 | 9.03 | 4.64 |
| `--bg` on `--p2` | ink on tint 2 | 7.88 | 6.60 |
| `--bg` on `--p3` | ink on tint 3 | 8.34 | 4.93 |
| `--bg` on `--p4` | ink on tint 4 | 7.97 | 5.02 |
| `--bg` on `--p5` | ink on tint 5 | 7.97 | 5.76 |
| `--bg` on `--p6` | ink on tint 6 | 7.71 | 5.79 |
| `--bg` on `--p7` | ink on tint 7 | 9.98 | 5.28 |

## Components

Every class is in `ui/src/app.css` under a section comment; no component adds its own.

| component | class | what it is |
|---|---|---|
| top bar | `.top` | brand, the seven screens with their digit and a count badge (pending, drift), theme and keys buttons; sticky |
| status line | `.foot` | stream state with a dot, listen address, schema, syslog sockets, uptime, clients, frames skipped, events skipped; fixed at the bottom |
| section head | `.head` | title, a note in `--fg-2`, controls pushed right; `.quiet` drops the rule |
| facts | `.facts` | label/value pairs in one wrapping line (record header, entity header, evidence params) |
| counters | `.counters` + `.kvs` + `.kv` | grouped counters, label left and number right, dotted rule between; `.on`/`.bad`/`.ok`/`.pend` colour the number |
| table | `.tbl` | short lists that need no virtualisation; sticky head, `.num` right-aligned mono, `.click` rows, `.sel` with an inset bar |
| virtual list | `VList.svelte` + `.vh`/`.vr`/`.vl` | fixed-height rows, only the visible window in the DOM; the tail, the pivot timeline, the diff entries, the byte ruler, the field lists |
| verdict | `.verdict` | a plain-words sentence with a state rule, then the values it rests on (digests, chain values, the timestamp bytes) |
| notice | `.notice` | the result of an action or an error from the server, with reason and any problems list |
| alert | `.alert` | one line on Live per thing needing attention, with the screen it links to |
| empty | `.empty` | a dashed box with a bold sentence saying what is missing and what fills it |
| loading | `.loading` | a sentence with a small spinner, naming what is being read |
| tag | `.tag` | an outlined mono mark: a state, a kind, `canonical`, `edited` |
| button | `.btn` (`.primary`, `.danger`, `.on`) | neutral; the primary is inverted ink; danger is red text; every button shows its key |
| segmented | `.kinds` | one-of-n choice: entity kind, diff entry kind |
| inputs | `input[type=search]`, `textarea.editor` | search boxes are mono; the definition editor is mono with a 56vh minimum |
| confirmation | `Confirm.svelte` + `.confirm` | see below |
| keyboard map | `.overlay` + `.keymap` | the full key map in two columns; `?` opens, Esc or a click outside closes |
| funnel | `.funnel` + `.fst` | the six pipeline stages as numbers with a proportional track and the loss between stages |
| queue | `.queue` | high-water against capacity with the producer's block count |
| byte ruler | `.bytes` | offset column, text (n bytes per row, never splitting a UTF-8 sequence) or hex (16 per row with ASCII); owned ranges lit by tint, control bytes as `\xNN` |
| legend | `.legend` | the tint of every source key |
| provenance lists | `.prov` | parser fields and normalized paths, each row lighting its bytes on hover, pinning on click |
| trail | `.trail` | the breadcrumb of pivots or the review path |
| entity | `.entity` | the pivoted value at `--t3` with its facts |
| lanes | `.lanes` + `.lane` + `.axis` | one lane per device over the loaded window, ticks merged when closer than a pixel, a five-tick time axis |
| related | `.related` | the ten most frequent co-occurring values per kind with a share bar; every value is a link that pivots |
| template | `.tpl` | one inferred template: id, support, verified, members, the pattern, the slot table (name, kind, why this name, after, distinct, examples), example lines, history |
| diff | `.diff` | a unified diff, added and removed lines washed |
| why | `.why` | the replay report's explanation lines set at `--t2`, above the counters they explain |
| meter | `.meter` | progress of a replay; `.busy` sweeps while a verify has no progress to report |
| proof | `.proof` | what an approve or reject wrote: path, parsers loaded, re-detected count |

### The confirmation

Approve, reject, replay and verify write to disk or start work; each goes through
`Confirm.svelte`. The letter that opens it (`a`, `x`, `v`) can never confirm it: Enter
confirms, Esc cancels, Tab moves between the two buttons, and focus lands on the confirming
button when the box opens so the flow is two keys and nothing else on the screen reacts
to either. The box states exactly what will be written (the file path, the version it
replaces, what is re-detected) and, for reject, is marked `danger` in red. A mouse user
sees the same box; there is no single-click path to any of the four.

## Keyboard map

| where | key | does |
|---|---|---|
| anywhere | `1` to `7` | Live, Review, Traceback, Pivot, Replay, Drift, Integrity |
| anywhere | `?` | the key map; Esc closes |
| anywhere | `t` | light or dark |
| anywhere | `/` | the search or filter box of this screen; Esc leaves it |
| anywhere | Esc | close the map, leave a box, release a pinned range, or go from a detail to its list |
| any list | `j` / `k`, arrows | move down, up; the selected row is kept in view |
| any list | `g` / `G` | first, last |
| any list | Enter | open the selected row |
| Live | space | hold the tail still (arrivals are counted, not stacked) and release it; moving in the tail holds it |
| Live | Enter | trace the selected event |
| Traceback | `j` / `k` | walk the normalized fields, lighting each field's bytes |
| Traceback | Enter, click | keep the selected range lit; Esc releases |
| Traceback | `h` | hex or text |
| Review | `s` | save the definition |
| Review | `a` | approve: opens the confirmation |
| Review | `x` | reject: opens the confirmation |
| Review | `d` | show or hide the diff against the parser an update replaces |
| Review | `m` | merge the picked templates into one and re-emit |
| Review | `r` | regenerate the definition from the kept templates |
| Pivot | Backspace | one step back along the trail |
| Pivot | `m` | load older events |
| Replay | `v` | start a replay: opens the confirmation |
| Replay | `m` | load more diff entries |
| Integrity | `v` | start a verify: opens the confirmation |
| confirmation | Enter / Esc / Tab | confirm, cancel, move between the two |

Focus is always visible: a 2 px `--fg` outline on any focused element. Every row a key can
select is also a click target, and every action the keys reach has a button showing its key.

## Under load

- The stream is applied on `requestAnimationFrame`: a frame that arrives before the previous
  one painted replaces it and is counted (`frames skipped` in the status line); nothing
  queues, so a full-rate engine cannot outrun the browser (`ui/src/state.svelte.js`).
- A tail row is flattened to seven strings the moment it arrives, never the nested event,
  so reactivity proxies nothing per field.
- Long lists are windows: the tail (500 rows), the pivot timeline, the diff entries, the
  entity search, the parser and normalized field lists and the byte ruler all render only
  the rows in view plus a margin (`VList.svelte`). Measured on the 4 MB single-line record:
  the ruler is on screen 1.3 s after navigation with 24 rows in the DOM, two frames take
  8 ms, hex mode holds 30 rows over a 5.7 million pixel ruler and scrolls to any offset.
- Provenance is resolved by byte range, not field name, so two fields sharing a key stay
  distinct, and a field that strictly contains other reported fields yields to its parts.

## Empty, loading and error

Every screen has all three, designed as sentences: a loading line names what is being read
("reading record 5 through the writer's lock"); an empty box says what is missing and what
fills it ("No events yet. The tail fills the moment the engine emits: drop a file into a
watched directory or send syslog to the listener in the status line"); an error notice
carries the server's `error` and `reason` and, where the server says more, the way out (a
trace of an id the store never issued shows the store's length and the valid range). A
fresh server with zero events looks intentional on all seven screens.

## Changing it

- A new colour is a new token with a row in the contrast table, or it does not exist.
- A new component is a class in `app.css` and a row in the inventory; it uses the row
  height, the type scale and the spacing scale, or it will look wrong beside everything else.
- A new action gets a key, shows it on its button, appears in the map above and in the
  overlay, and, if it writes, goes through `Confirm.svelte`.
- Rebuild with `pnpm build` in `ui/`, check `ui/dist` still holds exactly three files and
  that `grep -c 'url(data:font/woff2' ui/dist/app.css` says 1 (one rule per face is
  minified into one declaration block), then recapture with `ui/capture.mjs`.
