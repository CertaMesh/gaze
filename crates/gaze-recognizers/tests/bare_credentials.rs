//! Nonfunctional credential shapes assembled at runtime; no live-service checks.
// drift-ack: secrets 0.2.0 adds bare credentials; the existing drift corpus keeps its detections.
use gaze::{CleanDocument, DictionaryBundle, LocaleTag, PiiClass, SafetyNetPolicy, Scope, Session};
use gaze_assembly::CorePipelineConfig;

fn core() -> gaze_assembly::CorePipeline {
    CorePipelineConfig::new()
        .with_bundled_rulepack("secrets")
        .build()
        .unwrap()
}

fn assert_whole(core: &gaze_assembly::CorePipeline, raw: &str, values: &[&str]) {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let (clean, spans, _, _) = core
        .pipeline()
        .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
            &session,
            raw,
            &[LocaleTag::Global],
            &DictionaryBundle::default(),
            SafetyNetPolicy::default(),
        )
        .unwrap();
    let observed: Vec<_> = spans
        .iter()
        .map(|span| {
            assert_eq!(span.class, PiiClass::Custom("security_token".into()));
            &raw[span.raw_span.clone()]
        })
        .collect();
    assert_eq!(observed, values, "whole credential ownership");
    let CleanDocument::Text(clean) = clean else {
        panic!("text")
    };
    assert_eq!(
        core.pipeline()
            .restore_strict_text(&session, &clean)
            .unwrap(),
        raw
    );
    for value in values {
        assert!(!clean.contains(value));
    }
}

fn prefixed_values() -> Vec<String> {
    let mut values = Vec::new();
    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        for n in [36, 251] {
            values.push(format!("{prefix}{}", "A".repeat(n)));
        }
    }
    values.push(format!("github_pat_{}", "A_".repeat(41)));
    values.push(format!(
        "ghs_000000_eyJ{}.{}.{}",
        "A".repeat(80),
        "B".repeat(300),
        "C".repeat(100)
    ));
    values.push(format!("sk-{}", "A".repeat(48)));
    values.push(format!("sk-proj-{}", "A_-".repeat(52)));
    values.push(format!("sk-ant-api03-{}AA", "A_-".repeat(31)));
    for prefix in ["xoxa-", "xoxb-", "xoxp-", "xoxr-", "xoxs-"] {
        values.push(format!(
            "{prefix}{}-{}-{}",
            "0".repeat(12),
            "0".repeat(12),
            "A".repeat(32)
        ));
    }
    for prefix in ["sk_live_", "rk_live_", "sk_test_", "rk_test_"] {
        for n in [24, 99] {
            values.push(format!("{prefix}{}", "A".repeat(n)));
        }
    }
    values
}

fn pem(label: &str, newline: &str) -> String {
    // Base64 decodes to synthetic text, never private-key material.
    format!("-----BEGIN {label}-----{newline}bm90LWEta2V5{newline}c3ludGhldGlj{newline}-----END {label}-----")
}

#[test]
fn bare_vendor_prefixes_are_whole_restorable_credentials() {
    let core = core();
    for value in prefixed_values() {
        for raw in [
            value.clone(),
            format!("before ({value}), after"),
            format!(r#"{{"value":"{value}"}}"#),
        ] {
            assert_whole(&core, &raw, &[&value]);
        }
    }
}

#[test]
fn private_pem_blocks_include_headers_body_and_footer() {
    let core = core();
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
    ] {
        for newline in ["\n", "\r\n", r"\n", r"\r\n"] {
            let value = pem(label, newline);
            assert_whole(&core, &format!("before {value} after"), &[&value]);
            assert_whole(&core, &format!(r#"{{"value":"{value}"}}"#), &[&value]);
        }
    }
}

#[test]
fn adjacent_bare_credentials_do_not_consume_each_others_boundaries() {
    let core = core();
    let first = &prefixed_values()[0];
    let second = &prefixed_values()[2];
    assert_whole(&core, &format!("{first},{second}"), &[first, second]);
    let first = pem("RSA PRIVATE KEY", "\n");
    let second = pem("EC PRIVATE KEY", "\n");
    assert_whole(&core, &format!("{first}\n{second}"), &[&first, &second]);
}

#[test]
fn prose_and_public_material_remain_untouched() {
    let core = core();
    let mut negatives = vec![
        "sk-learn task- ghp in prose".to_string(),
        "a".repeat(64),
        "c3ludGhldGlj".repeat(8),
        "00000000-0000-4000-8000-000000000000".into(),
        pem("PUBLIC KEY", "\n"),
        pem("CERTIFICATE", "\n"),
    ];
    for value in prefixed_values() {
        negatives.extend([format!("word{value}"), format!("task-{value}")]);
    }
    for (prefix, length) in [
        ("ghp_", 35),
        ("ghp_", 252),
        ("github_pat_", 81),
        ("github_pat_", 246),
        ("sk-", 31),
        ("sk-", 257),
        ("sk-proj-", 31),
        ("sk-proj-", 257),
        ("sk-ant-", 31),
        ("sk-ant-", 257),
        ("xoxb-", 9),
        ("xoxb-", 257),
        ("sk_live_", 23),
        ("sk_live_", 257),
    ] {
        negatives.push(format!("{prefix}{}", "A".repeat(length)));
    }
    for raw in negatives {
        assert_whole(&core, &raw, &[]);
    }
}

#[test]
fn legacy_encrypted_rsa_headers_and_escaped_base64_stay_in_one_span() {
    let core = core();
    for newline in ["\n", "\r\n", r"\n", r"\r\n"] {
        let value = format!("-----BEGIN RSA PRIVATE KEY-----{newline}Proc-Type: 4,ENCRYPTED{newline}DEK-Info: AES-256-CBC,00000000000000000000000000000000{newline}{newline}bm90L2tleQ=={newline}-----END RSA PRIVATE KEY-----");
        assert_whole(&core, &value, &[&value]);
    }
    let value = pem("PRIVATE KEY", r"\n").replace("bm90LWEta2V5", r"bm90\/LWEta2V5");
    assert_whole(&core, &format!(r#"{{"value":"{value}"}}"#), &[&value]);
}
