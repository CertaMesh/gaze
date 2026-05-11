use gaze_types::{
    AmbiguityReason, AmbiguityRecord, LosingCandidate, PiiClass, ValidatorFailReason,
};

#[test]
fn ambiguity_record_round_trips_each_reason_variant() {
    let reasons = [
        AmbiguityReason::NoAnchor,
        AmbiguityReason::ValidatorIndeterminate,
        AmbiguityReason::MultiFamilyMatch,
        AmbiguityReason::PrecedenceTie,
    ];

    for reason in reasons {
        let record = AmbiguityRecord::new(
            PiiClass::Custom("postal_or_phone_de".to_string()),
            vec![
                LosingCandidate::new(
                    PiiClass::Custom("phone_de_national".to_string()),
                    "phone-de",
                ),
                LosingCandidate::new(PiiClass::Custom("postal_de".to_string()), "postal-de"),
            ],
            reason,
        );

        let json = serde_json::to_string(&record).expect("serialize ambiguity record");
        let decoded: AmbiguityRecord =
            serde_json::from_str(&json).expect("deserialize ambiguity record");
        assert_eq!(decoded, record);
    }
}

#[test]
fn ambiguity_reason_uses_snake_case_json() {
    assert_eq!(
        serde_json::to_string(&AmbiguityReason::NoAnchor).expect("serialize reason"),
        "\"no_anchor\""
    );
    assert_eq!(
        serde_json::to_string(&AmbiguityReason::PrecedenceTie).expect("serialize reason"),
        "\"precedence_tie\""
    );
}

#[test]
fn losing_candidate_keeps_canonical_class_and_recognizer_id() {
    let candidate = LosingCandidate::new(PiiClass::Custom("postal_de".to_string()), "postal-de");
    let value = serde_json::to_value(&candidate).expect("serialize candidate");

    assert_eq!(value["class"], "custom:postal_de");
    assert_eq!(value["recognizer_id"], "postal-de");
}

#[test]
fn validator_fail_reason_round_trips_all_variants() {
    let reasons = [
        ValidatorFailReason::LuhnFailed,
        ValidatorFailReason::IbanMod97Failed,
        ValidatorFailReason::EmailRfcFailed,
        ValidatorFailReason::E164PhoneFailed,
    ];

    for reason in reasons {
        let json = serde_json::to_string(&reason).expect("serialize reason");
        let decoded: ValidatorFailReason = serde_json::from_str(&json).expect("deserialize reason");
        assert_eq!(decoded, reason);
    }
}
