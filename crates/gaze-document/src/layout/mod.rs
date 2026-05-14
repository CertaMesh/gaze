//! Layout / reading-order helpers.
//!
//! OCR backends emit flat spans. This module keeps the reading-order policy in
//! `gaze-document` so downstream recognizers receive coherent text blocks
//! instead of row-wise interleaving from multi-column pages.

use crate::DocumentError;

/// Reading-order handle for a single document.
///
/// Pre-0.1 placeholder for the eventual public multi-page layout contract.
#[non_exhaustive]
pub struct ReadingOrder {
    _private: (),
}

impl ReadingOrder {
    /// Build a reading-order handle.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`] until the public
    /// reading-order contract lands.
    pub fn new() -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "ReadingOrder::new (public reading-order contract deferred)",
        ))
    }

    /// Infer reading order from raw page payloads.
    ///
    /// # Errors
    /// Returns [`DocumentError::NotImplemented`] until the public
    /// reading-order contract lands.
    pub fn infer(_pages: &[&[u8]]) -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented(
            "ReadingOrder::infer (public reading-order contract deferred)",
        ))
    }
}
