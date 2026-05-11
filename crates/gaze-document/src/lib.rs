//! Document ingestion + safe-bundle generation for the Gaze runtime.
//!
//! **Experimental scaffold (todo 728).** This crate currently exposes only
//! types and trait shapes. Every constructor is fail-loud — see the module
//! docs for `bundle`, `ocr`, `layout`, and `render`.
//!
//! Future PRs will add OCR adapters, layout extraction, and a CLI
//! sub-command. The contract surface is reserved here so adopters can pin
//! against it early.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod bundle;
pub mod layout;
pub mod ocr;
pub mod render;

pub use bundle::{BundleReport, LayoutSummary, SafeBundle};
pub use layout::ReadingOrder;
pub use ocr::OcrAdapter;
pub use render::Renderer;

/// Crate-level error type for `gaze-document`.
///
/// Pre-0.1: every variant is reserved until concrete adapters land.
#[non_exhaustive]
#[derive(Debug)]
pub enum DocumentError {
    /// The requested operation is part of the public contract but has no
    /// implementation yet. Returned by every stub constructor and trait
    /// method until follow-up PRs land.
    NotImplemented(&'static str),
}

impl core::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotImplemented(what) => {
                write!(f, "gaze-document: {what} is not yet implemented")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_compiles_and_error_renders() {
        let err = DocumentError::NotImplemented("smoke");
        assert!(err.to_string().contains("not yet implemented"));
    }
}
