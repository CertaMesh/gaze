//! OpenAI Privacy Filter safety-net adapter.
//!
//! Phase 4.1 command choice: Gaze binds to the official `openai/privacy-filter`
//! CLI (`opf --format json --output-mode typed`) installed from a pinned Git
//! revision or an official release. The official CLI has the clearest provenance
//! and documents pipe input plus a stable JSON schema. We intentionally do not
//! use the `chiefautism/privacy-parser` fork by default; it should only become a
//! backend if a later review confirms native byte spans and no PII-bearing JSON
//! fields cross the adapter boundary.
//!
//! The official OPF JSON contains PII-bearing `text` and `placeholder` fields.
//! This module treats those fields as private deserialization details and
//! reduces output to byte offsets, labels, and scores before returning anything
//! to the rest of Gaze.

pub mod backend;
pub mod class_map;

pub use backend::{OpenAiFilterBackend, RawSpan};
pub use class_map::{
    map_openai_label, openai_label_to_safety_net_class, validate_openai_label,
};
