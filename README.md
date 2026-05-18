# Gaze

[![Crates.io](https://img.shields.io/crates/v/gaze-pii.svg)](https://crates.io/crates/gaze-pii) [![License](https://img.shields.io/crates/l/gaze-pii.svg)](https://github.com/EmpireTwo/gaze#license) [![docs.rs](https://docs.rs/gaze-pii/badge.svg)](https://docs.rs/gaze-pii) [![Tests](https://github.com/EmpireTwo/gaze/actions/workflows/test.yml/badge.svg)](https://github.com/EmpireTwo/gaze/actions/workflows/test.yml) [![GitHub stars](https://img.shields.io/github/stars/EmpireTwo/gaze?style=social)](https://github.com/EmpireTwo/gaze/stargazers)

**Reversible PII pseudonymization for agentic LLM workflows.**

Your agent never sees a real email, phone number, or order ID. Your server keeps the only manifest that can read those tokens back. Detection is regex, validator, and locale-cue driven — every emitted token traces to a versioned recognizer, not to a second model's opinion of what was sensitive.

## In production: AI support drafts that never see the customer

[`EmpireTwo/gaze-ghostwriter`](https://github.com/EmpireTwo/gaze-ghostwriter) is a Laravel package that watches a support inbox over IMAP and drafts replies with an LLM. The application does the data lookup. Gaze pseudonymizes the resulting context. The LLM only composes prose.

```text
1. Customer email arrives via IMAP:
   "Hi Support, I'm Alice Schmidt, order #INV-2026-04-1872,
    my refund of €128.40 hasn't shown up..."
       ↓
2. App parses email → extracts identifiers:
   sender=customer@..., order_id=INV-2026-04-1872, amount=€128.40
       ↓
3. App looks up order in DB (real PII, no LLM involved):
   order #INV-2026-04-1872 → refund processed 2026-05-12,
   customer = Alice Schmidt
       ↓
4. App builds context bundle (still real PII):
   { name: "Alice Schmidt", order_id: "INV-2026-04-1872",
     amount: "€128.40", refund_processed: "2026-05-12",
     issue: "delayed refund" }
       ↓
5. gaze clean — pseudonymizes the bundle:
   { name: "<Name_1>", order_id: "<OrderId_1>",
     amount: "<Amount_1>", refund_processed: "<Date_1>",
     issue: "delayed refund" }
   + per-session manifest stored
       ↓
6. LLM drafts reply (sees only tokens + facts):
   "Hi <Name_1>, your refund of <Amount_1> for order <OrderId_1>
    was processed on <Date_1>. Please allow 3-5 business days to
    appear on your statement."
       ↓
7. gaze restore rehydrates draft:
   "Hi Alice Schmidt, your refund of €128.40 for order
    #INV-2026-04-1872 was processed on 2026-05-12. Please allow
    3-5 business days to appear on your statement."
       ↓
8. Support agent reviews → approves → reply sent.

LLM never saw "Alice Schmidt", "#INV-2026-04-1872", "€128.40", "2026-05-12".
App owns the lookup. gaze owns the manifest. LLM owns the prose.
Each layer's role is what it is built for.
```

`OrderId` and refund-amount shapes are tenant-specific custom recognizers in the host policy; email, names, IBAN, phone, postal, and credit-card shapes come from the bundled `core` rulepack.

### Try the loop

- Drop-in Laravel package: [`EmpireTwo/gaze-ghostwriter`](https://github.com/EmpireTwo/gaze-ghostwriter)

Agentic workflows (browser automation, tool execution) hook the same restore boundary at tool-call args, on the same manifest contract — the agent stays on tokens end-to-end.

CLI surface (`gaze clean`, `gaze restore`, audit, policy TOML): [Quickstart](#quickstart), [`gaze-cli` README](crates/gaze-cli/README.md).

## Why this exists

PII in agent workflows usually falls into one of three failure modes:

1. **No redaction.** Real emails, phone numbers, and order IDs end up in the model provider's logs.
2. **One-way redaction.** PII is stripped, the agent replies "I've sent the confirmation to `<REDACTED>`", and you have no way to thread the reply back to the actual customer.
3. **LLM-judged redaction.** A second model call decides what's PII. Non-deterministic, non-auditable, costs another round trip every turn.

Gaze is the fourth path: deterministic detection, signed restore manifest, every token traced to a versioned recognizer.

## What ships

Each feature, what you get, where the proof lives.

- **Multi-provider HTTP proxy with a daemon.** `gaze proxy start` puts a PII chokepoint in front of **API-key-authenticated** traffic to OpenAI's `/v1/chat/completions`, Anthropic's `/v1/messages`, and Gemini's `/v1beta/models/*:{generateContent,streamGenerateContent}` — i.e. when an SDK or agent authenticates with `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GOOGLE_API_KEY`. Consumer subscription tiers (ChatGPT Plus, Claude.ai, Gemini Advanced) route through web endpoints with cookie auth and are out of scope for this proxy; a separate browser-MITM project will cover that surface when it is public. SSE streams and tool-call argument JSON are accumulated chunk-by-chunk before redaction. Subcommands `serve`, `start`, `stop`, `status`, `logs`, `restart`, plus opt-in `install-launchd` / `install-systemd-user`. See [`crates/gaze-proxy/README.md`](crates/gaze-proxy/README.md).
- **OSS document ingestion.** `gaze document clean ./input.pdf --out ./safe-bundle/` OCRs PNG/JPG/PDF through Tesseract, runs the recognized text through the standard pipeline, and writes a `SafeBundle` — `clean.md` + `manifest.json` + `report.json`. Layout report v2 surfaces per-page OCR confidence, multi-column segmentation, table-cell preservation, and vector-PDF fallback when PDFs have selectable text. Plug in alternative OCR drivers via the `OcrBackend` trait. Adopter quickstart: [`docs/getting-started/document-workflow.md`](docs/getting-started/document-workflow.md). Full bundle contract: [`docs/architecture/document-extension.md`](docs/architecture/document-extension.md).
- **Long-lived stdio server for repeated redaction.** `gaze daemon` keeps one pipeline and model load hot, then serves JSON-per-line requests with per-`session_id` manifest isolation. It avoids binary/model cold starts on every agent turn, exits gracefully on SIGTERM, and evicts sessions by LRU or idle timeout. Adopter quickstart: [`docs/getting-started/daemon-adapter.md`](docs/getting-started/daemon-adapter.md). Full contract: [`docs/architecture/daemon-mode.md`](docs/architecture/daemon-mode.md).
- **Reversible by contract.** Tokens are session-scoped, counted per class (`Email_1`, `Email_2`), and only resolvable through a signed `SensitiveSnapshot`. There is no string-map fallback. Manifests written by an older minor restore on a newer minor — see the reversibility statement at the bottom of [`UPGRADE.md`](UPGRADE.md).
- **Defense in depth, observer-only.** Regex, dictionary, and optional NER form the detection floor. Pass-3 SafetyNet runs *after* tokenization, against the already-clean text plus the manifest, and can flag suspect bytes the rules missed — but it cannot mutate the clean output or the manifest. Two backends ship: the OpenAI Privacy Filter and the Apache-2.0 Kiji DistilBERT bundle (26 PII classes, ~8.8 MB). Contract: [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md).
- **Every token is auditable.** Each emission carries a `recognizer_id` plus `recognizer_version_id` (suffixed `_vN`) into the optional SQLite audit log. Pre-v0.8 rows surface as `legacy_unversioned`. The export column set never includes raw PII payloads.
- **10 validator-backed national IDs across 5 locale packs, 3 locale-gated regex IDs.** Aadhaar (Verhoeff), NIR (MOD-97 variant), Steuer-ID (MOD 11,10), BSN (MOD-11), CPF + CNPJ (MOD-11), NHS (MOD-11), US SSN, UK NINO, Indian PAN. Adopters in BR / FR / NL / IN / UK / US get coverage with one `--locale` flag. Full table in [Detection coverage](#detection-coverage).
- **Agentic shapes are first-class.** Tool-call JSON arguments, SSE-streamed deltas, multi-turn sessions with evolving manifest state, and structured documents (PNG / JPG / PDF → Tesseract → `SafeBundle`) all redact correctly. The MCP runtime in [`gaze-mcp-core`](crates/gaze-mcp-core/) puts the same chokepoint between agent tool calls and source systems.
- **Fail closed everywhere.** Ambiguous matches are tokenized, never silently passed. Unknown validators or normalizers fail at policy load — no degraded mode. Strict-mode SafetyNet exits `3` with `{"error":"SafetyNet","exit":3,"variant":"SuspectedLeak"}` and stdout stays empty.

## How it fits your stack

Three execution layers, one core invariant: PII crosses the agent boundary only as manifest-backed tokens.

```text
  Direct library          MCP source chokepoint        HTTP proxy in front of LLM

  Application code        Agent tool call              SDK / agent request
        │                       │                            │
        ▼                       ▼                            ▼
  gaze::Pipeline          gaze-mcp-rmcp transport       gaze-proxy provider driver
        │                       │                            │
        ▼                       ▼                            ▼
  owner-controlled        gaze-mcp-core dispatch        OpenAI / Anthropic / Gemini
  manifest + restore            │
                                ▼
                          source system call
```

- **Library** — link `gaze-pii` and own the data path. Use when your app already controls the LLM call.
- **MCP chokepoint** — every agent tool call passes through `PiiEnvelope::dispatch` before reaching its source. Use when your agent host already speaks MCP and you want one redaction boundary across many tools.
- **Proxy** — SDK base-URL swap, API-key path only. Use when the agent is a hosted product or vendor SDK that talks to `api.openai.com` / `api.anthropic.com` / `generativelanguage.googleapis.com` with an API key, and you cannot link a library or rewrite its tool layer. Subscription-tier web clients are out of scope (a separate browser-MITM project covers that surface).

Architecture overview with eight Key Design Decisions: [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Install

```sh
git clone https://github.com/EmpireTwo/gaze.git
cd gaze
cargo install --path crates/gaze-cli
```

Pre-built binaries for Apple Silicon macOS and Linux x86_64 (glibc 2.39+) are attached to each [GitHub release](https://github.com/EmpireTwo/gaze/releases). Other targets: `cargo build --release -p gaze-cli`.

For the LLM API proxy:

```sh
cargo install --path crates/gaze-cli
gaze proxy start
export OPENAI_BASE_URL=http://127.0.0.1:8787/v1
export ANTHROPIC_BASE_URL=http://127.0.0.1:8787
```

For MCP hosts (Claude Code, Claude Desktop, Cursor):

```sh
cargo install --path crates/gaze-cli --features mcp
gaze mcp install --client=claude-code
gaze mcp doctor
```

The MCP server exposes `gaze_read_file` and `gaze_read_text`, returning tokenized content plus a `manifest_id` for authorized restore flows. Client config paths: [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#mcp-installation).

For library use, see [Use from Rust](#use-from-rust) below.

## Quickstart

A guided path from zero PII configuration to a working clean run, with optional NER and the observer-only SafetyNet layered on top. Each step is copy-paste-able against the current `gaze` CLI.

### 1. First redact

Write the smallest policy that drives the bundled `core` rulepack and tokenizes emails:

```toml
# quickstart-policy.toml
schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "default"
action = "preserve"
```

Run `gaze clean` against it:

```sh
printf '%s' 'Contact alice@example.invalid for details.' \
  | gaze clean --policy quickstart-policy.toml
```

The output is JSON. `clean_text` is the only field that may reach the LLM; `session_blob` is the signed restore manifest and must never leave the server:

```json
{
  "clean_text": "Contact <{session_hex}:Email_1> for details.",
  "session_blob": "<base64>",
  "stats": {"detections": 1, "locale_chain": ["global"], "dictionaries_loaded": []}
}
```

Round-trip through restore to recover the original on the same manifest:

```sh
printf '{"session_blob":"<base64>","text":"Re: <{session_hex}:Email_1>"}' \
  | gaze restore
```

```json
{"text": "Re: alice@example.invalid"}
```

Schema and every rule kind / action live in [`docs/policy.md`](docs/policy.md).

### 2. Add NER

NER is opt-in and stacks on top of the deterministic regex and dictionary passes. Turn it on when the input has free-prose names that the cue-anchored Name recognizer in `core` does not cover.

Fetch the pinned mBERT bundle once:

```sh
bash scripts/fetch-ner-model.sh
```

The script verifies a release-pinned `SHA256SUMS.ner` and installs the artifact set into `${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl` (pass a directory argument to override). No model is downloaded at `gaze clean` runtime — Gaze only consumes the on-disk bundle.

Add the `[ner]` block to `quickstart-policy.toml` and a rule for the `name` class:

```toml
[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"
threshold = 0.3

[[rule]]
kind = "class"
class = "name"
action = "tokenize"
```

Re-run on free-prose German with a Name span the rule-based passes leave alone:

```sh
printf '%s' 'Bitte richten Sie es Dr. Erika Müller aus.' \
  | gaze clean --policy quickstart-policy.toml
```

NER contributes a `Name_*` span via the model's `PER` label:

```json
{
  "clean_text": "Bitte richten Sie es <{session_hex}:Name_1> aus.",
  "session_blob": "<base64>",
  "stats": {"detections": 1, "locale_chain": ["de-DE", "global"], "dictionaries_loaded": []}
}
```

Schema details, threshold range, and `~/` expansion rules: [`docs/policy.md`](docs/policy.md#ner-optional). Pinned artifact contract and adopter label map: [`crates/gaze/testdata/ner/README.md`](crates/gaze/testdata/ner/README.md) plus [`assets/ner/labels.davlan-mbert.json`](assets/ner/labels.davlan-mbert.json).

### 3. Add a SafetyNet (Pass-3 observer)

The SafetyNet is an **observer-only post-clean check**. It reads the already-tokenized text plus the manifest of emitted spans and reports any suspect bytes the deterministic passes missed. It cannot mutate the clean text, cannot mutate the manifest, and cannot affect restore — full contract in [`docs/architecture/safety-nets.md`](docs/architecture/safety-nets.md).

Two backends ship. `openai-filter` wraps the upstream OpenAI Privacy Filter and is the heavier option when that infrastructure is already approved. `kiji-distilbert` is the lighter alternative: an Apache-2.0 ONNX DistilBERT bundle, ~8.8 MB, 26-class upstream PII taxonomy, faster cold start. Pick on deployment constraints; both are observer-only and both run under the **`resolve` mode default with a `redact` fallback** — the reversibility-preserving production posture (see below).

#### OpenAI Privacy Filter

The safety-net code path is off the default build graph. Reinstall the CLI with the OpenAI backend compiled in:

```sh
cargo install --path crates/gaze-cli --features safety-net-openai
```

Install the upstream [`openai/privacy-filter`](https://github.com/openai/privacy-filter) `opf` binary and a checkpoint per its instructions. Gaze does not download or update either — bring-your-own-binary plus bring-your-own-weights is the contract. The checkpoint directory must be owned by the running user with mode `0700`.

Activate the filter on the same `gaze clean` invocation:

```sh
printf '%s' 'Contact alice@example.invalid for details.' \
  | gaze clean \
      --policy quickstart-policy.toml \
      --safety-net openai-filter \
      --openai-filter-command /opt/opf/bin/opf \
      --openai-filter-checkpoint /opt/opf/checkpoint \
      --openai-filter-device auto
```

`--openai-filter-device` accepts `auto` (default; the upstream `opf` picks), `cpu`, `cuda`, or `mps`.

A clean run produces a `leak_report` block alongside the usual JSON; `suspect_count = 0` is the contract for "no leaks":

```json
{
  "clean_text": "Contact <{session_hex}:Email_1> for details.",
  "session_blob": "<base64>",
  "stats": {"detections": 1},
  "leak_report": {
    "stats": {
      "suspect_count": 0,
      "uncovered_count": 0,
      "partial_bleed_count": 0,
      "class_mismatch_count": 0,
      "locale_skipped_count": 0
    }
  }
}
```

SafetyNet runs in **`resolve` mode by default** with a **`redact` fallback**. When the filter raises an `Uncovered` or `PartialBleed` suspect, Gaze first promotes the suspect into a synthetic custom-recognizer match and re-runs the resolver so the span can be tokenized into the manifest — preserving reversibility. If `resolve` cannot honor a suspect (validator-veto, missing anchor, or a residual suspect after the one-shot pass), the composable `--safety-net-fallback {strict|tolerant|redact}` flag (default `redact`) decides what happens next: by default the suspect span is overwritten with a sentinel string, the redaction is recorded in the audit trail, and the rest of the clean text continues to stdout. **The reversibility-first default is the production contract**: every suspect either becomes a fully restorable manifest token or is stripped before reaching the LLM, and every action emits a typed audit row.

Adopters who want the v0.7.x hard-fail posture can opt in with `--safety-net-mode strict` (any suspect exits `3`, stdout stays empty). Adopters who cannot afford the resolve pass can skip directly to strip-and-continue with `--safety-net-mode redact`. A `tolerant` mode exists for **local development only** — while debugging recognizer coverage or measuring SafetyNet recall, it downgrades suspects to a stderr warning instead of refusing the output. **Do not use `tolerant` in production traffic.** A tolerant-mode pipeline is one that has agreed to ship suspected leaks. Mode catalog, fallback composition matrix, and exit-code map: [`docs/architecture/safety-net-modes.md`](docs/architecture/safety-net-modes.md) and [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#safety-net).

#### Kiji DistilBERT

The Kiji backend is also feature-gated. Fetch the pinned model bundle once, then reinstall the CLI with the Kiji feature compiled in:

```sh
bash scripts/fetch-kiji-safetynet-model.sh
cargo install --path crates/gaze-cli --features safety-net-kiji
```

The fetcher verifies the release-pinned `SHA256SUMS.kiji` file and installs the runtime bundle into `${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert` by default. Gaze does not fetch or update the model during `gaze clean`.

Activate Kiji on the same `gaze clean` invocation:

```sh
printf '%s' 'Contact alice@example.invalid for details.' \
  | gaze clean \
      --policy quickstart-policy.toml \
      --safety-net kiji-distilbert \
      --safety-net-backend kiji-distilbert \
      --kiji-distilbert-command /opt/kiji/bin/kiji \
      --kiji-distilbert-model-dir ~/.local/share/gaze/models/kiji-distilbert
```

The output shape is the same `leak_report` block shown above; `suspect_count = 0` remains the contract for "no leaks". The Kiji model directory must contain `SHA256SUMS`, `labels.json`, `model.onnx`, and `tokenizer.json`. Missing artifacts fail closed before subprocess spawn with `{"error":"SafetyNetArtifactMissing","exit":2,...}`.

Full Kiji setup, backend switching, and failure-mode notes: [`docs/getting-started/kiji-safetynet-setup.md`](docs/getting-started/kiji-safetynet-setup.md).

## Pipeline shape

```text
                       regex (always-on)  ─┐
                       dictionary (opt-in) ├──► resolver ──► tokens ──► CleanDocument
                       NER (opt-in)        ─┘     │
                                                  │  conflict tiers:
                                                  │  class > rule > score > length > id
                                                  │
                                                  ├──► Pass-3 SafetyNet (observer)
                                                  │    reads clean text + manifest
                                                  │    emits LeakReport, never mutates
                                                  │
                                                  └──► SensitiveSnapshot (signed)
                                                              │
                                                              ▼
                                                          restore
```

Three deterministic detection passes plus an optional observer pass. The safety net cannot modify the clean text or the restore path; it only emits suspect reports against the manifest of emitted tokens.

## Detection coverage

All bundled detectors ship in the unified `core` rulepack. Activation is encoded in a closed `safety_tier` enum:

- **safe_default** — active whenever the bundle loads.
- **locale_gated** — active only when the resolved locale matches `recognizer.locales`.
- **opt_in** — active only when explicitly named under `[[policy.custom_recognizers]]`.

| Class | Locale | Validator | Tier |
|---|---|---|---|
| Email | global | RFC | safe_default |
| Phone (E.164) | global | parser (`phone-parser` feature) | safe_default |
| IPv4 / IPv6 | global | parser | safe_default |
| IBAN | global | MOD-97 | safe_default |
| Credit card | global | Luhn | safe_default |
| Ethereum address | global | EIP-55 | safe_default |
| Aadhaar | IN | Verhoeff | safe_default |
| NIR | FR | MOD-97 variant | safe_default |
| Steuer-ID | DE | MOD 11,10 | safe_default |
| BSN | NL | MOD-11 | safe_default |
| CPF | BR | MOD-11 | safe_default |
| CNPJ | BR | MOD-11 | safe_default |
| NHS number | UK | MOD-11 | safe_default |
| Name (cue-anchored) | DE, EN | locale cue buckets | safe_default |
| Phone (national) | DE, US | parser + locale | locale_gated |
| Postal code | DE, US | regex + locale | locale_gated |
| US SSN | US | cue + regex | locale_gated |
| UK NINO | UK | cue + regex | locale_gated |
| Indian PAN | IN | cue + regex | locale_gated |

Validator names are a closed enum; unknown names fail at rulepack load with a typed `RulepackError`. The locale chain is strict and ordered: CLI > policy > rulepack default > system default.

Tenant-specific PII — order IDs, song titles, artist names — needs a dictionary or custom regex recognizer. See [`docs/policy.md`](docs/policy.md).

## Audit and restore

Restore is manifest-first. Tokens are session-scoped, counted by class, and only resolvable through a signed `SensitiveSnapshot`. There is no string-map fallback.

Optional metadata audit log:

```sh
gaze clean --policy policy.toml --audit-db audit.sqlite < input.txt
gaze audit query --audit-db audit.sqlite --class email --action tokenize
gaze audit export --audit-db audit.sqlite --format jsonl --output redactions.jsonl
gaze audit purge --audit-db audit.sqlite --before 2026-01-01T00:00:00Z
```

The audit DB is opened read-only by `query` and `export`. The exported column set excludes raw PII payloads. Every row carries `recognizer_id` plus `recognizer_version_id` for lineage; pre-v0.8 rows carry a `legacy_unversioned` marker. There is no policy-level retention default and no background auto-purge — adopters drive retention explicitly.

## Limits

- Detection floor is regex + validator + locale cue. Tenant-specific PII needs a custom recognizer.
- Linux x86_64 binaries link against glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). Older distros: build from source.
- No Intel macOS, no musl, no Windows binaries today. Build from source.
- v0.9 NER model leaderboard: [`docs/research/v0.9-ner-model-leaderboard.md`](docs/research/v0.9-ner-model-leaderboard.md). Kiji DistilBERT (Apache-2.0) ships as default per the leaderboard's strategic read.
- SafetyNet benchmark cells for Kiji DistilBERT and OpenAI Privacy Filter direct-detector mode are populated in [`docs/research/v0.9-safety-net-benchmark.md`](docs/research/v0.9-safety-net-benchmark.md); observer-residual mode remains deferred.
- `gaze-proxy` ships OpenAI / Anthropic / Gemini adapters. Certificate management, PAC mode, Electron integration, and transparent MITM are out of scope here — those belong in a separate browser-MITM project, not the core proxy.

## Use from Rust

The CLI is a process boundary around the Rust runtime; you can link the runtime directly:

```toml
[dependencies]
gaze-pii = "0.9.0"
gaze-assembly = "0.9.0"
```

The crate is published as `gaze-pii` because the bare `gaze` name is in transfer on crates.io; the import path stays `use gaze::...` because `[lib].name = "gaze"` is preserved.

- Minimal example and the API surface table: [`crates/gaze/README.md`](crates/gaze/README.md) (also rendered on [crates.io/crates/gaze-pii](https://crates.io/crates/gaze-pii)).
- Full walk-through with structured documents, tenant-specific recognizers, and policy TOML: [`docs/getting-started.md`](docs/getting-started.md).

## Workspace and crates.io

Ten published crates. Pick the smallest surface that does the job.

| Crate | Use when |
|---|---|
| [`gaze-pii`](https://crates.io/crates/gaze-pii) (lib name `gaze`) | You link the runtime: `Pipeline`, `Session`, `Policy`, `Recognizer`, restore. |
| [`gaze-types`](https://crates.io/crates/gaze-types) | You want the value contracts (`RedactionLogger`, `Manifest`, `LeakReport`) without ML deps. |
| [`gaze-recognizers`](https://crates.io/crates/gaze-recognizers) | You're writing a custom recognizer or rulepack, or you want the bundled detectors and SafetyNet backends. |
| [`gaze-audit`](https://crates.io/crates/gaze-audit) | You want SQLite-backed metadata audit logging. `gaze` core has no `rusqlite` dep in any feature graph. |
| [`gaze-assembly`](https://crates.io/crates/gaze-assembly) | You want bundled defaults without hand-wiring recognizers. |
| [`gaze-cli`](https://crates.io/crates/gaze-cli) | You want a process boundary for non-Rust adapters (Laravel, Python). |
| [`gaze-document`](https://crates.io/crates/gaze-document) | You want PNG / JPG / PDF ingestion into `SafeBundle`s or MCP document tools. |
| [`gaze-mcp-core`](https://crates.io/crates/gaze-mcp-core) | You're building an MCP tool host and want every call to pass through Gaze's chokepoint. |
| [`gaze-mcp-rmcp`](https://crates.io/crates/gaze-mcp-rmcp) | You want the rmcp transport sink for `gaze-mcp-core` (stdio default, opt-in streamable HTTP). |
| [`gaze-proxy`](https://crates.io/crates/gaze-proxy) | You want an HTTP proxy in front of API-key traffic to OpenAI / Anthropic / Gemini (not consumer subscription tiers — those need a separate browser-MITM project), daemon-managed via `gaze proxy`. |

```sh
cargo add gaze-pii
```

Crate boundaries and the audit-isolation Dylint gate: [`docs/architecture/crates.md`](docs/architecture/crates.md). Document codec extension: [`docs/architecture/document-extension.md`](docs/architecture/document-extension.md).

## Publishing

The workspace publishes via the `publish-crates.yml` GitHub Actions workflow using crates.io trusted-publisher OIDC auth; it does not need a long-lived `CARGO_REGISTRY_TOKEN` secret.

- **Tag push** (`git tag v<version> && git push --tags`) runs a real publish on every workspace crate in topological order.
- **Manual dispatch** with `dry_run=true` packages each crate without publishing, useful for catching metadata or dependency issues before a release tag.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Repository gates (xtask + Dylint) enforce the contracts in [`docs/architecture/`](docs/architecture/). Run them locally before pushing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- ci-feature-matrix
```

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
