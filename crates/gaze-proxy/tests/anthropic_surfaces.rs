use gaze_proxy::adapters::AnthropicAdapter;
use gaze_proxy::ProviderAdapter;
use serde_json::json;
use url::Url;

#[test]
fn anthropic_walks_text_tool_use_and_tool_result_but_skips_image_source() {
    let adapter = AnthropicAdapter::new(Url::parse("https://api.anthropic.com").unwrap());
    let mut body = json!({
        "system": [{"type": "text", "text": "system alice@example.invalid"}],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "email alice@example.invalid"},
                {"type": "tool_use", "name": "lookup", "input": {"email": "alice@example.invalid"}},
                {"type": "tool_result", "content": [{"type": "text", "text": "alice@example.invalid"}]},
                {"type": "image", "source": {"data": "alice@example.invalid"}}
            ]
        }]
    });
    let surfaces = adapter.request_pii_surfaces(&mut body);
    let paths: Vec<_> = surfaces
        .iter()
        .map(|surface| surface.field_path.as_str())
        .collect();

    assert!(paths.contains(&"system[0].text"));
    assert!(paths.contains(&"messages[0].content[0].text"));
    assert!(paths.contains(&"messages[0].content[1].input.email"));
    assert!(paths.contains(&"messages[0].content[2].content[0].text"));
    assert!(!paths.iter().any(|path| path.contains("image")));
}
