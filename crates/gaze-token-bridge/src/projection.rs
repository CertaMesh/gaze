//! Track A — `DomainProjector` implementation: deterministic
//! `HMAC((tenant,domain) key, canonical_value)` → `IndexedEntityRef`. NEVER salt with
//! principal. Owner: sub-orchestrator A.
//! Contract: `crate::traits::DomainProjector`. Reference: spike `DomainProjection`.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::BridgeError;
use crate::model::{CanonicalEntity, IndexDomain, IndexedEntityRef};
use crate::traits::{DomainProjector, KeyManager};
use crate::util::hex;

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 projector keyed by the target domain's `projection_key_id`.
#[derive(Debug)]
pub struct HmacDomainProjector<'a, K: KeyManager + ?Sized> {
    keys: &'a K,
}

impl<'a, K: KeyManager + ?Sized> HmacDomainProjector<'a, K> {
    pub fn new(keys: &'a K) -> Self {
        Self { keys }
    }
}

impl<K: KeyManager + ?Sized> DomainProjector for HmacDomainProjector<'_, K> {
    fn project(
        &self,
        domain: &IndexDomain,
        entity: &CanonicalEntity,
    ) -> Result<IndexedEntityRef, BridgeError> {
        let key = self.keys.key(&domain.projection_key_id).ok_or_else(|| {
            BridgeError::Policy(format!(
                "missing projection key {} for domain {}",
                domain.projection_key_id, domain.domain_id
            ))
        })?;

        let mut mac = HmacSha256::new_from_slice(key)
            .map_err(|err| BridgeError::Policy(format!("invalid hmac key: {err:?}")))?;
        mac.update(entity.canonical_value.as_bytes());

        Ok(IndexedEntityRef {
            domain_id: domain.domain_id.clone(),
            key_id: domain.projection_key_id.clone(),
            entity_class: entity.class.clone(),
            fingerprint_hex: hex(mac.finalize().into_bytes().as_slice()),
        })
    }
}

pub type DomainProjection<'a> = HmacDomainProjector<'a, crate::registry::IndexDomainRegistry>;
