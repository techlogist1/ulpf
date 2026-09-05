#!/usr/bin/env python3
"""Extract the first fenced code block under a given Markdown heading.

Used by the cold_start criterion to read commands out of a tool's own README
without hand-copying them into the harness (the whole point of that check is
that the README is the only source of truth).

Usage: extract_fence.py README.md "## Heading text"
Prints each line of the first ``` fenced block found after that heading.
"""
import sys

path, heading = sys.argv[1], sys.argv[2]
lines = open(path, encoding="utf-8").read().splitlines()

start = None
for i, line in enumerate(lines):
    if line.strip() == heading.strip():
        start = i
        break
if start is None:
    sys.exit(0)  # heading not found -> empty output -> caller reports FAIL

fence_open = None
block = []
for line in lines[start + 1:]:
    if fence_open is None:
        if line.startswith("```"):
            fence_open = True
            continue
        if line.startswith("## "):  # next heading, no fence found
            break
        continue
    if line.startswith("```"):
        break
    block.append(line)

print("\n".join(block))
