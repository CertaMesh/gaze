//! gaze-token-bridge — owner-side authorization + translation bridge between a
//! short-lived [`RedactionSession`] and long-lived, policy-scoped IndexDomains.
//!
//! # Frozen contract (master-owned)
//! The modules [`model`], [`error`], [`session`], [`traits`], and [`util`] are the
//! shared contract. Sub-orchestrators OBEY them; they never re-derive or edit them.
//! A change to any of these is a master decision, made once and re-injected.
//!
//! # Implementation module ownership (one track each — disjoint file surface)
//! - **Track A** (policy & projection): [`registry`], [`policy`], [`projection`], [`keys`]
//! - **Track B** (index & search):      [`adapter`], [`ingest`]
//! - **Track C** (bridge runtime, gated): [`bridge`], [`capability`], [`translate`], [`audit`]
//!
//! Reference implementation (lift from, do not depend on): the throwaway spike crate
//! `crates/spike-token-bridge` on branch `agent/tokenbridge-mvp-spike` (22 passing tests).
//!
//! # Invariants (north-star axis 1 — never leak)
//! - Raw PII and the session manifest never reach agent-visible output.
//! - Index-domain aliases / fingerprints never reach agent-visible output.
//! - The LLM never decides authorization; `purpose` is owner-bound.
//! - Default-deny: no matching allow rule ⇒ deny. Every error path fails closed.
//! - HMAC projection is `(tenant, domain)`-keyed only — never salted with principal.

// --- Frozen contract (master) ---
pub mod error;
pub mod model;
pub mod session;
pub mod traits;
pub mod util;

// --- Track A: policy & projection ---
pub mod keys;
pub mod policy;
pub mod projection;
pub mod registry;

// --- Track B: index & search ---
pub mod adapter;
pub mod ingest;
pub mod persistent;

// --- Track C: bridge runtime (gated) ---
pub mod audit;
pub mod bridge;
pub mod capability;
pub mod translate;

pub use error::{BridgeError, DenyReason};
pub use model::*;
pub use session::RedactionSession;
pub use traits::{
    BridgeAuditSink, DomainProjector, KeyManager, PolicyGate, ResponseTranslator, SearchAdapter,
};
