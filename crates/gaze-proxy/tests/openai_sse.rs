use gaze::{
    Action, ClassRule, CleanDocument, DefaultRule, PiiClass, Pipeline, RawDocument, Scope, Session,
};
use gaze_proxy::adapter::SseEvent;
use gaze_proxy::adapters::OpenAiAdapter;
use gaze_proxy::ProviderAdapter;
use gaze_recognizers::RegexDetector;
use serde_json::json;
use url::Url;

#[test]
fn openai_sse_surfaces_include_delta_content_and_tool_arguments() {
    let pipeline = Pipeline::builder()
        .detector(RegexDetector::emails().unwrap())
        .rule(ClassRule::new(PiiClass::Email, Action::Tokenize))
        .rule(DefaultRule::new(Action::Preserve))
        .build()
        .unwrap();
    let session = Session::new(Scope::Conversation("openai-sse".to_string())).unwrap();
    let CleanDocument::Text(token) = pipeline
        .redact(
            &session,
            RawDocument::Text("alice@example.invalid".to_string()),
        )
        .unwrap()
    else {
        panic!("text clean document expected");
    };
    let mut event = SseEvent::new(
        None,
        json!({
            "choices": [{
                "delta": {
                    "content": token,
                    "tool_calls": [{
                        "function": {
                            "arguments": "{\"email\":\"<Email_1>\"}"
                        }
                    }]
                }
            }]
        }),
    );
    let adapter = OpenAiAdapter::new(Url::parse("https://api.openai.com").unwrap());
    let surfaces = adapter.sse_event_pii_surfaces(&mut event);
    let paths: Vec<_> = surfaces
        .iter()
        .map(|surface| surface.field_path.as_str())
        .collect();

    assert!(paths.iter().any(|path| path.ends_with(".delta.content")));
    assert!(paths
        .iter()
        .any(|path| path.ends_with(".delta.tool_calls[0].function.arguments")));
}
