# Screen captures

The 44 `*-1280.png` / `*-2560.png` captures in the first table without a tool-driven marker were taken headlessly by `ui/capture.mjs` against a populated `ulpf serve` (0.1.0 at http://127.0.0.1:7881); the `tool-*` rows were driven through the real Chrome (its own server on 127.0.0.1:7885) and the `app-*` rows are `screencapture` of the built ULPF.app on its own sidecar port. One line per file.

| file | screen | width | what it shows |
|---|---|---|---|
| live-1280.png | live | 1280 | live feed: rates, funnel, queue, tail, sources, parsers, every engine counter |
| review-list-1280.png | review | 1280 | review: the pending proposals, kind, lines, templates, unmatched, problems |
| review-detail-1280.png | review | 1280 | review: definition editor, actions, evidence with templates, slot names and the reason for each |
| review-update-1280.png | review | 1280 | review: a drift update proposal, the unified diff against the parser on disk above the definition and the evidence |
| trace-1280.png | trace | 1280 | traceback: verdicts, the byte ruler with every field lit, parser fields and normalized provenance |
| trace-hover-1280.png | trace | 1280 | traceback: j walks the normalized fields, the selected field is lit in the bytes and the parser fields |
| trace-hex-1280.png | trace | 1280 | traceback: the same record in hex, sixteen bytes per row, the lit field carried into the hex and ascii columns |
| trace-big-1280.png | trace | 1280 | traceback of the 4 MB single-line record: the byte ruler virtualises the text, the page stays responsive |
| pivot-search-1280.png | pivot | 1280 | pivot: kind selector and the entities with the most events |
| pivot-1280.png | pivot | 1280 | pivot of the busiest entity (dst_port 443): device lanes on a time axis, the timeline, the related entities |
| replay-1280.png | replay | 1280 | replay: why v2 differs, counters, parser changes, by field, versions, the diff entries |
| drift-1280.png | drift | 1280 | drift: every established source with its window rate against the baseline; tripped and proposed first |
| integrity-1280.png | integrity | 1280 | integrity: verdict of the last verify, records, store id, genesis and chain head |
| live-2560.png | live | 2560 | live feed: rates, funnel, queue, tail, sources, parsers, every engine counter |
| review-list-2560.png | review | 2560 | review: the pending proposals, kind, lines, templates, unmatched, problems |
| review-detail-2560.png | review | 2560 | review: definition editor, actions, evidence with templates, slot names and the reason for each |
| review-update-2560.png | review | 2560 | review: a drift update proposal, the unified diff against the parser on disk above the definition and the evidence |
| trace-2560.png | trace | 2560 | traceback: verdicts, the byte ruler with every field lit, parser fields and normalized provenance |
| trace-hover-2560.png | trace | 2560 | traceback: j walks the normalized fields, the selected field is lit in the bytes and the parser fields |
| trace-hex-2560.png | trace | 2560 | traceback: the same record in hex, sixteen bytes per row, the lit field carried into the hex and ascii columns |
| trace-big-2560.png | trace | 2560 | traceback of the 4 MB single-line record: the byte ruler virtualises the text, the page stays responsive |
| pivot-search-2560.png | pivot | 2560 | pivot: kind selector and the entities with the most events |
| pivot-2560.png | pivot | 2560 | pivot of the busiest entity (dst_port 443): device lanes on a time axis, the timeline, the related entities |
| replay-2560.png | replay | 2560 | replay: why v2 differs, counters, parser changes, by field, versions, the diff entries |
| drift-2560.png | drift | 2560 | drift: every established source with its window rate against the baseline; tripped and proposed first |
| integrity-2560.png | integrity | 2560 | integrity: verdict of the last verify, records, store id, genesis and chain head |
| keys-1280.png | keys | 1280 | the shortcut overlay (?) |
| empty-trace-1280.png | empty | 1280 | empty state: traceback with no record chosen |
| error-trace-1280.png | error | 1280 | error state: a trace of raw id 503741, which the store never issued |
| light-trace-1280.png | light | 1280 | the same traceback under the light theme (t) |
| light-live-1280.png | light | 1280 | the live feed under the light theme (t) |
| empty-pivot-value-1280.png | empty | 1280 | empty state: a pivot on a value no event carries |
| reject-confirm-1280.png | reject | 1280 | review: x opens the reject confirmation, marked as the destructive one; Enter confirms, Esc cancels |
| approve-1-1280.png | review | 1280 | keyboard approve 1: the digit 2 opens Review from anywhere |
| approve-2-1280.png | review | 1280 | keyboard approve 2: j selects the proposal (bench_slice) |
| approve-3-1280.png | review | 1280 | keyboard approve 3: Enter opens it; the definition, the actions and the evidence |
| approve-4-1280.png | review | 1280 | keyboard approve 4: a opens the confirmation; focus is on Approve, Esc would cancel |
| approve-5-1280.png | review | 1280 | keyboard approve 5: Enter confirms; the result names the file, the parsers loaded and how many buffered lines the new parser now claims |
| empty-live-1280.png | live | 1280 | empty state: a fresh server with zero events: rates, funnel, queue and the tail say what will fill them |
| empty-review-1280.png | review | 1280 | empty state: nothing to review: what makes a proposal appear |
| empty-pivot-1280.png | pivot | 1280 | empty state: no entities indexed yet |
| empty-replay-1280.png | replay | 1280 | empty state: no output versions yet |
| empty-drift-1280.png | drift | 1280 | empty state: no source established yet: the thresholds in words |
| empty-integrity-1280.png | integrity | 1280 | empty state: an empty store: the genesis is fixed, the head appears with the first record |
| tool-live-1280.png | live | 1280 | live feed while ~400 events/s arrive over syslog UDP: rates, funnel with the no_parser and parse_failed deficits, queue 45/64, the tail, sources and parsers; tool-driven (Chrome MCP) |
| tool-shortcuts-1280.png | keys | 1280 | the shortcut overlay opened with ? over the live feed; tool-driven (Chrome MCP) |
| tool-review-1280.png | review | 1280 | review of the edgerouter proposal reached by keyboard (2, j, Enter): definition, actions, template 1 with every slot name and the reason for it; tool-driven (Chrome MCP) |
| tool-review-confirm-1280.png | review | 1280 | a opens the approve confirmation for bench_slice_inferred, focus on Approve, Esc cancels, Tab reaches Cancel; tool-driven (Chrome MCP) |
| tool-review-approved-1280.png | review | 1280 | Enter confirmed: the result names the file written, 14 parsers loaded and 1,403 of 1,442 buffered lines now on the fast path (mikrotik was approved the same way first); tool-driven (Chrome MCP) |
| tool-pivot-1280.png | pivot | 1280 | pivot of the busiest entity (user jdoe, 19,331 events over 8 devices): device lane, timeline, seen-with lists; clicking dst_ip 10.0.0.2 re-pivoted to #/pivot/dst_ip/10.0.0.2 with the trail in the breadcrumb; tool-driven (Chrome MCP) |
| tool-trace-1280.png | trace | 1280 | traceback of raw id 209334 (palo_alto_panos, 113 pairs): j walked to connection_info.protocol_name, its bytes (udp) lit in the raw record; tool-driven (Chrome MCP) |
| tool-trace-hex-1280.png | trace | 1280 | the same record after h: sixteen bytes per row, the lit field (75 64 70) carried into the hex and ascii columns; tool-driven (Chrome MCP) |
| tool-trace-big-1280.png | trace | 1280 | traceback of the 4,000,035-byte record scrolled into its raw view (virtualised rows, 157 bytes each), rendered 959 ms after navigation with one 608 ms frame, then 120 Hz smooth scrolling; tool-driven (Chrome MCP) |
| tool-trace-missing-1280.png | error | 1280 | error state: raw id 999999999, HTTP 404, the notice says the store holds 249,407 records; tool-driven (Chrome MCP) |
| tool-replay-1280.png | replay | 1280 | replay v2 after renaming squid's response_time slot: the three why lines verbatim (new parser, changed parser with digests, reloads during v1), counters, parser changes, by field; tool-driven (Chrome MCP) |
| tool-drift-1280.png | drift | 1280 | drift: two established sources watching, window misses against the baseline, excess; tool-driven (Chrome MCP) |
| tool-integrity-1280.png | integrity | 1280 | integrity after POST /api/integrity/verify: clean verdict over 249,407 records in 0.53 s, store id, genesis, head; tool-driven (Chrome MCP) |
| tool-live-1512.png | live | 1512 | the live feed at the widest window this 1512 px display allows: the tail and both lower tables use the width; tool-driven (Chrome MCP) |
| tool-pivot-1512.png | pivot | 1512 | the jdoe pivot at 1512: timeline and seen-with lists side by side, no empty band; tool-driven (Chrome MCP) |

## The desktop app (lane C, `app/`)

Captured with `screencapture -x` of the real ULPF.app window built from the branch (`pnpm tauri build`). The add-files, drag, review and approve steps were driven with the computer-use tools; the `app-tool-*` rows at the end are the second, end-to-end pass on the final bundle (01:19-01:31 IST, 06 Sep), every step driven with the computer-use tools (native open panel by Cmd+O, Cmd+Shift+G and a typed path; a real mouse drag from a Finder window; clicks inside the webview), so these are tool-driven captures, not headless ones.

| file | what it shows | how |
|---|---|---|
| app-launch.png | first launch from a clean data directory: the title reads `ULPF · engine ok · 0 events · 0 pending`, the served live feed inside the window, 12 parsers seeded | `open ULPF.app`, screencapture |
| app-add-files.png | File > Add files… with samples/cisco_asa.log: the notice `Added 1 file to the watch folder: cisco_asa.log`, the source row and the title at 30 events | tool-driven (computer use): Cmd+O, Cmd+Shift+G, path, Return |
| app-drop.png | samples/juniper_srx.log dragged from a Finder window onto the app: the notice and the source row at 16 events | tool-driven (computer use): left_click_drag from Finder |
| app-review.png | heldout/mikrotik.log dropped: the title shows 1 pending and the Review screen inside the app shows the proposal's definition and evidence | tool-driven (computer use) |
| app-approved.png | the proposal approved inside the app: `Approved as mikrotik_inferred, written to …/dev.ulpf.desktop/parsers/…`, 0 pending | tool-driven (computer use) |
| app-tray.png | the tray menu (Show ULPF, Open output folder, Open in browser, Quit ULPF) while the window is closed and the engine keeps ingesting; the icon itself sits under this Mac's notch overlay | menu opened through Accessibility, screencapture |
| app-output-folder.png | File > Open output folder: Finder with out.jsonl selected in the app's data directory | screencapture |
| app-open-in-browser.png | Open in browser: the same server session in Chrome after a relaunch, records 313 | screencapture |
| app-tool-launch.png | first launch against an empty data directory: the live feed, 12 parsers loaded, `server.url` written and the title reading `ULPF · engine ok · 0 events · 0 pending` | tool-driven (computer use) |
| app-tool-drop.png | samples/cisco_asa.log dragged from a Finder window onto the window: the notice `Added 1 file to the watch folder: cisco_asa.log`, 30 events in the tail and in the title | tool-driven (computer use) |
| app-tool-live.png | the live feed a moment later: funnel 30/30/30/30/30/30, the cisco_asa.log source row, queue 1/64 | tool-driven (computer use) |
| app-tool-approve.png | heldout/mikrotik.log added through File > Add files…: the proposal open in Review (definition, 14 templates, slot names with the reason for each) with the approve confirmation raised, before it is confirmed | tool-driven (computer use) |
| app-tool-approved.png | the same proposal after Enter: `Approved: mikrotik_inferred is active`, written to `…/dev.ulpf.desktop/parsers/mikrotik_inferred.toml`, 13 parsers loaded, 250 of 250 buffered lines on the fast path, 0 pending | tool-driven (computer use) |
| app-tool-output-folder.png | File > Open output folder (Cmd+Shift+E): Finder with out.jsonl selected in the app's data directory | tool-driven (computer use) |
| app-tool-datadir-panel.png | File > Choose data directory…: the native folder panel, cancelled without changing anything | tool-driven (computer use) |
| app-tool-tray.png | the tray menu (Show ULPF, Open output folder, Open in browser, Quit ULPF) with the window closed and the engine still serving; this Mac's notch utility covers the icon, so the menu was opened through Accessibility | tool-driven (computer use) |
| app-tool-relaunch.png | after Quit and a second launch: a new port answers, the store keeps its 280 records and out.jsonl its 280 lines, and the parsers table carries mikrotik_inferred with origin `approved` | tool-driven (computer use) |
