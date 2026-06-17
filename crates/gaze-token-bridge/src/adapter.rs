//! Track B — `SearchAdapter` implementation: the real corpus index (store + query by
//! `IndexedEntityRef`), replacing the spike's in-memory fake. Enforces
//! entity_ref/domain/expiry/nonce guards defensively. Owner: sub-orchestrator B.
//! Contract: `crate::traits::SearchAdapter`, `crate::model::{ValidatedSearchRequest, IndexSearchHit}`.
//! Reference: spike `InMemorySearchAdapter`.
