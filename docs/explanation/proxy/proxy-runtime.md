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

Adapters do not decide what is PII. They only describe where strings live; the
configured `gaze::Pipeline` and recognizer registry make detection decisions.

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
If the PID is dead, the pidfile is stale and removed before continuing. Stop is
signal-only: `SIGTERM`, bounded wait, optional `SIGKILL` with `--force`.

The proxy exposes `/_gaze_proxy/healthz` for local health inspection. The path is
reserved outside all adapter-matched provider routes.
