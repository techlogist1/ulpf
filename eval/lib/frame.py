#!/usr/bin/env python3
"""Reference framer for raw_preservation: independent of any tool, so a tool's
"raw bytes of event N" answer has something to be checked against.

Framing rule (as specified for this harness): line-oriented; an event is one
line plus every following line that starts with space, tab, CR or LF, with all
terminators kept verbatim.

Usage:
  frame.py INPUT_FILE --count        -> prints the number of framed events
  frame.py INPUT_FILE --get N        -> writes event N's exact bytes to stdout
"""
import sys

CONT_PREFIXES = (0x20, 0x09, 0x0D, 0x0A)  # space, tab, CR, LF


def split_physical_lines(data: bytes):
    lines = []
    start = 0
    for i, b in enumerate(data):
        if b == 0x0A:
            lines.append(data[start:i + 1])
            start = i + 1
    if start < len(data):
        lines.append(data[start:])
    return lines


def group_events(physical_lines):
    events = []
    cur = None
    for pl in physical_lines:
        first = pl[0] if pl else None
        if cur is not None and first is not None and first in CONT_PREFIXES:
            cur += pl
        else:
            if cur is not None:
                events.append(cur)
            cur = pl
    if cur is not None:
        events.append(cur)
    return events


def main():
    path, mode = sys.argv[1], sys.argv[2]
    with open(path, "rb") as f:
        data = f.read()
    events = group_events(split_physical_lines(data))
    if mode == "--count":
        print(len(events))
    elif mode == "--get":
        n = int(sys.argv[3])
        if n < 0 or n >= len(events):
            print(f"event {n} out of range (0..{len(events) - 1})", file=sys.stderr)
            sys.exit(1)
        sys.stdout.buffer.write(events[n])
    else:
        print("usage: frame.py FILE --count | --get N", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
