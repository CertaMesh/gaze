mod anchored_match;
mod dictionary;
mod error;
mod ner;
mod regex;

pub use anchored_match::{
    is_person_name_candidate, AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape,
};
pub use dictionary::DictionaryRecognizer;
pub use error::{RecognizerError, Result};
pub use ner::{
    LabelMap, NerBackendKind, NerDetector, NerLoadError, NerOptions, NerRecognizer,
    VerifiedArtifacts,
};
pub use regex::{NormalizerKind, RegexDetector, ValidatorKind};

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

        assert_eq!(rulepack.recognizers.len(), 3);
        assert_eq!(rulepack.recognizers[0].id, "email.global");
        assert_eq!(rulepack.recognizers[1].id, "email.header.name");
        assert_eq!(rulepack.recognizers[2].id, "email.header.name.paren");
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
    }

    #[test]
    fn embedded_core_extended_rulepack_parses_and_contains_phase2_recognizers() {
        let core_extended = embedded("core-extended").expect("core-extended rulepack");
        let rulepack =
            Rulepack::load(RulepackSource::Embedded(core_extended)).expect("valid core-extended");

        assert_eq!(rulepack.recognizers.len(), 9);
        assert_eq!(rulepack.recognizers[0].id, "phone.structural");
        assert_eq!(rulepack.recognizers[1].id, "phone.national.de");
        assert_eq!(rulepack.recognizers[2].id, "phone.national.us");
        assert_eq!(rulepack.recognizers[3].id, "iban.structural");
        assert_eq!(rulepack.recognizers[4].id, "card.structural");
        assert_eq!(rulepack.recognizers[5].id, "ip.v4");
        assert_eq!(rulepack.recognizers[6].id, "ip.v6");
        assert_eq!(rulepack.recognizers[7].id, "postal.de");
        assert_eq!(rulepack.recognizers[8].id, "postal.us");
        assert!(rulepack
            .recognizers
            .iter()
            .all(|recognizer| matches!(recognizer.matcher, RawMatch::Regex { .. })));
    }
}
