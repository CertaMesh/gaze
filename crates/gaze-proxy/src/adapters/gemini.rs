use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{walk_all_strings, PiiSurface, ProviderAdapter, SseEvent};

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GeminiAdapter {
    upstream: Url,
}

impl GeminiAdapter {
    pub fn new(upstream: Url) -> Self {
        Self { upstream }
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn matches_path(&self, method: &Method, path: &str) -> bool {
        method == Method::POST
            && path.starts_with("/v1beta/models/")
            && (path.ends_with(":generateContent")
                || path.ends_with(":streamGenerateContent")
                || path.ends_with(":countTokens"))
    }

    fn upstream_base(&self) -> &Url {
        &self.upstream
    }

    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        collect_gemini_surfaces(body, true)
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        collect_gemini_surfaces(body, false)
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        self.response_pii_surfaces(&mut event.data)
    }
}

fn collect_gemini_surfaces(body: &mut Value, request: bool) -> Vec<PiiSurface<'_>> {
    let mut surfaces = Vec::new();
    if let Value::Object(root) = body {
        if request {
            for (key, value) in root {
                match key.as_str() {
                    "contents" => {
                        if let Value::Array(contents) = value {
                            for (index, content) in contents.iter_mut().enumerate() {
                                collect_content(
                                    &mut surfaces,
                                    format!("contents[{index}]"),
                                    content,
                                );
                            }
                        }
                    }
                    "systemInstruction" => {
                        collect_content(&mut surfaces, "systemInstruction".to_string(), value);
                    }
                    _ => {}
                }
            }
        } else if let Some(Value::Array(candidates)) = root.get_mut("candidates") {
            for (index, candidate) in candidates.iter_mut().enumerate() {
                if let Value::Object(candidate) = candidate {
                    if let Some(content) = candidate.get_mut("content") {
                        collect_content(
                            &mut surfaces,
                            format!("candidates[{index}].content"),
                            content,
                        );
                    }
                }
            }
        }
    }
    surfaces
}

fn collect_content<'a>(surfaces: &mut Vec<PiiSurface<'a>>, prefix: String, value: &'a mut Value) {
    let Value::Object(content) = value else {
        return;
    };
    if let Some(Value::Array(parts)) = content.get_mut("parts") {
        for (index, part) in parts.iter_mut().enumerate() {
            let Value::Object(part) = part else {
                continue;
            };
            for (key, value) in part {
                match key.as_str() {
                    "text" => {
                        if let Value::String(text) = value {
                            surfaces.push(PiiSurface {
                                field_path: format!("{prefix}.parts[{index}].text"),
                                text,
                            });
                        }
                    }
                    "functionCall" => {
                        if let Some(args) =
                            value.as_object_mut().and_then(|call| call.get_mut("args"))
                        {
                            walk_all_strings(
                                surfaces,
                                format!("{prefix}.parts[{index}].functionCall.args"),
                                args,
                            );
                        }
                    }
                    "functionResponse" => {
                        if let Some(response) = value
                            .as_object_mut()
                            .and_then(|call| call.get_mut("response"))
                        {
                            walk_all_strings(
                                surfaces,
                                format!("{prefix}.parts[{index}].functionResponse.response"),
                                response,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
