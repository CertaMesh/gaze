//! Track B — ingest pipeline: redact-before-index (run corpus text through the gaze
//! Pipeline so raw PII NEVER enters the index), canonicalize + project entities, build
//! the index. Add a CI gate asserting no raw PII enters the store. Owner: sub-orchestrator B.
//! Contract: `crate::model::{CanonicalEntity, IndexEntity, IndexSearchHit}`, `crate::util::domain_alias`.
//! Reference: spike `build_internal_hit` / `synthetic_docs`.
