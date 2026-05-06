# NER Adopter Assets

These files are adopter-facing contracts, not test fixtures. They define the
default NER bundle that framework adapters can install and verify.

## Files

- `labels.davlan-mbert.json` maps the model BIO tags to Gaze class strings.
  Values such as `"Name"`, `"Location"`, and `"Organization"` resolve to
  built-in classes. The `"drop"` sentinel means the detector skips that label.
  The onnx-community mirror exposes `DATE` labels, but this contract drops them
  because Gaze does not treat general-prose dates as PII by default.
- `policy-snippet.davlan-mbert.toml` is the canonical `[ner]` block plus
  class-map rules. Copy it into `policy.toml`, then adjust actions only after
  reviewing the restore/audit consequences.

## Source

The pinned model source is
`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
`cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`. The runtime `model.onnx` file is
downloaded from the mirror path `onnx/model_int8.onnx`.

Artifact checksums are embedded in the `gaze-recognizers` crate and verified
at load time. Any mismatch causes a fail-closed startup error.

## Installation

Until a first-class `gaze model fetch` CLI command ships, adapters should
obtain the model files by running the install script shipped with the CLI or
by copying these asset files from a pinned Gaze revision and fetching the
ONNX files from the mirror path above.
