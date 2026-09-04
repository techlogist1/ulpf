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

const NAMING_KEYS: [&str; 30] = [
    "user", "port", "proto", "protocol", "len", "length", "ttl", "id", "from", "to", "via", "interface", "in", "out",
    "src", "dst", "host", "rule", "reason", "code", "status", "method", "uri", "url", "path", "bytes", "size",
    "duration", "time", "action",
];

/// Columns to a `Template` plus the slot descriptions. Names are the preceding key when
/// the format has one (`IN=`, `src-mac X`, `user X`), marked suggested; otherwise
/// `kind+n`, unsuggested, for the reviewer to name. `rare` is `rare_count` for the cluster.
pub fn shape(cols: &[Col], rare: usize) -> (Template, Vec<Slot>) {
    let mut slots = Vec::new();
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut generic: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut group: Vec<Token> = Vec::new();
    let mut group_presence: Option<&Vec<bool>> = None;
    let last_slot = cols.iter().rposition(Col::is_slot);

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
        if !c.is_slot() {
            let text = if c.kind == Kind::Space { " ".to_string() } else { lossy(&c.text) };
            push_const(target, &text);
            continue;
        }
        let mut kind = slot_kind(c, cols, idx, rare);
        if kind == SlotKind::Text && Some(idx) == last_slot && idx + 1 == cols.len() && c.region {
            kind = SlotKind::Rest;
        }
        let preceded_by = preceding_text(cols, idx);
        let key = key_before(cols, idx);
        let (mut name, suggested) = match (&key, kind) {
            (_, SlotKind::Timestamp) => ("timestamp".to_string(), true),
            (Some(k), _) if is_keyed(cols, idx) || NAMING_KEYS.contains(&k.as_str()) || k.contains(['-', '_']) => (sanitize(k), true),
            _ => {
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
        slots.push(Slot { col: idx, name, kind, suggested, preceded_by });
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
