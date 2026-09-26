//! The bundled `locale-de` / `locale-en` street lexicons reach the pipeline
//! through assembly, per active locale (todo 3670). A stand-in labelled `ner`
//! marks the street, as the bundled NER recognizer would.
use gaze::{
    Candidate, CleanDocument, ConflictTier, Context, DetectContext, DictionaryBundle, LocaleChain,
    PiiClass, Policy, RawDocument, Recognizer, Rulepack, RulepackSource, SafetyNetPolicy, Scope,
    Session,
};
use gaze_assembly::build_pipeline_builder;

struct NerStreet(&'static str);

impl Recognizer for NerStreet {
    fn id(&self) -> &str {
        "ner"
    }
    fn supported_class(&self) -> &PiiClass {
        &PiiClass::Location
    }
    fn token_family(&self) -> &str {
        "counter"
    }
    fn detect(
        &self,
        input: &str,
        _: &DetectContext<'_>,
    ) -> Result<Vec<Candidate>, gaze::DetectError> {
        Ok(input
            .find(self.0)
            .map(|start| {
                Candidate::new(
                    start..start + self.0.len(),
                    PiiClass::Location,
                    "ner",
                    0.99,
                    0,
                    None,
                    "counter",
                    "ner/stand-in",
                    ConflictTier::None,
                    Vec::new(),
                )
            })
            .into_iter()
            .collect())
    }
}

fn policy(bundled: &str, locales: &str) -> Policy {
    let text = format!(
        "schema_version = \"0.1.0\"\n\n[session]\nscope = \"persistent\"\nttl_secs = 86400\n\n\
         [policy.rulepacks]\nbundled = [{bundled}]\n\n[locale]\nactive = [{locales}]\n\n\
         [[rule]]\nkind = \"default\"\naction = \"tokenize\"\n"
    );
    let path = std::env::temp_dir().join(format!(
        "gaze-street-house-numbers-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, text).expect("write policy");
    let policy = Policy::load(&path).expect("policy");
    let _ = std::fs::remove_file(&path);
    policy
}

/// Raw substrings tokenized when `input` is cleaned under `policy` with the
/// stand-in marking `street`.
fn tokenized(policy: &Policy, packs: &[&str], street: &'static str, input: &str) -> Vec<String> {
    let rulepacks = packs
        .iter()
        .map(|name| {
            Rulepack::load(RulepackSource::Embedded(
                gaze_recognizers::embedded(name).expect("embedded rulepack"),
            ))
            .expect("rulepack")
        })
        .collect::<Vec<_>>();
    let context = Context::from_json_str(r#"{"dictionaries":{},"class_map":{},"fields":{}}"#)
        .expect("context");
    let active = LocaleChain::merge_policy_and_cli(policy.locale.as_deref(), None);
    let pipeline = build_pipeline_builder(policy, &context, &rulepacks, &active, None)
        .expect("builder")
        .recognizer(NerStreet(street))
        .build()
        .expect("pipeline");
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, manifest, _) = pipeline
        .clean_with_safety_net_policy_detect_context(
            &session,
            RawDocument::Text(input.to_string()),
            active.as_slice(),
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .expect("clean");
    let CleanDocument::Text(text) = clean else {
        panic!("expected text");
    };
    assert_eq!(session.restore_strict_text(&text).expect("restore"), input);
    manifest
        .into_iter()
        .map(|span| input[span.raw_span].to_string())
        .collect()
}

const PACKS: &[&str] = &["core", "locale-de", "locale-en"];
const BOTH: &str = r#""core", "locale-de", "locale-en""#;

#[test]
fn german_pack_licenses_the_number_after_a_ner_street() {
    let policy = policy(BOTH, r#""de-DE""#);
    let found = tokenized(
        &policy,
        PACKS,
        "Hauptstraße",
        "Adresse: Hauptstraße 5, Bonn",
    );
    assert_eq!(found, ["Hauptstraße", "5"]);
}

#[test]
fn english_pack_licenses_the_number_before_a_ner_street() {
    let policy = policy(BOTH, r#""en-US""#);
    let found = tokenized(
        &policy,
        PACKS,
        "Harbor Road",
        "Ship to 230 Harbor Road please",
    );
    assert_eq!(found, ["230", "Harbor Road"]);
}

#[test]
fn lexicon_follows_the_active_locale_chain() {
    let policy = policy(BOTH, r#""en-US""#);
    let found = tokenized(
        &policy,
        PACKS,
        "Hauptstraße",
        "Adresse: Hauptstraße 5, Bonn",
    );
    assert_eq!(found, ["Hauptstraße"]);
}

#[test]
fn core_without_locale_packs_tokenizes_no_house_number() {
    let policy = policy(r#""core""#, r#""de-DE""#);
    let found = tokenized(
        &policy,
        &["core"],
        "Hauptstraße",
        "Adresse: Hauptstraße 5, Bonn",
    );
    assert_eq!(found, ["Hauptstraße"]);
}
