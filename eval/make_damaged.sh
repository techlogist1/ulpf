#!/usr/bin/env bash
# Generates the FIXED damaged-input corpus under eval/damaged/, deterministically,
# so every tool run through the harness sees identical bytes. Never modifies
# samples/ -- only reads samples/cisco_asa.log as seed material for the entries
# that need a "valid log" to corrupt. Re-run any time; it always overwrites.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/eval/damaged
SEED=$ROOT/samples/cisco_asa.log
[ -f "$SEED" ] || { echo "seed file missing: $SEED" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"

# 1. empty file
: > "$OUT/empty.log"

# 2. only newlines
python3 -c "open('$OUT/only_newlines.log','wb').write(b'\n' * 20)"

# 3. a single 8 MiB line
python3 -c "
n = 8 * 1024 * 1024
with open('$OUT/single_8mib_line.log', 'wb') as f:
    f.write(b'A' * n + b'\n')
"

# 4. binary garbage (deterministic pseudo-random bytes, guaranteed non-UTF8, incl. NULs)
python3 -c "
import random
r = random.Random(42)
with open('$OUT/binary_garbage.log', 'wb') as f:
    f.write(r.randbytes(4096))
"

# 5. UTF-16 with BOM (valid seed log re-encoded)
python3 -c "
data = open('$SEED', encoding='utf-8', errors='replace').read()
open('$OUT/utf16_bom.log', 'wb').write(data.encode('utf-16'))  # utf-16 with native BOM
"

# 6. CRLF line endings
python3 -c "
data = open('$SEED', 'rb').read().replace(b'\n', b'\r\n')
open('$OUT/crlf.log', 'wb').write(data)
"

# 7. truncated last line, no trailing newline
python3 -c "
data = open('$SEED', 'rb').read()
lines = data.split(b'\n')
lines = [l for l in lines if l][:5]
last = lines[-1]
lines[-1] = last[: max(1, len(last) // 2)]
open('$OUT/truncated_no_newline.log', 'wb').write(b'\n'.join(lines))
"

# 8. valid log with a NUL byte inserted mid-file
python3 -c "
data = bytearray(open('$SEED', 'rb').read())
mid = len(data) // 2
data[mid:mid] = b'\x00'
open('$OUT/nul_byte_mid.log', 'wb').write(bytes(data))
"

# 9. 0-byte file inside a nested dir
mkdir -p "$OUT/nested/deep/dir"
: > "$OUT/nested/deep/dir/zero_byte.log"

# 10. a directory symlink loop (macOS and Linux both create these fine)
ln -sfn symlink_loop "$OUT/symlink_loop"

# 11. valid file, 10% of lines cut mid-field (every 10th line truncated to half length)
python3 -c "
lines = [l for l in open('$SEED', 'rb').read().split(b'\n') if l]
out = []
for i, l in enumerate(lines):
    if i % 10 == 0 and len(l) > 4:
        l = l[: len(l) // 2]
    out.append(l)
open('$OUT/cut_mid_field_10pct.log', 'wb').write(b'\n'.join(out) + b'\n')
"

# 12. a format no parser covers (synthetic, distinctive, deterministic)
python3 -c "
lines = [
    f'<PROPRIETARY-WIDGETFW>2026-09-05T10:{i:02d}:00Z host=widget-{i} code={100+i} verdict=OPAQUE</PROPRIETARY-WIDGETFW>'
    for i in range(50)
]
open('$OUT/no_parser_format.log', 'w').write('\n'.join(lines) + '\n')
"

echo "damaged inputs written to $OUT:"
find "$OUT" \( -type f -o -type l \) | sort
