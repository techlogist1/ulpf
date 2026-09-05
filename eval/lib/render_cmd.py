#!/usr/bin/env python3
"""Render a command template for display (the exact-command-that-produced-it
requirement) using the same literal {key} substitution eval/lib/run_async.py
applies before it runs the command -- kept as a separate tiny script so the
scorecard can show the real argv without re-deriving it by hand.

Usage: render_cmd.py TEMPLATE key=value [key=value ...]
"""
import sys

tmpl = sys.argv[1]
for kv in sys.argv[2:]:
    k, _, v = kv.partition("=")
    tmpl = tmpl.replace("{%s}" % k, v)
print(tmpl)
