# Set up a policy by hand

Build a policy manually, then add NER and a safety net. For automatic setup,
use the [Quickstart](../../README.md#quickstart).

## Step 1: Redact with the core rulepack

Load `core` and tokenize all detected classes:

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
printf 'Contact %s@%s for details.' alice example.invalid \
  | gaze clean --policy quickstart-policy.toml
```

Send only `clean_text` to the LLM. Keep the signed `session_blob` owner-side:

```json
{
  "clean_text": "Contact <{session_hex}:Email_1> for details.",
  "session_blob": "<base64>",
  "stats": {"detections": 1, "locale_chain": ["global"], "dictionaries_loaded": []}
}
```

Restore with the same manifest:

```sh
printf '{"session_blob":"<base64>","text":"Re: <{session_hex}:Email_1>"}' \
  | gaze restore
```

```json
{"text": "Re: alice [at] example.invalid"}
```

The restored address is shown with `[at]` in this page; the actual restore
output contains the `@` from the synthetic input.

Schema and every rule kind / action live in [`docs/reference/policy.md`](../../docs/reference/policy.md).

## Step 2: Add NER

Add optional NER for free-prose names beyond `core`'s cue-based coverage.

Fetch the pinned mBERT bundle once:

```sh
bash scripts/fetch/fetch-ner-model.sh
```

The script verifies pinned `SHA256SUMS.ner` and installs to
`${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl`.
Pass a directory to override. `gaze clean` uses local bundles; it downloads none.

Add to `quickstart-policy.toml`; its default rule tokenizes names:

```toml
[ner]
model_dir = "~/.local/share/gaze/models/davlan-mbert-ner-hrl"
locale = "de"
threshold = 0.3
```

Try German free text:

```sh
printf '%s' 'Bitte richten Sie es Dr. Schmidt aus.' \
  | gaze clean --policy quickstart-policy.toml
```

The model's `PER` label contributes a `Name_*` span:

```json
{
  "clean_text": "Bitte richten Sie es Dr. <{session_hex}:Name_1> aus.",
  "session_blob": "<base64>",
  "stats": {"detections": 1, "locale_chain": ["de-DE", "global"], "dictionaries_loaded": []}
}
```

Schema details, threshold range, and `~/` expansion rules: [`docs/reference/policy.md`](../../docs/reference/policy.md#ner-optional). Pinned artifact contract and adopter label map: [`crates/gaze/testdata/ner/README.md`](../../crates/gaze/testdata/ner/README.md) plus [`crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json`](../../crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json).

## Step 3: Add a safety net (pass-3 observer)

Safety nets inspect cleaned text and emitted spans. The observers cannot change
text, manifests, or restore; the pipeline applies the selected mode to suspects.
See the [contract](../explanation/safety-net/safety-nets.md).

No `[safety_net]` table means no net. `gaze setup` enables in-process `nym`;
`openai-filter` uses an OPF subprocess. Both default to `resolve` with `redact`
fallback.

### OpenAI Privacy Filter

Compile the optional OpenAI backend:

```sh
cargo install --path crates/gaze-cli --features safety-net-openai
```

Install upstream [`openai/privacy-filter`](https://github.com/openai/privacy-filter)'s
`opf` binary and checkpoint. Gaze downloads or updates neither. The checkpoint
directory must belong to the running user with mode `0700`.

Activate the filter on the same `gaze clean` invocation:

```sh
printf 'Contact %s@%s for details.' alice example.invalid \
  | gaze clean \
      --policy quickstart-policy.toml \
      --safety-net openai-filter \
      --openai-filter-command /opt/opf/bin/opf \
      --openai-filter-checkpoint /opt/opf/checkpoint \
      --openai-filter-device auto
```

`--openai-filter-device` accepts `auto` (default; the upstream `opf` picks), `cpu`, `cuda`, or `mps`.

Output includes `leak_report`; `suspect_count = 0` means no suspects reported:

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

Select with `--safety-net-mode {resolve|strict|redact|tolerant}`.

| Mode | Result |
|---|---|
| `resolve` (default) | Tokenize `Uncovered`/`PartialBleed` suspects into the manifest; re-run nets once |
| `strict` | Any suspect exits `3`; stdout stays empty |
| `redact` | Replace suspects with one-way `[REDACTED:<class>]` markers and continue |
| `tolerant` | Warn on stderr and emit suspected leaks; local development only |

If resolution overlaps an existing token or leaves suspects after the re-run,
`--safety-net-fallback {strict|tolerant|redact}` applies (default `redact`).
Resolved tokens remain restorable; fallback markers do not. Redactions are
recorded in the manifest and audit trail; each action emits a typed audit row.
Never use `tolerant` in production.

See the [mode matrix](../../docs/explanation/safety-net/safety-net-modes.md)
and [CLI exit codes](../../crates/gaze-cli/README.md#safety-net).

### Nym-small

Run `gaze setup`, then `gaze clean --policy gaze.toml`. Nym defaults to building
numbers, licence plates, usernames, and dates of birth. Configure allowlists
and thresholds in `[safety_net.nym]`; `[safety_net].backend = "nym"` activates it.
See the [Nym contract, measurements, and open items](../../docs/explanation/safety-net/safety-nets.md#nym-small-adapter).
