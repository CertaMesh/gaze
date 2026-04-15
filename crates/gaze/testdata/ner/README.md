# Gaze NER testdata

This directory holds **test fixtures** for the `NerDetector` load contract.
No real model weights live here and none ever should — the runtime weights
are fetched out-of-repo via `scripts/fetch-ner-model.sh`.

## What's in here

- `SHA256SUMS.example` — mirrors the production SHA256SUMS shape with four
  placeholder hashes. Unit tests that build a temp model dir copy this
  file, write matching (fake) artifact bytes, and then exercise the
  checksum / missing-artifact fail-closed paths without any model.
- `labels.example.json` — example label map used by the fetch script and
  by integration tests that verify `labels.json` parsing. `MISC` maps to
  `"drop"` by default (the sentinel that causes `NerDetector` to silently
  skip those spans).
- Production `config.json` may include `"backend": "ort"` (or another
  supported driver name in the future). `NerDetector::load` reads that
  field to choose the runtime backend while keeping the pipeline API
  unchanged.

## Running the span-correctness tests (real model)

The unit tests under `crates/gaze/src/ner.rs` cover load-contract failure
modes without a real model. Real German/English span correctness tests
need the pinned artifact on disk.

1. Run `scripts/fetch-ner-model.sh` once to populate
   `${XDG_DATA_HOME:-~/.local/share}/gaze/models/davlan-mbert-ner-hrl/`.
2. Export `GAZE_NER_MODEL_DIR` pointing at that directory.
3. Run `cargo test -p gaze -- --ignored ner_span_correctness`.

Tests gated by `#[ignore]` or the `GAZE_NER_MODEL_DIR` env var never run
in CI. They are operator-invoked smoke tests for a specific artifact
version.
