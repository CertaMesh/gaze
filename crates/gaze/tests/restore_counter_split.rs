#![cfg(feature = "bundled-recognizers")]

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, DictionaryBundle, PiiClass, Pipeline,
    RawDocument, RestoreDecision, RestorePolicy, RestoreTelemetry, SafetyNetPolicy, Scope, Session,
};
use gaze_recognizers::RegexDetector;

fn pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

fn clean(pipeline: &Pipeline, session: &Session, raw: &str) -> String {
    let (CleanDocument::Text(text), _, _) = pipeline
        .clean_with_safety_net(session, RawDocument::Text(raw.into()), &[])
        .unwrap()
    else {
        panic!("text document")
    };
    text
}

fn assert_success(session: &Session, input: &str, expected: &str, bypass: u64) {
    let (restored, telemetry) = pipeline().restore_with_telemetry(session, input).unwrap();
    assert_eq!(restored.text, expected);
    assert_eq!(telemetry.restore_decision, RestoreDecision::Success);
    assert_eq!(telemetry.unknown_token_count, 0);
    assert_eq!(telemetry.manifest_bypass_count, bypass);
    assert_eq!(session.restore_strict_text(input).unwrap(), expected);
    assert_eq!(
        session.restore_strict_text_with_events(input).unwrap().0,
        expected
    );
}

#[test]
fn strict_roundtrip_accepts_original_bare_literal_en() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Keep FOO_12 and ORDER_12345; email alice@example.invalid.";
    assert_success(&session, &clean(&pipeline(), &session, raw), raw, 2);
}

#[test]
fn strict_roundtrip_accepts_original_bare_literal_de() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "Grüße: Kunde_7 bleibt; Kontakt alice@example.invalid.";
    assert_success(&session, &clean(&pipeline(), &session, raw), raw, 1);
}

#[test]
fn strict_restore_accepts_lowercase_log_identifiers() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    assert_success(
        &session,
        "run_1 finished, see log_2",
        "run_1 finished, see log_2",
        2,
    );
}

#[test]
fn strict_roundtrip_accepts_authorized_value_containing_shape() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for raw in [
        "record ORDER_12345",
        "record <deadbeef:Email_999>",
        "record <Email_1>",
        "record location_7",
    ] {
        let token = session.tokenize(&PiiClass::custom("record"), raw).unwrap();
        assert_success(&session, &format!("π/{token}."), &format!("π/{raw}."), 0);
    }
}

#[test]
fn strict_restore_rejects_unmapped_canonical_legacy_formats() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for text in [
        "<Email_1>",
        "<Name_99>",
        "<Foo_5>",
        "<foo_1>",
        "<Custom:class_alpha_1>",
        "custom:class_alpha_1",
        "Email_7",
        "email1@example.test",
        "email1@gaze-fake.invalid",
        "location_7",
        "name_1",
        "organization_1",
        "email_1",
    ] {
        assert_unknown(&session, text);
    }
}

#[test]
fn appended_canonical_placeholders_fail_after_valid_clean() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let p = pipeline();
    let clean = clean(&p, &session, "alice@example.invalid");
    for unknown in [
        "<Email_1>".to_string(),
        format!("<{}:Email_999>", session.session_hex()),
        "<deadbeef:Email_999>".to_string(),
    ] {
        let input = format!("{clean} {unknown}");
        let (_, telemetry) = p.restore_with_telemetry(&session, &input).unwrap();
        assert_eq!(telemetry.restore_decision, RestoreDecision::Failed);
        assert_eq!(telemetry.unknown_token_count, 1);
        assert_eq!(telemetry.manifest_bypass_count, 0);
        assert!(session.restore_strict_text(&input).is_err());
    }
}

fn assert_unknown(session: &Session, text: &str) {
    for (policy, decision) in [
        (RestorePolicy::Strict, RestoreDecision::Failed),
        (RestorePolicy::Lenient, RestoreDecision::Partial),
    ] {
        let (restored, telemetry) = pipeline()
            .restore_with_policy_telemetry(session, text, policy)
            .unwrap();
        assert_eq!(restored.text, text);
        assert_eq!(telemetry.restore_decision, decision);
        assert_eq!(telemetry.unknown_token_count, 1);
        assert_eq!(telemetry.manifest_bypass_count, 0);
    }
    assert!(session.restore_strict_text(text).is_err());
    assert!(session.restore_strict_text_with_events(text).is_err());
}

#[test]
fn strict_restore_rejects_missing_own_mapping() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    assert_unknown(&session, &format!("<{}:Email_999>", session.session_hex()));
}

#[test]
fn strict_restore_rejects_foreign_session_token_in_every_format() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let foreign = Session::new(Scope::Ephemeral).unwrap();
    for class in [PiiClass::Email, PiiClass::Name, PiiClass::custom("record")] {
        assert_unknown(
            &session,
            &foreign.tokenize(&class, "synthetic value").unwrap(),
        );
        assert_unknown(
            &session,
            &foreign
                .format_preserving_fake(&class, "synthetic value")
                .unwrap(),
        );
    }
}

#[test]
fn strict_restore_rejects_new_same_prefix_but_audits_generic_shapes() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    assert_unknown(
        &session,
        &format!("{}:custom:record_987654", session.session_hex()),
    );
    assert_success(&session, "FOO_13", "FOO_13", 1);
}

#[test]
fn lenient_restore_counters_are_independent_and_deterministic() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let input = format!("FOO_12 run_1 <{}:Email_999>", session.session_hex());
    let p = pipeline();
    let (_, a) = p
        .restore_with_policy_telemetry(&session, &input, RestorePolicy::Lenient)
        .unwrap();
    let (_, b) = p
        .restore_with_policy_telemetry(&session, &input, RestorePolicy::Lenient)
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(a.unknown_token_count, 1);
    assert_eq!(a.manifest_bypass_count, 2);
    assert_eq!(a.restore_decision, RestoreDecision::Partial);
    assert_eq!(a.fresh_pii_detected_count, 0);
    assert_eq!(
        a.phase_execution_mask & gaze_types::RESTORE_PHASE_FRESH_PII_SCAN,
        0
    );
    let (_, literal) = p
        .restore_with_policy_telemetry(&session, "FOO_12", RestorePolicy::Lenient)
        .unwrap();
    assert_eq!(literal.restore_decision, RestoreDecision::Success);
}

#[test]
fn authorized_ranges_do_not_exempt_adjacent_or_boundary_composed_unknowns() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let raw = "<deadbeef:Email_999>";
    let token = session.tokenize(&PiiClass::Name, raw).unwrap();
    let input = format!("{token} {raw}");
    let (_, telemetry) = pipeline().restore_with_telemetry(&session, &input).unwrap();
    assert_eq!(telemetry.unknown_token_count, 1);
    assert_eq!(telemetry.restore_decision, RestoreDecision::Failed);
    assert!(session.restore_strict_text(&input).is_err());

    let prefix = session.tokenize(&PiiClass::Location, "deadbeef:").unwrap();
    let seam = format!("{prefix}email_999");
    let (_, telemetry) = pipeline().restore_with_telemetry(&session, &seam).unwrap();
    assert_eq!(telemetry.unknown_token_count, 1);
    assert_eq!(telemetry.restore_decision, RestoreDecision::Failed);
    assert!(session.restore_strict_text(&seam).is_err());
}

#[test]
fn strict_session_malformed_nested_and_atomic_failure_contracts_remain() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    let token = session
        .tokenize(&PiiClass::Email, "alice@example.invalid")
        .unwrap();
    for input in [
        "<Email_>".to_string(),
        "<deadbeef:Email_1 suffix".to_string(),
        "<deadbeef:Custom:record_1 suffix".to_string(),
        format!("<{token}>"),
        format!("{token} <deadbeef:Email_999>"),
    ] {
        assert!(session.restore_strict_text(&input).is_err());
        assert!(session.restore_strict_text_with_events(&input).is_err());
    }
}

#[test]
fn incomplete_prefixed_wrappers_fail_shared_assessment() {
    let session = Session::new(Scope::Ephemeral).unwrap();
    for text in [
        "<deadbeef:Email_1 suffix",
        "<deadbeef:Custom:class_alpha_1 suffix",
    ] {
        assert_unknown(&session, text);
        let token = session
            .tokenize(&PiiClass::custom("class_alpha"), text)
            .unwrap();
        assert_success(&session, &token, text, 0);
        let (_, telemetry) = pipeline()
            .restore_with_telemetry(&session, &format!("{token} {text}"))
            .unwrap();
        assert_eq!(telemetry.unknown_token_count, 1);
        assert_eq!(telemetry.manifest_bypass_count, 0);
    }
}

#[test]
fn strict_literal_restore_survives_existing_snapshot_format() {
    let session = Session::new(Scope::Conversation("synthetic-restore".into())).unwrap();
    let raw = "Kunde_7 alice@example.invalid";
    let clean = clean(&pipeline(), &session, raw);
    let blob = session.export().unwrap();
    let imported = Session::import(blob).unwrap();
    assert_success(&imported, &clean, raw, 1);
}

#[test]
fn restore_telemetry_serde_legacy_defaults_and_decision_spellings() {
    let legacy = serde_json::json!({
        "unknown_token_count": 0, "manifest_bypass_count": 1,
        "fresh_pii_detected_count": 0, "restore_policy": "strict",
        "restore_decision": "success", "phase_execution_mask": 7
    });
    let decoded: RestoreTelemetry = serde_json::from_value(legacy).unwrap();
    let serialized = serde_json::to_value(&decoded).unwrap();
    assert_eq!(serialized["trap_shape_count"], 0);
    assert_eq!(
        serde_json::from_value::<RestoreTelemetry>(serialized).unwrap(),
        decoded
    );
    for (decision, spelling) in [
        (RestoreDecision::Success, "success"),
        (RestoreDecision::Partial, "partial"),
        (RestoreDecision::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(decision).unwrap(), spelling);
    }
}

#[test]
fn synthetic_trace_matches_production_restore_decision() {
    let p = pipeline();
    for raw in [
        "Keep FOO_12; alice@example.invalid",
        "Grüße Kunde_7; alice@example.invalid",
    ] {
        for traced in [false, true] {
            let session = Session::new(Scope::Ephemeral).unwrap();
            let clean = if traced {
                let (CleanDocument::Text(text), _, _, _) = p
                    .clean_text_with_safety_net_policy_detect_context_and_protection_trace(
                        &session,
                        raw,
                        &[],
                        &DictionaryBundle::default(),
                        SafetyNetPolicy::default(),
                    )
                    .unwrap()
                else {
                    panic!("text")
                };
                text
            } else {
                clean(&p, &session, raw)
            };
            assert_success(&session, &clean, raw, 1);
            let input = format!("{clean} <deadbeef:Email_999>");
            let (_, telemetry) = p.restore_with_telemetry(&session, &input).unwrap();
            assert_eq!(telemetry.restore_decision, RestoreDecision::Failed);
        }
    }
}

#[test]
fn differential_enumeration_only_relaxes_bare_identifiers_and_authorized_output() {
    // Expected directions come from the counter-split contract, independently of the classifier.
    let labels = [
        "bare_upper",
        "bare_lower",
        "own_mapped",
        "own_unmapped",
        "foreign",
        "legacy",
        "fp_mapped",
        "fp_foreign",
        "authorized_trap",
        "authorized_prefixed",
        "fp_own_unmapped",
    ];
    let mut divergences = [0usize; 11];
    let p = pipeline();
    for ordinal in 1..=400 {
        let session = Session::new(Scope::Ephemeral).unwrap();
        let own = session.session_hex();
        let foreign = if own == "deadbeef" {
            "cafebabe"
        } else {
            "deadbeef"
        };
        let mapped = session
            .tokenize(&PiiClass::Email, "alice@example.invalid")
            .unwrap();
        let fp = session
            .format_preserving_fake(&PiiClass::Name, "Dr. Schmidt")
            .unwrap();
        let trap = session
            .tokenize(&PiiClass::custom("record"), &format!("ORDER_{ordinal}"))
            .unwrap();
        let authorized = session
            .tokenize(
                &PiiClass::custom("reference"),
                &format!("<{foreign}:Email_{ordinal}>"),
            )
            .unwrap();
        let inputs = [
            format!("FOO_{ordinal}"),
            format!("run_{ordinal}"),
            mapped,
            format!("<{own}:Email_{}>", ordinal + 1000),
            format!("<{foreign}:Email_{ordinal}>"),
            format!("<Email_{ordinal}>"),
            fp,
            format!("email{ordinal}.{foreign}@gaze-fake.invalid"),
            trap,
            authorized,
            format!("email{}.{own}@gaze-fake.invalid", ordinal + 1000),
        ];
        for (category, input) in inputs.iter().enumerate() {
            let (restored, telemetry) = p.restore_with_telemetry(&session, input).unwrap();
            let old_failed = gaze::token_shape::pattern()
                .find_iter(&restored.text)
                .any(|m| {
                    gaze::token_shape::is_trap(m.as_str()) || !session.contains_token(m.as_str())
                });
            let new_failed = telemetry.restore_decision == RestoreDecision::Failed;
            let expected_old = !matches!(category, 2 | 6);
            let expected_new = matches!(category, 3 | 4 | 5 | 7 | 10);
            assert_eq!(old_failed, expected_old, "old {}", labels[category]);
            assert_eq!(new_failed, expected_new, "new {}", labels[category]);
            if old_failed != new_failed {
                assert!(old_failed && !new_failed);
                assert!(matches!(category, 0 | 1 | 8 | 9));
                divergences[category] += 1;
            }
        }
    }
    assert_eq!(divergences, [400, 400, 0, 0, 0, 0, 0, 0, 400, 400, 0]);
    for (label, count) in labels.iter().zip(divergences) {
        println!("{label}: cases=400 failed_to_success={count}");
    }
}
