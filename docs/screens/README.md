# Screen captures

## The desktop app (`app/`)

Captured with `screencapture -x` of the real ULPF.app window (the bundle the lead built from
the merged tree). Every step — launch, the drag from Finder, the native open panel, the review
and approve keys inside the webview, the tray, the relaunch — was driven with the computer-use
tools against a data directory emptied first, so this is one clean first run end to end.

| file | what it shows | how |
|---|---|---|
| app-tool-launch.png | first launch against an empty data directory: the live feed, 12 parsers loaded, `server.url` written and the title reading `ULPF · engine ok · 0 events · 0 pending` | tool-driven (computer use) |
| app-tool-drop.png | samples/cisco_asa.log dragged from a Finder window onto the window: the notice `Added 1 file to the watch folder: cisco_asa.log`, 30 events in the tail and in the title | tool-driven (computer use) |
| app-tool-live.png | the live feed a moment later: funnel 30/30/30/30/30/30, the cisco_asa.log source row, queue 1/64 | tool-driven (computer use) |
| app-tool-approve.png | heldout/mikrotik.log added through File > Add files…: the proposal open in Review (definition, 14 templates, slot names with the reason for each) with the approve confirmation raised, before it is confirmed | tool-driven (computer use) |
| app-tool-approved.png | the same proposal after Enter: `Approved: mikrotik_inferred is active`, written to `…/dev.ulpf.desktop/parsers/mikrotik_inferred.toml`, 13 parsers loaded, 250 of 250 buffered lines on the fast path, 0 pending | tool-driven (computer use) |
| app-tool-output-folder.png | File > Open output folder (Cmd+Shift+E): Finder with out.jsonl selected in the app's data directory | tool-driven (computer use) |
| app-tool-datadir-panel.png | File > Choose data directory…: the native folder panel, cancelled without changing anything | tool-driven (computer use) |
| app-tool-tray.png | the tray menu (Show ULPF, Open output folder, Open in browser, Quit ULPF) with the window closed and the engine still serving; this Mac's notch utility covers the icon, so the menu was opened through Accessibility | tool-driven (computer use) |
| app-tool-relaunch.png | after Quit and a second launch: a new port answers, the store keeps its 280 records and out.jsonl its 280 lines, and the parsers table carries mikrotik_inferred with origin `approved` | tool-driven (computer use) |
