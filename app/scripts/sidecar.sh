#!/usr/bin/env bash
# Copies the engine built by `cargo build --release -p ulpf` at the repo root into the
# name Tauri's externalBin expects: binaries/ulpf-<host triple>[.exe]. CI runs this same
# script on macOS and on Windows (Git Bash); the only platform difference is the .exe.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
triple="$(rustc -vV | sed -n 's/host: //p')"
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac

# A generated parser inside the bundle is a broken demo: the app would arrive already
# knowing the unseen format and could raise no proposal of its own. This is the first
# command of the bundle step, so it is where that is refused.
generated=""
for p in "$root"/parsers/*.toml; do
  [ -f "$p" ] || continue
  if grep -Eq '^[[:space:]]*origin' "$p" && grep -q 'inferred' "$p"; then
    generated="$generated $p"
  fi
done
if [ -n "$generated" ]; then
  echo "sidecar.sh: the bundle would carry a generated parser:" >&2
  for p in $generated; do echo "  $p" >&2; done
  echo "sidecar.sh: remove it with: ulpf demo --reset" >&2
  exit 1
fi

src="$root/target/release/ulpf$ext"
dst="$root/app/src-tauri/binaries/ulpf-$triple$ext"
[ -f "$src" ] || { echo "sidecar.sh: $src missing; run: cargo build --release -p ulpf" >&2; exit 1; }
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"
chmod +x "$dst"
echo "sidecar: $dst"
