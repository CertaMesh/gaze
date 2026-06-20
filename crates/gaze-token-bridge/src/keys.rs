//! Track A — `KeyManager`: per-domain projection key material + rotation (retain old
//! keys for read, addressed by `key_id`; missing key fails closed). Owner: sub-orchestrator A.
//! Contract: `crate::traits::KeyManager`. Reference: spike `IndexDomainRegistry::projection_key`.
