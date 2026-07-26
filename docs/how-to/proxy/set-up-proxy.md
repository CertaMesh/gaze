# Gaze Proxy Setup

This page is an adopter setup guide for `gaze proxy`, the HTTP chokepoint for
API-key-authenticated SDK traffic. For the full runtime contract, see
[`docs/explanation/proxy/proxy-runtime.md`](../../explanation/proxy/proxy-runtime.md).
Anthropic adopters must also follow the
[strict Anthropic Messages contract](../../explanation/proxy/anthropic-messages-contract.md).

## When To Use

Use `gaze proxy` when an application, worker, or agent already calls OpenAI,
Anthropic, or Gemini through provider SDKs and you want a drop-in PII
pseudonymization boundary without changing application code.

The application keeps using its provider API key. The only SDK-side change is
the base URL. Requests flow through the local proxy, Gaze tokenizes PII before
the upstream call, and owner-visible response text is restored on the way back.

## Prerequisites

- A `gaze` binary on PATH. The default release binary includes proxy support as
  of v0.8.1.
- A policy TOML file on disk. See [`docs/reference/policy.md`](../../reference/policy.md) for policy
  authoring.
- An application or SDK that can override provider base URLs with environment
  variables or equivalent client options.
- Provider API keys for the upstream services your application already calls.

For a minimal policy, start with the same deterministic floor you use for
`gaze clean`:

```toml
[session]
scope = "ephemeral"

[rules]
emails = "tokenize"
```

## Start The Proxy

Start the daemon with your policy:

```sh
gaze proxy start --policy ./policy.toml
```

The default listener is:

```text
http://127.0.0.1:8787
```

`start` persists the daemon config, launches the foreground proxy process, and
writes a pidfile in the platform local-data directory. Use `serve` instead when
you want the proxy in the foreground for a process supervisor you already own:

```sh
gaze proxy serve --policy ./policy.toml --bind 127.0.0.1:8787
```

## Point Your SDK At The Proxy

OpenAI SDKs usually expect `/v1` in the base URL:

```sh
export OPENAI_API_KEY=sk-test-api-key
export OPENAI_BASE_URL=http://127.0.0.1:8787/v1
```

Anthropic SDKs usually expect the provider root:

```sh
export ANTHROPIC_API_KEY=sk-ant-test-api-key
export ANTHROPIC_BASE_URL=http://127.0.0.1:8787
```

Do not append `/v1`: the strict Anthropic client base URL is the proxy root and
the SDK must issue exactly `POST /v1/messages`. The direct constructor is
ephemeral and rejects `x-gaze-session-id`. If the embedding host explicitly
enables session continuity, send `x-gaze-session-id` on every request with a
canonical lowercase UUIDv4 value. The proxy requires `x-api-key` and
`anthropic-version`; the default version allowlist contains only `2023-06-01`.
`anthropic-beta` is denied until its complete value is explicitly allowlisted.

Only `content-type`, `x-api-key`, `anthropic-version`, and the optional
allowlisted beta header can reach the Anthropic upstream. Unconfigured
`Authorization`, cookies, and other SDK headers are dropped. A configured local
`Authorization` credential is consumed as singleton principal input and is
also never forwarded.

Gemini clients use the Google API key and a Gemini base URL override:

```sh
export GOOGLE_API_KEY=test-google-api-key
export GEMINI_BASE_URL=http://127.0.0.1:8787
```

Now run the application the same way you did before. Text such as
`alice@example.invalid` is tokenized before the upstream provider sees it, then
restored for the owner-visible response path.

## Verify

Check the daemon state:

```sh
gaze proxy status
```

Expected shape:

```text
gaze-proxy running (pid=12345, bind=127.0.0.1:8787)
  adapters: openai -> https://api.openai.com/
            anthropic -> https://api.anthropic.com/
            gemini -> https://generativelanguage.googleapis.com/
```

Inspect logs:

```sh
gaze proxy logs
gaze proxy logs --follow
```

For a local health check, call the reserved proxy endpoint:

```sh
curl http://127.0.0.1:8787/_gaze_proxy/healthz
```

## Lifecycle

Stop the daemon:

```sh
gaze proxy stop
```

Restart it with the persisted config:

```sh
gaze proxy restart
```

Use a bounded stop window when a deployment needs one:

```sh
gaze proxy stop --timeout 30s
gaze proxy restart --timeout 30s
```

The CLI also exposes supervisor install hooks:

```sh
gaze proxy install-launchd
gaze proxy install-systemd-user
```

Those hooks are reserved for the platform integration path. Today they return a
typed message directing you to `gaze proxy start` and `gaze proxy stop`.

## Out Of Scope

`gaze proxy` covers provider API traffic authenticated by API keys, such as
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and `GOOGLE_API_KEY`.

Consumer subscription tiers are not part of this surface:

- ChatGPT Plus
- Claude.ai
- Gemini Advanced

Those products use browser sessions, cookie auth, and web endpoints instead of
provider SDK base URLs. They are outside the public proxy contract documented
here.

## Five-Axis Pitch

- Reliability: request text is tokenized before provider transit, including
  SSE deltas and tool-call JSON surfaces.
- Reversibility: responses restore through the active Gaze session manifest,
  not ad hoc string replacement.
- Agentic-first: base-URL swaps fit SDK agents, workers, and local automation
  without rewriting call sites.
- Trust: provider adapters only identify text surfaces; detection remains in
  the configured Gaze pipeline.
- Adopter ergonomics: one local daemon plus provider base URL overrides is
  enough for the common API-key path.

## Next Steps

- [`docs/explanation/proxy/proxy-runtime.md`](../../explanation/proxy/proxy-runtime.md) —
  adapter matrix, session TTL, and daemon lifecycle.
- [`docs/explanation/proxy/anthropic-messages-contract.md`](../../explanation/proxy/anthropic-messages-contract.md) —
  strict Anthropic setup, wire surfaces, limits, errors, inspection boundary,
  migration, and manual official-SDK gate.
- [`crates/gaze-proxy/README.md`](../../../crates/gaze-proxy/README.md) — crate
  README and provider endpoint list.
- [`docs/reference/cli.md#gaze-proxy`](../../reference/cli.md#gaze-proxy) — CLI guide and flag
  summary.
