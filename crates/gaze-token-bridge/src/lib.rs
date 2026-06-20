//! gaze-token-bridge — owner-side authorization + translation bridge between a
//! short-lived [`RedactionSession`] and long-lived, policy-scoped IndexDomains.
//!
//! # Contract modules
//! The modules [`model`], [`error`], [`session`], [`traits`], and [`util`] are the
//! shared public contract.
//!
//! # Implementation modules
//! - **Policy and projection**: [`registry`], [`policy`], [`projection`], [`keys`]
//! - **Index and search**: [`adapter`], [`ingest`]
//! - **Bridge runtime**: [`bridge`], [`capability`], [`translate`], [`audit`]
//!
//! # Invariants
//! - Raw PII and the session manifest never reach agent-visible output.
//! - Index-domain aliases / fingerprints never reach agent-visible output.
//! - The LLM never decides authorization; `purpose` is owner-bound.
//! - Default-deny: no matching allow rule ⇒ deny. Every error path fails closed.
//! - HMAC projection is `(tenant, domain)`-keyed only — never salted with principal.

// --- Shared contract ---
pub mod error;
pub mod model;
pub mod session;
pub mod traits;
pub mod util;

// --- Policy and projection ---
pub mod keys;
pub mod policy;
pub mod projection;
pub mod registry;

// --- Index and search ---
pub mod adapter;
pub mod ingest;
pub mod persistent;

// --- Bridge runtime ---
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
