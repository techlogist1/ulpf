#!/bin/zsh
# The runner is `ulpf demo` (D67): the same demo plays on macOS, Windows and Linux, where no
# shell exists. This wrapper only finds the binary and hands the flags over, so anything that
# referenced scripts/demo.sh keeps working. Any other shell re-executes this file with zsh.
#   scripts/demo.sh            # interactive: Enter advances; the server stays up for questions at the end
#   scripts/demo.sh --auto     # unattended rehearsal: fixed pauses, then stop and reset
#   scripts/demo.sh --check    # inputs, ports, and every title and command verbatim in PROGRESS.md
#   scripts/demo.sh --reset    # stop a leftover server and remove demo/
[ -n "${ZSH_VERSION:-}" ] || exec /bin/zsh "$0" "$@"
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/ulpf
[ -x "$BIN" ] || { echo "build first: cargo build --release"; exit 2; }
exec "$BIN" demo "$@"
