use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{
    push_string, push_text_blocks, walk_all_strings, PiiSurface, ProviderAdapter, SseEvent,
};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OpenAiAdapter {
    upstream: Url,
}

impl OpenAiAdapter {
    pub fn new(upstream: Url) -> Self {
        Self { upstream }
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for OpenAiAdapter {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn matches_path(&self, method: &Method, path: &str) -> bool {
        method == Method::POST
            && matches!(
                path,
                "/v1/chat/completions" | "/v1/completions" | "/v1/responses"
            )
    }

    fn upstream_base(&self) -> &Url {
        &self.upstream
    }

    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "system" => push_string(&mut surfaces, "system", value),
                    "messages" => {
                        if let Value::Array(messages) = value {
                            for (index, message) in messages.iter_mut().enumerate() {
                                collect_message_surfaces(
                                    &mut surfaces,
                                    format!("messages[{index}]"),
                                    message,
                                );
                            }
                        }
                    }
                    "input" => push_text_blocks(&mut surfaces, "input", value),
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "choices" => {
                        if let Value::Array(choices) = value {
                            for (index, choice) in choices.iter_mut().enumerate() {
                                if let Value::Object(choice) = choice {
                                    for (choice_key, choice_value) in choice {
                                        match choice_key.as_str() {
                                            "message" => collect_message_surfaces(
                                                &mut surfaces,
                                                format!("choices[{index}].message"),
                                                choice_value,
                                            ),
                                            "delta" => collect_message_surfaces(
                                                &mut surfaces,
                                                format!("choices[{index}].delta"),
                                                choice_value,
                                            ),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "output" => walk_all_strings(&mut surfaces, "output".to_string(), value),
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        self.response_pii_surfaces(&mut event.data)
    }
}

fn collect_message_surfaces<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: String,
    message: &'a mut Value,
) {
    let Value::Object(map) = message else {
        return;
    };
    for (key, value) in map {
        match key.as_str() {
            "content" => push_text_blocks(surfaces, &format!("{prefix}.content"), value),
            "tool_results" => push_text_blocks(surfaces, &format!("{prefix}.tool_results"), value),
            "tool_calls" => {
                if let Value::Array(tool_calls) = value {
                    for (index, tool_call) in tool_calls.iter_mut().enumerate() {
                        if let Value::Object(tool_call) = tool_call {
                            if let Some(Value::Object(function)) = tool_call.get_mut("function") {
                                if let Some(args) = function.get_mut("arguments") {
                                    push_string(
                                        surfaces,
                                        format!("{prefix}.tool_calls[{index}].function.arguments"),
                                        args,
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
