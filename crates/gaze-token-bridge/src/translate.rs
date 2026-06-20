//! Track C (gated) — `ResponseTranslator`: rewrite owner-side index snippets into the
//! active session namespace (mint fresh session tokens for newly-discovered entities),
//! fail closed if any domain alias or raw value would remain. Owner: sub-orchestrator C.
//! Contract: `crate::traits::ResponseTranslator`, `crate::util::contains_domain_alias`.
//! Reference: spike `ResponseTranslator`.
