#![cfg_attr(docsrs, feature(doc_cfg))]
//! Built-in recognizer backends for the Gaze pseudonymization pipeline.
//!
//! ## Feature flags
//!
//! | Feature | Default | What it enables |
//! |---------|---------|-----------------|
//! | `phone-parser` | yes | Parser-backed E.164 and national phone validation via `phonenumber` |
//! | `safety-net` | no | `NerSafetyNet` observer pass |
//! | `safety-net-openai` | no | OpenAI-filter safety net subprocess; also enables `safety-net` |
//! | `test-support` | no | Fixture helpers for safety-net tests; not for production use |
//!
//! With `phone-parser` disabled, parser-backed phone validators fail closed. There is
//! no silent regex-only fallback, so phone spans that require parser validation are not
//! detected.

mod anchored_match;
mod dictionary;
mod error;
mod locale_aware;
mod ner;
mod regex;
#[cfg(feature = "safety-net")]
pub mod safety_net;
pub mod validators;

pub use anchored_match::{
    is_person_name_candidate, AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape,
};
pub use dictionary::DictionaryRecognizer;
pub use error::{RecognizerError, Result};
#[cfg(feature = "phone-parser")]
pub use gaze_types::Region;
pub use gaze_types::{SafetyTier, ValidatorKind};
pub use locale_aware::{
    LocaleAwareModel, LocaleAwareModelRegistry, ModelError, ModelHints, ModelInput, ModelSpan,
    ModelStage,
};
pub use ner::{
    LabelMap, NerBackendKind, NerDetector, NerLoadError, NerOptions, NerRecognizer,
    VerifiedArtifacts,
};
pub use regex::{NormalizerKind, RegexDetector};

pub fn embedded(name: &str) -> Option<&'static str> {
    match name {
        "core" | "core-extended" => Some(include_str!("../embedded/core.toml")),
        "locale-de" => Some(include_str!("../embedded/locale-de.toml")),
        "locale-en" => Some(include_str!("../embedded/locale-en.toml")),
        "locale-br" => Some(include_str!("../embedded/locale-br.toml")),
        "locale-fr" => Some(include_str!("../embedded/locale-fr.toml")),
        "locale-in" => Some(include_str!("../embedded/locale-in.toml")),
        "locale-nl" => Some(include_str!("../embedded/locale-nl.toml")),
        "locale-uk" => Some(include_str!("../embedded/locale-uk.toml")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::embedded;
    use gaze::{RawMatch, Rulepack, RulepackSource};

    #[test]
    fn embedded_core_rulepack_parses_and_contains_unified_recognizers() {
        let core = embedded("core").expect("core rulepack");
        let rulepack = Rulepack::load(RulepackSource::Embedded(core)).expect("valid core");

        assert_eq!(rulepack.recognizers.len(), 26);
        assert_eq!(rulepack.recognizers[0].id, "email.global");
        assert_eq!(rulepack.recognizers[1].id, "email.header.name");
        assert_eq!(rulepack.recognizers[2].id, "email.header.name.paren");
        assert_eq!(rulepack.recognizers[3].id, "name.forward_marker");
        assert_eq!(rulepack.recognizers[4].id, "name.agent_recipient");
        assert_eq!(rulepack.recognizers[5].id, "name.auto_footer");
        assert!(matches!(
            rulepack.recognizers[0].matcher,
            RawMatch::Regex { .. }
        ));
        assert!(matches!(
            rulepack.recognizers[1].matcher,
            RawMatch::Regex { .. }
        ));
        assert!(matches!(
            rulepack.recognizers[2].matcher,
            RawMatch::Regex { .. }
        ));
        assert!(rulepack.recognizers[3..]
            .iter()
            .take(3)
            .all(|recognizer| matches!(recognizer.matcher, RawMatch::AnchoredMatch { .. })));
        assert!(rulepack.recognizers[6..]
            .iter()
            .all(|recognizer| matches!(recognizer.matcher, RawMatch::Regex { .. })));
    }

    #[test]
    fn embedded_core_extended_aliases_core() {
        assert_eq!(embedded("core-extended"), embedded("core"));
        let core_extended = embedded("core-extended").expect("core-extended rulepack");
        let rulepack =
            Rulepack::load(RulepackSource::Embedded(core_extended)).expect("valid core-extended");

        assert_eq!(rulepack.recognizers.len(), 26);
        assert!(rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.id == "phone.structural"));
    }
}
