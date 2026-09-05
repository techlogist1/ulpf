//! Shape 1 of the ULPF data model: raw bytes, preserved before anything understands them.
//!
//! `frame` splits input bytes into events without loss. `RawStore` appends events to an
//! on-disk segment and offsets index; `RawReader` reads them back. There is no API that
//! modifies or removes an existing record: immutability is a property of the interface.
//! Each index entry also carries the record's chain value, so a rewrite of any record is
//! provable against an `Attestation` taken before it (`RawReader::verify_against`).

pub mod frame;
mod store;

pub use frame::Framer;
pub use store::{
    Attestation, CHECKPOINT_EVERY, Checkpoint, OwnedRecord, RawId, RawReader, RawRecord, RawStore, VerifyReason, VerifyReport, hex,
};
