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
sources beside parsers, not a strip in the middle. Two breakpoints: under 1650 px the
sources and parsers tables stack (the sources table's ten columns need about 1000 px with a
long proposal id, and the parsers table 560 beside it); under 1100 px every two-column split
stacks. The page's bottom padding clears the fixed status line, so the last row of a table
is never under it.

## Motion

One rule reconciles a screen that is satisfying to watch with the bans above: motion is
allowed exactly where it shows the truth of the system and forbidden as decoration. Events
moving from station to station are data, not ornament; a badge that pops did change; a
verdict that fades in did just arrive. Nothing moves on load, nothing eases on hover, and
under `prefers-reduced-motion` every animation and transition is off (the stylesheet's one
media rule, and `reduced()` in `keys.js` before any script-driven animation is created): the
same numbers stand as a still diagram, a link with a rate shows its dashes standing still and
an idle link is a plain line (`docs/screens/flow-reduced-1280.png`).

### Tokens

| token | value | use |
|---|---|---|
| `--d1` | 120 ms | a value or badge changed |
| `--d2` | 240 ms | a screen arrived, a result replaced a confirmation, a pulse hid or showed, a queue bar grew, a chain mark or the branch lit |
| `--ease` | `cubic-bezier(0.2, 0, 0, 1)` | decelerate: things arrive and settle, nothing bounces |
| `--pulse` | 6 px | one dash of the flow pulse |
| `--pitch` | 32 px | dash to dash; one animation loop travels one pitch |

### The pulse and its rate law

A link between two stations is a 2 px track (`--line-2`) carrying one element, a repeating
gradient of `--pulse` dashes every `--pitch`, moved by a single Web Animations `translate` of
one pitch per second at playbackRate 1 and looped, so the dashes never end and the browser
composites it without script. The count of moving elements on the whole screen is six (five
links and the inference branch), whatever the event rate: a pulse is a count and a speed,
never one element per event. Every 500 ms metrics frame sets each animation's
`playbackRate` from that link's own rate, which changes the speed without a jump:

    px/s = 16 · log10(1 + events/s)        playbackRate = px/s ÷ 32

so 1 event/s crawls at 5 px/s, 100/s runs at 32 px/s, 10,000/s at 64, and 400,000/s at 90;
one order of magnitude more is one step faster, which is how throughput is read, where a
linear law would make a sample file invisible or a bench drop a blur. At rate 0 the dashes
fade out over `--d2` and the track stays: idle is a plain line, not a stopped pulse.

The rate behind each link is that stage's counter difference between the frames of the last
two seconds divided by their interval (`framed` for the headline, `stored` for the first
link, `detected`, `parsed`, `normalized`, `emitted` for the next four, `infer_buffered` for
the branch), computed in the client from every frame it receives. When a frame carries the
server's own window (`rate.framed_per_sec`, `rate.emitted_per_sec`, `rate.over_secs`, and
`queue.depth`, `queue.capacity`; the v4 contract) the headline and the queue use it and the
label under the number says which source is on screen; without it the queue shows only the
run's high-water tick and says the depth is unreported. While the stream is down the
pulses stop and the notice names the last frame's time.

### Where else motion is allowed

| what moved | motion | why |
|---|---|---|
| a screen replaced another (navigation) | `enter`: opacity 0 to 1 and 2 px up over `--d2`, only after the first hash change (`:root[data-nav]`) | the reader asked for a different screen; the first paint is never animated |
| a count badge changed (pending, drift, the tray) | `pop`: scale 1.25 to 1 over `--d1`, by a keyed re-mount, gated so the first frame's counts appear still | the number is different; the eye is told where |
| approve or reject completed | the result notice arrives with `enter` | the confirmation was replaced by what was written |
| a verify finished after the screen opened | the verdict arrives with `enter`; the one on screen at open does not | the state changed while the reader watched |
| a drift state changed after the screen opened | the state tag pops | the source tripped, proposed or cleared |
| the queue bar, the chain's newest mark, the branch's link | width, colour and opacity over `--d2` | a depth, a record count or a buffer the frame reported |
| replay and verify in flight | the meter and the busy sweep (unchanged) | work in progress |

Forbidden and absent: hover transitions, a selection moving (h / l on Flow snaps like j / k on every
list: the reader's own action reports nothing new), a hero animation on load, per-event particles,
pulses on the vertical tray link (a proposal waiting is a state, so that link is lit `--pend`,
not moving), any easing on data that did not change.

### Station to screen

Flow is the front door (`#/`, `#/flow`, key `0`; Esc from any top-level screen returns to it);
the seven windows are one step behind it. Each station opens the screen that holds that
stage's evidence:

| station | key | opens | because |
|---|---|---|---|
| ingest | `i` | Live | the sources, the tail and every counter are what came in and from where |
| preserve | `s` | Integrity | the store is the proof: chain head, verify, attestation |
| detect | `d` | Drift | detection is per source; Drift is where a source's parser stops claiming it |
| the branch and the tray | `r` | Review | unknown lines become a proposal there, and nothing is parsed until a human approves |
| parse | `p` | Traceback of the newest record | the parsed fields lit in the bytes is the one picture of parsing |
| normalize | `n` | Pivot | the entity index is built from normalized paths; one entity across every device is what normalization buys |
| emit | `e` | Replay | emit writes version 1; Replay writes the next version and diffs them |

`h` / `l` or the arrows move a selection along the line and Enter opens it; each station is
also a link, so a click does the same.

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
| top bar | `.top` | brand, Flow and the seven screens with their digit and a count badge (pending, drift), theme and keys buttons; sticky |
| flow | `.flow` + `.line` + `.station` + `.link` + `.under` | the front door: six stations on an eleven-column grid (station, link, station, ...), each with its name and key, its counter at `--t3`, its loss in `--warn`; a link is the 2 px track with one pulse element; the sub-row under a link or station is placed by grid column: `.queue` (the bar with the high-water tick), `.chain` (one mark per attestation checkpoint, the newest lit `--ok` while records arrive), `.branch` (the inference node and the tray on a `--pend` rule when lines are buffered or a proposal waits) |
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
| legend | `.legend` | the tint of every source key; two rows at most (`.clip`), a button shows all N when a record has more |
| provenance lists | `.prov` | parser fields and normalized paths, each row lighting its bytes on hover, pinning on click |
| trail | `.trail` | the breadcrumb of pivots or the review path |
| entity | `.entity` | the pivoted value at `--t3` with its facts |
| lanes | `.lanes` + `.lane` + `.axis` | one lane per device over the loaded window, ticks merged when closer than a pixel, a five-tick time axis |
| related | `.related` | the ten most frequent co-occurring values per kind; the bar is the value's share of the events the list was computed over, so equal counts are equal bars; every value is a link that pivots |
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
| anywhere | `0` | Flow, the front door |
| anywhere | `1` to `7` | Live, Review, Traceback, Pivot, Replay, Drift, Integrity |
| anywhere | `?` | the key map; Esc closes |
| anywhere | `t` | light or dark |
| anywhere | `/` | the search or filter box of this screen; Esc leaves it |
| anywhere | Esc | close the map, leave a box, release a pinned range, go from a detail to its list, or from a top-level screen back to Flow |
| Flow | `i` `s` `d` `p` `n` `e` | open the screen behind ingest, preserve, detect, parse, normalize, emit |
| Flow | `r` | the tray: Review |
| Flow | `h` / `l`, arrows | move the selection along the line; Enter opens it |
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
