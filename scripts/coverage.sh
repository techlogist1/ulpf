#!/usr/bin/env bash
# What the engine does to every file we have: samples, real captures, locally generated
# captures. One fresh store per file, every number read from --report-json with python3,
# never from the human-readable counter block.
#
#   scripts/coverage.sh > docs/coverage.md
#   scripts/coverage.sh /path/to/loglens-corpus > docs/coverage.md   # + a vendor-match table
#
# A DIR argument is graded per vendor: its immediate subdirectory names are the vendors,
# and a file matches when the parser that claimed it carries that name.
# ULPF=<binary> overrides ./target/release/ulpf.
set -euo pipefail
cd "$(dirname "$0")/.."

ULPF="${ULPF:-./target/release/ulpf}"
[ -x "$ULPF" ] || { echo "coverage.sh: $ULPF is not executable; cargo build --release -p ulpf" >&2; exit 1; }

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
manifest="$scratch/manifest.tsv"
: > "$manifest"

# group, file, vendor ("" outside the vendor tables)
run_one() {
  local group="$1" f="$2" vendor="${3:-}" key
  key="$(printf '%s' "$group|$f" | tr -c 'A-Za-z0-9' '_')"
  "$ULPF" run "$f" --store "$scratch/st-$key" --output "$scratch/out-$key.jsonl" \
      --infer-threshold 0 --report-json "$scratch/rep-$key.json" >/dev/null 2>&1 || true
  printf '%s\t%s\t%s\t%s\t%s\n' \
      "$group" "$f" "$vendor" "$scratch/rep-$key.json" "$scratch/out-$key.jsonl" >> "$manifest"
}

for f in samples/*.log; do
  run_one samples "$f"
done

# PROVENANCE.md and the setup/ recipes are documentation, not log input.
if [ -d corpus/real ]; then
  while IFS= read -r f; do run_one corpus_real "$f"; done < <(
    find corpus/real -type f ! -name '*.md' ! -path '*/setup/*' | sort)
fi
if [ -d corpus/generated ]; then
  while IFS= read -r f; do run_one corpus_generated "$f"; done < <(
    find corpus/generated -type f ! -name '*.md' ! -path '*/setup/*' | sort)
fi

for dir in "$@"; do
  [ -d "$dir" ] || { echo "coverage.sh: $dir is not a directory" >&2; exit 1; }
  while IFS= read -r vpath; do
    vendor="$(basename "$vpath")"
    while IFS= read -r f; do run_one "vendors:$dir" "$f" "$vendor"; done < <(
      find "$vpath" -type f ! -name '*.md' | sort)
  done < <(find "$dir" -mindepth 1 -maxdepth 1 -type d | sort)
done

export COVERAGE_COMMAND="scripts/coverage.sh${*:+ $*}"
export COVERAGE_COMMIT="$(git rev-parse --short HEAD)"
export COVERAGE_DATE="$(date -u '+%Y-%m-%d %H:%M UTC')"
export COVERAGE_BIN="$ULPF"

python3 - "$manifest" <<'PY'
import collections, json, os, re, sys

manifest = sys.argv[1]
rows = []
for line in open(manifest, encoding="utf-8"):
    group, path, vendor, report, out = line.rstrip("\n").split("\t")
    rows.append((group, path, vendor, report, out))

def lines_of(path):
    b = open(path, "rb").read()
    return b.count(b"\n") + (1 if b and not b.endswith(b"\n") else 0)

def report_of(path):
    try:
        return json.load(open(path, encoding="utf-8"))
    except (OSError, ValueError):
        return None

def reasons(pairs):
    return ", ".join(f"{r} {n}" for r, n in pairs) if pairs else "none"

# The parser that claimed the file: the commonest ulpf.parser over its output lines.
def detected_parser(out):
    counts = collections.Counter()
    try:
        fh = open(out, encoding="utf-8")
    except OSError:
        return None
    with fh:
        for line in fh:
            try:
                name = json.loads(line).get("ulpf", {}).get("parser")
            except ValueError:
                continue
            if name:
                counts[name] += 1
    return counts.most_common(1)[0][0] if counts else None

# parser name -> [parser] vendor, for the vendor-match column.
vendors = {}
for name in os.listdir("parsers") if os.path.isdir("parsers") else []:
    if not name.endswith(".toml"):
        continue
    text = open(os.path.join("parsers", name), encoding="utf-8", errors="replace").read()
    m = re.search(r'^\s*vendor\s*=\s*"([^"]*)"', text, re.M)
    vendors[name[:-5]] = m.group(1) if m else ""

COLS = ["file", "lines", "framed", "detected", "parsed", "parse_failed", "sub_uncovered",
        "sub_no_match", "time_from_receipt", "class_unknown", "unmapped_fields"]

def counters_table(group_rows):
    print("| " + " | ".join(COLS) + " |")
    print("|" + "|".join("---" for _ in COLS) + "|")
    for _, path, _, report, _ in group_rows:
        r = report_of(report)
        if r is None:
            print(f"| `{path}` | {lines_of(path)} | " + " | ".join(["run failed"] * 9) + " |")
            continue
        print("| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
            path, lines_of(path), r["framed"], r["detected"], r["parsed"],
            reasons(r["parse_failed"]), r["sub_uncovered"], r["sub_no_match"],
            r["time_from_receipt"], r["class_unknown"], r["unmapped_fields"]))

print("# Coverage")
print()
print("Every sample and every corpus file through the built binary, one fresh store each.")
print(f"Regenerate with `{os.environ['COVERAGE_COMMAND']} > docs/coverage.md`.")
print()
print(f"- binary: `{os.environ['COVERAGE_BIN']}` at commit `{os.environ['COVERAGE_COMMIT']}`")
print(f"- generated: {os.environ['COVERAGE_DATE']}")
print("- per file: `ulpf run <file> --store <fresh> --output <scratch> --infer-threshold 0"
      " --report-json <scratch>`; every number below is a field of that JSON report.")
print("- `lines` is the file's own line count; `framed` is what the engine made of it, so"
      " the two differ where a collector folded one event over two lines.")
print("- `PROVENANCE.md` and `setup/` are documentation and are not run.")
print()
print("The Zeek rows are the honest uncovered set: sixteen files, 23,434 lines, no parser"
      " claims one of them. Lane 3's CEF, LEEF and CloudTrail definitions have landed and"
      " did not move them; Zeek stays one of the unseen formats the live inference demo"
      " runs against (`corpus/README.md`) until a Zeek definition exists.")
print()

for group, title in (("samples", "## samples/"),
                     ("corpus_real", "## corpus/real/"),
                     ("corpus_generated", "## corpus/generated/")):
    group_rows = [r for r in rows if r[0] == group]
    if not group_rows:
        continue
    print(title)
    print()
    counters_table(group_rows)
    print()

for group in sorted({r[0] for r in rows if r[0].startswith("vendors:")}):
    group_rows = [r for r in rows if r[0] == group]
    print(f"## {group.split(':', 1)[1]} (graded by vendor directory)")
    print()
    counters_table(group_rows)
    print()
    print("| file | vendor | detected parser | match |")
    print("|---|---|---|---|")
    matched = 0
    for _, path, vendor, _, out in group_rows:
        parser = detected_parser(out)
        needle = vendor.lower()
        hay = f"{parser or ''} {vendors.get(parser or '', '')}".lower()
        ok = bool(parser) and needle in hay
        matched += ok
        print(f"| `{path}` | {vendor} | {parser or 'none'} | {'yes' if ok else 'no'} |")
    print()
    print(f"{matched} of {len(group_rows)} files claimed by a parser carrying their vendor's name.")
    print()
PY
