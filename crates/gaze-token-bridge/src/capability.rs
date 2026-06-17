//! Track C (gated) — capability issuance + lifecycle: mint entity-bound `SearchHandle`s
//! (nonce, expiry, single-use), validate them (entity_ref match, not expired, nonce
//! unused), project raw filter values before they reach the adapter. Owner: sub-orchestrator C.
//! Contract: `crate::model::{SearchHandle, SearchRequest, ValidatedSearchRequest}`.
//! Reference: spike `TokenBridge::{issue_handle, project_search_request}`.
