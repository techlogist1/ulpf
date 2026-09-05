#!/usr/bin/env python3
"""Compare a tool's JSONL output against one fixtures/*.expected.jsonl file.

Only what a black-box harness can see in a tool's *output* is checked: fixture
order-position, its "normalized" subset, "time", "time_policies", "parser",
"status", "sub". "fields"/"absent" describe the vendor-native ParsedEvent stage,
internal to whichever tool produced it -- not assertable from outside without
whitebox access, so out of scope here (ULPF's own `cargo test --test fixtures`
already covers that stage).

Usage: jsoncmp.py FIXTURE.jsonl OUTPUT.jsonl KEYMAP.json
Stdout: "total=N matched=N mismatched=N missing=N pct=NN.N"
Stderr: one line per mismatching/missing fixture event.

KEYMAP.json is {"map": {"<fixture key>": "<output dotted key>", ...}}. A fixture
key not in the map defaults to itself with any "normalized." prefix stripped --
correct for a tool whose output already nests fields the way OCSF fixtures do;
override individual keys for a tool with a differently-shaped schema.
"""
import json
import sys

fixture_path, output_path, keymap_path = sys.argv[1:4]

with open(keymap_path) as f:
    keymap = json.load(f).get("map", {})


def flatten(obj, prefix=""):
    flat = {}
    if isinstance(obj, dict):
        for k, v in obj.items():
            p = f"{prefix}.{k}" if prefix else k
            if isinstance(v, dict):
                flat.update(flatten(v, p))
            else:
                flat[p] = v
    return flat


def out_key(fixture_key):
    if fixture_key in keymap:
        return keymap[fixture_key]
    if fixture_key.startswith("normalized."):
        return fixture_key[len("normalized."):]
    return fixture_key


def load_jsonl(path, skip_comments):
    rows = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            if skip_comments and line.startswith("#"):
                continue
            rows.append(json.loads(line))
    return rows


fixture_lines = load_jsonl(fixture_path, skip_comments=True)
output_lines = load_jsonl(output_path, skip_comments=False)

total = len(fixture_lines)
matched = mismatched = missing = 0

for i, fx in enumerate(fixture_lines):
    if i >= len(output_lines):
        missing += 1
        print(f"line {i}: missing (tool emitted only {len(output_lines)} event(s))", file=sys.stderr)
        continue
    flat_out = flatten(output_lines[i])
    checks = {}
    for meta_key in ("parser", "status", "sub", "time", "time_policies"):
        if meta_key in fx:
            checks[meta_key] = fx[meta_key]
    for k, v in fx.get("normalized", {}).items():
        checks[f"normalized.{k}"] = v

    bad = []
    for fk, want in checks.items():
        ok = out_key(fk)
        got = flat_out.get(ok, "<absent>")
        if got == "<absent>" and want == "none":
            continue  # fixtures/README.md: "none" is the sentinel for "no parser", an absent key is that
        if got != want:
            bad.append(f"{fk} (-> {ok}): want {want!r} got {got!r}")
    if bad:
        mismatched += 1
        print(f"line {i}: " + "; ".join(bad), file=sys.stderr)
    else:
        matched += 1

pct = (matched / total * 100) if total else 0.0
print(f"total={total} matched={matched} mismatched={mismatched} missing={missing} pct={pct:.1f}")
