# Provenance — palo_alto_panos

| file | source URL | revision | path in source | licence | anonymised by source | kind | lines | fetch method |
|---|---|---|---|---|---|---|---|---|
| `palo_alto_panos.log` | https://github.com/chronosphereio/processing-templates/blob/main/processors/panw/ng_firewall/samples/ngfw-traffic.log | chronosphereio/processing-templates @ default branch, fetched 2026-09-05 via GitHub Contents API (response carried no separate commit SHA field; the API call itself is the record) | `processors/panw/ng_firewall/samples/ngfw-traffic.log`, **lines 101-150 of 150 only** | Apache-2.0 (root `LICENSE`, read via `gh api repos/chronosphereio/processing-templates/contents/LICENSE`, standard Apache License 2.0 text) | External IPs are real-looking public addresses (`81.2.69.144` etc. — the MaxMind GeoIP2 documented test-suite range, commonly seen in real captures that were run through GeoIP-consuming tooling); no usernames/hostnames beyond the box name `PA-VM` | sanitized-real (see caveat) | 50 (of the file's 150; see below) | `gh api repos/chronosphereio/processing-templates/contents/processors/panw/ng_firewall/samples/ngfw-traffic.log --jq .content \| base64 -d`, then `sed -n '101,150p'` — no line invented |

## Why only 50 of the file's 150 lines

The source file's first 100 lines are byte-identical (module the destination IP,
which this repo's copy collapsed to a single placeholder `175.16.199.1`) to
`elastic/beats`' `x-pack/filebeat/module/panw/panos/test/traffic.log` — confirmed
by diff against that file, fetched read-only for comparison only, never copied.
`x-pack/` in `elastic/beats` is Elastic License 2.0 (the whole tree, no
per-directory override found), which the brief excludes. Since chronosphereio's own
Apache-2.0 licence cannot re-license content it does not own the copyright to, the
first 100 lines are treated as Elastic-derived and were **not** copied here even
though the copying repo's own stated licence is permissive — see `not_obtained`.

Lines 101-150 are **not** present in the Elastic fixture (verified: `comm -12` on
sorted line sets shows zero overlap between them and the Elastic file) and use a
materially different, later PAN-OS build's column layout (`PA-VM`, the appended
`high_res_timestamp` ISO-8601 tail column, `intrazone-default`/`any allow` rules,
quoted empty tail fields). Those 50 lines are what was committed.

## Caveat on "sanitized-real"

I could not establish this tail's ultimate origin beyond chronosphereio's own
Apache-2.0 repository — it may itself be a copy of a real customer capture (its
shape — a `PA-VM` logging its own management-plane chatter as `intrazone-default`
traffic with a monotonically increasing internal session id — is consistent with a
genuine lab/home firewall, not a hand-written example). Flagging the uncertainty
rather than asserting it confidently.
