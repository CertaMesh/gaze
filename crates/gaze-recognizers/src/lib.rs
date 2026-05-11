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
mod ner;
mod regex;
#[cfg(feature = "safety-net")]
pub mod safety_net;

pub use anchored_match::{
    is_person_name_candidate, AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape,
};
pub use dictionary::DictionaryRecognizer;
pub use error::{RecognizerError, Result};
#[cfg(feature = "phone-parser")]
pub use gaze_types::Region;
pub use gaze_types::ValidatorKind;
pub use ner::{
    LabelMap, NerBackendKind, NerDetector, NerLoadError, NerOptions, NerRecognizer,
    VerifiedArtifacts,
};
pub use regex::{NormalizerKind, RegexDetector};

pub fn embedded(name: &str) -> Option<&'static str> {
    match name {
        "core" => Some(include_str!("../embedded/core.toml")),
        "core-extended" => Some(include_str!("../embedded/core-extended.toml")),
        "locale-de" => Some(include_str!("../embedded/locale-de.toml")),
        "locale-en" => Some(include_str!("../embedded/locale-en.toml")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::embedded;
    use gaze::{RawMatch, Rulepack, RulepackSource};

    #[test]
    fn embedded_core_rulepack_parses_and_contains_email_regex() {
        let core = embedded("core").expect("core rulepack");
        let rulepack = Rulepack::load(RulepackSource::Embedded(core)).expect("valid core");

        assert_eq!(rulepack.recognizers.len(), 6);
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
            .all(|recognizer| matches!(recognizer.matcher, RawMatch::AnchoredMatch { .. })));
    }

    #[test]
    fn embedded_core_extended_rulepack_parses_and_contains_phase2_recognizers() {
        let core_extended = embedded("core-extended").expect("core-extended rulepack");
        let rulepack =
            Rulepack::load(RulepackSource::Embedded(core_extended)).expect("valid core-extended");

        assert_eq!(rulepack.recognizers.len(), 10);
        assert_eq!(rulepack.recognizers[0].id, "phone.structural");
        assert_eq!(rulepack.recognizers[1].id, "phone.national.de");
        assert_eq!(rulepack.recognizers[2].id, "phone.national.us");
        assert_eq!(rulepack.recognizers[3].id, "iban.structural");
        assert_eq!(rulepack.recognizers[4].id, "card.structural");
        assert_eq!(rulepack.recognizers[5].id, "ip.v4");
        assert_eq!(rulepack.recognizers[6].id, "ip.v6");
        assert_eq!(rulepack.recognizers[7].id, "eth.address");
        assert_eq!(rulepack.recognizers[8].id, "postal.de");
        assert_eq!(rulepack.recognizers[9].id, "postal.us");
        assert!(rulepack
            .recognizers
            .iter()
            .all(|recognizer| matches!(recognizer.matcher, RawMatch::Regex { .. })));
    }
}
