//! Synthetic account-dump regressions for GitHub #824. Hashes need no field cue.
use gaze::{DictionaryBundle, LocaleTag, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

const BCRYPT: &str = "$2y$10$92IXUNpkjO0rOQ5byMi.Ye4oKoEa3Ro9llC/.og/at2.uheWG/igi";
const ARGON: &str = "$argon2id$v=19$m=65536,t=4,p=1$c29tZXNhbHQ$RdescudvJCsgt3ub+b+dWRWJTmaaJObG";
const DJANGO: &str = "pbkdf2_sha256$600000$somesalt$X1bTpnTbg0uO3k7v5F0jQm2c3c0kKk1v5Xr0bS2kQ1k=";

fn assert_value(raw: &str, value: &str, class: &str) {
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .with_locale(&[LocaleTag::parse("de-DE").unwrap()])
        .build()
        .unwrap();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, _) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global, LocaleTag::parse("de-DE").unwrap()],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    assert_eq!(
        spans.len(),
        1,
        "expected one complete credential: {raw:?}; {spans:?}"
    );
    assert_eq!(&raw[spans[0].raw_span.clone()], value, "split credential");
    assert_eq!(spans[0].class, gaze::PiiClass::Custom(class.into()));
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert!(!clean.contains(value));
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .unwrap(),
        raw
    );
}

#[test]
fn raw_password_hashes_in_seeders_and_sql_are_one_restorable_span() {
    for hash in [BCRYPT, DJANGO] {
        for raw in [
            format!("['digest' => '{hash}'],"),
            format!("INSERT INTO users (password) VALUES ('{hash}');"),
            hash.into(),
        ] {
            assert_value(&raw, hash, "password");
        }
    }
}

#[test]
fn whole_argon_hash_contains_the_postal_code_candidate() {
    assert_value(&format!("['argon' => '{ARGON}'],"), ARGON, "password");
}

#[test]
fn code_syntax_credentials_preserve_keys_quotes_and_punctuation() {
    for (raw, value, class) in [
        (
            "['api_token' => 'b3f1c9e27a4d4f0e9c1b8a7d6e5f4a3b2c1d0e9f8a7b6c5d'],",
            "b3f1c9e27a4d4f0e9c1b8a7d6e5f4a3b2c1d0e9f8a7b6c5d",
            "security_token",
        ),
        (
            "['remember_token' => 'Zk3uQm9xW2pL7vB4nT8rY1cJ6hD5sF0a'],",
            "Zk3uQm9xW2pL7vB4nT8rY1cJ6hD5sF0a",
            "security_token",
        ),
        (
            "['password' => 'synthetic!value'],",
            "synthetic!value",
            "password",
        ),
        (
            "{\"password\": \"synthetic!value\",}",
            "synthetic!value",
            "password",
        ),
        (
            "password: \"synthetic!value\",",
            "synthetic!value",
            "password",
        ),
    ] {
        assert_value(raw, value, class);
    }
}

fn detector(id: &str) -> gaze_recognizers::RegexDetector {
    let spec = gaze::Rulepack::parse_bundled(gaze_recognizers::embedded("secrets").unwrap())
        .unwrap()
        .recognizers
        .into_iter()
        .find(|spec| spec.id == id)
        .expect("recognizer");
    let gaze::RawMatch::Regex {
        pattern: Some(pattern),
        capture_groups,
        ..
    } = spec.matcher
    else {
        panic!("regex")
    };
    gaze_recognizers::RegexDetector::with_rulepack_fields(
        &pattern,
        spec.class,
        &spec.id,
        spec.locales,
        spec.scoring.base,
        spec.scoring.priority,
        "counter",
        capture_groups,
        vec![],
        None,
        None,
    )
    .unwrap()
}

fn found(id: &str, raw: &str) -> Vec<String> {
    use gaze::Recognizer;
    detector(id)
        .detect(
            raw,
            &gaze::DetectContext::new(&[LocaleTag::Global], &DictionaryBundle::default()),
        )
        .unwrap()
        .into_iter()
        .map(|hit| raw[hit.span].to_owned())
        .collect()
}

#[test]
fn hash_shapes_require_complete_components_and_boundaries() {
    assert_eq!(found("password.hash", BCRYPT), vec![BCRYPT]);
    for raw in [
        "$2y$10$short",
        "$argon2id$v=19$m=65536",
        "It costs $2 or $10 per month.",
        &format!("{BCRYPT}A"),
        &format!("x{BCRYPT}"),
        &format!("{BCRYPT}$tail"),
        &BCRYPT[..BCRYPT.len() - 1],
    ] {
        assert!(
            found("password.hash", raw).is_empty(),
            "partial hash: {raw}"
        );
    }
}

#[test]
fn expressions_and_unrelated_code_keys_are_not_credential_values() {
    for raw in [
        "'password' => Hash::make('secret'),",
        "\"password\": make(\"secret\"),",
        "'not_password' => 'synthetic!value',",
        "\"password\": \"unterminated",
        "\"password\": \"value\" + suffix",
        "'password\" => 'value',",
    ] {
        assert!(
            found("password.code", raw).is_empty(),
            "code expression: {raw}"
        );
    }
}
