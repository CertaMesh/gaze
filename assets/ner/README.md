# NER Adopter Assets

These files are adopter-facing contracts, not test fixtures. They define the
v0.5.2 default NER bundle that framework adapters can install and verify.

## Files

- `labels.davlan-mbert.json` maps the model BIO tags to Gaze class strings.
  Values such as `"Name"`, `"Location"`, and `"Organization"` resolve to built
  in classes. The `"drop"` sentinel means the detector skips that label. The
  onnx-community mirror exposes `DATE` labels, but this contract drops them
  because Gaze does not treat general-prose dates as PII by default.
- `policy-snippet.davlan-mbert.toml` is the canonical `[ner]` block plus
  class-map rules. Copy it into `policy.toml`, then adjust actions only after
  reviewing the restore/audit consequences.
- The repository-root `SHA256SUMS` verifies the installed model bundle. The
  fetch script copies this manifest into the model directory and fails closed
  on any mismatch.

## Source

The pinned model source is
`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` at commit
`cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`. The runtime `model.onnx` file is
downloaded from the mirror path `onnx/model_int8.onnx`.

## Future CLI Path

In v0.6.2+, `gaze model fetch <name>` and `gaze policy snippet ner` will read
these contracts from an embedded manifest (todos #294 and #302). Until then,
adapters should copy these files from a pinned Gaze revision.
