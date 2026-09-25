use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{
    push_string, push_text_blocks, walk_all_strings, PiiSurface, ProviderAdapter, SseEvent,
    SurfaceSyntax,
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

    /// Surfaces the free-text carriers of an OpenAI request.
    ///
    /// This allowlist enumerates positions whose contents the MODEL reads, and which are
    /// therefore safe to pseudonymize: whatever token is substituted round-trips through the
    /// provider and back through restore.
    ///
    /// Resource identifiers are deliberately NOT surfaced — `tool_call_id`, `previous_response_id`,
    /// and `model` are resolved provider-side, so substituting a token would silently break the
    /// request rather than protect it. PII in those positions is caught by the outbound residual
    /// re-scan and fails closed, which tells the adopter their identifier carries PII instead of
    /// handing them a broken request that looks fine.
    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = Vec::new();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "system" => push_string(&mut surfaces, "system", value, SurfaceSyntax::Text),
                    "prompt" => {
                        push_text_blocks(&mut surfaces, "prompt", value, SurfaceSyntax::Text);
                    }
                    "instructions" => {
                        push_string(&mut surfaces, "instructions", value, SurfaceSyntax::Text);
                    }
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
                    "input" => {
                        push_text_blocks(&mut surfaces, "input", value, SurfaceSyntax::Text);
                    }
                    // The documented end-user identifier. Free-form adopter text and a
                    // first-class PII carrier in practice; opaque to the provider beyond abuse
                    // monitoring, so pseudonymizing it is safe.
                    "user" => push_string(&mut surfaces, "user", value, SurfaceSyntax::Text),
                    // Adopter-attached key/value pairs, echoed back verbatim.
                    "metadata" => walk_all_strings(&mut surfaces, "metadata".to_string(), value),
                    // Stop sequences are matched against generated text, so they are model-visible.
                    "stop" => walk_all_strings(&mut surfaces, "stop".to_string(), value),
                    // Tool and schema declarations: names, descriptions, and enum members are all
                    // read by the model. `walk_all_strings` covers the whole declaration subtree.
                    "tools" => walk_all_strings(&mut surfaces, "tools".to_string(), value),
                    "functions" => walk_all_strings(&mut surfaces, "functions".to_string(), value),
                    "response_format" => {
                        walk_all_strings(&mut surfaces, "response_format".to_string(), value);
                    }
                    "prediction" => {
                        walk_all_strings(&mut surfaces, "prediction".to_string(), value);
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
                                            "text" => push_string(
                                                &mut surfaces,
                                                format!("choices[{index}].text"),
                                                choice_value,
                                                SurfaceSyntax::ModelOutput,
                                            ),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "output" => collect_output_surfaces(&mut surfaces, value),
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        self.response_pii_surfaces(&mut event.data)
    }

    /// Chat Completions selects JSON output with `response_format`, the Responses API with
    /// `text.format`; both take `json_object` or `json_schema`.
    fn requests_json_output(&self, request: &Value) -> bool {
        let is_json = |format: &Value| {
            matches!(
                format.get("type").and_then(Value::as_str),
                Some("json_object" | "json_schema")
            )
        };
        request.get("response_format").is_some_and(is_json)
            || request
                .get("text")
                .and_then(|text| text.get("format"))
                .is_some_and(is_json)
    }
}

/// Surfaces every string of the Responses API `output` items, typed by what carries it: the
/// `arguments` of a `function_call` or `mcp_call` item is a serialized JSON document, and the
/// `output_text` of a `message` item is model output.
fn collect_output_surfaces<'a>(surfaces: &mut Vec<PiiSurface<'a>>, output: &'a mut Value) {
    let Value::Array(items) = output else {
        walk_all_strings(surfaces, "output".to_string(), output);
        return;
    };
    for (index, item) in items.iter_mut().enumerate() {
        let prefix = format!("output[{index}]");
        let kind = item.get("type").and_then(Value::as_str);
        let is_call = matches!(kind, Some("function_call" | "mcp_call"));
        let is_message = kind == Some("message");
        let Value::Object(fields) = item else {
            walk_all_strings(surfaces, prefix, item);
            continue;
        };
        for (key, value) in fields {
            let path = format!("{prefix}.{key}");
            match key.as_str() {
                "arguments" if is_call && value.is_string() => {
                    push_string(surfaces, path, value, SurfaceSyntax::Json);
                }
                "content" if is_message => collect_output_content(surfaces, path, value),
                _ => walk_all_strings(surfaces, path, value),
            }
        }
    }
}

fn collect_output_content<'a>(
    surfaces: &mut Vec<PiiSurface<'a>>,
    prefix: String,
    content: &'a mut Value,
) {
    let Value::Array(parts) = content else {
        walk_all_strings(surfaces, prefix, content);
        return;
    };
    for (index, part) in parts.iter_mut().enumerate() {
        let part_prefix = format!("{prefix}[{index}]");
        let is_output_text = part.get("type").and_then(Value::as_str) == Some("output_text");
        let Value::Object(fields) = part else {
            walk_all_strings(surfaces, part_prefix, part);
            continue;
        };
        for (key, value) in fields {
            let path = format!("{part_prefix}.{key}");
            if is_output_text && key == "text" && value.is_string() {
                push_string(surfaces, path, value, SurfaceSyntax::ModelOutput);
            } else {
                walk_all_strings(surfaces, path, value);
            }
        }
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
            "content" => push_text_blocks(
                surfaces,
                &format!("{prefix}.content"),
                value,
                SurfaceSyntax::ModelOutput,
            ),
            "tool_results" => push_text_blocks(
                surfaces,
                &format!("{prefix}.tool_results"),
                value,
                SurfaceSyntax::Text,
            ),
            // The participant name on a message. A person's name by definition, and read by the
            // model, so it is pseudonymized rather than rejected.
            "name" => push_string(
                surfaces,
                format!("{prefix}.name"),
                value,
                SurfaceSyntax::Text,
            ),
            "tool_calls" => {
                if let Value::Array(tool_calls) = value {
                    for (index, tool_call) in tool_calls.iter_mut().enumerate() {
                        if let Value::Object(tool_call) = tool_call {
                            if let Some(Value::Object(function)) = tool_call.get_mut("function") {
                                // A serialized JSON object, in requests and responses alike.
                                if let Some(args) = function.get_mut("arguments") {
                                    push_string(
                                        surfaces,
                                        format!("{prefix}.tool_calls[{index}].function.arguments"),
                                        args,
                                        SurfaceSyntax::Json,
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
