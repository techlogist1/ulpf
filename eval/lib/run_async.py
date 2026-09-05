#!/usr/bin/env python3
"""Render a command template and run it, with a timeout and no shell.

Exists because the repo path contains a space ("ssh hackathon"), the eval
harness must run on both macOS (no `timeout` binary) and Linux, and macOS
ships bash 3.2 (no arrays that survive NUL-safe splitting cleanly). Doing the
substitution + argv split + timeout + pid tracking in one stdlib script sidesteps
all three instead of fighting bash quoting.

Two modes, chosen by whether the caller backgrounds this process (`&`) or waits on it:
  sync:  bash runs `run_async.py ... ` and blocks; exit summary goes to --exitfile
         and this process's own exit code, so `wait`/`$?` both work.
  async: bash runs it with `&`, polls --pidfile for the real child pid (written
         before the wait starts) to sample /kill it, then `wait`s the bash job so
         --exitfile is guaranteed flushed.

--exitfile gets one line: "<rc>|<elapsed_seconds>|<note>"
  rc:  the child's exit code, or -N if killed by signal N, or -1 if it never started
  note: ok | timeout | killed | exec_error:<msg> | bad_template:<msg>
"""
import argparse
import shlex
import signal
import subprocess
import sys
import time
import os

p = argparse.ArgumentParser()
p.add_argument("--template", required=True)
p.add_argument("--timeout", type=float, default=None)
p.add_argument("--out", required=True)
p.add_argument("--err", required=True)
p.add_argument("--cwd", default=None)
p.add_argument("--pidfile", default=None)
p.add_argument("--exitfile", required=True)
p.add_argument("--set", action="append", default=[], metavar="KEY=VALUE")
args = p.parse_args()

tmpl = args.template
for kv in args.set:
    k, _, v = kv.partition("=")
    tmpl = tmpl.replace("{%s}" % k, v)

def finish(rc, elapsed, note):
    with open(args.exitfile, "w") as f:
        f.write(f"{rc}|{elapsed:.3f}|{note}\n")
    sys.exit(0)

try:
    argv = shlex.split(tmpl)
except ValueError as e:
    finish(-1, 0.0, f"bad_template:{e}")

if not argv:
    finish(-1, 0.0, "bad_template:empty after substitution")

t0 = time.time()
try:
    with open(args.out, "wb") as outf, open(args.err, "wb") as errf:
        proc = subprocess.Popen(
            argv, stdout=outf, stderr=errf, cwd=args.cwd, start_new_session=True
        )
        if args.pidfile:
            with open(args.pidfile, "w") as pf:
                pf.write(str(proc.pid))
        note = "ok"
        try:
            rc = proc.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            note = "timeout"
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except Exception:
                proc.kill()
            try:
                rc = proc.wait(timeout=5)
            except Exception:
                rc = -9
        if rc < 0:
            note = "killed" if note == "ok" else note
except FileNotFoundError as e:
    finish(-1, time.time() - t0, f"exec_error:{e}")
except Exception as e:
    finish(-1, time.time() - t0, f"exec_error:{e}")

finish(rc, time.time() - t0, note)
