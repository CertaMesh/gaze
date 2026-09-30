# PII-Tracer on Gaze's corpus

Perplexity's [PII-Tracer](https://huggingface.co/perplexity-ai/PII-Tracer) is a 0.6B on-device PII detector. This page scores it on the same corpus, scorer and scored-label contracts as the other competitors in [competitors.md](competitors.md), full-corpus layer C. Byte leaks, false positives and character F2 use the shared scorer. Only aggregates are published. Gaze rows appear on competitors.md, where only tagged releases are shown.

**Declared configuration** (fixed from the model card before any result): model `perplexity-ai/PII-Tracer` at revision `d25c16f2e57e321f6d2527715c01df9112f956f5` (MIT); cpu, stored bfloat16, `predict()` decoding, non-overlapping 4080-token windows for longer documents, no threshold (the card documents none, so there is no sweep). torch 2.14.0, transformers 5.17.0. The checkpoint's own Python code runs under `trust_remote_code`; the worker refuses to load unless five pinned file hashes match.

**Corpus.** Identical to `comparison.json` (measured on agentic generator v4, 6,160 documents). Later generator versions add documents that are not measured here.

## Layer C, scored-label contract v3

| Tool | Char F2 | Leaked B | FP B | Documents leaking |
| --- | ---: | ---: | ---: | ---: |
| pii-tracer | 0.871 | 7,571 | 55,671 | 32.0% |
| gliner-high-recall | 0.795 | 16,900 | 69,716 | 53.7% |
| gliner | 0.794 | 20,152 | 53,355 | 58.0% |
| opf | 0.797 | 24,080 | 31,254 | 66.0% |
| presidio-strong-high-recall | 0.766 | 24,776 | 51,595 | 86.5% |
| presidio-strong | 0.765 | 25,314 | 49,720 | 87.3% |
| datafog-gliner | 0.754 | 26,834 | 49,434 | 75.8% |
| datafog-spacy | 0.565 | 29,142 | 246,970 | 86.3% |
| presidio-all | 0.719 | 30,615 | 58,274 | 83.5% |
| presidio-en-de | 0.719 | 30,615 | 58,274 | 83.5% |
| presidio-en | 0.454 | 71,562 | 30,749 | 87.0% |
| scrubadub-spacy | 0.360 | 84,397 | 15,310 | 97.0% |
| datafog-regex | 0.234 | 99,234 | 6,919 | 100.0% |
| scrubadub-base | 0.196 | 103,601 | 1,525 | 100.0% |
| datafog-core | 0.194 | 103,617 | 5,992 | 100.0% |

## PII-Tracer by layer and contract (leaked B / false-positive B)

| Layer | v3 | v2 | v1 |
| --- | ---: | ---: | ---: |
| C (prose holdout) | 7,571 / 55,671 | 7,571 / 55,671 | 7,631 / 56,359 |
| A (generated identifiers) | 3,213 / 6,773 | 3,213 / 6,773 | 3,213 / 6,773 |
| D (benign lookalikes) | 0 / 7,703 | 0 / 7,703 | 0 / 7,703 |
| R (repeated values) | 140 / 1,104 | 140 / 1,104 | 140 / 1,104 |

Layers A, D and R are generated. Layer D has no gold: every byte is a false positive.

## Latency

Latency is withheld. Other processes used up to 995% of a core while this ran (the rule is at most 100%), so no timing is comparable.

Harness `7b09e385`, macOS-26.5-arm64-arm-64bit. Worker `4f6727289fe1`, dependency lock `81b1d60bbab1`.
