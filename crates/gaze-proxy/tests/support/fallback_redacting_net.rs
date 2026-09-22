//! Net-only marker that is routed into the Redact FALLBACK rather than resolved.
//!
//! [`route_net::RouteNet`](super::route_net) reports one suspect, which the `Resolve` default
//! tokenizes: that mints a manifest entry whose `raw_span` the proxy's residual checks can see.
//! This net reports two OVERLAPPING `Uncovered` suspects over the same marker, so
//! `resolve_safety_net_suspects` refuses with `FallbackReason::OverlapConflict` and the marker
//! is REDACTED by the fallback instead.
//!
//! The fallback used to DELETE it, writing the empty string and emitting no manifest entry at
//! all -- a successful clean with an empty manifest, which a proxy check reading only the manifest
//! took for "nothing was found" while it still held the original bytes. The fallback now writes a
//! one-way `[REDACTED:<class>]` marker and records it as a manifest entry standing for the
//! original bytes, so the residual check has a raw span to test on the ordinary path.
//!
//! The wire-level proofs that use this net assert the OUTCOME -- provider-origin PII never
//! reaches the client -- and hold across that change. `safety_net_fallback_redaction_state.rs`
//! pins the precondition they now depend on.
//!
//! Requires no model, no subprocess and no network: the marker is a literal `find`.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Synthetic marker. Never real PII (AGENTS.md rule 2).
pub const MARKER: &str = "deleted@example.invalid";

pub struct FallbackRedactingNet {
    pub hits: Arc<AtomicUsize>,
}

impl SafetyNet for FallbackRedactingNet {
    fn id(&self) -> &str {
        "synthetic-fallback-redacting-net"
    }

    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }

    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let Some(start) = text.find(MARKER) else {
            // The terminal re-scan runs after the redaction and must find nothing, otherwise the
            // fallback fails closed on its own and the state under test is never reached.
            return Ok(vec![]);
        };
        self.hits.fetch_add(1, Ordering::SeqCst);
        let suspect = |end: usize, label: &str| {
            LeakSuspect::new(
                start..end,
                PiiClass::Email,
                self.id(),
                None,
                LeakKind::Uncovered,
                label,
                None,
            )
        };
        Ok(vec![
            suspect(start + MARKER.len(), "synthetic net-only marker"),
            // Shares `start`, so the two resolution plans overlap and resolve refuses.
            suspect(start + MARKER.len() - 1, "synthetic overlapping marker"),
        ])
    }
}
