use gaze::{
    Action, CleanDocument, Context, PiiClass, Policy, PolicyError, RawDocument, RuleSpec,
    SafetyNetPolicy, Scope, Session,
};
use gaze_assembly::{build_pipeline, resolve_policy_inputs, BuildError, CorePipelineConfig};
use std::path::{Path, PathBuf};

#[test]
fn core_pipeline_inline_dictionary_tokenizes_and_restores() {
    let path = std::env::temp_dir().join(format!(
        "gaze-core-inline-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(
        &path,
        r#"
schema_version = "0.1.0"
rulepack_id = "tenant-songs"
rulepack_version = "0.1.0"
default_locales = ["global"]

[[recognizers]]
id = "tenant.song"
class = "custom:tenant_song"
enabled = true

[recognizers.match]
kind = "dictionary"
terms = ["tenant-song-xyz"]
case_sensitive = true
"#,
    )
    .expect("write rulepack");
    let built = CorePipelineConfig::new()
        .with_rulepack_path(path.clone())
        .build();
    std::fs::remove_file(path).expect("remove rulepack");
    let core = built.expect("build core pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let input = "Play tenant-song-xyz again.";
    let CleanDocument::Text(clean) = core.pseudonymize_text(&session, input).expect("clean") else {
        panic!("expected text");
    };
    assert!(
        !clean.contains("tenant-song-xyz"),
        "dictionary term leaked: {clean}"
    );
    assert!(
        clean.contains(":Custom:tenant_song_1>"),
        "missing token: {clean}"
    );
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .expect("restore"),
        input
    );
}

struct Fixture {
    dir: PathBuf,
    inline: PathBuf,
    file: PathBuf,
    terms: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gaze-core-dictionaries-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir(&dir).expect("create fixture directory");
        let inline = dir.join("inline.toml");
        let file = dir.join("file.toml");
        let terms = dir.join("terms.txt");
        std::fs::write(
            &inline,
            include_str!("fixtures/core_dictionary_inline.toml"),
        )
        .expect("write inline fixture");
        std::fs::write(&terms, include_str!("fixtures/core_dictionary_terms.txt"))
            .expect("write terms fixture");
        write_file_rulepack(&file, &terms);
        Self {
            dir,
            inline,
            file,
            terms,
        }
    }

    fn config(&self) -> CorePipelineConfig {
        CorePipelineConfig::new()
            .with_rulepack_path(self.inline.clone())
            .with_rulepack_path(self.file.clone())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).expect("remove fixture directory");
    }
}

fn write_file_rulepack(pack: &Path, terms: &Path) {
    let quoted = serde_json::to_string(&terms.to_str().expect("UTF-8 path")).expect("quote path");
    let contents =
        include_str!("fixtures/core_dictionary_file.toml").replace("\"__TERMS_FILE__\"", &quoted);
    std::fs::write(pack, contents).expect("write file rulepack");
}

fn text(clean: CleanDocument) -> String {
    let CleanDocument::Text(text) = clean else {
        panic!("expected text")
    };
    text
}

#[test]
fn core_pipeline_dictionary_fixtures_match_policy_tokens_manifests_and_restore() {
    let fixture = Fixture::new();
    let core = fixture.config().build().expect("core pipeline");
    let mut policy = Policy::default();
    policy.rulepacks.bundled = vec!["core".into()];
    policy.rulepacks.paths = vec![fixture.inline.clone(), fixture.file.clone()];
    policy.rules = vec![RuleSpec::Default {
        action: Action::Tokenize,
    }];
    let inputs = resolve_policy_inputs(&policy, None, None, None).expect("policy inputs");
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("empty context");
    let pipeline = build_pipeline(
        &policy,
        &context,
        &inputs.rulepacks,
        &inputs.locale_chain,
        None,
    )
    .expect("policy pipeline");
    assert_eq!(
        core.locale_chain().as_slice(),
        inputs.locale_chain.as_slice()
    );
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/core_dictionary_cases.json"))
            .expect("fixture cases");
    for case in cases.as_array().expect("case array") {
        let input = case["input"].as_str().expect("input");
        // Import the same empty namespace so the literal tokens can be compared.
        let session =
            Session::new(Scope::Conversation("dictionary-parity".into())).expect("session");
        let policy_session = Session::import(session.export().expect("export")).expect("import");
        let manifest_session = Session::import(session.export().expect("export")).expect("import");
        let clean = text(core.pseudonymize_text(&session, input).expect("core clean"));
        let (policy_clean, policy_spans, _) = pipeline
            .clean_with_safety_net_policy_detect_context(
                &policy_session,
                RawDocument::Text(input.into()),
                inputs.locale_chain.as_slice(),
                &inputs.dictionaries,
                SafetyNetPolicy::default(),
            )
            .expect("policy clean");
        let (core_clean, core_spans, _) = core
            .pipeline()
            .clean_with_safety_net_policy_detect_context(
                &manifest_session,
                RawDocument::Text(input.into()),
                core.locale_chain().as_slice(),
                core.dictionaries(),
                SafetyNetPolicy::default(),
            )
            .expect("core manifest clean");
        assert_eq!(clean, text(policy_clean), "policy tokens: {input}");
        assert_eq!(clean, text(core_clean), "core manifest tokens: {input}");
        assert_eq!(core_spans, policy_spans, "manifest classes/spans: {input}");
        let expected_classes: Vec<_> = case["classes"]
            .as_array()
            .expect("classes")
            .iter()
            .map(|class| PiiClass::custom(class.as_str().expect("class")).expect("custom class"))
            .collect();
        assert_eq!(
            core_spans
                .iter()
                .map(|span| span.class.clone())
                .collect::<Vec<_>>(),
            expected_classes,
            "{input}"
        );
        if expected_classes.is_empty() {
            assert_eq!(clean, input, "benign near miss must stay raw");
        }
        for span in &core_spans {
            assert_eq!(
                session
                    .restore_strict(&clean[span.clean_span.clone()])
                    .expect("token restore"),
                &input[span.raw_span.clone()]
            );
        }
        assert_eq!(
            core.pipeline()
                .restore_strict_text(&session, &clean)
                .expect("core restore"),
            input
        );
        assert_eq!(
            pipeline
                .restore_strict_text(&policy_session, &clean)
                .expect("policy restore"),
            input
        );
    }
}

#[test]
fn core_pipeline_into_parts_retains_dictionary_inputs_on_every_call() {
    let fixture = Fixture::new();
    let (pipeline, locales, dictionaries) = fixture.config().build().expect("core").into_parts();
    // File terms are read at build time, just like policy-derived inputs.
    std::fs::remove_file(&fixture.terms).expect("remove terms after build");
    for input in [
        "tenant-file-song-xyz",
        "TENANT-ALBUM-ABC",
        "tenant-song-xyz",
    ] {
        let session = Session::new(Scope::Ephemeral).expect("session");
        let clean = text(
            pipeline
                .pseudonymize_with_detect_context(
                    &session,
                    RawDocument::Text(input.into()),
                    locales.as_slice(),
                    &dictionaries,
                )
                .expect("clean"),
        );
        assert_ne!(clean, input);
        assert_eq!(
            pipeline
                .restore_strict_text(&session, &clean)
                .expect("restore"),
            input
        );
    }
}

fn assert_bad_terms_file(config: CorePipelineConfig) {
    let error = match config.build() {
        Ok(_) => panic!("unreadable terms_file must fail closed"),
        Err(error) => error,
    };
    assert!(
        matches!(error, BuildError::Policy(PolicyError::BadDictionary { name, reason })
        if name == "tenant.file-song" && reason.starts_with("failed to read terms_file:"))
    );
}

#[test]
fn core_pipeline_missing_terms_file_is_a_typed_error() {
    let fixture = Fixture::new();
    std::fs::remove_file(&fixture.terms).expect("remove terms");
    assert_bad_terms_file(fixture.config());
}

#[test]
fn core_pipeline_unreadable_terms_file_is_a_typed_error() {
    let fixture = Fixture::new();
    write_file_rulepack(&fixture.file, &fixture.dir);
    assert_bad_terms_file(fixture.config());
}

#[test]
fn core_pipeline_non_utf8_terms_file_is_a_typed_error() {
    let fixture = Fixture::new();
    std::fs::write(&fixture.terms, [0xff]).expect("invalid UTF-8 file");
    assert_bad_terms_file(fixture.config());
}

fn build_context_dictionary(extra_source: &str) -> Result<gaze_assembly::CorePipeline, BuildError> {
    let fixture = Fixture::new();
    std::fs::write(&fixture.inline, format!(r#"
schema_version = "0.1.0"
rulepack_id = "tenant-context"
rulepack_version = "0.1.0"
default_locales = ["global"]
[[recognizers]]
id = "tenant.song"
class = "custom:tenant_song"
enabled = true
[recognizers.match]
kind = "dictionary"
terms_from_context = "tenant_terms"
case_sensitive = true
{extra_source}
"#)).expect("write context rulepack");
    CorePipelineConfig::new().with_rulepack_path(fixture.inline.clone()).build()
}

#[test]
fn core_pipeline_context_dictionary_is_a_typed_error() {
    let built = build_context_dictionary("");
    assert!(matches!(built, Err(BuildError::Policy(PolicyError::BadDictionary { ref name, .. })) if name == "tenant.song"),
        "CorePipeline must reject a dictionary whose context it cannot populate");
}

#[test]
fn core_pipeline_mixed_dictionary_sources_are_a_typed_error() {
    for extra_source in [r#"terms = ["tenant-song-xyz"]"#, r#"terms_file = "unused-terms.txt""#] {
        let built = build_context_dictionary(extra_source);
        assert!(matches!(built, Err(BuildError::Rulepack(_))),
            "rulepack validation must reject context combined with {extra_source}");
    }
}
