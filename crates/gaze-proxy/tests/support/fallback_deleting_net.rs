//! Net-only marker that is routed into the Redact FALLBACK rather than resolved.
//!
//! [`route_net::RouteNet`](super::route_net) reports one suspect, which the `Resolve` default
//! tokenizes: that mints a manifest entry whose `raw_span` the proxy's residual checks can see.
//! This net reports two OVERLAPPING `Uncovered` suspects over the same marker, so
//! `resolve_safety_net_suspects` refuses with `FallbackReason::OverlapConflict` and the marker
//! is DELETED by the fallback instead. Deletion writes `("" , None)` through
//! `replace_clean_span_checked`, so it emits no manifest entry at all.
//!
//! That is the state under test: the pipeline succeeded (`Ok`), the clean text is free of the
//! marker, and the manifest is empty. Any proxy check that reads only the manifest sees
//! "nothing was found" and admits the ORIGINAL bytes it still holds.
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

pub struct FallbackDeletingNet {
    pub hits: Arc<AtomicUsize>,
}

impl SafetyNet for FallbackDeletingNet {
    fn id(&self) -> &str {
        "synthetic-fallback-deleting-net"
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
            // The terminal re-scan runs after deletion and must find nothing, otherwise the
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
