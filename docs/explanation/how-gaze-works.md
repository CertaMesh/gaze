# How Gaze works

Gaze, explained for the person who owns it: one document followed from input to restore, in plain English, then the details. Start with the [project README](../../README.md) if you only want the short version; this page repeats its opening and adds everything below it.

## The promise in one picture

![Your app sends a ticket to Gaze on your server. Gaze swaps personal details for placeholders and keeps the manifest at home. Only placeholders reach the AI model. Gaze restores the real details in the reply.](../../docs/assets/gaze-promise-loop.svg)

*Text version:* your app → Gaze swaps details for placeholders (the manifest stays on your server) → the AI model reads and writes placeholders only → Gaze restores the real details → your app.

This is pseudonymization, not deletion. The model never needs to know who the customer is, only the shape of the task; your app puts the person back before anything leaves.

## A real support ticket, start to finish

A support agent asks the model: *"Draft a short reply confirming the refund."* The app attaches the ticket. The customer and every value are synthetic:

```text
Ticket #48213 from Laura Meyer <laura.meyer@example.com>, phone +49 30 5550 1234:
I sent back the headphones from order 2026-4471 two weeks ago and still have no refund.
Please pay it to my account DE89 3704 0044 0532 0130 00.
Address: Lindenstraße 8, 10115 Berlin.
```

**1. What the model receives** (real `gaze clean` output; the only edit is the per-session prefix, shortened from `<a37823d4:Name_1>` to `<Name_1>`):

```text
Ticket #<Custom:postal_code_1> from <Name_1> <<Email_1>>, phone <Custom:phone_1>:
I sent back the headphones from order 2026-4471 two weeks ago and still have no refund.
Please pay it to my account <Custom:family:payment-card-or-iban_1>.
Address: <Location_1> 8, <Custom:postal_code_2> <Location_2>.
```

The manifest, the list that turns placeholders back into values, stays on your server.

**2. What the model replies**, written with the placeholders it was given:

```text
Dear <Name_1>,

thank you for your patience. We received the headphones from order 2026-4471
and issued your refund today to the account
<Custom:family:payment-card-or-iban_1>.
It should arrive within 3 to 5 business days.
A confirmation is on its way to <Email_1>.

Best regards,
Support team
```

**3. What your app sends** after `gaze restore` (real output):

```text
Dear Laura Meyer,

thank you for your patience. We received the headphones from order 2026-4471
and issued your refund today to the account
DE89 3704 0044 0532 0130 00.
It should arrive within 3 to 5 business days.
A confirmation is on its way to laura.meyer@example.com.

Best regards,
Support team
```

What this run gets wrong, stated plainly:

- **Still raw:** the house number `8`. No bundled recognizer detects house numbers yet, so it reaches the model. The order number `2026-4471` also stays raw; order IDs are tenant-specific and need a custom recognizer in your policy.
- **Over-caught:** the ticket number `48213` was taken for a postal code. That costs precision, not privacy, and it restores to the same value.

The same boundary applies to tool-call arguments in agent frameworks: the JSON the model fills in carries placeholders, and Gaze restores them before your tool runs ([how it fits your stack](#how-it-fits-your-stack)).

This is the real output of the current `main` branch, not a picked best case. On the v0.14.0 benchmark (2,910 documents), the configuration that now ships by default (rules plus the NER model, no safety net) still let **20.7 % of personal-data (PII) bytes** through; the goal is zero ([benchmark](../../docs/reference/benchmarks/README.md#current-release)). The exact policy and commands: [reproduce this example](#reproduce-this-example).

## Seven steps

![Steps 1 to 4, normalize, recognize, resolve and swap, are the deterministic floor. Step 5, the optional safety net, and step 6, the output check, give a second opinion. The AI model sees placeholders only, and step 7 restores the reply.](../../docs/assets/gaze-seven-steps.svg)

1. **Normalize.** Tidy Unicode and spacing, and keep a map back to the original bytes.
2. **Recognize.** About 40 bundled rules (formats, checksums, cue words) plus one NER model (a model that spots names and places) each propose candidates.
3. **Resolve.** Where candidates overlap, one wins. The losers are logged.
4. **Swap.** Each winner becomes a placeholder plus a manifest entry. The same value always gets the same placeholder.
5. **Safety net** (optional). A second, different model rereads the output and raises suspects. It is a second opinion and cannot edit anything itself.
6. **Output check.** Each suspect becomes a placeholder, is deleted as a last resort, or the whole document is refused, depending on the mode below.
7. **Restore.** Placeholders in the reply become the originals. A placeholder Gaze never issued is refused, never guessed.

Steps 1 to 4 are the deterministic floor: same input, same output, every placeholder traceable to a versioned rule.

## What happens when the safety net disagrees

| Mode | What happens to a suspect | Reversible? | Who refuses |
|---|---|---|---|
| `resolve` **(default)** | Becomes a normal placeholder. If that is impossible, the fallback decides. | Yes | Only a `strict` fallback |
| `redact` | The suspect bytes are deleted from the text (no marker yet), and an audit row is written. | No, for that span | Nobody |
| `strict` | The whole document is refused (exit code 3, empty output). | Nothing was sent | Gaze |
| `tolerant` | A warning only. **The suspect reaches the model.** Development use only. | Yes | Nobody, the leak ships |

The fallback (`--safety-net-fallback`) can be `redact` (default), `strict`, or `tolerant`. Details: [safety-net modes](../../docs/explanation/safety-net/safety-net-modes.md).

## Reproduce this example

The example above is synthetic. It is shown as produced with the bundled `core` rules and the Davlan NER model, with no safety net. Install the NER model once with `gaze setup` (or `bash scripts/fetch/fetch-ner-model.sh`) and save this policy as `example-policy.toml`:

```toml
schema_version = "0.1.0"

[session]
scope = "conversation"

[locale]
active = ["de-DE"]

[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de-DE"
threshold = 0.3

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "location"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:phone"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:postal_code"
action = "tokenize"

[[rule]]
kind = "class"
class = "custom:birth_date"
action = "tokenize"

[[rule]]
kind = "default"
action = "tokenize"
```

Save the ticket as `ticket.txt` and the model's draft (the reply shown above, with the placeholders exactly as `gaze clean` printed them, session prefix included) as `reply.txt`. Then:

```sh
gaze clean --policy example-policy.toml < ticket.txt > clean.json
jq -r .clean_text clean.json            # what the model receives

jq --rawfile text reply.txt '{session_blob, text: $text}' clean.json \
  | gaze restore | jq -r .text          # what your app sends
```

`clean.json` also carries the manifest (`entries`) and the `session_blob` that `gaze restore` needs; neither goes to the model. The last policy rule tokenizes every detected class, so the raw house number is not a policy choice: no recognizer detects it. The per-session prefix on each placeholder changes on every run.

## Why this exists

PII in agent workflows usually falls into one of three failure modes:

1. **No redaction.** Real emails, phone numbers, and order IDs end up in the model provider's logs.
2. **One-way redaction.** PII is stripped, the agent replies "I've sent the confirmation to `<REDACTED>`", and you have no way to thread the reply back to the actual customer.
3. **LLM-judged redaction.** A second model call decides what's PII. Non-deterministic, non-auditable, costs another round trip every turn.

Gaze is the fourth path: deterministic detection, signed restore manifest, every token traced to a versioned recognizer.

Gaze is open-source privacy infrastructure for organisations that must meet GDPR or the EU AI Act while still using third-party LLMs. The detection layer and rulepacks are dual-licensed Apache-2.0 OR MIT — every PII recognizer that ships here is a contribution to a public commons that any privacy-sensitive project can audit, adopt, or extend.

Your agent never sees a real email, phone number, or order ID. Your server keeps the only manifest that can read those tokens back. Detection is regex, validator, and locale-cue driven — every emitted token traces to a versioned recognizer, not to a second model's opinion of what was sensitive.

## What ships

Each feature, what you get, where the proof lives.

- **Reversible by contract.** Tokens are session-scoped, counted per class (`Email_1`, `Email_2`), and only resolvable through a signed `SensitiveSnapshot`. There is no string-map fallback. Manifests written by an older minor restore on a newer minor — see the reversibility statement at the bottom of [`UPGRADE.md`](../../UPGRADE.md).
- **Every token is auditable.** Each emission carries a `recognizer_id` plus `recognizer_version_id` (suffixed `_vN`) into the optional SQLite audit log. Pre-v0.8 rows surface as `legacy_unversioned`. The export column set never includes raw PII payloads.
- **10 validator-backed national IDs across 5 locale packs, 3 locale-gated regex IDs.** Aadhaar (Verhoeff), NIR (MOD-97 variant), Steuer-ID (MOD 11,10), BSN (MOD-11), CPF + CNPJ (MOD-11), NHS (MOD-11), US SSN, UK NINO, Indian PAN. Adopters in BR / FR / NL / IN / UK / US get coverage with one `--locale` flag. Full table in [Detection coverage](#detection-coverage).
- **Defense in depth, observer-only.** Regex, dictionary, and optional NER form the detection floor. Every detector's `detect` returns a `Result`, so a backend failure fails **closed** — it aborts outbound redaction instead of silently returning an empty result, and long NER inputs (>512 tokens) are scanned in overlapping tokenizer-token windows so nothing slips past the model unscanned ([P0 #908](../../docs/explanation/detection/ner-failclosed.md)). Pass-3 SafetyNet runs *after* tokenization, against the already-clean text plus the manifest, and can flag suspect bytes the rules missed — but it cannot mutate the clean output or the manifest. No safety net runs by default. Two opt-in backends ship: the OpenAI Privacy Filter (subprocess) and Nym-small (in process). Contract: [`docs/explanation/safety-net/safety-nets.md`](../../docs/explanation/safety-net/safety-nets.md).
- **Fail closed everywhere.** Ambiguous matches are tokenized, never silently passed. Unknown validators or normalizers fail at policy load — no degraded mode. Strict-mode SafetyNet exits `3` with `{"error":"SafetyNet","exit":3,"variant":"SuspectedLeak"}` and stdout stays empty.
- **Agentic shapes are first-class.** Tool-call JSON arguments, SSE-streamed deltas, multi-turn sessions with evolving manifest state, and structured documents (PNG / JPG / PDF → Tesseract → `SafeBundle`) all redact correctly. The MCP runtime in [`gaze-mcp-core`](../../crates/gaze-mcp-core/) puts the same chokepoint between agent tool calls and source systems.
- **Multi-provider HTTP proxy with a daemon.** `gaze proxy start` puts a PII chokepoint in front of **API-key-authenticated** traffic to OpenAI's `/v1/chat/completions`, Anthropic's `/v1/messages`, and Gemini's `/v1beta/models/*:{generateContent,streamGenerateContent}` — i.e. when an SDK or agent authenticates with `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GOOGLE_API_KEY`. Consumer subscription tiers (ChatGPT Plus, Claude.ai, Gemini Advanced) use browser sessions and web endpoints and are outside this public proxy contract. SSE streams and tool-call argument JSON are accumulated chunk-by-chunk before redaction. The strict Anthropic profile proves each full request and response; see its [public contract](../../docs/explanation/proxy/anthropic-messages-contract.md). Subcommands `serve`, `start`, `stop`, `status`, `logs`, `restart`, plus opt-in `install-launchd` / `install-systemd-user`. See [`crates/gaze-proxy/README.md`](../../crates/gaze-proxy/README.md).
- **Opt-in local inspection dashboard (default-off).** `gaze proxy serve --dashboard` pairs an isolated, memory-only dashboard child that renders the proxy's provider-visible traffic — and, only with explicit per-domain risk acknowledgements, owner-raw or owner-restored payloads — on a fresh loopback origin behind a one-shot pairing token. **Enabling it expands your local trusted computing base:** captured payloads become visible to the paired browser session. The dashboard never ships in the default build (`gaze-cli` `dashboard` cargo feature, default-off), never persists payloads, and any activation failure disables only the dashboard while the proxy keeps serving. See [Run the local dashboard](../../docs/how-to/dashboard/run-local-dashboard.md) and the [dashboard trust boundary](../../docs/explanation/dashboard/trust-boundary.md).
- **OSS document ingestion.** `gaze document clean ./input.pdf --out ./safe-bundle/` OCRs PNG/JPG/PDF through Tesseract, runs the recognized text through the standard pipeline, and writes a `SafeBundle` — `clean.md` + `manifest.json` + `report.json`. Layout report v2 surfaces per-page OCR confidence, multi-column segmentation, table-cell preservation, and vector-PDF fallback when PDFs have selectable text. Plug in alternative OCR drivers via the `OcrBackend` trait. Adopter quickstart: [`docs/how-to/document/ingest-documents.md`](../../docs/how-to/document/ingest-documents.md). Full bundle contract: [`docs/explanation/document/document-extension.md`](../../docs/explanation/document/document-extension.md).
- **Long-lived stdio server for repeated redaction.** `gaze daemon` keeps one pipeline and model load hot, then serves JSON-per-line requests with per-`session_id` manifest isolation. It avoids binary/model cold starts on every agent turn, exits gracefully on SIGTERM, and evicts sessions by LRU or idle timeout. Adopter quickstart: [`docs/how-to/daemon/run-daemon.md`](../../docs/how-to/daemon/run-daemon.md). Full contract: [`docs/explanation/daemon/daemon-mode.md`](../../docs/explanation/daemon/daemon-mode.md).

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
- **Proxy** — SDK base-URL swap, API-key path only. Use when the agent is a hosted product or vendor SDK that talks to `api.openai.com` / `api.anthropic.com` / `generativelanguage.googleapis.com` with an API key, and you cannot link a library or rewrite its tool layer. Subscription-tier web clients are out of scope.

Architecture overview with eight Key Design Decisions: [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

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

Tenant-specific PII — order IDs, song titles, artist names — needs a dictionary or custom regex recognizer. See [`docs/reference/policy.md`](../../docs/reference/policy.md).

## Limits

- Detection floor is regex + validator + locale cue. Tenant-specific PII needs a custom recognizer.
- Linux x86_64 binaries link against glibc 2.39+ (Ubuntu 24.04, Debian 13, RHEL 10, or newer). Older distros: build from source.
- No Intel macOS, no musl, no Windows binaries today. Build from source.
- NER model leaderboard: [`docs/reference/benchmarks/README.md`](../../docs/reference/benchmarks/README.md#ner-model-leaderboard). The shipped default is the pinned Davlan mBERT NER model with no safety net.
- SafetyNet benchmark cells for the OpenAI Privacy Filter are populated in the [safety-net matrix](../../docs/reference/benchmarks/README.md#safety-net-matrix). Nym-small measurements are in [safety nets](../../docs/explanation/safety-net/safety-nets.md#measured).
- `gaze-proxy` ships OpenAI / Anthropic / Gemini adapters. Certificate management, PAC mode, Electron integration, transparent interception, browser sessions, and consumer subscription endpoints are outside its public contract.

## Glossary

- **Recognizer.** One detection rule or model that proposes "these bytes look like a phone number" (or a name, an IBAN, and so on). Every placeholder names the recognizer that produced it.
- **Manifest.** The private list that maps each placeholder back to its original value. It stays on your side and is never sent to the model.
- **NER.** Named-entity recognition: a model that spots names, places, and organizations in free text. In Gaze it is one candidate source among the rules, not the judge.
- **Safety net.** A second, different model that rereads the already-swapped output and raises suspects the rules missed. It cannot edit the output or the manifest itself.
- **Resolve.** The default safety-net mode: a suspect is fed back through conflict resolution so it becomes a normal, restorable placeholder.
- **Fallback / redact.** What happens when resolve cannot turn a suspect into a placeholder. The default, `redact`, deletes the suspect bytes from the text (a visible marker is planned, not shipped); those bytes cannot be restored.
- **Fail closed.** When something goes wrong (a model is missing, a rule is unknown, a suspect cannot be handled in `strict`), Gaze refuses instead of passing text through unchecked.
- **Format vs document basis.** A `format` recognizer runs for every document because the shape itself is the evidence (for example a US phone format). A `document` recognizer runs only when the document's locale matches.
- **Residual fragment.** The part of a suspect that is still uncovered after resolve, for example bytes next to an existing placeholder. The output check must still turn it into a placeholder, delete it, or refuse.
