# Kiji DistilBERT SafetyNet Setup

This page is an adopter setup guide for the `kiji-distilbert` SafetyNet
backend. For the full SafetyNet contract, see
[`docs/architecture/safety-nets.md`](../architecture/safety-nets.md).

Doc convention note: avoid version mentions unless the version is
load-bearing, such as a dependency line, release artifact selector, or
migration boundary. Setup docs should describe the current contract and link
to `CHANGELOG.md` / `UPGRADE.md` for release history so front-door docs do
not go stale.

## What This Is

Kiji DistilBERT is an observer-only Pass-3 SafetyNet backend. It runs after
Gaze has already produced clean text and a manifest, then reports residual PII
suspects that the deterministic passes may have missed. It never rewrites clean
text, never mutates the manifest, and never participates in restore. That
shape is intentional: SafetyNet is defense in depth for north-star Axis 1, not
a second redaction engine.

The backend wraps a local subprocess that serves pinned ONNX DistilBERT
weights. The adapter sends already-tokenized clean text to stdin and accepts
typed JSON spans from stdout. The runtime requires an Apache-2.0 model bundle
with `SHA256SUMS`, `labels.json`, `model.onnx`, and `tokenizer.json` on disk
before it will run. The canonical upstream source is
`onnx-community/distilbert-NER-ONNX` at commit
`3a19fe9404a4469d91aa3d551558a97f68872f67`; the runtime pins the canonical
bundle checksum file to SHA256
`c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f`.
The opt-in ORT int8 path adds `SHA256SUMS.int8` and `model.int8.onnx`; its
checksum file is pinned to SHA256
`6e7f238f38c5ee7977052ec391f6a8c68bbef038091f2ecff4747cc2268210cb`.
The local adapter validates emitted labels and maps them into Gaze's closed
SafetyNet classes before manifest diffing.

For the full pipeline view, including the canonical ASCII diagram that places
Kiji inside Pass 3, see
[`docs/architecture/safety-nets.md`](../architecture/safety-nets.md). Pass 3
SafetyNet is observer-only — it never mutates `clean_text` or the manifest.
Restore round-trip is unaffected.

Source anchors:
[`kiji_distilbert/mod.rs`][kiji-mod],
[`backend/subprocess.rs`][kiji-subprocess], and
[`class_map.rs`][kiji-class-map].

## When To Choose Kiji Over OpenAI Filter

- Choose Kiji when an OpenAI Privacy Filter install is not acceptable in your
  deployment path. Kiji is local-subprocess only; Gaze does not call a network
  service for SafetyNet checks.
- Choose Kiji when you want a smaller pinned artifact and faster cold start.
  The intended bundle is an ONNX DistilBERT model rather than the heavier OPF
  path.
- Choose Kiji when a second NER-oriented opinion is useful at the agent
  chokepoint. The trade-off is a narrower closed local label map than OPF's
  class taxonomy: Kiji emits validated person, location, organization, and
  miscellaneous labels into Gaze's SafetyNet manifest-diff layer, while the
  public upstream taxonomy remains the 26-class reference.

## Installation

1. Fetch the pinned model bundle:

   ```sh
   bash scripts/fetch-kiji-safetynet-model.sh
   ```

   The script installs to
   `${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert` by
   default. It fetches release checksums as `SHA256SUMS.kiji`, writes them into
   the model directory as `SHA256SUMS`, and verifies the bundle before
   returning. The script fails closed if the pinned upstream commit is not set,
   if the release checksum cannot be resolved, or if any required file is
   missing.

   To prepare the opt-in int8 ORT artifact, install `onnxruntime` with its
   quantization extras available and run:

   ```sh
   python3 scripts/quantize-kiji-int8.py \
     "${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert"
   ```

   The helper writes `model.int8.onnx` and `SHA256SUMS.int8`. Gaze verifies the
   int8 manifest against the separate int8 SHA pin and will not fall back to
   fp32 if `--kiji-distilbert-precision=int8` is requested.

2. Install the CLI with Kiji support:

   ```sh
   cargo install --path crates/gaze-cli --features safety-net-kiji
   ```

   If you want both SafetyNet backends in one binary:

   ```sh
   cargo install --path crates/gaze-cli --features safety-net-openai,safety-net-kiji
   ```

3. Verify the flag surface:

   ```sh
   gaze --help 2>&1 | grep kiji-distilbert
   ```

   You should see `kiji-distilbert` accepted by the SafetyNet flags. The CLI
   flag table lives in [`crates/gaze-cli/README.md`][cli-readme], and the
   activation path is implemented in [`crates/gaze-cli/src/pipeline/run.rs`][cli-run].

4. Install the reference subprocess wrapper dependencies and point Gaze at the
   wrapper:

   ```sh
   python3 -m pip install --user onnxruntime tokenizers numpy
   chmod +x scripts/kiji-runner.py
   export GAZE_KIJI_DISTILBERT_COMMAND=$PWD/scripts/kiji-runner.py
   ```

   If `python3 -m pip` is unavailable on macOS, bootstrap user-local pip first:

   ```sh
   python3 -m ensurepip --user
   python3 -m pip install --user onnxruntime tokenizers numpy
   ```

   Some externally managed Python distributions, including common Homebrew
   Python installs on macOS, reject `--user` installs under PEP 668. In that
   case, use a Python distribution or prebuilt runtime environment where these
   three packages are available without changing the system package manager.
   Keep the model bundle and wrapper local; do not add network fetches to the
   Gaze runtime path.

Keep the fetch step in deployment automation, not inside the hot path. The
SafetyNet backend is designed around a local pinned bundle: operators decide
when artifacts move, verify them once, and then run `gaze clean` without
network access. That makes SafetyNet startup boring and auditable. If your
environment builds immutable images, bake the model directory into the image
with owner-only permissions. If your environment provisions on first boot, run
the fetcher before accepting traffic and fail the host health check when it
cannot produce the required artifact set.

The Kiji subprocess command is deliberately separate from the model directory.
That lets you ship a small wrapper around the runtime you operate while keeping
the pinned model bundle under Gaze's artifact contract. Gaze ships
[`scripts/kiji-runner.py`][kiji-runner] as the reference wrapper. You may
replace it with a compiled helper or another local executable, but it must obey
the stdin/stdout contract described below. Do not emit diagnostics to stdout;
stdout is reserved for the JSON span array.

## First Clean Run With Kiji

Start from a policy that tokenizes emails, such as the root README Quickstart
policy. Then run:

```sh
printf '%s' 'Contact alice@example.invalid for details.' \
  | gaze clean \
      --policy quickstart-policy.toml \
      --safety-net kiji-distilbert \
      --safety-net-backend kiji-distilbert \
      --kiji-distilbert-command "$PWD/scripts/kiji-runner.py" \
      --kiji-distilbert-model-dir ~/.local/share/gaze/models/kiji-distilbert
```

The command path is the local Kiji subprocess you operate. The adapter invokes
it with `--format json --output-mode typed` and appends `--model-dir <path>`
when the model directory is configured. That subprocess must read the clean
text from stdin and emit JSON spans shaped like:

```json
[{"label":"PER","start":0,"end":11,"score":0.97}]
```

The reference wrapper emits bare upstream DistilBERT entity labels (`PER`,
`LOC`, `ORG`, `MISC`) after BIO decoding. Gaze validates those labels and maps
them into its closed SafetyNet classes before manifest diffing. Existing
wrappers that emit lower-case Gaze label ids (`person`, `location`,
`organization`, `miscellaneous`) remain accepted for compatibility.

A clean run emits the normal `gaze clean` JSON plus `leak_report`:

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

`suspect_count = 0` is the contract for no SafetyNet suspects. The clean text
and restore manifest still come only from the deterministic pipeline.

Treat non-zero suspect counts as routing signals, not as automatic replacement
instructions. A SafetyNet suspect says the backend saw a span worth reviewing
after deterministic tokenization. The next action is to inspect the suspect
class and leak kind, then decide whether a deterministic recognizer, dictionary
term, rulepack locale, or policy rule should own that class. Promoting repeated
SafetyNet findings into deterministic coverage is how the defense-in-depth
layer improves the default pipeline without letting an ML backend mutate the
manifest.

For agent integrations, keep the same data boundary you use without SafetyNet:
send only `clean_text` to the model, retain `session_blob` server-side, and
restore model output only through authorized restore flows. The `leak_report`
is operator metadata. It can be logged or audited through the metadata-only
safety-net table, but it is not part of the prompt payload.

## Switching Between Backends

The backend selector is `--safety-net-backend`. The legacy activator
`--safety-net <kind>` still turns on Pass-3 SafetyNet, and
`--safety-net-backend` chooses which implementation runs:

```sh
gaze clean \
  --policy quickstart-policy.toml \
  --safety-net openai-filter \
  --safety-net-backend kiji-distilbert \
  --kiji-distilbert-command "$PWD/scripts/kiji-runner.py" \
  --kiji-distilbert-model-dir ~/.local/share/gaze/models/kiji-distilbert
```

This switch does not require a policy or manifest change. Both backends read
the post-clean text, compare their typed spans against the manifest, and report
the same `LeakReport` shape. Restore remains manifest-first and
backend-independent.

Backend switching is useful when the same product must run in different
infrastructure tiers. A hosted environment might have the OpenAI Privacy Filter
approved and use OPF for continuity with existing review workflows. A
single-tenant or offline deployment might prefer Kiji because the artifact
bundle is smaller and easier to pin. You can keep one policy file and change
only the CLI backend flags per environment.

For locale-specific routing, use the registry mode instead of the single
backend selector:

```sh
gaze clean \
  --policy quickstart-policy.toml \
  --locale de-DE \
  --safety-net-registry \
  --safety-net-add kiji-distilbert \
  --kiji-distilbert-command /opt/kiji/bin/kiji \
  --kiji-distilbert-model-dir ~/.local/share/gaze/models/kiji-distilbert \
  --kiji-distilbert-locales en-US,en-GB \
  --safety-net-add openai-filter \
  --opf-command /opt/opf/bin/opf \
  --opf-checkpoint ~/.local/share/gaze/models/opf \
  --opf-locales de-DE,de-AT
```

Registry dispatch resolves the first backend matching the active locale
(`de-DE` in the example), then falls back to the parent language and `global`.
If more than one backend matches the same tier, v1 uses first-match wins.
Aggregation is a separate follow-up.

## Pinned-Artifact Contract

Kiji is fail-closed by design. The CLI checks the configured model directory
for every required artifact before the subprocess is spawned:

- `SHA256SUMS`
- `labels.json`
- `model.onnx`
- `tokenizer.json`

The required file list is defined in [`REQUIRED_KIJI_ARTIFACTS`][kiji-subprocess].
The checksum pin is defined beside that list as
`KIJI_DISTILBERT_BUNDLE_SHA256`. If any artifact is absent, the CLI returns
`SafetyNetArtifactMissing` with exit `2` before the backend process starts:

```json
{
  "error": "SafetyNetArtifactMissing",
  "exit": 2,
  "backend": "kiji-distilbert",
  "path": "<missing path>"
}
```

That exit is a configuration failure, not a leak report. It means the requested
SafetyNet could not be trusted to run against the pinned artifact set, so Gaze
refuses to silently continue with the backend disabled.

Once the initial artifact check passes, the subprocess backend also verifies
model directory presence and bundle integrity during backend initialization.
Missing weights at that layer map to `SafetyNetError::WeightsMissing`; a
checksum-file or artifact hash mismatch maps to
`SafetyNetError::ModelIntegrityMismatch`. The CLI treats both as SafetyNet
failures. Both checks preserve the same Axis-1 rule: requested privacy
infrastructure must either run with the pinned inputs or fail closed.

This contract is intentionally stricter than "try the backend if available."
Silent fallback would make SafetyNet availability depend on host drift, cache
state, or a deployment race. Instead, a configured Kiji backend has a clear
activation predicate: command present, model directory present, required files
present, and subprocess output parseable. If that predicate is false, the run
must surface a typed failure before any cleaned output can be mistaken for a
fully checked result.

## Failure Modes And Exit Codes

SafetyNet activation has two modes:

- `--safety-net-mode strict` is the default. If Kiji reports `Uncovered` or
  `PartialBleed`, `gaze clean` exits `3` with
  `{"error":"SafetyNet","exit":3,"variant":"SuspectedLeak"}` and stdout stays
  empty.
- `--safety-net-mode tolerant` keeps stdout available and emits a warning to
  stderr, such as
  `{"warning":"SafetyNet","variant":"SuspectedLeak","count":1}`.

`ClassMismatch` is handled differently. It means the deterministic pipeline
tokenized the bytes, but the manifest class disagrees with the SafetyNet class.
Strict mode warns for `ClassMismatch`; it does not block, because the suspect
bytes are already covered by a token.

Kiji suspect kinds come from `Manifest::diff_against`:

- `Uncovered`: Kiji found a span with no overlapping token in the manifest.
- `PartialBleed`: part of the Kiji span is covered by a token, but at least one
  byte range remains uncovered.
- `ClassMismatch`: the span is covered, but the manifest class differs from the
  Kiji-mapped class.

Other startup and runtime failures use the shared SafetyNet exit map. Missing
backend flags or missing compile features are configuration errors. Timeouts,
invalid JSON, non-finite scores, unsupported labels, and subprocess failures
become SafetyNet failures. The CLI maps timeout messages to the `Timeout`
variant; unsupported or malformed Kiji labels map to `InvalidOutput`.

Use strict mode for production paths where a residual leak suspect should stop
the response before it reaches an LLM. Use tolerant mode for measurement,
canarying, or migration periods where you need to observe SafetyNet findings
without blocking existing traffic. Tolerant mode is still useful only if stderr
or the audit sink is monitored; otherwise it hides the signal you enabled the
backend to collect.

When a run fails with `SafetyNetArtifactMissing`, fix deployment state. When it
fails with `SafetyNet` and a runtime variant, inspect the subprocess wrapper,
timeout, stdout shape, and model permissions. When it fails with
`SuspectedLeak`, inspect the reported leak kind and decide whether the
deterministic pipeline needs a new recognizer or policy change. Keep raw source
payloads out of tickets and logs; reproduce with project-approved synthetic
fixtures whenever possible.

## Cross-Links

- [`docs/architecture/safety-nets.md`](../architecture/safety-nets.md) is the
  full SafetyNet contract reference.
- [`docs/research/v0.8-kiji-class-gap.md`](../research/v0.8-kiji-class-gap.md)
  explains how the upstream 26-class taxonomy maps into Gaze's current
  deterministic and observer-only coverage story.
- [`docs/research/v0.8-kiji-benchmark.md`](../research/v0.8-kiji-benchmark.md)
  records the benchmark methodology and measured subset status.
- [`docs/architecture/safety-net-benchmark.md`](../architecture/safety-net-benchmark.md)
  records the v0.9 backend × locale × mode snapshot shape.
- [`crates/gaze-cli/README.md`][cli-safety-net] lists the full CLI flag and
  exit-code surface.

[cli-readme]: ../../crates/gaze-cli/README.md#clean
[cli-run]: ../../crates/gaze-cli/src/pipeline/run.rs
[cli-safety-net]: ../../crates/gaze-cli/README.md#safety-net
[kiji-class-map]: ../../crates/gaze-recognizers/src/safety_net/kiji_distilbert/class_map.rs
[kiji-mod]: ../../crates/gaze-recognizers/src/safety_net/kiji_distilbert/mod.rs
[kiji-runner]: ../../scripts/kiji-runner.py
[kiji-subprocess]: ../../crates/gaze-recognizers/src/safety_net/kiji_distilbert/backend/subprocess.rs
