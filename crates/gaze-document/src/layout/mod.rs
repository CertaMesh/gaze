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
    /// Build a reading-order handle.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`] until the
    /// multi-page PR lands. Single-page bundles never call this; the
    /// placeholder fails loudly so adopters cannot stumble into silent
    /// empty-order output (Axis 1 fail-closed).
    pub fn new() -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "ReadingOrder::new (multi-page deferred to follow-up PR)",
        ))
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
