//! Compiled mapping and the JSON Lines writer.

use std::collections::HashMap;

use serde_json::{Map, Value};
use ulpf_parse::Parsed;
use ulpf_time::Policies;

use crate::def::{ClassRule, EntityKind, MappingFile};

/// What the engine knows about an event that the mapping cannot: identity and provenance.
pub struct Provenance<'a> {
    pub raw_id: u64,
    pub source: &'a str,
    pub parser: Option<&'a str>,
    pub vendor: Option<&'a str>,
    pub product: Option<&'a str>,
    pub receipt_nanos: i64,
    /// `parsed`, `no_parser`, or a `ParseFailure::reason()`.
    pub parse_status: &'a str,
    pub sub_status: &'a str,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NormalizeStats {
    pub class_uid: i64,
    pub mapped: u32,
    pub unmapped: u32,
    /// Enum fields whose source value was in no list (emitted as Other).
    pub enum_other: u32,
    pub time_from_receipt: bool,
    pub utf8_lossy: bool,
    /// The emitted `time` (epoch milliseconds), for the entity index.
    pub time_ms: i64,
    /// Per `EntityKind` (indexed by `kind as usize`), the index into `parsed.fields` of the
    /// source field that fed that kind's schema path; `None` when nothing fed it.
    pub entities: [Option<u32>; 5],
}

/// One schema field the mapping set from a source field. Cold path (`Mapping::provenance`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldProvenance {
    /// Dotted schema path, as it appears in the emitted line.
    pub path: String,
    /// Index into `parsed.fields`.
    pub field_index: u32,
    /// The mapping rewrote the value (enum canonicalisation).
    pub canonical: bool,
    /// The emitted value as text.
    pub value: String,
}

/// (canonical value, id)
type Canonical = (String, Option<i64>);

/// What became of one parsed field: dropped as an absent value, fed a schema field, or
/// landed under `unmapped`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Absent,
    Winner,
    Unmapped,
}

struct EnumTable {
    id_field: Option<String>,
    unknown: Option<(String, Option<i64>)>,
    other: Option<(String, Option<i64>)>,
    /// lowercase raw → (canonical, id)
    raw: HashMap<Vec<u8>, Canonical>,
    /// source field → lowercase raw → (canonical, id). Nested so a lookup borrows both
    /// keys instead of cloning them per event.
    raw_by_field: HashMap<Vec<u8>, HashMap<Vec<u8>, Canonical>>,
}

impl EnumTable {
    fn lookup(&self, source_key: &[u8], lower: &[u8]) -> Option<&Canonical> {
        self.raw_by_field.get(source_key).and_then(|m| m.get(lower)).or_else(|| self.raw.get(lower))
    }
}

struct SchemaField {
    path: String,
    is_int: bool,
    enum_idx: Option<usize>,
}

pub struct Mapping {
    file: MappingFile,
    fields: Vec<SchemaField>,
    /// source field name → (schema field index, alias rank)
    aliases: HashMap<Vec<u8>, (usize, usize)>,
    enums: Vec<EnumTable>,
    default_class: ClassRule,
    /// Per `EntityKind`, the `fields` index of its schema path (`None` when the mapping
    /// declares no path, or a path no source field can feed).
    entity_field: [Option<usize>; 5],
}

impl Mapping {
    pub fn compile(file: MappingFile) -> Result<Mapping, String> {
        for c in file.class.iter().chain(file.default_class.iter()) {
            if !(0..=99_999_999).contains(&c.uid) {
                return Err(format!("[[class]] `{}`: uid {} is outside 0..=99999999", c.name, c.uid));
            }
        }
        let mut fields: Vec<SchemaField> = Vec::new();
        let mut aliases: HashMap<Vec<u8>, (usize, usize)> = HashMap::new();
        for (path, names) in &file.fields {
            if path.is_empty() || path.starts_with("unmapped") || path.starts_with("ulpf") {
                return Err(format!("[fields] `{path}` is reserved"));
            }
            let idx = fields.len();
            fields.push(SchemaField { path: path.clone(), is_int: file.types.int.contains(path), enum_idx: None });
            for (rank, n) in names.iter().enumerate() {
                if let Some((other, _)) = aliases.insert(n.as_bytes().to_vec(), (idx, rank)) {
                    return Err(format!("[fields] source field `{n}` is listed under both `{}` and `{path}`", fields[other].path));
                }
            }
        }
        let mut enums = Vec::new();
        for e in &file.enums {
            let field_idx = match fields.iter().position(|f| f.path == e.field) {
                Some(i) => i,
                None => {
                    fields.push(SchemaField { path: e.field.clone(), is_int: false, enum_idx: None });
                    fields.len() - 1
                }
            };
            let mut raw = HashMap::new();
            let mut raw_by_field = HashMap::new();
            for v in &e.values {
                for r in &v.raw {
                    raw.insert(r.to_ascii_lowercase().into_bytes(), (v.value.clone(), v.id));
                }
                for (src, list) in &v.raw_by_field {
                    let per_field: &mut HashMap<Vec<u8>, Canonical> = raw_by_field.entry(src.as_bytes().to_vec()).or_default();
                    for r in list {
                        per_field.insert(r.to_ascii_lowercase().into_bytes(), (v.value.clone(), v.id));
                    }
                }
            }
            fields[field_idx].enum_idx = Some(enums.len());
            enums.push(EnumTable {
                id_field: e.id_field.clone(),
                unknown: e.unknown.as_ref().map(|u| (u.value.clone(), u.id)),
                other: e.other.as_ref().map(|u| (u.value.clone(), u.id)),
                raw,
                raw_by_field,
            });
        }
        let default_class = file.default_class.clone().unwrap_or(ClassRule {
            uid: 0,
            name: "Base Event".into(),
            category_uid: 0,
            category_name: "Uncategorized".into(),
            constants: Default::default(),
            when: vec![],
        });
        let mut entity_field = [None; 5];
        for kind in EntityKind::ALL {
            let Some(path) = file.entities.path(kind) else { continue };
            match fields.iter().position(|f| f.path == path) {
                Some(i) => entity_field[kind as usize] = Some(i),
                None => {
                    // a path only a constant or an enum id can set carries no source field:
                    // legal (the caller falls back), but a typo must not pass silently
                    let settable = file
                        .class
                        .iter()
                        .chain(file.default_class.iter())
                        .any(|c| c.constants.contains_key(path))
                        || file.enums.iter().any(|e| e.id_field.as_deref() == Some(path));
                    if !settable {
                        return Err(format!("[entities] {} = `{path}`: no [fields] entry, class constant or enum id_field sets that path", kind.name()));
                    }
                }
            }
        }
        Ok(Mapping { file, fields, aliases, enums, default_class, entity_field })
    }

    /// The declared entity paths, for `GET /api/status` and the index rebuild.
    pub fn entities(&self) -> &crate::def::Entities {
        &self.file.entities
    }

    pub fn schema_name(&self) -> &str {
        &self.file.schema.name
    }

    pub fn file(&self) -> &MappingFile {
        &self.file
    }

    fn absent(&self, value: &[u8]) -> bool {
        self.file.values.absent.iter().any(|a| a.as_bytes().eq_ignore_ascii_case(value))
    }

    fn select_class(&self, parsed: &Parsed<'_>) -> &ClassRule {
        for rule in &self.file.class {
            for alt in &rule.when {
                let hit = alt.iter().all(|(field, values)| {
                    parsed
                        .get(field.as_bytes())
                        .is_some_and(|v| !self.absent(v) && values.iter().any(|x| x == "*" || x.as_bytes().eq_ignore_ascii_case(v)))
                });
                if hit {
                    return rule;
                }
            }
        }
        &self.default_class
    }

    /// Which parsed field feeds each schema field: `winners[schema field] = (alias rank,
    /// index into `parsed.fields`)`, lowest rank wins and the first field wins a tie;
    /// `fate[parsed field]` is the same answer from the field's side, so the caller needs
    /// no second `absent`/alias lookup. `normalize` and `provenance` both see the world
    /// through this one routine, so the two cannot disagree.
    fn choose(&self, parsed: &Parsed<'_>, winners: &mut Vec<Option<(usize, u32)>>, fate: &mut Vec<Fate>) {
        winners.clear();
        winners.resize(self.fields.len(), None);
        fate.clear();
        fate.resize(parsed.fields.len(), Fate::Unmapped);
        for (i, f) in parsed.fields.iter().enumerate() {
            if self.absent(&f.value) {
                fate[i] = Fate::Absent;
                continue;
            }
            if let Some(&(idx, rank)) = self.aliases.get(&*f.key)
                && winners[idx].is_none_or(|(r, _)| rank < r)
            {
                winners[idx] = Some((rank, i as u32));
            }
        }
        for (_, fi) in winners.iter().flatten() {
            fate[*fi as usize] = Fate::Winner;
        }
    }

    /// Appends one JSON line (with trailing newline) for `parsed` to `out`.
    pub fn normalize(&self, parsed: &Parsed<'_>, prov: &Provenance<'_>, out: &mut Vec<u8>) -> NormalizeStats {
        let mut stats = NormalizeStats::default();
        let mut root = Map::new();
        let mut unmapped = Map::new();
        let class = self.select_class(parsed);
        stats.class_uid = class.uid;

        let mut winners = Vec::new();
        let mut fate = Vec::new();
        self.choose(parsed, &mut winners, &mut fate);
        for (kind, slot) in self.entity_field.iter().enumerate() {
            if let Some(fi) = slot {
                stats.entities[kind] = winners[*fi].map(|(_, i)| i);
            }
        }

        // everything that did not win its schema field keeps its data under `unmapped`
        for (i, f) in parsed.fields.iter().enumerate() {
            if fate[i] != Fate::Unmapped {
                continue;
            }
            let key = lossy(&f.key, &mut stats);
            let value_text = lossy(&f.value, &mut stats);
            unmapped_insert(&mut unmapped, key.into_owned(), Value::String(value_text.into_owned()));
        }
        for (idx, slot) in winners.iter().enumerate() {
            let field = &self.fields[idx];
            let Some((_, fi)) = *slot else {
                if let Some(ei) = field.enum_idx
                    && let Some((name, id)) = &self.enums[ei].unknown
                {
                    set_path(&mut root, &field.path, Value::String(name.clone()));
                    if let (Some(idf), Some(id)) = (&self.enums[ei].id_field, id) {
                        set_path(&mut root, idf, Value::from(*id));
                    }
                }
                continue;
            };
            let src = &parsed.fields[fi as usize];
            let value = Value::String(lossy(&src.value, &mut stats).into_owned());
            stats.mapped += 1;
            match field.enum_idx {
                Some(ei) => {
                    let table = &self.enums[ei];
                    let lower = value.as_str().unwrap_or_default().to_ascii_lowercase().into_bytes();
                    match table.lookup(&src.key, &lower) {
                        Some((name, id)) => {
                            set_path(&mut root, &field.path, Value::String(name.clone()));
                            if let (Some(idf), Some(id)) = (&table.id_field, id) {
                                set_path(&mut root, idf, Value::from(*id));
                            }
                        }
                        None => {
                            stats.enum_other += 1;
                            if let Some((name, id)) = &table.other {
                                set_path(&mut root, &field.path, Value::String(name.clone()));
                                if let (Some(idf), Some(id)) = (&table.id_field, id) {
                                    set_path(&mut root, idf, Value::from(*id));
                                }
                            }
                            unmapped_insert(&mut unmapped, String::from_utf8_lossy(&src.key).into_owned(), value);
                        }
                    }
                }
                None => {
                    let v = if field.is_int { as_int(value) } else { value };
                    set_path(&mut root, &field.path, v);
                }
            }
        }

        root.insert("class_uid".into(), Value::from(class.uid));
        root.insert("class_name".into(), Value::String(class.name.clone()));
        root.insert("category_uid".into(), Value::from(class.category_uid));
        root.insert("category_name".into(), Value::String(class.category_name.clone()));
        for (k, v) in &class.constants {
            set_path(&mut root, k, toml_to_json(v));
        }
        if let Some(Value::Number(a)) = root.get("activity_id") {
            let a = a.as_i64().unwrap_or(0);
            root.insert("type_uid".into(), Value::from(class.uid.saturating_mul(100).saturating_add(a)));
        }

        // time
        let (nanos, policies) = match parsed.timestamp {
            Some(ts) => (ts.epoch_nanos, ts.policies),
            None => {
                stats.time_from_receipt = true;
                (prov.receipt_nanos, Policies::RECEIPT_FALLBACK)
            }
        };
        stats.time_ms = ulpf_time::epoch_millis(nanos);
        root.insert("time".into(), Value::from(stats.time_ms));
        let mut meta = match root.remove("metadata") {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        if let Some(v) = &self.file.schema.version {
            meta.insert("version".into(), Value::String(v.clone()));
        }
        let mut product = Map::new();
        if let Some(v) = prov.vendor {
            product.insert("vendor_name".into(), Value::String(v.into()));
        }
        if let Some(p) = prov.product {
            product.insert("name".into(), Value::String(p.into()));
        }
        if !product.is_empty() {
            meta.insert("product".into(), Value::Object(product));
        }
        if let Some(t) = &parsed.timestamp_text {
            meta.insert("original_time".into(), Value::String(String::from_utf8_lossy(t).into_owned()));
        }
        meta.insert("processed_time".into(), Value::from(ulpf_time::epoch_millis(prov.receipt_nanos)));
        let mut rfc = String::new();
        ulpf_time::format_rfc3339(nanos, &mut rfc);
        meta.insert("event_time_rfc3339".into(), Value::String(rfc));
        meta.insert("log_name".into(), Value::String(prov.source.into()));
        root.insert("metadata".into(), Value::Object(meta));

        let mut ulpf = Map::new();
        ulpf.insert("raw_id".into(), Value::from(prov.raw_id));
        if let Some(p) = prov.parser {
            ulpf.insert("parser".into(), Value::String(p.into()));
        }
        ulpf.insert("parse_status".into(), Value::String(prov.parse_status.into()));
        ulpf.insert("sub_status".into(), Value::String(prov.sub_status.into()));
        ulpf.insert("time_policies".into(), Value::Array(policies.names().map(|n| Value::String(n.into())).collect()));
        if let Some(e) = parsed.timestamp_error {
            ulpf.insert("time_error".into(), Value::String(e.into()));
        }
        if stats.utf8_lossy {
            ulpf.insert("utf8_lossy".into(), Value::Bool(true));
        }
        root.insert("ulpf".into(), Value::Object(ulpf));
        stats.unmapped = unmapped.len() as u32;
        if !unmapped.is_empty() {
            root.insert("unmapped".into(), Value::Object(unmapped));
        }
        serde_json::to_writer(&mut *out, &Value::Object(root)).expect("writing to a Vec cannot fail");
        out.push(b'\n');
        stats
    }

    /// Cold path (one traceback request): for every schema field `normalize` would set
    /// from a source field, which field fed it and what the emitted value is. Fields the
    /// mapping synthesises (class constants, enum `unknown`, enum id fields, `metadata.*`)
    /// have no entry, and neither has an enum miss with no `other` value, because nothing
    /// is emitted at that path.
    pub fn provenance(&self, parsed: &Parsed<'_>) -> Vec<FieldProvenance> {
        let mut winners = Vec::new();
        let mut fate = Vec::new();
        self.choose(parsed, &mut winners, &mut fate);
        let mut out = Vec::new();
        for (idx, slot) in winners.iter().enumerate() {
            let Some((_, fi)) = *slot else { continue };
            let field = &self.fields[idx];
            let src = &parsed.fields[fi as usize];
            let text = String::from_utf8_lossy(&src.value).into_owned();
            let (canonical, value) = match field.enum_idx {
                Some(ei) => {
                    let table = &self.enums[ei];
                    let lower = text.to_ascii_lowercase().into_bytes();
                    match table.lookup(&src.key, &lower).or(table.other.as_ref()) {
                        Some((name, _)) => (true, name.clone()),
                        None => continue,
                    }
                }
                None if field.is_int => match as_int(Value::String(text)) {
                    Value::String(s) => (false, s),
                    v => (false, v.to_string()),
                },
                None => (false, text),
            };
            out.push(FieldProvenance { path: field.path.clone(), field_index: fi, canonical, value });
        }
        out
    }
}

/// A source field name that repeats within one event keeps every value: a repeat lands
/// under `name#2`, `name#3`, ... (numbered in the order the values reach this map).
/// Nothing is dropped.
fn unmapped_insert(unmapped: &mut Map<String, Value>, key: String, value: Value) {
    if !unmapped.contains_key(&key) {
        unmapped.insert(key, value);
        return;
    }
    let mut n = 2;
    loop {
        let candidate = format!("{key}#{n}");
        if !unmapped.contains_key(&candidate) {
            unmapped.insert(candidate, value);
            return;
        }
        n += 1;
    }
}

fn lossy<'b>(bytes: &'b [u8], stats: &mut NormalizeStats) -> std::borrow::Cow<'b, str> {
    let s = String::from_utf8_lossy(bytes);
    if matches!(s, std::borrow::Cow::Owned(_)) {
        stats.utf8_lossy = true;
    }
    s
}

fn as_int(v: Value) -> Value {
    match &v {
        Value::String(s) => match s.parse::<i64>() {
            Ok(n) => Value::from(n),
            Err(_) => v,
        },
        _ => v,
    }
}

fn toml_to_json(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) => Value::from(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => Value::Object(t.iter().map(|(k, v)| (k.clone(), toml_to_json(v))).collect()),
    }
}

/// Inserts `value` at a dotted path, creating intermediate objects.
fn set_path(root: &mut Map<String, Value>, path: &str, value: Value) {
    let mut parts = path.split('.').peekable();
    let mut cur = root;
    while let Some(p) = parts.next() {
        if parts.peek().is_none() {
            cur.insert(p.to_owned(), value);
            return;
        }
        let entry = cur.entry(p.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        cur = entry.as_object_mut().expect("just ensured object");
    }
}
