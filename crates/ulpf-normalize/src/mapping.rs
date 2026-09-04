//! Compiled mapping and the JSON Lines writer.

use std::collections::HashMap;

use serde_json::{Map, Value};
use ulpf_parse::Parsed;
use ulpf_time::Policies;

use crate::def::{ClassRule, MappingFile};

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
}

struct EnumTable {
    id_field: Option<String>,
    unknown: Option<(String, Option<i64>)>,
    other: Option<(String, Option<i64>)>,
    /// lowercase raw → (canonical, id)
    raw: HashMap<Vec<u8>, (String, Option<i64>)>,
    /// (source field, lowercase raw) → (canonical, id)
    raw_by_field: HashMap<(Vec<u8>, Vec<u8>), (String, Option<i64>)>,
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
}

impl Mapping {
    pub fn compile(file: MappingFile) -> Result<Mapping, String> {
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
                    for r in list {
                        raw_by_field.insert((src.as_bytes().to_vec(), r.to_ascii_lowercase().into_bytes()), (v.value.clone(), v.id));
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
        Ok(Mapping { file, fields, aliases, enums, default_class })
    }

    pub fn schema_name(&self) -> &str {
        &self.file.schema.name
    }

    pub fn file(&self) -> &MappingFile {
        &self.file
    }

    fn select_class(&self, parsed: &Parsed<'_>) -> &ClassRule {
        for rule in &self.file.class {
            for alt in &rule.when {
                let hit = alt.iter().all(|(field, values)| {
                    parsed.get(field.as_bytes()).is_some_and(|v| values.iter().any(|x| x.as_bytes().eq_ignore_ascii_case(v)))
                });
                if hit {
                    return rule;
                }
            }
        }
        &self.default_class
    }

    /// Appends one JSON line (with trailing newline) for `parsed` to `out`.
    pub fn normalize(&self, parsed: &Parsed<'_>, prov: &Provenance<'_>, out: &mut Vec<u8>) -> NormalizeStats {
        let mut stats = NormalizeStats::default();
        let mut root = Map::new();
        let mut unmapped = Map::new();
        let class = self.select_class(parsed);
        stats.class_uid = class.uid;

        // best alias rank seen per schema field, so a lower-ranked alias never overrides
        let mut chosen: Vec<Option<(usize, Value)>> = (0..self.fields.len()).map(|_| None).collect();
        let mut chosen_src: Vec<Option<Vec<u8>>> = (0..self.fields.len()).map(|_| None).collect();
        for f in &parsed.fields {
            let key = lossy(&f.key, &mut stats);
            let value_text = lossy(&f.value, &mut stats);
            match self.aliases.get(&*f.key) {
                Some(&(idx, rank)) => {
                    let better = chosen[idx].as_ref().is_none_or(|(r, _)| rank < *r);
                    if better {
                        if let Some((_, prev)) = chosen[idx].take() {
                            // demoted alias keeps its data
                            unmapped.insert(String::from_utf8_lossy(chosen_src[idx].as_deref().unwrap_or_default()).into_owned(), prev);
                            stats.unmapped += 1;
                            stats.mapped -= 1;
                        }
                        chosen[idx] = Some((rank, Value::String(value_text.into_owned())));
                        chosen_src[idx] = Some(f.key.to_vec());
                        stats.mapped += 1;
                    } else {
                        unmapped.insert(key.into_owned(), Value::String(value_text.into_owned()));
                        stats.unmapped += 1;
                    }
                }
                None => {
                    unmapped.insert(key.into_owned(), Value::String(value_text.into_owned()));
                    stats.unmapped += 1;
                }
            }
        }
        for (idx, slot) in chosen.into_iter().enumerate() {
            let field = &self.fields[idx];
            let Some((_, value)) = slot else {
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
            match field.enum_idx {
                Some(ei) => {
                    let table = &self.enums[ei];
                    let raw = value.as_str().unwrap_or_default();
                    let lower = raw.to_ascii_lowercase().into_bytes();
                    let src = chosen_src[idx].clone().unwrap_or_default();
                    let hit = table.raw_by_field.get(&(src.clone(), lower.clone())).or_else(|| table.raw.get(&lower));
                    match hit {
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
                            unmapped.insert(String::from_utf8_lossy(&src).into_owned(), value);
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
            root.insert("type_uid".into(), Value::from(class.uid * 100 + a));
        }

        // time
        let (nanos, policies) = match parsed.timestamp {
            Some(ts) => (ts.epoch_nanos, ts.policies),
            None => {
                stats.time_from_receipt = true;
                (prov.receipt_nanos, Policies::RECEIPT_FALLBACK)
            }
        };
        root.insert("time".into(), Value::from(ulpf_time::epoch_millis(nanos)));
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
        if !unmapped.is_empty() {
            root.insert("unmapped".into(), Value::Object(unmapped));
        }
        serde_json::to_writer(&mut *out, &Value::Object(root)).expect("writing to a Vec cannot fail");
        out.push(b'\n');
        stats
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
