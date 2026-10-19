# PII-Tracer on Gaze's corpus

Perplexity's [PII-Tracer](https://huggingface.co/perplexity-ai/PII-Tracer) is a 0.6B on-device PII detector. This page scores it on the same corpus, scorer and scored-label contracts as the other competitors in [competitors.md](competitors.md), full-corpus layer C. Byte leaks, false positives and character F2 use the shared scorer. Only aggregates are published. Gaze rows appear on competitors.md, where only tagged releases are shown.

**Declared configuration** (fixed from the model card before any result): model `perplexity-ai/PII-Tracer` at revision `d25c16f2e57e321f6d2527715c01df9112f956f5` (MIT); cpu, stored bfloat16, `predict()` decoding, non-overlapping 4080-token windows for longer documents, no threshold (the card documents none, so there is no sweep). torch 2.14.0, transformers 5.17.0. The checkpoint's own Python code runs under `trust_remote_code`; the worker refuses to load unless five pinned file hashes match.

**Corpus.** Identical to `comparison.json` (measured on agentic generator v20, 17,780 documents). Later generator versions add documents that are not measured here.

## Layer C, scored-label contract v4

| Tool | Char F2 | Leaked B | FP B | Documents leaking |
| --- | ---: | ---: | ---: | ---: |
| pii-tracer | 0.876 | 7,631 | 56,359 | 32.4% |
| gliner-high-recall | 0.770 | 22,699 | 69,716 | 62.6% |
| gliner | 0.765 | 26,515 | 53,355 | 67.6% |
| presidio-strong-high-recall | 0.741 | 30,517 | 51,595 | 90.0% |
| presidio-strong | 0.740 | 31,055 | 49,720 | 90.5% |
| datafog-spacy | 0.561 | 33,270 | 246,970 | 89.1% |
| datafog-gliner | 0.725 | 33,349 | 49,434 | 82.7% |
| presidio-all | 0.704 | 35,130 | 58,274 | 86.6% |
| presidio-en-de | 0.704 | 35,130 | 58,274 | 86.6% |
| presidio-en | 0.446 | 76,658 | 30,749 | 88.9% |
| scrubadub-spacy | 0.356 | 89,546 | 15,310 | 97.6% |
| datafog-regex | 0.223 | 105,885 | 6,919 | 100.0% |
| scrubadub-base | 0.187 | 110,166 | 1,525 | 100.0% |
| datafog-core | 0.184 | 110,273 | 5,992 | 100.0% |
| opf | N/A | N/A | N/A | N/A |

OPF is unavailable after two unchanged 600-second socket timeouts; see the receipts in [competitors.md](competitors.md). No partial metrics are published.

## PII-Tracer by layer and contract (leaked B / false-positive B)

| Layer | v4 | v3 | v2 | v1 |
| --- | ---: | ---: | ---: | ---: |
| C (prose holdout) | 7,631 / 56,359 | 7,571 / 55,671 | 7,571 / 55,671 | 7,631 / 56,359 |
| A (generated identifiers) | 28,038 / 14,961 | 28,038 / 14,961 | 28,038 / 14,961 | 28,038 / 14,961 |
| D (benign lookalikes) | 0 / 53,564 | 0 / 53,564 | 0 / 53,564 | 0 / 53,564 |
| R (repeated values) | 44,994 / 4,428 | 44,994 / 4,428 | 44,994 / 4,428 | 44,994 / 4,428 |

Layers A, D and R are generated. Layer D has no gold: every byte is a false positive.

## Latency

Latency is withheld. Other processes used up to 1075% of a core while this ran (the rule is at most 100%), so no timing is comparable.

Harness `97cfca1f`, macOS-26.5-arm64-arm-64bit. Worker `4f6727289fe1`, dependency lock `81b1d60bbab1`.
