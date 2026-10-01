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
  files, and so does `tune.py split` for the corpus text and gold
  (`validation.pickle`, `test.pickle`). `tune.py select` opens
  `*.validation.jsonl` and `validation.pickle` only;
  `test_select_never_opens_the_test_half` runs the whole selection, through the
  real loader, with every test-half file unreadable, and
  `corpus.require_validation` refuses any test-half id that reaches the search.
  The committed selection was made by an earlier loader that built the whole
  corpus in memory and dropped the test half before the search; rerunning
  `select` through the split files reproduces its choices and every validation
  score exactly.
- **The split holds out document ids, not values.** Both halves come from the
  same generators, so the test half shares templates, generator groups and
  exact gold values with the validation half the custom recognizers were
  written from. [`overlap.py`](overlap.py) measures that dependence and the
  results page shows it beside the tuned rows.
- **Same documents and contracts as the comparison.** `corpus.load_measured`
  rebuilds exactly the corpus `comparison.json` measured (its agentic layers come
  from an older generator) and refuses on any identity mismatch.
- **Replay equals Presidio.** Candidates are scored by replaying recorded raw
  findings through Presidio's own threshold, de-duplication and allow-list steps
  and the comparison's resolver. A model-free test proves the replay equals
  `AnalyzerEngine.analyze`. `tune.py measure` also replays the comparison's
  `presidio-all` configuration and requires every committed byte count, and runs
  each chosen configuration live on a fixed sample of about one document in eight
  (every layer, both halves). A document whose live output differs is rerun
  twice; the page publishes both counts and refuses any persistent difference in
  covered bytes (a label-only difference, from Presidio's tie-break between
  equal-score results on one span, is published, not hidden).
- **Objectives.** `leak-first` is `compare.py`'s own threshold rule (fewest
  validation v3 leaked bytes, then fewest false-positive bytes, over C/A/D/R).
  Because a free search can satisfy that rule by redacting nearly everything,
  `f2` (highest validation v3 character F2, the panels' headline metric) is
  searched too. Both choices are published.
- **Measurement budget lineage.** `tune.py measure --budget-source-revision`
  requires a full committed SHA and counts rulepack commits, dates and path at
  that revision, recording `budget.source_revision`. This is the original
  measurement's effort budget, not the current branch's rulepack history.
  Later rulepack commits cannot alter a refresh of that budget; an explicitly
  chosen new lineage counts them. Missing, invalid or empty rulepack lineage
  fails before corpus loading or model execution.

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
export TUNED_BUDGET_SOURCE_REVISION="$(git show \
  e2f6a68474fbfe7cbe97f2f9844ef50e99e69c12:docs/reference/benchmarks/presidio-tuned.json \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["harness_revision"])')"
export TUNED_COMPARE_PYTHON=... TUNED_VENDOR_PYTHON=... TUNED_OUT=/outside/the/repo \
  TUNED_PRESIDIO_RESEARCH=... TUNED_DAVLAN=... TUNED_OPENMED=... \
  GAZE_COMPARE_EN_MODEL=... GAZE_COMPARE_DE_MODEL=... GAZE_COMPARE_NL_MODEL=... \
  GAZE_COMPARE_FR_MODEL=... GAZE_COMPARE_PT_MODEL=... GAZE_COMPARE_TRANSFORMER_MODEL=... \
  GAZE_COMPARE_GLINER_MODEL=... GAZE_COMPARE_GLINER_TOKENIZER=...
scripts/bench/compare/tuned_presidio/run-tuned.sh
```

The immutable committed report above resolves the original budget lineage to
`ef369b33525abaeb1c0d487edda43ebf4c9761af`. Do not derive it from a working report
that a refresh has overwritten. The budget is recomputed from this revision,
not copied as a fixed count. Source digests and `harness_revision` still record
the code that actually executes each fresh measurement.

The recordings and live runs take a few CPU hours (GLiNER dominates: Presidio
hands it every other entity name in the registry as an extra label). Raw
findings stay under `$TUNED_OUT`; the repository holds aggregates only.
