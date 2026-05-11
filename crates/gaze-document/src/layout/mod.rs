//! Layout / reading-order contract surface.
//!
//! Concrete extraction (column detection, multi-page reading order, table
//! flattening) lands in follow-up PRs. This module reserves the
//! [`ReadingOrder`] handle so adopters can pin against the eventual contract.

use crate::DocumentError;

/// Reading-order handle for a single document.
///
/// Pre-0.1 placeholder — single-page bundles flow through [`crate::clean`]
/// without consulting this type. Multi-page output will route through
/// [`ReadingOrder::infer`] in a follow-up PR.
#[non_exhaustive]
pub struct ReadingOrder {
    _private: (),
}

impl ReadingOrder {
    /// Build an empty reading-order handle (no inference performed).
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Infer reading order from raw page payloads.
    ///
    /// # Errors
    /// Returns [`DocumentError::NotImplemented`] until the multi-page PR
    /// lands. Single-page bundles do not need this path.
    pub fn infer(_pages: &[&[u8]]) -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "ReadingOrder::infer (multi-page deferred to follow-up PR)",
        ))
    }
}

impl Default for ReadingOrder {
    fn default() -> Self {
        Self::new()
    }
}
