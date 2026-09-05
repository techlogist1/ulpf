# Provenance — check_point

No file committed. No independently-licensed, real-format sample of Check Point's
Log Exporter **"syslog" format** (`[key:"value"; key:"value"; ...]`, the form
`parsers/check_point.toml` matches on: `contains = ["CheckPoint"]` +
`regex = '\[[A-Za-z_]+:"'`) could be obtained inside the time box. Every candidate
found was rejected — see `not_obtained` in the run's structured return for the
full list of URLs checked and why each was rejected (Elastic-License-derived,
no-licence repos, or generator templates producing placeholder/random values
rather than captured bytes).

Two things worth the lead's time at the demo:
1. Check Point's own CheckMates community forum (`community.checkpoint.com`) has
   threads with real pasted Log Exporter output in exactly this bracket form (e.g.
   the "R80.30 Firewall Logs via Log Exporter to McAfee SIEM" thread) but the site
   returns HTTP 403 to both WebFetch and a plain `curl -A "Mozilla/5.0"` from this
   box — it may be reachable live, or from a browser session.
2. Check Point Log Exporter also ships a "splunk" output format
   (`key=value`, no brackets, `|`- or space-separated) which real captures of
   *that* format are much easier to find (e.g. `jpvlsmv/cc-checkpoint-pack`,
   Apache-2.0, `data/samples/*.json` `_raw` fields) — but it does not match this
   parser's `[match]` block at all, so pulling it in would not exercise
   `check_point.toml` and was left out rather than logged as a false "no_parser".
