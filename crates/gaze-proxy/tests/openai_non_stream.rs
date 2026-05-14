use gaze::{
    token_shape, Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument,
    Scope, Session,
};
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::ProviderAdapter;
use gaze_recognizers::RegexDetector;
use serde_json::json;
use url::Url;

fn email_pipeline() -> Pipeline {
    Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap()
}

#[test]
fn openai_request_surfaces_redact_content_and_tool_arguments() {
    let adapter = OpenAiAdapter::new(Url::parse("https://api.openai.com").unwrap());
    let pipeline = email_pipeline();
    let session = Session::new(Scope::Conversation("openai".to_string())).unwrap();
    let mut body = json!({
        "messages": [{
            "role": "user",
            "content": "email alice@example.invalid",
            "tool_calls": [{
                "function": {
                    "name": "lookup",
                    "arguments": "{\"email\":\"alice@example.invalid\"}"
                }
            }]
        }]
    });

    for surface in adapter.request_pii_surfaces(&mut body) {
        let CleanDocument::Text(clean) = pipeline
            .redact(&session, RawDocument::Text(surface.text.clone()))
            .unwrap()
        else {
            panic!("text clean document expected");
        };
        *surface.text = clean;
    }

    let serialized = body.to_string();
    assert!(!serialized.contains("alice@example.invalid"));
    assert!(serialized.contains("Email_1"));
    let token = token_shape::find_token(&serialized).expect("token emitted");
    assert_eq!(
        session.restore(token).as_deref(),
        Some("alice@example.invalid")
    );
}
