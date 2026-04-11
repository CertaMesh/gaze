//! Error types.
//!
//! Error surface mirrors the spec's "Sanitize Errors" and "Restore Errors"
//! sections. Non-fatal situations are expressed via `Warning` in types.rs,
//! not as errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SanitizeError {
    #[error("invalid request payload: {0}")]
    InvalidRequest(String),

    #[error("detector failure: {0}")]
    DetectorFailure(String),

    #[error("placeholder mapping failure: {0}")]
    PlaceholderMapping(String),

    #[error("blob encoding failure: {0}")]
    BlobEncoding(String),
}

#[derive(Debug, Error)]
pub enum RestoreError {
    #[error("missing session blob")]
    MissingSessionBlob,

    #[error("invalid session blob: {0}")]
    InvalidSessionBlob(String),

    #[error("invalid request payload: {0}")]
    InvalidRequest(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_error_display_contains_detector_failure() {
        let e = SanitizeError::DetectorFailure("simple nlp died".into());
        assert!(e.to_string().contains("detector"));
        assert!(e.to_string().contains("simple nlp died"));
    }

    #[test]
    fn restore_error_invalid_blob_is_distinct_from_missing() {
        let missing = RestoreError::MissingSessionBlob;
        let invalid = RestoreError::InvalidSessionBlob("base64 decode failed".into());
        assert_ne!(missing.to_string(), invalid.to_string());
    }
}
