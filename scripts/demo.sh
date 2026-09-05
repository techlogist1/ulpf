#!/bin/zsh
# The runner is `ulpf demo` (D67): the demo is orchestration inside the binary, so playing it
# needs no shell. Played end to end on macOS only; the Windows branches are compiled by CI and
# have not been run (D74). This wrapper only finds the binary and hands the flags over, so
# anything that referenced scripts/demo.sh keeps working. Any other shell re-execs this with zsh.
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
