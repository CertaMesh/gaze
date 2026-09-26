# Set up a policy by hand

Prefer to wire the policy by hand instead of `gaze setup`? This guided path goes from zero PII configuration to a working clean run, with optional NER and the observer-only SafetyNet layered on top. Each step is copy-paste-able against the current `gaze` CLI. (For the one-command path, see [Quickstart](../../README.md#quickstart) in the project README.)

## Step 1: Redact with the core rulepack

Write the smallest policy that drives the bundled `core` rulepack and tokenizes every detected class:

```toml
# quickstart-policy.toml
schema_version = "0.1.0"

[session]
scope = "persistent"
ttl_secs = 86400

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "default"
action = "tokenize"
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

Schema and every rule kind / action live in [`docs/reference/policy.md`](../../docs/reference/policy.md).

## Step 2: Add NER

NER is opt-in and stacks on top of the deterministic regex and dictionary passes. Turn it on when the input has free-prose names that the cue-anchored Name recognizer in `core` does not cover.

Fetch the pinned mBERT bundle once:

```sh
bash scripts/fetch/fetch-ner-model.sh
```

The script verifies a release-pinned `SHA256SUMS.ner` and installs the artifact set into `${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl` (pass a directory argument to override). No model is downloaded at `gaze clean` runtime — Gaze only consumes the on-disk bundle.

Add the `[ner]` block to `quickstart-policy.toml`. The default rule already tokenizes detected names:

```toml
[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"
threshold = 0.3
```

Re-run on free-prose German with a Name span the rule-based passes leave alone:

```sh
printf '%s' 'Bitte richten Sie es Dr. Schmidt aus.' \
  | gaze clean --policy quickstart-policy.toml
```

NER contributes a `Name_*` span via the model's `PER` label:

```json
{
  "clean_text": "Bitte richten Sie es Dr. <{session_hex}:Name_1> aus.",
  "session_blob": "<base64>",
  "stats": {"detections": 1, "locale_chain": ["de-DE", "global"], "dictionaries_loaded": []}
}
```

Schema details, threshold range, and `~/` expansion rules: [`docs/reference/policy.md`](../../docs/reference/policy.md#ner-optional). Pinned artifact contract and adopter label map: [`crates/gaze/testdata/ner/README.md`](../../crates/gaze/testdata/ner/README.md) plus [`crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json`](../../crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json).

## Step 3: Add a safety net (pass-3 observer)

The SafetyNet is an **observer-only post-clean check**. It reads the already-tokenized text plus the manifest of emitted spans and reports any suspect bytes the deterministic passes missed. It cannot mutate the clean text, cannot mutate the manifest, and cannot affect restore — full contract in [`docs/explanation/safety-net/safety-nets.md`](../../docs/explanation/safety-net/safety-nets.md).

A hand-written policy with no `[safety_net]` table runs no net. `gaze setup` enables Nym by default. Two backends ship: `openai-filter` wraps the upstream OpenAI Privacy Filter as a subprocess, and `nym` runs the Nym-small token classifier in process. Both are observer-only and both run under the **`resolve` mode default with a `redact` fallback**, the reversibility-preserving production posture (see below).

### OpenAI Privacy Filter

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

SafetyNet runs in **`resolve` mode by default** with a **`redact` fallback**. When the filter raises an `Uncovered` or `PartialBleed` suspect, Gaze first tokenizes the suspect span directly into the manifest as a restorable token of the suspect's class, then runs the nets once more — preserving reversibility. If `resolve` cannot tokenize a suspect (it overlaps an existing token, or the one re-run still reports a residual suspect), the composable `--safety-net-fallback {strict|tolerant|redact}` flag (default `redact`) decides what happens next: by default the suspect span is replaced with a one-way `[REDACTED:<class>]` marker in the clean text, the redaction is recorded in the manifest and in the audit trail, and the rest of the clean text continues to stdout. **The reversibility-first default is the production contract**: every suspect either becomes a fully restorable manifest token or is replaced by a one-way marker before reaching the LLM, and every action emits a typed audit row.

Adopters who want the v0.7.x hard-fail posture can opt in with `--safety-net-mode strict` (any suspect exits `3`, stdout stays empty). Adopters who cannot afford the resolve pass can skip directly to strip-and-continue with `--safety-net-mode redact`. A `tolerant` mode exists for **local development only** — while debugging recognizer coverage or measuring SafetyNet recall, it downgrades suspects to a stderr warning instead of refusing the output. **Do not use `tolerant` in production traffic.** A tolerant-mode pipeline is one that has agreed to ship suspected leaks. Mode catalog, fallback composition matrix, and exit-code map: [`docs/explanation/safety-net/safety-net-modes.md`](../../docs/explanation/safety-net/safety-net-modes.md) and [`crates/gaze-cli/README.md`](../../crates/gaze-cli/README.md#safety-net).

### Nym-small

The Nym-small net runs the multilingual Nym-small token classifier in process and flags only building numbers, licence plates, usernames and dates of birth by default: `gaze setup`, then `gaze clean --policy gaze.toml`. The allowlist and thresholds are policy data (`[safety_net.nym]`), and `[safety_net].backend = "nym"` activates it. Contract, measurements and open items (latency, licence review): [`docs/explanation/safety-net/safety-nets.md`](../../docs/explanation/safety-net/safety-nets.md#nym-small-adapter).
