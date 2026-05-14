use gaze_proxy::adapters::GeminiAdapter;
use gaze_proxy::ProviderAdapter;
use serde_json::json;
use url::Url;

#[test]
fn gemini_walks_text_function_call_and_function_response() {
    let adapter =
        GeminiAdapter::new(Url::parse("https://generativelanguage.googleapis.com").unwrap());
    let mut body = json!({
        "systemInstruction": {"parts": [{"text": "system alice@example.invalid"}]},
        "contents": [{
            "role": "user",
            "parts": [
                {"text": "email alice@example.invalid"},
                {"functionCall": {"name": "lookup", "args": {"email": "alice@example.invalid"}}},
                {"functionResponse": {"name": "lookup", "response": {"email": "alice@example.invalid"}}}
            ]
        }]
    });
    let surfaces = adapter.request_pii_surfaces(&mut body);
    let paths: Vec<_> = surfaces
        .iter()
        .map(|surface| surface.field_path.as_str())
        .collect();

    assert!(paths.contains(&"systemInstruction.parts[0].text"));
    assert!(paths.contains(&"contents[0].parts[0].text"));
    assert!(paths.contains(&"contents[0].parts[1].functionCall.args.email"));
    assert!(paths.contains(&"contents[0].parts[2].functionResponse.response.email"));
}
