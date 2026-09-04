#![allow(dead_code)]
use std::path::PathBuf;

use ulpf_parse::{Context, Parsed, Parser, load_str};

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 2026-09-04T12:00:00Z, UTC default.
pub fn ctx() -> Context {
    Context { receipt_epoch_nanos: 1_788_523_200_000_000_000, default_offset_secs: 0 }
}

pub fn parser(toml: &str) -> Parser {
    load_str(std::path::Path::new("<inline>"), toml).unwrap_or_else(|e| panic!("{e}"))
}

pub fn events(path: &std::path::Path) -> Vec<Vec<u8>> {
    let bytes = std::fs::read(path).unwrap();
    ulpf_store::Framer::new(&bytes, true).map(|r| bytes[r].to_vec()).collect()
}

pub fn field<'a>(p: &'a Parsed<'a>, key: &str) -> Option<&'a [u8]> {
    p.get(key.as_bytes()).map(|v| &**v)
}

#[track_caller]
pub fn assert_field(p: &Parsed<'_>, key: &str, expect: &[u8]) {
    match p.get(key.as_bytes()) {
        Some(v) => assert_eq!(&**v, expect, "field {key}: got {:?}", String::from_utf8_lossy(v)),
        None => panic!("field {key} missing; have {:?}", p.fields.iter().map(|f| String::from_utf8_lossy(&f.key).into_owned()).collect::<Vec<_>>()),
    }
}

pub fn pairs(p: &Parsed<'_>) -> Vec<(Vec<u8>, Vec<u8>)> {
    p.fields.iter().map(|f| (f.key.to_vec(), f.value.to_vec())).collect()
}
