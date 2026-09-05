#!/usr/bin/env bash
# Copies the engine built by `cargo build --release -p ulpf` at the repo root into the
# name Tauri's externalBin expects: binaries/ulpf-<host triple>[.exe]. CI runs this same
# script on macOS and on Windows (Git Bash); the only platform difference is the .exe.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
triple="$(rustc -vV | sed -n 's/host: //p')"
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
src="$root/target/release/ulpf$ext"
dst="$root/app/src-tauri/binaries/ulpf-$triple$ext"
[ -f "$src" ] || { echo "sidecar.sh: $src missing; run: cargo build --release -p ulpf" >&2; exit 1; }
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"
chmod +x "$dst"
echo "sidecar: $dst"
