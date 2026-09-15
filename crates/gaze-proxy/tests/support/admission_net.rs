//! Net behavior at real request admission, without external models.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Reflag,
    Malformed,
    Spill,
    Error,
}

pub struct AdmissionNet {
    pub mode: Mode,
    pub hits: Arc<AtomicUsize>,
}

impl SafetyNet for AdmissionNet {
    fn id(&self) -> &str {
        "synthetic-admission"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        context: SafetyNetContext<'_>,
    ) -> std::result::Result<Vec<LeakSuspect>, SafetyNetError> {
        let Some(span) = context.manifest.spans.first() else {
            return Ok(vec![]);
        };
        assert!(!text.contains("alice@example.invalid"));
        self.hits.fetch_add(1, Ordering::SeqCst);
        let range = match self.mode {
            Mode::Reflag => span.clean_span.clone(),
            Mode::Malformed => span.clean_span.start..span.clean_span.start,
            Mode::Spill => span.clean_span.start..text.len(),
            Mode::Error => {
                return Err(SafetyNetError::Runtime {
                    message: "synthetic admission failure".into(),
                })
            }
        };
        Ok(vec![LeakSuspect::new(
            range,
            PiiClass::Name,
            self.id(),
            None,
            LeakKind::ClassMismatch {
                pipeline_class: PiiClass::Email,
                safety_net_class: PiiClass::Name,
            },
            "synthetic",
            None,
        )])
    }
}
