# Competitor comparison

Same corpus and scorer; tools run with documented configurations. UTF-8 byte counts use the Gaze scorer. For v4 and v3, FP is the scorer's false-positive count after its audited gold-gap credit. Presidio all runs English, German, Dutch, French, and Portuguese spaCy models with the documented German recognizers. Presidio English default is a secondary row. Latency was not measured under a quiet machine; timing comparisons are withheld. This measures detection; competitor restore and manifest behavior is not scored. The agentic-layer rows were measured on generator v4 (test corpus `387a35ac1551…`, 3,250 documents). The current generator is v13; its added documents are not measured here.

Leaked and false-positive byte counts are class-agnostic. A skipped document's scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to get leakage on processed documents. For example, Presidio English-only v4 Synthetic identifiers in agentic formats leaks 34,733 B, including 28,151 B of scored gold from 1,320 skipped non-English documents. The reviewed label map controls repeated-gold credit and the exact typed-span metrics below.

Gaze is not listed: the comparison run measured an unreleased build, and this page shows tagged Gaze releases only. Released Gaze numbers are in the [release history](README.md#release-history) and the benchmark panels. Home-normalized setup policy SHA-256: `481f5df7a9b0b562bf2c2db7274cfbf2ea701231c52f1287b27de00554453ff1`.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`. GLiNER uses model-card labels, library threshold 0.5. OpenAI Privacy Filter (OPF) 0.1.0 at source `f7f00ca7fb869683eb732c010299d901457f19c3`.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published.

| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | Latency |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| v4 | Synthetic identifiers in agentic formats | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-all | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-en | 76,658 | 30,749 | 1,545 | 1,365 | 57,835 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-en-de | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 31,055 | 49,720 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 30,517 | 51,595 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-core | 110,273 | 5,992 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-regex | 105,885 | 6,919 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-spacy | 33,270 | 246,970 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-gliner | 33,349 | 49,434 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | scrubadub-base | 110,166 | 1,525 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | scrubadub-spacy | 89,546 | 15,310 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | gliner | 26,515 | 53,355 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | gliner-high-recall | 22,699 | 69,716 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | opf | 25,485 | 31,383 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-all | 30,615 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-en | 71,562 | 26,086 | 1,545 | 1,365 | 54,629 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-en-de | 30,615 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 25,314 | 40,898 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 24,776 | 42,768 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | datafog-core | 103,617 | 5,839 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | datafog-regex | 99,234 | 6,766 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | datafog-spacy | 29,142 | 243,401 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | datafog-gliner | 26,834 | 47,142 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | scrubadub-base | 103,601 | 1,449 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | scrubadub-spacy | 84,397 | 11,942 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | gliner | 20,152 | 51,384 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | gliner-high-recall | 16,900 | 67,549 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Kiji EN/DE holdout and A4 negatives | opf | 24,080 | 27,544 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-all | 30,615 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-en | 71,562 | 30,749 | 1,545 | 1,365 | 54,629 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-en-de | 30,615 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 25,314 | 49,720 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 24,776 | 51,595 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | datafog-core | 103,617 | 5,992 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | datafog-regex | 99,234 | 6,919 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | datafog-spacy | 29,142 | 246,970 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | datafog-gliner | 26,834 | 49,434 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | scrubadub-base | 103,601 | 1,525 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | scrubadub-spacy | 84,397 | 15,310 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | gliner | 20,152 | 53,355 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | gliner-high-recall | 16,900 | 69,716 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Kiji EN/DE holdout and A4 negatives | opf | 24,080 | 31,254 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-all | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-en | 76,658 | 30,749 | 1,545 | 1,365 | 57,835 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-en-de | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 31,055 | 49,720 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 30,517 | 51,595 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | datafog-core | 110,273 | 5,992 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | datafog-regex | 105,885 | 6,919 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | datafog-spacy | 33,270 | 246,970 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | datafog-gliner | 33,349 | 49,434 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | scrubadub-base | 110,166 | 1,525 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | scrubadub-spacy | 89,546 | 15,310 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | gliner | 26,515 | 53,355 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | gliner-high-recall | 22,699 | 69,716 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Kiji EN/DE holdout and A4 negatives | opf | 25,485 | 31,383 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |

## Heldout safety and entity metrics

The validation/test split is fixed by document ID. Thresholds are selected on validation; the table below reports the disjoint test half. Product coverage scores unsupported gold as missed. Common intersection scores only classes claimed by every listed configuration. Entity scores require an exact UTF-8 byte span and a compatible reviewed label mapping.

Common classes: CREDITCARDNUMBER, DATEOFBIRTH, EMAIL, PHONENUMBER, ZIP.

| Contract | Layer | View | Tool | PII docs | Leaked B | FP B | Leaking docs | Doc leak % | Leaking entities | Entity leak % | Redaction load % | TP | FP | FN | Entity P | Entity R | F1 | F2 |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 1,119 | 10,209 | 5,468 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 1,119 | 16,395 | 2,379 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 1,119 | 12,942 | 4,409 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 1,119 | 10,654 | 4,864 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 1,119 | 10,510 | 4,864 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 1,119 | 14,872 | 989 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 1,119 | 11,258 | 670 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 1,119 | 9,119 | 4,812 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 1,119 | 10,181 | 1,117 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 1,119 | 19,659 | 79 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 1,119 | 19,039 | 982 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | gliner | 1,119 | 6,300 | 3,051 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 1,119 | 3,823 | 4,878 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | opf | 1,119 | 5,335 | 2,600 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 166 | 722 | 2,655 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 166 | 1,113 | 907 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 166 | 722 | 2,014 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 166 | 879 | 749 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 166 | 100 | 670 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 166 | 40 | 2,170 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 166 | 0 | 875 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 166 | 1,965 | 79 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 166 | 2,163 | 79 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | gliner | 166 | 171 | 143 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 166 | 95 | 202 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | opf | 166 | 197 | 1,222 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,861 | 4,427 | 4,870 | 0.393 | 0.370 | 0.381 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 39,889 | 15,145 | 872 | 87.9 | 4,792 | 62.0 | 8.8 | 1,614 | 2,443 | 6,117 | 0.398 | 0.209 | 0.274 | 0.231 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 16,067 | 25,514 | 890 | 89.7 | 2,284 | 29.5 | 15.7 | 3,212 | 3,902 | 4,519 | 0.452 | 0.415 | 0.433 | 0.422 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 15,855 | 26,356 | 886 | 89.3 | 2,265 | 29.3 | 15.9 | 3,217 | 4,047 | 4,514 | 0.443 | 0.416 | 0.429 | 0.421 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 57,944 | 3,073 | 992 | 100.0 | 6,977 | 90.2 | 2.7 | 671 | 408 | 7,060 | 0.622 | 0.087 | 0.152 | 0.105 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 55,630 | 3,179 | 992 | 100.0 | 6,865 | 88.8 | 3.2 | 744 | 528 | 6,987 | 0.585 | 0.096 | 0.165 | 0.116 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 17,316 | 131,449 | 883 | 89.0 | 2,135 | 27.6 | 36.8 | 2,663 | 9,833 | 5,068 | 0.213 | 0.344 | 0.263 | 0.307 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 17,681 | 24,786 | 819 | 82.6 | 1,642 | 21.2 | 15.2 | 1,727 | 3,880 | 6,004 | 0.308 | 0.223 | 0.259 | 0.236 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 57,771 | 703 | 992 | 100.0 | 7,213 | 93.3 | 2.3 | 440 | 115 | 7,291 | 0.793 | 0.057 | 0.106 | 0.070 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 47,004 | 7,610 | 966 | 97.4 | 5,809 | 75.1 | 5.8 | 819 | 1,387 | 6,912 | 0.371 | 0.106 | 0.165 | 0.124 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 13,551 | 26,371 | 660 | 66.5 | 1,327 | 17.2 | 16.4 | 2,014 | 3,517 | 5,717 | 0.364 | 0.261 | 0.304 | 0.276 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 11,640 | 34,318 | 613 | 61.8 | 1,146 | 14.8 | 18.4 | 2,040 | 4,218 | 5,691 | 0.326 | 0.264 | 0.292 | 0.274 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | opf | 992 | 12,904 | 16,056 | 667 | 67.2 | 1,417 | 18.3 | 14.4 | 1,472 | 3,336 | 6,259 | 0.306 | 0.190 | 0.235 | 0.206 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-all | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en | 781 | 6,713 | 6,005 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en-de | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong | 781 | 3,666 | 9,822 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong-high-recall | 781 | 3,646 | 10,017 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-core | 781 | 3,977 | 1,054 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-regex | 781 | 3,362 | 2,258 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-spacy | 781 | 2,725 | 20,978 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-gliner | 781 | 886 | 11,668 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-base | 781 | 6,122 | 519 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-spacy | 781 | 6,331 | 414 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner | 781 | 971 | 7,433 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner-high-recall | 781 | 708 | 8,739 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v4 | Kiji EN/DE holdout and A4 negatives | common_intersection | opf | 781 | 255 | 10,210 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 3,724 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 2,940 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 3,631 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 2,859 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 2,871 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 1,874 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 1,460 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 2,883 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 1,766 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 650 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 684 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 1,415 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | opf | 0 | 0 | 2,625 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 1,061 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 1,571 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 682 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 758 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 1,520 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 855 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 19 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 423 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | opf | 0 | 0 | 1,948 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-all | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-en | 53 | 1,638 | 121 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-en-de | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-strong | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-core | 53 | 1,478 | 0 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-regex | 53 | 1,140 | 0 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-spacy | 53 | 165 | 356 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-gliner | 53 | 216 | 184 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v4 | Repeated PII values with decoys | product_coverage | scrubadub-base | 53 | 1,495 | 0 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v4 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 53 | 910 | 91 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v4 | Repeated PII values with decoys | product_coverage | gliner | 53 | 241 | 176 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v4 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 53 | 109 | 600 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v4 | Repeated PII values with decoys | product_coverage | opf | 53 | 72 | 432 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-all | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-en | 40 | 428 | 30 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-en-de | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-strong | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-core | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-regex | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-spacy | 40 | 0 | 166 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-gliner | 40 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v4 | Repeated PII values with decoys | common_intersection | scrubadub-base | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 40 | 23 | 0 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v4 | Repeated PII values with decoys | common_intersection | gliner | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | opf | 40 | 0 | 329 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 1,119 | 10,209 | 5,468 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 1,119 | 16,395 | 2,379 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 1,119 | 12,942 | 4,409 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 1,119 | 10,654 | 4,864 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 1,119 | 10,510 | 4,864 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 1,119 | 14,872 | 989 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 1,119 | 11,258 | 670 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 1,119 | 9,119 | 4,812 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 1,119 | 10,181 | 1,117 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 1,119 | 19,659 | 79 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 1,119 | 19,039 | 982 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | gliner | 1,119 | 6,300 | 3,051 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 1,119 | 3,823 | 4,878 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | opf | 1,119 | 5,335 | 2,600 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 166 | 722 | 2,655 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 166 | 1,113 | 907 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 166 | 722 | 2,014 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 166 | 879 | 749 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 166 | 100 | 670 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 166 | 40 | 2,170 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 166 | 0 | 875 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 166 | 1,965 | 79 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 166 | 2,163 | 79 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | gliner | 166 | 171 | 143 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 166 | 95 | 202 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | opf | 166 | 197 | 1,222 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,861 | 4,348 | 4,646 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 37,308 | 15,145 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,614 | 2,383 | 5,893 | 0.404 | 0.215 | 0.281 | 0.237 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 13,143 | 25,514 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,212 | 3,863 | 4,295 | 0.454 | 0.428 | 0.441 | 0.433 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 12,931 | 26,356 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,217 | 4,008 | 4,290 | 0.445 | 0.429 | 0.437 | 0.432 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 54,530 | 3,073 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 52,216 | 3,179 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 15,252 | 131,449 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 14,327 | 24,786 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 54,427 | 703 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 44,411 | 7,610 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 10,250 | 26,371 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 8,660 | 34,318 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | opf | 992 | 12,185 | 15,986 | 649 | 65.4 | 1,368 | 18.2 | 13.9 | 1,369 | 3,260 | 6,138 | 0.296 | 0.182 | 0.226 | 0.198 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-all | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en | 781 | 6,713 | 6,005 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en-de | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong | 781 | 3,666 | 9,822 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong-high-recall | 781 | 3,646 | 10,017 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-core | 781 | 3,977 | 1,054 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-regex | 781 | 3,362 | 2,258 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-spacy | 781 | 2,725 | 20,978 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-gliner | 781 | 886 | 11,668 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-base | 781 | 6,122 | 519 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-spacy | 781 | 6,331 | 414 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner | 781 | 971 | 7,433 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner-high-recall | 781 | 708 | 8,739 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | opf | 781 | 255 | 10,210 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 3,724 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 2,940 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 3,631 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 2,859 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 2,871 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 1,874 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 1,460 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 2,883 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 1,766 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 650 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 684 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 1,415 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | opf | 0 | 0 | 2,625 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 1,061 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 1,571 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 682 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 758 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 1,520 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 855 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 19 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 423 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | opf | 0 | 0 | 1,948 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-all | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-en | 53 | 1,638 | 121 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-en-de | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-strong | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-core | 53 | 1,478 | 0 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-regex | 53 | 1,140 | 0 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-spacy | 53 | 165 | 356 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-gliner | 53 | 216 | 184 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v3 | Repeated PII values with decoys | product_coverage | scrubadub-base | 53 | 1,495 | 0 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v3 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 53 | 910 | 91 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v3 | Repeated PII values with decoys | product_coverage | gliner | 53 | 241 | 176 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v3 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 53 | 109 | 600 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v3 | Repeated PII values with decoys | product_coverage | opf | 53 | 72 | 432 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-all | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-en | 40 | 428 | 30 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-en-de | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-strong | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-core | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-regex | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-spacy | 40 | 0 | 166 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-gliner | 40 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v3 | Repeated PII values with decoys | common_intersection | scrubadub-base | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 40 | 23 | 0 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v3 | Repeated PII values with decoys | common_intersection | gliner | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | opf | 40 | 0 | 329 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 1,119 | 10,209 | 5,468 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 1,119 | 16,395 | 2,379 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 1,119 | 12,942 | 4,409 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 1,119 | 10,654 | 4,864 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 1,119 | 10,510 | 4,864 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 1,119 | 14,872 | 989 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 1,119 | 11,258 | 670 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 1,119 | 9,119 | 4,812 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 1,119 | 10,181 | 1,117 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 1,119 | 19,659 | 79 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 1,119 | 19,039 | 982 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | gliner | 1,119 | 6,300 | 3,051 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 1,119 | 3,823 | 4,878 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | opf | 1,119 | 5,335 | 2,600 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 166 | 722 | 2,655 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 166 | 1,113 | 907 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 166 | 722 | 2,014 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 166 | 879 | 749 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 166 | 100 | 670 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 166 | 40 | 2,170 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 166 | 0 | 875 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 166 | 1,965 | 79 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 166 | 2,163 | 79 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | gliner | 166 | 171 | 143 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 166 | 95 | 202 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | opf | 166 | 197 | 1,222 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,861 | 4,348 | 4,646 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 37,308 | 15,145 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,614 | 2,383 | 5,893 | 0.404 | 0.215 | 0.281 | 0.237 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 13,143 | 25,514 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,212 | 3,863 | 4,295 | 0.454 | 0.428 | 0.441 | 0.433 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 12,931 | 26,356 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,217 | 4,008 | 4,290 | 0.445 | 0.429 | 0.437 | 0.432 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 54,530 | 3,073 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 52,216 | 3,179 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 15,252 | 131,449 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 14,327 | 24,786 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 54,427 | 703 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 44,411 | 7,610 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 10,250 | 26,371 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 8,660 | 34,318 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | opf | 992 | 12,185 | 15,986 | 649 | 65.4 | 1,368 | 18.2 | 13.9 | 1,369 | 3,260 | 6,138 | 0.296 | 0.182 | 0.226 | 0.198 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-all | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en | 781 | 6,713 | 6,005 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en-de | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong | 781 | 3,666 | 9,822 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong-high-recall | 781 | 3,646 | 10,017 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-core | 781 | 3,977 | 1,054 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-regex | 781 | 3,362 | 2,258 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-spacy | 781 | 2,725 | 20,978 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-gliner | 781 | 886 | 11,668 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-base | 781 | 6,122 | 519 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-spacy | 781 | 6,331 | 414 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner | 781 | 971 | 7,433 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner-high-recall | 781 | 708 | 8,739 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | opf | 781 | 255 | 10,210 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 3,724 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 2,940 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 3,631 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 2,859 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 2,871 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 1,874 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 1,460 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 2,883 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 1,766 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 650 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 684 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 1,415 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | opf | 0 | 0 | 2,625 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 1,061 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 1,571 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 682 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 758 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 1,520 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 855 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 19 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 423 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | opf | 0 | 0 | 1,948 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-all | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-en | 53 | 1,638 | 121 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-en-de | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-strong | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-core | 53 | 1,478 | 0 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-regex | 53 | 1,140 | 0 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-spacy | 53 | 165 | 356 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-gliner | 53 | 216 | 184 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v2 | Repeated PII values with decoys | product_coverage | scrubadub-base | 53 | 1,495 | 0 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v2 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 53 | 910 | 91 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v2 | Repeated PII values with decoys | product_coverage | gliner | 53 | 241 | 176 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v2 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 53 | 109 | 600 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v2 | Repeated PII values with decoys | product_coverage | opf | 53 | 72 | 432 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-all | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-en | 40 | 428 | 30 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-en-de | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-strong | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-core | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-regex | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-spacy | 40 | 0 | 166 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-gliner | 40 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v2 | Repeated PII values with decoys | common_intersection | scrubadub-base | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 40 | 23 | 0 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v2 | Repeated PII values with decoys | common_intersection | gliner | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | opf | 40 | 0 | 329 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 1,119 | 10,209 | 5,468 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 1,119 | 16,395 | 2,379 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 1,119 | 12,942 | 4,409 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 1,119 | 10,654 | 4,864 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 1,119 | 10,510 | 4,864 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 1,119 | 14,872 | 989 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 1,119 | 11,258 | 670 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 1,119 | 9,119 | 4,812 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 1,119 | 10,181 | 1,117 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 1,119 | 19,659 | 79 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 1,119 | 19,039 | 982 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | gliner | 1,119 | 6,300 | 3,051 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 1,119 | 3,823 | 4,878 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | opf | 1,119 | 5,335 | 2,600 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 166 | 722 | 2,655 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 166 | 1,113 | 907 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 166 | 722 | 2,014 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 166 | 779 | 2,701 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 166 | 879 | 749 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 166 | 100 | 670 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 166 | 40 | 2,170 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 166 | 0 | 875 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 166 | 1,965 | 79 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 166 | 2,163 | 79 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | gliner | 166 | 171 | 143 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 166 | 95 | 202 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | opf | 166 | 197 | 1,222 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,861 | 4,427 | 4,870 | 0.393 | 0.370 | 0.381 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 39,889 | 15,145 | 872 | 87.9 | 4,792 | 62.0 | 8.8 | 1,614 | 2,443 | 6,117 | 0.398 | 0.209 | 0.274 | 0.231 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 16,067 | 25,514 | 890 | 89.7 | 2,284 | 29.5 | 15.7 | 3,212 | 3,902 | 4,519 | 0.452 | 0.415 | 0.433 | 0.422 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 15,855 | 26,356 | 886 | 89.3 | 2,265 | 29.3 | 15.9 | 3,217 | 4,047 | 4,514 | 0.443 | 0.416 | 0.429 | 0.421 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 57,944 | 3,073 | 992 | 100.0 | 6,977 | 90.2 | 2.7 | 671 | 408 | 7,060 | 0.622 | 0.087 | 0.152 | 0.105 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 55,630 | 3,179 | 992 | 100.0 | 6,865 | 88.8 | 3.2 | 744 | 528 | 6,987 | 0.585 | 0.096 | 0.165 | 0.116 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 17,316 | 131,449 | 883 | 89.0 | 2,135 | 27.6 | 36.8 | 2,663 | 9,833 | 5,068 | 0.213 | 0.344 | 0.263 | 0.307 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 17,681 | 24,786 | 819 | 82.6 | 1,642 | 21.2 | 15.2 | 1,727 | 3,880 | 6,004 | 0.308 | 0.223 | 0.259 | 0.236 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 57,771 | 703 | 992 | 100.0 | 7,213 | 93.3 | 2.3 | 440 | 115 | 7,291 | 0.793 | 0.057 | 0.106 | 0.070 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 47,004 | 7,610 | 966 | 97.4 | 5,809 | 75.1 | 5.8 | 819 | 1,387 | 6,912 | 0.371 | 0.106 | 0.165 | 0.124 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 13,551 | 26,371 | 660 | 66.5 | 1,327 | 17.2 | 16.4 | 2,014 | 3,517 | 5,717 | 0.364 | 0.261 | 0.304 | 0.276 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 11,640 | 34,318 | 613 | 61.8 | 1,146 | 14.8 | 18.4 | 2,040 | 4,218 | 5,691 | 0.326 | 0.264 | 0.292 | 0.274 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | opf | 992 | 12,904 | 16,056 | 667 | 67.2 | 1,417 | 18.3 | 14.4 | 1,472 | 3,336 | 6,259 | 0.306 | 0.190 | 0.235 | 0.206 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-all | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en | 781 | 6,713 | 6,005 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-en-de | 781 | 3,323 | 12,162 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong | 781 | 3,666 | 9,822 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | presidio-strong-high-recall | 781 | 3,646 | 10,017 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-core | 781 | 3,977 | 1,054 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-regex | 781 | 3,362 | 2,258 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-spacy | 781 | 2,725 | 20,978 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | datafog-gliner | 781 | 886 | 11,668 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-base | 781 | 6,122 | 519 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | scrubadub-spacy | 781 | 6,331 | 414 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner | 781 | 971 | 7,433 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | gliner-high-recall | 781 | 708 | 8,739 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | opf | 781 | 255 | 10,210 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 3,724 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 2,940 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 3,631 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 2,859 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 2,871 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 1,874 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 1,460 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 2,883 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 1,766 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 650 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 684 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 1,415 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | opf | 0 | 0 | 2,625 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 1,061 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 1,571 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 1,242 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 682 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 758 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 1,520 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 855 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 62 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 19 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 423 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | opf | 0 | 0 | 1,948 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-all | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-en | 53 | 1,638 | 121 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-en-de | 53 | 177 | 645 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-strong | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 53 | 264 | 587 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-core | 53 | 1,478 | 0 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-regex | 53 | 1,140 | 0 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-spacy | 53 | 165 | 356 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-gliner | 53 | 216 | 184 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v1 | Repeated PII values with decoys | product_coverage | scrubadub-base | 53 | 1,495 | 0 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v1 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 53 | 910 | 91 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v1 | Repeated PII values with decoys | product_coverage | gliner | 53 | 241 | 176 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v1 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 53 | 109 | 600 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v1 | Repeated PII values with decoys | product_coverage | opf | 53 | 72 | 432 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-all | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-en | 40 | 428 | 30 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-en-de | 40 | 0 | 296 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-strong | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 40 | 0 | 266 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-core | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-regex | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-spacy | 40 | 0 | 166 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-gliner | 40 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v1 | Repeated PII values with decoys | common_intersection | scrubadub-base | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 40 | 23 | 0 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v1 | Repeated PII values with decoys | common_intersection | gliner | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 40 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | opf | 40 | 0 | 329 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |

Threshold choice uses validation only: gliner → gliner-high-recall; presidio-strong → presidio-strong-high-recall.

## Tuned Presidio

Three more Presidio rows on the same documents, contracts and scorer as above. Every number in this section names its split; test-half numbers are the comparison.

- **Tuned by its authors:** Presidio Research's own tuned setup for its corpus (notebook 5, custom analyzer: OpenMed NER recognizer, title/year/age pattern recognizers, lemma context enhancement, 14 predefined recognizers removed, score threshold 0.3; `notebooks/5_Evaluate_Custom_Presidio_Analyzer.ipynb` at presidio-research `6db3769a`), applied unchanged. It supports English only, so every document is analyzed as English. Its entities without a label in this corpus: BIOMETRIC_IDENTIFIER, BLOOD_TYPE, COORDINATE, CVV, EDUCATION_LEVEL, EMPLOYMENT_STATUS, ETHNICITY, GENDER, ID, IP_ADDRESS, LANGUAGE, MAC_ADDRESS, MEDICAL_LICENSE, OCCUPATION, POLITICAL_VIEW, PROFESSIONAL_LICENSE, RELIGIOUS_BELIEF, SEXUALITY, SWIFT_CODE, USER_NAME, US_BANK_NUMBER, http_cookie. Scored labels here that none of its entities maps to: BSN, CPF, GIVENNAME, IDCARDNUM, IPADDRESS, NATIONALID, NHSNUMBER, ORGANIZATION, TELEPHONENUM, USERNAME, ZIPCODE. Labels affect only the typed metrics; leaked and false-positive bytes ignore them.
- **Tuned for this corpus:** a search over a space declared in code before any run ([`space.py`](../../../scripts/bench/compare/tuned_presidio/space.py)): the NLP-engine NER (spaCy large, `dslim/bert-base-NER` or none), extra NER recognizers Presidio ships (the multilingual Davlan model Gaze itself installs, the OpenMed PII model Presidio Research chose, GLiNER), every predefined recognizer, 21 custom pattern and deny-list recognizers for this corpus's classes, per recognizer and entity thresholds, the context enhancer and an allow list learned from validation false positives. The search scored only the validation half (3,093 documents) and evaluated 19,480 candidate configurations by coordinate descent. Two objectives: the comparison's own rule (fewest validation v3 leaked bytes, then fewest false-positive bytes) and the panels' headline (highest validation v3 character F2). The custom recognizers were written after reading validation-half gold examples; the search never received test-half text, gold or output. The committed choice was made by an earlier loader that built the whole corpus in memory and dropped the test half before the search; rerunning selection through per-half files, with the test-half file never opened, reproduces every choice and validation score. A guard test runs the current real loader with every test-half file unreadable. The NER models' training data is not fully published, so overlap with this synthetic corpus's style cannot be ruled out for them (nor for the NER model in Gaze's own setup). Both halves are synthetic and share their generators, and the split holds out document ids only, not templates or values: 922 of 1,119 (82.4 %) of the Synthetic identifiers in agentic formats test documents reuse a validation gold value (every layer is counted beside the test-half table below). Tuning on validation therefore learns those templates and values, and the test half measures fit to this corpus, not robustness to unseen phrasing or values. Gaze's rules were developed against the same corpus.
- **Budget:** Gaze's rules received 58 rulepack commits (2026-04-24 to 2026-09-30) at the original measurement lineage `ef369b33`, made with the whole corpus visible, test half included. This budget is pinned to that lineage, not the current branch's rulepack history. The tuned Presidio search is at least as generous in iterations: 19,480 measured candidate configurations against 58 rulepack commits, on top of a hand-written recognizer for the classes Gaze commits to. It saw the validation half only during scoring, so its test-half documents are held out, by id, while Gaze's are not.

Validation choice (v3, C/A/D/R summed):

| Objective | Start | Leaked B | FP B | Char F2 | Chosen |
| --- | --- | ---: | ---: | ---: | --- |
| f2 | everything | 1,865 | 18,763 | 0.942 |  |
| f2 | presidio-default | 1,173 | 19,761 | 0.945 | yes |
| leak-first | everything | 3 | 44,182 | 0.906 | yes |
| leak-first | presidio-default | 3 | 46,520 | 0.902 |  |

Chosen configurations:

- **f2:** NLP-engine NER dslim; extra NER davlan all, gliner off, openmed all; context default; 39 pattern recognizers on; 52 raised thresholds; allow list of 182 texts. Full configuration in [`presidio-tuned-selection.json`](presidio-tuned-selection.json).
- **leak-first:** NLP-engine NER none; extra NER davlan off, gliner all, openmed all; context default; 75 pattern recognizers on; 141 raised thresholds; allow list of 510 texts. Full configuration in [`presidio-tuned-selection.json`](presidio-tuned-selection.json).

**The split holds out document ids, not values.** The tuned-here rows were fitted on the validation half, and the test half repeats much of it. No test document is identical to a validation document, but most share a template, a generator group or exact gold values with one:

| Layer | Test docs | Share a template | Share a generator group | Reuse a gold value | Gold spans repeating a value |
| --- | ---: | ---: | ---: | ---: | ---: |
| Kiji EN/DE holdout and A4 negatives | 1,480 | n/a | n/a | 988 of 1,480 (66.8 %) | 5,181 of 7,731 (67.0 %) |
| Synthetic identifiers in agentic formats | 1,119 | 1,119 of 1,119 (100.0 %) | 1,119 of 1,119 (100.0 %) | 922 of 1,119 (82.4 %) | 1,166 of 1,372 (85.0 %) |
| Synthetic benign lookalikes | 415 | 415 of 415 (100.0 %) | 337 of 415 (81.2 %) | 0 of 415 (0.0 %) | no gold |
| Repeated PII values with decoys | 53 | 47 of 53 (88.7 %) | 0 of 53 (0.0 %) | 39 of 53 (73.6 %) | 108 of 239 (45.2 %) |

The tuned F2 choice's custom pattern recognizers alone, with every NER model off, cover 19,284 of 23,210 (83.1 %) of Synthetic identifiers in agentic formats test gold bytes; the full choice leaks 2 bytes there. These counts measure how far the test half depends on the validation half, not how much of any result is memorization. They weigh most on the generated layers; Kiji EN/DE holdout and A4 negatives has no templates of that kind, and its comparison with Gaze below stands as measured. Templates and groups exist only in the generated layers (n/a above). Computed by [`overlap.py`](../../../scripts/bench/compare/tuned_presidio/overlap.py) into [`presidio-tuned-overlap.json`](presidio-tuned-overlap.json).

Test half (product coverage):

| Contract | Layer | Configuration | Leaked B | FP B | Char F2 | Entity F2 |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 2,887 | 5,291 | 0.855 | 0.113 |
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 4 | 1,190 | 0.990 | 0.414 |
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 2 | 453 | 0.996 | 0.388 |
| v4 | Synthetic identifiers in agentic formats | presidio-all | 10,209 | 5,468 | 0.600 | 0.196 |
| v4 | Synthetic identifiers in agentic formats | presidio-strong | 10,654 | 4,864 | 0.585 | 0.205 |
| v4 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 10,510 | 4,864 | 0.591 | 0.204 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 3,443 | 41,396 | 0.854 | 0.015 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 2,127 | 45,546 | 0.859 | 0.086 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 4,254 | 20,727 | 0.894 | 0.388 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-all | 18,047 | 29,918 | 0.710 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 16,067 | 25,514 | 0.743 | 0.422 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 15,855 | 26,356 | 0.744 | 0.421 |
| v4 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 3,761 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 2,874 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 1,596 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-all | 0 | 3,724 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-strong | 0 | 2,859 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 2,871 | 0.000 | 0.000 |
| v4 | Repeated PII values with decoys | Presidio (tuned by its authors) | 115 | 497 | 0.934 | 0.215 |
| v4 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 15 | 438 | 0.966 | 0.203 |
| v4 | Repeated PII values with decoys | Presidio (tuned here, F2) | 82 | 108 | 0.969 | 0.322 |
| v4 | Repeated PII values with decoys | presidio-all | 177 | 645 | 0.907 | 0.258 |
| v4 | Repeated PII values with decoys | presidio-strong | 264 | 587 | 0.886 | 0.285 |
| v4 | Repeated PII values with decoys | presidio-strong-high-recall | 264 | 587 | 0.886 | 0.285 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 2,887 | 5,291 | 0.855 | 0.113 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 4 | 1,190 | 0.990 | 0.414 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 2 | 453 | 0.996 | 0.388 |
| v3 | Synthetic identifiers in agentic formats | presidio-all | 10,209 | 5,468 | 0.600 | 0.196 |
| v3 | Synthetic identifiers in agentic formats | presidio-strong | 10,654 | 4,864 | 0.585 | 0.205 |
| v3 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 10,510 | 4,864 | 0.591 | 0.204 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 2,426 | 41,396 | 0.859 | 0.016 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 190 | 45,546 | 0.874 | 0.088 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 1,222 | 20,727 | 0.925 | 0.397 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-all | 15,775 | 29,918 | 0.724 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 13,143 | 25,514 | 0.767 | 0.433 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 12,931 | 26,356 | 0.768 | 0.432 |
| v3 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 3,761 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 2,874 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 1,596 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-all | 0 | 3,724 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-strong | 0 | 2,859 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 2,871 | 0.000 | 0.000 |
| v3 | Repeated PII values with decoys | Presidio (tuned by its authors) | 115 | 497 | 0.934 | 0.215 |
| v3 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 15 | 438 | 0.966 | 0.203 |
| v3 | Repeated PII values with decoys | Presidio (tuned here, F2) | 82 | 108 | 0.969 | 0.322 |
| v3 | Repeated PII values with decoys | presidio-all | 177 | 645 | 0.907 | 0.258 |
| v3 | Repeated PII values with decoys | presidio-strong | 264 | 587 | 0.886 | 0.285 |
| v3 | Repeated PII values with decoys | presidio-strong-high-recall | 264 | 587 | 0.886 | 0.285 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 2,887 | 5,291 | 0.855 | 0.113 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 4 | 1,190 | 0.990 | 0.414 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 2 | 453 | 0.996 | 0.388 |
| v2 | Synthetic identifiers in agentic formats | presidio-all | 10,209 | 5,468 | 0.600 | 0.196 |
| v2 | Synthetic identifiers in agentic formats | presidio-strong | 10,654 | 4,864 | 0.585 | 0.205 |
| v2 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 10,510 | 4,864 | 0.591 | 0.204 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 2,426 | 41,396 | 0.859 | 0.016 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 190 | 45,546 | 0.874 | 0.088 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 1,222 | 20,727 | 0.925 | 0.397 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-all | 15,775 | 29,918 | 0.724 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 13,143 | 25,514 | 0.767 | 0.433 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 12,931 | 26,356 | 0.768 | 0.432 |
| v2 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 3,761 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 2,874 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 1,596 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-all | 0 | 3,724 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-strong | 0 | 2,859 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 2,871 | 0.000 | 0.000 |
| v2 | Repeated PII values with decoys | Presidio (tuned by its authors) | 115 | 497 | 0.934 | 0.215 |
| v2 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 15 | 438 | 0.966 | 0.203 |
| v2 | Repeated PII values with decoys | Presidio (tuned here, F2) | 82 | 108 | 0.969 | 0.322 |
| v2 | Repeated PII values with decoys | presidio-all | 177 | 645 | 0.907 | 0.258 |
| v2 | Repeated PII values with decoys | presidio-strong | 264 | 587 | 0.886 | 0.285 |
| v2 | Repeated PII values with decoys | presidio-strong-high-recall | 264 | 587 | 0.886 | 0.285 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 2,887 | 5,291 | 0.855 | 0.113 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 4 | 1,190 | 0.990 | 0.414 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 2 | 453 | 0.996 | 0.388 |
| v1 | Synthetic identifiers in agentic formats | presidio-all | 10,209 | 5,468 | 0.600 | 0.196 |
| v1 | Synthetic identifiers in agentic formats | presidio-strong | 10,654 | 4,864 | 0.585 | 0.205 |
| v1 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 10,510 | 4,864 | 0.591 | 0.204 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 3,443 | 41,396 | 0.854 | 0.015 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 2,127 | 45,546 | 0.859 | 0.086 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 4,254 | 20,727 | 0.894 | 0.388 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-all | 18,047 | 29,918 | 0.710 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 16,067 | 25,514 | 0.743 | 0.422 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 15,855 | 26,356 | 0.744 | 0.421 |
| v1 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 3,761 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 2,874 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 1,596 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-all | 0 | 3,724 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-strong | 0 | 2,859 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 2,871 | 0.000 | 0.000 |
| v1 | Repeated PII values with decoys | Presidio (tuned by its authors) | 115 | 497 | 0.934 | 0.215 |
| v1 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 15 | 438 | 0.966 | 0.203 |
| v1 | Repeated PII values with decoys | Presidio (tuned here, F2) | 82 | 108 | 0.969 | 0.322 |
| v1 | Repeated PII values with decoys | presidio-all | 177 | 645 | 0.907 | 0.258 |
| v1 | Repeated PII values with decoys | presidio-strong | 264 | 587 | 0.886 | 0.285 |
| v1 | Repeated PII values with decoys | presidio-strong-high-recall | 264 | 587 | 0.886 | 0.285 |

Against the latest Gaze release (v0.16.0) on all of Kiji EN/DE holdout and A4 negatives, the only layer that release was measured on in this corpus's form. The tuned-here rows include the validation half they were selected on, which can only flatter them. False positives are after v4's gold-gap credit.

| Contract | Configuration | Leaked B | FP B | Char F2 |
| --- | --- | ---: | ---: | ---: |
| v4 | Gaze v0.16.0 | 13,291 | 15,141 | 0.879 |
| v4 | Presidio (tuned by its authors) | 6,748 | 80,797 | 0.851 |
| v4 | Presidio (tuned here, leak-first) | 4,066 | 86,590 | 0.859 |
| v4 | Presidio (tuned here, F2) | 8,261 | 39,329 | 0.894 |
| v3 | Gaze v0.16.0 | 7,348 | 15,141 | n/a |
| v3 | Presidio (tuned by its authors) | 4,752 | 74,113 | n/a |
| v3 | Presidio (tuned here, leak-first) | 193 | 81,701 | n/a |
| v3 | Presidio (tuned here, F2) | 2,283 | 29,158 | n/a |
| v2 | Gaze v0.16.0 | 7,348 | 26,902 | n/a |
| v2 | Presidio (tuned by its authors) | 4,752 | 80,797 | n/a |
| v2 | Presidio (tuned here, leak-first) | 193 | 86,590 | n/a |
| v2 | Presidio (tuned here, F2) | 2,283 | 39,329 | n/a |
| v1 | Gaze v0.16.0 | 13,291 | 26,902 | n/a |
| v1 | Presidio (tuned by its authors) | 6,748 | 80,797 | n/a |
| v1 | Presidio (tuned here, leak-first) | 4,066 | 86,590 | n/a |
| v1 | Presidio (tuned here, F2) | 8,261 | 39,329 | n/a |

Where tuned Presidio beats Gaze v0.16.0 here: Presidio (tuned by its authors) leaks fewer v4 bytes; Presidio (tuned here, leak-first) leaks fewer v4 bytes; Presidio (tuned here, F2) leaks fewer v4 bytes; Presidio (tuned here, F2) has the higher v4 character F2; Presidio (tuned by its authors) leaks fewer v3 bytes; Presidio (tuned here, leak-first) leaks fewer v3 bytes; Presidio (tuned here, F2) leaks fewer v3 bytes; Presidio (tuned by its authors) leaks fewer v2 bytes; Presidio (tuned here, leak-first) leaks fewer v2 bytes; Presidio (tuned here, F2) leaks fewer v2 bytes; Presidio (tuned by its authors) leaks fewer v1 bytes; Presidio (tuned here, leak-first) leaks fewer v1 bytes; Presidio (tuned here, F2) leaks fewer v1 bytes.

Live check: each chosen configuration also ran live on a fixed sample (about one document in eight, every layer, both halves); Presidio (tuned here, F2): 762 documents, 0 differed on the first run, 0 on every rerun; Presidio (tuned here, leak-first): 762 documents, 1 differed on the first run, 1 on every rerun (1 with identical bytes and a different entity label, from Presidio's tie-break between equal-score results on one span).

The panels' Presidio bar is **Presidio (tuned here, F2)**, the highest test-half v3 layer C character F2 of the three: Presidio (tuned here, F2) 0.925, Presidio (tuned here, leak-first) 0.874, Presidio (tuned by its authors) 0.859. The default rows above stay unchanged.

Aggregate source: [`presidio-tuned.json`](presidio-tuned.json); reproduce with [`tuned_presidio/README.md`](../../../scripts/bench/compare/tuned_presidio/README.md).
