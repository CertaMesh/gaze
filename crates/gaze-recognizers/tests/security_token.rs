//! Regression fixtures for the `security_token.*` core recognizers (solo todo #2318).
//!
//! EVERY positive fixture here encodes a structural shape that was MEASURED in the Dataiku EN/DE
//! holdout, not a phrasing invented alongside the implementation. That discipline exists because
//! todo #2412 shipped 962 green tests for URL cue phrasings that occur in 5 of 276 gold spans and
//! produced a zero real-corpus delta. The measured distribution of the 219 gold SECURITYTOKEN
//! spans is:
//!
//! | dimension        | counts                                                     |
//! |------------------|------------------------------------------------------------|
//! | language         | en 117, de 102                                             |
//! | charset          | base64url 202, hex-only 9, uuid 5, other 3                 |
//! | entropy proxy    | >=0.7 on 204 of 219 (93%)                                  |
//! | utf8 length      | min 10, p25 16, median 20, p90 29, max 55                  |
//! | fixed prefix     | `AKIA` 78 (35.6%), JWT `eyJ` 6, `sk_` 1, none 134          |
//! | cue adjacency    | 193 of 219 (88.1%) — "token" 192, "sicherheitstoken" 37    |
//!
//! The cue-adjacency row is the inverse of URL's (1.8%) and is why a cue-anchored rule is correct
//! here and was wrong there. Only measurement distinguishes the two cases.
//!
//! Fixture values are synthetic. The AWS shapes use the key IDs published in AWS's own
//! documentation as non-functional examples; the JWT and cue-anchored payloads are hand-built
//! base64url strings that decode to nothing sensitive.

use gaze::Context;
use gaze::{
    Action, CleanDocument, DictionaryBundle, LocaleChain, LocaleTag, PiiClass, Pipeline,
    RawDocument, RuleSpec, Rulepack, RulepackSource, Scope, Session,
};
use gaze_recognizers::embedded;

fn empty_context() -> Context {
    Context {
        dictionaries: std::collections::HashMap::new(),
        class_map: std::collections::HashMap::new(),
        fields: serde_json::Map::new(),
    }
}

fn security_token_class() -> PiiClass {
    PiiClass::custom("security_token")
}

/// Builds the core bundle under an explicit locale chain with locale-gated auto-activation OFF.
///
/// `auto_activate_locale_gated = false` is the weakest configuration a default adopter can have.
/// Both `security_token.*` recognizers are declared `safety_tier = "safe_default"` with
/// `locales = ["global"]`, so `recognizer_activates` admits them unconditionally. If either were
/// declared `locale_gated` instead, these tests would still pass under the benchmark's
/// configuration (all three cells set `auto_activate_locale_gated = true`) while a default adopter
/// got no credential protection at all. That is exactly the silent-inertness failure behind todo
/// #2403, so the distinction is made to fail loudly here.
fn pipeline_for(chain: &[LocaleTag]) -> Pipeline {
    let rulepack = Rulepack::load(RulepackSource::Embedded(
        embedded("core").expect("core rulepack"),
    ))
    .expect("core loads");
    let mut policy = gaze::Policy::default();
    policy.rules = vec![
        RuleSpec::Class {
            class: security_token_class(),
            action: Action::Tokenize,
        },
        RuleSpec::Default {
            action: Action::Preserve,
        },
    ];
    policy.rulepacks.bundled = vec!["core".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(chain));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &[rulepack], &locale_chain, None)
        .expect("pipeline")
}

fn clean_under(chain: &[LocaleTag], text: &str) -> String {
    let pipeline = pipeline_for(chain);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, _, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            chain,
            &DictionaryBundle::default(),
        )
        .expect("clean");
    match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    }
}

fn clean(text: &str) -> String {
    clean_under(&[LocaleTag::Global], text)
}

/// Asserts the whole credential is gone and the surrounding prose survives.
///
/// Whole-span coverage is the point. In the before-state the kiji cell overlapped 65 of 150
/// SECURITYTOKEN entities while fully covering only 2 — 63 overlapped-but-not-covered. A
/// fragment of a credential is still a leaked credential.
fn assert_token_removed(text: &str, token: &str, surviving_context: &[&str]) {
    let cleaned = clean(text);
    assert!(
        !cleaned.contains(token),
        "credential {token:?} survived tokenization in {cleaned:?}"
    );
    for fragment in surviving_context {
        assert!(
            cleaned.contains(fragment),
            "context {fragment:?} should survive but is missing from {cleaned:?}"
        );
    }
}

fn assert_unchanged(text: &str) {
    assert_eq!(clean(text), text, "text must pass through untouched");
}

// ------------------------------------------------- provider-prefixed: AWS (78 of 219 gold spans)

#[test]
fn aws_access_key_id_is_tokenized_whole() {
    assert_token_removed(
        "Please rotate AKIAIOSFODNN7EXAMPLE before Friday.",
        "AKIAIOSFODNN7EXAMPLE",
        &["Please rotate ", " before Friday."],
    );
}

#[test]
fn aws_temporary_access_key_id_is_tokenized_whole() {
    // `ASIA` has zero occurrences in the holdout and contributes nothing to the benchmark number.
    // It ships because omitting it would catch permanent AWS keys and silently miss temporary
    // ones. This fixture is the adopter-facing guarantee, not a coverage claim.
    assert_token_removed(
        "Temporary credential ASIAY34FZKBOKMUTVV7A expires in one hour.",
        "ASIAY34FZKBOKMUTVV7A",
        &["Temporary credential ", " expires in one hour."],
    );
}

#[test]
fn aws_shape_is_case_sensitive() {
    // Lowercasing the prefix would admit ordinary prose. The rule is deliberately case-sensitive.
    assert_unchanged("the akiaiosfodnn7example string is not a credential");
}

#[test]
fn aws_shape_requires_exact_sixteen_trailing_characters() {
    // 15 trailing characters: one short of the fixed AWS shape, so not a match.
    assert_unchanged("AKIAIOSFODNN7EXAMPL is one character short");
}

// ------------------------------------------------------- provider-prefixed: JWT (6 of 219 spans)

#[test]
fn jwt_three_segment_structure_is_tokenized_whole() {
    // `eyJ` is the base64url encoding of `{"`, so this asserts the first segment decodes to a JSON
    // object — the JOSE header — rather than asserting a vendor brand.
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJleGFtcGxlIn0.c2lnbmF0dXJlLXNhbXBsZQ";
    assert_token_removed(
        &format!("Authorization header carried {jwt} in the request."),
        jwt,
        &["Authorization header carried ", " in the request."],
    );
}

#[test]
fn jwt_requires_all_three_segments() {
    // Two segments is not a JWT. Structure is the whole basis for this arm, so a partial
    // structure must not match.
    assert_unchanged("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJleGFtcGxlIn0");
}

// ------------------------------------------------- cue-anchored (193 of 219 spans carry a cue)

#[test]
fn english_token_cue_is_tokenized_and_cue_survives() {
    // "token" is the dominant cue: 192 of 219 gold spans.
    assert_token_removed(
        "The API token: Rk9PQkFSLXNhbXBsZQ was issued yesterday.",
        "Rk9PQkFSLXNhbXBsZQ",
        &["The API token: ", " was issued yesterday."],
    );
}

#[test]
fn german_sicherheitstoken_cue_is_tokenized() {
    // "sicherheitstoken" appears before 37 gold spans. The German cues are literal alternatives
    // inside the pattern, NOT `[locale.cues.*]` bundle lookups, so they need no locale bundle.
    assert_token_removed(
        "Das Sicherheitstoken lautet Rk9PQkFSLXNhbXBsZQ und ist heute gültig.",
        "Rk9PQkFSLXNhbXBsZQ",
        &["Das Sicherheitstoken lautet ", " und ist heute gültig."],
    );
}

#[test]
fn bearer_cue_is_tokenized() {
    assert_token_removed(
        "Send Bearer Rk9PQkFSLXNhbXBsZQ with every call.",
        "Rk9PQkFSLXNhbXBsZQ",
        &["Send Bearer ", " with every call."],
    );
}

#[test]
fn cue_anchored_minimum_length_is_fourteen() {
    // The >=14 knee is measured: >=14 covers 151 spans at 1 non-gold holdout match, >=12 covers
    // 159 at 5, >=10 covers 163 at 7. A 13-character run must NOT match, or the knee is fiction.
    assert_unchanged("api key: Rk9PQkFSLXNh1");
    assert_token_removed(
        "api key: Rk9PQkFSLXNhbQ now active.",
        "Rk9PQkFSLXNhbQ",
        &["api key: ", " now active."],
    );
}

// ----------------------------------------------------------------------- hard negatives (A4)

#[test]
fn unanchored_high_entropy_runs_are_not_credentials() {
    // These are the shapes the committed A4 negative corpus is full of: an unanchored base64url
    // run of >=20 chars matches 448 times across 320 of its 1,024 documents, unanchored hex >=32
    // in 128 documents, and a bare UUID in 128 documents — all `code_log_syntax` and
    // `commerce_identifiers`. Every one of them must pass through untouched.
    assert_unchanged("commit 9f8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c landed");
    assert_unchanged("request id 3f2504e0-4f89-11d3-9a0c-0305e82c3301 completed");
    assert_unchanged("order reference AB12CD34EF56GH78IJ90 shipped");
}

#[test]
fn cue_without_high_entropy_run_is_not_a_credential() {
    // The cue alone must not tokenize ordinary following prose.
    assert_unchanged("The token expired and had to be reissued by support.");
}

// -------------------------------------------------------------------- locale-chain activation
//
// Todo #2403 shipped a leak because a locale-gated recognizer silently never fired on a chain
// pinned to Global. The benchmark builds its chain as `[<lang>-<region>, "global"]`
// (gaze_bench_score.py:87), and the corpus is en 117 / de 102, so roughly half the value of this
// change sits on the German side. These tests assert firing on every chain that matters instead
// of assuming `locales = ["global"]` behaves as documented.

#[test]
fn fires_under_every_benchmark_and_default_adopter_chain() {
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJleGFtcGxlIn0.c2lnbmF0dXJlLXNhbXBsZQ";
    for chain in [
        vec![LocaleTag::Global],
        vec![LocaleTag::parse("en-US").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("en-GB").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-DE").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-AT").expect("tag"), LocaleTag::Global],
        vec![LocaleTag::parse("de-CH").expect("tag"), LocaleTag::Global],
    ] {
        let aws = clean_under(&chain, "rotate AKIAIOSFODNN7EXAMPLE now");
        assert!(
            !aws.contains("AKIAIOSFODNN7EXAMPLE"),
            "AWS key survived on chain {chain:?}: {aws:?}"
        );

        let cued = clean_under(
            &chain,
            "Das Sicherheitstoken lautet Rk9PQkFSLXNhbXBsZQ heute.",
        );
        assert!(
            !cued.contains("Rk9PQkFSLXNhbXBsZQ"),
            "German cue-anchored credential survived on chain {chain:?}: {cued:?}"
        );

        let structured = clean_under(&chain, &format!("header {jwt} sent"));
        assert!(
            !structured.contains(jwt),
            "JWT survived on chain {chain:?}: {structured:?}"
        );
    }
}

// ------------------------------------------------------------------------------ restore path

#[test]
fn credentials_restore_exactly() {
    // Axis 2: reversibility. A credential that cannot be restored is a broken contract, not a fix.
    let pipeline = pipeline_for(&[LocaleTag::Global]);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let original = "rotate AKIAIOSFODNN7EXAMPLE and the api key: Rk9PQkFSLXNhbXBsZQ today";
    let (clean, _manifest, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(original.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let clean_text = match clean {
        CleanDocument::Text(text) => text,
        _ => panic!("expected text"),
    };
    assert!(!clean_text.contains("AKIAIOSFODNN7EXAMPLE"));
    let restored = pipeline
        .restore_strict_text(&session, &clean_text)
        .expect("restore");
    assert_eq!(restored, original, "manifest-first restore must round-trip");
}
