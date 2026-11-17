//! JSON-text carriers on the request path: labelled fields inside OpenAI `arguments` bind with
//! the stock `core` activation, including when a session-owned token splits a labelled leaf.
//! All values are synthetic.
use super::*;

fn stock() -> (Pipeline, Vec<LocaleTag>) {
    let core = gaze_assembly::CorePipelineConfig::new().build().unwrap();
    let locales = core.locale_chain().as_slice().to_vec();
    (core.into_pipeline(), locales)
}

fn arguments_body(arguments: &str) -> Value {
    serde_json::json!({
        "model":"gpt-test",
        "messages":[
            {"role":"user","content":"look up the record"},
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"lookup","arguments":arguments}}
            ]}
        ]
    })
}

fn forwarded_arguments(body: &Value) -> String {
    body.pointer("/messages/1/tool_calls/0/function/arguments")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned()
}

// fixture-cited(crates/gaze-proxy/src/server/json_carrier_tests.rs:server::json_carrier_tests::owned_token_in_a_labelled_argument_never_exposes_the_rest)
const SYNTHETIC_EMAIL: &str = "synthetic@example.invalid";

#[test]
fn owned_token_in_a_labelled_argument_never_exposes_the_rest() {
    let (pipeline, locales) = stock();
    let session = Session::new(Scope::Conversation("synthetic-json-carrier".into())).unwrap();
    let owned = session
        .tokenize(&gaze::PiiClass::Email, SYNTHETIC_EMAIL)
        .unwrap();
    let adapter = crate::adapters::OpenAiAdapter::new(Url::parse("http://127.0.0.1:1").unwrap());
    let arguments = format!(r#"{{"customer_id":"{owned} C-731284","employee_id":"E-731285"}}"#);
    let mut body = arguments_body(&arguments);
    let staged = redact_surfaces(
        &pipeline,
        &session,
        adapter.request_pii_surfaces(&mut body),
        &locales,
        &DictionaryBundle::default(),
    )
    .unwrap();
    let clean = forwarded_arguments(&body);
    assert!(!clean.contains("73128"), "{clean}");
    assert!(
        clean.contains(&owned),
        "owned token keeps its authority: {clean}"
    );
    let inner: Value = serde_json::from_str(&clean).expect("arguments stay JSON");
    assert!(inner["customer_id"].is_string() && inner["employee_id"].is_string());
    staged.transaction.commit().unwrap();
    // Restore expands every token, the owned one included, back to the raw arguments.
    assert_eq!(
        session.restore_strict_text(&clean).unwrap(),
        arguments.replace(&owned, SYNTHETIC_EMAIL)
    );
}

#[test]
fn labelled_numbers_in_arguments_refuse_typed_and_unlabelled_stay() {
    let (pipeline, locales) = stock();
    let session = Session::new(Scope::Ephemeral).unwrap();
    let adapter = crate::adapters::OpenAiAdapter::new(Url::parse("http://127.0.0.1:1").unwrap());
    let mut body = arguments_body(r#"{"customer_id":731284,"limit":20}"#);
    let refused = redact_surfaces(
        &pipeline,
        &session,
        adapter.request_pii_surfaces(&mut body),
        &locales,
        &DictionaryBundle::default(),
    );
    match refused {
        Err(ProxyError::Refused { refusal }) => assert!(matches!(
            refusal.error,
            gaze::ProtectionError::Untransformable {
                reason: gaze_types::UntransformableReason::JsonNumberNoTextOnlyPath,
                ..
            }
        )),
        Err(other) => panic!("typed refusal expected, got {other:?}"),
        Ok(_) => panic!("typed refusal expected, got success"),
    }

    let unlabelled = r#"{"count":731284,"batch_id":"O-731284"}"#;
    let mut body = arguments_body(unlabelled);
    let staged = redact_surfaces(
        &pipeline,
        &session,
        adapter.request_pii_surfaces(&mut body),
        &locales,
        &DictionaryBundle::default(),
    )
    .unwrap();
    drop(staged);
    assert_eq!(forwarded_arguments(&body), unlabelled);
}
