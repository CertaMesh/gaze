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
            format!("digest={hash}"),
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
        assert_eq!(
            found("password.hash", &format!("encoded={hash}")),
            vec![hash.clone()]
        );
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
    assert!(found("password.code", "password: \"value\"").is_empty());
    assert_eq!(
        found("password.code", "password: \"value\","),
        vec!["value"]
    );
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

#[test]
fn php_array_credentials_cover_compact_first_final_and_nested_elements() {
    for (key, class) in [("password", "password"), ("api_token", "security_token")] {
        for raw in [
            format!("array('{key}' => 'synthetic!value');"),
            format!("array('{key}'=>'synthetic!value','enabled'=>true);"),
            format!("array('enabled'=>true,'{key}'=>'synthetic!value');"),
            format!("array('outer'=>array('{key}'=>'synthetic!value'));"),
            format!("array( {key} => 'synthetic!value' );"),
        ] {
            assert_value(&raw, "synthetic!value", class);
        }
    }
}

#[test]
fn producer_hashes_keep_alternative_salts_and_exact_digest_widths() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/password_hash_producers.json")).unwrap();
    for row in fixtures.as_array().unwrap() {
        let hash = row["encoded"].as_str().unwrap();
        assert_value(&format!("digest={hash}"), hash, "password");
        assert_eq!(found("password.hash", hash), vec![hash]);
        for malformed in [hash[..hash.len() - 1].to_string(), format!("{hash}A")] {
            assert!(found("password.hash", &malformed).is_empty(), "{malformed}");
        }
        let (parameters, checksum) = hash.rsplit_once('$').unwrap();
        assert!(found(
            "password.hash",
            &format!("{parameters}${}!", &checksum[..checksum.len() - 1])
        )
        .is_empty());
    }
    for malformed in [
        format!("$5$bad:salt${}", "a".repeat(43)),
        format!("$6$bad\nsalt${}", "a".repeat(86)),
        format!("$1$bad\0salt${}", "a".repeat(22)),
        format!("pbkdf2_sha256$1000$${}=", "a".repeat(43)),
        format!("pbkdf2_sha1$1000$bad$salt${}=", "a".repeat(27)),
        format!("scrypt$1024$$8$1${}==", "a".repeat(86)),
    ] {
        assert!(
            found("password.hash", &malformed).is_empty(),
            "{malformed:?}"
        );
    }
}

#[test]
fn generic_token_metadata_does_not_establish_a_code_credential() {
    for raw in [
        r#"{"token":"Hello","logprob":-0.1}"#,
        r#"{"tokenizer":{"token":"indivisible"}}"#,
        r#"{"type":"object","example":{"token":"string"}}"#,
        "token: 'Hello',",
    ] {
        assert!(found("security_token.code", raw).is_empty(), "{raw}");
    }
}

#[test]
fn ambiguous_password_metadata_remains_fail_closed_with_real_passphrases() {
    for value in [
        "synthetic long pass phrase",
        "synthetic|secret",
        "string",
        "********",
        "required|string|min:8|confirmed",
        "The provided password is incorrect.",
    ] {
        assert_value(&format!("array('password'=>'{value}');"), value, "password");
    }
}

#[derive(Clone, Default)]
struct HashAudit(std::sync::Arc<std::sync::Mutex<Vec<gaze::RedactionEntry>>>);

impl gaze::RedactionLogger for HashAudit {
    fn log(&self, entry: &gaze::RedactionEntry) -> Result<(), gaze::RedactionLogError> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(())
    }
}

fn assert_escaped_hash(raw: &str, value: &str) {
    let audit = HashAudit::default();
    let core = CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .build()
        .unwrap()
        .into_pipeline()
        .with_redaction_logger(audit.clone());
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, _) = core
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    assert_eq!(spans.len(), 1, "one complete hash: {raw:?}; {spans:?}");
    assert_eq!(&raw[spans[0].raw_span.clone()], value, "split hash");
    let gaze::CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(core.restore_strict_text(&session, &clean).unwrap(), raw);
    assert!(!clean.contains(value));
    assert!(audit
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|entry| !entry.conflict_loser
            && entry.recognizer_id.as_deref() == Some("password.hash")));
}

fn source_escape_fixtures(family: &str) {
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/password_hash_source_escapes.json")).unwrap();
    for row in rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["family"] == family)
    {
        let raw = row["input"].as_str().unwrap();
        let value = row["value"].as_str().unwrap();
        assert_eq!(found("password.hash", raw), vec![value], "{raw}");
        assert_escaped_hash(raw, value);
    }
}

#[test]
fn escaped_crypt_salts_count_decoded_units_at_producer_limits() {
    source_escape_fixtures("crypt");
    for (kind, width, limit) in [("1", 22, 8), ("5", 43, 16), ("6", 86, 16)] {
        for quote in ["'", "''", r"\'"] {
            let salt = format!("{}{}", "a".repeat(limit - 1), quote);
            let hash = format!("${kind}${salt}${}", "a".repeat(width));
            assert_escaped_hash(&format!("INSERT INTO accounts VALUES ('{hash}');"), &hash);
            let invalid = format!("${kind}$a{salt}${}", "a".repeat(width));
            assert!(
                found("password.hash", &invalid).is_empty(),
                "overlong salt: {invalid}"
            );
            for size in [width - 1, width + 1] {
                let invalid = format!("${kind}${salt}${}", "a".repeat(size));
                assert!(
                    found("password.hash", &invalid).is_empty(),
                    "checksum: {invalid}"
                );
            }
        }
    }
}

#[test]
fn escaped_json_slashes_are_complete_hash_alphabet_units() {
    source_escape_fixtures("json");
    for hash in [
        format!("$2b$12${}", "/".repeat(53)),
        "$argon2id$v=19$m=8192,t=1,p=1$////$////".into(),
        format!("$scrypt$ln=16,r=8,p=1$////${}", "/".repeat(43)),
        format!("$7${}{}", "/".repeat(33), "$".to_string() + &"/".repeat(43)),
        format!("$y$j9T$////${}", "/".repeat(43)),
        format!("$P${}", "/".repeat(31)),
        format!("pbkdf2_sha256$1000$salt${}=", "/".repeat(43)),
        format!("pbkdf2_sha1$1000$salt${}=", "/".repeat(27)),
        format!("scrypt$1024$salt$8$1${}==", "/".repeat(86)),
    ] {
        let encoded = hash.replace('/', r"\/");
        assert_escaped_hash(&format!(r#"{{"stored_digest":"{encoded}"}}"#), &encoded);
        for suffix in [r"\", r"\q", r"\u002", r"\u00xz", "_tail"] {
            assert!(found("password.hash", &format!("{encoded}{suffix}")).is_empty());
        }
    }
    for count in [52, 54] {
        let invalid = format!("$2b$12${}", r"\/".repeat(count));
        assert!(
            found("password.hash", &invalid).is_empty(),
            "bcrypt width: {invalid}"
        );
    }
    for invalid in [
        r"$argon2id$v=19$m=8192,t=1,p=1$aaaa$aaa\/",
        r"$argon2id$v=19$m=8192,t=1,p=1$aaa\/$aaaa",
    ] {
        // Three letters plus one escaped slash is valid, but removing a decoded
        // letter must never leave a complete component or a truncated match.
        let broken = invalid.replace("aaa", "aa");
        assert!(found("password.hash", &broken).is_empty());
    }
}

#[test]
fn json_separators_after_complete_hashes_preserve_source_spans() {
    for hash in [
        "$2y$10$92IXUNpkjO0rOQ5byMi.Ye4oKoEa3Ro9llC/.og/at2.uheWG/igi".to_string(),
        "$argon2id$v=19$m=8192,t=1,p=1$aaaa$bbbb".to_string(),
        format!("$6$salt${}", "a".repeat(86)),
    ] {
        for value in [hash.clone(), hash.replace('/', r"\/")] {
            for separator in [
                r"\n", r"\t", r"\r", r#"\""#, r"\\", r"\u000a", r"\u002f", r"\uABCD", r"\b", r"\f",
            ] {
                let raw = format!(r#"{{"stdout":"{value}{separator}"}}"#);
                assert_escaped_hash(&raw, &value);
            }
            for tail in [r"\", r"\q", r"\u", r"\u000", r"\u00xz", "_tail"] {
                assert!(found("password.hash", &format!("{value}{tail}")).is_empty());
            }
        }
    }
    for width in [52, 54] {
        let hash = format!("$2b$12${}", "a".repeat(width));
        for separator in [r"\n", r"\t", r"\r", r#"\""#, r"\\", r"\u000a"] {
            assert!(found("password.hash", &format!("{hash}{separator}")).is_empty());
        }
    }
}
