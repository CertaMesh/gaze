use std::borrow::Cow;

use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{
    walk_all_scalars, AdapterContract, PiiSurface, ProviderAdapter, RequestSurfaces, SseEvent,
    SseStreamKey, SseStreamSurface, SurfacePath, SurfaceSyntax,
};

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
    fn contract(&self) -> AdapterContract<'_> {
        AdapterContract::legacy()
    }

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

    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> RequestSurfaces<'a> {
        collect_gemini_surfaces(body, true)
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        collect_gemini_surfaces(body, false).strings
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        self.response_pii_surfaces(&mut event.data)
    }

    /// Every `streamGenerateContent` chunk continues each candidate's text. Its answer text and
    /// its thought summaries are separate streams, keyed by the candidate's `index`, which
    /// protobuf JSON omits when it is the default 0. Function-call arguments arrive whole.
    fn sse_event_stream_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<SseStreamSurface<'a>> {
        collect_gemini_stream_surfaces(&mut event.data, false, true, &mut Vec::new())
    }

    /// `generationConfig.responseMimeType = "application/json"` (which a response schema
    /// requires) makes every answer text part a JSON document.
    fn requests_json_output(&self, request: &Value) -> bool {
        let Value::Object(root) = request else {
            return false;
        };
        root.iter()
            .filter(|(key, _)| canonical_field_name(key) == "generationConfig")
            .filter_map(|(_, config)| config.as_object())
            .flatten()
            .any(|(key, value)| {
                canonical_field_name(key) == "responseMimeType"
                    && value
                        .as_str()
                        .is_some_and(|mime| mime.eq_ignore_ascii_case("application/json"))
            })
    }
}

/// Surfaces the free-text carriers of a Gemini request, or the model-authored text of a response.
///
/// The request allowlist enumerates positions whose contents the MODEL reads, and which are
/// therefore safe to pseudonymize.
///
/// Resource identifiers and opaque blobs are deliberately NOT surfaced — `cachedContent` is a
/// provider-side resource name, `fileData.fileUri` is fetched by the provider, and
/// `inlineData.data` is base64 payload. Substituting a token in any of them would silently break
/// the request rather than protect it, so PII there is caught by the outbound residual re-scan and
/// fails closed.
fn collect_gemini_surfaces(body: &mut Value, request: bool) -> RequestSurfaces<'_> {
    let mut numbers = Vec::new();
    let strings = collect_gemini_stream_surfaces(body, request, false, &mut numbers)
        .into_iter()
        .map(|fragment| fragment.surface)
        .collect();
    RequestSurfaces { strings, numbers }
}

/// [`collect_gemini_surfaces`], keying each response candidate's text fragments by the
/// candidate when `streamed`.
fn collect_gemini_stream_surfaces<'a>(
    body: &'a mut Value,
    request: bool,
    streamed: bool,
    numbers: &mut Vec<gaze_types::FieldProvenance>,
) -> Vec<SseStreamSurface<'a>> {
    let mut surfaces = Vec::new();
    if let Value::Object(root) = body {
        if request {
            for (key, value) in root {
                match canonical_field_name(key).as_ref() {
                    "contents" => {
                        if let Value::Array(contents) = value {
                            for (index, content) in contents.iter_mut().enumerate() {
                                collect_content(
                                    &mut surfaces,
                                    SurfacePath::root(key).index(index),
                                    content,
                                    None,
                                    numbers,
                                );
                            }
                        }
                    }
                    "systemInstruction" => {
                        collect_content(
                            &mut surfaces,
                            SurfacePath::root(key),
                            value,
                            None,
                            numbers,
                        );
                    }
                    // Tool and schema declarations: names, descriptions, and enum members are
                    // all read by the model.
                    "tools" | "toolConfig" => {
                        walk_complete(&mut surfaces, SurfacePath::root(key), value, numbers);
                    }
                    // Covers `stopSequences`, which is matched against generated text. Numeric
                    // members of this subtree are untouched here and are classified by the
                    // outbound residual re-scan's non-carrier declaration instead.
                    "generationConfig" => {
                        walk_complete(&mut surfaces, SurfacePath::root(key), value, numbers);
                    }
                    _ => {}
                }
            }
        } else if let Some(Value::Array(candidates)) = root.get_mut("candidates") {
            for (index, candidate) in candidates.iter_mut().enumerate() {
                if let Value::Object(candidate) = candidate {
                    let stream = streamed.then(|| {
                        // Proto3 JSON omits a default zero index. An explicit malformed
                        // index is unknown, and must not be guessed to be candidate zero.
                        match candidate.get("index") {
                            None => Some(0),
                            Some(index) => index.as_u64(),
                        }
                    });
                    if let Some(content) = candidate.get_mut("content") {
                        collect_content(
                            &mut surfaces,
                            SurfacePath::root("candidates")
                                .index(index)
                                .member("content"),
                            content,
                            stream,
                            numbers,
                        );
                    }
                }
            }
        }
    }
    surfaces
}

fn walk_complete<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    prefix: SurfacePath,
    value: &'a mut Value,
    numbers: &mut Vec<gaze_types::FieldProvenance>,
) {
    let mut complete = RequestSurfaces::default();
    walk_all_scalars(&mut complete, prefix, value);
    numbers.append(&mut complete.numbers);
    surfaces.extend(complete.into_iter().map(SseStreamSurface::complete));
}

/// Surfaces one `Content`. With a candidate `stream`, its text parts are fragments of that
/// candidate's answer stream, or of its thought stream for a thought summary.
fn collect_content<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    prefix: SurfacePath,
    value: &'a mut Value,
    stream: Option<Option<u64>>,
    numbers: &mut Vec<gaze_types::FieldProvenance>,
) {
    let Value::Object(content) = value else {
        return;
    };
    let Some((parts_key, Value::Array(parts))) = content
        .iter_mut()
        .find(|(key, _)| canonical_field_name(key) == "parts")
    else {
        return;
    };
    let parts_prefix = prefix.member(parts_key);
    for (index, part) in parts.iter_mut().enumerate() {
        let Value::Object(part) = part else {
            continue;
        };
        // A thought summary stays prose even when the answer is JSON.
        let (text_syntax, text_stream) =
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                (SurfaceSyntax::Text, "thought")
            } else {
                (SurfaceSyntax::ModelOutput, "text")
            };
        for (key, value) in part {
            let part_prefix = parts_prefix.index(index).member(key);
            match canonical_field_name(key).as_ref() {
                "text" => {
                    if let Value::String(text) = value {
                        surfaces.push(SseStreamSurface {
                            stream: stream.map(|index| {
                                SseStreamKey::new(format!("candidates.{text_stream}"), [index])
                            }),
                            surface: part_prefix.surface(text, text_syntax),
                        });
                    }
                }
                "functionCall" => {
                    if let Some((args_key, args)) = value
                        .as_object_mut()
                        .and_then(|call| call.iter_mut().find(|(key, _)| *key == "args"))
                    {
                        walk_complete(surfaces, part_prefix.member(args_key), args, numbers);
                    }
                }
                "functionResponse" => {
                    if let Some((response_key, response)) = value
                        .as_object_mut()
                        .and_then(|call| call.iter_mut().find(|(key, _)| *key == "response"))
                    {
                        walk_complete(
                            surfaces,
                            part_prefix.member(response_key),
                            response,
                            numbers,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}
