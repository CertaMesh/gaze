use std::collections::BTreeMap;

use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, Pipeline, PiiClass, RawDocument, RegexDetector,
    Scope, Session, Value,
};

const CANARY: &str = "CANARY_EMAIL_DO_NOT_LEAK@test.local";

#[test]
fn canary_never_leaks_through_structured_redaction() {
    let session = Session::new(Scope::Ephemeral).expect("session");
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().expect("email detector"))
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .expect("pipeline");

    let raw = RawDocument::Structured(BTreeMap::from([
        ("email".to_string(), Value::String(CANARY.to_string())),
        ("id".to_string(), Value::I64(1)),
    ]));

    let clean = pipeline.redact(&session, raw).expect("redact");
    let json = serde_json::to_string(&clean).expect("serialize clean");

    assert!(
        !json.contains(CANARY),
        "canary leaked through structured redaction: {json}"
    );

    let token = match clean {
        CleanDocument::Structured(fields) => fields["email"]
            .as_str()
            .expect("tokenized email must be a string")
            .to_string(),
        CleanDocument::Text(_) => panic!("expected structured clean document"),
    };

    assert!(token.starts_with("Email_"), "unexpected token format: {token}");
    assert_eq!(
        session.restore_strict(&token).expect("restore token"),
        CANARY
    );
}
