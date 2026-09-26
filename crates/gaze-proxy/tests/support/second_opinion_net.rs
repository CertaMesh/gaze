//! A net whose re-run finds something new, so Resolve's `Strict` fallback refuses (todo 3847).
//!
//! First sweep: flags `alpha` as a name, which Resolve tokenizes. Every later sweep, with
//! `alpha` gone, flags `beta` as a location: a residual the fallback must refuse.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};

pub const TEXT: &str = "alpha beta";

pub struct SecondOpinionNet;

impl SafetyNet for SecondOpinionNet {
    fn id(&self) -> &str {
        "synthetic-second-opinion"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let (word, class) = if text.contains("alpha") {
            ("alpha", PiiClass::Name)
        } else {
            ("beta", PiiClass::Location)
        };
        Ok(text
            .find(word)
            .map(|start| {
                LeakSuspect::new(
                    start..start + word.len(),
                    class,
                    self.id(),
                    None,
                    LeakKind::Uncovered,
                    "synthetic",
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}
