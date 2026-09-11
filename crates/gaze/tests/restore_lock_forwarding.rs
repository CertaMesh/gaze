#![cfg(all(feature = "experimental-benchmark-baseline-lock", unix))]

use gaze::{Pipeline, Scope, Session};

#[test]
fn locked_restore_uses_shared_classifier_without_provider() {
    use gaze::experimental_benchmark_baseline_lock::BenchmarkLockPolicy;
    use gaze::{PiiClass, RestoreDecision, RuleSpec};

    let baseline = Pipeline::builder()
        .detector(gaze_recognizers::RegexDetector::emails().unwrap())
        .build()
        .unwrap();
    let locked = BenchmarkLockPolicy::try_from(Vec::<RuleSpec>::new())
        .unwrap()
        .bind(baseline)
        .unwrap();
    // This is the forwarding method used by the producer's RestoreSource::Locked.
    let session = Session::new(Scope::Ephemeral).unwrap();
    let value = "authorized <Email_1> ORDER_12345 <deadbeef:Custom:class_alpha_1";
    let token = session
        .tokenize(&PiiClass::custom("class_alpha"), value)
        .unwrap();
    let input = format!("{token} Kunde_7 run_1");
    let (restored, telemetry) = locked.restore_with_telemetry(&session, &input).unwrap();
    assert_eq!(restored.text, format!("{value} Kunde_7 run_1"));
    assert_eq!(telemetry.restore_decision, RestoreDecision::Success);
    assert_eq!(telemetry.unknown_token_count, 0);
    assert_eq!(telemetry.manifest_bypass_count, 2);
    assert_eq!(session.restore_strict_text(&input).unwrap(), restored.text);

    for unknown in [
        "<Email_1>".to_owned(),
        "location_7".to_owned(),
        format!("<{}:Email_999>", session.session_hex()),
        "<deadbeef:Email_999>".to_owned(),
        "<deadbeef:Custom:class_alpha_1".to_owned(),
    ] {
        let input = format!("{token} {unknown}");
        let (_, telemetry) = locked.restore_with_telemetry(&session, &input).unwrap();
        assert_eq!(
            telemetry.restore_decision,
            RestoreDecision::Failed,
            "{unknown}"
        );
        assert_eq!(telemetry.unknown_token_count, 1, "{unknown}");
        assert_eq!(telemetry.manifest_bypass_count, 0, "{unknown}");
        assert!(session.restore_strict_text(&input).is_err());
    }
}
