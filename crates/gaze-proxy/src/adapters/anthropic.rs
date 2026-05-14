use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{
    push_string, push_text_blocks, walk_all_strings, PiiSurface, ProviderAdapter, SseEvent,
};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AnthropicAdapter {
    upstream: Url,
}

impl AnthropicAdapter {
    pub fn new(upstream: Url) -> Self {
        Self { upstream }
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn matches_path(&self, method: &Method, path: &str) -> bool {
        method == Method::POST && path == "/v1/messages"
    }

    fn upstream_base(&self) -> &Url {
        &self.upstream
    }

    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "system" => push_text_blocks(&mut surfaces, "system", value),
                    "messages" => {
                        if let Value::Array(messages) = value {
                            for (index, message) in messages.iter_mut().enumerate() {
                                if let Value::Object(message) = message {
                                    for (message_key, message_value) in message {
                                        if message_key == "content" {
                                            collect_content_blocks(
                                                &mut surfaces,
                                                &format!("messages[{index}].content"),
                                                message_value,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            if let Some(content) = root.get_mut("content") {
                collect_content_blocks(&mut surfaces, "content", content);
            }
        }
        surfaces
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = &mut event.data {
            if matches!(root.get("type"), Some(Value::String(kind)) if kind == "content_block_delta")
            {
                if let Some(Value::Object(delta)) = root.get_mut("delta") {
                    for (key, value) in delta {
                        if let Value::String(text) = value {
                            match key.as_str() {
                                "text" => surfaces.push(PiiSurface {
                                    field_path: "delta.text".to_string(),
                                    text,
                                }),
                                "partial_json" => surfaces.push(PiiSurface {
                                    field_path: "delta.partial_json".to_string(),
                                    text,
                                }),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        surfaces
    }
}

fn collect_content_blocks<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: &str,
    content: &'a mut Value,
) {
    match content {
        Value::String(_) => push_string(surfaces, prefix, content),
        Value::Array(blocks) => {
            for (index, block) in blocks.iter_mut().enumerate() {
                let Value::Object(block) = block else {
                    continue;
                };
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(Value::String(text)) = block.get_mut("text") {
                            surfaces.push(PiiSurface {
                                field_path: format!("{prefix}[{index}].text"),
                                text,
                            });
                        }
                    }
                    Some("tool_use") => {
                        if let Some(input) = block.get_mut("input") {
                            walk_all_strings(surfaces, format!("{prefix}[{index}].input"), input);
                        }
                    }
                    Some("tool_result") => {
                        if let Some(result) = block.get_mut("content") {
                            collect_content_blocks(
                                surfaces,
                                &format!("{prefix}[{index}].content"),
                                result,
                            );
                        }
                    }
                    Some("image") | None | Some(_) => {}
                }
            }
        }
        _ => {}
    }
}
