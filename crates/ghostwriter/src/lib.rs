//! Ghostwriter — deterministic text sanitization and exact-token restoration.
//!
//! See `docs/superpowers/specs/2026-04-11-ghostwriter-sanitization-design.md`
//! for the full design.

pub mod blob;
pub mod detect;
pub mod errors;
pub mod known_context;
pub mod placeholder;
pub mod restore;
pub mod sanitize;
pub mod typed_unknown;
pub mod types;

pub use errors::{RestoreError, SanitizeError};
pub use restore::restore;
pub use sanitize::sanitize;
pub use types::{
    Context, Metadata, RestoreRequest, RestoreResponse, SanitizeRequest, SanitizeResponse, Warning,
};
