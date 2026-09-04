//! The mapping file format (TOML), one schema per file set. A mapping speaks only in
//! source field names and source values; there is no key in which a vendor or parser
//! identity could be written, and `deny_unknown_fields` rejects any attempt.
//!
//! Several files may carry the same `[schema] name`; they are merged in file-name order
//! so four teammates can add aliases for their families without editing one shared file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingFile {
    pub schema: Schema,
    /// schema path → source field names, first present wins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub types: Types,
    #[serde(default)]
    pub values: Values,
    #[serde(default, rename = "enum", skip_serializing_if = "Vec::is_empty")]
    pub enums: Vec<EnumSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class: Vec<ClassRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_class: Option<ClassRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Schema paths whose values are emitted as JSON numbers when the text is numeric.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Types {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub int: Vec<String>,
}

/// Source values that carry no information.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Values {
    /// Values meaning "not present" (`-`, `N/A`, empty), compared case-insensitively. A
    /// field holding one is neither mapped nor reported as unmapped, and does not satisfy
    /// a class condition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<String>,
}

/// Canonical values for one schema field, plus the sibling id field OCSF pairs with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumSpec {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_field: Option<String>,
    /// Emitted when no source field feeds `field` at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown: Option<EnumValue>,
    /// Emitted when a source value is present but in no list; the raw value goes to `unmapped`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<EnumValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumValue {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// Raw source values, matched case-insensitively.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<String>,
    /// Raw values that count only when they came from this source field (`alert.severity`
    /// uses 1 = high where syslog uses 1 = alert).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw_by_field: BTreeMap<String, Vec<String>>,
}

/// Class selection. `when` is a list of alternatives; within one alternative every
/// listed source field must equal one of its values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassRule {
    pub uid: i64,
    pub name: String,
    pub category_uid: i64,
    pub category_name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub constants: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<BTreeMap<String, Vec<String>>>,
}
