use super::*;
use crate::{detector_wiring::derive_source_short_label, template::lower_pattern_template};
use gaze::{
    Action, CleanDocument, ConflictTier, DetectorKind, LocaleTag, NerPolicy, PiiClass, PolicyError,
    RawDocument, RedactionEntry, RedactionLogError, RedactionLogger, RulepackError, Scope, Session,
    SessionPolicy,
};
use gaze_recognizers::{
    AnchoredBoundary, AnchoredMatchRecognizer, CuePosition, NameShape, RecognizerError,
    RegexDetector,
};
use std::sync::{Arc, Mutex};

fn policy() -> gaze::Policy {
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Name,
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.locale = Some(vec![LocaleTag::DeDe]);
    policy
}

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn empty_policy() -> gaze::Policy {
    gaze::Policy::default()
}

#[cfg(feature = "safety-net-nym")]
#[test]
fn preloaded_nym_attachment_changes_the_built_pipeline() {
    struct StubNet;
    impl gaze::SafetyNet for StubNet {
        fn id(&self) -> &str {
            "stub-nym"
        }

        fn supported_locales(&self) -> &[LocaleTag] {
            &[]
        }

        fn check(
            &self,
            _clean_text: &str,
            _context: gaze::SafetyNetContext<'_>,
        ) -> Result<Vec<gaze::LeakSuspect>, gaze::SafetyNetError> {
            Ok(Vec::new())
        }
    }

    let pipeline = CorePipelineConfig::new()
        .build()
        .unwrap()
        .pipeline()
        .clone();
    let before = pipeline.safety_net_count();
    let pipeline = attach_preloaded_safety_net(pipeline, StubNet).unwrap();
    assert_eq!(pipeline.safety_net_count(), before + 1);
}

#[test]
fn safety_net_count_requires_exactly_one_added() {
    assert!(require_net_added(2, 3, BuildError::NymNotAttached).is_ok());
    assert!(matches!(
        require_net_added(2, 2, BuildError::NymNotAttached),
        Err(BuildError::NymNotAttached)
    ));
    assert!(matches!(
        require_net_added(2, 1, BuildError::NymNotAttached),
        Err(BuildError::NymNotAttached)
    ));
}

#[test]
fn safety_net_attachment_rejects_a_missing_increment() {
    let pipeline = CorePipelineConfig::new()
        .build()
        .unwrap()
        .pipeline()
        .clone();
    let result =
        attach_safety_net_checked(pipeline, Ok::<_, BuildError>, BuildError::NymNotAttached);
    assert!(matches!(result, Err(BuildError::NymNotAttached)));
}

fn embedded_rulepack(name: &str) -> Rulepack {
    Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded(name).expect("embedded rulepack"),
    ))
    .expect("rulepack")
}

#[test]
fn build_pipeline_empty_inputs_returns_no_recognizers() {
    let policy = empty_policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(&policy, &empty_context(), &[], &active_locales, None) {
        Ok(_) => panic!("empty inputs must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(err, BuildError::NoRecognizers));
}

#[test]
fn policy_nym_missing_model_fails_closed_in_rust_assembly() {
    let mut policy = policy();
    policy.safety_net.backend = gaze::SafetyNetPolicyBackend::Nym;
    let rulepack = embedded_rulepack("core");
    let locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(&policy, &empty_context(), &[rulepack], &locales, None) {
        Ok(_) => panic!("requested Nym must fail without a model"),
        Err(err) => err,
    };
    #[cfg(feature = "safety-net-nym")]
    assert!(matches!(err, BuildError::NymModelDirMissing));
    #[cfg(not(feature = "safety-net-nym"))]
    assert!(matches!(err, BuildError::NymFeatureDisabled));
}

#[test]
fn build_pipeline_all_disabled_rulepack_returns_no_recognizers() {
    let policy = empty_policy();
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "disabled-only"
rulepack_version = "0.6.0"
default_locales = ["global"]

[[recognizers]]
id = "disabled.email"
class = "Email"
enabled = false

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#,
    )
    .expect("rulepack");
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("all-disabled rulepack must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(err, BuildError::NoRecognizers));
}

#[test]
fn build_pipeline_locale_filtered_rulepack_returns_no_recognizers() {
    let policy = empty_policy();
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "filtered"
rulepack_version = "0.6.0"
default_locales = ["de-DE"]

[[recognizers]]
id = "filtered.email"
class = "Email"
enabled = true
locales = ["de-DE"]

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#,
    )
    .expect("rulepack");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("locale-filtered rulepack must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(err, BuildError::NoRecognizers));
}

#[test]
fn format_basis_rulepack_ignores_document_locale() {
    let policy = name_email_policy(vec![LocaleTag::EnUs]);
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "format-basis"
rulepack_version = "0.1.0"
default_locales = ["de-DE"]

[[recognizers]]
id = "format.email"
class = "Email"
enabled = true
locales = ["de-DE"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#,
    )
    .expect("rulepack");

    let text = clean_with_policy_and_rulepacks(&policy, &[rulepack], "Email alice@example.invalid");

    assert!(text.contains(":Email_"), "{text}");
}

// S10-F1 (audit 7201): the guard and registration must agree on ONE locale
// predicate — the detect-time `LocaleChain::intersects` (empty list ⇒ matches).
// A rulepack that omits both `default_locales` and per-recognizer `locales`
// yields an empty locale list; the runtime would run it everywhere, so assembly
// must register it instead of silently dropping it.
#[test]
fn empty_locale_rulepack_recognizer_registers_under_document_chain() {
    let policy = name_email_policy(vec![LocaleTag::DeDe]);
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "no-locales"
rulepack_version = "0.1.0"

[[recognizers]]
id = "unscoped.email"
class = "Email"
enabled = true

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#,
    )
    .expect("rulepack");
    assert!(
        rulepack.recognizers[0].locales.is_empty(),
        "precondition: loader keeps an empty locale list when neither default_locales nor locales is set"
    );
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    assert!(
        active_locales.intersects(&rulepack.recognizers[0].locales),
        "precondition: detect-time predicate treats an empty locale list as matching"
    );

    let text = clean_with_policy_and_rulepacks(&policy, &[rulepack], "Email alice@example.invalid");

    assert!(text.contains(":Email_"), "{text}");
}

// S10-F1 (audit 7201): an anchored_match recognizer whose optional builtin cue
// bucket is absent under the active locale chain is skipped at registration.
// The guard must see that skip; otherwise an anchored-only rulepack builds a
// zero-recognizer pipeline that preserves every byte (silent fail-open).
#[test]
fn anchored_only_rulepack_without_cue_bucket_returns_no_recognizers() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "anchored-only"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
    )
    .expect("rulepack");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("anchored-only rulepack without its cue bucket must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(err, BuildError::NoRecognizers), "{err:?}");
}

// Skipped optional-cue recognizers must not lower a live variant's precedence.
// Otherwise the Preserve variant wins and exposes the synthetic secret.
#[test]
fn skipped_anchored_match_does_not_leak_collision_into_family_policy() {
    let real_rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "real-collision"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "real.alpha"
class = "custom:alpha"
enabled = true
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''SECRET\d+'''

[recognizers.collision]
family = "doc"
variant = "alpha"
precedence = 10

[[recognizers]]
id = "real.beta"
class = "custom:beta"
enabled = true
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''SECRET\d+'''

[recognizers.collision]
family = "doc"
variant = "beta"
precedence = 20
"#,
    )
    .expect("real rulepack");

    let skip_rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "skipped-collision"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "skip.me"
class = "custom:beta"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"

[recognizers.collision]
family = "doc"
variant = "beta"
precedence = 5
"#,
    )
    .expect("skip rulepack");

    let mut policy = empty_policy();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("alpha").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::custom("beta").expect("valid custom class"),
            action: Action::Preserve,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];

    let text =
        clean_with_policy_and_rulepacks(&policy, &[real_rulepack, skip_rulepack], "SECRET123");

    assert!(
        !text.contains("SECRET123"),
        "skipped metadata must not expose PII: {text}"
    );
    assert!(
        text.contains(":Custom:alpha_"),
        "alpha (precedence 10) must win the overlap; got: {text}"
    );
    assert!(
        !text.contains(":Custom:beta_"),
        "the skipped recognizer's leaked collision must not flip arbitration to beta; got: {text}"
    );
}

// Built anchored recognizers still need collision metadata to override class priority.
#[test]
fn built_anchored_match_registers_collision_so_family_policy_arbitrates() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "anchored-collision"
rulepack_version = "0.1.0"
default_locales = ["global"]

[locale.forward_markers]
names = ["Forwarded message from"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"

[recognizers.collision]
family = "doc"
variant = "anchor"
precedence = 20

[[recognizers]]
id = "regex.name"
class = "custom:regex"
enabled = true
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''Alice\s+Example'''

[recognizers.scoring]
base = 0.9
priority = 0

[recognizers.collision]
family = "doc"
variant = "regex"
precedence = 10
"#,
    )
    .expect("rulepack");
    let mut policy = empty_policy();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Name,
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::custom("regex").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];

    let text = clean_with_policy_and_rulepacks(
        &policy,
        &[rulepack],
        "Forwarded message from Alice Example:",
    );

    assert!(
        regex::Regex::new(r"^Forwarded message from <[0-9a-f]{8}:Custom:regex_\d+>:$")
            .unwrap()
            .is_match(&text),
        "family policy must let the lower-precedence regex win; got: {text}"
    );
    assert!(
        !text.contains(":Name_"),
        "the higher class-priority anchored recognizer must lose to family policy; got: {text}"
    );
}

// Pins #414: a format-basis recognizer registers regardless of the document
// locale chain, so a format-only rulepack passes the guard under a chain that
// would filter the same recognizer on document basis (compare
// `build_pipeline_locale_filtered_rulepack_returns_no_recognizers`).
#[test]
fn format_basis_only_rulepack_passes_guard_under_non_matching_document_chain() {
    let policy = empty_policy();
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "format-only"
rulepack_version = "0.1.0"
default_locales = ["de-DE"]

[[recognizers]]
id = "format.email"
class = "Email"
enabled = true
locales = ["de-DE"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''alice@example\.invalid'''
"#,
    )
    .expect("rulepack");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )
    .expect("format-basis recognizer must register under a non-matching document chain");
}

// A configured-but-unloadable NER model surfaces as the NER load error; the
// guard must not mask it as `NoRecognizers` (nor pass on the mere presence of a
// `model_dir` — see `build_pipeline_ner_without_model_dir_returns_no_recognizers`).
#[test]
fn ner_load_failure_is_reported_not_masked_as_no_recognizers() {
    let mut policy = empty_policy();
    let mut ner = NerPolicy::default();
    ner.model_dir = Some(std::path::PathBuf::from(
        "/nonexistent/gaze-assembly-ner-model-dir",
    ));
    policy.ner = Some(ner);
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(&policy, &empty_context(), &[], &active_locales, None) {
        Ok(_) => panic!("unloadable NER model must not build"),
        Err(err) => err,
    };

    assert!(
        matches!(err, BuildError::Policy(PolicyError::NerLoad(_))),
        "{err:?}"
    );
}

#[test]
fn build_pipeline_ner_without_model_dir_returns_no_recognizers() {
    let mut policy = empty_policy();
    policy.ner = Some(NerPolicy::default());
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(&policy, &empty_context(), &[], &active_locales, None) {
        Ok(_) => panic!("threshold-only NER must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(err, BuildError::NoRecognizers));
}

#[test]
fn build_pipeline_context_only_still_succeeds() {
    let mut policy = empty_policy();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("song").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    let context = Context {
        dictionaries: std::collections::HashMap::from([(
            "song".to_string(),
            gaze::ContextDictionary {
                terms: vec!["context-song-123".to_string()],
                case_sensitive: true,
            },
        )]),
        class_map: std::collections::HashMap::from([(
            "song".to_string(),
            PiiClass::custom("song").expect("valid custom class"),
        )]),
        fields: serde_json::Map::new(),
    };
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(&policy, &context, &[], &active_locales, None)
        .expect("context-only assembly must remain supported");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let dictionaries = gaze::dictionary_bundle_from_context(&context);
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text("track context-song-123".to_string()),
            active_locales.as_slice(),
            &dictionaries,
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(text.contains(":Custom:song_"));
}

fn clean_with_rulepacks(rulepacks: &[Rulepack], locales: &[LocaleTag], input: &str) -> String {
    let mut policy = policy();
    policy.locale = Some(locales.to_vec());
    clean_with_policy_and_rulepacks(&policy, rulepacks, input)
}

fn clean_with_policy_and_rulepacks(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    input: &str,
) -> String {
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(policy, &empty_context(), rulepacks, &active_locales, None)
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(&session, RawDocument::Text(input.to_string()))
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    text
}

fn clean_text(clean: CleanDocument) -> String {
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    text
}

fn name_email_policy(locales: Vec<LocaleTag>) -> gaze::Policy {
    let mut policy = policy();
    policy.locale = Some(locales);
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::Name,
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::Email,
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy
}

#[test]
fn core_pipeline_config_tokenizes_synthetic_email() {
    let core = CorePipelineConfig::new().build().expect("core pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = clean_text(
        core.pseudonymize_text(&session, "Email alice@example.invalid")
            .expect("redact"),
    );

    assert!(text.contains(":Email_"), "{text}");
}

#[test]
fn core_pipeline_config_core_tokenizes_safe_default_phone() {
    let core = CorePipelineConfig::new()
        .with_locale(&[LocaleTag::EnUs])
        .build()
        .expect("core pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Phone +12025550100";
    let text = clean_text(core.pseudonymize_text(&session, input).expect("redact"));

    assert!(text.contains(":Custom:phone_"), "{text}");
}

#[test]
fn core_pipeline_config_core_extended_alias_tokenizes_synthetic_phone() {
    let core = CorePipelineConfig::new()
        .with_locale(&[LocaleTag::EnUs])
        .with_bundled_rulepack("core-extended")
        .build()
        .expect("extended pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = clean_text(
        core.pseudonymize_text(&session, "Phone +12025550100")
            .expect("redact"),
    );

    assert!(text.contains(":Custom:phone_"), "{text}");
}

// S10-F2 (audit 7201): the auto-activate locale set is derived from the loaded
// rulepacks, not spelled out. This adopter path rulepack declares a
// document-basis `locale_gated` recognizer for `es-ES` under `global` defaults;
// `core-extended` (auto-activate) must put `es-ES` on the chain so it activates.
const ES_LOCALE_GATED_RULEPACK: &str = r#"
schema_version = "0.1.0"
rulepack_id = "es-locale-gated"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "es.test_id"
class = "custom:es_test_id"
enabled = true
safety_tier = "locale_gated"
locales = ["es-ES"]

[recognizers.match]
kind = "regex"
pattern = '''ES-TEST-[0-9]{6}'''
"#;

fn write_temp_rulepack(name: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "gaze-assembly-{name}-{}-{}.toml",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&path, contents).expect("write temp rulepack");
    path
}

// The derived set for the bundled `core` recognizers equals the literal list
// the CLI/daemon/library used to carry (`en-US, de-DE, de-AT, de-CH`) — this is
// the derived-set == pack-union assertion; it also gates the bundle: a new
// bundled `locale_gated` recognizer changes this set and must update the pin
// plus the "Shipped default activation" reference table.
#[test]
fn locale_gated_activation_locales_for_core_bundle_match_compat_list() {
    let core = embedded_rulepack("core");

    assert_eq!(
        locale_gated_activation_locales(&[core]),
        vec![
            LocaleTag::EnUs,
            LocaleTag::DeDe,
            LocaleTag::DeAt,
            LocaleTag::DeCh,
        ]
    );
}

// Set rules: enabled + `locale_gated` + document basis contribute; `global`,
// format-basis, disabled, and safe_default recognizers do not. Ordering:
// compatibility tags first in their shipped order, then canonical string order.
#[test]
fn locale_gated_activation_locales_derive_set_and_order_from_loaded_packs() {
    let pack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "mixed-locale-gated"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "gated.es"
class = "custom:a"
enabled = true
safety_tier = "locale_gated"
locales = ["es-ES", "global"]

[recognizers.match]
kind = "regex"
pattern = '''ES-A-[0-9]{6}'''

[[recognizers]]
id = "gated.gb_and_de"
class = "custom:b"
enabled = true
safety_tier = "locale_gated"
locales = ["en-GB", "de-DE"]

[recognizers.match]
kind = "regex"
pattern = '''GB-B-[0-9]{6}'''

[[recognizers]]
id = "gated.format_basis"
class = "custom:c"
enabled = true
safety_tier = "locale_gated"
locales = ["fr-FR"]
locale_basis = "format"

[recognizers.match]
kind = "regex"
pattern = '''FR-C-[0-9]{6}'''

[[recognizers]]
id = "gated.disabled"
class = "custom:d"
enabled = false
safety_tier = "locale_gated"
locales = ["nl-NL"]

[recognizers.match]
kind = "regex"
pattern = '''NL-D-[0-9]{6}'''

[[recognizers]]
id = "safe.default"
class = "custom:e"
enabled = true
safety_tier = "safe_default"
locales = ["pt-BR"]

[recognizers.match]
kind = "regex"
pattern = '''BR-E-[0-9]{6}'''
"#,
    )
    .expect("rulepack");
    let core = embedded_rulepack("core");

    let es = LocaleTag::parse("es-ES").expect("es-ES parses");
    assert_eq!(
        locale_gated_activation_locales(std::slice::from_ref(&pack)),
        vec![LocaleTag::DeDe, LocaleTag::EnGb, es.clone()],
        "compat tag de-DE first, then en-GB < es-ES by canonical string"
    );
    assert_eq!(
        locale_gated_activation_locales(&[core, pack]),
        vec![
            LocaleTag::EnUs,
            LocaleTag::DeDe,
            LocaleTag::DeAt,
            LocaleTag::DeCh,
            LocaleTag::EnGb,
            es,
        ],
        "shipped compat order is stable regardless of pack load order"
    );
}

// Pins the shipped compatibility chain: for the bundled `core` recognizers the
// derived activation set is exactly the v0.6 alias order, so S10-F2 changes no
// shipped behaviour.
#[test]
fn core_extended_alias_locale_chain_is_compat_order() {
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("core-extended")
        .build()
        .expect("extended pipeline");

    assert_eq!(
        core.locale_chain().as_slice(),
        &[
            LocaleTag::Global,
            LocaleTag::EnUs,
            LocaleTag::DeDe,
            LocaleTag::DeAt,
            LocaleTag::DeCh,
        ]
    );
}

#[test]
fn core_extended_alias_auto_activates_locale_gated_recognizer_from_path_rulepack() {
    let path = write_temp_rulepack("es-locale-gated", ES_LOCALE_GATED_RULEPACK);
    let built = CorePipelineConfig::new()
        .with_bundled_rulepack("core-extended")
        .with_rulepack_path(path.clone())
        .build();
    let _ = std::fs::remove_file(&path);
    let core = built.expect("extended pipeline with path rulepack");

    assert!(
        core.locale_chain()
            .as_slice()
            .iter()
            .any(|locale| locale.as_str() == "es-ES"),
        "auto-activate chain must carry the locale-gated recognizer's locale: {:?}",
        core.locale_chain()
    );
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = clean_text(
        core.pseudonymize_text(&session, "id ES-TEST-123456")
            .expect("redact"),
    );
    assert!(text.contains(":Custom:es_test_id_"), "{text}");
}

#[test]
fn core_pipeline_config_core_extended_alias_tokenizes_synthetic_iban() {
    let core = CorePipelineConfig::new()
        .with_locale(&[LocaleTag::DeDe])
        .with_bundled_rulepack("core-extended")
        .with_bundled_rulepack("locale-de")
        .build()
        .expect("extended pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let text = clean_text(
        core.pseudonymize_text(&session, "IBAN DE89 3704 0044 0532 0130 00")
            .expect("redact"),
    );

    assert!(text.contains(":Custom:iban_"), "{text}");
}

#[test]
fn core_pipeline_restore_round_trip() {
    let core = CorePipelineConfig::new().build().expect("core pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let original = "alice@example.invalid";
    let text = clean_text(
        core.pseudonymize_text(&session, format!("Email {original}"))
            .expect("redact"),
    );
    let token = regex::Regex::new(r"<[^>]+:Email_\d+>")
        .expect("token regex")
        .find(&text)
        .map(|matched| matched.as_str())
        .expect("email token");

    assert_eq!(session.restore_strict(token).expect("restore"), original);
}

#[test]
fn unknown_bundled_rulepack_fails_closed() {
    let err = match CorePipelineConfig::new()
        .with_bundled_rulepack("missing-core-addon")
        .build()
    {
        Ok(_) => panic!("unknown bundled rulepack must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Policy(PolicyError::BundledRulepackUnknown { value })
            if value == "missing-core-addon"
    ));
}

#[test]
fn recognizer_error_passthrough_preserves_non_matcher_variant() {
    let err = BuildError::from(RecognizerError::UnsupportedValidator {
        kind: "future_model_loader".to_string(),
    });

    match err {
        BuildError::Recognizer(RecognizerError::UnsupportedValidator { kind }) => {
            assert_eq!(kind, "future_model_loader");
        }
        other => panic!("expected recognizer passthrough, got {other:?}"),
    }
}

#[derive(Clone, Default)]
struct MemoryLogger {
    entries: Arc<Mutex<Vec<RedactionEntry>>>,
}

impl MemoryLogger {
    fn entries(&self) -> Vec<RedactionEntry> {
        self.entries.lock().expect("entries lock").clone()
    }
}

impl RedactionLogger for MemoryLogger {
    fn log(&self, entry: &RedactionEntry) -> Result<(), RedactionLogError> {
        self.entries
            .lock()
            .expect("entries lock")
            .push(entry.clone());
        Ok(())
    }
}

fn policy_with_registered_dictionary(rules: Vec<RuleSpec>) -> gaze::Policy {
    let mut detector = gaze::DetectorSpec::default();
    detector.kind = DetectorKind::Dictionary;
    detector.name = "alpha".to_string();
    detector.class = PiiClass::custom("foo").expect("valid custom class");
    detector.dictionary_name = Some("dict_alpha".to_string());
    detector.case_sensitive = true;

    let mut policy = gaze::Policy::default();
    policy.detectors = vec![detector];
    policy.rules = rules;
    policy.locale = Some(vec![LocaleTag::Global]);
    policy
}

fn context_with_alpha_override() -> Context {
    Context {
        dictionaries: std::collections::HashMap::from([(
            "dict_alpha".to_string(),
            gaze::ContextDictionary {
                terms: vec!["context-song-123".to_string()],
                case_sensitive: true,
            },
        )]),
        class_map: std::collections::HashMap::from([(
            "dict_alpha".to_string(),
            PiiClass::custom("bar").expect("valid custom class"),
        )]),
        fields: serde_json::Map::new(),
    }
}

#[test]
fn t20_context_class_map_overrides_policy_dict_class() {
    let policy = policy_with_registered_dictionary(vec![
        RuleSpec::Class {
            class: PiiClass::custom("bar").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);
    let context = context_with_alpha_override();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(&policy, &context, &[], &active_locales, None).expect("pipeline");
    let dictionaries = gaze::dictionary_bundle_from_context(&context);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .pseudonymize_with_detect_context(
            &session,
            RawDocument::Text("track context-song-123".to_string()),
            active_locales.as_slice(),
            &dictionaries,
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(regex::Regex::new(r"^track <[0-9a-f]{8}:Custom:bar_\d+>$")
        .unwrap()
        .is_match(&text));
}

#[test]
fn t20a_class_map_override_fails_closed_when_action_rule_uncovered() {
    let policy = policy_with_registered_dictionary(vec![
        RuleSpec::Class {
            class: PiiClass::custom("foo").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);
    let context = context_with_alpha_override();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(&policy, &context, &[], &active_locales, None) {
        Ok(_) => panic!("uncovered class_map override must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::ClassMapOverrideClash {
            dict,
            old_class,
            new_class,
            ..
        }) if dict == "dict_alpha"
            && old_class == PiiClass::custom("foo").expect("valid custom class")
            && new_class == PiiClass::custom("bar").expect("valid custom class")
    ));
}

#[test]
fn t20b_rulepack_context_dict_override_fails_closed_when_uncovered() {
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("foo").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.locale = Some(vec![LocaleTag::Global]);
    let context = context_with_alpha_override();
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "tenant-rulepack"
rulepack_version = "0.4.6"
default_locales = ["global"]

[[recognizers]]
id = "tenant.alpha"
class = "custom:foo"
enabled = true

[recognizers.match]
kind = "dictionary"
terms_from_context = "dict_alpha"
case_sensitive = true
"#,
    )
    .expect("rulepack");
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(&policy, &context, &[rulepack], &active_locales, None) {
        Ok(_) => panic!("rulepack context dictionary override must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::ClassMapOverrideClash {
            dict,
            old_class,
            new_class,
            ..
        }) if dict == "dict_alpha"
            && old_class == PiiClass::custom("foo").expect("valid custom class")
            && new_class == PiiClass::custom("bar").expect("valid custom class")
    ));
}

#[test]
fn pattern_template_lowers_correctly_under_locale_chain_de() {
    let core = Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("core").expect("core"),
    ))
    .expect("core");
    let de = Rulepack::load(gaze::RulepackSource::Embedded(
        gaze_recognizers::embedded("locale-de").expect("locale-de"),
    ))
    .expect("de");
    let policy = policy();
    let active_locales =
        LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), Some(&[LocaleTag::DeDe]));
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[core, de],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("Von: Dana Weber <user@example.invalid>".into()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(
        regex::Regex::new(r"^Von: <[0-9a-f]{8}:Name_\d+> <[a-z0-9._%+\-]+@example\.invalid>$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn merged_vocab_for_de_de_loads_only_locale_de_buckets() {
    let core = embedded_rulepack("core");
    let de = embedded_rulepack("locale-de");
    let en = embedded_rulepack("locale-en");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::DeDe]), None);
    let vocab = merged_locale_vocab(&[core, de, en], &active_locales);

    let forward_markers = vocab
        .get("forward_markers")
        .expect("locale-de forward markers");
    assert_eq!(
        forward_markers,
        &vec![
            "Weitergeleitete Nachricht von".to_string(),
            "Forwarded message from".to_string(),
            "Begin forwarded message from".to_string(),
            "----- Forwarded message".to_string(),
        ]
    );
    assert!(
        !forward_markers.contains(&"Begin forwarded message".to_string()),
        "de-DE chain must not merge locale-en-only buckets; English safety cues are duplicated into locale-de deliberately"
    );
    assert_eq!(
        vocab
            .get("agent_recipient_cues")
            .expect("agent recipient cues"),
        &vec![
            "antwortest".to_string(),
            "antworte".to_string(),
            "schreibst".to_string(),
            "reply to".to_string(),
            "respond to".to_string(),
            "draft for".to_string(),
            "draft to".to_string(),
        ]
    );
}

#[test]
fn merged_vocab_for_en_us_loads_only_locale_en_buckets() {
    let core = embedded_rulepack("core");
    let de = embedded_rulepack("locale-de");
    let en = embedded_rulepack("locale-en");
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);
    let vocab = merged_locale_vocab(&[core, de, en], &active_locales);

    assert_eq!(
        vocab
            .get("forward_markers")
            .expect("locale-en forward markers"),
        &vec![
            "Forwarded message from".to_string(),
            "Begin forwarded message".to_string(),
            "----- Forwarded message".to_string(),
        ]
    );
    assert!(
        !vocab
            .get("agent_recipient_cues")
            .expect("agent recipient cues")
            .contains(&"antwortest".to_string()),
        "en-US chain must not merge locale-de cues"
    );
}

#[test]
fn adopter_default_pipeline_repro_locks_in_v0_6_closure() {
    let policy = name_email_policy(vec![LocaleTag::DeDe]);
    let fixtures = [
        (
            "Von: Alice Example <alice@example.invalid>",
            r"^Von: <[0-9a-f]{8}:Name_\d+> <<[0-9a-f]{8}:Email_\d+>>$",
        ),
        (
            "From: alice@example.invalid (Alice Example)",
            r"^From: <[0-9a-f]{8}:Email_\d+> \(<[0-9a-f]{8}:Name_\d+>\)$",
        ),
        (
            "Forwarded message from Alice Example:",
            r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$",
        ),
        (
            "Du antwortest als Artistfy-Support an Alice Example.",
            r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$",
        ),
    ];
    for rulepacks in [
        vec![embedded_rulepack("core"), embedded_rulepack("locale-de")],
        vec![
            embedded_rulepack("core"),
            embedded_rulepack("locale-de"),
            embedded_rulepack("locale-en"),
        ],
    ] {
        for (input, expected) in fixtures {
            let text = clean_with_policy_and_rulepacks(&policy, &rulepacks, input);
            assert!(
                regex::Regex::new(expected).unwrap().is_match(&text),
                "{input} produced {text}"
            );
        }
    }
}

#[test]
fn paren_display_name_from_header_tokenizes_single_address() {
    // Matrix row 5 / R2-Patch-9: paren display name beside a header email.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "From: alice@example.invalid (Alice Example)",
    );

    assert!(
        regex::Regex::new(r"^From: alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn paren_display_name_von_header_tokenizes_under_de_locale_chain() {
    // Matrix row 5 / R2-Patch-9: DE locale bucket lowers `Von`.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-de")],
        &[LocaleTag::DeDe],
        "Von: alice@example.invalid (Alice Example)",
    );

    assert!(
        regex::Regex::new(r"^Von: alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn paren_display_name_from_header_tokenizes_multi_address_line() {
    // Matrix row 5 / R2-Patch-9: regex.rs uses captures_iter, so after the
    // first header-anchored address the comma arm walks the second paren form.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "From: bob@example.invalid (Bob B), alice@example.invalid (Alice A)",
    );

    assert!(
        regex::Regex::new(
            r"^From: bob@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\), alice@example\.invalid \(<[0-9a-f]{8}:Name_\d+>\)$"
        )
        .unwrap()
        .is_match(&text)
    );
    assert_eq!(text.matches(":Name_").count(), 2);
}

#[test]
fn anchored_footer_catches_sender_name_without_org_suffix() {
    // Matrix row 3 / R2-Patch-8: max-greedy captures `Alice Example`;
    // single-component `Mailgun` and organization-shaped `Acme Corp` stay out.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "Sent by Alice Example via Mailgun on behalf of Acme Corp",
    );

    assert!(regex::Regex::new(
        r"^Sent by <[0-9a-f]{8}:Name_\d+> via Mailgun on behalf of Acme Corp$"
    )
    .unwrap()
    .is_match(&text));
    assert_eq!(text.matches(":Name_").count(), 1);
}

#[test]
fn anchored_agent_recipient_catches_german_prompt_preamble() {
    // Matrix row 4 / R2-Patch-11: cue is literal `antwortest`; the 48-char
    // window walks past `als Artistfy-Support an `, while the 2-component
    // minimum skips the product-role fragment and captures `Alice Example`.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-de")],
        &[LocaleTag::DeDe],
        "Du antwortest als Artistfy-Support an Alice Example.",
    );

    assert!(
        regex::Regex::new(r"^Du antwortest als Artistfy-Support an <[0-9a-f]{8}:Name_\d+>\.$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn anchored_forward_marker_catches_single_path() {
    // Matrix row 6 / R2-Patch-13: fixture is covered by anchored_match only.
    let text = clean_with_rulepacks(
        &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
        &[LocaleTag::EnUs],
        "Forwarded message from Alice Example:",
    );

    assert!(
        regex::Regex::new(r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$")
            .unwrap()
            .is_match(&text)
    );
    assert_eq!(text.matches(":Name_").count(), 1);
}

#[test]
fn deferred_and_not_caught_rows_do_not_emit_name_tokens() {
    for (locale, input) in [
        (LocaleTag::EnUs, "Re: ticket update from Alice Example"),
        (
            LocaleTag::EnUs,
            "Cc: bob@example.invalid, alice@example.invalid",
        ),
        (LocaleTag::EnUs, "Schedule a call with Alice next Tuesday"),
    ] {
        let text = clean_with_rulepacks(
            &[embedded_rulepack("core"), embedded_rulepack("locale-en")],
            &[locale],
            input,
        );
        assert!(!text.contains(":Name_"), "{input} produced {text}");
    }
}

#[test]
fn same_span_structural_name_overlap_logs_loser() {
    let logger = MemoryLogger::default();
    let pipeline = gaze::Pipeline::builder()
        .recognizer(
            RegexDetector::with_rulepack_fields(
                r"reply to alice@example\.invalid \(([^)]+)\)",
                PiiClass::Name,
                "email.header.name.paren",
                vec![LocaleTag::Global],
                0.85,
                100,
                "name.counter",
                Some(vec![1]),
                Vec::new(),
                None,
                None,
            )
            .expect("regex recognizer"),
        )
        .recognizer(AnchoredMatchRecognizer::new(
            "name.agent_recipient".to_string(),
            vec!["reply to alice@example.invalid".to_string()],
            AnchoredBoundary::Punctuation,
            48,
            NameShape::PersonName,
            CuePosition::Before,
            "agent_recipient".to_string(),
            2,
            0.88,
            110,
        ))
        .rule(gaze::ClassRule::new(PiiClass::Name, Action::Tokenize))
        .rule(gaze::DefaultRule::new(Action::Preserve))
        .redaction_logger(logger.clone())
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("reply to alice@example.invalid (Alice Example)".to_string()),
        )
        .expect("redact");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };

    assert!(text.contains(":Name_"));
    let entries = logger.entries();
    assert_eq!(entries.len(), 2, "{entries:?}");
    let loser = entries
        .iter()
        .find(|entry| entry.conflict_loser)
        .expect("loser");
    assert_eq!(loser.source, "email.header.name.paren");
    assert_eq!(loser.decided_by, ConflictTier::Merged);
}

#[test]
fn pattern_template_preserves_regex_quantifiers() {
    let locale_vocab =
        std::collections::HashMap::from([("email_headers".to_string(), vec!["From".to_string()])]);
    let pattern = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale_email_headers}): ([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3})$",
        &locale_vocab,
    )
    .expect("lowered pattern");

    assert!(pattern.contains(r"{0,3}"));
    let regex = regex::Regex::new(&pattern).expect("compiled regex");
    let captures = regex
        .captures("From: Alice Example")
        .expect("email header captures");
    assert_eq!(captures.get(1).map(|m| m.as_str()), Some("Alice Example"));
}

#[test]
fn locale_email_headers_placeholder_is_non_capturing() {
    let locale_vocab =
        std::collections::HashMap::from([("email_headers".to_string(), vec!["Von".to_string()])]);
    let pattern = lower_pattern_template(
        "email.header.name",
        r#"^(?:{locale_email_headers}):\s*(?:"([^"]+)"|([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,3}))\s+<[^>]+>"#,
        &locale_vocab,
    )
    .expect("lowered pattern");
    let regex = regex::Regex::new(&pattern).expect("compiled regex");

    let quoted = regex
        .captures(r#"Von: "Doe, Jane" <jane@example.invalid>"#)
        .expect("quoted capture");
    assert_eq!(quoted.get(1).map(|m| m.as_str()), Some("Doe, Jane"));
    assert!(quoted.get(2).is_none());

    let bare = regex
        .captures("Von: Alice Example <alice@example.invalid>")
        .expect("bare capture");
    assert!(bare.get(1).is_none());
    assert_eq!(bare.get(2).map(|m| m.as_str()), Some("Alice Example"));
}

#[test]
fn locale_email_headers_legacy_alias_matches_bucket_syntax() {
    let locale_vocab = std::collections::HashMap::from([(
        "email_headers".to_string(),
        vec!["From".to_string(), "Reply-To".to_string()],
    )]);

    let legacy = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale_email_headers}):\s+(.+)$",
        &locale_vocab,
    )
    .expect("legacy placeholder");
    let bucket = lower_pattern_template(
        "email.header.name",
        r"^(?:{locale.email_headers}):\s+(.+)$",
        &locale_vocab,
    )
    .expect("bucket placeholder");

    assert_eq!(legacy, bucket);
}

#[test]
fn locale_bucket_placeholder_lowers_neutral_bucket() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "neutral-template"
rulepack_version = "0.4.2"
default_locales = ["global"]

[locale.salutations]
names = ["Mx", "Dr"]

[[recognizers]]
id = "neutral.salutation.name"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''(?m)^(?:{locale.salutations}):\s+([A-Z][a-z]+)$'''
capture_groups = [1]
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(&session, RawDocument::Text("Mx: Schmidt".to_string()))
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(regex::Regex::new(r"^Mx: <[0-9a-f]{8}:Name_\d+>$")
        .unwrap()
        .is_match(&text));
}

#[test]
fn locale_bucket_placeholder_unknown_bucket_fails_closed() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-locale-bucket"
rulepack_version = "0.4.2"
default_locales = ["global"]

[[recognizers]]
id = "bad.locale.bucket"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''{locale.missing_bucket}: (.+)'''
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("unknown locale bucket must fail"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::Policy(PolicyError::UnknownLocaleBucket { name })
            if name == "missing_bucket"
    ));
}

#[test]
fn pattern_template_unknown_placeholder_fails_closed() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-template"
rulepack_version = "0.4.1"
default_locales = ["global"]

[[recognizers]]
id = "bad.template"
class = "Name"
enabled = true

[recognizers.match]
kind = "regex"
pattern_template = '''{unknown_placeholder}: (.+)'''
"#,
    )
    .expect("parse");

    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("unknown placeholder must fail"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        BuildError::Rulepack(RulepackError::UnknownPatternTemplatePlaceholder {
            placeholder,
            ..
        }) if placeholder == "unknown_placeholder"
    ));
}

#[test]
fn anchored_match_missing_bucket_fails_closed_with_recognizer_id() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "missing_bucket"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("missing anchored_match bucket must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::UnknownLocaleBucket {
            recognizer_id,
            bucket,
        } if recognizer_id == "name.forward_marker" && bucket == "missing_bucket"
    ));
}

#[test]
fn anchored_match_empty_bucket_fails_closed_with_recognizer_id() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "bad-anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[locale.empty_bucket]
names = []

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "empty_bucket"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);

    let err = match build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    ) {
        Ok(_) => panic!("empty anchored_match bucket must fail closed"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        BuildError::UnknownLocaleBucket {
            recognizer_id,
            bucket,
        } if recognizer_id == "name.forward_marker" && bucket == "empty_bucket"
    ));
}

#[test]
fn anchored_match_builds_with_constructor_owned_cues() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "anchored"
rulepack_version = "0.6.0"
default_locales = ["global"]

[locale.forward_markers]
names = ["Forwarded message from"]

[[recognizers]]
id = "name.forward_marker"
class = "Name"
enabled = true

[recognizers.match]
kind = "anchored_match"
cues_bucket = "forward_markers"
boundary = "punctuation"
right_window_chars = 64
name_shape = "person_name"
cue_position = "before"

[recognizers.scoring]
base = 0.95
priority = 60
"#,
    )
    .expect("parse");
    let policy = policy();
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(
        &policy,
        &empty_context(),
        &[rulepack],
        &active_locales,
        None,
    )
    .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = pipeline
        .redact(
            &session,
            RawDocument::Text("Forwarded message from Alice Example:".to_string()),
        )
        .expect("redact");

    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert!(
        regex::Regex::new(r"^Forwarded message from <[0-9a-f]{8}:Name_\d+>:$")
            .unwrap()
            .is_match(&text)
    );
}

#[test]
fn source_short_label_derivation_handles_builtin_and_adopter_shapes() {
    assert_eq!(
        derive_source_short_label("name.agent_recipient"),
        "agent_recipient"
    );
    assert_eq!(
        derive_source_short_label("name.forward_marker"),
        "forward_marker"
    );
    assert_eq!(derive_source_short_label("name.auto_footer"), "footer");
    assert_eq!(
        derive_source_short_label("name.x.team_handoff"),
        "team_handoff"
    );
}

// Regression coverage for issue #360: a policy that tokenizes `custom:iban` but
// leaves the default at `preserve` silently leaks IBAN spans emitted under the
// collision-family fallback class `custom:family:payment-card-or-iban`.

fn iban_preserve_default_policy() -> gaze::Policy {
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("iban").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy
}

#[test]
fn uncovered_family_classes_flags_a_named_member_with_an_unnamed_family() {
    let policy = iban_preserve_default_policy();
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        uncovered.contains(&"custom:family:payment-card-or-iban".to_string()),
        "a policy naming custom:iban but not the family must be told the family class: {uncovered:?}"
    );
}

#[test]
fn uncovered_family_classes_flags_a_named_member_even_under_a_tokenize_default() {
    // The notice is informational (the token class differs from the class the
    // adopter named), so a protective default does not silence it.
    let mut policy = iban_preserve_default_policy();
    policy.rules = vec![
        RuleSpec::Class {
            class: PiiClass::custom("iban").expect("valid custom class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Tokenize,
        },
    ];
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        uncovered.contains(&"custom:family:payment-card-or-iban".to_string()),
        "a named member is flagged whatever the default: {uncovered:?}"
    );
}

#[test]
fn uncovered_family_classes_empty_when_no_rule_names_the_family_or_a_member() {
    let mut policy = iban_preserve_default_policy();
    // A bare default shows no intent about the family; the notice stays quiet.
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        uncovered.is_empty(),
        "a bare default rule names nothing to notice: {uncovered:?}"
    );
}

#[test]
fn uncovered_family_classes_respects_explicit_family_rule() {
    let mut policy = iban_preserve_default_policy();
    // Adding the family rule the warning recommends must clear it from the list.
    policy.rules.insert(
        0,
        RuleSpec::Class {
            class: PiiClass::Custom("family:payment-card-or-iban".to_string()),
            action: Action::Tokenize,
        },
    );
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        !uncovered.contains(&"custom:family:payment-card-or-iban".to_string()),
        "an explicit family rule must satisfy coverage: {uncovered:?}"
    );
}

#[test]
fn uncovered_family_classes_flags_a_member_rule_shadowed_by_default() {
    // Review 3746 finding 6: a member rule pasted AFTER the default rule is
    // dead too, and the family token then falls to the default. The old notice
    // fired for this shape; the reworked one must keep doing so.
    let mut policy = iban_preserve_default_policy();
    policy.rules = vec![
        RuleSpec::Default {
            action: Action::Preserve,
        },
        RuleSpec::Class {
            class: PiiClass::custom("iban").expect("valid custom class"),
            action: Action::Tokenize,
        },
    ];
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        uncovered.contains(&"custom:family:payment-card-or-iban".to_string()),
        "a member rule shadowed by an earlier default rule shows intent and must be flagged: {uncovered:?}"
    );
}

#[test]
fn uncovered_family_classes_ignores_family_rule_shadowed_by_default() {
    let mut policy = iban_preserve_default_policy();
    // Pasting the family rule AFTER the default rule (the natural end-of-file
    // edit) leaves it unreachable at runtime: `rule::resolve` is
    // first-match-wins and `Default` matches unconditionally. The checker must
    // keep flagging the family so the adopter learns the rule is dead.
    policy.rules.push(RuleSpec::Class {
        class: PiiClass::Custom("family:payment-card-or-iban".to_string()),
        action: Action::Tokenize,
    });
    let rulepacks = [embedded_rulepack("core")];
    let active_locales = LocaleChain::merge_policy_and_cli(Some(&[LocaleTag::EnUs]), None);

    let uncovered = uncovered_collision_family_classes(&policy, &rulepacks, &active_locales);

    assert!(
        uncovered.contains(&"custom:family:payment-card-or-iban".to_string()),
        "a family rule shadowed by an earlier default rule is dead code and must stay flagged: {uncovered:?}"
    );
}

// ---------------------------------------------------------------------------
// todo 3746: a collision-family token derives its action from its member
// classes' rules (strictest wins, never laxer than the family's own default)
// unless the policy names the family class explicitly. Product path:
// `build_pipeline` on the bundled `core` + `locale-de` packs under de-DE.
// ---------------------------------------------------------------------------

/// No IBAN cue (`Überweisung` is not one), non-Luhn BBAN: `iban.structural`
/// fires alone, the mandatory anchor is missing, and the span is emitted as
/// `custom:family:payment-card-or-iban`.
const NO_CUE_IBAN: &str = "Überweisung DE89 3704 0044 0532 0130 00";
/// No cue, Luhn-valid BBAN: `card.structural` also fires, the family policy
/// settles the overlap for the IBAN (#619), so the span keeps `custom:iban`.
/// Already protected on main; pinned so the fix does not disturb it.
const NO_CUE_LUHN_BBAN_IBAN: &str = "Überweisung DE24 9635 8749 2586 6121 02";
/// No cue and a trailing number: a Luhn-valid card run crosses the IBAN's end
/// into the number, the collision falls to the family fallback, and on main
/// the WHOLE IBAN shipped raw under a member-only policy (todo 3746, comment 2115).
const TRAILING_NUMBER_IBANS: [&str; 3] = [
    "Bitte überweisen auf FO14 5878 0013 4155 73 1234",
    "Bitte überweisen auf GL07 3135 5673 6936 21 1234",
    // Its digit run holds no Luhn-valid card layout window. `SA77 … 7425` did
    // (`4281 2318 7317 7425`), which since todo 3843 makes it the Luhn-valid BBAN
    // case: the settled narrow IBAN token.
    "Bitte überweisen auf SA50 3476 4281 2318 7317 7426 1234",
];
const FAMILY_TOKEN_MARKER: &str = ":Custom:family:payment-card-or-iban_";
/// No cue, long IBAN: under de-DE `phone.national.de` wins a sub-run on rule
/// priority, the unanchored IBAN candidate loses, and its remaining bytes are
/// only covered by residual cells previewed on its standalone view, the family
/// class.
const PHONE_WIN_IBAN: &str = "Bitte überweisen auf AD56 7551 0585 4139 9502 9893 BIC";
/// No cue, a German phone shape straddling the IBAN's last group and a
/// trailing number (todo 3769): a PARTIAL overlap, which containment
/// precedence leaves to today's rungs, so the phone still wins its sub-run on
/// rule priority and the IBAN's remainder still reaches residual coverage on
/// its standalone view, the family class.
const PARTIAL_PHONE_IBAN: &str = "Bitte überweisen auf AD82 5402 2980 2202 2393 0418 1234";

fn payment_family_policy(rules: &[(&str, Action)], default: Action) -> gaze::Policy {
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.locale = Some(vec![LocaleTag::DeDe]);
    policy.rules = rules
        .iter()
        .map(|(class, action)| RuleSpec::Class {
            class: PiiClass::from_policy_name(class).expect("policy class"),
            action: *action,
        })
        .chain(std::iter::once(RuleSpec::Default { action: default }))
        .collect();
    policy
}

fn member_only_tokenize_policy() -> gaze::Policy {
    payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Tokenize),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Preserve,
    )
}

fn clean_payment(policy: &gaze::Policy, input: &str) -> String {
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    clean_with_policy_and_rulepacks(policy, &rulepacks, input)
}

/// Every space-separated group of the IBAN between `prefix` and `suffix` must
/// be gone from `clean`. Failure messages carry only the group index, never
/// the bytes.
fn assert_no_group_survives(clean: &str, input: &str, prefix: &str, suffix: &str) {
    let value = input
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .expect("fixture prefix and suffix");
    let clean = gaze::token_shape::pattern().replace_all(clean, "\0");
    for (index, group) in value.split(' ').enumerate() {
        assert!(
            !clean.contains(group),
            "group {index} of the value survived in the clean text"
        );
    }
}

#[test]
fn member_only_policy_tokenizes_a_no_cue_iban_family_token() {
    let clean = clean_payment(&member_only_tokenize_policy(), NO_CUE_IBAN);

    assert!(
        clean.contains(FAMILY_TOKEN_MARKER),
        "expected one family-level token, got: {clean}"
    );
    assert_no_group_survives(&clean, NO_CUE_IBAN, "Überweisung ", "");
}

#[test]
fn member_only_policy_tokenizes_trailing_number_family_tokens() {
    for (index, input) in TRAILING_NUMBER_IBANS.iter().enumerate() {
        let clean = clean_payment(&member_only_tokenize_policy(), input);
        assert!(
            clean.contains(FAMILY_TOKEN_MARKER),
            "fixture {index}: expected a family-level token, got: {clean}"
        );
        assert_no_group_survives(&clean, input, "Bitte überweisen auf ", " 1234");
    }
}

#[test]
fn no_cue_luhn_valid_bban_iban_keeps_its_settled_iban_token() {
    let clean = clean_payment(&member_only_tokenize_policy(), NO_CUE_LUHN_BBAN_IBAN);

    assert!(
        clean.contains(":Custom:iban_"),
        "the settled family verdict must keep the narrow class: {clean}"
    );
    assert_no_group_survives(&clean, NO_CUE_LUHN_BBAN_IBAN, "Überweisung ", "");
}

#[test]
fn explicit_family_preserve_rule_overrides_member_derivation() {
    let policy = payment_family_policy(
        &[
            ("custom:family:payment-card-or-iban", Action::Preserve),
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Tokenize),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Preserve,
    );

    assert_eq!(clean_payment(&policy, NO_CUE_IBAN), NO_CUE_IBAN);
}

#[test]
fn all_members_preserve_under_a_preserve_default_keeps_the_family_token_raw() {
    let policy = payment_family_policy(
        &[
            ("custom:iban", Action::Preserve),
            ("custom:credit_card", Action::Preserve),
            ("custom:phone", Action::Preserve),
        ],
        Action::Preserve,
    );

    assert_eq!(clean_payment(&policy, NO_CUE_IBAN), NO_CUE_IBAN);
}

#[test]
fn all_members_preserve_under_a_tokenize_default_still_tokenizes_the_family_token() {
    // Monotone: derivation never lands below the family's own default action.
    let policy = payment_family_policy(
        &[
            ("custom:iban", Action::Preserve),
            ("custom:credit_card", Action::Preserve),
            ("custom:phone", Action::Preserve),
        ],
        Action::Tokenize,
    );
    let clean = clean_payment(&policy, NO_CUE_IBAN);

    assert!(clean.contains(FAMILY_TOKEN_MARKER), "got: {clean}");
    assert_no_group_survives(&clean, NO_CUE_IBAN, "Überweisung ", "");
}

/// Two policy dictionary recognizers in one tenant family with equal
/// precedence. Dictionary detectors register as `dict/<name>` on both the
/// recognizer and the collision side; the regex analogue is the todo 3757
/// section below.
fn tenant_tie_policy(rules: Vec<RuleSpec>) -> (gaze::Policy, Context) {
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.locale = Some(vec![LocaleTag::Global]);
    policy.rules = rules;
    let mut dictionaries = std::collections::HashMap::new();
    for (name, class, variant) in [
        ("tenant.alpha", "custom:alpha_doc", "alpha"),
        ("tenant.beta", "custom:beta_doc", "beta"),
    ] {
        let mut detector = gaze::DetectorSpec::default();
        detector.kind = DetectorKind::Dictionary;
        detector.name = name.to_string();
        detector.dictionary_name = Some(format!("dict_{variant}"));
        detector.case_sensitive = true;
        detector.class = PiiClass::from_policy_name(class).expect("class");
        detector.collision = Some(gaze::CollisionMembership::new(
            "tenant-document",
            variant,
            10,
            None,
        ));
        policy.detectors.push(detector);
        dictionaries.insert(
            format!("dict_{variant}"),
            gaze::ContextDictionary {
                terms: vec!["CASE-0001".to_string()],
                case_sensitive: true,
            },
        );
    }
    let context = Context {
        dictionaries,
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    };
    (policy, context)
}

fn clean_tenant_tie(policy: &gaze::Policy, context: &Context, input: &str) -> String {
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(policy, context, &[], &active_locales, None).expect("pipeline");
    let dictionaries = gaze::dictionary_bundle_from_context(context);
    let session = Session::new(Scope::Ephemeral).expect("session");
    clean_text(
        pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(input.to_string()),
                active_locales.as_slice(),
                &dictionaries,
            )
            .expect("redact"),
    )
}

#[test]
fn precedence_tie_family_token_derives_its_action_from_member_rules() {
    let (policy, context) = tenant_tie_policy(vec![
        RuleSpec::Class {
            class: PiiClass::from_policy_name("custom:alpha_doc").expect("class"),
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::from_policy_name("custom:beta_doc").expect("class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);
    let clean = clean_tenant_tie(&policy, &context, "ticket CASE-0001 open");

    assert!(
        clean.contains(":Custom:family:tenant-document_"),
        "equal precedence must emit the family token: {clean}"
    );
    assert!(!clean.contains("CASE-0001"), "tie token leaked: {clean}");
}

#[test]
fn precedence_tie_family_token_honours_an_explicit_family_rule() {
    let (policy, context) = tenant_tie_policy(vec![
        RuleSpec::Class {
            class: PiiClass::family("tenant-document"),
            action: Action::Preserve,
        },
        RuleSpec::Class {
            class: PiiClass::from_policy_name("custom:alpha_doc").expect("class"),
            action: Action::Tokenize,
        },
        RuleSpec::Class {
            class: PiiClass::from_policy_name("custom:beta_doc").expect("class"),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ]);

    assert_eq!(
        clean_tenant_tie(&policy, &context, "ticket CASE-0001 open"),
        "ticket CASE-0001 open"
    );
}

/// Ruling 3746 #1 (c): the derived action and the member it came from are
/// visible on the family token's audit row.
#[test]
fn derived_family_action_is_recorded_on_the_audit_row() {
    let logger = MemoryLogger::default();
    let policy = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Redact),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Preserve,
    );
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline =
        build_pipeline_builder(&policy, &empty_context(), &rulepacks, &active_locales, None)
            .expect("builder")
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        pipeline
            .redact(&session, RawDocument::Text(NO_CUE_IBAN.to_string()))
            .expect("redact"),
    );

    // Redact outranks Tokenize: the card member's rule wins the ambiguous span.
    assert_eq!(clean, "Überweisung [REDACTED]");
    let row = logger
        .entries()
        .into_iter()
        .find(|entry| {
            entry.class == PiiClass::family("payment-card-or-iban") && !entry.conflict_loser
        })
        .expect("family token row");
    assert_eq!(row.action, Action::Redact);
    assert_eq!(row.decided_by, ConflictTier::AnchoredContext);
    let derived = row
        .ambiguity_record
        .as_ref()
        .and_then(|record| record.derived_action.as_ref())
        .expect("derived action on the ambiguity record");
    assert_eq!(derived.action, Action::Redact);
    assert_eq!(
        derived.member_class,
        Some(PiiClass::from_policy_name("custom:credit_card").expect("class"))
    );
}

/// An explicit family rule leaves no derivation trace.
#[test]
fn explicit_family_rule_leaves_no_derived_action_on_the_audit_row() {
    let logger = MemoryLogger::default();
    let policy = payment_family_policy(
        &[
            ("custom:family:payment-card-or-iban", Action::Tokenize),
            ("custom:iban", Action::Redact),
        ],
        Action::Preserve,
    );
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline =
        build_pipeline_builder(&policy, &empty_context(), &rulepacks, &active_locales, None)
            .expect("builder")
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        pipeline
            .redact(&session, RawDocument::Text(NO_CUE_IBAN.to_string()))
            .expect("redact"),
    );

    assert!(clean.contains(FAMILY_TOKEN_MARKER), "got: {clean}");
    let row = logger
        .entries()
        .into_iter()
        .find(|entry| {
            entry.class == PiiClass::family("payment-card-or-iban") && !entry.conflict_loser
        })
        .expect("family token row");
    assert_eq!(row.action, Action::Tokenize);
    assert_eq!(
        row.ambiguity_record
            .as_ref()
            .and_then(|record| record.derived_action.as_ref()),
        None
    );
}

/// Length of the longest byte run shared by `a` and `b`.
fn longest_common_run(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut previous = vec![0usize; b.len() + 1];
    let mut best = 0;
    for &x in a {
        let mut current = vec![0usize; b.len() + 1];
        for (j, &y) in b.iter().enumerate() {
            if x == y {
                current[j + 1] = previous[j] + 1;
                best = best.max(current[j + 1]);
            }
        }
        previous = current;
    }
    best
}

/// Ruling 3746 #1 (a) and (b): every protective action executes on a family
/// token, and the number of original bytes that reach the output is measured,
/// not argued. The session hex is stripped from the replacement first so a
/// chance digit overlap with the random prefix cannot skew the measurement.
/// The table this prints is copied into the REPORT and the policy reference.
#[test]
fn protective_actions_execute_on_family_tokens_and_leak_no_original_byte() {
    let hex = regex::Regex::new(r"[0-9a-f]{8}").unwrap();
    let fixtures = [
        (NO_CUE_IBAN, "Überweisung ", ""),
        (TRAILING_NUMBER_IBANS[0], "Bitte überweisen auf ", " 1234"),
    ];
    for (fixture, (input, prefix, suffix)) in fixtures.iter().enumerate() {
        let value = input
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .expect("fixture shape");
        for action in [
            Action::Redact,
            Action::Tokenize,
            Action::Generalize,
            Action::FormatPreserve,
        ] {
            let policy = payment_family_policy(
                &[
                    ("custom:iban", action),
                    ("custom:credit_card", Action::Preserve),
                    ("custom:phone", Action::Preserve),
                ],
                Action::Preserve,
            );
            let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
            let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
            let pipeline =
                build_pipeline(&policy, &empty_context(), &rulepacks, &active_locales, None)
                    .expect("pipeline");
            let session = Session::new(Scope::Ephemeral).expect("session");
            let clean = clean_text(
                pipeline
                    .redact(&session, RawDocument::Text(input.to_string()))
                    .unwrap_or_else(|err| {
                        panic!("{action:?} must execute on a family token: {err:?}")
                    }),
            );
            let replacement = clean
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .unwrap_or_else(|| panic!("{action:?} fixture {fixture}: output shape changed"));
            let measured = hex.replace_all(replacement, "");
            let run = longest_common_run(value, &measured);
            println!(
                "family-token action table | fixture {fixture} | {} | original bytes surviving (longest run) {run} | restorable {}",
                action.as_str(),
                session.restore(replacement).is_some()
            );
            assert!(
                run < 4,
                "{action:?} fixture {fixture}: a run of {run} original bytes survived"
            );
            let restorable = session.restore(replacement).is_some();
            assert_eq!(
                restorable,
                matches!(action, Action::Tokenize | Action::FormatPreserve),
                "{action:?} restorability"
            );
        }
    }
}

/// Under de-DE a long no-cue IBAN wholly contains a `phone.national.de`
/// shape. Containment precedence hands the validated IBAN the whole span
/// (equal tiers go to the container), the missing anchor then rebuilds it as
/// the family token, and the family action derives to `tokenize` from the
/// members; the span leaves as one family token with no phone sub-run and
/// no IBAN group readable. (Before the rung the phone won the sub-run on
/// rule priority and the IBAN's remainder reached residual coverage on its
/// family view, found by the policy-matrix enumeration, 976 documents.)
#[test]
fn unanchored_iban_containing_a_phone_shape_is_one_family_token() {
    let mut policy = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Tokenize),
            ("custom:phone", Action::Tokenize),
            ("custom:postal_code", Action::Tokenize),
        ],
        Action::Preserve,
    );
    policy.locale = Some(vec![LocaleTag::DeDe]);
    let input = PHONE_WIN_IBAN;
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let pipeline =
        build_pipeline_builder(&policy, &empty_context(), &rulepacks, &active_locales, None)
            .expect("builder")
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");

    let clean = clean_text(
        pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(input.to_string()),
                active_locales.as_slice(),
                &gaze::DictionaryBundle::default(),
            )
            .expect("the residual cell must resolve like its preview"),
    );

    assert_phone_shape_iban_fully_covered(&clean, &logger, FAMILY_TOKEN_MARKER);
}

/// Product path (`build_pipeline_builder`, core + locale-de) with the redaction
/// log attached, so a fixture can prove it exercised the sub-run win it pins.
fn clean_payment_logged(policy: &gaze::Policy, input: &str) -> (String, MemoryLogger) {
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let pipeline =
        build_pipeline_builder(policy, &empty_context(), &rulepacks, &active_locales, None)
            .expect("builder")
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(input.to_string()),
                active_locales.as_slice(),
                &gaze::DictionaryBundle::default(),
            )
            .expect("a residual cell must resolve like its preview"),
    );
    (clean, logger)
}

/// The phone-shape document must leave the process as one replacement over
/// the whole IBAN (`replacement` names its shape: the family token marker or
/// the one-way `[REDACTED]`), with no phone token and no IBAN group readable.
/// Since containment precedence (todo #3740) the validated IBAN wins the
/// whole span over the validated phone shape inside it (equal tiers go to
/// the container); the phone is a loser row that names that rung, and its
/// old sub-run win is exactly what this fixture must no longer exhibit.
fn assert_phone_shape_iban_fully_covered(clean: &str, logger: &MemoryLogger, replacement: &str) {
    assert!(
        !clean.contains(":Custom:phone_"),
        "phone must not win: {clean}"
    );
    assert!(
        clean.contains(replacement),
        "the IBAN leaves as one {replacement} replacement: {clean}"
    );
    assert_no_group_survives(clean, PHONE_WIN_IBAN, "Bitte überweisen auf ", " BIC");
    // The loser row carries the winner's final label: the containment rung,
    // or `AnchoredContext` once the cue-less IBAN is rebuilt as the family
    // fallback (the fallback relabels every row it carries, as before).
    let entries = logger.entries();
    assert!(
        entries.iter().any(|entry| {
            entry.recognizer_id.as_deref() == Some("phone.national.de")
                && entry.conflict_loser
                && matches!(
                    entry.decided_by,
                    ConflictTier::ContainmentPrecedence | ConflictTier::AnchoredContext
                )
        }),
        "fixture must exercise the containment win over the phone shape, or it pins nothing"
    );
    assert!(
        !entries.iter().any(|entry| {
            entry.recognizer_id.as_deref() == Some("phone.national.de") && !entry.conflict_loser
        }),
        "the phone shape must not win a sub-run any more"
    );
}

/// Review 3746 finding 7: one member (`custom:credit_card`) set to `redact`
/// makes the losing IBAN's standalone view, the family class, derive `redact`.
/// Residual coverage used to admit a loser only when its previewed action was
/// exactly `tokenize`, so raising the family action silently dropped every
/// residual cell and the IBAN bytes beside the phone win shipped raw (872
/// documents in the reviewer's 192-arm matrix, `AD56 7551` and `9893` raw).
/// Admission is "the resolved action protects the span"; residual cells keep
/// emitting tokens.
#[test]
fn a_redacting_member_keeps_the_losing_iban_evidence_covered() {
    let policy = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Redact),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Tokenize,
    );

    let (clean, logger) = clean_payment_logged(&policy, PHONE_WIN_IBAN);

    assert_phone_shape_iban_fully_covered(&clean, &logger, "[REDACTED]");
}

/// The #624 regression class on the shape that still reaches residual
/// coverage under containment precedence (ORCH-RULING 3740 #1): the phone
/// wins its straddling sub-run, and the losing IBAN's remaining bytes are
/// covered under the claimant's own derived action. With
/// `custom:credit_card = redact` the family view derives `redact`, so the
/// fragment is the one-way `[REDACTED:<family class>]` marker; an admission
/// gate of "exactly `tokenize`" would drop the cell and ship `AD82 5402 2980
/// 2202 2393` raw.
#[test]
fn a_redacting_member_keeps_a_partially_overlapped_iban_covered() {
    let redacting = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Redact),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Tokenize,
    );
    let (clean, logger) = clean_payment_logged(&redacting, PARTIAL_PHONE_IBAN);
    assert!(
        clean.contains(":Custom:phone_"),
        "phone sub-run win kept: {clean}"
    );
    let family_marker = gaze::redaction_marker(&PiiClass::family("payment-card-or-iban"));
    assert!(
        clean.contains(&family_marker),
        "the IBAN's remainder leaves under the family's derived redact: {clean}"
    );
    assert_no_group_survives(&clean, PARTIAL_PHONE_IBAN, "Bitte überweisen auf ", "");
    let entries = logger.entries();
    let fragment = entries
        .iter()
        .find(|entry| entry.provenance_stage.as_deref() == Some("primary_pipeline.residual"))
        .expect("residual fragment row");
    assert_eq!(fragment.class, PiiClass::family("payment-card-or-iban"));
    assert_eq!(fragment.action, Action::Redact);
    assert_eq!(fragment.decided_by, ConflictTier::None);
    assert!(
        entries.iter().any(|entry| {
            entry.recognizer_id.as_deref() == Some("phone.national.de") && !entry.conflict_loser
        }),
        "fixture must exercise the partial-overlap phone win, or it pins nothing"
    );

    // Same shape under a `redact` default with every member tokenized: the
    // family class takes the default, so the remainder is the same one-way
    // marker (review 3746 finding 7's hole, on a letter that still reaches
    // residual coverage).
    let redact_default = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Tokenize),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Redact,
    );
    let (clean, logger) = clean_payment_logged(&redact_default, PARTIAL_PHONE_IBAN);
    assert!(clean.contains(":Custom:phone_"), "{clean}");
    assert!(clean.contains(&family_marker), "{clean}");
    assert_no_group_survives(&clean, PARTIAL_PHONE_IBAN, "Bitte überweisen auf ", "");
    assert!(
        logger.entries().iter().any(|entry| {
            entry.provenance_stage.as_deref() == Some("primary_pipeline.residual")
                && entry.action == Action::Redact
        }),
        "the remainder must leave as a residual redact fragment"
    );

    // Same shape, every member tokenized: the remainder is a family token.
    let (clean, _) = clean_payment_logged(&member_only_tokenize_policy(), PARTIAL_PHONE_IBAN);
    assert!(clean.contains(":Custom:phone_"), "{clean}");
    assert!(clean.contains(FAMILY_TOKEN_MARKER), "{clean}");
    assert_no_group_survives(&clean, PARTIAL_PHONE_IBAN, "Bitte überweisen auf ", "");
}

/// Review 3746 finding 4: under an active protection trace (the MCP and proxy
/// chokepoints) only `tokenize` and `preserve` are executable, so a family
/// token that derives `redact` fails closed with `UnsupportedActionVariant`.
/// That is the same failure an explicit `redact` rule on a member class
/// already produces on main; the derivation adds no new failure class, it
/// only makes the existing one reachable from a member rule.
#[test]
fn a_derived_redact_fails_under_a_protection_trace_like_an_explicit_one() {
    let cases = [
        (
            "explicit member rule, settled custom:iban token",
            payment_family_policy(
                &[
                    ("custom:iban", Action::Redact),
                    ("custom:credit_card", Action::Tokenize),
                    ("custom:phone", Action::Tokenize),
                ],
                Action::Preserve,
            ),
            NO_CUE_LUHN_BBAN_IBAN,
        ),
        (
            "derived from a redacting member, family token",
            payment_family_policy(
                &[
                    ("custom:iban", Action::Tokenize),
                    ("custom:credit_card", Action::Redact),
                    ("custom:phone", Action::Tokenize),
                ],
                Action::Preserve,
            ),
            NO_CUE_IBAN,
        ),
    ];
    for (case, policy, input) in cases {
        let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
        let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
        let pipeline = build_pipeline(&policy, &empty_context(), &rulepacks, &active_locales, None)
            .expect("pipeline");
        let session = Session::new(Scope::Ephemeral).expect("session");

        let err = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                input,
                active_locales.as_slice(),
                &gaze::DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .err()
            .unwrap_or_else(|| panic!("{case}: a redact under a trace must fail closed"));

        assert!(
            matches!(err, gaze::Error::UnsupportedActionVariant),
            "{case}: {err:?}"
        );
    }
}

/// The pre-existing hole behind finding 7: an explicit `default = redact`
/// reached the same `tokenize`-only residual gate, where the family class took
/// the default, so the losing IBAN's bytes shipped raw there too. Since
/// containment precedence (todo #3740) this whole-IBAN letter no longer
/// reaches residual coverage: the family token itself derives `redact` and the
/// span leaves as one `[REDACTED]`. The residual arm of the same derivation is
/// pinned on the partial-overlap letter in
/// `a_redacting_member_keeps_a_partially_overlapped_iban_covered`.
#[test]
fn a_redact_default_keeps_the_losing_iban_evidence_covered() {
    let policy = payment_family_policy(
        &[
            ("custom:iban", Action::Tokenize),
            ("custom:credit_card", Action::Tokenize),
            ("custom:phone", Action::Tokenize),
        ],
        Action::Redact,
    );

    let (clean, logger) = clean_payment_logged(&policy, PHONE_WIN_IBAN);

    assert_phone_shape_iban_fully_covered(&clean, &logger, "[REDACTED]");
}

// ---------------------------------------------------------------------------
// todo 3757: policy regex custom recognizers bind their collision metadata
// through the registry. Product path: `build_pipeline` on a policy whose
// custom recognizers are regex rules, the `[[policy.custom_recognizers]]`
// shape from docs/reference/policy.md, with no bundled pack except where an
// anchor cue pack is needed.
//
// The candidate side always bound (a regex rule's candidates carry the policy
// `name` as `recognizer_id`), so precedence, ties and anchors decided before
// this fix. The registry side did not: the rule was wrapped as a detector with
// a constant id and a placeholder class, so `family_member_classes` saw no
// member and a family token derived its action from the default alone. Under a
// `preserve` default the tie token and the no-anchor token shipped raw.
// ---------------------------------------------------------------------------

struct RegexMember {
    name: &'static str,
    class: &'static str,
    variant: &'static str,
    precedence: u32,
    mandatory_anchor: Option<&'static str>,
}

fn regex_family_policy(
    family: &str,
    pattern: &str,
    members: &[RegexMember],
    rules: Vec<RuleSpec>,
    locale: LocaleTag,
) -> gaze::Policy {
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.locale = Some(vec![locale]);
    policy.rules = rules;
    for member in members {
        let mut detector = gaze::DetectorSpec::default();
        detector.kind = DetectorKind::Regex;
        detector.name = member.name.to_string();
        detector.pattern = Some(pattern.to_string());
        detector.class = PiiClass::from_policy_name(member.class).expect("class");
        detector.collision = Some(gaze::CollisionMembership::new(
            family,
            member.variant,
            member.precedence,
            member.mandatory_anchor.map(str::to_string),
        ));
        policy.detectors.push(detector);
    }
    policy
}

fn class_rule(class: &str, action: Action) -> RuleSpec {
    RuleSpec::Class {
        class: PiiClass::from_policy_name(class).expect("class"),
        action,
    }
}

fn clean_regex_family(
    policy: &gaze::Policy,
    rulepacks: &[Rulepack],
    input: &str,
) -> (String, MemoryLogger) {
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let logger = MemoryLogger::default();
    let pipeline =
        build_pipeline_builder(policy, &empty_context(), rulepacks, &active_locales, None)
            .expect("builder")
            .redaction_logger(logger.clone())
            .build()
            .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let clean = clean_text(
        pipeline
            .pseudonymize_with_detect_context(
                &session,
                RawDocument::Text(input.to_string()),
                active_locales.as_slice(),
                &gaze::DictionaryBundle::default(),
            )
            .expect("redact"),
    );
    (clean, logger)
}

fn family_token_row(logger: &MemoryLogger, family: &str) -> RedactionEntry {
    logger
        .entries()
        .into_iter()
        .find(|entry| entry.class == PiiClass::family(family) && !entry.conflict_loser)
        .expect("family token row")
}

fn loser_row(logger: &MemoryLogger, recognizer_id: &str) -> RedactionEntry {
    logger
        .entries()
        .into_iter()
        .find(|entry| entry.conflict_loser && entry.recognizer_id.as_deref() == Some(recognizer_id))
        .unwrap_or_else(|| panic!("loser row for {recognizer_id}"))
}

/// The todo's two-recognizer family: `tenant.alpha` and `tenant.beta` on one
/// pattern, one family, equal precedence.
fn tenant_document_members(precedence: (u32, u32)) -> [RegexMember; 2] {
    [
        RegexMember {
            name: "tenant.alpha",
            class: "custom:alpha_doc",
            variant: "alpha",
            precedence: precedence.0,
            mandatory_anchor: None,
        },
        RegexMember {
            name: "tenant.beta",
            class: "custom:beta_doc",
            variant: "beta",
            precedence: precedence.1,
            mandatory_anchor: None,
        },
    ]
}

const TENANT_TICKET: &str = "ticket CASE-0001 open";
const TENANT_FAMILY_MARKER: &str = ":Custom:family:tenant-document_";

#[test]
fn policy_regex_precedence_tie_family_token_derives_the_member_action() {
    let policy = regex_family_policy(
        "tenant-document",
        r"CASE-[0-9]{4}",
        &tenant_document_members((10, 10)),
        vec![
            class_rule("custom:alpha_doc", Action::Tokenize),
            class_rule("custom:beta_doc", Action::Tokenize),
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        LocaleTag::Global,
    );
    let (clean, logger) = clean_regex_family(&policy, &[], TENANT_TICKET);

    assert!(
        clean.contains(TENANT_FAMILY_MARKER),
        "equal precedence must emit and protect the family token: {clean}"
    );
    assert!(!clean.contains("CASE-0001"), "tie token leaked: {clean}");

    let row = family_token_row(&logger, "tenant-document");
    assert_eq!(row.action, Action::Tokenize);
    assert_eq!(row.decided_by, ConflictTier::CollisionPolicy);
    let record = row.ambiguity_record.as_ref().expect("ambiguity record");
    let derived = record.derived_action.as_ref().expect("derived action");
    assert_eq!(derived.action, Action::Tokenize);
    assert_eq!(
        derived.member_class,
        Some(PiiClass::from_policy_name("custom:alpha_doc").expect("class")),
        "the member whose rule set the action is credited"
    );
    assert_eq!(
        record.losing_candidates,
        vec![
            gaze::LosingCandidate::new(
                PiiClass::from_policy_name("custom:alpha_doc").expect("class"),
                "tenant.alpha",
            ),
            gaze::LosingCandidate::new(
                PiiClass::from_policy_name("custom:beta_doc").expect("class"),
                "tenant.beta",
            ),
        ],
        "both tied members are listed with their own class"
    );
    for (recognizer_id, class) in [
        ("tenant.alpha", "custom:alpha_doc"),
        ("tenant.beta", "custom:beta_doc"),
    ] {
        let loser = loser_row(&logger, recognizer_id);
        assert_eq!(
            loser.class,
            PiiClass::from_policy_name(class).expect("class"),
            "loser row carries the member's own class, not the winner's"
        );
        assert_eq!(loser.collision_family.as_deref(), Some("tenant-document"));
    }
}

#[test]
fn policy_regex_precedence_tie_takes_the_strictest_member_action() {
    let policy = regex_family_policy(
        "tenant-document",
        r"CASE-[0-9]{4}",
        &tenant_document_members((10, 10)),
        vec![
            class_rule("custom:alpha_doc", Action::Tokenize),
            class_rule("custom:beta_doc", Action::Redact),
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        LocaleTag::Global,
    );
    let (clean, logger) = clean_regex_family(&policy, &[], TENANT_TICKET);

    assert_eq!(clean, "ticket [REDACTED] open");
    let derived = family_token_row(&logger, "tenant-document")
        .ambiguity_record
        .and_then(|record| record.derived_action)
        .expect("derived action");
    assert_eq!(derived.action, Action::Redact);
    assert_eq!(
        derived.member_class,
        Some(PiiClass::from_policy_name("custom:beta_doc").expect("class"))
    );
}

/// The shape of the docs/reference/policy.md precedence example, under neutral
/// names (`xtask no-tenant-knowledge` denies tenant-shaped identifiers in crate
/// sources): the lower `precedence` member outranks the other. Both directions,
/// so registration order cannot fake the verdict.
#[test]
fn policy_regex_precedence_decides_the_family_winner() {
    for (ref_precedence, code_precedence, winner, loser) in [
        (50, 60, "ticket_ref", "ticket_code"),
        (60, 50, "ticket_code", "ticket_ref"),
    ] {
        let policy = regex_family_policy(
            "tenant-tickets",
            r"ORD-[0-9]+",
            &[
                RegexMember {
                    name: "tenant.ticket_ref",
                    class: "custom:ticket_ref",
                    variant: "ticket-ref",
                    precedence: ref_precedence,
                    mandatory_anchor: None,
                },
                RegexMember {
                    name: "tenant.ticket_code",
                    class: "custom:ticket_code",
                    variant: "ticket-code",
                    precedence: code_precedence,
                    mandatory_anchor: None,
                },
            ],
            vec![
                class_rule("custom:ticket_ref", Action::Tokenize),
                class_rule("custom:ticket_code", Action::Tokenize),
                RuleSpec::Default {
                    action: Action::Preserve,
                },
            ],
            LocaleTag::Global,
        );
        let (clean, logger) = clean_regex_family(&policy, &[], "order ORD-1234 shipped");

        assert!(
            clean.contains(&format!(":Custom:{winner}_")),
            "lower precedence wins: {clean}"
        );
        assert!(!clean.contains("ORD-1234"), "leaked: {clean}");
        let winner_row = logger
            .entries()
            .into_iter()
            .find(|entry| !entry.conflict_loser)
            .expect("winner row");
        assert_eq!(winner_row.decided_by, ConflictTier::CollisionPolicy);
        assert_eq!(
            winner_row.recognizer_id.as_deref(),
            Some(format!("tenant.{winner}").as_str())
        );
        let loser_row = loser_row(&logger, &format!("tenant.{loser}"));
        assert_eq!(
            loser_row.class,
            PiiClass::from_policy_name(&format!("custom:{loser}")).expect("class")
        );
    }
}

/// A policy regex member with `mandatory_anchor = "iban"` under the `locale-de`
/// cue pack: no cue in range falls back to the family token, whose action
/// derives from the member's rule; a cue in range keeps the member's class.
#[test]
fn policy_regex_mandatory_anchor_applies_and_the_fallback_derives_the_member_action() {
    let policy = regex_family_policy(
        "tenant-account",
        r"K-[0-9]{6}",
        &[RegexMember {
            name: "tenant.konto",
            class: "custom:konto",
            variant: "konto",
            precedence: 10,
            mandatory_anchor: Some("iban"),
        }],
        vec![
            class_rule("custom:konto", Action::Tokenize),
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
        LocaleTag::DeDe,
    );
    let rulepacks = [embedded_rulepack("locale-de")];

    let (clean, logger) = clean_regex_family(&policy, &rulepacks, "Zahlung K-123456 heute");
    assert!(
        clean.contains(":Custom:family:tenant-account_"),
        "no cue in range must fall back to the family token: {clean}"
    );
    assert!(
        !clean.contains("K-123456"),
        "no-anchor token leaked: {clean}"
    );
    let row = family_token_row(&logger, "tenant-account");
    assert_eq!(row.action, Action::Tokenize);
    assert_eq!(row.decided_by, ConflictTier::AnchoredContext);
    let record = row.ambiguity_record.expect("ambiguity record");
    assert_eq!(record.reason, gaze::AmbiguityReason::NoAnchor);
    assert_eq!(
        record.losing_candidates,
        vec![gaze::LosingCandidate::new(
            PiiClass::from_policy_name("custom:konto").expect("class"),
            "tenant.konto",
        )]
    );
    let derived = record.derived_action.expect("derived action");
    assert_eq!(derived.action, Action::Tokenize);
    assert_eq!(
        derived.member_class,
        Some(PiiClass::from_policy_name("custom:konto").expect("class"))
    );

    let (clean, _) = clean_regex_family(&policy, &rulepacks, "IBAN K-123456 heute");
    assert!(
        clean.contains(":Custom:konto_"),
        "a cue in range keeps the member class: {clean}"
    );
}

#[test]
fn policy_regex_recognizers_register_under_their_policy_name() {
    let policy = regex_family_policy(
        "tenant-document",
        r"CASE-[0-9]{4}",
        &tenant_document_members((10, 10)),
        vec![RuleSpec::Default {
            action: Action::Tokenize,
        }],
        LocaleTag::Global,
    );
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline =
        build_pipeline(&policy, &empty_context(), &[], &active_locales, None).expect("pipeline");

    for (name, class) in [
        ("tenant.alpha", "custom:alpha_doc"),
        ("tenant.beta", "custom:beta_doc"),
    ] {
        let recognizer = pipeline
            .registry()
            .recognizer(name)
            .unwrap_or_else(|| panic!("{name} is registered under its policy name"));
        assert_eq!(recognizer.id(), name);
        assert_eq!(
            recognizer.supported_class(),
            &PiiClass::from_policy_name(class).expect("class")
        );
    }
    assert_eq!(
        pipeline.registry().family_member_classes("tenant-document"),
        vec![
            PiiClass::from_policy_name("custom:alpha_doc").expect("class"),
            PiiClass::from_policy_name("custom:beta_doc").expect("class"),
        ]
    );
}

/// A policy `kind = "regex"` recognizer emits at the confidence the `Detector`
/// wrapper hard-coded (1.0), not `RegexDetector::with_source`'s 0.70 default.
/// Class priority and rule priority tie here (same class, both rules at
/// priority 0) and the two spans overlap without being identical, so the score
/// rung decides ahead of span length, and dropping
/// `with_base_score` in `register_policy_detectors` would silently hand it to
/// the rulepack rule. Pins that behaviour-preserver: the policy rule must win,
/// and it must win *on score*.
#[test]
fn policy_regex_rule_outranks_a_same_class_rulepack_rule_on_score() {
    let rulepack = Rulepack::parse(
        r#"
schema_version = "0.1.0"
rulepack_id = "score-rival"
rulepack_version = "0.6.0"
default_locales = ["global"]

[[recognizers]]
id = "pack.shape"
class = "custom:shape"
enabled = true
locales = ["global"]

[recognizers.match]
kind = "regex"
pattern = 'ACME-[0-9]{4} END'

[recognizers.scoring]
base = 0.70
priority = 0
"#,
    )
    .expect("rulepack");
    let mut policy = gaze::Policy::default();
    policy.session = SessionPolicy::default();
    policy.locale = Some(vec![LocaleTag::Global]);
    policy.rules = vec![
        class_rule("custom:shape", Action::Tokenize),
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    let mut detector = gaze::DetectorSpec::default();
    detector.kind = DetectorKind::Regex;
    detector.name = "tenant.shape".to_string();
    detector.pattern = Some("ACME-[0-9]{4}".to_string());
    detector.class = PiiClass::from_policy_name("custom:shape").expect("class");
    policy.detectors.push(detector);

    let (clean, logger) = clean_regex_family(&policy, &[rulepack], "ref ACME-1234 END");

    assert!(!clean.contains("ACME-1234"), "leaked: {clean}");
    let winner = logger
        .entries()
        .into_iter()
        .find(|entry| !entry.conflict_loser)
        .expect("winner row");
    assert_eq!(
        winner.recognizer_id.as_deref(),
        Some("tenant.shape"),
        "the policy rule's score must outrank the rulepack rule's 0.70"
    );
    assert_eq!(
        winner.decided_by,
        ConflictTier::Score,
        "the score rung decides it; any other tier means the scores tied"
    );
    let loser = loser_row(&logger, "pack.shape");
    assert_eq!(
        loser.class,
        PiiClass::from_policy_name("custom:shape").expect("class")
    );
}

/// Member classes of every anchored family, from the built registry.
fn registry_anchored_family_members(
    pipeline: &gaze::Pipeline,
) -> BTreeMap<String, BTreeSet<PiiClass>> {
    let registry = pipeline.registry();
    registry
        .family_policy()
        .anchored_families()
        .into_iter()
        .map(|family| {
            let members = registry
                .family_member_classes(&family)
                .into_iter()
                .collect::<BTreeSet<_>>();
            (family, members)
        })
        .collect()
}

/// Todo 3761's validation: the registry (which decides the runtime action)
/// and `mandatory_anchor_families` (which decides the load-time notice) must
/// agree on which classes belong to every anchored family, for the same
/// rulepacks and policy, policy regex members included.
#[test]
fn collision_family_members_agree_between_registry_and_assembly() {
    let policy = regex_family_policy(
        "tenant-account",
        r"K-[0-9]{6}",
        &[
            RegexMember {
                name: "tenant.konto",
                class: "custom:konto",
                variant: "konto",
                precedence: 10,
                mandatory_anchor: Some("iban"),
            },
            RegexMember {
                name: "tenant.kunde",
                class: "custom:kunde",
                variant: "kunde",
                precedence: 20,
                mandatory_anchor: None,
            },
        ],
        vec![RuleSpec::Default {
            action: Action::Tokenize,
        }],
        LocaleTag::DeDe,
    );
    let rulepacks = [embedded_rulepack("core"), embedded_rulepack("locale-de")];
    let active_locales = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline(&policy, &empty_context(), &rulepacks, &active_locales, None)
        .expect("pipeline");

    let from_registry = registry_anchored_family_members(&pipeline);
    let from_assembly = mandatory_anchor_families(&policy, &rulepacks, &active_locales);

    assert_eq!(from_registry, from_assembly);
    assert_eq!(
        from_registry.get("tenant-account"),
        Some(&BTreeSet::from([
            PiiClass::from_policy_name("custom:konto").expect("class"),
            PiiClass::from_policy_name("custom:kunde").expect("class"),
        ])),
        "the policy regex family is anchored and both members are visible"
    );
    assert!(
        from_registry.contains_key("payment-card-or-iban"),
        "the bundled anchored family is in the comparison: {from_registry:?}"
    );
}
