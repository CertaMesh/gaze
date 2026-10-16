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
    assert_value_with_core(&core, raw, value, class);
}

fn assert_value_with_core(core: &gaze_assembly::CorePipeline, raw: &str, value: &str, class: &str) {
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
fn argon_containment_wins_even_below_the_postal_rule_priority() {
    let pack = gaze_recognizers::embedded("secrets").unwrap();
    let weaker = pack.replace("base = 0.95\npriority = 100", "base = 0.95\npriority = 1");
    assert_ne!(pack, weaker);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    std::fs::write(&path, weaker).unwrap();
    let core = CorePipelineConfig::new()
        .with_rulepack_path(path)
        .with_locale(&[LocaleTag::parse("de-DE").unwrap()])
        .build()
        .unwrap();
    assert_value_with_core(&core, ARGON, ARGON, "password");
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

#[test]
fn every_supported_hash_format_has_exact_whole_value_candidates() {
    let hashes = vec![
        BCRYPT.to_string(),
        BCRYPT.replace("$2y$", "$2a$"),
        BCRYPT.replace("$2y$", "$2b$"),
        BCRYPT.replace("$2y$", "$2x$"),
        ARGON.into(),
        ARGON.replace("argon2id", "argon2i"),
        ARGON.replace("argon2id", "argon2d"),
        format!("$scrypt$ln=16,r=8,p=1$c29tZXNhbHQ${}", "a".repeat(43)),
        format!("$7${}{}", "a".repeat(33), "$".to_string() + &"b".repeat(43)),
        format!("$y$j9T$salt${}", "a".repeat(43)),
        format!("$5$salt${}", "a".repeat(43)),
        format!("$6$rounds=10000$salt${}", "b".repeat(86)),
        format!("$1$salt${}", "a".repeat(22)),
        format!("$P${}", "a".repeat(31)),
        format!("$H${}", "b".repeat(31)),
        DJANGO.into(),
        format!("pbkdf2_sha1$600000$somesalt${}=", "a".repeat(27)),
        format!("argon2{ARGON}"),
        format!("bcrypt_sha256${BCRYPT}"),
        format!("bcrypt${BCRYPT}"),
        format!("scrypt$16384$somesalt$8$5${}==", "a".repeat(86)),
    ];
    for hash in hashes {
        let adjacent = format!("'{hash}','{hash}'");
        assert_eq!(
            found("password.hash", &adjacent),
            vec![hash.clone(), hash.clone()]
        );
        for raw in [
            format!("x{hash}"),
            format!("{hash}$tail"),
            format!("{hash}_tail"),
        ] {
            assert!(
                found("password.hash", &raw).is_empty(),
                "partial format: {raw}"
            );
        }
        if !hash.contains("argon2") {
            assert!(
                found("password.hash", &hash[..hash.len() - 1]).is_empty(),
                "truncated: {hash}"
            );
        }
    }
    for raw in [
        format!("$5$salt${}", "a".repeat(44)),
        format!("$1$toolongsalt${}", "a".repeat(22)),
        format!("$P${}!", "a".repeat(30)),
        BCRYPT.replace("/", "!"),
        ARGON.replace("v=19", "v=1"),
    ] {
        assert!(found("password.hash", &raw).is_empty(), "malformed: {raw}");
    }
}

#[test]
fn code_literal_boundaries_and_raw_escapes_are_complete() {
    for (id, key) in [
        ("password.code", "password"),
        ("security_token.code", "api_token"),
    ] {
        for value in [r"literal\'quote", r"literal\u1234", &"x".repeat(1024)] {
            let raw = format!("['{key}' => '{value}', '{key}' => '{value}']");
            assert_eq!(found(id, &raw), vec![value, value]);
        }
        for raw in [
            format!("'not_{key}' => 'value',"),
            format!("'{key}' => Hash::make('secret'),"),
            format!("'{key}' => 'value' + expression"),
            format!("'{key}' => 'unterminated"),
            format!("'{key}' => \"unterminated"),
            format!("'{key}\" => 'value',"),
            format!("'{key}' ?? 'value',"),
            format!("x'{key}' => 'value',"),
            format!("'{key}' => 'value\nsecond line',"),
            format!("{key}_suffix: 'value',"),
        ] {
            assert!(found(id, &raw).is_empty(), "unowned value: {raw}");
        }
    }
}
