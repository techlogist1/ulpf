# Samples

Every file here is SYNTHETIC. It was written from public vendor documentation during
the v0.1 build, then deliberately dirtied (truncated lines, a non-UTF-8 byte, a
multi-line event, no-year timestamps) because documentation examples cluster
beautifully and real logs do not. Real samples collected by the team replace these
file-for-file; keep the same file names so fixtures keep matching.

One `samples/<parser>.log` per `parsers/<parser>.toml`, with expected output in
`fixtures/<parser>.expected.jsonl`.
