#!/usr/bin/env python3
"""Flatten a tool.toml into `eval "$(cfg.py tool.toml)"`-able bash assignments.

[templates] run = "..."  ->  CFG_templates_run='...'
name = "ulpf"             ->  CFG_name='ulpf'
Lists become newline-joined so a bash `while read` can walk them; nothing else
gets clever, this is a config loader, not a language.
"""
import sys
import tomllib
import shlex

path = sys.argv[1]
with open(path, "rb") as f:
    data = tomllib.load(f)


def flatten(prefix, value, out):
    if isinstance(value, dict):
        for k, v in value.items():
            flatten(f"{prefix}_{k}" if prefix else k, v, out)
    elif isinstance(value, list):
        out[prefix] = "\n".join(str(x) for x in value)
    else:
        out[prefix] = str(value)


flat = {}
flatten("", data, flat)
for k, v in flat.items():
    print(f"CFG_{k}={shlex.quote(v)}")
