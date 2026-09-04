use std::path::{Path, PathBuf};

use crate::def::MappingFile;
use crate::mapping::Mapping;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{}: {}", self.path.display(), l, self.message),
            None => write!(f, "{}: {}", self.path.display(), self.message),
        }
    }
}

pub struct LoadReport {
    /// One compiled mapping per schema name, in first-seen order.
    pub mappings: Vec<Mapping>,
    pub errors: Vec<LoadError>,
}

pub fn parse_file(path: &Path, text: &str) -> Result<MappingFile, LoadError> {
    toml::from_str(text).map_err(|e| LoadError {
        path: path.to_path_buf(),
        line: e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1),
        message: e.message().to_owned(),
    })
}

/// Loads every `*.toml` in `dir` in name order and merges by schema name.
pub fn load_dir(dir: &Path) -> std::io::Result<LoadReport> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    Ok(load_files(&paths))
}

pub fn load_files(paths: &[PathBuf]) -> LoadReport {
    let mut files: Vec<(PathBuf, MappingFile)> = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match std::fs::read_to_string(path) {
            Ok(text) => match parse_file(path, &text) {
                Ok(f) => files.push((path.clone(), f)),
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(LoadError { path: path.clone(), line: None, message: format!("cannot read: {e}") }),
        }
    }
    let mut merged: Vec<(String, Vec<PathBuf>, MappingFile)> = Vec::new();
    for (path, f) in files {
        match merged.iter_mut().find(|(n, _, _)| *n == f.schema.name) {
            Some((_, paths, base)) => {
                paths.push(path);
                merge(base, f);
            }
            None => merged.push((f.schema.name.clone(), vec![path], f)),
        }
    }
    let mut mappings = Vec::new();
    for (_, paths, f) in merged {
        match Mapping::compile(f) {
            Ok(m) => mappings.push(m),
            Err(message) => errors.push(LoadError { path: paths[0].clone(), line: None, message }),
        }
    }
    LoadReport { mappings, errors }
}

/// Later files extend earlier ones: aliases append (deduplicated), enum raw lists append
/// to the value with the same name, class `when` alternatives append to the same uid.
fn merge(base: &mut MappingFile, add: MappingFile) {
    if base.schema.version.is_none() {
        base.schema.version = add.schema.version;
    }
    for (path, aliases) in add.fields {
        let list = base.fields.entry(path).or_default();
        for a in aliases {
            if !list.contains(&a) {
                list.push(a);
            }
        }
    }
    for t in add.types.int {
        if !base.types.int.contains(&t) {
            base.types.int.push(t);
        }
    }
    for e in add.enums {
        match base.enums.iter_mut().find(|b| b.field == e.field) {
            Some(b) => {
                if b.id_field.is_none() {
                    b.id_field = e.id_field;
                }
                if b.unknown.is_none() {
                    b.unknown = e.unknown;
                }
                if b.other.is_none() {
                    b.other = e.other;
                }
                for v in e.values {
                    match b.values.iter_mut().find(|bv| bv.value == v.value) {
                        Some(bv) => {
                            if bv.id.is_none() {
                                bv.id = v.id;
                            }
                            bv.raw.extend(v.raw);
                            for (k, list) in v.raw_by_field {
                                bv.raw_by_field.entry(k).or_default().extend(list);
                            }
                        }
                        None => b.values.push(v),
                    }
                }
            }
            None => base.enums.push(e),
        }
    }
    for c in add.class {
        match base.class.iter_mut().find(|b| b.uid == c.uid) {
            Some(b) => {
                b.when.extend(c.when);
                for (k, v) in c.constants {
                    b.constants.entry(k).or_insert(v);
                }
            }
            None => base.class.push(c),
        }
    }
    if base.default_class.is_none() {
        base.default_class = add.default_class;
    }
}
