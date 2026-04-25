mod dictionary;
mod ner;
mod regex;

pub use dictionary::DictionaryRecognizer;
pub use ner::{
    LabelMap, NerBackendKind, NerDetector, NerLoadError, NerOptions, NerRecognizer,
    VerifiedArtifacts,
};
pub use regex::{NormalizerKind, RegexDetector, ValidatorKind};

pub fn embedded(name: &str) -> Option<&'static str> {
    match name {
        "core" => Some(include_str!("../embedded/core.toml")),
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

        assert_eq!(rulepack.recognizers.len(), 2);
        assert_eq!(rulepack.recognizers[0].id, "email.global");
        assert_eq!(rulepack.recognizers[1].id, "email.header.name");
        assert!(matches!(
            rulepack.recognizers[0].matcher,
            RawMatch::Regex { .. }
        ));
        assert!(matches!(
            rulepack.recognizers[1].matcher,
            RawMatch::Regex { .. }
        ));
    }
}
