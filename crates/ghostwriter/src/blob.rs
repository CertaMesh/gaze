//! Session blob schema.
//!
//! A `SessionBlob` carries the opaque `gaze::SensitiveSnapshot` plus the
//! outward placeholder aliases ghostwriter exposes to callers. Laravel still
//! sees a single opaque base64 string.

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gaze::SensitiveSnapshot;
use serde::{Deserialize, Serialize};

use crate::errors::{RestoreError, SanitizeError};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AliasEntry {
    pub external: String,
    pub internal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionBlob {
    pub schema_version: u32,
    pub snapshot: String,
    pub aliases: Vec<AliasEntry>,
}

impl SessionBlob {
    pub fn new(snapshot: SensitiveSnapshot) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            snapshot: B64.encode(snapshot.into_bytes()),
            aliases: Vec::new(),
        }
    }

    pub fn insert_alias(
        &mut self,
        external: impl Into<String>,
        internal: impl Into<String>,
    ) {
        self.aliases.push(AliasEntry {
            external: external.into(),
            internal: internal.into(),
        });
    }

    pub fn snapshot(&self) -> Result<SensitiveSnapshot, RestoreError> {
        let bytes = B64
            .decode(self.snapshot.as_bytes())
            .map_err(|e| RestoreError::InvalidSessionBlob(format!("snapshot base64: {e}")))?;
        Ok(bytes.into())
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
        Self::new(Vec::new().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_blob_roundtrips() {
        let b = SessionBlob::new(vec![1, 2, 3].into());
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
    }

    #[test]
    fn blob_with_aliases_roundtrips() {
        let mut b = SessionBlob::new(vec![1, 2, 3].into());
        b.insert_alias("<CUSTOMER_NAME>", "CustomerName_1");
        b.insert_alias("<EMAIL_1>", "Email_1");
        let encoded = b.encode().unwrap();
        let decoded = SessionBlob::decode(&encoded).unwrap();
        assert_eq!(b, decoded);
        assert_eq!(decoded.snapshot().unwrap().into_bytes(), vec![1, 2, 3]);
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
            "snapshot": "",
            "aliases": []
        });
        let encoded = B64.encode(serde_json::to_vec(&bad).unwrap());
        let err = SessionBlob::decode(&encoded).unwrap_err();
        assert!(matches!(err, RestoreError::InvalidSessionBlob(_)));
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut a = SessionBlob::new(vec![1, 2, 3].into());
        a.insert_alias("<B>", "Email_2");
        a.insert_alias("<A>", "Email_1");
        let mut b = SessionBlob::new(vec![1, 2, 3].into());
        b.insert_alias("<B>", "Email_2");
        b.insert_alias("<A>", "Email_1");
        assert_eq!(a.encode().unwrap(), b.encode().unwrap());
    }
}
