use std::borrow::Cow;

use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{walk_all_strings, PiiSurface, ProviderAdapter, SseEvent};

/// Canonicalizes a protobuf-JSON field name to its lowerCamelCase spelling.
///
/// The Generative Language API is a protobuf-JSON surface, and protobuf JSON parsers accept
/// BOTH the lowerCamelCase name and the original snake_case field name for every field. Matching
/// the camelCase spelling literally therefore left `system_instruction` — the same field, spelled
/// the other legal way — with no detection at all, while `systemInstruction` was covered.
///
/// Callers match on the canonical form but keep the caller's ORIGINAL key in field paths, so a
/// path still names the bytes actually on the wire.
fn canonical_field_name(key: &str) -> Cow<'_, str> {
    if !key.contains('_') {
        return Cow::Borrowed(key);
    }
    let mut canonical = String::with_capacity(key.len());
    let mut capitalize_next = false;
    for character in key.chars() {
        if character == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            canonical.extend(character.to_uppercase());
            capitalize_next = false;
        } else {
            canonical.push(character);
        }
    }
    Cow::Owned(canonical)
}

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
                match canonical_field_name(key).as_ref() {
                    "contents" => {
                        if let Value::Array(contents) = value {
                            for (index, content) in contents.iter_mut().enumerate() {
                                collect_content(&mut surfaces, format!("{key}[{index}]"), content);
                            }
                        }
                    }
                    "systemInstruction" => {
                        collect_content(&mut surfaces, key.clone(), value);
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
    let Some((parts_key, Value::Array(parts))) = content
        .iter_mut()
        .find(|(key, _)| canonical_field_name(key) == "parts")
    else {
        return;
    };
    let parts_prefix = format!("{prefix}.{parts_key}");
    for (index, part) in parts.iter_mut().enumerate() {
        let Value::Object(part) = part else {
            continue;
        };
        for (key, value) in part {
            let part_prefix = format!("{parts_prefix}[{index}].{key}");
            match canonical_field_name(key).as_ref() {
                "text" => {
                    if let Value::String(text) = value {
                        surfaces.push(PiiSurface {
                            field_path: part_prefix,
                            text,
                        });
                    }
                }
                "functionCall" => {
                    if let Some((args_key, args)) = value
                        .as_object_mut()
                        .and_then(|call| call.iter_mut().find(|(key, _)| *key == "args"))
                    {
                        walk_all_strings(surfaces, format!("{part_prefix}.{args_key}"), args);
                    }
                }
                "functionResponse" => {
                    if let Some((response_key, response)) = value
                        .as_object_mut()
                        .and_then(|call| call.iter_mut().find(|(key, _)| *key == "response"))
                    {
                        walk_all_strings(
                            surfaces,
                            format!("{part_prefix}.{response_key}"),
                            response,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}
