//! Regression fixtures for the `security_token.*` recognizers. They live in
//! the opt-in `secrets` bundle, so every pipeline here loads `core` plus `secrets` explicitly.
//!
//! The original positive fixtures encode structural shapes MEASURED in the Dataiku EN/DE
//! holdout, not a phrasing invented alongside the implementation. That discipline exists because
//! An earlier change shipped 962 green tests for URL cue phrasings that occur in 5 of 276 gold spans and
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
//! | cue context      | 193 of 219 (88.1%) have "token" or "secret" in prior 64 chars |
//!
//! The cue-context row is the inverse of URL's (1.8%) and is why a cue-anchored rule is correct
//! here and was wrong there. The shipping grammar is narrower: 126 gold spans have a supported
//! cue directly before the value, 113 meet the >=14 value shape, and all 193 broad cue-context
//! spans contain a delimiter other than `_` or `-`. Only measurement distinguishes the cases.
//!
//! Fixture values are synthetic. The AWS shapes use the key IDs published in AWS's own
//! documentation as non-functional examples; the JWT and cue-anchored payloads are hand-built
//! base64url strings. The JWT ownership regressions use only alice@example.invalid
//! and a synthetic signature, and make no holdout coverage claim.

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
        record_match_kinds: Default::default(),
        record_value_rejections: Default::default(),
    }
}

fn security_token_class() -> PiiClass {
    PiiClass::custom("security_token").expect("valid custom class")
}

/// Builds the core + secrets bundles under an explicit locale chain with locale-gated
/// auto-activation OFF.
///
/// `auto_activate_locale_gated = false` is the weakest configuration a default adopter can have.
/// The `security_token.anchored` recognizer is declared `safety_tier = "safe_default"` with
/// `locales = ["global"]`, so `recognizer_activates` admits it unconditionally. If it were
/// declared `locale_gated` instead, these tests would still pass under the benchmark's
/// configuration (all three cells set `auto_activate_locale_gated = true`) while a default adopter
/// got no credential protection at all. That is exactly the silent-inertness failure behind todo
/// #2403, so the distinction is made to fail loudly here.
fn pipeline_for(chain: &[LocaleTag]) -> Pipeline {
    pipeline_with_rules(
        chain,
        vec![
            RuleSpec::Class {
                class: security_token_class(),
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
    )
}

fn pipeline_with_rules(chain: &[LocaleTag], rules: Vec<RuleSpec>) -> Pipeline {
    let rulepacks = ["core", "secrets"].map(|bundle| {
        Rulepack::load(RulepackSource::Embedded(
            embedded(bundle).expect("bundled rulepack"),
        ))
        .expect("bundled rulepack loads")
    });
    let mut policy = gaze::Policy::default();
    policy.rules = rules;
    policy.rulepacks.bundled = vec!["core".to_string(), "secrets".to_string()];
    policy.rulepacks.auto_activate_locale_gated = false;
    let locale_chain = LocaleChain::merge_cli_policy_rulepack_default(None, None, Some(chain));
    gaze_assembly::build_pipeline(&policy, &empty_context(), &rulepacks, &locale_chain, None)
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
/// Whole-span coverage is the point. In the before-state the v0.14 full-stack cell overlapped 65 of 150
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

// The payload is {"sub":"alice@example.invalid"}; the signature is synthetic.
const PERSONAL_JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJhbGljZUBleGFtcGxlLmludmFsaWQifQ.c2lnbmF0dXJlLWJ5dGVzLWFyZS1zeW50aGV0aWMtMDAx";

fn assert_whole_values(text: &str, values: &[&str]) {
    let pipeline = pipeline_for(&[LocaleTag::Global]);
    let session = Session::new(Scope::Ephemeral).expect("session");
    let (clean, spans, _) = pipeline
        .clean_with_safety_net_detect_context(
            &session,
            RawDocument::Text(text.to_string()),
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
        )
        .expect("clean");
    let CleanDocument::Text(clean) = clean else {
        panic!("expected text");
    };
    assert_eq!(spans.len(), values.len(), "{text:?}: {spans:?}");
    let mut raw_at = 0;
    let mut clean_at = 0;
    for (span, value) in spans.iter().zip(values) {
        assert_eq!(&text[span.raw_span.clone()], *value, "{text:?}");
        let replacement = &clean[span.clean_span.clone()];
        assert!(replacement.contains(":Custom:security_token_"));
        assert_eq!(session.restore(replacement).as_deref(), Some(*value));
        assert_eq!(
            &clean[clean_at..span.clean_span.start],
            &text[raw_at..span.raw_span.start]
        );
        raw_at = span.raw_span.end;
        clean_at = span.clean_span.end;
    }
    assert_eq!(&clean[clean_at..], &text[raw_at..]);
    assert_eq!(
        pipeline.restore_strict_text(&session, &clean).unwrap(),
        text
    );
}

#[test]
fn bearer_jwt_owns_payload_and_signature_in_one_restorable_token() {
    assert_whole_values(
        &format!("Authorization: Bearer {PERSONAL_JWT}. Next request."),
        &[PERSONAL_JWT],
    );
}

#[test]
fn jwt_is_whole_after_every_supported_cue_spelling() {
    for cue in [
        "securitytoken",
        "security token",
        "security_token",
        "security-token",
        "sicherheitstoken",
        "zugangstoken",
        "zugangsschlussel",
        "zugangsschlüssel",
        "apikey",
        "api key",
        "api_key",
        "api-key",
        "accesstoken",
        "access token",
        "access_token",
        "access-token",
        "authtoken",
        "auth token",
        "auth_token",
        "auth-token",
        "authorizationtoken",
        "authorization token",
        "authorization_token",
        "authorization-token",
        "bearer",
        "token",
        "BEARER",
        "SICHERHEITSTOKEN",
    ] {
        assert_whole_values(&format!("{cue}: {PERSONAL_JWT}. Next."), &[PERSONAL_JWT]);
    }
}

#[test]
fn jwt_is_whole_with_supported_delimiters_and_across_lines() {
    for delimiter in [
        " ",
        "    ",
        "\t",
        "\t\t\t\t",
        ":",
        ": ",
        "=",
        " = ",
        "#",
        " # ",
        " is ",
        " ist ",
        " lautet ",
        " - ",
        " is: ",
        " lautet = ",
        "\n",
        "\r\n",
        ":\n",
        "     ",
    ] {
        // When whitespace cannot form a cue, the bare JWT arm still owns the value.
        assert_whole_values(
            &format!("token{delimiter}{PERSONAL_JWT}\n"),
            &[PERSONAL_JWT],
        );
    }
    let opaque = "synthetic_opaque_value";
    let text = format!("Bearer {PERSONAL_JWT}.\nBearer {PERSONAL_JWT};token: {opaque}.");
    assert_whole_values(&text, &[PERSONAL_JWT, PERSONAL_JWT, opaque]);
}

#[test]
fn bare_and_json_key_jwts_keep_whole_value_ownership() {
    assert_whole_values(PERSONAL_JWT, &[PERSONAL_JWT]);
    for key in ["token", "api_key", "access_token", "Authorization"] {
        assert_whole_values(&format!(r#"{{"{key}":"{PERSONAL_JWT}"}}"#), &[PERSONAL_JWT]);
    }
    let jwt_with_base64url_edges = format!("{PERSONAL_JWT}_-");
    assert_whole_values(
        &format!("Bearer {jwt_with_base64url_edges}. Next."),
        &[&jwt_with_base64url_edges],
    );
}

#[test]
fn cue_selection_keeps_the_two_public_capture_indices() {
    let rulepack = Rulepack::load(RulepackSource::Embedded(embedded("secrets").unwrap())).unwrap();
    let spec = rulepack
        .recognizers
        .iter()
        .find(|spec| spec.id == "security_token.anchored")
        .unwrap();
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = &spec.matcher
    else {
        panic!("literal regex");
    };
    assert_eq!(capture_groups.as_deref(), Some(&[1, 2][..]));
    let regex = regex::Regex::new(pattern).unwrap();
    assert_eq!(regex.captures_len(), 3);
    assert!(regex.capture_names().all(|name| name.is_none()));
    for (text, index, full_start) in [
        (PERSONAL_JWT.to_string(), 1, 0),
        (format!("Bearer {PERSONAL_JWT}"), 2, 0),
        (format!("token: {PERSONAL_JWT}"), 2, 0),
        (format!(r#"{{"token":"{PERSONAL_JWT}"}}"#), 1, 9),
    ] {
        let captures = regex.captures(&text).unwrap();
        assert_eq!(captures.get(0).unwrap().start(), full_start, "{text:?}");
        assert_eq!(
            captures.get(index).unwrap().as_str(),
            PERSONAL_JWT,
            "{text:?}"
        );
        assert!(captures.get(3 - index).is_none(), "{text:?}");
    }
}

#[test]
fn opaque_tokens_and_aws_keys_preserve_sentence_punctuation() {
    for value in [
        "synthetic_opaque_value",
        "AKIAIOSFODNN7EXAMPLE",
        "ASIAIOSFODNN7EXAMPLE",
    ] {
        for suffix in ["", ".", ". Next", ", next", ";next", "\n"] {
            assert_whole_values(&format!("token: {value}{suffix}"), &[value]);
        }
    }
    for value in ["AKIAIOSFODNN7EXAMPLE", "ASIAIOSFODNN7EXAMPLE"] {
        assert_whole_values(&format!("rotate {value}. Next"), &[value]);
    }
    assert_unchanged("akiaiosfodnn7example");
    assert_unchanged("ASIAIOSFODNN7EXAMPL");
    assert_unchanged("AKIAIOSFODNN7EXAMPLEA");
}

#[test]
fn malformed_dotted_values_never_emit_a_partial_prefix() {
    let header = "eyJhbGciOiJIUzI1NiJ9";
    let payload = "eyJzdWIiOiJhbGljZUBleGFtcGxlLmludmFsaWQifQ";
    for value in [
        format!("{header}.{payload}"),
        format!("{header}.{payload}.abcd"),
        format!("{header}.abcd.synthetic_signature"),
        format!("eyJabcd.{payload}.synthetic_signature"),
        format!("{header}..synthetic_signature"),
        format!("{PERSONAL_JWT}.extra"),
        format!("{PERSONAL_JWT}._"),
        format!("{PERSONAL_JWT}.."),
        format!("prefix-{PERSONAL_JWT}"),
        format!("prefix.{PERSONAL_JWT}"),
        "synthetic_opaque_value.more".to_string(),
        "AKIAIOSFODNN7EXAMPLE.extra".to_string(),
    ] {
        assert_unchanged(&value);
        assert_unchanged(&format!("token: {value}"));
    }
    let opaque = "AKIAIOSFODNN7EXAMPLE-extra";
    assert_unchanged(opaque);
    assert_whole_values(&format!("token: {opaque}"), &[opaque]);
    for input in [
        "token:\nsynthetic_opaque_value",
        "token:     synthetic_opaque_value",
        "token-synthetic_opaque_value",
        "token synthetic_val!",
    ] {
        assert_unchanged(input);
    }
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

#[test]
fn supported_real_delimiter_forms_are_tokenized() {
    let credential = "Rk9PQkFSLXNhbXBsZQ";
    for text in [
        format!("token: {credential}"),
        format!("token = {credential}"),
        format!("token {credential}"),
        format!("token ist {credential}"),
        format!("token lautet {credential}"),
        format!("token is {credential}"),
        format!("token - {credential}"),
    ] {
        assert_token_removed(&text, credential, &[]);
    }
}

// ---------------------------------------------------------- identifier-splitting hard negatives

#[test]
fn tokenization_helper_registry_is_not_split() {
    assert_unchanged("Case A: the tokenization_helper_registry module was refactored.");
}

#[test]
fn bearer_authentication_handler_impl_is_not_split() {
    assert_unchanged("Case B: see bearer_authentication_handler_impl for details.");
}

#[test]
fn api_key_rotation_schedule_v2_is_not_split() {
    assert_unchanged("Case C: api_key_rotation_schedule_v2 is documented elsewhere.");
}

#[test]
fn access_token_refresh_coordinator_is_not_split() {
    assert_unchanged("Case D: access_token_refresh_coordinator handles retries.");
}

#[test]
fn authorization_token_provider_factory_is_not_split() {
    assert_unchanged("Case G: authorization_token_provider_factory implements the trait.");
}

#[test]
fn tokenizer_vocabulary_builder_test_is_not_split() {
    assert_unchanged("Case J: tokenizer_vocabulary_builder_test passed.");
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
// An earlier change shipped a leak because a locale-gated recognizer silently never fired on a chain
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

#[test]
fn jwt_manifest_export_import_restores_whole_value_and_following_email() {
    let pipeline = pipeline_with_rules(
        &[LocaleTag::Global],
        vec![
            RuleSpec::Class {
                class: security_token_class(),
                action: Action::Tokenize,
            },
            RuleSpec::Class {
                class: PiiClass::Email,
                action: Action::Tokenize,
            },
            RuleSpec::Default {
                action: Action::Preserve,
            },
        ],
    );
    for text in [
        format!("Authorization: Bearer {PERSONAL_JWT};alice@example.invalid. Next."),
        format!(r#"{{"token":"{PERSONAL_JWT}","email":"alice@example.invalid"}}"#),
    ] {
        let session =
            Session::new(Scope::Conversation("synthetic-jwt-roundtrip".to_string())).unwrap();
        let (clean, spans, _, trace) = pipeline
            .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                &session,
                &text,
                &[LocaleTag::Global],
                &DictionaryBundle::default(),
                gaze::SafetyNetPolicy::default(),
            )
            .unwrap();
        let CleanDocument::Text(clean) = clean else {
            panic!("text")
        };
        assert_eq!(spans.len(), 2);
        assert_eq!(trace.len(), 2);
        let values = [PERSONAL_JWT, "alice@example.invalid"];
        let classes = [security_token_class(), PiiClass::Email];
        let sources = ["security_token.anchored", "email.global"];
        let mut expected_clean = text.clone();
        for (((span, value), class), source) in spans.iter().zip(values).zip(classes).zip(sources) {
            assert_eq!(&text[span.raw_span.clone()], value);
            assert_eq!(span.class, class);
            assert!(span.origin.is_whole());
            let token = &clean[span.clean_span.clone()];
            assert_eq!(session.restore_strict(token).unwrap(), value);
            expected_clean = expected_clean.replace(value, token);
            let evidence = trace
                .iter()
                .find(|item| {
                    item.raw_start() == span.raw_span.start && item.raw_end() == span.raw_span.end
                })
                .unwrap();
            assert_eq!(evidence.class(), &class);
            assert_eq!(evidence.source_ids(), &[source.to_string()]);
        }
        assert_eq!(clean, expected_clean, "every non-gold byte must survive");
        let bytes = session.export().unwrap().into_bytes();
        let imported = Session::import(gaze::SensitiveSnapshot::from(bytes)).unwrap();
        assert_eq!(
            pipeline.restore_strict_text(&imported, &clean).unwrap(),
            text
        );
        for (span, value) in spans.iter().zip(values) {
            assert_eq!(
                imported
                    .restore_strict(&clean[span.clean_span.clone()])
                    .unwrap(),
                value
            );
        }
    }
}

#[test]
fn jwt_manifest_residual_protects_whole_jwt_inside_preserved_password_field() {
    let pipeline = pipeline_for(&[LocaleTag::Global]);
    let text = format!("password: \"Bearer {PERSONAL_JWT}\"");
    let session = Session::new(Scope::Conversation("synthetic-jwt-residual".to_string())).unwrap();
    let (clean, spans, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            &text,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            gaze::SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(spans.len(), 1);
    let span = &spans[0];
    assert_eq!(&text[span.raw_span.clone()], PERSONAL_JWT);
    assert_eq!(span.class, security_token_class());
    assert!(span.origin.is_residual_fragment());
    let token = &clean[span.clean_span.clone()];
    assert_eq!(clean, format!("password: \"Bearer {token}\""));
    assert_eq!(trace.len(), 1);
    assert!(trace[0]
        .source_ids()
        .iter()
        .any(|source| source == "security_token.anchored"));
    assert_eq!(trace[0].class(), &security_token_class());
    let imported = Session::import(session.export().unwrap()).unwrap();
    assert_eq!(imported.restore_strict(token).unwrap(), PERSONAL_JWT);
    assert_eq!(
        pipeline.restore_strict_text(&imported, &clean).unwrap(),
        text
    );
}

#[test]
fn jwt_manifest_preserve_policy_leaves_entire_jwt_and_email_untouched() {
    let pipeline = pipeline_with_rules(
        &[LocaleTag::Global],
        vec![RuleSpec::Default {
            action: Action::Preserve,
        }],
    );
    let text = format!("Authorization: Bearer {PERSONAL_JWT};alice@example.invalid.");
    let session = Session::new(Scope::Conversation("synthetic-jwt-preserve".to_string())).unwrap();
    let (clean, spans, _, trace) = pipeline
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            &text,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            gaze::SafetyNetPolicy::default(),
        )
        .unwrap();
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(clean, text);
    assert!(spans.is_empty());
    assert!(trace.is_empty());
    assert!(session.snapshot_entries().is_empty());
    let imported = Session::import(session.export().unwrap()).unwrap();
    assert_eq!(
        pipeline.restore_strict_text(&imported, &text).unwrap(),
        text
    );
}
