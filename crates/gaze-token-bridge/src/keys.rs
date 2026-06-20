//! Per-domain projection key material and rotation.
//! Old keys are retained for reads; missing keys fail closed.

use std::collections::HashMap;

use crate::error::BridgeError;
use crate::traits::KeyManager;

/// Projection keys retained by stable `key_id`.
///
/// Rotation is read-compatible by construction: loading a new active key for a
/// domain does not remove old key material from this store.
#[derive(Debug, Clone, Default)]
pub struct ProjectionKeyStore {
    keys: HashMap<String, Vec<u8>>,
}

impl ProjectionKeyStore {
    pub fn from_pairs<I>(pairs: I) -> Result<Self, BridgeError>
    where
        I: IntoIterator<Item = (String, Vec<u8>)>,
    {
        let mut store = Self::default();
        for (key_id, material) in pairs {
            store.insert(key_id, material)?;
        }
        Ok(store)
    }

    pub fn insert(&mut self, key_id: String, material: Vec<u8>) -> Result<(), BridgeError> {
        if key_id.is_empty() {
            return Err(BridgeError::Policy(
                "projection key id must not be empty".to_string(),
            ));
        }
        if material.is_empty() {
            return Err(BridgeError::Policy(format!(
                "projection key {key_id} material must not be empty"
            )));
        }
        if self.keys.insert(key_id.clone(), material).is_some() {
            return Err(BridgeError::Policy(format!(
                "duplicate projection key {key_id}"
            )));
        }
        Ok(())
    }

    pub fn contains_key(&self, key_id: &str) -> bool {
        self.keys.contains_key(key_id)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl KeyManager for ProjectionKeyStore {
    fn key(&self, key_id: &str) -> Option<&[u8]> {
        self.keys.get(key_id).map(Vec::as_slice)
    }
}
