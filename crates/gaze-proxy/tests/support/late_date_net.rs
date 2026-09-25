//! Stand-in for the PR #660 review repro `user jweber84 born 1984-03-12` under real Nym and the
//! `gaze setup` policy: the first sweep flags the username, and only the re-run, with the
//! username tokenized, flags the date. `gaze clean`'s default `redact` fallback tokenizes that
//! late date in a second reversible batch; the proxy runs the `strict` fallback and refuses.
use gaze::{
    LeakKind, LeakSuspect, LocaleTag, PiiClass, SafetyNet, SafetyNetContext, SafetyNetError,
};

pub const TEXT: &str = "user jweber84 born 1984-03-12";
const USERNAME: &str = "jweber84";
const DATE: &str = "1984-03-12";

pub struct LateDateNet;

impl SafetyNet for LateDateNet {
    fn id(&self) -> &str {
        "synthetic-late-date"
    }
    fn supported_locales(&self) -> &[LocaleTag] {
        &[LocaleTag::Global]
    }
    fn check(
        &self,
        text: &str,
        _: SafetyNetContext<'_>,
    ) -> Result<Vec<LeakSuspect>, SafetyNetError> {
        let (word, class) = if text.contains(USERNAME) {
            (USERNAME, "username")
        } else {
            (DATE, "date")
        };
        Ok(text
            .find(word)
            .map(|start| {
                LeakSuspect::new(
                    start..start + word.len(),
                    PiiClass::Custom(class.to_string()),
                    self.id(),
                    Some(0.95),
                    LeakKind::Uncovered,
                    "synthetic",
                    None,
                )
            })
            .into_iter()
            .collect())
    }
}
