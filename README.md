# Gaze

[![Crates.io](https://img.shields.io/crates/v/gaze-pii.svg)](https://crates.io/crates/gaze-pii) [![License](https://img.shields.io/crates/l/gaze-pii.svg)](https://github.com/CertaMesh/gaze#license) [![docs.rs](https://docs.rs/gaze-pii/badge.svg)](https://docs.rs/gaze-pii) [![Tests](https://github.com/CertaMesh/gaze/actions/workflows/test.yml/badge.svg)](https://github.com/CertaMesh/gaze/actions/workflows/test.yml) [![GitHub stars](https://img.shields.io/github/stars/CertaMesh/gaze?style=social)](https://github.com/CertaMesh/gaze/stargazers)

**Gaze swaps the personal details in your text for placeholders before an AI model sees it, then swaps the real details back into the model's reply.** The model works with `<Name_1>`; only your server knows that means Anna Berg.

*Pre-1.0, API stabilizing. Reversibility is guaranteed across minor versions — manifests written by an older minor restore on a newer minor (see [`UPGRADE.md`](UPGRADE.md)).*

## The promise in one picture

```text
your app    "Please call Anna Berg back."
   │
   ▼
Gaze        finds the name, swaps it, remembers:
   │        <Name_1> = Anna Berg   (this list stays with you)
   ▼
AI model    sees "Please call <Name_1> back."
   │        replies "Calling <Name_1> now."
   ▼
Gaze        swaps back
   ▼
your app    "Calling Anna Berg now."
```

This is pseudonymization, not deletion. The reply still makes sense because Gaze keeps the mapping, and only you hold it.

## One document, start to finish

A synthetic customer note goes in (the person and every value are made up):

```text
Kundin Anna Berg, Hauptstraße 12, 1010 Wien, Tel +43 1 5550123, geb. 03.04.1988.
```

The model receives this (placeholders shortened; real ones carry a per-session prefix such as `<f4f1d368:Name_1>`):

```text
<Name_2> <Name_1>, <Location_1> 12, 1010 <Location_2>, Tel <Custom:phone_1>, geb. 03.04.1988.
```

The manifest, the list that turns placeholders back into values, is never sent:

```text
Name_1          = Anna Berg
Name_2          = Kundin
Location_1      = Hauptstraße
Location_2      = Wien
Custom:phone_1  = +43 1 5550123
```

A reply such as `Calling <Name_1> now at <Custom:phone_1>` restores to `Calling Anna Berg now at +43 1 5550123`.

What this run gets wrong, stated plainly:

- **Still raw today:** the house number `12`, the Austrian postal code `1010`, and the birth date after `geb.`. No bundled recognizer detects these shapes yet, so they reach the model.
- **Over-caught:** `Kundin` (German for "female customer") was flagged as a name by the safety net. That costs precision, not privacy, and it restores to the same word.

This is the real output of the current `main` branch on this sentence, not a picked best case. On the v0.14.0 benchmark (2,910 documents), the shipped default still let **19.3 % of personal-data (PII) bytes** through; the goal is zero ([benchmark](docs/reference/benchmarks/README.md#current-release)). The exact policy and command: [reproduce this example](docs/explanation/how-gaze-works.md#reproduce-this-example).

## Seven steps

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

The fallback (`--safety-net-fallback`) can be `redact` (default), `strict`, or `tolerant`. Details: [safety-net modes](docs/explanation/safety-net/safety-net-modes.md).

**Scope:** outbound PII control with reversibility. Gaze is *not* a guardrail, prompt-injection defense, or content-safety filter — it keeps real PII out of the model and restores it in the reply.

**Go deeper:** [How Gaze works](docs/explanation/how-gaze-works.md) covers why this exists (and the GDPR / open-commons positioning), what ships, how it fits your stack, the pipeline shape, detection coverage, limits, and a glossary.

## Quickstart

Three commands from zero to redacting real PII:

```sh
cargo install gaze-cli --version 0.14.0                     # `gaze setup` ships in the default build
gaze setup                                                   # installs + SHA-verifies the NER model, writes ./gaze.toml, runs a doctor check
echo "Contact Markus Gottschaue at markus@acme.com" | gaze clean --policy gaze.toml
```

```text
{"clean_text":"Contact <Name_1> at <Email_1>", "entries":[{"class":"Name",...},{"class":"Email",...}], ...}
```

`gaze setup` fetches the pinned, SHA-verified NER model into your data dir, generates a working policy wired to it, and confirms detection runs — no manual model fetch or flag-wrangling. The model never sees `Markus Gottschaue` or `markus@acme.com`; rehydrate the reply with `gaze restore` on the same per-session manifest.

Want explicit control over rulepacks, locales, and the observer-only SafetyNet? See [Manual setup](#manual-setup).

## In production: AI support drafts that never see the customer

[`CertaMesh/gaze-ghostwriter`](https://github.com/CertaMesh/gaze-ghostwriter) is a Laravel package that watches a support inbox over IMAP and drafts replies with an LLM. The application does the data lookup. Gaze pseudonymizes the resulting context. The LLM only composes prose.

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

- Drop-in Laravel package: [`CertaMesh/gaze-ghostwriter`](https://github.com/CertaMesh/gaze-ghostwriter)

Agentic workflows (browser automation, tool execution) hook the same restore boundary at tool-call args, on the same manifest contract — the agent stays on tokens end-to-end.

CLI surface (`gaze clean`, `gaze restore`, audit, policy TOML): [Quickstart](#quickstart), [`gaze-cli` README](crates/gaze-cli/README.md).

## Install

Install the CLI from crates.io:

```sh
cargo install gaze-cli --version 0.14.0
```

Or build from source (latest `main`, or to enable extra features):

```sh
git clone https://github.com/CertaMesh/gaze.git
cd gaze
cargo install --path crates/gaze-cli
```

Pre-built binaries for Apple Silicon macOS and Linux x86_64 (glibc 2.39+) are attached to each [GitHub release](https://github.com/CertaMesh/gaze/releases). Other targets: `cargo build --release -p gaze-cli`.

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

## Manual setup

Prefer to wire the policy by hand instead of `gaze setup`? This guided path goes from zero PII configuration to a working clean run, with optional NER and the observer-only SafetyNet layered on top. Each step is copy-paste-able against the current `gaze` CLI. (For the one-command path, see [Quickstart](#quickstart) above.)

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

Schema and every rule kind / action live in [`docs/reference/policy.md`](docs/reference/policy.md).

### 2. Add NER

NER is opt-in and stacks on top of the deterministic regex and dictionary passes. Turn it on when the input has free-prose names that the cue-anchored Name recognizer in `core` does not cover.

Fetch the pinned mBERT bundle once:

```sh
bash scripts/fetch/fetch-ner-model.sh
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

Schema details, threshold range, and `~/` expansion rules: [`docs/reference/policy.md`](docs/reference/policy.md#ner-optional). Pinned artifact contract and adopter label map: [`crates/gaze/testdata/ner/README.md`](crates/gaze/testdata/ner/README.md) plus [`crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json`](crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json).

### 3. Add a SafetyNet (Pass-3 observer)

The SafetyNet is an **observer-only post-clean check**. It reads the already-tokenized text plus the manifest of emitted spans and reports any suspect bytes the deterministic passes missed. It cannot mutate the clean text, cannot mutate the manifest, and cannot affect restore — full contract in [`docs/explanation/safety-net/safety-nets.md`](docs/explanation/safety-net/safety-nets.md).

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

SafetyNet runs in **`resolve` mode by default** with a **`redact` fallback**. When the filter raises an `Uncovered` or `PartialBleed` suspect, Gaze first promotes the suspect into a synthetic custom-recognizer match and re-runs the resolver so the span can be tokenized into the manifest — preserving reversibility. If `resolve` cannot honor a suspect (validator-veto, missing anchor, or a residual suspect after the one-shot pass), the composable `--safety-net-fallback {strict|tolerant|redact}` flag (default `redact`) decides what happens next: by default the suspect span is deleted from the clean text, the redaction is recorded in the audit trail, and the rest of the clean text continues to stdout. **The reversibility-first default is the production contract**: every suspect either becomes a fully restorable manifest token or is stripped before reaching the LLM, and every action emits a typed audit row.

Adopters who want the v0.7.x hard-fail posture can opt in with `--safety-net-mode strict` (any suspect exits `3`, stdout stays empty). Adopters who cannot afford the resolve pass can skip directly to strip-and-continue with `--safety-net-mode redact`. A `tolerant` mode exists for **local development only** — while debugging recognizer coverage or measuring SafetyNet recall, it downgrades suspects to a stderr warning instead of refusing the output. **Do not use `tolerant` in production traffic.** A tolerant-mode pipeline is one that has agreed to ship suspected leaks. Mode catalog, fallback composition matrix, and exit-code map: [`docs/explanation/safety-net/safety-net-modes.md`](docs/explanation/safety-net/safety-net-modes.md) and [`crates/gaze-cli/README.md`](crates/gaze-cli/README.md#safety-net).

#### Kiji DistilBERT

The Kiji backend is also feature-gated. Fetch the pinned model bundle once, then reinstall the CLI with the Kiji feature compiled in:

```sh
bash scripts/fetch/fetch-kiji-safetynet-model.sh
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

Full Kiji setup, backend switching, and failure-mode notes: [`docs/how-to/safety-net/set-up-kiji-safetynet.md`](docs/how-to/safety-net/set-up-kiji-safetynet.md).

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

## Use from Rust

The CLI is a process boundary around the Rust runtime; you can link the runtime directly:

```sh
cargo add gaze-pii gaze-assembly
```

The crate is published as `gaze-pii` because the bare `gaze` name is in transfer on crates.io; the import path stays `use gaze::...` because `[lib].name = "gaze"` is preserved.

- Minimal example and the API surface table: [`crates/gaze/README.md`](crates/gaze/README.md) (also rendered on [crates.io/crates/gaze-pii](https://crates.io/crates/gaze-pii)).
- Full walk-through with structured documents, tenant-specific recognizers, and policy TOML: [`docs/tutorials/getting-started.md`](docs/tutorials/getting-started.md).

## Workspace and crates.io

Eleven published crates. Pick the smallest surface that does the job.

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
| [`gaze-mcp-bridge`](https://crates.io/crates/gaze-mcp-bridge) | You want the policy-gated MCP bridge that restores approved token fields before calling downstream MCP servers. |
| [`gaze-proxy`](https://crates.io/crates/gaze-proxy) | You want an HTTP proxy in front of API-key traffic to OpenAI / Anthropic / Gemini; consumer subscription tiers are outside this surface. The proxy is daemon-managed via `gaze proxy`. |

```sh
cargo add gaze-pii
```

Crate boundaries and the audit-isolation Dylint gate: [`docs/reference/crates.md`](docs/reference/crates.md). Document codec extension: [`docs/explanation/document/document-extension.md`](docs/explanation/document/document-extension.md).

## Publishing

The workspace publishes via the `publish-crates.yml` GitHub Actions workflow using crates.io trusted-publisher OIDC auth; it does not need a long-lived `CARGO_REGISTRY_TOKEN` secret.

- **Tag push** (`git tag v<version> && git push --tags`) runs a real publish on every workspace crate in topological order.
- **Manual dispatch** with `dry_run=true` packages each crate without publishing, useful for catching metadata or dependency issues before a release tag.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). **New here?** Browse the [good first issues](https://github.com/CertaMesh/gaze/labels/good%20first%20issue) — locale rulepack entries and new validator-backed recognizers are natural starting points.

Apache-2.0 OR MIT, **no CLA** (DCO sign-off only, `git commit -s`); the project is run as a commons — open detection forever, no bait-and-switch, commercial features in separate repos. See [`docs/explanation/governance.md`](docs/explanation/governance.md).

Repository gates (xtask + Dylint) enforce the contracts in [`docs/explanation/`](docs/explanation/). Run them locally before pushing:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- ci-feature-matrix
```

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
