//! Deterministic net-only marker, independent of primary email recognition.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
pub const RESIDUAL: &str = "residual@example.invalid";
pub struct RouteNet {
    pub error: bool,
    pub hits: Arc<AtomicUsize>,
}
impl SafetyNet for RouteNet {
    fn id(&self) -> &str {
        "synthetic-route-net"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let Some(start) = text.find(RESIDUAL) else {
            return Ok(vec![]);
        };
        self.hits.fetch_add(1, Ordering::SeqCst);
        if self.error {
            return Err(SafetyNetError::Runtime {
                message: "synthetic route failure".into(),
            });
        }
        Ok(vec![LeakSuspect::new(
            start..start + RESIDUAL.len(),
            PiiClass::Email,
            self.id(),
            None,
            LeakKind::Uncovered,
            "synthetic net-only marker",
            None,
        )])
    }
}
