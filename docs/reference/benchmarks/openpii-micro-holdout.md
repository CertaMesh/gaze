# OpenPII Micro External Holdout

## Decision

Gaze keeps the validation split of `ai4privacy/pii-masking-micro-100k` as a
secondary multilingual masking holdout. This split is evaluation-only: its UIDs
must never enter a training, fine-tuning, prompt-example, dictionary,
rule-authoring, or threshold-selection corpus. The primary product-language
scorecard is now the pinned English/German Dataiku test split documented in
[`dataiku-en-de-holdout.md`](dataiku-en-de-holdout.md).

The corpus is a useful first gate because it is entirely synthetic, licensed
under CC BY 4.0, and supplies character-level spans across 30 languages and 37
language-region combinations. Those properties let the benchmark exercise
multilingual offsets without putting real personal data into Gaze fixtures.

It is not a sufficient final benchmark. Every source row contains at least one
annotated entity, and synthetic templates do not reproduce all ambiguity,
formatting, OCR, tool-call JSON, or tenant-specific PII found in agentic
workflows. A separate synthetic negative-only corpus and agentic-domain holdout
remain required before a replacement detector can ship.

## Immutable provenance

| Field | Pinned value |
| --- | --- |
| Repository | `ai4privacy/pii-masking-micro-100k` |
| Revision | `3cd59c65631280839f830d3ba96dcdfe1785cab1` |
| File | `data/validation.jsonl` |
| License | `CC-BY-4.0` |
| Declared data kind | synthetic PII only |
| Rows | `9,990` |
| Bytes | `32,536,978` |
| SHA-256 | `bb15da1b5fbb11b3cc6fd4c95eca256197573ecd066230eb3c1fe6898f27a578` |

The benchmark runner refuses any file whose size or digest differs. It also
validates the split marker, UID uniqueness, entity bounds, and every annotated
substring before sending a document to Gaze.

The pinned file contains 72,087 annotations. Its observed schema has 26 label
values, including a very small tail beyond the 19 labels advertised in the
dataset card. The runner reports observed counts from the file rather than
silently dropping that tail.

## Scoring contract

The primary score asks whether annotated PII bytes would reach a downstream
LLM. For each document, the runner merges adjacent or overlapping gold spans,
merges Gaze manifest spans, and measures their intersection in UTF-8 bytes.

Primary metrics are:

- PII byte recall and its complement, leaked-byte rate;
- byte precision, F1, recall-weighted F2, and non-PII-byte false-positive rate;
- zero-leak document rate;
- full-entity coverage recall, where any uncovered byte makes the entity a
  miss.

Entity overlap and exact-boundary recall are diagnostics. Label matching is not
part of the primary score because pseudonymization safety depends first on
covering the bytes, and Gaze's class vocabulary does not exactly match every
source taxonomy. Per-label recall still exposes class-shaped coverage gaps.

The runner publishes two additional recall slices:

- direct identifiers, such as names, contact details, account identifiers, and
  addresses;
- contextual PII, such as dates, ages, titles, sex, gender, time, amount, and
  currency.

All labels remain in the primary all-PII score. The slices do not weaken the
fail-closed contract.

## Baseline cells

The default run compares:

1. `rule-floor-extended`: the shipped deterministic recognizers;
2. `pass2-ner`: that same floor plus the configured `NerRecognizer`, using the
   model bundle named by `GAZE_NER_MODEL_DIR` and threshold `0.3` by default;
3. `full-stack-kiji-resolve`: Pass 2 plus the in-process Kiji SafetyNet using
   the shipped Resolve/fallback policy, exact restore checks, manifest-integrity
   checks, and a post-policy SafetyNet scan.

The harness initializes each pipeline once and streams every document through
the same process. Per-document timings exclude Cargo compilation and model
loading; process start-to-first-response is reported separately.

Run the pinned default benchmark:

```bash
python3 scripts/bench/openpii_gaze_bench.py
```

Run a fast English/German diagnostic without changing the primary holdout:

```bash
python3 scripts/bench/openpii_gaze_bench.py --no-download --language en --language de
```

Result JSON is written to
`target/bench-data/openpii-micro/gaze-benchmark.json` by default. That directory
is ignored and must not be treated as a training-data source.

## Other evaluated datasets

PIIMB has an excellent masking-oriented, character-level methodology and adds
multiple sources plus negative sentences. Its assembled benchmark is CC
BY-NC 4.0, so it is not the default reproducible corpus for Gaze's unrestricted
open-source/commercial adopter workflow. REDACT remains useful as a future
curated secondary evaluation, but its files require access approval. Neither
constraint justifies weakening the current synthetic holdout gate.
