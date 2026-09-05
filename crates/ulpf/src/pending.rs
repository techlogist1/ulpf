//! The pending directory: one proposal per source, as three files the reviewer can read
//! and edit by hand. `<id>.toml` is the parser definition (the only thing approval
//! moves), `<id>.json` the evidence and review state, `<id>.lines` the unknown lines it
//! was built from. Approval is the only path from here to the parsers directory; a
//! rejected proposal's fingerprint is remembered so the engine cannot resubmit it.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ulpf_infer::{Evidence, Params, Proposal, TemplateEvidence, Update};
use ulpf_parse::def::ParserDefinition;

pub struct Pending {
    dir: PathBuf,
    rejected: Mutex<HashSet<String>>,
    /// Every mutating operation runs under this lock: the inference thread's `write`
    /// and a reviewer's edit or approval must never interleave on the same three files.
    ops: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRecord {
    pub id: String,
    pub source: String,
    pub created_nanos: i64,
    pub edited: bool,
    pub evidence: Evidence,
    /// Present when the proposal is a new version of an active parser (drift).
    #[serde(default)]
    pub updates: Option<Update>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingSummary {
    pub id: String,
    pub source: String,
    pub name: Option<String>,
    pub created_nanos: i64,
    pub lines: u64,
    pub templates: u64,
    pub unmatched: u64,
    pub edited: bool,
    pub problems: u64,
    pub updates: Option<String>,
    pub version: u64,
    pub current_version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingDetail {
    pub id: String,
    pub source: String,
    pub definition: String,
    pub problems: Vec<String>,
    pub record: PendingRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    Replaced,
    SkippedEdited,
    SkippedDuplicate,
    SkippedRejected,
    SkippedEmpty,
}

impl WriteOutcome {
    pub fn skip_reason(self) -> Option<&'static str> {
        match self {
            WriteOutcome::SkippedEdited => Some("edited"),
            WriteOutcome::SkippedDuplicate => Some("duplicate"),
            WriteOutcome::SkippedRejected => Some("rejected"),
            WriteOutcome::SkippedEmpty => Some("no_templates"),
            WriteOutcome::Written | WriteOutcome::Replaced => None,
        }
    }
}

#[derive(Debug)]
pub enum ReviewError {
    NotFound(String),
    Invalid(Vec<String>),
    Conflict(String),
    Io(String),
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReviewError::NotFound(id) => write!(f, "no pending proposal `{id}`"),
            ReviewError::Invalid(p) => write!(f, "definition does not load: {}", p.join("; ")),
            ReviewError::Conflict(n) => write!(f, "an active parser is already named `{n}`; change [parser] name first"),
            ReviewError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for ReviewError {
    fn from(e: io::Error) -> Self {
        ReviewError::Io(e.to_string())
    }
}

pub struct Approved {
    pub name: String,
    pub path: PathBuf,
    pub source: String,
    /// The version the approval replaced, for an update.
    pub replaced_version: Option<u64>,
}

fn now_nanos() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)
}

fn rejected_key(source: &str, fingerprint: &str) -> String {
    format!("{source}\0{fingerprint}")
}

/// Load report for a definition text: `path:line: message` strings, empty when it loads.
pub fn problems_of(path: &Path, text: &str) -> Vec<String> {
    match ulpf_parse::load_str(path, text) {
        Ok(_) => vec![],
        Err(e) => vec![e.to_string()],
    }
}

impl Pending {
    /// Creates the directory layout and remembers every fingerprint under `rejected/`.
    pub fn open(dir: &Path) -> io::Result<Pending> {
        fs::create_dir_all(dir.join("rejected"))?;
        fs::create_dir_all(dir.join("approved"))?;
        let mut rejected = HashSet::new();
        for entry in fs::read_dir(dir.join("rejected"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|x| x == "json")
                && let Ok(text) = fs::read_to_string(&path)
                && let Ok(rec) = serde_json::from_str::<PendingRecord>(&text)
            {
                rejected.insert(rejected_key(&rec.source, &rec.evidence.fingerprint));
            }
        }
        Ok(Pending { dir: dir.to_path_buf(), rejected: Mutex::new(rejected), ops: Mutex::new(()) })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn id_for(source: &str) -> String {
        ulpf_infer::slug(source)
    }

    /// Ids of every proposal on disk: a directory scan, no file is opened.
    pub fn ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return vec![] };
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        ids.sort();
        ids
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn toml_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }
    fn json_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
    fn lines_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.lines"))
    }

    fn record(&self, id: &str) -> Result<PendingRecord, ReviewError> {
        // the id becomes three file names under the pending directory; the slug charset is
        // the only one a proposal can have, so anything else is simply not a proposal
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(ReviewError::NotFound(id.to_string()));
        }
        let text = fs::read_to_string(self.json_path(id)).map_err(|_| ReviewError::NotFound(id.to_string()))?;
        serde_json::from_str(&text).map_err(|e| ReviewError::Io(format!("{}: {e}", self.json_path(id).display())))
    }

    fn save_record(&self, rec: &PendingRecord) -> Result<(), ReviewError> {
        let text = serde_json::to_string_pretty(rec).map_err(|e| ReviewError::Io(e.to_string()))?;
        atomic_write(&self.json_path(&rec.id), text.as_bytes())?;
        Ok(())
    }

    pub fn list(&self) -> Vec<PendingSummary> {
        let mut out = Vec::new();
        for id in self.ids() {
            if let Ok(detail) = self.get(&id) {
                let def = toml::from_str::<ParserDefinition>(&detail.definition).ok();
                out.push(PendingSummary {
                    id: detail.id,
                    source: detail.source,
                    name: def.as_ref().map(|d| d.parser.name.clone()),
                    created_nanos: detail.record.created_nanos,
                    lines: detail.record.evidence.lines_seen,
                    templates: detail.record.evidence.templates.len() as u64,
                    unmatched: detail.record.evidence.unmatched.count,
                    edited: detail.record.edited,
                    problems: detail.problems.len() as u64,
                    updates: detail.record.updates.as_ref().map(|u| u.name.clone()),
                    version: def.as_ref().map(|d| d.parser.version).unwrap_or(1),
                    current_version: detail.record.updates.as_ref().map(|u| u.current_version),
                });
            }
        }
        out
    }

    pub fn get(&self, id: &str) -> Result<PendingDetail, ReviewError> {
        let record = self.record(id)?;
        let path = self.toml_path(id);
        // the record exists, so a missing definition is damage, not absence
        let definition = fs::read_to_string(&path).map_err(|e| ReviewError::Io(format!("proposal `{id}` is damaged: {}: {e}", path.display())))?;
        let problems = problems_of(&path, &definition);
        Ok(PendingDetail { id: id.to_string(), source: record.source.clone(), definition, problems, record })
    }

    /// For an update: the active parser's current text and a unified diff against the
    /// proposal, for the review screen. `(None, None)` for a fresh proposal.
    pub fn current_and_diff(&self, id: &str, parsers_dir: &Path) -> (Option<String>, Option<String>) {
        let Ok(detail) = self.get(id) else { return (None, None) };
        let Some(u) = &detail.record.updates else { return (None, None) };
        let path = parsers_dir.join(format!("{}.toml", u.name));
        let Ok(current) = fs::read_to_string(&path) else { return (None, None) };
        let diff = unified_diff(&format!("parsers/{}.toml (v{})", u.name, u.current_version), &format!("pending/{id}.toml (proposed)"), &current, &detail.definition);
        (Some(current), Some(diff))
    }

    /// The unknown events the proposal was built from, terminators included, framed the
    /// way the engine framed them so `members` indices line up (a blank or indented line
    /// belongs to the event before it).
    pub fn lines(&self, id: &str) -> Vec<Vec<u8>> {
        let Ok(bytes) = fs::read(self.lines_path(id)) else { return vec![] };
        ulpf_store::Framer::new(&bytes, true).map(|r| bytes[r].to_vec()).collect()
    }

    /// Saves an edited definition, valid or not, and marks the proposal edited so the
    /// engine stops replacing it. Returns the load problems of the new text.
    pub fn put_text(&self, id: &str, text: &str) -> Result<Vec<String>, ReviewError> {
        let _ops = self.lock();
        let mut rec = self.record(id)?;
        atomic_write(&self.toml_path(id), text.as_bytes())?;
        if !rec.edited {
            rec.edited = true;
            self.save_record(&rec)?;
        }
        Ok(problems_of(&self.toml_path(id), text))
    }

    /// Writes or replaces the proposal for the source, unless a human edited the pending
    /// one, an identical one is already pending, or the same fingerprint was rejected.
    pub fn write(&self, proposal: &Proposal, lines: &[Vec<u8>]) -> Result<WriteOutcome, ReviewError> {
        // a fresh proposal with nothing to parse is nothing; an update composed on a kv or
        // delimiter prior legitimately has no `patterns` (its strategy is the prior's)
        if proposal.evidence.templates.is_empty() || (proposal.definition.strategy.patterns.is_empty() && proposal.updates.is_none()) {
            return Ok(WriteOutcome::SkippedEmpty);
        }
        let id = Self::id_for(&proposal.source);
        let _ops = self.lock();
        if self.rejected.lock().unwrap_or_else(|e| e.into_inner()).contains(&rejected_key(&proposal.source, &proposal.evidence.fingerprint)) {
            return Ok(WriteOutcome::SkippedRejected);
        }
        let existing = self.record(&id).ok();
        let outcome = match &existing {
            Some(rec) if rec.edited => return Ok(WriteOutcome::SkippedEdited),
            Some(rec) if rec.evidence.fingerprint == proposal.evidence.fingerprint => return Ok(WriteOutcome::SkippedDuplicate),
            Some(_) => WriteOutcome::Replaced,
            None => WriteOutcome::Written,
        };
        let text = toml::to_string(&proposal.definition).map_err(|e| ReviewError::Io(e.to_string()))?;
        let mut joined = Vec::with_capacity(lines.iter().map(Vec::len).sum::<usize>() + lines.len());
        for l in lines {
            joined.extend_from_slice(l);
            if !l.ends_with(b"\n") {
                joined.push(b'\n');
            }
        }
        atomic_write(&self.lines_path(&id), &joined)?;
        atomic_write(&self.toml_path(&id), text.as_bytes())?;
        self.save_record(&PendingRecord { id, source: proposal.source.clone(), created_nanos: now_nanos(), edited: false, evidence: proposal.evidence.clone(), updates: proposal.updates.clone() })?;
        Ok(outcome)
    }

    /// Re-emits the definition from the kept templates, in evidence order, after merging
    /// each group of `merge` into one template built from the union of their lines.
    /// Human edits to everything but `patterns` survive; the evidence gains the merged
    /// templates. Returns the new text and its load problems.
    pub fn regenerate(&self, id: &str, keep: &[u64], merge: &[Vec<u64>], params: &Params) -> Result<(String, Vec<String>), ReviewError> {
        let _ops = self.lock();
        let mut rec = self.record(id)?;
        let text = fs::read_to_string(self.toml_path(id)).map_err(|_| ReviewError::NotFound(id.to_string()))?;
        let mut def: ParserDefinition = toml::from_str(&text).map_err(|e| ReviewError::Invalid(vec![format!("{}: {}", self.toml_path(id).display(), e.message())]))?;
        let lines = self.lines(id);
        let syslog = rec.evidence.envelope.syslog;
        let mut keep: Vec<u64> = keep.to_vec();
        for group in merge {
            let members: Vec<&[u8]> = rec
                .evidence
                .templates
                .iter()
                .filter(|t| group.contains(&t.id))
                .flat_map(|t| t.members.iter().filter_map(|m| lines.get(*m as usize).map(Vec::as_slice)))
                .collect();
            if members.is_empty() {
                continue;
            }
            if let Some(mut merged) = ulpf_infer::merge(&members, syslog, params) {
                merged.id = rec.evidence.templates.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                merged.members = rec.evidence.templates.iter().filter(|t| group.contains(&t.id)).flat_map(|t| t.members.iter().copied()).collect();
                merged.history.insert(0, format!("merged in review from templates {}", group.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")));
                keep.retain(|k| !group.contains(k));
                keep.push(merged.id);
                rec.evidence.templates.push(merged);
                rec.evidence.decisions.push(format!("review: templates {} merged", group.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")));
            }
        }
        let kept: Vec<&TemplateEvidence> = rec.evidence.templates.iter().filter(|t| keep.contains(&t.id)).collect();
        def.strategy.patterns = kept.iter().map(|t| t.pattern.clone()).collect();
        def.strategy.pattern = None;
        let text = toml::to_string(&def).map_err(|e| ReviewError::Io(e.to_string()))?;
        atomic_write(&self.toml_path(id), text.as_bytes())?;
        rec.edited = true;
        rec.evidence.decisions.push(format!("review: definition regenerated from templates {}", keep.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")));
        self.save_record(&rec)?;
        Ok((text.clone(), problems_of(&self.toml_path(id), &text)))
    }

    /// Moves the definition into the parsers directory. The text must load and its name
    /// must not collide with an active parser. The lines stay under `approved/` with the
    /// evidence; the caller reloads the registry and reports what now detects.
    pub fn approve(&self, id: &str, parsers_dir: &Path, active_names: &[String]) -> Result<Approved, ReviewError> {
        let _ops = self.lock();
        let rec = self.record(id)?;
        let text = fs::read_to_string(self.toml_path(id)).map_err(|_| ReviewError::NotFound(id.to_string()))?;
        let parser = ulpf_parse::load_str(&self.toml_path(id), &text).map_err(|e| ReviewError::Invalid(vec![e.to_string()]))?;
        let name = parser.name().to_string();
        // the name becomes a file name under parsers/: no separators, no dots, nothing a
        // reviewer's typo or a hostile PUT could turn into a path
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            return Err(ReviewError::Invalid(vec![format!("[parser] name `{name}` must be [A-Za-z0-9_-]+ (it names the file under parsers/)")]));
        }
        let path = parsers_dir.join(format!("{name}.toml"));
        let stamp = now_nanos();
        let approved = self.dir.join("approved");
        // an update may only replace the parser it was composed on; the replaced text is
        // kept beside the evidence, so no version is ever lost
        let is_update = rec.updates.as_ref().is_some_and(|u| u.name == name);
        let mut replaced_version = None;
        if is_update {
            if let Ok(current) = fs::read_to_string(&path) {
                let v = toml::from_str::<ParserDefinition>(&current).map(|d| d.parser.version).unwrap_or(1);
                atomic_write(&approved.join(format!("{name}.v{v}.toml")), current.as_bytes())?;
                replaced_version = Some(v);
            }
        } else if active_names.contains(&name) || path.exists() {
            return Err(ReviewError::Conflict(name));
        }
        atomic_write(&path, text.as_bytes())?;
        // the record is the proposal's identity: it moves first; if that fails the parser
        // file is taken back so the two directories never disagree
        if let Err(e) = fs::rename(self.json_path(id), approved.join(format!("{id}-{stamp}.json"))) {
            let _ = fs::remove_file(&path);
            return Err(e.into());
        }
        let _ = fs::remove_file(self.toml_path(id));
        let _ = fs::rename(self.lines_path(id), approved.join(format!("{id}-{stamp}.lines")));
        Ok(Approved { name, path, source: rec.source, replaced_version })
    }

    /// Moves the proposal under `rejected/` and remembers its fingerprint.
    pub fn reject(&self, id: &str) -> Result<PathBuf, ReviewError> {
        let _ops = self.lock();
        let rec = self.record(id)?;
        let stamp = now_nanos();
        let rejected = self.dir.join("rejected");
        let target = rejected.join(format!("{id}-{stamp}.toml"));
        // record first (identity), then the definition; an orphaned toml is invisible to
        // `list` and overwritten by the next proposal, an orphaned json would be a ghost
        fs::rename(self.json_path(id), rejected.join(format!("{id}-{stamp}.json")))?;
        let _ = fs::rename(self.toml_path(id), &target);
        let _ = fs::remove_file(self.lines_path(id));
        self.rejected.lock().unwrap_or_else(|e| e.into_inner()).insert(rejected_key(&rec.source, &rec.evidence.fingerprint));
        Ok(target)
    }
}

/// A unified diff of two texts (LCS over lines, three lines of context). Enough for a
/// review screen; no dependency.
pub fn unified_diff(a_name: &str, b_name: &str, a: &str, b: &str) -> String {
    let al: Vec<&str> = a.lines().collect();
    let bl: Vec<&str> = b.lines().collect();
    let (n, m) = (al.len(), bl.len());
    if n > 4000 || m > 4000 {
        return format!("--- {a_name}\n+++ {b_name}\n(diff suppressed: {n} against {m} lines; open both texts)\n");
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if al[i] == bl[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    // ops: (' ', a-line) | ('-', a-line) | ('+', b-line)
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && al[i] == bl[j] {
            ops.push((' ', al[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            ops.push(('+', bl[j]));
            j += 1;
        } else {
            ops.push(('-', al[i]));
            i += 1;
        }
    }
    let mut out = format!("--- {a_name}\n+++ {b_name}\n");
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (c, _))| *c != ' ').map(|(k, _)| k).collect();
    if changed.is_empty() {
        return out;
    }
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(3);
        let mut end = changed[k] + 3;
        while k + 1 < changed.len() && changed[k + 1] <= end + 3 {
            k += 1;
            end = changed[k] + 3;
        }
        let end = end.min(ops.len() - 1);
        let (mut a_start, mut b_start, mut a_len, mut b_len) = (0usize, 0usize, 0usize, 0usize);
        for (idx, (c, _)) in ops.iter().enumerate() {
            if idx < start {
                a_start += (*c != '+') as usize;
                b_start += (*c != '-') as usize;
            } else if idx <= end {
                a_len += (*c != '+') as usize;
                b_len += (*c != '-') as usize;
            }
        }
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", a_start + 1, a_len, b_start + 1, b_len));
        for (c, line) in &ops[start..=end] {
            out.push(*c);
            out.push_str(line);
            out.push('\n');
        }
        k += 1;
    }
    out
}

/// Write to a sibling temp file, sync it, and rename, so a reader never sees a
/// half-written file and a power loss leaves either the old file or the new one.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    {
        let mut f = fs::File::create(&tmp)?;
        io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}
