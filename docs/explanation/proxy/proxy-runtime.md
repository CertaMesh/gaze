# Proxy Runtime

`gaze-proxy` is a pass-through-per-provider HTTP runtime. Its north-star role is
to keep PII out of provider calls while preserving each SDK's native request and
response shape.

## Adapter Contract

Adapters implement `ProviderAdapter`:

- `matches_path(method, path)` claims provider-native endpoints.
- `request_pii_surfaces(body)` returns mutable text leaves to redact before
  forwarding upstream.
- `response_pii_surfaces(body)` returns mutable text leaves to restore on the
  owner-visible response.
- `sse_event_pii_surfaces(event)` handles provider-native event payloads.
- `requests_json_output(request)` reports whether the request asked the model
  for JSON output. It defaults to `false`.

Adapters do not decide what is PII. They only describe where strings live; the
configured `gaze::Pipeline` and recognizer registry make detection decisions.

Each surface also declares its `SurfaceSyntax`, which decides how restore
writes a raw value back:

- `Text`: the raw value is written byte for byte.
- `Json`: the surface is a serialized JSON document, such as
  `tool_calls[].function.arguments` or a Responses `function_call` or
  `mcp_call` item's `arguments`. A token can only stand inside one of its
  string literals, so restore JSON-escapes the raw value. A verbatim `"`, `\`, or control character
  would break the document or change the value the agent parses. The escape
  does not depend on where the literal's quotes are, so it stays correct when a
  streamed fragment carries the token and a neighbouring SSE event carries the
  quotes. The one exception is a value the request carried inside a JSON string
  literal, such as a field of a JSON tool result. The manifest stores that value
  in its escaped spelling (`\"`, `\u00fc`), so restore writes it into a JSON
  document as it is. The legacy session records which tokens those are.
  Known limit: the manifest keeps only that escaped spelling, so the same value
  restores into a `Text` destination with its escapes (`\"`, `\u00fc`), not
  decoded. Recording the spelling per token is planned for v0.16.
- `ModelOutput`: answer text. It restores as `Json` when
  `requests_json_output` is true and as `Text` otherwise. OpenAI reads
  `response_format.type` (Chat Completions) or `text.format.type` (Responses)
  for `json_object` or `json_schema`. Gemini reads
  `generationConfig.responseMimeType` for `application/json`; thought summaries
  stay `Text`.

Only restore reads the syntax. Request protection scans each surface's text as
it stands.

## Provider Surface Matrix

| Provider | Request surfaces | Response surfaces | Streaming surfaces |
| --- | --- | --- | --- |
| OpenAI | `messages[].content`, `system`, `tool_calls[].function.arguments`, `input` | `choices[].message.content`, `choices[].message.tool_calls[].function.arguments`, `output` | `choices[].delta.content`, `choices[].delta.tool_calls[].function.arguments` |
| Anthropic | Strict, schema-aware Messages codec; see the [public contract](anthropic-messages-contract.md) | Strict, schema-aware Messages codec; see the [public contract](anthropic-messages-contract.md) | Strict lifecycle with proved replay; see the [public contract](anthropic-messages-contract.md) |
| Gemini | `contents[].parts[].text`, `functionCall.args`, `functionResponse.response`, `systemInstruction.parts[].text` | `candidates[].content.parts[].text`, `functionCall.args` | same parts shape per chunk |

The legacy OpenAI and Gemini adapters walk tool and function objects as native
JSON without provider-shape transcoding. The strict Anthropic direct profile is
different: it admits only its documented Messages schema, rejects unknown or
opaque media surfaces, and proves the complete transformed request or response.

## Safety Nets and Refusals

With a safety net configured, such as Nym in the policy `gaze setup` writes,
each surfaced request string goes through three steps before provider I/O:

1. The primary pipeline tokenizes what the rules detect.
2. The nets scan the result, and every span they flag becomes a restorable
   token. This is the same Resolve step `gaze clean` and `gaze daemon` run,
   through the same library function, so the proxy forwards what `gaze clean`
   would print for the same text and policy. A date that Nym flags as
   `DATE_OF_BIRTH` is forwarded as `<…:Custom:date_1>` and restored in the
   response.
3. Admission scans the final text once more and refuses any raw span a net
   still flags.

The proxy never deletes flagged bytes one way. Whatever step 2 cannot turn into
a token, and whatever step 3 still flags, is refused before anything reaches
the provider.

A refusal carries its reason, never the text:

- `error`: the `ProtectionError` variant: `Residual` (a net flagged raw bytes
  that could not be protected), `SafetyNet` (a net failed to run), `Primary`,
  `Provenance`, `UnsupportedCoverage` or `EmptyPrimary`.
- `fallback_reason`: set when step 2 refused: `residual_suspect` (a later scan
  flagged something new), `overlap_conflict`, `validator_veto` or
  `anchor_missing`. It is `null` when admission refused.
- `suspect_classes`: class names, deduplicated, in a stable order. Admission
  names the suspect it rejected. A step 2 refusal names every class the nets
  flagged in that string.

The legacy OpenAI and Gemini adapters answer `422 Unprocessable Entity`:

```json
{
  "error": "Refused",
  "refusal": {
    "error": "Residual",
    "fallback_reason": "residual_suspect",
    "suspect_classes": ["name", "location"]
  }
}
```

The Anthropic direct profile answers `422` with its usual error envelope and
the same `refusal` object:

```json
{
  "type": "error",
  "error": {
    "type": "api_error",
    "message": "proxy_validation_failed",
    "code": "ProtectionRefused",
    "phase": "RequestTransform",
    "refusal": {
      "error": "Residual",
      "fallback_reason": "residual_suspect",
      "suspect_classes": ["name", "location"]
    }
  }
}
```

Each refusal also writes one line to the proxy's stderr, which `gaze proxy
start` sends to its log file:

```text
gaze-proxy: request refused: {"error":"Residual","fallback_reason":"residual_suspect","suspect_classes":["name","location"]}
```

Without a configured net, steps 2 and 3 do nothing and the proxy forwards the
primary output as before.

## Anthropic Direct Sessions

`AnthropicAdapter::new` is intentionally ephemeral and single-request. It creates
an internal session for that request and rejects any `x-gaze-session-id` header;
it does not infer continuity from a supplied header.

Continuity is opt-in through the adapter builder or equivalent host
configuration. Once enabled, every request must carry `x-gaze-session-id` with
a canonical lowercase UUIDv4 value. Active mappings are bounded and held in
memory; expiry is reported as `SessionExpired` (`410`) and is not silently
recreated. See the [strict Anthropic Messages contract](anthropic-messages-contract.md)
for registry bounds, principal resolution, and the migration from legacy
header behavior.

## Daemon Lifecycle

`gaze proxy start` persists config, reexecs `gaze proxy serve --_foreground-daemon`,
and writes a pidfile:

```text
<pid>
bind=<addr>
started_at=<rfc3339>
```

The pidfile is UTF-8 and capped at 200 bytes. It lives in the platform local-data
directory:

- macOS: `~/Library/Application Support/gaze/proxy.pid`
- Linux: `$XDG_DATA_HOME/gaze/proxy.pid` or `~/.local/share/gaze/proxy.pid`
- Windows: `%LOCALAPPDATA%\gaze\proxy.pid`

Status and start always validate the recorded PID with process liveness checks.
If the PID is dead, the command treats the pidfile as stale and removes it
before continuing. Stop is signal-only: `SIGTERM`, bounded wait, optional
`SIGKILL` with `--force`.

The proxy exposes `/_gaze_proxy/healthz` for local health inspection. The path is
reserved outside all adapter-matched provider routes.
