//! Track A — `DomainProjector` implementation: deterministic
//! `HMAC((tenant,domain) key, canonical_value)` → `IndexedEntityRef`. NEVER salt with
//! principal. Owner: sub-orchestrator A.
//! Contract: `crate::traits::DomainProjector`. Reference: spike `DomainProjection`.
