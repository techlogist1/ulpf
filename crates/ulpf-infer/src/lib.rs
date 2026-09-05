//! Parser inference: unknown lines of one source become a candidate parser definition
//! plus the evidence that produced it. The output is a `ParserDefinition` (shape 2
//! vocabulary only: the engine never learns an output-schema name here) and an
//! `Evidence` record that says, per template and per decision, why. Nothing in this
//! crate touches the store, the registry or a file; the engine owns when to run it and
//! where the proposal goes.

mod align;
mod cluster;
mod token;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ulpf_parse::def::{Envelope, Matcher, Meta, ParserDefinition, Strategy, StrategyKind, TimestampSpec};
use ulpf_parse::{Context, Parsed, Parser, Registry, Scratch, Template};

pub use cluster::Params;

/// Lines longer than this in tokens are not clustered: the alignment tables are
/// quadratic in token count, and a 20k-token line is a payload dump, not a message.
pub const MAX_TOKENS: usize = 2048;
use cluster::{Col, lossy};
use token::{Kind, Tok};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub source: String,
    pub definition: ParserDefinition,
    pub evidence: Evidence,
    /// Set when the definition is a new version of an existing parser (drift).
    #[serde(default)]
    pub updates: Option<Update>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Update {
    pub name: String,
    pub current_version: u64,
    /// `patterns_added`, `matcher_widened`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub source: String,
    pub lines_seen: u64,
    pub lines_used: u64,
    pub params: Params,
    pub envelope: EnvelopeEvidence,
    pub templates: Vec<TemplateEvidence>,
    pub unmatched: Unmatched,
    pub decisions: Vec<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeEvidence {
    pub syslog: bool,
    pub example_header: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEvidence {
    pub id: u64,
    pub pattern: String,
    pub support: u64,
    pub verified: u64,
    pub examples: Vec<String>,
    /// Indices into the source lines this template was built from.
    pub members: Vec<u32>,
    pub slots: Vec<SlotEvidence>,
    pub history: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotEvidence {
    pub name: String,
    pub kind: String,
    pub suggested: bool,
    /// The rule that chose the name, in one line, or why the slot stayed `kind+n`.
    pub reason: String,
    pub preceded_by: String,
    pub distinct: u64,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Unmatched {
    pub count: u64,
    pub examples: Vec<String>,
    pub by_reason: BTreeMap<String, u64>,
}

struct Candidate {
    template: Template,
    members: Vec<usize>,
    history: Vec<String>,
    slot_evidence: Vec<SlotEvidence>,
}

fn body(line: &[u8]) -> &[u8] {
    let mut b = line;
    while let Some((&last, rest)) = b.split_last() {
        if last == b'\n' || last == b'\r' {
            b = rest;
        } else {
            break;
        }
    }
    b
}

/// Strips the syslog envelope the way the runtime will, when the source carries one.
fn bodies<'a>(lines: &[&'a [u8]]) -> (bool, Option<String>, Vec<&'a [u8]>, usize) {
    let mut headed = 0usize;
    let mut example = None;
    let mut stripped = Vec::with_capacity(lines.len());
    let mut scratch = Parsed::default();
    for line in lines {
        let b = body(line);
        scratch.clear();
        let msg = ulpf_parse::strip_envelope(b, &mut scratch);
        let has_header = scratch.get(b"syslog_timestamp").is_some() || scratch.get(b"syslog_pri").is_some();
        if has_header {
            headed += 1;
            if example.is_none() && msg.len() < b.len() {
                example = Some(lossy(&b[..b.len() - msg.len()]).trim().to_string());
            }
        }
        stripped.push((has_header, msg, b));
    }
    let non_empty = lines.iter().filter(|l| !body(l).is_empty()).count().max(1);
    let syslog = headed * 5 >= non_empty; // a fifth of the lines carrying a header is a syslog source
    let out = stripped.into_iter().map(|(_, msg, b)| if syslog { msg } else { b }).collect();
    (syslog, if syslog { example } else { None }, out, headed)
}

fn words<'a>(toks: &[Tok<'a>]) -> Vec<&'a [u8]> {
    toks.iter().filter(|t| t.kind == Kind::Word).map(|t| t.text).collect()
}

/// Word-level similarity when both lines have words to compare; otherwise the shape of
/// every non-space token (a headerless access log has almost no bare words).
fn line_similarity(a: &[Tok<'_>], b: &[Tok<'_>]) -> f64 {
    let (wa, wb) = (words(a), words(b));
    if wa.len() >= 2 && wb.len() >= 2 {
        // the message type lives in the first words; a long free-text tail must not
        // outvote them, so the head alone may carry the decision
        let head = |w: &[&[u8]]| w.len().min(6);
        return align::similarity(&wa, &wb).max(align::similarity(&wa[..head(&wa)], &wb[..head(&wb)]));
    }
    let na: Vec<&Tok<'_>> = a.iter().filter(|t| t.kind != Kind::Space).collect();
    let nb: Vec<&Tok<'_>> = b.iter().filter(|t| t.kind != Kind::Space).collect();
    if na.is_empty() && nb.is_empty() {
        return 1.0;
    }
    let k = align::lcs(na.len(), nb.len(), |i, j| token::same_shape(na[i].kind, na[i].text, nb[j].kind, nb[j].text)).len();
    2.0 * k as f64 / (na.len() + nb.len()) as f64
}

struct Cluster {
    seed: usize,
    members: Vec<usize>,
}

/// Every line joins the most similar cluster whose seed it resembles enough, else seeds
/// its own. Seeds never change, so assignment does not erode as members accumulate.
fn assign(toks: &[Vec<Tok<'_>>], params: &Params) -> Vec<Cluster> {
    let mut clusters: Vec<Cluster> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.is_empty() {
            continue;
        }
        let mut best: Option<(usize, f64)> = None;
        for (ci, c) in clusters.iter().enumerate() {
            let sim = line_similarity(t, &toks[c.seed]);
            if sim >= params.similarity && best.is_none_or(|(_, b)| sim > b) {
                best = Some((ci, sim));
            }
        }
        match best {
            Some((ci, _)) => clusters[ci].members.push(i),
            None => clusters.push(Cluster { seed: i, members: vec![i] }),
        }
    }
    clusters
}

/// Builds the candidates for one member set, splitting on keyword slots recursively.
fn candidates_for(members: Vec<usize>, toks: &[Vec<Tok<'_>>], params: &Params, history: Vec<String>, decisions: &mut Vec<String>, out: &mut Vec<Candidate>, depth: usize) {
    let member_toks: Vec<Vec<Tok<'_>>> = members.iter().map(|&m| toks[m].clone()).collect();
    let cols = cluster::consensus(&member_toks);
    let (cols, mut notes) = cluster::presence_rules(cols, members.len(), params);
    let (cols, pre) = cluster::collapse_variable_word_runs(cols, members.len());
    notes.extend(pre);
    let label = history.last().cloned().unwrap_or_default();
    if depth < 4
        && let Some((idx, groups)) = cluster::enum_split(&cols, params)
    {
        let values: Vec<String> = groups.iter().map(|(v, ms)| format!("`{}` ({})", lossy(v), ms.len())).collect();
        decisions.push(format!("{label}: slot after `{}` has {} distinct keyword values {}: split so the words stay constant", cluster::key_before(&cols, idx).unwrap_or_default(), groups.len(), values.join(", ")));
        for (value, group) in groups {
            let mut h = history.clone();
            let group_members: Vec<usize> = group.iter().map(|&g| members[g]).collect();
            h.push(format!("split on `{}` ({} lines)", lossy(&value), group_members.len()));
            candidates_for(group_members, toks, params, h, decisions, out, depth + 1);
        }
        return;
    }
    // a leaf: its presence decisions are the ones that shaped the template
    for n in &notes {
        decisions.push(format!("{label}: {n}"));
    }
    let (cols, notes) = cluster::collapse_word_runs(cols, members.len());
    let (cols, notes2) = cluster::collapse_messy_runs(cols, members.len());
    for n in notes.iter().chain(&notes2) {
        decisions.push(format!("{label}: {n}"));
    }
    let (template, slots) = cluster::shape(&cols, cluster::rare_count(members.len(), params));
    let slot_evidence = slots.iter().map(|s| slot_evidence(s, &cols)).collect();
    out.push(Candidate { template, members, history, slot_evidence });
}

/// Keyword splits happen per cluster, so the same message shape can come out of two
/// clusters twice (`wlan1: connected` from one, `wlan2: connected` from another). Identical
/// patterns merge; patterns that differ in exactly one constant word merge and are
/// re-derived without splitting, so the word becomes a slot.
fn dedupe(candidates: &mut Vec<Candidate>, toks: &[Vec<Tok<'_>>], params: &Params, decisions: &mut Vec<String>) {
    let mut i = 0;
    while i < candidates.len() {
        let mut j = i + 1;
        while j < candidates.len() {
            let (a, b) = (shape_words(&candidates[i].template), shape_words(&candidates[j].template));
            let differing: Option<Vec<usize>> = (a.len() == b.len()).then(|| (0..a.len()).filter(|k| a[*k] != b[*k]).collect());
            // identical, or one differing token with the same slots in it whose constant
            // text is an identifier (`wlan1`/`wlan2`), never a keyword (`in`/`out`: that was a split)
            let mergeable = match &differing {
                Some(d) if d.is_empty() => Some("identical pattern from another cluster"),
                Some(d) if d.len() == 1 => {
                    let (x, y) = (&a[d[0]], &b[d[0]]);
                    let consts = |t: &str| t.replace("{}", "").bytes().filter(|b| b.is_ascii_alphanumeric()).collect::<Vec<u8>>();
                    (x.matches("{}").count() == y.matches("{}").count() && !cluster::keyword_like(&consts(x)) && !cluster::keyword_like(&consts(y))).then_some("pattern differing in one constant word from another cluster")
                }
                _ => None,
            };
            let Some(note) = mergeable else {
                j += 1;
                continue;
            };
            let other = candidates.remove(j);
            let (mine, theirs) = (candidates[i].members.len(), other.members.len());
            let mut members = std::mem::take(&mut candidates[i].members);
            members.extend(other.members);
            members.sort_unstable();
            members.dedup();
            decisions.push(format!("merged templates: {note} ({mine} + {theirs} lines)"));
            let mut history = candidates[i].history.clone();
            history.push(format!("merged with a template built from `{}`: {note}", other.history.last().cloned().unwrap_or_default()));
            let mut rebuilt = Vec::new();
            candidates_for(members, toks, params, history, decisions, &mut rebuilt, 4);
            if let Some(c) = rebuilt.pop() {
                candidates[i] = c;
            }
            // start over on this candidate: its shape changed
            j = i + 1;
        }
        i += 1;
    }
}

fn slot_evidence(s: &cluster::Slot, cols: &[Col]) -> SlotEvidence {
    let distinct = cols[s.col].distinct();
    SlotEvidence {
        name: s.name.clone(),
        kind: s.kind.name().to_string(),
        suggested: s.suggested,
        reason: s.reason.clone(),
        preceded_by: s.preceded_by.clone(),
        distinct: distinct.len() as u64,
        examples: distinct.iter().take(3).map(|(v, _)| lossy(v)).collect(),
    }
}

/// Compiles one body pattern into a runnable parser exactly as the runtime would (the
/// envelope is already stripped from the bodies it is tested on).
fn compile_pattern(pattern: &str) -> Result<Parser, String> {
    let def = ParserDefinition {
        parser: Meta { name: "candidate".into(), vendor: "x".into(), product: "x".into(), description: None, version: 1, origin: None },
        matcher: Matcher { contains: vec![], starts_with: None, regex: Some(".".into()), priority: 0 },
        envelope: Envelope { syslog: false },
        strategy: Strategy::pattern(pattern),
        timestamp: vec![],
        sub: vec![],
    };
    Parser::from_definition(def)
}

/// Specificity for ordering: required constant text only. Optional constants weigh
/// nothing, or a template made of optional groups would outrank the specific ones and
/// take their lines first.
fn constant_chars(t: &Template) -> usize {
    t.tokens.iter().map(|tok| match tok {
        ulpf_parse::Token::Const(s) => s.trim().len(),
        _ => 0,
    }).sum()
}

/// Template shape for dedupe: constants as text, every slot as `{}`, optional groups
/// flattened, split on spaces. Built from the tokens, not by re-parsing pattern text.
fn shape_words(t: &Template) -> Vec<String> {
    fn push(tokens: &[ulpf_parse::Token], out: &mut String) {
        for tok in tokens {
            match tok {
                ulpf_parse::Token::Const(s) => out.push_str(s),
                ulpf_parse::Token::Slot { .. } => out.push_str("{}"),
                ulpf_parse::Token::Optional(inner) => push(inner, out),
            }
        }
    }
    let mut s = String::new();
    push(&t.tokens, &mut s);
    s.split(' ').map(str::to_string).collect()
}

fn fnv(parts: &[String]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Words present in nearly every line make a `contains` signature; otherwise the
/// templates' leading constants form a `regex` alternation. Priority -1: a generated
/// parser never takes an event from a hand-written one.
fn matcher(toks: &[Vec<Tok<'_>>], templates: &[&Template], decisions: &mut Vec<String>) -> Matcher {
    let used: Vec<usize> = (0..toks.len()).filter(|i| !toks[*i].is_empty()).collect();
    // Words seen per line: bare words plus the words inside quoted strings; atoms (and
    // therefore month names inside timestamps) are skipped, so a one-day file cannot make
    // `Sep` the signature.
    let mut freq: BTreeMap<&[u8], usize> = BTreeMap::new();
    for &i in &used {
        let mut seen: Vec<&[u8]> = Vec::new();
        for t in &toks[i] {
            let candidates: Vec<&[u8]> = match t.kind {
                Kind::Word => vec![t.text],
                Kind::Quoted => t.text.split(|b| !(b.is_ascii_alphanumeric() || *b == b'_')).collect(),
                _ => vec![],
            };
            for w in candidates {
                if w.len() >= 3 && w.iter().any(u8::is_ascii_alphabetic) && !seen.contains(&w) {
                    seen.push(w);
                    *freq.entry(w).or_default() += 1;
                }
            }
        }
    }
    let need = (used.len() as f64 * 0.98).ceil() as usize;
    if let Some((w, n)) = freq.iter().filter(|(_, n)| **n >= need).max_by_key(|(w, _)| w.len()) {
        decisions.push(format!("signature: `{}` appears in {n}/{} lines, used as contains", lossy(w), used.len()));
        return Matcher { contains: vec![lossy(w)], starts_with: None, regex: None, priority: -1 };
    }
    let mut alts: Vec<String> = Vec::new();
    for t in templates {
        let lead = match t.tokens.first() {
            Some(ulpf_parse::Token::Const(s)) => s.trim().to_string(),
            _ => String::new(),
        };
        let alt = if lead.len() >= 3 {
            lead
        } else {
            // first constant word anywhere in the template
            t.tokens.iter().find_map(|tok| if let ulpf_parse::Token::Const(s) = tok { s.split_whitespace().find(|w| w.len() >= 3).map(str::to_string) } else { None }).unwrap_or_default()
        };
        if !alt.is_empty() && !alts.contains(&alt) {
            alts.push(alt);
        }
    }
    if alts.is_empty() {
        decisions.push("signature: no constant text shared or leading; matcher is `regex = \".\"` (matches anything not claimed by another parser)".into());
        return Matcher { contains: vec![], starts_with: None, regex: Some(".".into()), priority: -1 };
    }
    let re = format!("(?:{})", alts.iter().map(|a| regex::escape(a)).collect::<Vec<_>>().join("|"));
    decisions.push(format!("signature: no word in 98% of lines; regex over {} template leads", alts.len()));
    Matcher { contains: vec![], starts_with: None, regex: Some(re), priority: -1 }
}

pub fn slug(source: &str) -> String {
    let stem = source.rsplit('/').next().unwrap_or(source);
    let stem = stem.strip_suffix(".log").or_else(|| stem.strip_suffix(".txt")).unwrap_or(stem);
    let mut s: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "source".into() } else { s }
}

/// Runs inference over the unknown lines of one source. Lines are whole events as
/// stored (terminators included). Never fails: a source with nothing usable yields a
/// proposal with zero templates, which the engine does not write.
pub fn infer(source: &str, lines: &[&[u8]], params: &Params) -> Proposal {
    let mut decisions = Vec::new();
    let (syslog, example_header, bodies, headed) = bodies(lines);
    decisions.push(format!("envelope: syslog header on {headed} of {} lines -> syslog = {syslog} (a fifth is enough)", lines.len()));
    let mut toks: Vec<Vec<Tok<'_>>> = bodies.iter().map(|b| token::tokenize(b)).collect();
    let mut too_long: Vec<usize> = Vec::new();
    for (i, t) in toks.iter_mut().enumerate() {
        if t.len() > MAX_TOKENS {
            too_long.push(i);
            t.clear();
        }
    }
    let clusters = assign(&toks, params);
    decisions.push(format!("clustering: {} lines into {} clusters at similarity {}", toks.iter().filter(|t| !t.is_empty()).count(), clusters.len(), params.similarity));

    let mut unmatched = Unmatched::default();
    let mut unmatched_lines: Vec<usize> = Vec::new();
    let note_unmatched = |i: usize, reason: &str, unmatched: &mut Unmatched, unmatched_lines: &mut Vec<usize>| {
        unmatched.count += 1;
        *unmatched.by_reason.entry(reason.to_string()).or_default() += 1;
        if unmatched.examples.len() < 20 {
            unmatched.examples.push(lossy(bodies[i]));
        }
        unmatched_lines.push(i);
    };
    for (i, t) in toks.iter().enumerate() {
        if too_long.contains(&i) {
            note_unmatched(i, "too_long", &mut unmatched, &mut unmatched_lines);
        } else if t.is_empty() {
            note_unmatched(i, "empty", &mut unmatched, &mut unmatched_lines);
        }
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut small = 0usize;
    for (ci, c) in clusters.into_iter().enumerate() {
        if c.members.len() < params.min_support {
            small += 1;
            for m in c.members {
                note_unmatched(m, "below_support", &mut unmatched, &mut unmatched_lines);
            }
            continue;
        }
        let label = format!("cluster {} ({} lines, seed `{}`)", ci + 1, c.members.len(), lossy(bodies[c.seed]).chars().take(40).collect::<String>());
        candidates_for(c.members, &toks, params, vec![label], &mut decisions, &mut candidates, 0);
    }
    if small > 0 {
        decisions.push(format!("{small} clusters below min_support {} sent to unmatched", params.min_support));
    }
    dedupe(&mut candidates, &toks, params, &mut decisions);
    // specific before general: more constant text first, then support
    candidates.sort_by(|a, b| constant_chars(&b.template).cmp(&constant_chars(&a.template)).then_with(|| b.members.len().cmp(&a.members.len())));
    if candidates.len() > params.max_templates {
        let dropped: Vec<Candidate> = candidates.drain(params.max_templates..).collect();
        decisions.push(format!("{} templates beyond max_templates {} sent to unmatched", dropped.len(), params.max_templates));
        for c in dropped {
            for m in c.members {
                note_unmatched(m, "template_cap", &mut unmatched, &mut unmatched_lines);
            }
        }
    }

    // Verify: every line against the compiled patterns in emitted order, like the runtime.
    // Only candidates the definition will hold take part, so `verified` describes the
    // file that will be approved, not a superset of it.
    let compiled: Vec<Result<Parser, String>> = candidates.iter().map(|c| compile_pattern(&c.template.to_pattern())).collect();
    let eligible: Vec<bool> = candidates.iter().enumerate().map(|(k, c)| compiled[k].is_ok() && c.members.len() >= params.min_support).collect();
    let mut verified = vec![0u64; candidates.len()];
    let mut scratch = Scratch::default();
    let mut parsed = Parsed::default();
    let ctx = ulpf_parse::Context { receipt_epoch_nanos: 0, default_offset_secs: 0 };
    let mut is_unmatched = vec![false; bodies.len()];
    for &i in &unmatched_lines {
        is_unmatched[i] = true;
    }
    for (i, b) in bodies.iter().enumerate() {
        if toks[i].is_empty() || is_unmatched[i] {
            continue;
        }
        let hit = compiled.iter().enumerate().position(|(k, p)| eligible[k] && p.as_ref().is_ok_and(|p| p.parse(b, &ctx, &mut scratch, &mut parsed).is_ok()));
        match hit {
            Some(k) => verified[k] += 1,
            None => note_unmatched(i, "no_template", &mut unmatched, &mut unmatched_lines),
        }
    }
    for (k, c) in candidates.iter().enumerate() {
        match &compiled[k] {
            Err(e) => decisions.push(format!("template {} does not compile and is left out: {e}: `{}`", k + 1, c.template.to_pattern())),
            Ok(_) if !eligible[k] => {}
            Ok(_) if verified[k] as usize > c.members.len() => decisions.push(format!("template {} took {} lines first, {} more than its own cluster (it is more general than a later template)", k + 1, verified[k], verified[k] as usize - c.members.len())),
            Ok(_) if (verified[k] as usize) < c.members.len() => decisions.push(format!("template {} verified {}/{} of its own lines (the rest matched an earlier template or are in unmatched)", k + 1, verified[k], c.members.len())),
            Ok(_) => {}
        }
    }

    let templates: Vec<TemplateEvidence> = candidates
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let mut history = c.history.clone();
            if compiled[k].is_err() {
                history.push("not in the definition: the pattern does not compile (see decisions)".into());
            } else if verified[k] == 0 && eligible[k] {
                history.push("not in the definition: every line it covers matched an earlier template".into());
            } else if c.members.len() < params.min_support {
                history.push(format!("not in the definition: {} lines after a split is below min_support {} (keep it in the review screen if it is a real message type)", c.members.len(), params.min_support));
            }
            TemplateEvidence {
                id: k as u64 + 1,
                pattern: c.template.to_pattern(),
                support: c.members.len() as u64,
                verified: verified[k],
                examples: c.members.iter().take(3).map(|&m| lossy(bodies[m])).collect(),
                members: c.members.iter().map(|&m| m as u32).collect(),
                slots: c.slot_evidence.clone(),
                history,
            }
        })
        .collect();
    let in_definition = |t: &TemplateEvidence| t.verified > 0 && t.support as usize >= params.min_support && compiled[t.id as usize - 1].is_ok();
    let left_out = templates.iter().filter(|t| !in_definition(t)).count();
    if left_out > 0 {
        decisions.push(format!("{left_out} templates left out of the definition (no compile, matched no line first, or below min_support after a split); they stay in the evidence"));
    }
    let patterns: Vec<String> = templates.iter().filter(|t| in_definition(t)).map(|t| t.pattern.clone()).collect();
    let tpl_refs: Vec<&Template> = candidates.iter().map(|c| &c.template).collect();
    let matcher = matcher(&toks, &tpl_refs, &mut decisions);
    // only a pattern that reaches the definition can produce the field the spec names
    let has_ts = patterns.iter().any(|p| p.contains(":timestamp}"));
    let name = format!("{}_inferred", slug(source));
    let definition = ParserDefinition {
        parser: Meta {
            name,
            vendor: "unknown".into(),
            product: source.to_string(),
            description: Some(format!("Inferred from {} unknown lines of {source}; review every slot name before trusting.", lines.len())),
            version: 1,
            origin: Some("inferred".into()),
        },
        matcher,
        envelope: Envelope { syslog },
        strategy: Strategy { kind: StrategyKind::Pattern, patterns, ..Default::default() },
        timestamp: if has_ts && !syslog { vec![TimestampSpec { field: Some("timestamp".into()), fields: vec![], format: "auto".into() }] } else { vec![] },
        sub: vec![],
    };
    let fingerprint = fnv(&definition.strategy.patterns);
    let evidence = Evidence {
        source: source.to_string(),
        lines_seen: lines.len() as u64,
        lines_used: (lines.len() as u64).saturating_sub(unmatched.count),
        params: params.clone(),
        envelope: EnvelopeEvidence { syslog, example_header },
        templates,
        unmatched,
        decisions,
        fingerprint,
    };
    Proposal { source: source.to_string(), definition, evidence, updates: None }
}

/// Drift: the source had an established parser and its lines stopped matching. The
/// prior's own parser is run over the lines first; what it covers is not drift. The rest
/// is clustered as usual, and the result is composed onto the prior: a pattern prior gets
/// the new patterns appended (prior patterns first, so its behaviour on old lines cannot
/// change); any prior whose strategy still parses at least 90% of the lines when the
/// signature is bypassed gets its matcher widened; otherwise the proposal stands alone
/// and says why. Every step is a decision line.
pub fn infer_with_prior(source: &str, lines: &[&[u8]], prior: &ParserDefinition, params: &Params) -> Proposal {
    let mut decisions = vec![format!("prior: `{}` v{} ({} strategy, priority {}) is this source's established parser", prior.parser.name, prior.parser.version, prior.strategy.kind.name(), prior.matcher.priority)];
    let (mut detected, mut parsed_ok, mut covered) = (0usize, 0usize, 0usize);
    let mut remaining: Vec<&[u8]> = Vec::new();
    // original positions of the drift lines: template members must index the lines file
    // the pending record is written with, which holds every buffered line
    let mut positions: Vec<u32> = Vec::new();
    match Parser::from_definition(prior.clone()) {
        Ok(p) => {
            let reg = Registry::new(vec![p]);
            let p = reg.get(0);
            let ctx = Context { receipt_epoch_nanos: 0, default_offset_secs: 0 };
            let mut scratch = reg.scratch();
            let mut parsed = Parsed::default();
            for (pos, l) in lines.iter().enumerate() {
                let det = reg.detect(l, None).is_some();
                let ok = p.parse(l, &ctx, &mut scratch, &mut parsed).is_ok();
                detected += det as usize;
                parsed_ok += ok as usize;
                if det && ok {
                    covered += 1;
                } else {
                    remaining.push(l);
                    positions.push(pos as u32);
                }
            }
        }
        Err(e) => {
            decisions.push(format!("prior does not compile ({e}); treated as absent"));
            remaining = lines.to_vec();
            positions = (0..lines.len() as u32).collect();
        }
    }
    decisions.push(format!("prior covers {covered} of {} lines (signature matched {detected}, strategy parsed {parsed_ok}); {} lines are drift", lines.len(), remaining.len()));
    let mut p = infer(source, &remaining, params);
    for t in &mut p.evidence.templates {
        for m in &mut t.members {
            *m = positions.get(*m as usize).copied().unwrap_or(*m);
        }
    }
    let mut tail = std::mem::take(&mut p.evidence.decisions);
    decisions.append(&mut tail);
    p.evidence.decisions = decisions;
    p.evidence.lines_seen = lines.len() as u64;
    let parse_rate = if lines.is_empty() { 0.0 } else { parsed_ok as f64 / lines.len() as f64 };
    let kind = if prior.strategy.kind == StrategyKind::Pattern {
        "patterns_added"
    } else if parse_rate >= 0.9 {
        "matcher_widened"
    } else {
        p.evidence.decisions.push(format!("prior strategy `{}` parses only {:.0}% of the lines with the signature bypassed; this is a new format, so the proposal stands alone", prior.strategy.kind.name(), parse_rate * 100.0));
        return p;
    };
    if p.definition.strategy.patterns.is_empty() && kind == "patterns_added" {
        p.evidence.decisions.push("no new template reached the definition; nothing to add to the prior".into());
        return p;
    }
    let mut def = prior.clone();
    def.parser.version = prior.parser.version + 1;
    def.parser.origin.get_or_insert_with(|| "inferred".into());
    def.parser.description = Some(format!("v{}: updated from {} drifted lines of {source} on top of v{}; review the added patterns and the signature.", def.parser.version, lines.len(), prior.parser.version));
    if kind == "patterns_added" {
        let mut pats = prior.strategy.patterns.clone();
        if let Some(single) = &prior.strategy.pattern {
            pats.insert(0, single.clone());
        }
        let added = p.definition.strategy.patterns.len();
        pats.extend(p.definition.strategy.patterns.iter().cloned());
        def.strategy.patterns = pats;
        def.strategy.pattern = None;
        p.evidence.decisions.push(format!("update: {added} pattern(s) appended after the prior's {}; the prior's patterns are tried first so old lines parse as before", def.strategy.patterns.len() - added));
    } else {
        p.evidence.decisions.push(format!("update: the prior's strategy parses {:.0}% of the lines; only the signature changed", parse_rate * 100.0));
    }
    let generated_is_catch_all = p.definition.matcher.contains.is_empty() && p.definition.matcher.regex.as_deref() == Some(".");
    if detected < lines.len() && generated_is_catch_all {
        p.evidence.decisions.push(format!("signature: the prior's [match] rejected {} of {} lines, but the generated signature is the catch-all `.`; the prior's is kept rather than widened to everything", lines.len() - detected, lines.len()));
    } else if detected < lines.len() {
        def.matcher = union_matcher(&prior.matcher, &p.definition.matcher);
        p.evidence.decisions.push(format!("signature: the prior's [match] rejected {} of {} lines; widened to the union of the prior's signature and the generated one (priority kept at {})", lines.len() - detected, lines.len(), def.matcher.priority));
    }
    if p.evidence.envelope.syslog && !prior.envelope.syslog {
        def.envelope.syslog = true;
        p.evidence.decisions.push("envelope: the drifted lines carry a syslog header the prior did not strip; syslog = true (every header part is optional, so old lines are unaffected)".into());
    }
    p.evidence.fingerprint = fnv(&def.strategy.patterns);
    p.definition = def;
    p.updates = Some(Update { name: prior.parser.name.clone(), current_version: prior.parser.version, kind: kind.to_string() });
    p
}

/// A regex that accepts what either matcher accepts (a superset of each: a `contains`
/// list is ANDed, its first word alone is looser, which is what a union needs).
fn union_matcher(a: &Matcher, b: &Matcher) -> Matcher {
    fn side(m: &Matcher) -> String {
        if let Some(r) = &m.regex {
            return format!("(?:{r})");
        }
        if let Some(w) = m.contains.first() {
            return regex::escape(w);
        }
        if let Some(s) = &m.starts_with {
            return format!("^{}", regex::escape(s));
        }
        ".".into()
    }
    Matcher { contains: vec![], starts_with: None, regex: Some(format!("{}|{}", side(a), side(b))), priority: a.priority }
}

/// One template from a chosen set of lines, with keyword splitting off: the review
/// screen's merge. `syslog` must be the proposal's envelope decision. The result has no
/// id and no members: they are indices into the caller's proposal, which the caller sets.
pub fn merge(lines: &[&[u8]], syslog: bool, params: &Params) -> Option<TemplateEvidence> {
    let bodies: Vec<&[u8]> = lines.iter().map(|l| {
        let b = body(l);
        if syslog {
            let mut p = Parsed::default();
            ulpf_parse::strip_envelope(b, &mut p)
        } else {
            b
        }
    }).collect();
    let toks: Vec<Vec<Tok<'_>>> = bodies.iter().map(|b| token::tokenize(b)).filter(|t| !t.is_empty() && t.len() <= MAX_TOKENS).collect();
    if toks.is_empty() {
        return None;
    }
    let cols = cluster::consensus(&toks);
    let (cols, mut notes) = cluster::presence_rules(cols, toks.len(), params);
    let (cols, notes2) = cluster::collapse_word_runs(cols, toks.len());
    let (cols, notes3) = cluster::collapse_messy_runs(cols, toks.len());
    notes.extend(notes2);
    notes.extend(notes3);
    let (template, slots) = cluster::shape(&cols, cluster::rare_count(toks.len(), params));
    let pattern = template.to_pattern();
    let verified = compile_pattern(&pattern).ok().map(|p| {
        let mut scratch = Scratch::default();
        let mut parsed = Parsed::default();
        let ctx = ulpf_parse::Context { receipt_epoch_nanos: 0, default_offset_secs: 0 };
        bodies.iter().filter(|b| p.parse(b, &ctx, &mut scratch, &mut parsed).is_ok()).count() as u64
    }).unwrap_or(0);
    let mut history = vec![format!("merged from {} lines", toks.len())];
    history.extend(notes);
    Some(TemplateEvidence {
        id: 0,
        pattern,
        support: toks.len() as u64,
        verified,
        examples: bodies.iter().take(3).map(|b| lossy(b)).collect(),
        members: vec![],
        slots: slots.iter().map(|s| slot_evidence(s, &cols)).collect(),
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&[u8]> {
        s.lines().map(str::as_bytes).collect()
    }

    #[test]
    fn optional_field_becomes_a_group_not_a_second_template() {
        let text = "\
<134>Sep  4 10:15:23 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:55, proto TCP (SYN), 203.0.113.9:44321->10.0.0.1:443, len 60
<134>Sep  4 10:15:24 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:66, proto TCP (SYN), 203.0.113.10:44322->10.0.0.1:22, len 52
<134>Sep  4 10:15:25 gw firewall,info input: in:ether2 out:(none), src-mac 00:11:22:33:44:77, proto TCP (SYN), 198.51.100.7:1000->10.0.0.2:443
<134>Sep  4 10:15:26 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:88, proto TCP (SYN), 198.51.100.8:1001->10.0.0.2:443, len 40
<134>Sep  4 10:15:27 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:99, proto TCP (SYN), 198.51.100.9:1002->10.0.0.3:443
<134>Sep  4 10:15:28 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:aa, proto TCP (SYN), 198.51.100.10:1003->10.0.0.3:443, len 44
";
        let p = infer("gw.log", &lines(text), &Params::default());
        assert!(p.definition.envelope.syslog);
        assert_eq!(p.evidence.templates.len(), 1, "{:#?}", p.evidence.decisions);
        let t = &p.evidence.templates[0];
        assert!(t.pattern.contains("{?, len {len:int}}"), "{}", t.pattern);
        assert!(t.pattern.contains("proto TCP (SYN), {src_ip:ipv4}:{src_port:port}->{dst_ip:ipv4}:{dst_port:port}"), "{}", t.pattern);
        assert!(t.pattern.contains("in:{in_interface:word}"), "{}", t.pattern);
        assert!(t.pattern.contains("src-mac {src_mac:mac}"), "{}", t.pattern);
        assert_eq!(t.verified, 6, "{:#?}", p.evidence);
        assert_eq!(p.evidence.unmatched.count, 0);
        let toml = toml::to_string(&p.definition).unwrap();
        ulpf_parse::load_str(std::path::Path::new("gen.toml"), &toml).unwrap();
        assert!(p.definition.matcher.priority < 0);
    }

    #[test]
    fn dispositions_stay_constant_and_free_text_collapses() {
        let text = "\
<38>Sep  4 10:15:23 gw sshd[1201]: Accepted publickey for bob from 203.0.113.9 port 50000 ssh2
<38>Sep  4 10:15:24 gw sshd[1202]: Accepted publickey for alice from 203.0.113.10 port 50001 ssh2
<38>Sep  4 10:15:25 gw sshd[1203]: Failed password for root from 198.51.100.7 port 50002 ssh2
<38>Sep  4 10:15:26 gw sshd[1204]: Failed password for admin from 198.51.100.8 port 50003 ssh2
<38>Sep  4 10:15:27 gw sshd[1205]: Accepted publickey for carol from 203.0.113.11 port 50004 ssh2
<38>Sep  4 10:15:28 gw sshd[1206]: Failed password for bob from 198.51.100.9 port 50005 ssh2
<30>Sep  4 10:15:29 gw system,info,account user bob logged in from 10.0.0.5 via ssh
<30>Sep  4 10:15:30 gw system,info,account user alice logged out from 10.0.0.6 via winbox
<30>Sep  4 10:15:31 gw system,info,account user carol logged in from 10.0.0.7 via web
<30>Sep  4 10:15:32 gw system,info,account user dave logged out from 10.0.0.8 via ssh
<30>Sep  4 10:15:33 gw system,info,account user erin logged in from 10.0.0.9 via ssh
<30>Sep  4 10:15:34 gw system,info,account user frank logged out from 10.0.0.10 via web
<30>Sep  4 10:15:35 gw wireless,info AA:BB:CC:DD:EE:01@wlan1: disconnected, key exchange timeout, signal strength -67
<30>Sep  4 10:15:36 gw wireless,info AA:BB:CC:DD:EE:02@wlan1: disconnected, signal too weak, signal strength -88
<30>Sep  4 10:15:37 gw wireless,info AA:BB:CC:DD:EE:03@wlan1: disconnected, group key exchange timeout, signal strength -70
<30>Sep  4 10:15:38 gw wireless,info AA:BB:CC:DD:EE:04@wlan1: disconnected, extensive data loss, signal strength -71
<30>Sep  4 10:15:39 gw wireless,info AA:BB:CC:DD:EE:05@wlan1: disconnected, received deauth: sending station leaving (3), signal strength -60
";
        let p = infer("gw.log", &lines(text), &Params::default());
        let pats: Vec<&str> = p.evidence.templates.iter().map(|t| t.pattern.as_str()).collect();
        let joined = pats.join("\n");
        assert!(joined.contains("Accepted publickey for"), "{joined}\n{:#?}", p.evidence.decisions);
        assert!(joined.contains("Failed password for"), "{joined}");
        assert!(joined.contains("logged in from"), "{joined}\n{:#?}", p.evidence.decisions);
        assert!(joined.contains("logged out from"), "{joined}");
        assert!(!joined.contains("{word1:word} {word2:word} for"), "disposition swallowed: {joined}");
        let wireless = pats.iter().find(|p| p.contains("wireless")).expect("wireless template");
        assert!(wireless.contains("disconnected, {text1:text}, signal strength"), "{wireless}");
        assert!(wireless.contains("{mac1:mac}@"), "{wireless}");
        assert_eq!(p.evidence.unmatched.count, 0, "{:#?}", p.evidence);
        for t in &p.evidence.templates {
            assert_eq!(t.verified, t.support, "{}", t.pattern);
        }
        let toml = toml::to_string(&p.definition).unwrap();
        ulpf_parse::load_str(std::path::Path::new("gen.toml"), &toml).unwrap();
    }

    #[test]
    fn headerless_format_with_timestamp_slot_gets_a_timestamp_candidate() {
        let text = r#"203.0.113.9 - - [04/Sep/2026:10:15:23 +0000] "GET /index.html HTTP/1.1" 200 5124 "-" "Mozilla/5.0"
203.0.113.10 - bob [04/Sep/2026:10:15:24 +0000] "POST /api/login HTTP/1.1" 302 0 "https://example.com/" "curl/8.0"
198.51.100.7 - - [04/Sep/2026:10:15:25 +0000] "GET /favicon.ico HTTP/1.1" 404 153 "-" "Mozilla/5.0"
2001:db8::7 - - [04/Sep/2026:10:15:26 +0000] "GET /a?b=1 HTTP/2.0" 200 88 "-" "Mozilla/5.0"
198.51.100.9 - alice [04/Sep/2026:10:15:27 +0000] "DELETE /x HTTP/1.1" 403 12 "-" "python-requests/2.31"
"#;
        let p = infer("nginx_access.log", &lines(text), &Params::default());
        assert!(!p.definition.envelope.syslog);
        assert_eq!(p.evidence.templates.len(), 1, "{:#?}", p.evidence);
        let t = &p.evidence.templates[0];
        assert!(t.pattern.starts_with("{src_ip:ip} - {user:word} [{timestamp:timestamp}] {request:quoted} {status_code:int} {bytes:int} {referer:quoted} {user_agent:quoted}"), "{}", t.pattern);
        assert_eq!(p.definition.timestamp.len(), 1);
        assert_eq!(t.verified, 5);
        assert_eq!(p.definition.parser.name, "nginx_access_inferred");
        assert!(p.definition.matcher.contains == vec!["HTTP".to_string()], "{:?}", p.definition.matcher);
    }

    #[test]
    fn junk_and_truncation_land_in_unmatched_with_reasons() {
        let mut text = String::new();
        for i in 0..20 {
            text.push_str(&format!("<134>Sep  4 10:15:{i:02} gw dhcp,info dhcp1 assigned 10.0.0.{} to 00:11:22:33:44:{i:02x}\n", 10 + i));
        }
        text.push_str("<134>Sep  4 10:16:00 gw dhcp,info dhcp1 assigned 10.0.0.9 to 00:11:\n");
        text.push('\n');
        text.push_str("<78>Sep  4 10:17:01 gw CRON[1234]: (root) CMD (run-parts /etc/cron.hourly)\n");
        text.push_str("garbage \u{fffd}\u{fffd}\n");
        let p = infer("gw.log", &lines(&text), &Params::default());
        assert_eq!(p.evidence.templates.len(), 1, "{:#?}", p.evidence);
        assert_eq!(p.evidence.templates[0].verified, 20);
        assert_eq!(p.evidence.unmatched.count, 4, "{:#?}", p.evidence.unmatched);
        assert_eq!(p.evidence.unmatched.by_reason.get("empty"), Some(&1));
        assert_eq!(p.evidence.unmatched.by_reason.get("below_support"), Some(&2));
        assert_eq!(p.evidence.unmatched.by_reason.get("no_template"), Some(&1), "{:#?}", p.evidence);
    }


    /// `(name, kind, suggested, reason)` per slot of the one template `text` produces.
    fn slots(text: &str) -> Vec<(String, String, bool, String)> {
        let p = infer("t.log", &lines(text), &Params::default());
        assert_eq!(p.evidence.templates.len(), 1, "{:#?}", p.evidence);
        p.evidence.templates[0].slots.iter().map(|s| (s.name.clone(), s.kind.clone(), s.suggested, s.reason.clone())).collect()
    }

    #[track_caller]
    fn named(slots: &[(String, String, bool, String)], name: &str, kind: &str, reason_has: &str) {
        let s = slots.iter().find(|s| s.0 == name).unwrap_or_else(|| panic!("no slot `{name}` in {slots:#?}"));
        assert_eq!(s.1, kind, "slot `{name}`");
        assert!(s.2, "slot `{name}` is not marked suggested");
        assert!(s.3.contains(reason_has), "slot `{name}`: reason `{}` lacks `{reason_has}`", s.3);
    }

    #[track_caller]
    fn generic(slots: &[(String, String, bool, String)], name: &str, reason_has: &str) {
        let s = slots.iter().find(|s| s.0 == name).unwrap_or_else(|| panic!("no slot `{name}` in {slots:#?}"));
        assert!(!s.2, "slot `{name}` is marked suggested");
        assert!(s.3.contains(reason_has), "slot `{name}`: reason `{}` lacks `{reason_has}`", s.3);
    }

    #[test]
    fn netfilter_keys_bracketed_rule_and_tcp_flags() {
        let text = "\
<4>Sep  4 10:15:23 gw kernel: [WAN_IN-default-D]IN=eth0 OUT=eth1 SRC=203.0.113.9 DST=10.0.0.1 LEN=60 TTL=51 PROTO=TCP SPT=44321 DPT=443 WINDOW=1221 SYN URGP=0
<4>Sep  4 10:15:24 gw kernel: [WAN_IN-10-A]IN=eth1 OUT=eth0 SRC=203.0.113.10 DST=10.0.0.2 LEN=52 TTL=52 PROTO=TCP SPT=44322 DPT=22 WINDOW=1222 ACK URGP=0
<4>Sep  4 10:15:25 gw kernel: [LAN_IN-default-D]IN=eth0 OUT=eth1 SRC=198.51.100.7 DST=10.0.0.3 LEN=40 TTL=53 PROTO=TCP SPT=1000 DPT=443 WINDOW=1223 RST URGP=0
<4>Sep  4 10:15:26 gw kernel: [LAN_IN-20-A]IN=eth1 OUT=eth0 SRC=198.51.100.8 DST=10.0.0.4 LEN=44 TTL=54 PROTO=TCP SPT=1001 DPT=80 WINDOW=1224 PSH URGP=0
";
        let s = slots(text);
        named(&s, "rule", "word", "bracketed label");
        named(&s, "in_interface", "word", "IN={word}");
        named(&s, "out_interface", "word", "OUT={word}");
        named(&s, "src_ip", "ipv4", "SRC={ip}");
        named(&s, "dst_ip", "ipv4", "DST={ip}");
        named(&s, "src_port", "port", "SPT={port}");
        named(&s, "dst_port", "port", "DPT={port}");
        named(&s, "len", "int", "len {int}");
        named(&s, "ttl", "int", "TTL={int}");
        named(&s, "tcp_flags", "word", "TCP flag mnemonics");
        // an unknown key still names its value, and says so
        named(&s, "window", "int", "key `window` before the value");
    }

    #[test]
    fn address_pair_around_the_arrow_and_icmp_type_code() {
        let text = "\
<134>Sep  4 10:15:23 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:55, proto ICMP (type=8, code=0), 203.0.113.9->10.0.0.1, len 60
<134>Sep  4 10:15:24 gw firewall,info input: in:ether2 out:(none), src-mac 00:11:22:33:44:66, proto ICMP (type=0, code=1), 203.0.113.10->10.0.0.2, len 52
<134>Sep  4 10:15:25 gw firewall,info input: in:ether3 out:(none), src-mac 00:11:22:33:44:77, proto ICMP (type=3, code=0), 198.51.100.7->10.0.0.3, len 40
<134>Sep  4 10:15:26 gw firewall,info input: in:ether1 out:(none), src-mac 00:11:22:33:44:88, proto ICMP (type=11, code=1), 198.51.100.8->10.0.0.4, len 44
";
        let s = slots(text);
        named(&s, "src_ip", "ipv4", "names the pair src/dst");
        named(&s, "dst_ip", "ipv4", "names the pair src/dst");
        named(&s, "icmp_type", "int", "in an ICMP line");
        named(&s, "icmp_code", "int", "in an ICMP line");
        named(&s, "src_mac", "mac", "key `src-mac` before the value");
        named(&s, "in_interface", "word", "in:{word}");
    }

    #[test]
    fn ports_attached_to_the_addresses_of_a_pair() {
        let text = "\
<134>Sep  4 10:15:23 gw firewall,info input: in:ether1 out:(none), proto UDP, 203.0.113.9:44321->10.0.0.1:443, len 60
<134>Sep  4 10:15:24 gw firewall,info input: in:ether2 out:(none), proto UDP, 203.0.113.10:44322->10.0.0.2:22, len 52
<134>Sep  4 10:15:25 gw firewall,info input: in:ether3 out:(none), proto UDP, 198.51.100.7:1000->10.0.0.3:443, len 40
<134>Sep  4 10:15:26 gw firewall,info input: in:ether1 out:(none), proto UDP, 198.51.100.8:1001->10.0.0.4:80, len 44
";
        let s = slots(text);
        named(&s, "src_ip", "ipv4", "{ip}:{port}->{ip}:{port}");
        named(&s, "src_port", "port", "{ip}:{port}->{ip}:{port}");
        named(&s, "dst_ip", "ipv4", "{ip}:{port}->{ip}:{port}");
        named(&s, "dst_port", "port", "{ip}:{port}->{ip}:{port}");
    }

    #[test]
    fn sshd_pid_user_and_the_port_of_the_from_address() {
        let text = "\
<38>Sep  4 10:15:23 gw sshd[1201]: Accepted publickey for bob from 203.0.113.9 port 50000 ssh2
<38>Sep  4 10:15:24 gw sshd[1202]: Accepted publickey for alice from 203.0.113.10 port 50001 ssh2
<38>Sep  4 10:15:25 gw sshd[1203]: Accepted publickey for carol from 198.51.100.7 port 50002 ssh2
<38>Sep  4 10:15:26 gw sshd[1204]: Accepted publickey for dave from 198.51.100.8 port 50003 ssh2
";
        let s = slots(text);
        named(&s, "pid", "int", "{word}[{int}]:");
        named(&s, "user", "word", "for {word} from");
        named(&s, "src_ip", "ipv4", "from {ip}");
        named(&s, "src_port", "port", "from {ip} port {port}");
    }

    #[test]
    fn dhcpd_macs_and_the_words_that_name_nothing() {
        let text = "\
<30>Sep  4 10:15:23 gw dhcpd: DHCPACK on 10.0.0.5 to 23:af:69:b3:6d:91 via eth1
<30>Sep  4 10:15:24 gw dhcpd: DHCPACK on 10.0.0.6 to 23:af:69:b3:6d:92 via eth2
<30>Sep  4 10:15:25 gw dhcpd: DHCPACK on 10.0.0.7 to 23:af:69:b3:6d:93 via eth1
<30>Sep  4 10:15:26 gw dhcpd: DHCPACK on 10.0.0.8 to 23:af:69:b3:6d:94 via eth2
";
        let s = slots(text);
        named(&s, "dst_mac", "mac", "to {mac}");
        named(&s, "via", "word", "via {word}");
        generic(&s, "ip1", "`on` before the slot is a connective");
    }

    #[test]
    fn a_preceding_constant_names_a_slot_but_a_syslog_tag_does_not() {
        let text = "\
<30>Sep  4 10:15:23 gw watchdog: alice reconnected after 5 seconds
<30>Sep  4 10:15:24 gw watchdog: bob reconnected after 6 seconds
<30>Sep  4 10:15:25 gw watchdog: carol reconnected after 7 seconds
<30>Sep  4 10:15:26 gw watchdog: dave reconnected after 8 seconds
";
        let s = slots(text);
        named(&s, "after", "int", "constant `after` before an int slot");
        generic(&s, "word1", "`watchdog:` is a syslog tag, not a key");
    }

    #[test]
    fn the_ncsa_combined_format_names_every_field_by_position() {
        let text = r#"203.0.113.9 - - [04/Sep/2026:10:15:23 +0000] "GET /index.html HTTP/1.1" 200 5124 "-" "Mozilla/5.0"
203.0.113.10 - bob [04/Sep/2026:10:15:24 +0000] "POST /api/login HTTP/1.1" 302 12 "https://example.com/" "curl/8.0"
198.51.100.7 - - [04/Sep/2026:10:15:25 +0000] "GET /favicon.ico HTTP/1.1" 404 153 "-" "Mozilla/5.0"
198.51.100.9 - alice [04/Sep/2026:10:15:27 +0000] "DELETE /x HTTP/1.1" 403 12 "-" "python-requests/2.31"
"#;
        let s = slots(text);
        for (name, kind) in [
            ("src_ip", "ipv4"), ("user", "word"), ("timestamp", "timestamp"), ("request", "quoted"),
            ("status_code", "int"), ("bytes", "int"), ("referer", "quoted"), ("user_agent", "quoted"),
        ] {
            named(&s, name, kind, "NCSA");
        }
    }

    #[test]
    fn a_second_address_pair_stays_generic_and_says_why() {
        let text = "\
<134>Sep  4 10:15:23 gw firewall,info forward: in:ether1, 10.0.0.1:1000->1.1.1.1:80, NAT (10.0.0.1:1000->1.1.1.1:80)->2.2.2.2:80, len 60
<134>Sep  4 10:15:24 gw firewall,info forward: in:ether2, 10.0.0.2:1001->1.1.1.2:81, NAT (10.0.0.2:1001->1.1.1.2:81)->2.2.2.3:81, len 52
<134>Sep  4 10:15:25 gw firewall,info forward: in:ether3, 10.0.0.3:1002->1.1.1.3:82, NAT (10.0.0.3:1002->1.1.1.3:82)->2.2.2.4:82, len 40
<134>Sep  4 10:15:26 gw firewall,info forward: in:ether1, 10.0.0.4:1003->1.1.1.4:83, NAT (10.0.0.4:1003->1.1.1.4:83)->2.2.2.5:83, len 44
";
        let s = slots(text);
        named(&s, "src_ip", "ipv4", "names the pair src/dst");
        named(&s, "dst_port", "port", "names the pair src/dst");
        generic(&s, "ip1", "a second address pair on the line");
        generic(&s, "port1", "a second address pair on the line");
    }

    #[test]
    fn nothing_usable_yields_no_templates() {
        let p = infer("x", &lines("a\nb\nc\n"), &Params::default());
        assert!(p.evidence.templates.is_empty());
        assert_eq!(p.evidence.unmatched.count, 3);
        assert_eq!(slug("/var/log/My Router-1.log"), "my_router_1");
    }
}
