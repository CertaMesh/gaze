# Tuned Presidio on Gaze's own corpus

The comparison in [`../README.md`](../README.md) runs Presidio with documented
default and "strong" configurations. This directory adds two tuned kinds of
Presidio row, so that Presidio is measured at its best, not at its defaults:

- **Tuned by its authors** (`presidio-tuned-presidio-research`): Presidio
  Research's own tuned analyzer for its corpus (notebook 5, the setup
  [`../theirbench`](../theirbench/README.md) reproduces), applied unchanged. Only
  the label map is ours. The setup supports English only, so every document is
  analyzed as English.
- **Tuned for this corpus** (`presidio-tuned-own-leak-first`,
  `presidio-tuned-own-f2`): a search over the space declared in
  [`space.py`](space.py), selected on the validation half and reported on the
  disjoint test half.

Results: [`competitors.md`](../../../../docs/reference/benchmarks/competitors.md#tuned-presidio).
The panels' own-corpus Presidio bar is the tuned row with the higher test-half
layer C character F2; the defaults stay in the detail tables.

## Rules the harness enforces

- **Declared before any run.** `space.py` was committed before any candidate
  was scored. Every recording records its SHA-256, and `tune.py` refuses a
  recording or a selection made under a different space.
- **Validation only.** The split is the comparison's own (SHA-256 first byte of
  the document id below 128). Each recording pass writes the halves to separate
  files. `tune.py select` opens `*.validation.jsonl` and validation documents
  only; `test_select_never_opens_the_test_half` runs the whole selection with
  every test-half file unreadable, and `corpus.require_validation` refuses any
  test-half id that reaches the search.
- **Same documents and contracts as the comparison.** `corpus.load_measured`
  rebuilds exactly the corpus `comparison.json` measured (its agentic layers come
  from an older generator) and refuses on any identity mismatch.
- **Replay equals Presidio.** Candidates are scored by replaying recorded raw
  findings through Presidio's own threshold, de-duplication and allow-list steps
  and the comparison's resolver. A model-free test proves the replay equals
  `AnalyzerEngine.analyze`. `tune.py measure` also replays the comparison's
  `presidio-all` configuration and requires every committed byte count, and runs
  each chosen configuration live over every document, requiring output identical
  to the replay.
- **Objectives.** `leak-first` is `compare.py`'s own threshold rule (fewest
  validation v3 leaked bytes, then fewest false-positive bytes, over C/A/D/R).
  Because a free search can satisfy that rule by redacting nearly everything,
  `f2` (highest validation v3 character F2, the panels' headline metric) is
  searched too. Both choices are published.

## The search

Coordinate descent from two starts (`presidio-default`, the comparison's
`presidio-all` configuration, and `everything`): each round tries every
single-unit move in a fixed order and keeps a move only when it strictly improves
the objective. The units are the NLP-engine NER, the extra NER recognizers (off,
English, every language), every predefined recognizer (off, native languages,
every language), each custom recognizer, the context enhancer, one threshold per
recognizer and entity, and an allow list learned from validation false positives.
Every evaluated candidate and its validation score is in
[`presidio-tuned-search-log.jsonl.gz`](../../../../docs/reference/benchmarks/presidio-tuned-search-log.jsonl.gz);
the choices are in
[`presidio-tuned-selection.json`](../../../../docs/reference/benchmarks/presidio-tuned-selection.json).

## Reproduce

Use the comparison's environment (`../requirements.lock` plus the spaCy wheels in
`../model-wheels.json`) and the vendor environment
(`../theirbench/requirements-theirbench.lock`) with a presidio-research checkout
at the commit `../theirbench/vendor-tuned.json` pins. Model snapshots are pinned
by revision in `space.py` and by tree digest in [`model-pins.json`](model-pins.json):
the Davlan and OpenMed snapshots are downloaded with `huggingface_hub.snapshot_download`
at those revisions, then their `.cache` directories removed.

```sh
export TUNED_COMPARE_PYTHON=... TUNED_VENDOR_PYTHON=... TUNED_OUT=/outside/the/repo \
  TUNED_PRESIDIO_RESEARCH=... TUNED_DAVLAN=... TUNED_OPENMED=... \
  GAZE_COMPARE_EN_MODEL=... GAZE_COMPARE_DE_MODEL=... GAZE_COMPARE_NL_MODEL=... \
  GAZE_COMPARE_FR_MODEL=... GAZE_COMPARE_PT_MODEL=... GAZE_COMPARE_TRANSFORMER_MODEL=... \
  GAZE_COMPARE_GLINER_MODEL=... GAZE_COMPARE_GLINER_TOKENIZER=...
scripts/bench/compare/tuned_presidio/run-tuned.sh
```

The recordings and live runs take a few CPU hours (GLiNER dominates: Presidio
hands it every other entity name in the registry as an extra label). Raw
findings stay under `$TUNED_OUT`; the repository holds aggregates only.
