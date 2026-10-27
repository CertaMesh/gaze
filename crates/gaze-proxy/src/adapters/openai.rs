use http::Method;
use serde_json::Value;
use url::Url;

use crate::adapter::{
    push_string, push_text_blocks, walk_all_scalars, AdapterContract, PiiSurface, ProviderAdapter,
    RequestSurfaces, SseEvent, SseStreamKey, SseStreamSurface, SurfacePath, SurfaceSyntax,
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
    fn contract(&self) -> AdapterContract<'_> {
        AdapterContract::legacy()
    }

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
    fn request_pii_surfaces<'a>(&self, body: &'a mut Value) -> RequestSurfaces<'a> {
        let mut surfaces = RequestSurfaces::default();
        if let Value::Object(root) = body {
            for (key, value) in root {
                match key.as_str() {
                    "system" => push_string(
                        &mut surfaces,
                        SurfacePath::root("system"),
                        value,
                        SurfaceSyntax::Text,
                    ),
                    "prompt" => {
                        push_text_blocks(
                            &mut surfaces,
                            &SurfacePath::root("prompt"),
                            value,
                            SurfaceSyntax::Text,
                        );
                    }
                    "instructions" => {
                        push_string(
                            &mut surfaces,
                            SurfacePath::root("instructions"),
                            value,
                            SurfaceSyntax::Text,
                        );
                    }
                    "messages" => {
                        if let Value::Array(messages) = value {
                            for (index, message) in messages.iter_mut().enumerate() {
                                collect_message_surfaces(
                                    &mut surfaces,
                                    SurfacePath::root("messages").index(index),
                                    message,
                                );
                            }
                        }
                    }
                    "input" => {
                        push_text_blocks(
                            &mut surfaces,
                            &SurfacePath::root("input"),
                            value,
                            SurfaceSyntax::Text,
                        );
                    }
                    // The documented end-user identifier. Free-form adopter text and a
                    // first-class PII carrier in practice; opaque to the provider beyond abuse
                    // monitoring, so pseudonymizing it is safe.
                    "user" => push_string(
                        &mut surfaces,
                        SurfacePath::root("user"),
                        value,
                        SurfaceSyntax::Text,
                    ),
                    // Adopter-attached key/value pairs, echoed back verbatim.
                    "metadata" => {
                        walk_all_scalars(&mut surfaces, SurfacePath::root("metadata"), value)
                    }
                    // Stop sequences are matched against generated text, so they are model-visible.
                    "stop" => walk_all_scalars(&mut surfaces, SurfacePath::root("stop"), value),
                    // Tool and schema declarations: names, descriptions, and enum members are all
                    // read by the model. `walk_all_scalars` covers the whole declaration subtree.
                    "tools" => walk_all_scalars(&mut surfaces, SurfacePath::root("tools"), value),
                    "functions" => {
                        walk_all_scalars(&mut surfaces, SurfacePath::root("functions"), value)
                    }
                    "response_format" => {
                        walk_all_scalars(
                            &mut surfaces,
                            SurfacePath::root("response_format"),
                            value,
                        );
                    }
                    "prediction" => {
                        walk_all_scalars(&mut surfaces, SurfacePath::root("prediction"), value);
                    }
                    _ => {}
                }
            }
        }
        surfaces
    }

    fn response_pii_surfaces<'a>(&self, body: &'a mut Value) -> Vec<PiiSurface<'a>> {
        let mut surfaces = RequestSurfaces::default();
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
                                                SurfacePath::root("choices")
                                                    .index(index)
                                                    .member("message"),
                                                choice_value,
                                            ),
                                            "delta" => collect_message_surfaces(
                                                &mut surfaces,
                                                SurfacePath::root("choices")
                                                    .index(index)
                                                    .member("delta"),
                                                choice_value,
                                            ),
                                            "text" => push_string(
                                                &mut surfaces,
                                                SurfacePath::root("choices")
                                                    .index(index)
                                                    .member("text"),
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
        surfaces.strings
    }

    fn sse_event_pii_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<PiiSurface<'a>> {
        self.sse_event_stream_surfaces(event)
            .into_iter()
            .map(|fragment| fragment.surface)
            .collect()
    }

    /// Chat Completions chunks stream `delta.content`, `delta.tool_calls[].function.arguments`
    /// and legacy `text` per choice; the Responses API streams typed `response.*` events. Each
    /// fragment is keyed by the provider's own `index`, `output_index` and `content_index`
    /// fields: a chunk carries its choice and tool call at array position 0, so a position
    /// would merge parallel tool calls and alternative choices into one value.
    fn sse_event_stream_surfaces<'a>(&self, event: &'a mut SseEvent) -> Vec<SseStreamSurface<'a>> {
        let mut surfaces = Vec::new();
        let kind = event
            .data
            .get("type")
            .and_then(Value::as_str)
            .and_then(|kind| kind.strip_prefix("response."))
            .map(str::to_owned);
        if let Some(kind) = kind {
            collect_responses_event_surfaces(&mut surfaces, &kind, &mut event.data);
            return surfaces;
        }
        let Value::Object(root) = &mut event.data else {
            return surfaces;
        };
        for (key, value) in root {
            match key.as_str() {
                "choices" => collect_choice_stream_surfaces(&mut surfaces, value),
                "output" => {
                    let mut complete = RequestSurfaces::default();
                    collect_output_surfaces(&mut complete, value);
                    surfaces.extend(complete.into_iter().map(SseStreamSurface::complete));
                }
                _ => {}
            }
        }
        surfaces
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
fn collect_output_surfaces<'a>(surfaces: &mut RequestSurfaces<'a>, output: &'a mut Value) {
    let Value::Array(items) = output else {
        walk_all_scalars(surfaces, SurfacePath::root("output"), output);
        return;
    };
    for (index, item) in items.iter_mut().enumerate() {
        collect_output_item(surfaces, SurfacePath::root("output").index(index), item);
    }
}

/// Surfaces one Responses API output item.
fn collect_output_item<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: SurfacePath,
    item: &'a mut Value,
) {
    let kind = item.get("type").and_then(Value::as_str);
    let is_call = matches!(kind, Some("function_call" | "mcp_call"));
    let is_message = kind == Some("message");
    let Value::Object(fields) = item else {
        walk_all_scalars(surfaces, prefix, item);
        return;
    };
    for (key, value) in fields {
        let path = prefix.member(key);
        match key.as_str() {
            "arguments" if is_call && value.is_string() => {
                push_string(surfaces, path, value, SurfaceSyntax::Json);
            }
            "content" if is_message => collect_output_content(surfaces, path, value),
            _ => walk_all_scalars(surfaces, path, value),
        }
    }
}

fn collect_output_content<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: SurfacePath,
    content: &'a mut Value,
) {
    let Value::Array(parts) = content else {
        walk_all_scalars(surfaces, prefix, content);
        return;
    };
    for (index, part) in parts.iter_mut().enumerate() {
        collect_output_part(surfaces, prefix.index(index), part);
    }
}

/// Surfaces one content part of a Responses API `message` output item.
fn collect_output_part<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: SurfacePath,
    part: &'a mut Value,
) {
    let is_output_text = part.get("type").and_then(Value::as_str) == Some("output_text");
    let Value::Object(fields) = part else {
        walk_all_scalars(surfaces, prefix, part);
        return;
    };
    for (key, value) in fields {
        let path = prefix.member(key);
        if is_output_text && key == "text" && value.is_string() {
            push_string(surfaces, path, value, SurfaceSyntax::ModelOutput);
        } else {
            walk_all_scalars(surfaces, path, value);
        }
    }
}

/// Appends `complete` surfaces as fragments of `stream`, or as complete surfaces without one.
fn extend_stream<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    stream: Option<SseStreamKey>,
    complete: RequestSurfaces<'a>,
) {
    surfaces.extend(complete.into_iter().map(|surface| SseStreamSurface {
        stream: stream.clone(),
        surface,
    }));
}

/// Surfaces one Responses API streaming event; `kind` is its `type` without `response.`.
///
/// A `*.delta` event carries one fragment of a value, keyed by its output item and content
/// part. The `*.done`, `output_item.*` and `content_part.*` events and the `response`
/// snapshots carry whole values and restore alone. An event not listed here carries no
/// model-authored text the proxy restores, so its tokens stay pseudonymized.
fn collect_responses_event_surfaces<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    kind: &str,
    data: &'a mut Value,
) {
    let index = |field: &str| data.get(field).and_then(Value::as_u64);
    let output_index = index("output_index");
    let content_index = index("content_index");
    let summary_index = index("summary_index");
    let content_stream = |part: &str| {
        Some(SseStreamKey::new(
            format!("responses.content.{part}"),
            [output_index, content_index],
        ))
    };
    let (field, syntax, stream) = match kind {
        "output_text.delta" => ("delta", SurfaceSyntax::ModelOutput, content_stream("text")),
        "output_text.done" => ("text", SurfaceSyntax::ModelOutput, None),
        "refusal.delta" => ("delta", SurfaceSyntax::Text, content_stream("refusal")),
        "refusal.done" => ("refusal", SurfaceSyntax::Text, None),
        "reasoning_text.delta" => ("delta", SurfaceSyntax::Text, content_stream("reasoning")),
        "reasoning_text.done" | "reasoning_summary_text.done" => {
            ("text", SurfaceSyntax::Text, None)
        }
        "reasoning_summary_text.delta" => (
            "delta",
            SurfaceSyntax::Text,
            Some(SseStreamKey::new(
                "responses.summary.text",
                [output_index, summary_index],
            )),
        ),
        "function_call_arguments.delta" | "mcp_call_arguments.delta" => (
            "delta",
            SurfaceSyntax::Json,
            Some(SseStreamKey::new("responses.arguments", [output_index])),
        ),
        "function_call_arguments.done" | "mcp_call_arguments.done" => {
            ("arguments", SurfaceSyntax::Json, None)
        }
        _ => {
            collect_responses_snapshot_surfaces(surfaces, kind, data);
            return;
        }
    };
    let mut complete = RequestSurfaces::default();
    if let Some(value) = data.get_mut(field) {
        push_string(&mut complete, SurfacePath::root(field), value, syntax);
    }
    extend_stream(surfaces, stream, complete);
}

/// Surfaces the Responses API streaming events that carry whole output items, content parts or
/// the response itself.
fn collect_responses_snapshot_surfaces<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    kind: &str,
    data: &'a mut Value,
) {
    let mut complete = RequestSurfaces::default();
    match kind {
        "output_item.added" | "output_item.done" => {
            if let Some(item) = data.get_mut("item") {
                collect_output_item(&mut complete, SurfacePath::root("item"), item);
            }
        }
        "content_part.added" | "content_part.done" => {
            if let Some(part) = data.get_mut("part") {
                collect_output_part(&mut complete, SurfacePath::root("part"), part);
            }
        }
        "reasoning_summary_part.added" | "reasoning_summary_part.done" => {
            if let Some(part) = data.get_mut("part") {
                walk_all_scalars(&mut complete, SurfacePath::root("part"), part);
            }
        }
        "created" | "in_progress" | "queued" | "completed" | "incomplete" | "failed" => {
            if let Some(output) = data
                .get_mut("response")
                .and_then(|response| response.get_mut("output"))
            {
                collect_output_surfaces(&mut complete, output);
            }
        }
        _ => {}
    }
    extend_stream(surfaces, None, complete);
}

/// Surfaces the `choices` of a Chat Completions chunk. Streamed fragments are keyed by the
/// choice's `index` and, for tool calls, the call's `index`. Missing required indices disable
/// joining for every compatible stream, preserving only streams whose separation is known.
fn collect_choice_stream_surfaces<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    choices: &'a mut Value,
) {
    let Value::Array(choices) = choices else {
        return;
    };
    for (position, choice) in choices.iter_mut().enumerate() {
        let Value::Object(choice) = choice else {
            continue;
        };
        let choice_index = choice.get("index").and_then(Value::as_u64);
        for (key, value) in choice {
            let prefix = SurfacePath::root("choices").index(position).member(key);
            match key.as_str() {
                "delta" => collect_delta_stream_surfaces(surfaces, &prefix, choice_index, value),
                "message" => {
                    let mut complete = RequestSurfaces::default();
                    collect_message_surfaces(&mut complete, prefix, value);
                    extend_stream(surfaces, None, complete);
                }
                "text" => {
                    let mut fragment = RequestSurfaces::default();
                    push_string(&mut fragment, prefix, value, SurfaceSyntax::ModelOutput);
                    let stream = Some(SseStreamKey::new("choices.text", [choice_index]));
                    extend_stream(surfaces, stream, fragment);
                }
                _ => {}
            }
        }
    }
}

/// Surfaces one chunk's `delta`, keying its `content` and tool-call `arguments` fragments.
fn collect_delta_stream_surfaces<'a>(
    surfaces: &mut Vec<SseStreamSurface<'a>>,
    prefix: &SurfacePath,
    choice_index: Option<u64>,
    delta: &'a mut Value,
) {
    let Value::Object(map) = delta else {
        return;
    };
    for (key, value) in map {
        let mut fragment = RequestSurfaces::default();
        match key.as_str() {
            "content" if value.is_string() => {
                push_string(
                    &mut fragment,
                    prefix.member("content"),
                    value,
                    SurfaceSyntax::ModelOutput,
                );
                let stream = Some(SseStreamKey::new("choices.content", [choice_index]));
                extend_stream(surfaces, stream, fragment);
            }
            "tool_calls" => {
                let Value::Array(tool_calls) = value else {
                    continue;
                };
                for (position, tool_call) in tool_calls.iter_mut().enumerate() {
                    let call_index = tool_call.get("index").and_then(Value::as_u64);
                    let mut fragment = RequestSurfaces::default();
                    collect_tool_call_arguments(
                        &mut fragment,
                        &prefix.member("tool_calls").index(position),
                        tool_call,
                    );
                    let stream = Some(SseStreamKey::new(
                        "choices.tool_calls.arguments",
                        [choice_index, call_index],
                    ));
                    extend_stream(surfaces, stream, fragment);
                }
            }
            _ => {
                collect_message_field(&mut fragment, prefix, key, value);
                extend_stream(surfaces, None, fragment);
            }
        }
    }
}

fn collect_message_surfaces<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: SurfacePath,
    message: &'a mut Value,
) {
    let Value::Object(map) = message else {
        return;
    };
    for (key, value) in map {
        collect_message_field(surfaces, &prefix, key, value);
    }
}

fn collect_message_field<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: &SurfacePath,
    key: &str,
    value: &'a mut Value,
) {
    match key {
        "content" => push_text_blocks(
            surfaces,
            &prefix.member("content"),
            value,
            SurfaceSyntax::ModelOutput,
        ),
        "tool_results" => push_text_blocks(
            surfaces,
            &prefix.member("tool_results"),
            value,
            SurfaceSyntax::Text,
        ),
        // The participant name on a message. A person's name by definition, and read by the
        // model, so it is pseudonymized rather than rejected.
        "name" => push_string(surfaces, prefix.member("name"), value, SurfaceSyntax::Text),
        "tool_calls" => {
            if let Value::Array(tool_calls) = value {
                for (index, tool_call) in tool_calls.iter_mut().enumerate() {
                    collect_tool_call_arguments(
                        surfaces,
                        &prefix.member("tool_calls").index(index),
                        tool_call,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Surfaces `function.arguments` of one tool call: a serialized JSON object, in requests and
/// responses alike.
fn collect_tool_call_arguments<'a>(
    surfaces: &mut RequestSurfaces<'a>,
    prefix: &SurfacePath,
    tool_call: &'a mut Value,
) {
    if let Some(args) = tool_call
        .get_mut("function")
        .and_then(|function| function.get_mut("arguments"))
    {
        push_string(
            surfaces,
            prefix.member("function").member("arguments"),
            args,
            SurfaceSyntax::Json,
        );
    }
}
