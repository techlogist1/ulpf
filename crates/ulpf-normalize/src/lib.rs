//! Shape 3 of the ULPF data model: schema field names and canonical values. Receives
//! `(source field, value)` pairs and a provenance record; knows no vendor.
//!
//! `load_dir` reads `mappings/*.toml` and merges files by schema name. `Mapping::normalize`
//! writes one JSON line per event. Everything unmapped lands under `unmapped`, so a
//! field the mapping has not learned yet is visible in the output, not lost.

pub mod def;
mod load;
mod mapping;

pub use def::*;
pub use load::{LoadError, LoadReport, load_dir, load_files};
pub use mapping::{FieldProvenance, Mapping, NormalizeStats, Provenance};
