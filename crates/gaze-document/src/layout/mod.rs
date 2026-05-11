//! Layout / reading-order contract surface.
//!
//! Concrete extraction (column detection, table flattening, etc.) lands in
//! follow-up PRs. This module currently reserves only the [`ReadingOrder`]
//! handle and its fail-loud constructor path.

use crate::DocumentError;

/// Reading-order handle for a single document.
///
/// Pre-0.1 placeholder. Eventual shape encodes per-page block sequencing
/// so OCR + layout can be reassembled into deterministic Markdown.
#[non_exhaustive]
#[derive(Default)]
pub struct ReadingOrder {
    _private: (),
}

impl ReadingOrder {
    /// Reserved constructor.
    ///
    /// Returns an empty handle today; concrete inference lives in
    /// [`ReadingOrder::infer`].
    #[must_use]
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Infer reading order from raw page payloads.
    ///
    /// # Errors
    /// Always returns [`DocumentError::NotImplemented`] until the layout
    /// PR lands.
    pub fn infer(_pages: &[&[u8]]) -> Result<Self, DocumentError> {
        Err(DocumentError::NotImplemented("ReadingOrder::infer"))
    }
}
