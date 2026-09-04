# Fixtures

One `fixtures/<parser>.expected.jsonl` per `parsers/<parser>.toml`, paired with
`samples/<parser>.log`. One JSON object per sample event, in order; `#` lines and blank
lines are ignored. The test `cargo test -p ulpf --test fixtures` runs every pair through
the production pipeline with the receipt time fixed at 2026-09-04T12:00:00Z and UTC as
the default zone, and reports every mismatch as `file:line: what differed`. Every parser
must be covered by at least one fixture line or the test fails.

Keys (all optional; a key present is asserted, a key absent is not):

| key | meaning |
|---|---|
| `parser` | parser that must be detected, or `"none"` |
| `status` | `parsed`, `no_parser`, or a parse-failure reason (`pattern_no_match`, ...) |
| `sub` | `not_applicable` (no subs, or none of the fields the subs re-parse is present), `matched`, `no_match` (a gated sub ran and failed), `uncovered` (a field with subs is present but no sub is gated for it) |
| `fields` | parsed vendor fields that must be present with these exact values |
| `absent` | parsed field names that must not be present |
| `normalized` | dotted OCSF paths and their exact values |
| `time` | event time in epoch milliseconds |
| `time_policies` | exact list, e.g. `["year_assumed", "tz_assumed"]` |

Generate a starting point, then review every line against the vendor's log reference
before committing it — the generator reflects what the code does today, not what is
correct:

```
cargo run --release -- fixture samples/<parser>.log > fixtures/<parser>.expected.jsonl
```

The generator writes every parsed field and a fixed subset of normalized paths (class,
action, severity, endpoints, rule, user, event code). Trim `normalized` further to the
values you actually verified; a fixture that asserts everything is a snapshot, not a test.
The committed fixtures were generated this way and then reviewed line by line during the
v0.1 build (see `docs/DECISIONS.md`, D30).
