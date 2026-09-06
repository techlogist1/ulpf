#!/usr/bin/env bash
# Copies the engine into the name Tauri's externalBin expects: binaries/ulpf-<host
# triple>[.exe]. CI runs this same script on macOS and on Windows (Git Bash); the only
# platform difference is the .exe. The engine wanted is the shipped one,
# `cargo build --profile dist -p ulpf`; target/release/ is taken only as a fallback, with a
# warning naming the profile, because a release binary is not what the numbers were
# measured on. CARGO_TARGET_DIR is honoured: cargo puts the build where that says, so this
# script has to look in the same place.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
target="${CARGO_TARGET_DIR:-$root/target}"
triple="$(rustc -vV | sed -n 's/host: //p')"
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac

# A generated parser inside the bundle is a broken demo: the app would arrive already
# knowing the unseen format and could raise no proposal of its own. This is the first
# command of the bundle step, so it is where that is refused.
# Named as it is found: this repo's own path has a space in it, so a list collected in one
# string and re-split would print two half-paths instead of one file.
generated=0
for p in "$root"/parsers/*.toml; do
  [ -f "$p" ] || continue
  # One line carrying both, as sidecar.ps1 and the shell's `is_generated` (src/lib.rs) test
  # it: two greps over the whole file refused a hand-written parser whose prose said
  # "inferred".
  if grep -Eq '^[[:space:]]*origin.*inferred' "$p"; then
    [ "$generated" -eq 0 ] && echo "sidecar.sh: the bundle would carry a generated parser:" >&2
    generated=$((generated + 1))
    echo "  $p" >&2
  fi
done
if [ "$generated" -gt 0 ]; then
  echo "sidecar.sh: remove the $generated of them with: ulpf demo --reset" >&2
  exit 1
fi

src="$target/dist/ulpf$ext"
profile=dist
if [ ! -f "$src" ]; then
  src="$target/release/ulpf$ext"
  profile=release
  echo "sidecar.sh: warning: no $target/dist/ulpf$ext; taking the release profile instead (build the shipped one with: cargo build --profile dist -p ulpf)" >&2
fi
[ -f "$src" ] || { echo "sidecar.sh: $src missing; run: cargo build --profile dist -p ulpf" >&2; exit 1; }

dst="$root/app/src-tauri/binaries/ulpf-$triple$ext"
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"
chmod +x "$dst"
echo "sidecar: $dst (profile $profile)"
