//! Session blob schema.
//!
//! A `SessionBlob` carries the mapping from placeholder tokens back to raw
//! values. It is opaque to callers: we serialize to JSON, then wrap in
//! base64 so Laravel can transport it as a single string.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::errors::{RestoreError, SanitizeError};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionBlob {
    pub schema_version: u32,
    /// Map from placeholder token (e.g. "<CUSTOMER_NAME>") to raw value.
    /// Uses BTreeMap so the serialized form is deterministic.
    pub placeholders: BTreeMap<String, String>,
}

impl SessionBlob {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            placeholders: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, placeholder: impl Into<String>, raw: impl Into<String>) {
        self.placeholders.insert(placeholder.into(), raw.into());
    }

    pub fn encode(&self) -> Result<String, SanitizeError> {
        let json = serde_json::to_vec(self)
            .map_err(|e| SanitizeError::BlobEncoding(e.to_string()))?;
        Ok(B64.encode(json))
    }

    pub fn decode(s: &str) -> Result<Self, RestoreError> {
        if s.is_empty() {
            return Err(RestoreError::MissingSessionBlob);
        }
        let bytes = B64
            .decode(s.as_bytes())
            .map_err(|e| RestoreError::InvalidSessionBlob(format!("base64: {e}")))?;
        let blob: SessionBlob = serde_json::from_slice(&bytes)
            .map_err(|e| RestoreError::InvalidSessionBlob(format!("json: {e}")))?;
        if blob.schema_version != SCHEMA_VERSION {
            return Err(RestoreError::InvalidSessionBlob(format!(
                "unsupported schema_version {}",
                blob.schema_version
            )));
        }
        Ok(blob)
    }
}

impl Default for SessionBlob {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_blob_roundtrips() {
        let b = SessionBlob::new();
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
    }

    #[test]
    fn blob_with_entries_roundtrips() {
        let mut b = SessionBlob::new();
        b.insert("<CUSTOMER_NAME>", "Markus Mueller");
        b.insert("<EMAIL_1>", "markus.mueller@example.de");
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
        assert_eq!(
            decoded.placeholders.get("<CUSTOMER_NAME>").unwrap(),
            "Markus Mueller"
        );
    }

    #[test]
    fn decode_empty_string_returns_missing() {
        let err = SessionBlob::decode("").unwrap_err();
        assert!(matches!(err, RestoreError::MissingSessionBlob));
    }

    #[test]
    fn decode_invalid_base64_returns_invalid() {
        let err = SessionBlob::decode("not-base64-!!!").unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn decode_rejects_wrong_schema_version() {
        let bad = serde_json::json!({
            "schema_version": 999,
            "placeholders": {}
        });
        let encoded = B64.encode(serde_json::to_vec(&bad).unwrap());
        let err = SessionBlob::decode(&encoded).unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut a = SessionBlob::new();
        a.insert("<B>", "second");
        a.insert("<A>", "first");
        let mut b = SessionBlob::new();
        b.insert("<A>", "first");
        b.insert("<B>", "second");
        assert_eq!(a.encode().unwrap(), b.encode().unwrap());
    }
}
