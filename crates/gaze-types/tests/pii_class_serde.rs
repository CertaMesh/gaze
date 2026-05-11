use gaze_types::PiiClass;

#[test]
fn pii_class_serializes_to_canonical_audit_form() {
    let cases = [
        (PiiClass::Email, "\"email\""),
        (PiiClass::Name, "\"name\""),
        (PiiClass::Location, "\"location\""),
        (PiiClass::Organization, "\"organization\""),
        (PiiClass::Custom("foo".to_string()), "\"custom:foo\""),
    ];

    for (class, expected_json) in cases {
        let encoded = serde_json::to_string(&class).expect("serialize pii class");
        assert_eq!(encoded, expected_json);

        let decoded: PiiClass = serde_json::from_str(&encoded).expect("deserialize pii class");
        assert_eq!(decoded, class);
    }
}

#[test]
fn pii_class_rejects_unknown_canonical_form() {
    let err = serde_json::from_str::<PiiClass>("\"unknown\"").unwrap_err();
    assert!(err.to_string().contains("unknown PII class unknown"));
}
