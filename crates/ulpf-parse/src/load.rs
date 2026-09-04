//! Directory loading. Every file is independent: a malformed one is reported with its
//! path and line and the rest still load.

use std::path::{Path, PathBuf};

use crate::compile::Parser;
use crate::def::ParserDefinition;

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
    pub parsers: Vec<Parser>,
    pub errors: Vec<LoadError>,
}

/// Parses and compiles one definition. Syntax errors carry a line number.
pub fn load_str(path: &Path, text: &str) -> Result<Parser, LoadError> {
    let def: ParserDefinition = toml::from_str(text).map_err(|e| LoadError {
        path: path.to_path_buf(),
        line: e.span().map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1),
        message: e.message().to_owned(),
    })?;
    Parser::from_definition(def).map_err(|message| LoadError { path: path.to_path_buf(), line: None, message })
}

/// Loads every `*.toml` in `dir` in name order. Only an unreadable directory is an `Err`.
pub fn load_dir(dir: &Path) -> std::io::Result<LoadReport> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();
    let mut report = LoadReport { parsers: Vec::new(), errors: Vec::new() };
    for path in paths {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                report.errors.push(LoadError { path, line: None, message: format!("cannot read: {e}") });
                continue;
            }
        };
        match load_str(&path, &text) {
            Ok(p) => {
                if report.parsers.iter().any(|q| q.name() == p.name()) {
                    report.errors.push(LoadError { path, line: None, message: format!("duplicate parser name `{}`", p.name()) });
                } else {
                    report.parsers.push(p);
                }
            }
            Err(e) => report.errors.push(e),
        }
    }
    Ok(report)
}
