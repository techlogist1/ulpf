//! Shape 2 of the ULPF data model: the device's own field names, nothing about any
//! output schema. This crate never depends on `ulpf-normalize`; that absence is the
//! parser/mapping wall.
//!
//! `load_dir` reads `parsers/*.toml`; `Registry::detect` picks a parser for an event;
//! `Parser::parse` fills a `Parsed` with borrowed byte ranges of the event.

pub mod def;
pub mod template;

mod compile;
mod delimiter;
mod detect;
mod envelope;
mod kv;
mod load;
mod pattern;
mod structured;

use std::borrow::Cow;

pub use compile::{ParseFailure, Parser, Scratch, SubStatus};
pub use def::*;
pub use detect::Registry;
pub use load::{LoadError, LoadReport, load_dir, load_str};
pub use template::{SlotKind, Template, Token};
pub use ulpf_time::{Context, Policies, Timestamp};

pub type Bytes<'a> = Cow<'a, [u8]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field<'a> {
    pub key: Bytes<'a>,
    pub value: Bytes<'a>,
}

/// Output of one parse. Reused across events: `clear()` keeps the allocation.
#[derive(Debug, Default)]
pub struct Parsed<'a> {
    pub fields: Vec<Field<'a>>,
    /// Event time from the definition's `[[timestamp]]` candidates or the syslog header.
    /// `None` means no candidate carried a value; the caller falls back to receipt time.
    pub timestamp: Option<Timestamp>,
    /// The original timestamp text, retained verbatim for the output.
    pub timestamp_text: Option<Bytes<'a>>,
    /// Reason the last present-but-unparseable candidate failed, if any.
    pub timestamp_error: Option<&'static str>,
    pub sub: SubStatus,
    /// Buffer a multi-field timestamp is joined into; handed out as `timestamp_text` and
    /// taken back by `clear`, so the join never allocates after the first event.
    spare: Vec<u8>,
}

impl<'a> Parsed<'a> {
    pub fn clear(&mut self) {
        self.fields.clear();
        self.timestamp = None;
        if let Some(Cow::Owned(v)) = self.timestamp_text.take() {
            self.spare = v;
        }
        self.timestamp_error = None;
        self.sub = SubStatus::NotApplicable;
    }

    /// The reusable join buffer, empty. Return it through `timestamp_text` or `give_back`.
    pub(crate) fn take_spare(&mut self) -> Vec<u8> {
        let mut v = std::mem::take(&mut self.spare);
        v.clear();
        v
    }

    pub(crate) fn give_back(&mut self, v: Vec<u8>) {
        self.spare = v;
    }

    #[inline]
    pub fn push(&mut self, key: impl Into<Bytes<'a>>, value: impl Into<Bytes<'a>>) {
        self.fields.push(Field { key: key.into(), value: value.into() });
    }

    /// First field with this key.
    pub fn get(&self, key: &[u8]) -> Option<&Bytes<'a>> {
        self.fields.iter().find(|f| &*f.key == key).map(|f| &f.value)
    }
}
