# gaze-proxy

[![Crates.io](https://img.shields.io/crates/v/gaze-proxy.svg)](https://crates.io/crates/gaze-proxy)
[![docs.rs](https://docs.rs/gaze-proxy/badge.svg)](https://docs.rs/gaze-proxy)
[![License](https://img.shields.io/crates/l/gaze-proxy.svg)](https://github.com/CertaMesh/gaze#license)

`gaze-proxy` is the feature-gated HTTP proxy runtime for LLM SDK base-URL swaps.
It preserves each provider's native wire shape and uses adapters only to locate
PII-bearing fields before calling the supplied `gaze::Pipeline`.

## Cargo

```toml
[dependencies]
gaze-proxy = "0.12.0"
```

## Quickstart

```bash
cargo install gaze-cli --version 0.12.0
gaze proxy start
```

Then point SDKs at the daemon:

```bash
export OPENAI_BASE_URL=http://127.0.0.1:8787/v1
export ANTHROPIC_BASE_URL=http://127.0.0.1:8787
export GEMINI_BASE_URL=http://127.0.0.1:8787
```

The default config is written to `~/.config/gaze/proxy.toml` and includes:

```toml
bind = "127.0.0.1:8787"
session_ttl = "30m"
rulepack = "core"

[adapters.openai]
upstream = "https://api.openai.com/"

[adapters.anthropic]
upstream = "https://api.anthropic.com/"

[adapters.gemini]
upstream = "https://generativelanguage.googleapis.com/"
```

## Providers

- OpenAI: `POST /v1/chat/completions`, `/v1/completions`, `/v1/responses`
- Anthropic: `POST /v1/messages`
- Gemini: `POST /v1beta/models/*:{generateContent,streamGenerateContent,countTokens}`

Each adapter walks text, tool-call, tool-result, and function argument surfaces
in that provider's native JSON. The proxy does not transcode requests.

## Daemon Commands

```bash
gaze proxy serve
gaze proxy start
gaze proxy status
gaze proxy stop
gaze proxy restart
```

Pidfiles live under the platform local-data directory, never `/tmp`. Stale
pidfiles are revalidated with process liveness checks and cleaned before start.

`gaze proxy logs --follow` is also available for local daemon inspection.

## Security Notes

The strict Anthropic Messages profile rebuilds its outbound headers from a closed
allowlist: `content-type`, `x-api-key`, `anthropic-version`, and an optional,
explicitly allowlisted `anthropic-beta`. Unconfigured `Authorization`, bearer
credentials, cookies, and unknown SDK headers are accepted at ingress and
dropped. If an embedding host configures `Authorization` as local listener
authentication, it is a singleton consumed only by the trusted principal
resolver and is still never forwarded.

`AnthropicAdapter::new` is an ephemeral, single-request profile and rejects
`x-gaze-session-id`. Session continuity is an explicit builder/configuration
choice and then requires that header with a canonical lowercase UUIDv4 value.
The SDK base URL is the proxy root, while the only direct route is exactly
`POST /v1/messages`.

The listener does not become an access-control boundary merely because provider
credentials pass through it. Bind to loopback unless an explicit trusted
principal resolver protects a non-loopback listener. See the
[strict Anthropic Messages contract](../../docs/explanation/proxy/anthropic-messages-contract.md)
for the complete wire, proof, inspection, migration, and manual SDK-test
contract.
