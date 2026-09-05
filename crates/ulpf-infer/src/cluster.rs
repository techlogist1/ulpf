//! From member lines to one template. A cluster's lines are aligned progressively against
//! a growing pivot of columns; every column remembers which members carry it and what
//! each member had there. Presence decides optional groups, disagreement decides slots,
//! and the values decide the slot type. Nothing here is incremental in the erosion sense:
//! the template is derived once from all members, after assignment.

use std::collections::BTreeMap;
use std::ops::Range;

use ulpf_parse::{SlotKind, Template, Token};

use crate::align::align;
use crate::token::{Kind, Tok, ip_like, num_like, same_shape};

pub struct Col {
    pub kind: Kind,
    /// The seed's text: the constant, or the first value of a slot.
    pub text: Vec<u8>,
    pub present: Vec<bool>,
    /// A substitution was seen here: two members had different constants.
    pub variable: bool,
    /// Several pivot tokens collapsed into one free-text region.
    pub region: bool,
    /// `(member, kind, text)` per member that had a value here; constants that never
    /// varied record nothing and `value_for` answers with `text`.
    pub values: Vec<(usize, Kind, Vec<u8>)>,
    pub optional: bool,
}

impl Col {
    fn from_tok(t: &Tok, member: usize, n: usize) -> Col {
        let mut present = vec![false; n];
        present[member] = true;
        let values = if t.kind.is_variable() { vec![(member, t.kind, t.text.to_vec())] } else { vec![] };
        Col { kind: t.kind, text: t.text.to_vec(), present, variable: false, region: false, values, optional: false }
    }

    pub fn is_slot(&self) -> bool {
        self.variable || self.region || self.kind.is_variable()
    }

    pub fn present_count(&self) -> usize {
        self.present.iter().filter(|p| **p).count()
    }

    pub fn value_for(&self, m: usize) -> Option<&[u8]> {
        if !self.present[m] {
            return None;
        }
        self.values.iter().find(|(i, _, _)| *i == m).map(|(_, _, t)| t.as_slice()).or(Some(&self.text))
    }

    fn kind_for(&self, m: usize) -> Kind {
        self.values.iter().find(|(i, _, _)| *i == m).map(|(_, k, _)| *k).unwrap_or(self.kind)
    }

    /// Alignment weight against a member token: 0 no match; 1 a slot accepting a token of
    /// its family; 2 a typed match (same atom family, quoted, chain, space); 3 an exact constant.
    fn weight(&self, t: &Tok) -> u16 {
        if self.region {
            return 0;
        }
        if self.variable {
            return match (self.kind, t.kind) {
                (_, Kind::Space) | (Kind::Space, _) => 0,
                (Kind::Atom(_), Kind::Atom(_)) => 1,
                (Kind::Atom(_), _) | (_, Kind::Atom(_)) => 0,
                (Kind::Chain, Kind::Chain) => 1,
                (Kind::Chain, _) | (_, Kind::Chain) => 0,
                _ => 1,
            };
        }
        if !same_shape(self.kind, &self.text, t.kind, t.text) {
            return 0;
        }
        match self.kind {
            Kind::Word | Kind::Punct => 3,
            Kind::Space => 1,
            _ => 2,
        }
    }

    /// Distinct values with counts, most common first.
    pub fn distinct(&self) -> Vec<(Vec<u8>, usize)> {
        let mut counts: BTreeMap<&[u8], usize> = BTreeMap::new();
        for m in 0..self.present.len() {
            if let Some(v) = self.value_for(m) {
                *counts.entry(v).or_default() += 1;
            }
        }
        let mut v: Vec<(Vec<u8>, usize)> = counts.into_iter().map(|(k, c)| (k.to_vec(), c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
}

/// Aligns every member onto the first and returns the columns.
pub fn consensus(members: &[Vec<Tok<'_>>]) -> Vec<Col> {
    let n = members.len();
    let mut cols: Vec<Col> = members[0].iter().map(|t| Col::from_tok(t, 0, n)).collect();
    for (m, toks) in members.iter().enumerate().skip(1) {
        cols = align_member(cols, toks, m, n);
    }
    cols
}

fn align_member(cols: Vec<Col>, toks: &[Tok<'_>], m: usize, n: usize) -> Vec<Col> {
    let ncols = cols.len();
    let pairs = align(ncols, toks.len(), |i, j| cols[i].weight(&toks[j]), |i, j| cols[i].kind != Kind::Space && !cols[i].region && toks[j].kind != Kind::Space);
    let mut cols: Vec<Option<Col>> = cols.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(ncols + 4);
    let (mut pi, mut mj) = (0, 0);
    for (i, j) in pairs.into_iter().chain(std::iter::once((ncols, toks.len()))) {
        gap(&mut cols, pi..i, &toks[mj..j], m, n, &mut out);
        if i < ncols {
            let mut c = cols[i].take().expect("column consumed once");
            c.present[m] = true;
            if c.is_slot() {
                c.values.push((m, toks[j].kind, toks[j].text.to_vec()));
            }
            out.push(c);
        }
        pi = i + 1;
        mj = j + 1;
    }
    out
}

fn join(toks: &[Tok<'_>]) -> Vec<u8> {
    toks.iter().flat_map(|t| t.text.iter().copied()).collect()
}

/// One gap between two anchors: pivot columns `p` against member tokens `mt`.
fn gap(cols: &mut [Option<Col>], p: Range<usize>, mt: &[Tok<'_>], m: usize, n: usize, out: &mut Vec<Col>) {
    let pc: Vec<Col> = p.map(|i| cols[i].take().expect("column consumed once")).collect();
    match (pc.is_empty(), mt.is_empty()) {
        (true, true) => {}
        // the member has tokens the pivot lacks: new columns, present for this member only
        (true, false) => out.extend(mt.iter().map(|t| Col::from_tok(t, m, n))),
        // the pivot has tokens the member lacks: they stay, absent for this member
        (false, true) => out.extend(pc),
        (false, false) => {
            if pc.len() == 1 && pc[0].region {
                let mut c = pc.into_iter().next().expect("one column");
                c.present[m] = true;
                c.values.push((m, Kind::Word, join(mt)));
                out.push(c);
            } else if pc.len() == mt.len() && pc.iter().zip(mt).all(|(c, t)| c.kind != Kind::Space && t.kind != Kind::Space) {
                // one-for-one disagreement: each column becomes a slot
                for (mut c, t) in pc.into_iter().zip(mt) {
                    c.variable = true;
                    c.present[m] = true;
                    c.values.push((m, t.kind, t.text.to_vec()));
                    out.push(c);
                }
            } else {
                collapse(pc, mt, m, n, out);
            }
        }
    }
}

/// Several pivot columns against a differently shaped run: one free-text region. A single
/// column against a run is a substitution of its first token instead, so one damaged line
/// (`00:11:` where a MAC belongs) cannot turn a typed column into free text; the type
/// rules decide later whether the odd value is a minority to ignore or a real disagreement.
fn collapse(pc: Vec<Col>, mt: &[Tok<'_>], m: usize, n: usize, out: &mut Vec<Col>) {
    if pc.len() == 1 {
        // `connected` against `disconnected, extensive data loss`: the first token is the
        // substitution, the rest are this member's own inserted columns; keyword
        // splitting can then still see `connected` and `disconnected` as two plain words
        let mut c = pc.into_iter().next().expect("one column");
        let first = mt.iter().position(|t| t.kind != Kind::Space).unwrap_or(0);
        c.variable = true;
        c.present[m] = true;
        c.values.push((m, mt[first].kind, mt[first].text.to_vec()));
        for t in &mt[..first] {
            out.push(Col::from_tok(t, m, n));
        }
        out.push(c);
        out.extend(mt[first + 1..].iter().map(|t| Col::from_tok(t, m, n)));
        return;
    }
    if mt.iter().filter(|t| t.kind != Kind::Space).count() == 1 {
        // the mirror image: `disconnected, extensive data loss` in the pivot against a
        // member's `connected`; the first pivot word takes the substitution and the rest
        // of the run is simply absent for this member
        let tok = mt.iter().find(|t| t.kind != Kind::Space).expect("one token");
        let first = pc.iter().position(|c| c.kind != Kind::Space).unwrap_or(0);
        for (k, mut c) in pc.into_iter().enumerate() {
            if k == first {
                c.variable = true;
                c.present[m] = true;
                c.values.push((m, tok.kind, tok.text.to_vec()));
            }
            out.push(c);
        }
        return;
    }
    let mut present = vec![false; n];
    let mut values = Vec::new();
    for (i, slot) in present.iter_mut().enumerate() {
        let mut joined = Vec::new();
        let mut parts = 0;
        let mut kind = Kind::Word;
        for c in &pc {
            if let Some(v) = c.value_for(i) {
                joined.extend_from_slice(v);
                parts += 1;
                kind = c.kind_for(i);
            }
        }
        if parts > 0 {
            *slot = true;
            values.push((i, if parts == 1 { kind } else { Kind::Word }, joined));
        }
    }
    present[m] = true;
    values.push((m, if mt.len() == 1 { mt[0].kind } else { Kind::Word }, join(mt)));
    out.push(Col { kind: Kind::Word, text: pc[0].text.clone(), present, variable: true, region: true, values, optional: false });
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Params {
    /// Word-level LCS similarity a line needs against a cluster's seed to join it. Loose
    /// on purpose: free-text tails must not fragment, and `enum_split` keeps dispositions
    /// that a loose merge swallowed as constants.
    pub similarity: f64,
    /// Clusters smaller than this produce no template.
    pub min_support: usize,
    /// A word slot with at most this many distinct values is a keyword, not a value: the
    /// cluster is split so the words stay constant.
    pub enum_max: usize,
    /// A token absent from (or present in) fewer than this share of members, and fewer
    /// than two members, is damage or junk rather than an optional field.
    pub rare_share: f64,
    /// Proposals list at most this many templates; the rest go to unmatched.
    pub max_templates: usize,
}

impl Default for Params {
    fn default() -> Params {
        Params { similarity: 0.6, min_support: 3, enum_max: 3, rare_share: 0.05, max_templates: 40 }
    }
}

/// Members below this count are damage or junk, not evidence of an optional field.
pub fn rare_count(n: usize, params: &Params) -> usize {
    2.max((n as f64 * params.rare_share).ceil() as usize)
}

/// Decides optional columns, drops junk, forces rare absences to required. Returns the
/// surviving columns and the decisions taken, as text for the evidence.
pub fn presence_rules(cols: Vec<Col>, n: usize, params: &Params) -> (Vec<Col>, Vec<String>) {
    let rare = rare_count(n, params);
    let mut out = Vec::with_capacity(cols.len());
    let mut notes = Vec::new();
    for mut c in cols {
        let p = c.present_count();
        let absent = n - p;
        if absent == 0 {
            out.push(c);
        } else if p < rare {
            notes.push(format!("dropped `{}`: present in {p}/{n} lines (junk below {rare})", lossy(&c.text)));
        } else if absent < rare {
            notes.push(format!("`{}` required although absent in {absent}/{n} lines (rare absence, damaged lines fail)", lossy(&c.text)));
            out.push(c);
        } else {
            c.optional = true;
            notes.push(format!("`{}` optional: present in {p}/{n} lines", lossy(&c.text)));
            out.push(c);
        }
    }
    (out, notes)
}

fn is_wordish_slot(c: &Col) -> bool {
    c.is_slot() && !matches!(c.kind, Kind::Atom(_) | Kind::Quoted | Kind::Chain) && c.values.iter().all(|(_, k, _)| !matches!(k, Kind::Atom(_) | Kind::Quoted | Kind::Chain))
}

fn is_anchor(c: &Col) -> bool {
    (!c.is_slot() && !c.optional && c.kind != Kind::Space) || matches!(c.kind, Kind::Atom(_) | Kind::Chain | Kind::Quoted) || c.values.iter().any(|(_, k, _)| matches!(k, Kind::Atom(_) | Kind::Chain | Kind::Quoted))
}

/// Between two anchors (a required constant, or a typed column), a stretch of optional
/// and free-text columns is the residue of aligning values that have no fixed shape
/// (`(run-parts /etc/cron.hourly)` beside `(/usr/bin/x)`): one text slot says what it is.
pub fn collapse_messy_runs(mut cols: Vec<Col>, n: usize) -> (Vec<Col>, Vec<String>) {
    let mut notes = Vec::new();
    let mut i = 0;
    while i < cols.len() {
        if is_anchor(&cols[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < cols.len() && !is_anchor(&cols[j]) {
            j += 1;
        }
        // trim required spaces at both edges so the anchors keep their spacing
        let mut a = i;
        let mut b = j;
        while a < b && cols[a].kind == Kind::Space && !cols[a].optional && !cols[a].is_slot() {
            a += 1;
        }
        while b > a && cols[b - 1].kind == Kind::Space && !cols[b - 1].optional && !cols[b - 1].is_slot() {
            b -= 1;
        }
        let wordish = cols[a..b].iter().filter(|c| is_wordish_slot(c)).count();
        // optional punctuation beside a free-text slot is alignment residue; an optional
        // keyword (` MAC=`) or an optional slot on its own is an optional field, which is structure
        let optional = cols[a..b].iter().filter(|c| c.optional && !c.is_slot() && c.kind == Kind::Punct).count();
        let keyword = cols[a..b].iter().any(|c| c.optional && !c.is_slot() && c.kind == Kind::Word);
        if wordish >= 2 || (wordish >= 1 && optional >= 2 && !keyword) {
            let run: Vec<Col> = cols.drain(a..b).collect();
            let mut present = vec![false; n];
            let mut values = Vec::new();
            for (m, slot) in present.iter_mut().enumerate() {
                let mut joined = Vec::new();
                let mut any = false;
                for c in &run {
                    if let Some(v) = c.value_for(m) {
                        joined.extend_from_slice(v);
                        any = true;
                    }
                }
                if any {
                    *slot = true;
                    values.push((m, Kind::Word, joined));
                }
            }
            notes.push(format!("{} columns ({wordish} free-text slots, {optional} optional) between anchors collapsed into one text slot", run.len()));
            let optional = present.iter().any(|p| !p);
            cols.insert(a, Col { kind: Kind::Word, text: run[0].text.clone(), present, variable: true, region: true, values, optional });
            i = a + 1;
        } else {
            i = j.max(i + 1);
        }
    }
    (cols, notes)
}

/// Two or more free-text slots separated only by spaces are one free-text slot: a
/// disconnect reason is one field, not `{word1} {word2} {word3}`.
pub fn collapse_word_runs(cols: Vec<Col>, n: usize) -> (Vec<Col>, Vec<String>) {
    collapse_word_runs_where(cols, n, false)
}

/// Before keyword splitting: only runs whose length varies across members (`ACK PSH`
/// beside `SYN`) collapse, so a flag list becomes one text slot while a fixed pair of
/// words (`Accepted publickey` beside `Failed password`) stays two keyword slots.
pub fn collapse_variable_word_runs(cols: Vec<Col>, n: usize) -> (Vec<Col>, Vec<String>) {
    collapse_word_runs_where(cols, n, true)
}

fn collapse_word_runs_where(mut cols: Vec<Col>, n: usize, only_variable_length: bool) -> (Vec<Col>, Vec<String>) {
    let mut notes = Vec::new();
    let mut i = 0;
    while i < cols.len() {
        if !is_wordish_slot(&cols[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut slots = 1;
        loop {
            // pattern: slot (space slot)*
            let next_space = j + 1 < cols.len() && cols[j + 1].kind == Kind::Space && !cols[j + 1].is_slot();
            let next_slot = j + 2 < cols.len() && is_wordish_slot(&cols[j + 2]);
            if next_space && next_slot {
                j += 2;
                slots += 1;
            } else {
                break;
            }
        }
        if slots >= 2 && (!only_variable_length || cols[i..=j].iter().any(|c| c.optional)) {
            let run: Vec<Col> = cols.drain(i..=j).collect();
            let mut present = vec![false; n];
            let mut values = Vec::new();
            for (m, slot) in present.iter_mut().enumerate() {
                let mut joined = Vec::new();
                let mut any = false;
                for c in &run {
                    if let Some(v) = c.value_for(m) {
                        joined.extend_from_slice(v);
                        any = true;
                    }
                }
                if any {
                    *slot = true;
                    values.push((m, Kind::Word, joined));
                }
            }
            notes.push(format!("{slots} adjacent word slots collapsed into one text slot"));
            let optional = present.iter().any(|p| !p);
            cols.insert(i, Col { kind: Kind::Word, text: run[0].text.clone(), present, variable: true, region: true, values, optional });
        }
        i += 1;
    }
    (cols, notes)
}

const IDENTITY_KEYS: [&str; 20] = [
    "user", "for", "from", "to", "via", "by", "host", "hostname", "name", "client", "server", "cn", "sni", "mac",
    "src-mac", "dst-mac", "remote", "peer", "username", "login",
];

/// Nearest preceding constant word (through a `=` or `:`), lowercase.
pub fn key_before(cols: &[Col], idx: usize) -> Option<String> {
    let mut i = idx;
    let mut through_sep = false;
    while i > 0 {
        i -= 1;
        let c = &cols[i];
        if c.kind == Kind::Space {
            if through_sep {
                continue;
            }
            // `key value`: the word right before the space
            continue;
        }
        if c.is_slot() {
            return None;
        }
        match c.kind {
            Kind::Punct if matches!(c.text.as_slice(), b"=" | b":") && !through_sep => through_sep = true,
            Kind::Punct => return None,
            Kind::Word => return Some(lossy(&c.text).to_lowercase()),
            _ => return None,
        }
    }
    None
}

/// Keywords are plain words: `deny`, `Accepted`, `TCP`, `input`. Anything with digits,
/// dashes or dots (`eth0`, `backup-config`, `3000-0148`) is an identifier, not a message type.
pub fn keyword_like(v: &[u8]) -> bool {
    !v.is_empty() && v.iter().all(u8::is_ascii_alphabetic)
}

/// `(value, member indices)` per distinct value of a keyword slot.
pub type Groups = Vec<(Vec<u8>, Vec<usize>)>;

/// The first slot that is really a keyword: few distinct values, all plain words, not
/// named by an identity key, each seen at least twice. Returns the column and the member
/// groups per value (members without the column form the `absent` group).
pub fn enum_split(cols: &[Col], params: &Params) -> Option<(usize, Groups)> {
    for (idx, c) in cols.iter().enumerate() {
        if !c.is_slot() || c.region || !is_wordish_slot(c) {
            continue;
        }
        let distinct = c.distinct();
        if distinct.len() < 2 || distinct.len() > params.enum_max {
            continue;
        }
        if !distinct.iter().all(|(v, _)| keyword_like(v)) {
            continue;
        }
        if let Some(key) = key_before(cols, idx)
            && IDENTITY_KEYS.contains(&key.as_str())
        {
            continue;
        }
        let mut groups: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
        for m in 0..c.present.len() {
            groups.entry(c.value_for(m).map(<[u8]>::to_vec).unwrap_or_default()).or_default().push(m);
        }
        let mut groups: Groups = groups.into_iter().collect();
        // a keyword seen once is not evidence of a message type; leave the slot alone
        if groups.iter().any(|(_, ms)| ms.len() < 2) {
            continue;
        }
        groups.sort_by_key(|(_, ms)| std::cmp::Reverse(ms.len()));
        return Some((idx, groups));
    }
    None
}

pub struct Slot {
    pub col: usize,
    pub name: String,
    pub kind: SlotKind,
    pub suggested: bool,
    pub preceded_by: String,
    /// Why this name, in one line: the rule that fired, or why none did.
    pub reason: String,
}

/// Families a value can belong to; the dominant family decides the type when the
/// dissenters are fewer than `rare` (damaged lines), otherwise the union rules apply.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    Atom(SlotKind),
    Quoted,
    Word,
    Spaced,
}

fn family(kind: Kind, value: &[u8]) -> Family {
    match kind {
        Kind::Atom(k) => Family::Atom(k),
        Kind::Quoted => Family::Quoted,
        Kind::Chain => Family::Word,
        _ if value.iter().any(|b| *b == b' ' || *b == b'\t') => Family::Spaced,
        _ => Family::Word,
    }
}

fn slot_kind(c: &Col, cols: &[Col], idx: usize, rare: usize) -> SlotKind {
    let n = c.present.len();
    let mut counts: BTreeMap<Family, usize> = BTreeMap::new();
    for m in 0..n {
        if let Some(v) = c.value_for(m) {
            *counts.entry(family(c.kind_for(m), v)).or_default() += 1;
        }
    }
    let all: Vec<Family> = counts.keys().copied().collect();
    let atoms: Vec<SlotKind> = all.iter().filter_map(|f| if let Family::Atom(k) = f { Some(*k) } else { None }).collect();
    // compatible atoms widen for free: one IPv6 client among IPv4 ones is `ip`, not damage
    if atoms.len() == all.len() {
        if atoms.len() == 1 {
            return port_or(atoms[0], c, cols, idx);
        }
        if atoms.iter().all(|k| ip_like(*k)) {
            return SlotKind::Ip;
        }
        if atoms.iter().all(|k| num_like(*k)) {
            return SlotKind::Float;
        }
    }
    let total: usize = counts.values().sum();
    let Some((&dominant, &dom_n)) = counts.iter().max_by_key(|(_, n)| **n) else { return SlotKind::Text };
    let families: Vec<Family> = if total - dom_n < rare { vec![dominant] } else { all };
    if families.contains(&Family::Spaced) || (families.contains(&Family::Quoted) && families.len() > 1) {
        return SlotKind::Text;
    }
    if families == [Family::Quoted] {
        return SlotKind::Quoted;
    }
    if let [Family::Atom(k)] = families.as_slice() {
        return port_or(*k, c, cols, idx);
    }
    SlotKind::Word
}

/// An int slot is a port when every value fits and the context says so: `ip:NNN`, or a
/// key such as `port`, `spt`, `dpt`, `sport`.
fn port_or(k: SlotKind, c: &Col, cols: &[Col], idx: usize) -> SlotKind {
    if k != SlotKind::Int {
        return k;
    }
    let fits = (0..c.present.len()).filter_map(|m| c.value_for(m)).all(|v| lossy(v).parse::<u32>().is_ok_and(|p| p <= 65535));
    if !fits {
        return k;
    }
    let after_ip_colon = idx >= 2 && cols[idx - 1].kind == Kind::Punct && cols[idx - 1].text == b":" && matches!(cols[idx - 2].kind, Kind::Atom(a) if ip_like(a));
    let keyed = key_before(cols, idx).is_some_and(|key| key.ends_with("port") || matches!(key.as_str(), "spt" | "dpt" | "sport" | "dport"));
    if after_ip_colon || keyed { SlotKind::Port } else { k }
}

/// Value families a vocabulary row accepts. Coarser than `SlotKind` so one row covers
/// `ipv4`/`ipv6`/`ip` and `int`/`port`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fam {
    Ip,
    Mac,
    Num,
    Word,
}

fn fam_of(k: SlotKind) -> Fam {
    match k {
        SlotKind::Ip | SlotKind::Ipv4 | SlotKind::Ipv6 => Fam::Ip,
        SlotKind::Mac => Fam::Mac,
        SlotKind::Int | SlotKind::Float | SlotKind::Port => Fam::Num,
        _ => Fam::Word,
    }
}

/// One row of the curated vocabulary: a key a real vendor format writes before a value,
/// and the device-side name that value carries. `docs/slot-vocabulary.md` is the same
/// table with an example line per row. Names here are always the device's vocabulary,
/// never an output-schema field: this crate cannot see `ulpf-normalize` (D38), and each
/// name below is already an alias in `mappings/ocsf.toml` where one exists, so an
/// approved proposal normalizes without a mapping edit.
struct Convention {
    key: &'static str,
    ctx: &'static str,
    fam: Fam,
    name: &'static str,
    source: &'static str,
}

const VOCAB: [Convention; 25] = [
    Convention { key: "from", ctx: "from {ip}", fam: Fam::Ip, name: "src_ip", source: "BSD syslog, OpenSSH sshd" },
    Convention { key: "src", ctx: "SRC={ip}", fam: Fam::Ip, name: "src_ip", source: "netfilter LOG target" },
    Convention { key: "saddr", ctx: "saddr={ip}", fam: Fam::Ip, name: "src_ip", source: "netfilter conntrack, nftables" },
    Convention { key: "source-address", ctx: "source-address={ip}", fam: Fam::Ip, name: "src_ip", source: "Juniper SRX RT_FLOW" },
    Convention { key: "to", ctx: "to {ip}", fam: Fam::Ip, name: "dst_ip", source: "BSD syslog, ISC dhcpd" },
    Convention { key: "dst", ctx: "DST={ip}", fam: Fam::Ip, name: "dst_ip", source: "netfilter LOG target" },
    Convention { key: "daddr", ctx: "daddr={ip}", fam: Fam::Ip, name: "dst_ip", source: "netfilter conntrack, nftables" },
    Convention { key: "destination-address", ctx: "destination-address={ip}", fam: Fam::Ip, name: "dst_ip", source: "Juniper SRX RT_FLOW" },
    Convention { key: "from", ctx: "from {mac}", fam: Fam::Mac, name: "src_mac", source: "ISC dhcpd" },
    Convention { key: "to", ctx: "to {mac}", fam: Fam::Mac, name: "dst_mac", source: "ISC dhcpd" },
    Convention { key: "spt", ctx: "SPT={port}", fam: Fam::Num, name: "src_port", source: "netfilter LOG target" },
    Convention { key: "sport", ctx: "sport={port}", fam: Fam::Num, name: "src_port", source: "OpenBSD pf, Suricata EVE" },
    Convention { key: "dpt", ctx: "DPT={port}", fam: Fam::Num, name: "dst_port", source: "netfilter LOG target" },
    Convention { key: "dport", ctx: "dport={port}", fam: Fam::Num, name: "dst_port", source: "OpenBSD pf, Suricata EVE" },
    Convention { key: "in", ctx: "in:{word} / IN={word}", fam: Fam::Word, name: "in_interface", source: "MikroTik RouterOS, netfilter LOG" },
    Convention { key: "out", ctx: "out:{word} / OUT={word}", fam: Fam::Word, name: "out_interface", source: "MikroTik RouterOS, netfilter LOG" },
    Convention { key: "proto", ctx: "proto {word}", fam: Fam::Word, name: "proto", source: "MikroTik RouterOS, netfilter PROTO=" },
    Convention { key: "protocol", ctx: "protocol {word}", fam: Fam::Word, name: "proto", source: "Cisco ASA, pfSense filterlog" },
    Convention { key: "len", ctx: "len {int}", fam: Fam::Num, name: "len", source: "MikroTik RouterOS, netfilter LEN=" },
    Convention { key: "length", ctx: "length {int}", fam: Fam::Num, name: "len", source: "Cisco ASA, Squid" },
    Convention { key: "ttl", ctx: "TTL={int}", fam: Fam::Num, name: "ttl", source: "netfilter LOG target" },
    Convention { key: "via", ctx: "via {word}", fam: Fam::Word, name: "via", source: "ISC dhcpd, MikroTik login log" },
    Convention { key: "user", ctx: "user {word}", fam: Fam::Word, name: "user", source: "MikroTik RouterOS account log, OpenSSH sshd" },
    Convention { key: "username", ctx: "username={word}", fam: Fam::Word, name: "user", source: "Cisco ASA, FortiGate" },
    Convention { key: "login", ctx: "login={word}", fam: Fam::Word, name: "user", source: "Check Point, SonicWall" },
];

/// Words that precede a value without naming it: English connectives, syslog severity
/// words that sit in a topic list (`wireless,info <mac>`), and protocol keywords.
const STOPWORDS: [&str; 27] = [
    "for", "by", "of", "the", "a", "an", "and", "or", "is", "was", "are", "were", "at", "on", "with", "this", "that",
    "not", "info", "warn", "warning", "error", "debug", "notice", "tcp", "udp", "icmp",
];

/// TCP flag mnemonics as netfilter's LOG target and RouterOS print them.
const TCP_FLAGS: [&str; 9] = ["SYN", "ACK", "FIN", "RST", "PSH", "URG", "ECE", "CWR", "NS"];

fn vocab(key: &str, kind: SlotKind) -> Option<&'static Convention> {
    VOCAB.iter().find(|c| c.key == key && c.fam == fam_of(kind))
}

fn vocab_reason(ctx: &str, name: &str, source: &str) -> String {
    format!("vocabulary: `{ctx}` names {name} ({source})")
}

fn a_kind(k: SlotKind) -> String {
    let n = k.name();
    if matches!(n.as_bytes()[0], b'a' | b'e' | b'i' | b'o' | b'u') { format!("an {n}") } else { format!("a {n}") }
}

/// The name for one slot, with the reason a reviewer reads. `None` means no rule fired
/// and the caller falls back to `kind+n`; the reason then says why it stayed generic.
fn name_slot(cols: &[Col], idx: usize, kind: SlotKind, placed: &Placed) -> (Option<String>, String) {
    if let Some((name, reason)) = placed.get(&idx) {
        return (name.clone(), reason.clone());
    }
    if kind == SlotKind::Timestamp {
        return (Some("timestamp".to_string()), "the slot's own type is a timestamp".to_string());
    }
    let Some(key) = key_before(cols, idx) else {
        return (None, format!("no key or known constant before {} slot", a_kind(kind)));
    };
    // `tag: value` is a syslog tag, not a key: a real key attaches to its value
    if idx >= 2 && cols[idx - 1].kind == Kind::Space && cols[idx - 2].kind == Kind::Punct && cols[idx - 2].text == b":" {
        return (None, format!("`{key}:` is a syslog tag, not a key"));
    }
    if let Some(c) = vocab(&key, kind) {
        return (Some(c.name.to_string()), vocab_reason(c.ctx, c.name, c.source));
    }
    if is_keyed(cols, idx) || key.contains(['-', '_']) {
        return (Some(sanitize(&key)), format!("key `{key}` before the value"));
    }
    if STOPWORDS.contains(&key.as_str()) {
        return (None, format!("`{key}` before the slot is a connective, not a field name"));
    }
    if key.bytes().any(|b| b.is_ascii_alphabetic()) && key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return (Some(sanitize(&key)), format!("constant `{key}` before {} slot", a_kind(kind)));
    }
    (None, format!("no key or known constant before {} slot", a_kind(kind)))
}

/// Names that come from the shape of the line rather than from a key. Runs before the key
/// rules, so `kernel: [{rule}]` is not named after the syslog tag and an address pair
/// beats the `from`/`to` rows. Earlier rules win: entries are inserted, never replaced.
/// The input's own names (a JSON key) come first: they are the device's vocabulary
/// verbatim, so no heuristic, not even the slot's type, outranks them.
fn positional(cols: &[Col], kinds: &[Option<SlotKind>]) -> Placed {
    let slots: Vec<usize> = (0..cols.len()).filter(|i| kinds[*i].is_some()).collect();
    let mut out: Placed = BTreeMap::new();
    json_keys(cols, kinds, &mut out);
    ncsa_combined(cols, &slots, kinds, &mut out);
    address_pair(cols, &slots, kinds, &mut out);
    by_column(cols, kinds, &mut out);
    port_after_address(cols, kinds, &mut out);
    out
}

/// Column index to `(name, reason)`; a `None` name is a rule that explains why the slot
/// stays `kind+n` without proposing one.
type Placed = BTreeMap<usize, (Option<String>, String)>;

fn place(out: &mut Placed, idx: usize, name: &str, reason: &str) {
    out.entry(idx).or_insert_with(|| (Some(name.to_string()), reason.to_string()));
}

fn leave_generic(out: &mut Placed, idx: usize, reason: &str) {
    out.entry(idx).or_insert_with(|| (None, reason.to_string()));
}

/// `"key":{value}` in a JSON object (the tokenizer keeps such a key as a constant word):
/// the key is the device's own name for the value. A nested object names by its innermost
/// key (the path is not tracked, the reason says so); an array's elements take the array's key.
fn json_keys(cols: &[Col], kinds: &[Option<SlotKind>], out: &mut Placed) {
    let punct = |i: usize, t: &[u8]| cols.get(i).is_some_and(|c| !c.is_slot() && c.kind == Kind::Punct && c.text == t);
    let mut depth = 0usize;
    for (idx, kind) in kinds.iter().enumerate() {
        depth = depth.saturating_add(usize::from(punct(idx, b"{"))).saturating_sub(usize::from(punct(idx, b"}")));
        if kind.is_none() {
            continue;
        }
        let element = idx >= 1 && punct(idx - 1, b"[");
        let colon = idx - usize::from(element);
        if colon < 2 || !punct(colon - 1, b":") {
            continue;
        }
        let k = &cols[colon - 2];
        if k.is_slot() || k.kind != Kind::Word || k.text.len() < 3 || k.text[0] != b'"' || k.text[k.text.len() - 1] != b'"' {
            continue;
        }
        let key = lossy(&k.text[1..k.text.len() - 1]);
        let name = sanitize(&key);
        let mut reason = format!("json key `{key}`");
        if element {
            reason.push_str(" (first array element)");
        }
        if depth > 1 {
            reason.push_str(" (innermost key of a nested object)");
        }
        if name != key {
            reason.push_str(&format!(" (written `{name}`)"));
        }
        place(out, idx, &name, &reason);
    }
}

/// `%h %l %u %t "%r" %>s %b "%{Referer}i" "%{User-Agent}i"`: the field order is the
/// format, so position names every slot. The request line stays one quoted slot.
fn ncsa_combined(cols: &[Col], slots: &[usize], kinds: &[Option<SlotKind>], out: &mut Placed) {
    const NAMES: [&str; 8] = ["src_ip", "user", "timestamp", "request", "status_code", "bytes", "referer", "user_agent"];
    const KINDS: [SlotKind; 8] = [
        SlotKind::Ip, SlotKind::Word, SlotKind::Timestamp, SlotKind::Quoted, SlotKind::Int, SlotKind::Int, SlotKind::Quoted, SlotKind::Quoted,
    ];
    let fits = |i: usize| {
        slots.get(i).and_then(|s| kinds[*s]).is_some_and(|k| match KINDS[i] {
            SlotKind::Ip => ip_like(k),
            want => want == k,
        })
    };
    if slots.first() != Some(&0) || !(0..4).all(fits) || const_between(cols, slots[0], slots[1]).trim() != "-" {
        return;
    }
    let reason = vocab_reason(
        r#"{ip} - {user} [{timestamp}] "{request}" {status} {bytes}"#,
        "the NCSA fields",
        "Apache LogFormat combined, nginx log_format combined",
    );
    for (i, name) in NAMES.iter().enumerate().take_while(|(i, _)| fits(*i)) {
        place(out, slots[i], name, &reason);
    }
}

/// `{ip}:{port}->{ip}:{port}`, `{ip} -> {ip}`, `{ip}/{port} -> {ip}/{port}`: the arrow
/// points from source to destination. Only the first pair on a line is named; a second
/// pair (RouterOS logs the translated addresses after `NAT`) is left for the reviewer.
fn address_pair(cols: &[Col], slots: &[usize], kinds: &[Option<SlotKind>], out: &mut Placed) {
    let addrs: Vec<usize> = slots.iter().copied().filter(|i| ip_like(kinds[*i].expect("slot kind"))).collect();
    for w in addrs.windows(2) {
        let (a, b) = (w[0], w[1]);
        let src_port = attached_port(cols, kinds, a).filter(|p| *p < b);
        // only the source port may sit between the two addresses
        if slots.iter().any(|s| *s > a && *s < b && Some(*s) != src_port) {
            continue;
        }
        let sep = const_between(cols, a, b);
        if !(sep.contains("->") || sep.contains("=>") || sep.contains(" > ")) {
            continue;
        }
        let reason = vocab_reason("{ip}:{port}->{ip}:{port}", "the pair src/dst", "MikroTik RouterOS firewall log, Cisco ASA");
        place(out, a, "src_ip", &reason);
        place(out, b, "dst_ip", &reason);
        if let Some(p) = src_port {
            place(out, p, "src_port", &reason);
        }
        if let Some(p) = attached_port(cols, kinds, b) {
            place(out, p, "dst_port", &reason);
        }
        let rest = "a second address pair on the line (RouterOS logs the translated addresses); the first pair took src/dst";
        for later in addrs.iter().copied().filter(|i| *i > b) {
            leave_generic(out, later, rest);
            if let Some(p) = attached_port(cols, kinds, later) {
                leave_generic(out, p, rest);
            }
        }
        return;
    }
}

/// The rules that need only one column and its neighbours.
fn by_column(cols: &[Col], kinds: &[Option<SlotKind>], out: &mut Placed) {
    let icmp = cols.iter().any(|c| !c.is_slot() && c.kind == Kind::Word && lossy(&c.text).to_ascii_uppercase().starts_with("ICMP"));
    for (idx, c) in cols.iter().enumerate() {
        let Some(kind) = kinds[idx] else { continue };
        if fam_of(kind) == Fam::Num && is_pid(cols, idx) {
            place(out, idx, "pid", &vocab_reason("{word}[{int}]:", "pid", "RFC 3164 syslog TAG, RFC 5424 PROCID"));
            continue;
        }
        if icmp && fam_of(kind) == Fam::Num {
            match key_before(cols, idx).as_deref() {
                Some("type") => {
                    place(out, idx, "icmp_type", &vocab_reason("type={int} in an ICMP line", "icmp_type", "RFC 792, netfilter LOG TYPE="));
                    continue;
                }
                Some("code") => {
                    place(out, idx, "icmp_code", &vocab_reason("code={int} in an ICMP line", "icmp_code", "RFC 792, netfilter LOG CODE="));
                    continue;
                }
                _ => {}
            }
        }
        if fam_of(kind) == Fam::Word && all_bracketed(c) {
            place(out, idx, "rule", &vocab_reason("[{word}] bracketed label", "rule", "iptables --log-prefix, EdgeRouter/ufw rule names"));
            continue;
        }
        if fam_of(kind) == Fam::Word && all_tcp_flags(c) {
            place(out, idx, "tcp_flags", &vocab_reason("a run of TCP flag mnemonics", "tcp_flags", "netfilter LOG target, MikroTik `proto TCP (SYN,ACK)`"));
            continue;
        }
        if fam_of(kind) == Fam::Word
            && key_before(cols, idx).as_deref() == Some("for")
            && word_after(cols, idx).as_deref() == Some("from")
        {
            place(out, idx, "user", &vocab_reason("for {word} from", "user", "OpenSSH sshd auth log"));
        }
    }
}

/// `from {ip} port {port}`: the port belongs to the address the same clause named.
fn port_after_address(cols: &[Col], kinds: &[Option<SlotKind>], out: &mut Placed) {
    for idx in 0..cols.len() {
        let Some(kind) = kinds[idx] else { continue };
        if fam_of(kind) != Fam::Num || key_before(cols, idx).as_deref() != Some("port") {
            continue;
        }
        let Some(addr) = (0..idx).rev().find(|i| kinds[*i].is_some_and(ip_like)) else { continue };
        let named = out.get(&addr).and_then(|(n, _)| n.clone()).or_else(|| {
            let key = key_before(cols, addr)?;
            vocab(&key, kinds[addr].expect("slot kind")).map(|c| c.name.to_string())
        });
        let name = match named.as_deref() {
            Some("src_ip") => "src_port",
            Some("dst_ip") => "dst_port",
            _ => continue,
        };
        place(out, idx, name, &vocab_reason("from {ip} port {port}", name, "OpenSSH sshd auth log"));
    }
}

/// True when the slot is the `[1234]` of a `tag[1234]:` syslog header.
fn is_pid(cols: &[Col], idx: usize) -> bool {
    let punct = |i: usize, t: &[u8]| cols.get(i).is_some_and(|c| !c.is_slot() && c.kind == Kind::Punct && c.text == t);
    idx >= 2 && punct(idx - 1, b"[") && cols[idx - 2].kind == Kind::Word && !cols[idx - 2].is_slot() && punct(idx + 1, b"]") && punct(idx + 2, b":")
}

fn all_bracketed(c: &Col) -> bool {
    let d = c.distinct();
    !d.is_empty() && d.iter().all(|(v, _)| v.len() > 2 && v[0] == b'[' && v[v.len() - 1] == b']')
}

fn all_tcp_flags(c: &Col) -> bool {
    let d = c.distinct();
    !d.is_empty()
        && d.iter().all(|(v, _)| {
            let text = lossy(v);
            let mut parts = text.split(|ch: char| !ch.is_ascii_alphabetic()).filter(|p| !p.is_empty()).peekable();
            parts.peek().is_some() && parts.all(|p| TCP_FLAGS.contains(&p.to_ascii_uppercase().as_str()))
        })
}

/// The `{port}` slot right after an address, separated only by `:` or `/`.
fn attached_port(cols: &[Col], kinds: &[Option<SlotKind>], addr: usize) -> Option<usize> {
    let mut sep = false;
    for i in addr + 1..cols.len() {
        if let Some(k) = kinds[i] {
            return (sep && fam_of(k) == Fam::Num).then_some(i);
        }
        if cols[i].kind == Kind::Punct && matches!(cols[i].text.as_slice(), b":" | b"/") && !sep {
            sep = true;
        } else {
            return None;
        }
    }
    None
}

/// Constant text strictly between two columns, slots skipped.
fn const_between(cols: &[Col], a: usize, b: usize) -> String {
    cols[a + 1..b].iter().filter(|c| !c.is_slot()).map(|c| if c.kind == Kind::Space { " ".to_string() } else { lossy(&c.text) }).collect()
}

/// The next constant word after a slot, lowercase, spaces skipped.
fn word_after(cols: &[Col], idx: usize) -> Option<String> {
    let c = cols.get(idx + 1..)?.iter().find(|c| c.kind != Kind::Space)?;
    (!c.is_slot() && c.kind == Kind::Word).then(|| lossy(&c.text).to_lowercase())
}

/// Columns to a `Template` plus the slot descriptions. Every name comes from a printed
/// rule — a key, a preceding constant, the vocabulary or the slot's own type — and every
/// slot carries the reason, including the reason it stayed `kind+n`. `rare` is
/// `rare_count` for the cluster.
pub fn shape(cols: &[Col], rare: usize) -> (Template, Vec<Slot>) {
    let mut slots = Vec::new();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut generic: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut group: Vec<Token> = Vec::new();
    let mut group_presence: Option<&Vec<bool>> = None;
    let last_slot = cols.iter().rposition(Col::is_slot);
    let kinds: Vec<Option<SlotKind>> = cols
        .iter()
        .enumerate()
        .map(|(idx, c)| {
            if !c.is_slot() {
                return None;
            }
            let kind = slot_kind(c, cols, idx, rare);
            if kind == SlotKind::Text && Some(idx) == last_slot && idx + 1 == cols.len() && c.region {
                return Some(SlotKind::Rest);
            }
            Some(kind)
        })
        .collect();
    let placed = positional(cols, &kinds);

    let flush_group = |tokens: &mut Vec<Token>, group: &mut Vec<Token>| {
        if !group.is_empty() {
            tokens.push(Token::Optional(std::mem::take(group)));
        }
    };

    for (idx, c) in cols.iter().enumerate() {
        // optional runs with identical presence form one group
        if c.optional {
            if group_presence.is_some_and(|p| p != &c.present) {
                flush_group(&mut tokens, &mut group);
            }
            group_presence = Some(&c.present);
        } else if group_presence.is_some() {
            flush_group(&mut tokens, &mut group);
            group_presence = None;
        }
        let target = if c.optional { &mut group } else { &mut tokens };
        let Some(kind) = kinds[idx] else {
            let text = if c.kind == Kind::Space { " ".to_string() } else { lossy(&c.text) };
            push_const(target, &text);
            continue;
        };
        let (found, reason) = name_slot(cols, idx, kind, &placed);
        let (mut name, suggested) = match found {
            Some(n) => (n, true),
            None => {
                let base = match kind {
                    SlotKind::Ipv4 | SlotKind::Ip => "ip",
                    SlotKind::Ipv6 => "ip6",
                    SlotKind::Float => "num",
                    k => k.name(),
                };
                let n = generic.entry(base).or_default();
                *n += 1;
                (format!("{base}{n}"), false)
            }
        };
        let count = used.entry(name.clone()).or_default();
        *count += 1;
        if *count > 1 {
            name = format!("{name}_{count}");
        }
        target.push(Token::Slot { name: name.clone(), kind });
        slots.push(Slot { col: idx, name, kind, suggested, preceded_by: preceding_text(cols, idx), reason });
    }
    flush_group(&mut tokens, &mut group);
    (Template { tokens }, slots)
}

/// True when the slot directly follows `key=` or `key:`.
fn is_keyed(cols: &[Col], idx: usize) -> bool {
    idx >= 2 && cols[idx - 1].kind == Kind::Punct && matches!(cols[idx - 1].text.as_slice(), b"=" | b":") && cols[idx - 2].kind == Kind::Word && !cols[idx - 2].is_slot()
}

fn preceding_text(cols: &[Col], idx: usize) -> String {
    let mut s = String::new();
    let mut i = idx;
    while i > 0 && s.len() < 24 {
        i -= 1;
        if cols[i].is_slot() {
            break;
        }
        let t = if cols[i].kind == Kind::Space { " ".to_string() } else { lossy(&cols[i].text) };
        s.insert_str(0, &t);
    }
    s.trim().to_string()
}

fn push_const(tokens: &mut Vec<Token>, text: &str) {
    if let Some(Token::Const(last)) = tokens.last_mut() {
        last.push_str(text);
    } else {
        tokens.push(Token::Const(text.to_string()));
    }
}

fn sanitize(key: &str) -> String {
    let mut s: String = key.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.is_empty() || s.as_bytes()[0].is_ascii_digit() {
        s.insert(0, 'f');
    }
    s
}

pub fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
