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
// Pinned model bundle verification, shared by the primary NER bundle and the Nym safety net.
mod bundle;
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
    verify_davlan_ner_bundle, LabelMap, NerBackendKind, NerDetector, NerLoadError, NerOptions,
    NerRecognizer, VerifiedArtifacts, DAVLAN_NER_BUNDLE_SHA256, DAVLAN_NER_HF_COMMIT,
    DAVLAN_NER_HF_REPO, DAVLAN_NER_LABELS_JSON, DAVLAN_NER_MODEL_DIR_NAME, DAVLAN_NER_SHA256SUMS,
    DAVLAN_NER_UPSTREAM_FILES, REQUIRED_DAVLAN_NER_ARTIFACTS,
};
pub use regex::{NormalizerKind, RegexDetector};

// drift-ack: core snapshot version0.5.3 matches the field rulepack; all detection fields are unchanged.
// drift-ack: core 0.6.0 moves security_token.anchored and password.field into the opt-in
// `secrets` bundle and drops username.field; the new secrets snapshot pins the moved rules.
const EMBEDDED_RULEPACKS: &[(&str, &str)] = &[
    ("core", include_str!("../embedded/core.toml")),
    ("locale-de", include_str!("../embedded/locale-de.toml")),
    ("locale-en", include_str!("../embedded/locale-en.toml")),
    ("locale-br", include_str!("../embedded/locale-br.toml")),
    ("locale-fr", include_str!("../embedded/locale-fr.toml")),
    ("locale-in", include_str!("../embedded/locale-in.toml")),
    ("locale-nl", include_str!("../embedded/locale-nl.toml")),
    ("locale-uk", include_str!("../embedded/locale-uk.toml")),
    // Credentials are opt-in, never part of setup's default activation.
    ("secrets", include_str!("../embedded/secrets.toml")),
];

/// Canonical embedded names and contents. `core-extended` is an alias, not a second pack.
pub fn embedded_rulepacks() -> impl Iterator<Item = (&'static str, &'static str)> {
    EMBEDDED_RULEPACKS.iter().copied()
}

pub fn embedded(name: &str) -> Option<&'static str> {
    let canonical = if name == "core-extended" {
        "core"
    } else {
        name
    };
    embedded_rulepacks().find_map(|(id, content)| (id == canonical).then_some(content))
}

#[cfg(test)]
mod tests {
    use super::embedded;
    use gaze::{RawMatch, Rulepack, RulepackSource};

    #[test]
    fn embedded_core_rulepack_parses_and_contains_unified_recognizers() {
        let core = embedded("core").expect("core rulepack");
        let rulepack = Rulepack::load(RulepackSource::Embedded(core)).expect("valid core");

        assert_eq!(rulepack.recognizers.len(), 40);
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

        assert_eq!(rulepack.recognizers.len(), 40);
        assert!(rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.id == "phone.structural"));
        assert!(rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.id == "phone.e164.spaced"));
        assert!(rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.id == "vat.de"));
        assert!(rulepack
            .recognizers
            .iter()
            .any(|recognizer| recognizer.id == "vat.es"));
    }

    #[test]
    fn embedded_secrets_rulepack_carries_only_the_credential_recognizers() {
        let secrets = embedded("secrets").expect("secrets rulepack");
        let rulepack = Rulepack::parse_bundled(secrets).expect("valid secrets");

        let ids = rulepack
            .recognizers
            .iter()
            .map(|recognizer| recognizer.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["security_token.anchored", "password.field"]);
        let core = Rulepack::load(RulepackSource::Embedded(embedded("core").expect("core")))
            .expect("valid core");
        assert!(core.recognizers.iter().all(|recognizer| !matches!(
            recognizer.id.as_str(),
            "security_token.anchored" | "password.field" | "username.field"
        )));
    }

    #[test]
    fn embedded_core_declares_locale_basis_for_every_recognizer() {
        let core = embedded("core").expect("core rulepack");

        Rulepack::parse_bundled(core).expect("bundled recognizers declare locale_basis");
    }
}
