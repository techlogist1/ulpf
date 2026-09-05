//! The engine: memory-map inputs, frame, store raw bytes, then detect → parse →
//! normalize → JSON Lines on a bounded, ordered, multi-threaded pipeline, with every
//! stage counted. `pipeline::Pipeline` is the single per-event code path shared by the
//! engine workers and the fixture harness.

pub mod cli;
pub mod engine;
pub mod fixture;
pub mod inference;
pub mod metrics;
pub mod pending;
pub mod pipeline;
pub mod pivot;
pub mod replay;
pub mod server;
pub mod tail;
