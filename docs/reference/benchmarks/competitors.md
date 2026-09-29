# Competitor comparison

Same corpus and scorer; tools run with documented configurations. UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's false-positive count after its audited gold-gap credit. Presidio all runs English, German, Dutch, French, and Portuguese spaCy models with the documented German recognizers. Presidio English default is a secondary row. Latency was not measured under a quiet machine; timing comparisons are withheld. This measures detection; competitor restore and manifest behavior is not scored.

Leaked and false-positive byte counts are class-agnostic. A skipped document's scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to get leakage on processed documents. For example, Presidio English-only v3 Synthetic identifiers in agentic formats leaks 34,733 B, including 28,151 B of scored gold from 1,320 skipped non-English documents. The reviewed label map controls v3's repeated-gold credit and the exact typed-span metrics below.

Gaze measured at `a2f6fefd33cebd040a4fc6e5a384f318807717af` (release `v0.15.1`). Home-normalized setup policy SHA-256: `481f5df7a9b0b562bf2c2db7274cfbf2ea701231c52f1287b27de00554453ff1`.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`. GLiNER uses model-card labels, library threshold 0.5. OpenAI Privacy Filter (OPF) 0.1.0 at source `f7f00ca7fb869683eb732c010299d901457f19c3`.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published.

| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | Latency |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| v3 | Synthetic identifiers in agentic formats | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
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
| v3 | Kiji EN/DE holdout and A4 negatives | gaze | 9,256 | 15,560 | 2,910 | 0 | 0 | not measured under a quiet machine |
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
| v3 | Synthetic benign lookalikes | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
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
| v3 | Repeated PII values with decoys | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
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
| v2 | Synthetic identifiers in agentic formats | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
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
| v2 | Kiji EN/DE holdout and A4 negatives | gaze | 9,256 | 27,282 | 2,910 | 0 | 0 | not measured under a quiet machine |
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
| v2 | Synthetic benign lookalikes | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
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
| v2 | Repeated PII values with decoys | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
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
| v1 | Synthetic identifiers in agentic formats | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
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
| v1 | Kiji EN/DE holdout and A4 negatives | gaze | 15,443 | 27,282 | 2,910 | 0 | 0 | not measured under a quiet machine |
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
| v1 | Synthetic benign lookalikes | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
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
| v1 | Repeated PII values with decoys | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
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
| v3 | Synthetic identifiers in agentic formats | product_coverage | gaze | 1,119 | 2,861 | 421 | 205 | 18.3 | 234 | 17.1 | 27.3 | 349 | 862 | 1,023 | 0.288 | 0.254 | 0.270 | 0.260 |
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
| v3 | Synthetic identifiers in agentic formats | common_intersection | gaze | 166 | 202 | 49 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
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
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | gaze | 992 | 4,806 | 13,997 | 524 | 52.8 | 693 | 9.2 | 15.0 | 4,985 | 2,847 | 2,522 | 0.636 | 0.664 | 0.650 | 0.658 |
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
| v3 | Kiji EN/DE holdout and A4 negatives | common_intersection | gaze | 781 | 1,260 | 6,138 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
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
| v3 | Synthetic benign lookalikes | product_coverage | gaze | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v3 | Synthetic benign lookalikes | common_intersection | gaze | 0 | 0 | 495 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v3 | Repeated PII values with decoys | product_coverage | gaze | 53 | 20 | 193 | 3 | 5.7 | 3 | 1.3 | 36.2 | 59 | 125 | 180 | 0.321 | 0.247 | 0.279 | 0.259 |
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
| v3 | Repeated PII values with decoys | common_intersection | gaze | 40 | 0 | 111 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
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
| v2 | Synthetic identifiers in agentic formats | product_coverage | gaze | 1,119 | 2,861 | 421 | 205 | 18.3 | 234 | 17.1 | 27.3 | 349 | 862 | 1,023 | 0.288 | 0.254 | 0.270 | 0.260 |
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
| v2 | Synthetic identifiers in agentic formats | common_intersection | gaze | 166 | 202 | 49 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
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
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | gaze | 992 | 4,806 | 13,997 | 524 | 52.8 | 693 | 9.2 | 15.0 | 4,985 | 2,847 | 2,522 | 0.636 | 0.664 | 0.650 | 0.658 |
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
| v2 | Kiji EN/DE holdout and A4 negatives | common_intersection | gaze | 781 | 1,260 | 6,138 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
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
| v2 | Synthetic benign lookalikes | product_coverage | gaze | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v2 | Synthetic benign lookalikes | common_intersection | gaze | 0 | 0 | 495 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v2 | Repeated PII values with decoys | product_coverage | gaze | 53 | 20 | 193 | 3 | 5.7 | 3 | 1.3 | 36.2 | 59 | 125 | 180 | 0.321 | 0.247 | 0.279 | 0.259 |
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
| v2 | Repeated PII values with decoys | common_intersection | gaze | 40 | 0 | 111 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
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
| v1 | Synthetic identifiers in agentic formats | product_coverage | gaze | 1,119 | 2,861 | 421 | 205 | 18.3 | 234 | 17.1 | 27.3 | 349 | 862 | 1,023 | 0.288 | 0.254 | 0.270 | 0.260 |
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
| v1 | Synthetic identifiers in agentic formats | common_intersection | gaze | 166 | 202 | 49 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
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
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | gaze | 992 | 7,937 | 13,997 | 633 | 63.8 | 907 | 11.7 | 15.0 | 4,985 | 2,871 | 2,746 | 0.635 | 0.645 | 0.640 | 0.643 |
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
| v1 | Kiji EN/DE holdout and A4 negatives | common_intersection | gaze | 781 | 1,260 | 6,138 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
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
| v1 | Synthetic benign lookalikes | product_coverage | gaze | 0 | 0 | 1,634 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v1 | Synthetic benign lookalikes | common_intersection | gaze | 0 | 0 | 495 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
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
| v1 | Repeated PII values with decoys | product_coverage | gaze | 53 | 20 | 193 | 3 | 5.7 | 3 | 1.3 | 36.2 | 59 | 125 | 180 | 0.321 | 0.247 | 0.279 | 0.259 |
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
| v1 | Repeated PII values with decoys | common_intersection | gaze | 40 | 0 | 111 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
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

## Where Gaze trails

On the v3 Kiji EN/DE holdout and A4 negatives common-intersection test half, Gaze leaks 1,260 B across 146/781 PII documents; opf leaks 255 B across 56/781. This is a measured Gaze loss.

Competitors have a lower leak, false-positive, or redaction-load value, or a better typed-entity value, in the following measured cells. Lower redaction load alone can reflect missed PII. Each pair reads competitor vs Gaze. Full aggregate rows span both halves and use v3's audited gold-gap FP credit; detailed rows use raw FP and also show validation and test separately. Counts below are metric cells; overlapping views and splits must not be summed as independent cases.

Loss counts by metric: Redaction load %: 468; FP: 417; Entity P: 219; F1: 159; Leaked B: 99; Leaking docs: 92; Doc leak %: 92; Leaking entities: 90; Entity leak %: 90; TP: 105; FN: 105; Entity R: 105; full aggregate FP B: 48; FP B: 303; F2: 135; full aggregate Leaked B: 3.

<details>
<summary>All 686 losing rows</summary>

- v3 Synthetic identifiers in agentic formats product_coverage full presidio-all: Redaction load % 23.8% vs 27.7%
- v3 Synthetic identifiers in agentic formats product_coverage validation presidio-all: Redaction load % 23.3% vs 28.1%
- v3 Synthetic identifiers in agentic formats product_coverage test presidio-all: Redaction load % 24.3% vs 27.3%
- v3 Synthetic identifiers in agentic formats product_coverage full presidio-en: Redaction load % 11.3% vs 27.7%; FP 1,143 vs 1,782
- v3 Synthetic identifiers in agentic formats product_coverage validation presidio-en: Redaction load % 10.6% vs 28.1%; FP 569 vs 920
- v3 Synthetic identifiers in agentic formats product_coverage test presidio-en: Redaction load % 12.1% vs 27.3%; FP 574 vs 862
- v3 Synthetic identifiers in agentic formats common_intersection full presidio-en: Redaction load % 4.9% vs 5.5%
- v3 Synthetic identifiers in agentic formats common_intersection validation presidio-en: Redaction load % 4.4% vs 5.5%
- v3 Synthetic identifiers in agentic formats common_intersection test presidio-en: Redaction load % 5.5% vs 5.6%
- v3 Synthetic identifiers in agentic formats product_coverage full presidio-en-de: Redaction load % 18.8% vs 27.7%
- v3 Synthetic identifiers in agentic formats product_coverage validation presidio-en-de: Redaction load % 18.3% vs 28.1%
- v3 Synthetic identifiers in agentic formats product_coverage test presidio-en-de: Redaction load % 19.3% vs 27.3%
- v3 Synthetic identifiers in agentic formats product_coverage full presidio-strong: Redaction load % 22.4% vs 27.7%
- v3 Synthetic identifiers in agentic formats product_coverage validation presidio-strong: Redaction load % 21.9% vs 28.1%
- v3 Synthetic identifiers in agentic formats product_coverage test presidio-strong: Redaction load % 22.9% vs 27.3%
- v3 Synthetic identifiers in agentic formats product_coverage full presidio-strong-high-recall: Redaction load % 22.6% vs 27.7%
- v3 Synthetic identifiers in agentic formats product_coverage validation presidio-strong-high-recall: Redaction load % 22.2% vs 28.1%
- v3 Synthetic identifiers in agentic formats product_coverage test presidio-strong-high-recall: Redaction load % 23.1% vs 27.3%
- v3 Synthetic identifiers in agentic formats product_coverage full datafog-core: Redaction load % 12.0% vs 27.7%; FP 1,222 vs 1,782
- v3 Synthetic identifiers in agentic formats product_coverage validation datafog-core: Redaction load % 11.8% vs 28.1%; FP 623 vs 920
- v3 Synthetic identifiers in agentic formats product_coverage test datafog-core: Redaction load % 12.3% vs 27.3%; FP 599 vs 862
- v3 Synthetic identifiers in agentic formats common_intersection full datafog-core: Redaction load % 5.3% vs 5.5%
- v3 Synthetic identifiers in agentic formats common_intersection validation datafog-core: Redaction load % 5.0% vs 5.5%
- v3 Synthetic identifiers in agentic formats product_coverage full datafog-regex: Redaction load % 17.0% vs 27.7%; FP 964 vs 1,782; Entity P 0.417 vs 0.298; F1 0.308 vs 0.282
- v3 Synthetic identifiers in agentic formats product_coverage validation datafog-regex: Redaction load % 17.3% vs 28.1%; FP 490 vs 920; Entity P 0.432 vs 0.307; F1 0.321 vs 0.292
- v3 Synthetic identifiers in agentic formats product_coverage test datafog-regex: Redaction load % 16.6% vs 27.3%; FP 474 vs 862; Entity P 0.402 vs 0.288; F1 0.294 vs 0.270
- v3 Synthetic identifiers in agentic formats common_intersection full datafog-regex: Leaked B 200 vs 452; Leaking docs 20 vs 30; Doc leak % 5.9% vs 8.8%; Leaking entities 20 vs 30; Entity leak % 5.4% vs 8.1%; TP 350 vs 335; FN 20 vs 35; Entity R 0.946 vs 0.905
- v3 Synthetic identifiers in agentic formats common_intersection validation datafog-regex: Leaked B 100 vs 250; Leaking docs 10 vs 17; Doc leak % 5.7% vs 9.8%; Leaking entities 10 vs 17; Entity leak % 5.2% vs 8.9%; TP 182 vs 172; FN 10 vs 20; Entity R 0.948 vs 0.896
- v3 Synthetic identifiers in agentic formats common_intersection test datafog-regex: Leaked B 100 vs 202; Leaking docs 10 vs 13; Doc leak % 6.0% vs 7.8%; Leaking entities 10 vs 13; Entity leak % 5.6% vs 7.3%; TP 168 vs 163; FN 10 vs 15; Entity R 0.944 vs 0.916
- v3 Synthetic identifiers in agentic formats product_coverage full datafog-spacy: Redaction load % 24.8% vs 27.7%
- v3 Synthetic identifiers in agentic formats product_coverage validation datafog-spacy: Redaction load % 24.8% vs 28.1%
- v3 Synthetic identifiers in agentic formats product_coverage test datafog-spacy: Redaction load % 24.9% vs 27.3%
- v3 Synthetic identifiers in agentic formats common_intersection full datafog-spacy: Leaked B 130 vs 452; Leaking docs 13 vs 30; Doc leak % 3.8% vs 8.8%; Leaking entities 13 vs 30; Entity leak % 3.5% vs 8.1%; TP 357 vs 335; FN 13 vs 35; Entity R 0.965 vs 0.905
- v3 Synthetic identifiers in agentic formats common_intersection validation datafog-spacy: Leaked B 90 vs 250; Leaking docs 9 vs 17; Doc leak % 5.2% vs 9.8%; Leaking entities 9 vs 17; Entity leak % 4.7% vs 8.9%; TP 183 vs 172; FN 9 vs 20; Entity R 0.953 vs 0.896
- v3 Synthetic identifiers in agentic formats common_intersection test datafog-spacy: Leaked B 40 vs 202; Leaking docs 4 vs 13; Doc leak % 2.4% vs 7.8%; Leaking entities 4 vs 13; Entity leak % 2.2% vs 7.3%; TP 174 vs 163; FN 4 vs 15; Entity R 0.978 vs 0.916
- v3 Synthetic identifiers in agentic formats product_coverage full datafog-gliner: Redaction load % 19.2% vs 27.7%; FP 1,260 vs 1,782; Entity P 0.360 vs 0.298; F1 0.296 vs 0.282
- v3 Synthetic identifiers in agentic formats product_coverage validation datafog-gliner: Redaction load % 19.8% vs 28.1%; FP 650 vs 920; Entity P 0.370 vs 0.307; F1 0.307 vs 0.292
- v3 Synthetic identifiers in agentic formats product_coverage test datafog-gliner: Redaction load % 18.6% vs 27.3%; FP 610 vs 862; Entity P 0.350 vs 0.288; F1 0.284 vs 0.270
- v3 Synthetic identifiers in agentic formats common_intersection full datafog-gliner: Leaked B 0 vs 452; Leaking docs 0 vs 30; Doc leak % 0.0% vs 8.8%; Leaking entities 0 vs 30; Entity leak % 0.0% vs 8.1%; TP 370 vs 335; FN 0 vs 35; Entity R 1.000 vs 0.905
- v3 Synthetic identifiers in agentic formats common_intersection validation datafog-gliner: Leaked B 0 vs 250; Leaking docs 0 vs 17; Doc leak % 0.0% vs 9.8%; Leaking entities 0 vs 17; Entity leak % 0.0% vs 8.9%; TP 192 vs 172; FN 0 vs 20; Entity R 1.000 vs 0.896
- v3 Synthetic identifiers in agentic formats common_intersection test datafog-gliner: Leaked B 0 vs 202; Leaking docs 0 vs 13; Doc leak % 0.0% vs 7.8%; Leaking entities 0 vs 13; Entity leak % 0.0% vs 7.3%; TP 178 vs 163; FN 0 vs 15; Entity R 1.000 vs 0.916
- v3 Synthetic identifiers in agentic formats full aggregate scrubadub-base: FP B 133 vs 882
- v3 Synthetic identifiers in agentic formats product_coverage full scrubadub-base: FP B 133 vs 882; Redaction load % 4.6% vs 27.7%; FP 328 vs 1,782
- v3 Synthetic identifiers in agentic formats product_coverage validation scrubadub-base: FP B 54 vs 461; Redaction load % 4.3% vs 28.1%; FP 161 vs 920
- v3 Synthetic identifiers in agentic formats product_coverage test scrubadub-base: FP B 79 vs 421; Redaction load % 4.8% vs 27.3%; FP 167 vs 862
- v3 Synthetic identifiers in agentic formats common_intersection full scrubadub-base: Redaction load % 2.3% vs 5.5%; FP 14 vs 33
- v3 Synthetic identifiers in agentic formats common_intersection validation scrubadub-base: FP B 54 vs 83; Redaction load % 2.2% vs 5.5%; FP 7 vs 22
- v3 Synthetic identifiers in agentic formats common_intersection test scrubadub-base: Redaction load % 2.5% vs 5.6%; FP 7 vs 11
- v3 Synthetic identifiers in agentic formats product_coverage full scrubadub-spacy: Redaction load % 6.4% vs 27.7%; FP 673 vs 1,782
- v3 Synthetic identifiers in agentic formats product_coverage validation scrubadub-spacy: Redaction load % 6.1% vs 28.1%; FP 335 vs 920
- v3 Synthetic identifiers in agentic formats product_coverage test scrubadub-spacy: Redaction load % 6.8% vs 27.3%; FP 338 vs 862
- v3 Synthetic identifiers in agentic formats common_intersection full scrubadub-spacy: Redaction load % 2.1% vs 5.5%; FP 14 vs 33
- v3 Synthetic identifiers in agentic formats common_intersection validation scrubadub-spacy: FP B 54 vs 83; Redaction load % 2.1% vs 5.5%; FP 7 vs 22
- v3 Synthetic identifiers in agentic formats common_intersection test scrubadub-spacy: Redaction load % 2.1% vs 5.6%; FP 7 vs 11
- v3 Synthetic identifiers in agentic formats product_coverage full gliner: Redaction load % 26.3% vs 27.7%; TP 1,075 vs 756; FP 1,034 vs 1,782; FN 1,755 vs 2,074; Entity P 0.510 vs 0.298; Entity R 0.380 vs 0.267; F1 0.435 vs 0.282; F2 0.400 vs 0.273
- v3 Synthetic identifiers in agentic formats product_coverage validation gliner: Redaction load % 26.3% vs 28.1%; TP 568 vs 407; FP 513 vs 920; FN 890 vs 1,051; Entity P 0.525 vs 0.307; Entity R 0.390 vs 0.279; F1 0.447 vs 0.292; F2 0.411 vs 0.284
- v3 Synthetic identifiers in agentic formats product_coverage test gliner: Redaction load % 26.3% vs 27.3%; TP 507 vs 349; FP 521 vs 862; FN 865 vs 1,023; Entity P 0.493 vs 0.288; Entity R 0.370 vs 0.254; F1 0.422 vs 0.270; F2 0.389 vs 0.260
- v3 Synthetic identifiers in agentic formats common_intersection full gliner: Leaked B 266 vs 452; Leaking docs 14 vs 30; Doc leak % 4.1% vs 8.8%; Leaking entities 14 vs 30; Entity leak % 3.8% vs 8.1%
- v3 Synthetic identifiers in agentic formats common_intersection validation gliner: Leaked B 95 vs 250; Leaking docs 5 vs 17; Doc leak % 2.9% vs 9.8%; Leaking entities 5 vs 17; Entity leak % 2.6% vs 8.9%
- v3 Synthetic identifiers in agentic formats common_intersection test gliner: Leaked B 171 vs 202; Leaking docs 9 vs 13; Doc leak % 5.4% vs 7.8%; Leaking entities 9 vs 13; Entity leak % 5.1% vs 7.3%
- v3 Synthetic identifiers in agentic formats product_coverage full gliner-high-recall: TP 1,099 vs 756; FN 1,731 vs 2,074; Entity P 0.374 vs 0.298; Entity R 0.388 vs 0.267; F1 0.381 vs 0.282; F2 0.385 vs 0.273
- v3 Synthetic identifiers in agentic formats product_coverage validation gliner-high-recall: TP 578 vs 407; FP 916 vs 920; FN 880 vs 1,051; Entity P 0.387 vs 0.307; Entity R 0.396 vs 0.279; F1 0.392 vs 0.292; F2 0.394 vs 0.284
- v3 Synthetic identifiers in agentic formats product_coverage test gliner-high-recall: TP 521 vs 349; FN 851 vs 1,023; Entity P 0.362 vs 0.288; Entity R 0.380 vs 0.254; F1 0.370 vs 0.270; F2 0.376 vs 0.260
- v3 Synthetic identifiers in agentic formats common_intersection full gliner-high-recall: Leaked B 152 vs 452; Leaking docs 8 vs 30; Doc leak % 2.4% vs 8.8%; Leaking entities 8 vs 30; Entity leak % 2.2% vs 8.1%
- v3 Synthetic identifiers in agentic formats common_intersection validation gliner-high-recall: Leaked B 57 vs 250; Leaking docs 3 vs 17; Doc leak % 1.7% vs 9.8%; Leaking entities 3 vs 17; Entity leak % 1.6% vs 8.9%
- v3 Synthetic identifiers in agentic formats common_intersection test gliner-high-recall: Leaked B 95 vs 202; Leaking docs 5 vs 13; Doc leak % 3.0% vs 7.8%; Leaking entities 5 vs 13; Entity leak % 2.8% vs 7.3%
- v3 Synthetic identifiers in agentic formats product_coverage full opf: Redaction load % 27.6% vs 27.7%; TP 913 vs 756; FP 1,281 vs 1,782; FN 1,917 vs 2,074; Entity P 0.416 vs 0.298; Entity R 0.323 vs 0.267; F1 0.363 vs 0.282; F2 0.338 vs 0.273
- v3 Synthetic identifiers in agentic formats product_coverage validation opf: TP 476 vs 407; FP 676 vs 920; FN 982 vs 1,051; Entity P 0.413 vs 0.307; Entity R 0.326 vs 0.279; F1 0.365 vs 0.292; F2 0.341 vs 0.284
- v3 Synthetic identifiers in agentic formats product_coverage test opf: Redaction load % 26.9% vs 27.3%; TP 437 vs 349; FP 605 vs 862; FN 935 vs 1,023; Entity P 0.419 vs 0.288; Entity R 0.319 vs 0.254; F1 0.362 vs 0.270; F2 0.335 vs 0.260
- v3 Synthetic identifiers in agentic formats common_intersection full opf: Leaked B 401 vs 452; Leaking docs 25 vs 30; Doc leak % 7.4% vs 8.8%; Leaking entities 25 vs 30; Entity leak % 6.8% vs 8.1%
- v3 Synthetic identifiers in agentic formats common_intersection validation opf: Leaked B 204 vs 250; Leaking docs 11 vs 17; Doc leak % 6.3% vs 9.8%; Leaking entities 11 vs 17; Entity leak % 5.7% vs 8.9%; TP 173 vs 172; FN 19 vs 20; Entity R 0.901 vs 0.896
- v3 Synthetic identifiers in agentic formats common_intersection test opf: Leaked B 197 vs 202
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full presidio-en: Redaction load % 8.7% vs 14.9%; FP 4,735 vs 5,445
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation presidio-en: Redaction load % 8.7% vs 14.7%; FP 2,352 vs 2,598
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test presidio-en: Redaction load % 8.7% vs 15.0%; FP 2,383 vs 2,847
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full presidio-en: Redaction load % 2.7% vs 3.9%; FP 1,440 vs 1,781
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation presidio-en: Redaction load % 2.8% vs 3.9%; FP 724 vs 870
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test presidio-en: FP B 6,005 vs 6,138; Redaction load % 2.7% vs 4.0%; FP 716 vs 911
- v3 Kiji EN/DE holdout and A4 negatives full aggregate datafog-core: FP B 5,839 vs 15,560
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-core: FP B 5,992 vs 27,282; Redaction load % 2.7% vs 14.9%; FP 829 vs 5,445
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-core: FP B 2,919 vs 13,285; Redaction load % 2.7% vs 14.7%; FP 421 vs 2,598
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-core: FP B 3,073 vs 13,997; Redaction load % 2.7% vs 15.0%; FP 408 vs 2,847
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-core: FP B 2,244 vs 11,929; Redaction load % 2.2% vs 3.9%; FP 353 vs 1,781; Entity P 0.767 vs 0.512
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-core: FP B 1,190 vs 5,791; Redaction load % 2.2% vs 3.9%; FP 190 vs 870; Entity P 0.746 vs 0.503
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-core: FP B 1,054 vs 6,138; Redaction load % 2.2% vs 4.0%; FP 163 vs 911; Entity P 0.787 vs 0.521
- v3 Kiji EN/DE holdout and A4 negatives full aggregate datafog-regex: FP B 6,766 vs 15,560
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-regex: FP B 6,919 vs 27,282; Redaction load % 3.3% vs 14.9%; FP 1,105 vs 5,445
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-regex: FP B 3,740 vs 13,285; Redaction load % 3.4% vs 14.7%; FP 577 vs 2,598
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-regex: FP B 3,179 vs 13,997; Redaction load % 3.2% vs 15.0%; FP 528 vs 2,847
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-regex: FP B 5,200 vs 11,929; Redaction load % 2.7% vs 3.9%; FP 553 vs 1,781; Entity P 0.687 vs 0.512
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-regex: FP B 2,942 vs 5,791; Redaction load % 2.8% vs 3.9%; FP 307 vs 870; Entity P 0.655 vs 0.503
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-regex: FP B 2,258 vs 6,138; Redaction load % 2.6% vs 4.0%; FP 246 vs 911; Entity P 0.719 vs 0.521
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-gliner: Leaked B 1,719 vs 2,569; Leaking docs 265 vs 294; Doc leak % 17.8% vs 19.7%; Leaking entities 280 vs 315; Entity leak % 12.6% vs 14.1%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-gliner: Leaked B 833 vs 1,309; Leaking docs 127 vs 148; Doc leak % 17.9% vs 20.9%; Leaking entities 133 vs 162; Entity leak % 12.5% vs 15.2%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-gliner: Leaked B 886 vs 1,260; Leaking docs 138 vs 146; Doc leak % 17.7% vs 18.7%; Leaking entities 147 vs 153; Entity leak % 12.6% vs 13.1%
- v3 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-base: FP B 1,449 vs 15,560
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-base: FP B 1,525 vs 27,282; Redaction load % 2.3% vs 14.9%; FP 214 vs 5,445; Entity P 0.801 vs 0.636
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-base: FP B 822 vs 13,285; Redaction load % 2.3% vs 14.7%; FP 105 vs 2,598; Entity P 0.800 vs 0.636
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-base: FP B 703 vs 13,997; Redaction load % 2.3% vs 15.0%; FP 109 vs 2,847; Entity P 0.801 vs 0.636
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-base: FP B 1,074 vs 11,929; Redaction load % 1.6% vs 3.9%; FP 108 vs 1,781; Entity P 0.850 vs 0.512
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-base: FP B 555 vs 5,791; Redaction load % 1.6% vs 3.9%; FP 55 vs 870; Entity P 0.841 vs 0.503
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-base: FP B 519 vs 6,138; Redaction load % 1.6% vs 4.0%; FP 53 vs 911; Entity P 0.859 vs 0.521
- v3 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-spacy: FP B 11,942 vs 15,560
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-spacy: FP B 15,310 vs 27,282; Redaction load % 5.7% vs 14.9%; FP 2,601 vs 5,445
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-spacy: FP B 7,700 vs 13,285; Redaction load % 5.8% vs 14.7%; FP 1,270 vs 2,598
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-spacy: FP B 7,610 vs 13,997; Redaction load % 5.7% vs 15.0%; FP 1,331 vs 2,847
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-spacy: FP B 884 vs 11,929; Redaction load % 1.5% vs 3.9%; FP 89 vs 1,781; Entity P 0.870 vs 0.512
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-spacy: FP B 470 vs 5,791; Redaction load % 1.5% vs 3.9%; FP 46 vs 870; Entity P 0.858 vs 0.503
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-spacy: FP B 414 vs 6,138; Redaction load % 1.5% vs 4.0%; FP 43 vs 911; Entity P 0.880 vs 0.521
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full gliner: Leaked B 1,934 vs 2,569; Leaking docs 179 vs 294; Doc leak % 12.0% vs 19.7%; Leaking entities 204 vs 315; Entity leak % 9.1% vs 14.1%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner: Leaked B 963 vs 1,309; Leaking docs 91 vs 148; Doc leak % 12.9% vs 20.9%; Leaking entities 100 vs 162; Entity leak % 9.4% vs 15.2%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test gliner: Leaked B 971 vs 1,260; Leaking docs 88 vs 146; Doc leak % 11.3% vs 18.7%; Leaking entities 104 vs 153; Entity leak % 8.9% vs 13.1%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full gliner-high-recall: Leaked B 1,497 vs 2,569; Leaking docs 136 vs 294; Doc leak % 9.1% vs 19.7%; Leaking entities 158 vs 315; Entity leak % 7.1% vs 14.1%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner-high-recall: Leaked B 789 vs 1,309; Leaking docs 71 vs 148; Doc leak % 10.0% vs 20.9%; Leaking entities 80 vs 162; Entity leak % 7.5% vs 15.2%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test gliner-high-recall: Leaked B 708 vs 1,260; Leaking docs 65 vs 146; Doc leak % 8.3% vs 18.7%; Leaking entities 78 vs 153; Entity leak % 6.7% vs 13.1%
- v3 Kiji EN/DE holdout and A4 negatives product_coverage full opf: Redaction load % 13.7% vs 14.9%
- v3 Kiji EN/DE holdout and A4 negatives product_coverage validation opf: Redaction load % 13.5% vs 14.7%
- v3 Kiji EN/DE holdout and A4 negatives product_coverage test opf: Redaction load % 13.9% vs 15.0%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection full opf: Leaked B 675 vs 2,569; Leaking docs 125 vs 294; Doc leak % 8.4% vs 19.7%; Leaking entities 131 vs 315; Entity leak % 5.9% vs 14.1%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection validation opf: Leaked B 420 vs 1,309; Leaking docs 69 vs 148; Doc leak % 9.7% vs 20.9%; Leaking entities 75 vs 162; Entity leak % 7.0% vs 15.2%
- v3 Kiji EN/DE holdout and A4 negatives common_intersection test opf: Leaked B 255 vs 1,260; Leaking docs 56 vs 146; Doc leak % 7.2% vs 18.7%; Leaking entities 56 vs 153; Entity leak % 4.8% vs 13.1%
- v3 Synthetic benign lookalikes full aggregate datafog-regex: FP B 3,051 vs 3,544
- v3 Synthetic benign lookalikes product_coverage full datafog-regex: FP B 3,051 vs 3,544; Redaction load % 6.4% vs 7.4%; FP 324 vs 394
- v3 Synthetic benign lookalikes product_coverage validation datafog-regex: FP B 1,591 vs 1,910; Redaction load % 6.5% vs 7.9%; FP 168 vs 216
- v3 Synthetic benign lookalikes product_coverage test datafog-regex: FP B 1,460 vs 1,634; Redaction load % 6.2% vs 6.9%; FP 156 vs 178
- v3 Synthetic benign lookalikes product_coverage full datafog-gliner: FP 375 vs 394
- v3 Synthetic benign lookalikes product_coverage validation datafog-gliner: FP B 1,887 vs 1,910; Redaction load % 7.8% vs 7.9%; FP 195 vs 216
- v3 Synthetic benign lookalikes full aggregate scrubadub-base: FP B 156 vs 3,544
- v3 Synthetic benign lookalikes product_coverage full scrubadub-base: FP B 156 vs 3,544; Redaction load % 0.3% vs 7.4%; FP 15 vs 394
- v3 Synthetic benign lookalikes product_coverage validation scrubadub-base: FP B 94 vs 1,910; Redaction load % 0.4% vs 7.9%; FP 9 vs 216
- v3 Synthetic benign lookalikes product_coverage test scrubadub-base: FP B 62 vs 1,634; Redaction load % 0.3% vs 6.9%; FP 6 vs 178
- v3 Synthetic benign lookalikes common_intersection full scrubadub-base: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v3 Synthetic benign lookalikes common_intersection validation scrubadub-base: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v3 Synthetic benign lookalikes common_intersection test scrubadub-base: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v3 Synthetic benign lookalikes full aggregate scrubadub-spacy: FP B 1,297 vs 3,544
- v3 Synthetic benign lookalikes product_coverage full scrubadub-spacy: FP B 1,297 vs 3,544; Redaction load % 2.7% vs 7.4%; FP 104 vs 394
- v3 Synthetic benign lookalikes product_coverage validation scrubadub-spacy: FP B 647 vs 1,910; Redaction load % 2.7% vs 7.9%; FP 50 vs 216
- v3 Synthetic benign lookalikes product_coverage test scrubadub-spacy: FP B 650 vs 1,634; Redaction load % 2.7% vs 6.9%; FP 54 vs 178
- v3 Synthetic benign lookalikes common_intersection full scrubadub-spacy: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v3 Synthetic benign lookalikes common_intersection validation scrubadub-spacy: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v3 Synthetic benign lookalikes common_intersection test scrubadub-spacy: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v3 Synthetic benign lookalikes full aggregate gliner: FP B 1,684 vs 3,544
- v3 Synthetic benign lookalikes product_coverage full gliner: FP B 1,684 vs 3,544; Redaction load % 3.5% vs 7.4%; FP 129 vs 394
- v3 Synthetic benign lookalikes product_coverage validation gliner: FP B 1,000 vs 1,910; Redaction load % 4.1% vs 7.9%; FP 75 vs 216
- v3 Synthetic benign lookalikes product_coverage test gliner: FP B 684 vs 1,634; Redaction load % 2.9% vs 6.9%; FP 54 vs 178
- v3 Synthetic benign lookalikes common_intersection full gliner: FP B 115 vs 1,167; Redaction load % 0.2% vs 2.4%; FP 7 vs 152
- v3 Synthetic benign lookalikes common_intersection validation gliner: FP B 96 vs 672; Redaction load % 0.4% vs 2.8%; FP 6 vs 89
- v3 Synthetic benign lookalikes common_intersection test gliner: FP B 19 vs 495; Redaction load % 0.1% vs 2.1%; FP 1 vs 63
- v3 Synthetic benign lookalikes full aggregate gliner-high-recall: FP B 3,264 vs 3,544
- v3 Synthetic benign lookalikes product_coverage full gliner-high-recall: FP B 3,264 vs 3,544; Redaction load % 6.8% vs 7.4%; FP 271 vs 394
- v3 Synthetic benign lookalikes product_coverage validation gliner-high-recall: FP B 1,849 vs 1,910; Redaction load % 7.6% vs 7.9%; FP 151 vs 216
- v3 Synthetic benign lookalikes product_coverage test gliner-high-recall: FP B 1,415 vs 1,634; Redaction load % 6.0% vs 6.9%; FP 120 vs 178
- v3 Synthetic benign lookalikes common_intersection full gliner-high-recall: FP B 899 vs 1,167; Redaction load % 1.9% vs 2.4%; FP 69 vs 152
- v3 Synthetic benign lookalikes common_intersection validation gliner-high-recall: FP B 476 vs 672; Redaction load % 2.0% vs 2.8%; FP 36 vs 89
- v3 Synthetic benign lookalikes common_intersection test gliner-high-recall: FP B 423 vs 495; Redaction load % 1.8% vs 2.1%; FP 33 vs 63
- v3 Synthetic benign lookalikes product_coverage full opf: FP 369 vs 394
- v3 Synthetic benign lookalikes product_coverage validation opf: FP 193 vs 216
- v3 Synthetic benign lookalikes product_coverage test opf: FP 176 vs 178
- v3 Repeated PII values with decoys product_coverage full presidio-all: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation presidio-all: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v3 Repeated PII values with decoys product_coverage test presidio-all: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v3 Repeated PII values with decoys full aggregate presidio-en: FP B 222 vs 419
- v3 Repeated PII values with decoys product_coverage full presidio-en: FP B 222 vs 419; Redaction load % 15.1% vs 33.5%; FP 122 vs 224
- v3 Repeated PII values with decoys product_coverage validation presidio-en: FP B 101 vs 226; Redaction load % 14.5% vs 30.5%; FP 55 vs 99
- v3 Repeated PII values with decoys product_coverage test presidio-en: FP B 121 vs 193; Redaction load % 15.6% vs 36.2%; FP 67 vs 125
- v3 Repeated PII values with decoys common_intersection full presidio-en: FP B 67 vs 274; Redaction load % 8.9% vs 15.8%; FP 13 vs 22; Entity P 0.772 vs 0.761
- v3 Repeated PII values with decoys common_intersection validation presidio-en: FP B 37 vs 163; Redaction load % 8.5% vs 14.4%; FP 7 vs 13; Entity P 0.741 vs 0.698
- v3 Repeated PII values with decoys common_intersection test presidio-en: FP B 30 vs 111; Redaction load % 9.2% vs 17.2%; FP 6 vs 9
- v3 Repeated PII values with decoys product_coverage full presidio-en-de: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation presidio-en-de: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v3 Repeated PII values with decoys product_coverage test presidio-en-de: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v3 Repeated PII values with decoys product_coverage full presidio-strong: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation presidio-strong: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v3 Repeated PII values with decoys product_coverage test presidio-strong: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v3 Repeated PII values with decoys product_coverage full presidio-strong-high-recall: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation presidio-strong-high-recall: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v3 Repeated PII values with decoys product_coverage test presidio-strong-high-recall: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v3 Repeated PII values with decoys full aggregate datafog-core: FP B 0 vs 419
- v3 Repeated PII values with decoys product_coverage full datafog-core: FP B 0 vs 419; Redaction load % 13.9% vs 33.5%; FP 38 vs 224; Entity P 0.648 vs 0.335
- v3 Repeated PII values with decoys product_coverage validation datafog-core: FP B 0 vs 226; Redaction load % 11.6% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v3 Repeated PII values with decoys product_coverage test datafog-core: FP B 0 vs 193; Redaction load % 16.1% vs 36.2%; FP 25 vs 125; Entity P 0.615 vs 0.321
- v3 Repeated PII values with decoys common_intersection full datafog-core: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation datafog-core: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v3 Repeated PII values with decoys common_intersection test datafog-core: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v3 Repeated PII values with decoys full aggregate datafog-regex: FP B 0 vs 419
- v3 Repeated PII values with decoys product_coverage full datafog-regex: FP B 0 vs 419; Redaction load % 18.2% vs 33.5%; FP 34 vs 224; Entity P 0.746 vs 0.335; F1 0.348 vs 0.291
- v3 Repeated PII values with decoys product_coverage validation datafog-regex: FP B 0 vs 226; Redaction load % 16.2% vs 30.5%; FP 13 vs 99; Entity P 0.776 vs 0.353; F1 0.347 vs 0.305
- v3 Repeated PII values with decoys product_coverage test datafog-regex: FP B 0 vs 193; Redaction load % 20.2% vs 36.2%; FP 21 vs 125; Entity P 0.724 vs 0.321; F1 0.349 vs 0.279; F2 0.266 vs 0.259
- v3 Repeated PII values with decoys common_intersection full datafog-regex: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation datafog-regex: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v3 Repeated PII values with decoys common_intersection test datafog-regex: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v3 Repeated PII values with decoys product_coverage full datafog-spacy: Redaction load % 32.9% vs 33.5%
- v3 Repeated PII values with decoys product_coverage validation datafog-spacy: Redaction load % 29.1% vs 30.5%
- v3 Repeated PII values with decoys full aggregate datafog-gliner: FP B 310 vs 419
- v3 Repeated PII values with decoys product_coverage full datafog-gliner: FP B 310 vs 419; Redaction load % 30.1% vs 33.5%; FP 168 vs 224; Entity P 0.373 vs 0.335
- v3 Repeated PII values with decoys product_coverage validation datafog-gliner: FP B 126 vs 226; Redaction load % 26.2% vs 30.5%; FP 67 vs 99; Entity P 0.402 vs 0.353
- v3 Repeated PII values with decoys product_coverage test datafog-gliner: FP B 184 vs 193; Redaction load % 33.7% vs 36.2%; FP 101 vs 125; Entity P 0.353 vs 0.321
- v3 Repeated PII values with decoys common_intersection full datafog-gliner: FP B 100 vs 274; Redaction load % 14.4% vs 15.8%; FP 10 vs 22; Entity P 0.875 vs 0.761; F1 0.933 vs 0.864; F2 0.972 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation datafog-gliner: FP B 40 vs 163; Redaction load % 12.4% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v3 Repeated PII values with decoys common_intersection test datafog-gliner: FP B 60 vs 111; Redaction load % 16.4% vs 17.2%; FP 6 vs 9; Entity P 0.870 vs 0.816; F1 0.930 vs 0.899; F2 0.971 vs 0.957
- v3 Repeated PII values with decoys full aggregate scrubadub-base: FP B 38 vs 419
- v3 Repeated PII values with decoys product_coverage full scrubadub-base: FP B 38 vs 419; Redaction load % 13.7% vs 33.5%; FP 34 vs 224; Entity P 0.673 vs 0.335
- v3 Repeated PII values with decoys product_coverage validation scrubadub-base: FP B 38 vs 226; Redaction load % 11.5% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v3 Repeated PII values with decoys product_coverage test scrubadub-base: FP B 0 vs 193; Redaction load % 15.9% vs 36.2%; FP 21 vs 125; Entity P 0.656 vs 0.321
- v3 Repeated PII values with decoys common_intersection full scrubadub-base: FP B 38 vs 274; Redaction load % 13.9% vs 15.8%; FP 4 vs 22; Entity P 0.946 vs 0.761; F1 0.972 vs 0.864; F2 0.989 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation scrubadub-base: FP B 38 vs 163; Redaction load % 12.3% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v3 Repeated PII values with decoys common_intersection test scrubadub-base: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v3 Repeated PII values with decoys full aggregate scrubadub-spacy: FP B 193 vs 419
- v3 Repeated PII values with decoys product_coverage full scrubadub-spacy: FP B 193 vs 419; Redaction load % 21.6% vs 33.5%; FP 140 vs 224; Entity P 0.355 vs 0.335
- v3 Repeated PII values with decoys product_coverage validation scrubadub-spacy: FP B 102 vs 226; Redaction load % 18.9% vs 30.5%; FP 60 vs 99; Entity P 0.362 vs 0.353
- v3 Repeated PII values with decoys product_coverage test scrubadub-spacy: FP B 91 vs 193; Redaction load % 24.1% vs 36.2%; FP 80 vs 125; Entity P 0.350 vs 0.321
- v3 Repeated PII values with decoys common_intersection full scrubadub-spacy: FP B 38 vs 274; Redaction load % 13.6% vs 15.8%; FP 4 vs 22; Entity P 0.944 vs 0.761; F1 0.958 vs 0.864; F2 0.966 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation scrubadub-spacy: FP B 38 vs 163; Redaction load % 12.0% vs 14.4%; FP 4 vs 13; Entity P 0.879 vs 0.698; F1 0.921 vs 0.822; F2 0.948 vs 0.920
- v3 Repeated PII values with decoys common_intersection test scrubadub-spacy: FP B 0 vs 111; Redaction load % 15.1% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 0.987 vs 0.899; F2 0.980 vs 0.957
- v3 Repeated PII values with decoys product_coverage full gliner: Redaction load % 32.0% vs 33.5%; TP 120 vs 113; FP 178 vs 224; FN 320 vs 327; Entity P 0.403 vs 0.335; Entity R 0.273 vs 0.257; F1 0.325 vs 0.291; F2 0.292 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation gliner: TP 67 vs 54; FP 75 vs 99; FN 134 vs 147; Entity P 0.472 vs 0.353; Entity R 0.333 vs 0.269; F1 0.391 vs 0.305; F2 0.354 vs 0.282
- v3 Repeated PII values with decoys product_coverage test gliner: FP B 176 vs 193; Redaction load % 33.3% vs 36.2%; FP 103 vs 125; Entity P 0.340 vs 0.321
- v3 Repeated PII values with decoys common_intersection full gliner: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation gliner: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v3 Repeated PII values with decoys common_intersection test gliner: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v3 Repeated PII values with decoys product_coverage full gliner-high-recall: TP 124 vs 113; FN 316 vs 327; Entity P 0.344 vs 0.335; Entity R 0.282 vs 0.257; F1 0.310 vs 0.291; F2 0.292 vs 0.269
- v3 Repeated PII values with decoys product_coverage validation gliner-high-recall: Leaked B 135 vs 214; TP 68 vs 54; FP 98 vs 99; FN 133 vs 147; Entity P 0.410 vs 0.353; Entity R 0.338 vs 0.269; F1 0.371 vs 0.305; F2 0.351 vs 0.282
- v3 Repeated PII values with decoys common_intersection full gliner-high-recall: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v3 Repeated PII values with decoys common_intersection validation gliner-high-recall: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v3 Repeated PII values with decoys common_intersection test gliner-high-recall: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v3 Repeated PII values with decoys full aggregate opf: Leaked B 182 vs 234
- v3 Repeated PII values with decoys product_coverage full opf: Leaked B 182 vs 234
- v3 Repeated PII values with decoys product_coverage validation opf: Leaked B 110 vs 214; Leaking docs 7 vs 9; Doc leak % 14.9% vs 19.1%; Leaking entities 13 vs 17; Entity leak % 6.5% vs 8.5%
- v2 Synthetic identifiers in agentic formats product_coverage full presidio-all: Redaction load % 23.8% vs 27.7%
- v2 Synthetic identifiers in agentic formats product_coverage validation presidio-all: Redaction load % 23.3% vs 28.1%
- v2 Synthetic identifiers in agentic formats product_coverage test presidio-all: Redaction load % 24.3% vs 27.3%
- v2 Synthetic identifiers in agentic formats product_coverage full presidio-en: Redaction load % 11.3% vs 27.7%; FP 1,143 vs 1,782
- v2 Synthetic identifiers in agentic formats product_coverage validation presidio-en: Redaction load % 10.6% vs 28.1%; FP 569 vs 920
- v2 Synthetic identifiers in agentic formats product_coverage test presidio-en: Redaction load % 12.1% vs 27.3%; FP 574 vs 862
- v2 Synthetic identifiers in agentic formats common_intersection full presidio-en: Redaction load % 4.9% vs 5.5%
- v2 Synthetic identifiers in agentic formats common_intersection validation presidio-en: Redaction load % 4.4% vs 5.5%
- v2 Synthetic identifiers in agentic formats common_intersection test presidio-en: Redaction load % 5.5% vs 5.6%
- v2 Synthetic identifiers in agentic formats product_coverage full presidio-en-de: Redaction load % 18.8% vs 27.7%
- v2 Synthetic identifiers in agentic formats product_coverage validation presidio-en-de: Redaction load % 18.3% vs 28.1%
- v2 Synthetic identifiers in agentic formats product_coverage test presidio-en-de: Redaction load % 19.3% vs 27.3%
- v2 Synthetic identifiers in agentic formats product_coverage full presidio-strong: Redaction load % 22.4% vs 27.7%
- v2 Synthetic identifiers in agentic formats product_coverage validation presidio-strong: Redaction load % 21.9% vs 28.1%
- v2 Synthetic identifiers in agentic formats product_coverage test presidio-strong: Redaction load % 22.9% vs 27.3%
- v2 Synthetic identifiers in agentic formats product_coverage full presidio-strong-high-recall: Redaction load % 22.6% vs 27.7%
- v2 Synthetic identifiers in agentic formats product_coverage validation presidio-strong-high-recall: Redaction load % 22.2% vs 28.1%
- v2 Synthetic identifiers in agentic formats product_coverage test presidio-strong-high-recall: Redaction load % 23.1% vs 27.3%
- v2 Synthetic identifiers in agentic formats product_coverage full datafog-core: Redaction load % 12.0% vs 27.7%; FP 1,222 vs 1,782
- v2 Synthetic identifiers in agentic formats product_coverage validation datafog-core: Redaction load % 11.8% vs 28.1%; FP 623 vs 920
- v2 Synthetic identifiers in agentic formats product_coverage test datafog-core: Redaction load % 12.3% vs 27.3%; FP 599 vs 862
- v2 Synthetic identifiers in agentic formats common_intersection full datafog-core: Redaction load % 5.3% vs 5.5%
- v2 Synthetic identifiers in agentic formats common_intersection validation datafog-core: Redaction load % 5.0% vs 5.5%
- v2 Synthetic identifiers in agentic formats product_coverage full datafog-regex: Redaction load % 17.0% vs 27.7%; FP 964 vs 1,782; Entity P 0.417 vs 0.298; F1 0.308 vs 0.282
- v2 Synthetic identifiers in agentic formats product_coverage validation datafog-regex: Redaction load % 17.3% vs 28.1%; FP 490 vs 920; Entity P 0.432 vs 0.307; F1 0.321 vs 0.292
- v2 Synthetic identifiers in agentic formats product_coverage test datafog-regex: Redaction load % 16.6% vs 27.3%; FP 474 vs 862; Entity P 0.402 vs 0.288; F1 0.294 vs 0.270
- v2 Synthetic identifiers in agentic formats common_intersection full datafog-regex: Leaked B 200 vs 452; Leaking docs 20 vs 30; Doc leak % 5.9% vs 8.8%; Leaking entities 20 vs 30; Entity leak % 5.4% vs 8.1%; TP 350 vs 335; FN 20 vs 35; Entity R 0.946 vs 0.905
- v2 Synthetic identifiers in agentic formats common_intersection validation datafog-regex: Leaked B 100 vs 250; Leaking docs 10 vs 17; Doc leak % 5.7% vs 9.8%; Leaking entities 10 vs 17; Entity leak % 5.2% vs 8.9%; TP 182 vs 172; FN 10 vs 20; Entity R 0.948 vs 0.896
- v2 Synthetic identifiers in agentic formats common_intersection test datafog-regex: Leaked B 100 vs 202; Leaking docs 10 vs 13; Doc leak % 6.0% vs 7.8%; Leaking entities 10 vs 13; Entity leak % 5.6% vs 7.3%; TP 168 vs 163; FN 10 vs 15; Entity R 0.944 vs 0.916
- v2 Synthetic identifiers in agentic formats product_coverage full datafog-spacy: Redaction load % 24.8% vs 27.7%
- v2 Synthetic identifiers in agentic formats product_coverage validation datafog-spacy: Redaction load % 24.8% vs 28.1%
- v2 Synthetic identifiers in agentic formats product_coverage test datafog-spacy: Redaction load % 24.9% vs 27.3%
- v2 Synthetic identifiers in agentic formats common_intersection full datafog-spacy: Leaked B 130 vs 452; Leaking docs 13 vs 30; Doc leak % 3.8% vs 8.8%; Leaking entities 13 vs 30; Entity leak % 3.5% vs 8.1%; TP 357 vs 335; FN 13 vs 35; Entity R 0.965 vs 0.905
- v2 Synthetic identifiers in agentic formats common_intersection validation datafog-spacy: Leaked B 90 vs 250; Leaking docs 9 vs 17; Doc leak % 5.2% vs 9.8%; Leaking entities 9 vs 17; Entity leak % 4.7% vs 8.9%; TP 183 vs 172; FN 9 vs 20; Entity R 0.953 vs 0.896
- v2 Synthetic identifiers in agentic formats common_intersection test datafog-spacy: Leaked B 40 vs 202; Leaking docs 4 vs 13; Doc leak % 2.4% vs 7.8%; Leaking entities 4 vs 13; Entity leak % 2.2% vs 7.3%; TP 174 vs 163; FN 4 vs 15; Entity R 0.978 vs 0.916
- v2 Synthetic identifiers in agentic formats product_coverage full datafog-gliner: Redaction load % 19.2% vs 27.7%; FP 1,260 vs 1,782; Entity P 0.360 vs 0.298; F1 0.296 vs 0.282
- v2 Synthetic identifiers in agentic formats product_coverage validation datafog-gliner: Redaction load % 19.8% vs 28.1%; FP 650 vs 920; Entity P 0.370 vs 0.307; F1 0.307 vs 0.292
- v2 Synthetic identifiers in agentic formats product_coverage test datafog-gliner: Redaction load % 18.6% vs 27.3%; FP 610 vs 862; Entity P 0.350 vs 0.288; F1 0.284 vs 0.270
- v2 Synthetic identifiers in agentic formats common_intersection full datafog-gliner: Leaked B 0 vs 452; Leaking docs 0 vs 30; Doc leak % 0.0% vs 8.8%; Leaking entities 0 vs 30; Entity leak % 0.0% vs 8.1%; TP 370 vs 335; FN 0 vs 35; Entity R 1.000 vs 0.905
- v2 Synthetic identifiers in agentic formats common_intersection validation datafog-gliner: Leaked B 0 vs 250; Leaking docs 0 vs 17; Doc leak % 0.0% vs 9.8%; Leaking entities 0 vs 17; Entity leak % 0.0% vs 8.9%; TP 192 vs 172; FN 0 vs 20; Entity R 1.000 vs 0.896
- v2 Synthetic identifiers in agentic formats common_intersection test datafog-gliner: Leaked B 0 vs 202; Leaking docs 0 vs 13; Doc leak % 0.0% vs 7.8%; Leaking entities 0 vs 13; Entity leak % 0.0% vs 7.3%; TP 178 vs 163; FN 0 vs 15; Entity R 1.000 vs 0.916
- v2 Synthetic identifiers in agentic formats full aggregate scrubadub-base: FP B 133 vs 882
- v2 Synthetic identifiers in agentic formats product_coverage full scrubadub-base: FP B 133 vs 882; Redaction load % 4.6% vs 27.7%; FP 328 vs 1,782
- v2 Synthetic identifiers in agentic formats product_coverage validation scrubadub-base: FP B 54 vs 461; Redaction load % 4.3% vs 28.1%; FP 161 vs 920
- v2 Synthetic identifiers in agentic formats product_coverage test scrubadub-base: FP B 79 vs 421; Redaction load % 4.8% vs 27.3%; FP 167 vs 862
- v2 Synthetic identifiers in agentic formats common_intersection full scrubadub-base: Redaction load % 2.3% vs 5.5%; FP 14 vs 33
- v2 Synthetic identifiers in agentic formats common_intersection validation scrubadub-base: FP B 54 vs 83; Redaction load % 2.2% vs 5.5%; FP 7 vs 22
- v2 Synthetic identifiers in agentic formats common_intersection test scrubadub-base: Redaction load % 2.5% vs 5.6%; FP 7 vs 11
- v2 Synthetic identifiers in agentic formats product_coverage full scrubadub-spacy: Redaction load % 6.4% vs 27.7%; FP 673 vs 1,782
- v2 Synthetic identifiers in agentic formats product_coverage validation scrubadub-spacy: Redaction load % 6.1% vs 28.1%; FP 335 vs 920
- v2 Synthetic identifiers in agentic formats product_coverage test scrubadub-spacy: Redaction load % 6.8% vs 27.3%; FP 338 vs 862
- v2 Synthetic identifiers in agentic formats common_intersection full scrubadub-spacy: Redaction load % 2.1% vs 5.5%; FP 14 vs 33
- v2 Synthetic identifiers in agentic formats common_intersection validation scrubadub-spacy: FP B 54 vs 83; Redaction load % 2.1% vs 5.5%; FP 7 vs 22
- v2 Synthetic identifiers in agentic formats common_intersection test scrubadub-spacy: Redaction load % 2.1% vs 5.6%; FP 7 vs 11
- v2 Synthetic identifiers in agentic formats product_coverage full gliner: Redaction load % 26.3% vs 27.7%; TP 1,075 vs 756; FP 1,034 vs 1,782; FN 1,755 vs 2,074; Entity P 0.510 vs 0.298; Entity R 0.380 vs 0.267; F1 0.435 vs 0.282; F2 0.400 vs 0.273
- v2 Synthetic identifiers in agentic formats product_coverage validation gliner: Redaction load % 26.3% vs 28.1%; TP 568 vs 407; FP 513 vs 920; FN 890 vs 1,051; Entity P 0.525 vs 0.307; Entity R 0.390 vs 0.279; F1 0.447 vs 0.292; F2 0.411 vs 0.284
- v2 Synthetic identifiers in agentic formats product_coverage test gliner: Redaction load % 26.3% vs 27.3%; TP 507 vs 349; FP 521 vs 862; FN 865 vs 1,023; Entity P 0.493 vs 0.288; Entity R 0.370 vs 0.254; F1 0.422 vs 0.270; F2 0.389 vs 0.260
- v2 Synthetic identifiers in agentic formats common_intersection full gliner: Leaked B 266 vs 452; Leaking docs 14 vs 30; Doc leak % 4.1% vs 8.8%; Leaking entities 14 vs 30; Entity leak % 3.8% vs 8.1%
- v2 Synthetic identifiers in agentic formats common_intersection validation gliner: Leaked B 95 vs 250; Leaking docs 5 vs 17; Doc leak % 2.9% vs 9.8%; Leaking entities 5 vs 17; Entity leak % 2.6% vs 8.9%
- v2 Synthetic identifiers in agentic formats common_intersection test gliner: Leaked B 171 vs 202; Leaking docs 9 vs 13; Doc leak % 5.4% vs 7.8%; Leaking entities 9 vs 13; Entity leak % 5.1% vs 7.3%
- v2 Synthetic identifiers in agentic formats product_coverage full gliner-high-recall: TP 1,099 vs 756; FN 1,731 vs 2,074; Entity P 0.374 vs 0.298; Entity R 0.388 vs 0.267; F1 0.381 vs 0.282; F2 0.385 vs 0.273
- v2 Synthetic identifiers in agentic formats product_coverage validation gliner-high-recall: TP 578 vs 407; FP 916 vs 920; FN 880 vs 1,051; Entity P 0.387 vs 0.307; Entity R 0.396 vs 0.279; F1 0.392 vs 0.292; F2 0.394 vs 0.284
- v2 Synthetic identifiers in agentic formats product_coverage test gliner-high-recall: TP 521 vs 349; FN 851 vs 1,023; Entity P 0.362 vs 0.288; Entity R 0.380 vs 0.254; F1 0.370 vs 0.270; F2 0.376 vs 0.260
- v2 Synthetic identifiers in agentic formats common_intersection full gliner-high-recall: Leaked B 152 vs 452; Leaking docs 8 vs 30; Doc leak % 2.4% vs 8.8%; Leaking entities 8 vs 30; Entity leak % 2.2% vs 8.1%
- v2 Synthetic identifiers in agentic formats common_intersection validation gliner-high-recall: Leaked B 57 vs 250; Leaking docs 3 vs 17; Doc leak % 1.7% vs 9.8%; Leaking entities 3 vs 17; Entity leak % 1.6% vs 8.9%
- v2 Synthetic identifiers in agentic formats common_intersection test gliner-high-recall: Leaked B 95 vs 202; Leaking docs 5 vs 13; Doc leak % 3.0% vs 7.8%; Leaking entities 5 vs 13; Entity leak % 2.8% vs 7.3%
- v2 Synthetic identifiers in agentic formats product_coverage full opf: Redaction load % 27.6% vs 27.7%; TP 913 vs 756; FP 1,281 vs 1,782; FN 1,917 vs 2,074; Entity P 0.416 vs 0.298; Entity R 0.323 vs 0.267; F1 0.363 vs 0.282; F2 0.338 vs 0.273
- v2 Synthetic identifiers in agentic formats product_coverage validation opf: TP 476 vs 407; FP 676 vs 920; FN 982 vs 1,051; Entity P 0.413 vs 0.307; Entity R 0.326 vs 0.279; F1 0.365 vs 0.292; F2 0.341 vs 0.284
- v2 Synthetic identifiers in agentic formats product_coverage test opf: Redaction load % 26.9% vs 27.3%; TP 437 vs 349; FP 605 vs 862; FN 935 vs 1,023; Entity P 0.419 vs 0.288; Entity R 0.319 vs 0.254; F1 0.362 vs 0.270; F2 0.335 vs 0.260
- v2 Synthetic identifiers in agentic formats common_intersection full opf: Leaked B 401 vs 452; Leaking docs 25 vs 30; Doc leak % 7.4% vs 8.8%; Leaking entities 25 vs 30; Entity leak % 6.8% vs 8.1%
- v2 Synthetic identifiers in agentic formats common_intersection validation opf: Leaked B 204 vs 250; Leaking docs 11 vs 17; Doc leak % 6.3% vs 9.8%; Leaking entities 11 vs 17; Entity leak % 5.7% vs 8.9%; TP 173 vs 172; FN 19 vs 20; Entity R 0.901 vs 0.896
- v2 Synthetic identifiers in agentic formats common_intersection test opf: Leaked B 197 vs 202
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full presidio-en: Redaction load % 8.7% vs 14.9%; FP 4,735 vs 5,445
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation presidio-en: Redaction load % 8.7% vs 14.7%; FP 2,352 vs 2,598
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test presidio-en: Redaction load % 8.7% vs 15.0%; FP 2,383 vs 2,847
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full presidio-en: Redaction load % 2.7% vs 3.9%; FP 1,440 vs 1,781
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation presidio-en: Redaction load % 2.8% vs 3.9%; FP 724 vs 870
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test presidio-en: FP B 6,005 vs 6,138; Redaction load % 2.7% vs 4.0%; FP 716 vs 911
- v2 Kiji EN/DE holdout and A4 negatives full aggregate datafog-core: FP B 5,992 vs 27,282
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-core: FP B 5,992 vs 27,282; Redaction load % 2.7% vs 14.9%; FP 829 vs 5,445
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-core: FP B 2,919 vs 13,285; Redaction load % 2.7% vs 14.7%; FP 421 vs 2,598
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-core: FP B 3,073 vs 13,997; Redaction load % 2.7% vs 15.0%; FP 408 vs 2,847
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-core: FP B 2,244 vs 11,929; Redaction load % 2.2% vs 3.9%; FP 353 vs 1,781; Entity P 0.767 vs 0.512
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-core: FP B 1,190 vs 5,791; Redaction load % 2.2% vs 3.9%; FP 190 vs 870; Entity P 0.746 vs 0.503
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-core: FP B 1,054 vs 6,138; Redaction load % 2.2% vs 4.0%; FP 163 vs 911; Entity P 0.787 vs 0.521
- v2 Kiji EN/DE holdout and A4 negatives full aggregate datafog-regex: FP B 6,919 vs 27,282
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-regex: FP B 6,919 vs 27,282; Redaction load % 3.3% vs 14.9%; FP 1,105 vs 5,445
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-regex: FP B 3,740 vs 13,285; Redaction load % 3.4% vs 14.7%; FP 577 vs 2,598
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-regex: FP B 3,179 vs 13,997; Redaction load % 3.2% vs 15.0%; FP 528 vs 2,847
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-regex: FP B 5,200 vs 11,929; Redaction load % 2.7% vs 3.9%; FP 553 vs 1,781; Entity P 0.687 vs 0.512
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-regex: FP B 2,942 vs 5,791; Redaction load % 2.8% vs 3.9%; FP 307 vs 870; Entity P 0.655 vs 0.503
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-regex: FP B 2,258 vs 6,138; Redaction load % 2.6% vs 4.0%; FP 246 vs 911; Entity P 0.719 vs 0.521
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-gliner: Leaked B 1,719 vs 2,569; Leaking docs 265 vs 294; Doc leak % 17.8% vs 19.7%; Leaking entities 280 vs 315; Entity leak % 12.6% vs 14.1%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-gliner: Leaked B 833 vs 1,309; Leaking docs 127 vs 148; Doc leak % 17.9% vs 20.9%; Leaking entities 133 vs 162; Entity leak % 12.5% vs 15.2%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-gliner: Leaked B 886 vs 1,260; Leaking docs 138 vs 146; Doc leak % 17.7% vs 18.7%; Leaking entities 147 vs 153; Entity leak % 12.6% vs 13.1%
- v2 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-base: FP B 1,525 vs 27,282
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-base: FP B 1,525 vs 27,282; Redaction load % 2.3% vs 14.9%; FP 214 vs 5,445; Entity P 0.801 vs 0.636
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-base: FP B 822 vs 13,285; Redaction load % 2.3% vs 14.7%; FP 105 vs 2,598; Entity P 0.800 vs 0.636
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-base: FP B 703 vs 13,997; Redaction load % 2.3% vs 15.0%; FP 109 vs 2,847; Entity P 0.801 vs 0.636
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-base: FP B 1,074 vs 11,929; Redaction load % 1.6% vs 3.9%; FP 108 vs 1,781; Entity P 0.850 vs 0.512
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-base: FP B 555 vs 5,791; Redaction load % 1.6% vs 3.9%; FP 55 vs 870; Entity P 0.841 vs 0.503
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-base: FP B 519 vs 6,138; Redaction load % 1.6% vs 4.0%; FP 53 vs 911; Entity P 0.859 vs 0.521
- v2 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-spacy: FP B 15,310 vs 27,282
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-spacy: FP B 15,310 vs 27,282; Redaction load % 5.7% vs 14.9%; FP 2,601 vs 5,445
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-spacy: FP B 7,700 vs 13,285; Redaction load % 5.8% vs 14.7%; FP 1,270 vs 2,598
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-spacy: FP B 7,610 vs 13,997; Redaction load % 5.7% vs 15.0%; FP 1,331 vs 2,847
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-spacy: FP B 884 vs 11,929; Redaction load % 1.5% vs 3.9%; FP 89 vs 1,781; Entity P 0.870 vs 0.512
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-spacy: FP B 470 vs 5,791; Redaction load % 1.5% vs 3.9%; FP 46 vs 870; Entity P 0.858 vs 0.503
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-spacy: FP B 414 vs 6,138; Redaction load % 1.5% vs 4.0%; FP 43 vs 911; Entity P 0.880 vs 0.521
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full gliner: Leaked B 1,934 vs 2,569; Leaking docs 179 vs 294; Doc leak % 12.0% vs 19.7%; Leaking entities 204 vs 315; Entity leak % 9.1% vs 14.1%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner: Leaked B 963 vs 1,309; Leaking docs 91 vs 148; Doc leak % 12.9% vs 20.9%; Leaking entities 100 vs 162; Entity leak % 9.4% vs 15.2%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test gliner: Leaked B 971 vs 1,260; Leaking docs 88 vs 146; Doc leak % 11.3% vs 18.7%; Leaking entities 104 vs 153; Entity leak % 8.9% vs 13.1%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full gliner-high-recall: Leaked B 1,497 vs 2,569; Leaking docs 136 vs 294; Doc leak % 9.1% vs 19.7%; Leaking entities 158 vs 315; Entity leak % 7.1% vs 14.1%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner-high-recall: Leaked B 789 vs 1,309; Leaking docs 71 vs 148; Doc leak % 10.0% vs 20.9%; Leaking entities 80 vs 162; Entity leak % 7.5% vs 15.2%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test gliner-high-recall: Leaked B 708 vs 1,260; Leaking docs 65 vs 146; Doc leak % 8.3% vs 18.7%; Leaking entities 78 vs 153; Entity leak % 6.7% vs 13.1%
- v2 Kiji EN/DE holdout and A4 negatives product_coverage full opf: Redaction load % 13.7% vs 14.9%
- v2 Kiji EN/DE holdout and A4 negatives product_coverage validation opf: Redaction load % 13.5% vs 14.7%
- v2 Kiji EN/DE holdout and A4 negatives product_coverage test opf: Redaction load % 13.9% vs 15.0%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection full opf: Leaked B 675 vs 2,569; Leaking docs 125 vs 294; Doc leak % 8.4% vs 19.7%; Leaking entities 131 vs 315; Entity leak % 5.9% vs 14.1%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection validation opf: Leaked B 420 vs 1,309; Leaking docs 69 vs 148; Doc leak % 9.7% vs 20.9%; Leaking entities 75 vs 162; Entity leak % 7.0% vs 15.2%
- v2 Kiji EN/DE holdout and A4 negatives common_intersection test opf: Leaked B 255 vs 1,260; Leaking docs 56 vs 146; Doc leak % 7.2% vs 18.7%; Leaking entities 56 vs 153; Entity leak % 4.8% vs 13.1%
- v2 Synthetic benign lookalikes full aggregate datafog-regex: FP B 3,051 vs 3,544
- v2 Synthetic benign lookalikes product_coverage full datafog-regex: FP B 3,051 vs 3,544; Redaction load % 6.4% vs 7.4%; FP 324 vs 394
- v2 Synthetic benign lookalikes product_coverage validation datafog-regex: FP B 1,591 vs 1,910; Redaction load % 6.5% vs 7.9%; FP 168 vs 216
- v2 Synthetic benign lookalikes product_coverage test datafog-regex: FP B 1,460 vs 1,634; Redaction load % 6.2% vs 6.9%; FP 156 vs 178
- v2 Synthetic benign lookalikes product_coverage full datafog-gliner: FP 375 vs 394
- v2 Synthetic benign lookalikes product_coverage validation datafog-gliner: FP B 1,887 vs 1,910; Redaction load % 7.8% vs 7.9%; FP 195 vs 216
- v2 Synthetic benign lookalikes full aggregate scrubadub-base: FP B 156 vs 3,544
- v2 Synthetic benign lookalikes product_coverage full scrubadub-base: FP B 156 vs 3,544; Redaction load % 0.3% vs 7.4%; FP 15 vs 394
- v2 Synthetic benign lookalikes product_coverage validation scrubadub-base: FP B 94 vs 1,910; Redaction load % 0.4% vs 7.9%; FP 9 vs 216
- v2 Synthetic benign lookalikes product_coverage test scrubadub-base: FP B 62 vs 1,634; Redaction load % 0.3% vs 6.9%; FP 6 vs 178
- v2 Synthetic benign lookalikes common_intersection full scrubadub-base: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v2 Synthetic benign lookalikes common_intersection validation scrubadub-base: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v2 Synthetic benign lookalikes common_intersection test scrubadub-base: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v2 Synthetic benign lookalikes full aggregate scrubadub-spacy: FP B 1,297 vs 3,544
- v2 Synthetic benign lookalikes product_coverage full scrubadub-spacy: FP B 1,297 vs 3,544; Redaction load % 2.7% vs 7.4%; FP 104 vs 394
- v2 Synthetic benign lookalikes product_coverage validation scrubadub-spacy: FP B 647 vs 1,910; Redaction load % 2.7% vs 7.9%; FP 50 vs 216
- v2 Synthetic benign lookalikes product_coverage test scrubadub-spacy: FP B 650 vs 1,634; Redaction load % 2.7% vs 6.9%; FP 54 vs 178
- v2 Synthetic benign lookalikes common_intersection full scrubadub-spacy: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v2 Synthetic benign lookalikes common_intersection validation scrubadub-spacy: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v2 Synthetic benign lookalikes common_intersection test scrubadub-spacy: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v2 Synthetic benign lookalikes full aggregate gliner: FP B 1,684 vs 3,544
- v2 Synthetic benign lookalikes product_coverage full gliner: FP B 1,684 vs 3,544; Redaction load % 3.5% vs 7.4%; FP 129 vs 394
- v2 Synthetic benign lookalikes product_coverage validation gliner: FP B 1,000 vs 1,910; Redaction load % 4.1% vs 7.9%; FP 75 vs 216
- v2 Synthetic benign lookalikes product_coverage test gliner: FP B 684 vs 1,634; Redaction load % 2.9% vs 6.9%; FP 54 vs 178
- v2 Synthetic benign lookalikes common_intersection full gliner: FP B 115 vs 1,167; Redaction load % 0.2% vs 2.4%; FP 7 vs 152
- v2 Synthetic benign lookalikes common_intersection validation gliner: FP B 96 vs 672; Redaction load % 0.4% vs 2.8%; FP 6 vs 89
- v2 Synthetic benign lookalikes common_intersection test gliner: FP B 19 vs 495; Redaction load % 0.1% vs 2.1%; FP 1 vs 63
- v2 Synthetic benign lookalikes full aggregate gliner-high-recall: FP B 3,264 vs 3,544
- v2 Synthetic benign lookalikes product_coverage full gliner-high-recall: FP B 3,264 vs 3,544; Redaction load % 6.8% vs 7.4%; FP 271 vs 394
- v2 Synthetic benign lookalikes product_coverage validation gliner-high-recall: FP B 1,849 vs 1,910; Redaction load % 7.6% vs 7.9%; FP 151 vs 216
- v2 Synthetic benign lookalikes product_coverage test gliner-high-recall: FP B 1,415 vs 1,634; Redaction load % 6.0% vs 6.9%; FP 120 vs 178
- v2 Synthetic benign lookalikes common_intersection full gliner-high-recall: FP B 899 vs 1,167; Redaction load % 1.9% vs 2.4%; FP 69 vs 152
- v2 Synthetic benign lookalikes common_intersection validation gliner-high-recall: FP B 476 vs 672; Redaction load % 2.0% vs 2.8%; FP 36 vs 89
- v2 Synthetic benign lookalikes common_intersection test gliner-high-recall: FP B 423 vs 495; Redaction load % 1.8% vs 2.1%; FP 33 vs 63
- v2 Synthetic benign lookalikes product_coverage full opf: FP 369 vs 394
- v2 Synthetic benign lookalikes product_coverage validation opf: FP 193 vs 216
- v2 Synthetic benign lookalikes product_coverage test opf: FP 176 vs 178
- v2 Repeated PII values with decoys product_coverage full presidio-all: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation presidio-all: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v2 Repeated PII values with decoys product_coverage test presidio-all: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v2 Repeated PII values with decoys full aggregate presidio-en: FP B 222 vs 419
- v2 Repeated PII values with decoys product_coverage full presidio-en: FP B 222 vs 419; Redaction load % 15.1% vs 33.5%; FP 122 vs 224
- v2 Repeated PII values with decoys product_coverage validation presidio-en: FP B 101 vs 226; Redaction load % 14.5% vs 30.5%; FP 55 vs 99
- v2 Repeated PII values with decoys product_coverage test presidio-en: FP B 121 vs 193; Redaction load % 15.6% vs 36.2%; FP 67 vs 125
- v2 Repeated PII values with decoys common_intersection full presidio-en: FP B 67 vs 274; Redaction load % 8.9% vs 15.8%; FP 13 vs 22; Entity P 0.772 vs 0.761
- v2 Repeated PII values with decoys common_intersection validation presidio-en: FP B 37 vs 163; Redaction load % 8.5% vs 14.4%; FP 7 vs 13; Entity P 0.741 vs 0.698
- v2 Repeated PII values with decoys common_intersection test presidio-en: FP B 30 vs 111; Redaction load % 9.2% vs 17.2%; FP 6 vs 9
- v2 Repeated PII values with decoys product_coverage full presidio-en-de: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation presidio-en-de: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v2 Repeated PII values with decoys product_coverage test presidio-en-de: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v2 Repeated PII values with decoys product_coverage full presidio-strong: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation presidio-strong: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v2 Repeated PII values with decoys product_coverage test presidio-strong: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v2 Repeated PII values with decoys product_coverage full presidio-strong-high-recall: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation presidio-strong-high-recall: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v2 Repeated PII values with decoys product_coverage test presidio-strong-high-recall: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v2 Repeated PII values with decoys full aggregate datafog-core: FP B 0 vs 419
- v2 Repeated PII values with decoys product_coverage full datafog-core: FP B 0 vs 419; Redaction load % 13.9% vs 33.5%; FP 38 vs 224; Entity P 0.648 vs 0.335
- v2 Repeated PII values with decoys product_coverage validation datafog-core: FP B 0 vs 226; Redaction load % 11.6% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v2 Repeated PII values with decoys product_coverage test datafog-core: FP B 0 vs 193; Redaction load % 16.1% vs 36.2%; FP 25 vs 125; Entity P 0.615 vs 0.321
- v2 Repeated PII values with decoys common_intersection full datafog-core: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation datafog-core: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v2 Repeated PII values with decoys common_intersection test datafog-core: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v2 Repeated PII values with decoys full aggregate datafog-regex: FP B 0 vs 419
- v2 Repeated PII values with decoys product_coverage full datafog-regex: FP B 0 vs 419; Redaction load % 18.2% vs 33.5%; FP 34 vs 224; Entity P 0.746 vs 0.335; F1 0.348 vs 0.291
- v2 Repeated PII values with decoys product_coverage validation datafog-regex: FP B 0 vs 226; Redaction load % 16.2% vs 30.5%; FP 13 vs 99; Entity P 0.776 vs 0.353; F1 0.347 vs 0.305
- v2 Repeated PII values with decoys product_coverage test datafog-regex: FP B 0 vs 193; Redaction load % 20.2% vs 36.2%; FP 21 vs 125; Entity P 0.724 vs 0.321; F1 0.349 vs 0.279; F2 0.266 vs 0.259
- v2 Repeated PII values with decoys common_intersection full datafog-regex: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation datafog-regex: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v2 Repeated PII values with decoys common_intersection test datafog-regex: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v2 Repeated PII values with decoys product_coverage full datafog-spacy: Redaction load % 32.9% vs 33.5%
- v2 Repeated PII values with decoys product_coverage validation datafog-spacy: Redaction load % 29.1% vs 30.5%
- v2 Repeated PII values with decoys full aggregate datafog-gliner: FP B 310 vs 419
- v2 Repeated PII values with decoys product_coverage full datafog-gliner: FP B 310 vs 419; Redaction load % 30.1% vs 33.5%; FP 168 vs 224; Entity P 0.373 vs 0.335
- v2 Repeated PII values with decoys product_coverage validation datafog-gliner: FP B 126 vs 226; Redaction load % 26.2% vs 30.5%; FP 67 vs 99; Entity P 0.402 vs 0.353
- v2 Repeated PII values with decoys product_coverage test datafog-gliner: FP B 184 vs 193; Redaction load % 33.7% vs 36.2%; FP 101 vs 125; Entity P 0.353 vs 0.321
- v2 Repeated PII values with decoys common_intersection full datafog-gliner: FP B 100 vs 274; Redaction load % 14.4% vs 15.8%; FP 10 vs 22; Entity P 0.875 vs 0.761; F1 0.933 vs 0.864; F2 0.972 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation datafog-gliner: FP B 40 vs 163; Redaction load % 12.4% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v2 Repeated PII values with decoys common_intersection test datafog-gliner: FP B 60 vs 111; Redaction load % 16.4% vs 17.2%; FP 6 vs 9; Entity P 0.870 vs 0.816; F1 0.930 vs 0.899; F2 0.971 vs 0.957
- v2 Repeated PII values with decoys full aggregate scrubadub-base: FP B 38 vs 419
- v2 Repeated PII values with decoys product_coverage full scrubadub-base: FP B 38 vs 419; Redaction load % 13.7% vs 33.5%; FP 34 vs 224; Entity P 0.673 vs 0.335
- v2 Repeated PII values with decoys product_coverage validation scrubadub-base: FP B 38 vs 226; Redaction load % 11.5% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v2 Repeated PII values with decoys product_coverage test scrubadub-base: FP B 0 vs 193; Redaction load % 15.9% vs 36.2%; FP 21 vs 125; Entity P 0.656 vs 0.321
- v2 Repeated PII values with decoys common_intersection full scrubadub-base: FP B 38 vs 274; Redaction load % 13.9% vs 15.8%; FP 4 vs 22; Entity P 0.946 vs 0.761; F1 0.972 vs 0.864; F2 0.989 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation scrubadub-base: FP B 38 vs 163; Redaction load % 12.3% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v2 Repeated PII values with decoys common_intersection test scrubadub-base: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v2 Repeated PII values with decoys full aggregate scrubadub-spacy: FP B 193 vs 419
- v2 Repeated PII values with decoys product_coverage full scrubadub-spacy: FP B 193 vs 419; Redaction load % 21.6% vs 33.5%; FP 140 vs 224; Entity P 0.355 vs 0.335
- v2 Repeated PII values with decoys product_coverage validation scrubadub-spacy: FP B 102 vs 226; Redaction load % 18.9% vs 30.5%; FP 60 vs 99; Entity P 0.362 vs 0.353
- v2 Repeated PII values with decoys product_coverage test scrubadub-spacy: FP B 91 vs 193; Redaction load % 24.1% vs 36.2%; FP 80 vs 125; Entity P 0.350 vs 0.321
- v2 Repeated PII values with decoys common_intersection full scrubadub-spacy: FP B 38 vs 274; Redaction load % 13.6% vs 15.8%; FP 4 vs 22; Entity P 0.944 vs 0.761; F1 0.958 vs 0.864; F2 0.966 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation scrubadub-spacy: FP B 38 vs 163; Redaction load % 12.0% vs 14.4%; FP 4 vs 13; Entity P 0.879 vs 0.698; F1 0.921 vs 0.822; F2 0.948 vs 0.920
- v2 Repeated PII values with decoys common_intersection test scrubadub-spacy: FP B 0 vs 111; Redaction load % 15.1% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 0.987 vs 0.899; F2 0.980 vs 0.957
- v2 Repeated PII values with decoys product_coverage full gliner: Redaction load % 32.0% vs 33.5%; TP 120 vs 113; FP 178 vs 224; FN 320 vs 327; Entity P 0.403 vs 0.335; Entity R 0.273 vs 0.257; F1 0.325 vs 0.291; F2 0.292 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation gliner: TP 67 vs 54; FP 75 vs 99; FN 134 vs 147; Entity P 0.472 vs 0.353; Entity R 0.333 vs 0.269; F1 0.391 vs 0.305; F2 0.354 vs 0.282
- v2 Repeated PII values with decoys product_coverage test gliner: FP B 176 vs 193; Redaction load % 33.3% vs 36.2%; FP 103 vs 125; Entity P 0.340 vs 0.321
- v2 Repeated PII values with decoys common_intersection full gliner: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation gliner: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v2 Repeated PII values with decoys common_intersection test gliner: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v2 Repeated PII values with decoys product_coverage full gliner-high-recall: TP 124 vs 113; FN 316 vs 327; Entity P 0.344 vs 0.335; Entity R 0.282 vs 0.257; F1 0.310 vs 0.291; F2 0.292 vs 0.269
- v2 Repeated PII values with decoys product_coverage validation gliner-high-recall: Leaked B 135 vs 214; TP 68 vs 54; FP 98 vs 99; FN 133 vs 147; Entity P 0.410 vs 0.353; Entity R 0.338 vs 0.269; F1 0.371 vs 0.305; F2 0.351 vs 0.282
- v2 Repeated PII values with decoys common_intersection full gliner-high-recall: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v2 Repeated PII values with decoys common_intersection validation gliner-high-recall: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v2 Repeated PII values with decoys common_intersection test gliner-high-recall: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v2 Repeated PII values with decoys full aggregate opf: Leaked B 182 vs 234
- v2 Repeated PII values with decoys product_coverage full opf: Leaked B 182 vs 234
- v2 Repeated PII values with decoys product_coverage validation opf: Leaked B 110 vs 214; Leaking docs 7 vs 9; Doc leak % 14.9% vs 19.1%; Leaking entities 13 vs 17; Entity leak % 6.5% vs 8.5%
- v1 Synthetic identifiers in agentic formats product_coverage full presidio-all: Redaction load % 23.8% vs 27.7%
- v1 Synthetic identifiers in agentic formats product_coverage validation presidio-all: Redaction load % 23.3% vs 28.1%
- v1 Synthetic identifiers in agentic formats product_coverage test presidio-all: Redaction load % 24.3% vs 27.3%
- v1 Synthetic identifiers in agentic formats product_coverage full presidio-en: Redaction load % 11.3% vs 27.7%; FP 1,143 vs 1,782
- v1 Synthetic identifiers in agentic formats product_coverage validation presidio-en: Redaction load % 10.6% vs 28.1%; FP 569 vs 920
- v1 Synthetic identifiers in agentic formats product_coverage test presidio-en: Redaction load % 12.1% vs 27.3%; FP 574 vs 862
- v1 Synthetic identifiers in agentic formats common_intersection full presidio-en: Redaction load % 4.9% vs 5.5%
- v1 Synthetic identifiers in agentic formats common_intersection validation presidio-en: Redaction load % 4.4% vs 5.5%
- v1 Synthetic identifiers in agentic formats common_intersection test presidio-en: Redaction load % 5.5% vs 5.6%
- v1 Synthetic identifiers in agentic formats product_coverage full presidio-en-de: Redaction load % 18.8% vs 27.7%
- v1 Synthetic identifiers in agentic formats product_coverage validation presidio-en-de: Redaction load % 18.3% vs 28.1%
- v1 Synthetic identifiers in agentic formats product_coverage test presidio-en-de: Redaction load % 19.3% vs 27.3%
- v1 Synthetic identifiers in agentic formats product_coverage full presidio-strong: Redaction load % 22.4% vs 27.7%
- v1 Synthetic identifiers in agentic formats product_coverage validation presidio-strong: Redaction load % 21.9% vs 28.1%
- v1 Synthetic identifiers in agentic formats product_coverage test presidio-strong: Redaction load % 22.9% vs 27.3%
- v1 Synthetic identifiers in agentic formats product_coverage full presidio-strong-high-recall: Redaction load % 22.6% vs 27.7%
- v1 Synthetic identifiers in agentic formats product_coverage validation presidio-strong-high-recall: Redaction load % 22.2% vs 28.1%
- v1 Synthetic identifiers in agentic formats product_coverage test presidio-strong-high-recall: Redaction load % 23.1% vs 27.3%
- v1 Synthetic identifiers in agentic formats product_coverage full datafog-core: Redaction load % 12.0% vs 27.7%; FP 1,222 vs 1,782
- v1 Synthetic identifiers in agentic formats product_coverage validation datafog-core: Redaction load % 11.8% vs 28.1%; FP 623 vs 920
- v1 Synthetic identifiers in agentic formats product_coverage test datafog-core: Redaction load % 12.3% vs 27.3%; FP 599 vs 862
- v1 Synthetic identifiers in agentic formats common_intersection full datafog-core: Redaction load % 5.3% vs 5.5%
- v1 Synthetic identifiers in agentic formats common_intersection validation datafog-core: Redaction load % 5.0% vs 5.5%
- v1 Synthetic identifiers in agentic formats product_coverage full datafog-regex: Redaction load % 17.0% vs 27.7%; FP 964 vs 1,782; Entity P 0.417 vs 0.298; F1 0.308 vs 0.282
- v1 Synthetic identifiers in agentic formats product_coverage validation datafog-regex: Redaction load % 17.3% vs 28.1%; FP 490 vs 920; Entity P 0.432 vs 0.307; F1 0.321 vs 0.292
- v1 Synthetic identifiers in agentic formats product_coverage test datafog-regex: Redaction load % 16.6% vs 27.3%; FP 474 vs 862; Entity P 0.402 vs 0.288; F1 0.294 vs 0.270
- v1 Synthetic identifiers in agentic formats common_intersection full datafog-regex: Leaked B 200 vs 452; Leaking docs 20 vs 30; Doc leak % 5.9% vs 8.8%; Leaking entities 20 vs 30; Entity leak % 5.4% vs 8.1%; TP 350 vs 335; FN 20 vs 35; Entity R 0.946 vs 0.905
- v1 Synthetic identifiers in agentic formats common_intersection validation datafog-regex: Leaked B 100 vs 250; Leaking docs 10 vs 17; Doc leak % 5.7% vs 9.8%; Leaking entities 10 vs 17; Entity leak % 5.2% vs 8.9%; TP 182 vs 172; FN 10 vs 20; Entity R 0.948 vs 0.896
- v1 Synthetic identifiers in agentic formats common_intersection test datafog-regex: Leaked B 100 vs 202; Leaking docs 10 vs 13; Doc leak % 6.0% vs 7.8%; Leaking entities 10 vs 13; Entity leak % 5.6% vs 7.3%; TP 168 vs 163; FN 10 vs 15; Entity R 0.944 vs 0.916
- v1 Synthetic identifiers in agentic formats product_coverage full datafog-spacy: Redaction load % 24.8% vs 27.7%
- v1 Synthetic identifiers in agentic formats product_coverage validation datafog-spacy: Redaction load % 24.8% vs 28.1%
- v1 Synthetic identifiers in agentic formats product_coverage test datafog-spacy: Redaction load % 24.9% vs 27.3%
- v1 Synthetic identifiers in agentic formats common_intersection full datafog-spacy: Leaked B 130 vs 452; Leaking docs 13 vs 30; Doc leak % 3.8% vs 8.8%; Leaking entities 13 vs 30; Entity leak % 3.5% vs 8.1%; TP 357 vs 335; FN 13 vs 35; Entity R 0.965 vs 0.905
- v1 Synthetic identifiers in agentic formats common_intersection validation datafog-spacy: Leaked B 90 vs 250; Leaking docs 9 vs 17; Doc leak % 5.2% vs 9.8%; Leaking entities 9 vs 17; Entity leak % 4.7% vs 8.9%; TP 183 vs 172; FN 9 vs 20; Entity R 0.953 vs 0.896
- v1 Synthetic identifiers in agentic formats common_intersection test datafog-spacy: Leaked B 40 vs 202; Leaking docs 4 vs 13; Doc leak % 2.4% vs 7.8%; Leaking entities 4 vs 13; Entity leak % 2.2% vs 7.3%; TP 174 vs 163; FN 4 vs 15; Entity R 0.978 vs 0.916
- v1 Synthetic identifiers in agentic formats product_coverage full datafog-gliner: Redaction load % 19.2% vs 27.7%; FP 1,260 vs 1,782; Entity P 0.360 vs 0.298; F1 0.296 vs 0.282
- v1 Synthetic identifiers in agentic formats product_coverage validation datafog-gliner: Redaction load % 19.8% vs 28.1%; FP 650 vs 920; Entity P 0.370 vs 0.307; F1 0.307 vs 0.292
- v1 Synthetic identifiers in agentic formats product_coverage test datafog-gliner: Redaction load % 18.6% vs 27.3%; FP 610 vs 862; Entity P 0.350 vs 0.288; F1 0.284 vs 0.270
- v1 Synthetic identifiers in agentic formats common_intersection full datafog-gliner: Leaked B 0 vs 452; Leaking docs 0 vs 30; Doc leak % 0.0% vs 8.8%; Leaking entities 0 vs 30; Entity leak % 0.0% vs 8.1%; TP 370 vs 335; FN 0 vs 35; Entity R 1.000 vs 0.905
- v1 Synthetic identifiers in agentic formats common_intersection validation datafog-gliner: Leaked B 0 vs 250; Leaking docs 0 vs 17; Doc leak % 0.0% vs 9.8%; Leaking entities 0 vs 17; Entity leak % 0.0% vs 8.9%; TP 192 vs 172; FN 0 vs 20; Entity R 1.000 vs 0.896
- v1 Synthetic identifiers in agentic formats common_intersection test datafog-gliner: Leaked B 0 vs 202; Leaking docs 0 vs 13; Doc leak % 0.0% vs 7.8%; Leaking entities 0 vs 13; Entity leak % 0.0% vs 7.3%; TP 178 vs 163; FN 0 vs 15; Entity R 1.000 vs 0.916
- v1 Synthetic identifiers in agentic formats full aggregate scrubadub-base: FP B 133 vs 882
- v1 Synthetic identifiers in agentic formats product_coverage full scrubadub-base: FP B 133 vs 882; Redaction load % 4.6% vs 27.7%; FP 328 vs 1,782
- v1 Synthetic identifiers in agentic formats product_coverage validation scrubadub-base: FP B 54 vs 461; Redaction load % 4.3% vs 28.1%; FP 161 vs 920
- v1 Synthetic identifiers in agentic formats product_coverage test scrubadub-base: FP B 79 vs 421; Redaction load % 4.8% vs 27.3%; FP 167 vs 862
- v1 Synthetic identifiers in agentic formats common_intersection full scrubadub-base: Redaction load % 2.3% vs 5.5%; FP 14 vs 33
- v1 Synthetic identifiers in agentic formats common_intersection validation scrubadub-base: FP B 54 vs 83; Redaction load % 2.2% vs 5.5%; FP 7 vs 22
- v1 Synthetic identifiers in agentic formats common_intersection test scrubadub-base: Redaction load % 2.5% vs 5.6%; FP 7 vs 11
- v1 Synthetic identifiers in agentic formats product_coverage full scrubadub-spacy: Redaction load % 6.4% vs 27.7%; FP 673 vs 1,782
- v1 Synthetic identifiers in agentic formats product_coverage validation scrubadub-spacy: Redaction load % 6.1% vs 28.1%; FP 335 vs 920
- v1 Synthetic identifiers in agentic formats product_coverage test scrubadub-spacy: Redaction load % 6.8% vs 27.3%; FP 338 vs 862
- v1 Synthetic identifiers in agentic formats common_intersection full scrubadub-spacy: Redaction load % 2.1% vs 5.5%; FP 14 vs 33
- v1 Synthetic identifiers in agentic formats common_intersection validation scrubadub-spacy: FP B 54 vs 83; Redaction load % 2.1% vs 5.5%; FP 7 vs 22
- v1 Synthetic identifiers in agentic formats common_intersection test scrubadub-spacy: Redaction load % 2.1% vs 5.6%; FP 7 vs 11
- v1 Synthetic identifiers in agentic formats product_coverage full gliner: Redaction load % 26.3% vs 27.7%; TP 1,075 vs 756; FP 1,034 vs 1,782; FN 1,755 vs 2,074; Entity P 0.510 vs 0.298; Entity R 0.380 vs 0.267; F1 0.435 vs 0.282; F2 0.400 vs 0.273
- v1 Synthetic identifiers in agentic formats product_coverage validation gliner: Redaction load % 26.3% vs 28.1%; TP 568 vs 407; FP 513 vs 920; FN 890 vs 1,051; Entity P 0.525 vs 0.307; Entity R 0.390 vs 0.279; F1 0.447 vs 0.292; F2 0.411 vs 0.284
- v1 Synthetic identifiers in agentic formats product_coverage test gliner: Redaction load % 26.3% vs 27.3%; TP 507 vs 349; FP 521 vs 862; FN 865 vs 1,023; Entity P 0.493 vs 0.288; Entity R 0.370 vs 0.254; F1 0.422 vs 0.270; F2 0.389 vs 0.260
- v1 Synthetic identifiers in agentic formats common_intersection full gliner: Leaked B 266 vs 452; Leaking docs 14 vs 30; Doc leak % 4.1% vs 8.8%; Leaking entities 14 vs 30; Entity leak % 3.8% vs 8.1%
- v1 Synthetic identifiers in agentic formats common_intersection validation gliner: Leaked B 95 vs 250; Leaking docs 5 vs 17; Doc leak % 2.9% vs 9.8%; Leaking entities 5 vs 17; Entity leak % 2.6% vs 8.9%
- v1 Synthetic identifiers in agentic formats common_intersection test gliner: Leaked B 171 vs 202; Leaking docs 9 vs 13; Doc leak % 5.4% vs 7.8%; Leaking entities 9 vs 13; Entity leak % 5.1% vs 7.3%
- v1 Synthetic identifiers in agentic formats product_coverage full gliner-high-recall: TP 1,099 vs 756; FN 1,731 vs 2,074; Entity P 0.374 vs 0.298; Entity R 0.388 vs 0.267; F1 0.381 vs 0.282; F2 0.385 vs 0.273
- v1 Synthetic identifiers in agentic formats product_coverage validation gliner-high-recall: TP 578 vs 407; FP 916 vs 920; FN 880 vs 1,051; Entity P 0.387 vs 0.307; Entity R 0.396 vs 0.279; F1 0.392 vs 0.292; F2 0.394 vs 0.284
- v1 Synthetic identifiers in agentic formats product_coverage test gliner-high-recall: TP 521 vs 349; FN 851 vs 1,023; Entity P 0.362 vs 0.288; Entity R 0.380 vs 0.254; F1 0.370 vs 0.270; F2 0.376 vs 0.260
- v1 Synthetic identifiers in agentic formats common_intersection full gliner-high-recall: Leaked B 152 vs 452; Leaking docs 8 vs 30; Doc leak % 2.4% vs 8.8%; Leaking entities 8 vs 30; Entity leak % 2.2% vs 8.1%
- v1 Synthetic identifiers in agentic formats common_intersection validation gliner-high-recall: Leaked B 57 vs 250; Leaking docs 3 vs 17; Doc leak % 1.7% vs 9.8%; Leaking entities 3 vs 17; Entity leak % 1.6% vs 8.9%
- v1 Synthetic identifiers in agentic formats common_intersection test gliner-high-recall: Leaked B 95 vs 202; Leaking docs 5 vs 13; Doc leak % 3.0% vs 7.8%; Leaking entities 5 vs 13; Entity leak % 2.8% vs 7.3%
- v1 Synthetic identifiers in agentic formats product_coverage full opf: Redaction load % 27.6% vs 27.7%; TP 913 vs 756; FP 1,281 vs 1,782; FN 1,917 vs 2,074; Entity P 0.416 vs 0.298; Entity R 0.323 vs 0.267; F1 0.363 vs 0.282; F2 0.338 vs 0.273
- v1 Synthetic identifiers in agentic formats product_coverage validation opf: TP 476 vs 407; FP 676 vs 920; FN 982 vs 1,051; Entity P 0.413 vs 0.307; Entity R 0.326 vs 0.279; F1 0.365 vs 0.292; F2 0.341 vs 0.284
- v1 Synthetic identifiers in agentic formats product_coverage test opf: Redaction load % 26.9% vs 27.3%; TP 437 vs 349; FP 605 vs 862; FN 935 vs 1,023; Entity P 0.419 vs 0.288; Entity R 0.319 vs 0.254; F1 0.362 vs 0.270; F2 0.335 vs 0.260
- v1 Synthetic identifiers in agentic formats common_intersection full opf: Leaked B 401 vs 452; Leaking docs 25 vs 30; Doc leak % 7.4% vs 8.8%; Leaking entities 25 vs 30; Entity leak % 6.8% vs 8.1%
- v1 Synthetic identifiers in agentic formats common_intersection validation opf: Leaked B 204 vs 250; Leaking docs 11 vs 17; Doc leak % 6.3% vs 9.8%; Leaking entities 11 vs 17; Entity leak % 5.7% vs 8.9%; TP 173 vs 172; FN 19 vs 20; Entity R 0.901 vs 0.896
- v1 Synthetic identifiers in agentic formats common_intersection test opf: Leaked B 197 vs 202
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full presidio-en: Redaction load % 8.8% vs 14.8%; FP 4,844 vs 5,489
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation presidio-en: Redaction load % 8.8% vs 14.6%; FP 2,401 vs 2,618
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test presidio-en: Redaction load % 8.8% vs 15.0%; FP 2,443 vs 2,871
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full presidio-en: Redaction load % 2.7% vs 3.9%; FP 1,440 vs 1,781
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation presidio-en: Redaction load % 2.8% vs 3.9%; FP 724 vs 870
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test presidio-en: FP B 6,005 vs 6,138; Redaction load % 2.7% vs 4.0%; FP 716 vs 911
- v1 Kiji EN/DE holdout and A4 negatives full aggregate datafog-core: FP B 5,992 vs 27,282
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-core: FP B 5,992 vs 27,282; Redaction load % 2.7% vs 14.8%; FP 830 vs 5,489
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-core: FP B 2,919 vs 13,285; Redaction load % 2.7% vs 14.6%; FP 422 vs 2,618
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-core: FP B 3,073 vs 13,997; Redaction load % 2.7% vs 15.0%; FP 408 vs 2,871
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-core: FP B 2,244 vs 11,929; Redaction load % 2.2% vs 3.9%; FP 353 vs 1,781; Entity P 0.767 vs 0.512
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-core: FP B 1,190 vs 5,791; Redaction load % 2.2% vs 3.9%; FP 190 vs 870; Entity P 0.746 vs 0.503
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-core: FP B 1,054 vs 6,138; Redaction load % 2.2% vs 4.0%; FP 163 vs 911; Entity P 0.787 vs 0.521
- v1 Kiji EN/DE holdout and A4 negatives full aggregate datafog-regex: FP B 6,919 vs 27,282
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full datafog-regex: FP B 6,919 vs 27,282; Redaction load % 3.3% vs 14.8%; FP 1,107 vs 5,489
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation datafog-regex: FP B 3,740 vs 13,285; Redaction load % 3.4% vs 14.6%; FP 579 vs 2,618
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test datafog-regex: FP B 3,179 vs 13,997; Redaction load % 3.2% vs 15.0%; FP 528 vs 2,871
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-regex: FP B 5,200 vs 11,929; Redaction load % 2.7% vs 3.9%; FP 553 vs 1,781; Entity P 0.687 vs 0.512
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-regex: FP B 2,942 vs 5,791; Redaction load % 2.8% vs 3.9%; FP 307 vs 870; Entity P 0.655 vs 0.503
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-regex: FP B 2,258 vs 6,138; Redaction load % 2.6% vs 4.0%; FP 246 vs 911; Entity P 0.719 vs 0.521
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full datafog-gliner: Leaked B 1,719 vs 2,569; Leaking docs 265 vs 294; Doc leak % 17.8% vs 19.7%; Leaking entities 280 vs 315; Entity leak % 12.6% vs 14.1%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation datafog-gliner: Leaked B 833 vs 1,309; Leaking docs 127 vs 148; Doc leak % 17.9% vs 20.9%; Leaking entities 133 vs 162; Entity leak % 12.5% vs 15.2%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test datafog-gliner: Leaked B 886 vs 1,260; Leaking docs 138 vs 146; Doc leak % 17.7% vs 18.7%; Leaking entities 147 vs 153; Entity leak % 12.6% vs 13.1%
- v1 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-base: FP B 1,525 vs 27,282
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-base: FP B 1,525 vs 27,282; Redaction load % 2.3% vs 14.8%; FP 222 vs 5,489; Entity P 0.795 vs 0.634
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-base: FP B 822 vs 13,285; Redaction load % 2.3% vs 14.6%; FP 107 vs 2,618; Entity P 0.797 vs 0.634
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-base: FP B 703 vs 13,997; Redaction load % 2.3% vs 15.0%; FP 115 vs 2,871; Entity P 0.793 vs 0.635
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-base: FP B 1,074 vs 11,929; Redaction load % 1.6% vs 3.9%; FP 108 vs 1,781; Entity P 0.850 vs 0.512
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-base: FP B 555 vs 5,791; Redaction load % 1.6% vs 3.9%; FP 55 vs 870; Entity P 0.841 vs 0.503
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-base: FP B 519 vs 6,138; Redaction load % 1.6% vs 4.0%; FP 53 vs 911; Entity P 0.859 vs 0.521
- v1 Kiji EN/DE holdout and A4 negatives full aggregate scrubadub-spacy: FP B 15,310 vs 27,282
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full scrubadub-spacy: FP B 15,310 vs 27,282; Redaction load % 5.8% vs 14.8%; FP 2,701 vs 5,489
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation scrubadub-spacy: FP B 7,700 vs 13,285; Redaction load % 5.9% vs 14.6%; FP 1,314 vs 2,618
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test scrubadub-spacy: FP B 7,610 vs 13,997; Redaction load % 5.8% vs 15.0%; FP 1,387 vs 2,871
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full scrubadub-spacy: FP B 884 vs 11,929; Redaction load % 1.5% vs 3.9%; FP 89 vs 1,781; Entity P 0.870 vs 0.512
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation scrubadub-spacy: FP B 470 vs 5,791; Redaction load % 1.5% vs 3.9%; FP 46 vs 870; Entity P 0.858 vs 0.503
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test scrubadub-spacy: FP B 414 vs 6,138; Redaction load % 1.5% vs 4.0%; FP 43 vs 911; Entity P 0.880 vs 0.521
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full gliner: Leaked B 1,934 vs 2,569; Leaking docs 179 vs 294; Doc leak % 12.0% vs 19.7%; Leaking entities 204 vs 315; Entity leak % 9.1% vs 14.1%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner: Leaked B 963 vs 1,309; Leaking docs 91 vs 148; Doc leak % 12.9% vs 20.9%; Leaking entities 100 vs 162; Entity leak % 9.4% vs 15.2%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test gliner: Leaked B 971 vs 1,260; Leaking docs 88 vs 146; Doc leak % 11.3% vs 18.7%; Leaking entities 104 vs 153; Entity leak % 8.9% vs 13.1%
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full gliner-high-recall: Leaking docs 1,181 vs 1,198; Doc leak % 62.6% vs 63.5%
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test gliner-high-recall: Leaking docs 613 vs 633; Doc leak % 61.8% vs 63.8%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full gliner-high-recall: Leaked B 1,497 vs 2,569; Leaking docs 136 vs 294; Doc leak % 9.1% vs 19.7%; Leaking entities 158 vs 315; Entity leak % 7.1% vs 14.1%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation gliner-high-recall: Leaked B 789 vs 1,309; Leaking docs 71 vs 148; Doc leak % 10.0% vs 20.9%; Leaking entities 80 vs 162; Entity leak % 7.5% vs 15.2%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test gliner-high-recall: Leaked B 708 vs 1,260; Leaking docs 65 vs 146; Doc leak % 8.3% vs 18.7%; Leaking entities 78 vs 153; Entity leak % 6.7% vs 13.1%
- v1 Kiji EN/DE holdout and A4 negatives product_coverage full opf: Redaction load % 14.2% vs 14.8%
- v1 Kiji EN/DE holdout and A4 negatives product_coverage validation opf: Redaction load % 14.0% vs 14.6%
- v1 Kiji EN/DE holdout and A4 negatives product_coverage test opf: Redaction load % 14.4% vs 15.0%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection full opf: Leaked B 675 vs 2,569; Leaking docs 125 vs 294; Doc leak % 8.4% vs 19.7%; Leaking entities 131 vs 315; Entity leak % 5.9% vs 14.1%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection validation opf: Leaked B 420 vs 1,309; Leaking docs 69 vs 148; Doc leak % 9.7% vs 20.9%; Leaking entities 75 vs 162; Entity leak % 7.0% vs 15.2%
- v1 Kiji EN/DE holdout and A4 negatives common_intersection test opf: Leaked B 255 vs 1,260; Leaking docs 56 vs 146; Doc leak % 7.2% vs 18.7%; Leaking entities 56 vs 153; Entity leak % 4.8% vs 13.1%
- v1 Synthetic benign lookalikes full aggregate datafog-regex: FP B 3,051 vs 3,544
- v1 Synthetic benign lookalikes product_coverage full datafog-regex: FP B 3,051 vs 3,544; Redaction load % 6.4% vs 7.4%; FP 324 vs 394
- v1 Synthetic benign lookalikes product_coverage validation datafog-regex: FP B 1,591 vs 1,910; Redaction load % 6.5% vs 7.9%; FP 168 vs 216
- v1 Synthetic benign lookalikes product_coverage test datafog-regex: FP B 1,460 vs 1,634; Redaction load % 6.2% vs 6.9%; FP 156 vs 178
- v1 Synthetic benign lookalikes product_coverage full datafog-gliner: FP 375 vs 394
- v1 Synthetic benign lookalikes product_coverage validation datafog-gliner: FP B 1,887 vs 1,910; Redaction load % 7.8% vs 7.9%; FP 195 vs 216
- v1 Synthetic benign lookalikes full aggregate scrubadub-base: FP B 156 vs 3,544
- v1 Synthetic benign lookalikes product_coverage full scrubadub-base: FP B 156 vs 3,544; Redaction load % 0.3% vs 7.4%; FP 15 vs 394
- v1 Synthetic benign lookalikes product_coverage validation scrubadub-base: FP B 94 vs 1,910; Redaction load % 0.4% vs 7.9%; FP 9 vs 216
- v1 Synthetic benign lookalikes product_coverage test scrubadub-base: FP B 62 vs 1,634; Redaction load % 0.3% vs 6.9%; FP 6 vs 178
- v1 Synthetic benign lookalikes common_intersection full scrubadub-base: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v1 Synthetic benign lookalikes common_intersection validation scrubadub-base: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v1 Synthetic benign lookalikes common_intersection test scrubadub-base: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v1 Synthetic benign lookalikes full aggregate scrubadub-spacy: FP B 1,297 vs 3,544
- v1 Synthetic benign lookalikes product_coverage full scrubadub-spacy: FP B 1,297 vs 3,544; Redaction load % 2.7% vs 7.4%; FP 104 vs 394
- v1 Synthetic benign lookalikes product_coverage validation scrubadub-spacy: FP B 647 vs 1,910; Redaction load % 2.7% vs 7.9%; FP 50 vs 216
- v1 Synthetic benign lookalikes product_coverage test scrubadub-spacy: FP B 650 vs 1,634; Redaction load % 2.7% vs 6.9%; FP 54 vs 178
- v1 Synthetic benign lookalikes common_intersection full scrubadub-spacy: FP B 156 vs 1,167; Redaction load % 0.3% vs 2.4%; FP 15 vs 152
- v1 Synthetic benign lookalikes common_intersection validation scrubadub-spacy: FP B 94 vs 672; Redaction load % 0.4% vs 2.8%; FP 9 vs 89
- v1 Synthetic benign lookalikes common_intersection test scrubadub-spacy: FP B 62 vs 495; Redaction load % 0.3% vs 2.1%; FP 6 vs 63
- v1 Synthetic benign lookalikes full aggregate gliner: FP B 1,684 vs 3,544
- v1 Synthetic benign lookalikes product_coverage full gliner: FP B 1,684 vs 3,544; Redaction load % 3.5% vs 7.4%; FP 129 vs 394
- v1 Synthetic benign lookalikes product_coverage validation gliner: FP B 1,000 vs 1,910; Redaction load % 4.1% vs 7.9%; FP 75 vs 216
- v1 Synthetic benign lookalikes product_coverage test gliner: FP B 684 vs 1,634; Redaction load % 2.9% vs 6.9%; FP 54 vs 178
- v1 Synthetic benign lookalikes common_intersection full gliner: FP B 115 vs 1,167; Redaction load % 0.2% vs 2.4%; FP 7 vs 152
- v1 Synthetic benign lookalikes common_intersection validation gliner: FP B 96 vs 672; Redaction load % 0.4% vs 2.8%; FP 6 vs 89
- v1 Synthetic benign lookalikes common_intersection test gliner: FP B 19 vs 495; Redaction load % 0.1% vs 2.1%; FP 1 vs 63
- v1 Synthetic benign lookalikes full aggregate gliner-high-recall: FP B 3,264 vs 3,544
- v1 Synthetic benign lookalikes product_coverage full gliner-high-recall: FP B 3,264 vs 3,544; Redaction load % 6.8% vs 7.4%; FP 271 vs 394
- v1 Synthetic benign lookalikes product_coverage validation gliner-high-recall: FP B 1,849 vs 1,910; Redaction load % 7.6% vs 7.9%; FP 151 vs 216
- v1 Synthetic benign lookalikes product_coverage test gliner-high-recall: FP B 1,415 vs 1,634; Redaction load % 6.0% vs 6.9%; FP 120 vs 178
- v1 Synthetic benign lookalikes common_intersection full gliner-high-recall: FP B 899 vs 1,167; Redaction load % 1.9% vs 2.4%; FP 69 vs 152
- v1 Synthetic benign lookalikes common_intersection validation gliner-high-recall: FP B 476 vs 672; Redaction load % 2.0% vs 2.8%; FP 36 vs 89
- v1 Synthetic benign lookalikes common_intersection test gliner-high-recall: FP B 423 vs 495; Redaction load % 1.8% vs 2.1%; FP 33 vs 63
- v1 Synthetic benign lookalikes product_coverage full opf: FP 369 vs 394
- v1 Synthetic benign lookalikes product_coverage validation opf: FP 193 vs 216
- v1 Synthetic benign lookalikes product_coverage test opf: FP 176 vs 178
- v1 Repeated PII values with decoys product_coverage full presidio-all: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation presidio-all: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v1 Repeated PII values with decoys product_coverage test presidio-all: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v1 Repeated PII values with decoys full aggregate presidio-en: FP B 222 vs 419
- v1 Repeated PII values with decoys product_coverage full presidio-en: FP B 222 vs 419; Redaction load % 15.1% vs 33.5%; FP 122 vs 224
- v1 Repeated PII values with decoys product_coverage validation presidio-en: FP B 101 vs 226; Redaction load % 14.5% vs 30.5%; FP 55 vs 99
- v1 Repeated PII values with decoys product_coverage test presidio-en: FP B 121 vs 193; Redaction load % 15.6% vs 36.2%; FP 67 vs 125
- v1 Repeated PII values with decoys common_intersection full presidio-en: FP B 67 vs 274; Redaction load % 8.9% vs 15.8%; FP 13 vs 22; Entity P 0.772 vs 0.761
- v1 Repeated PII values with decoys common_intersection validation presidio-en: FP B 37 vs 163; Redaction load % 8.5% vs 14.4%; FP 7 vs 13; Entity P 0.741 vs 0.698
- v1 Repeated PII values with decoys common_intersection test presidio-en: FP B 30 vs 111; Redaction load % 9.2% vs 17.2%; FP 6 vs 9
- v1 Repeated PII values with decoys product_coverage full presidio-en-de: TP 129 vs 113; FN 311 vs 327; Entity R 0.293 vs 0.257; F1 0.298 vs 0.291; F2 0.295 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation presidio-en-de: TP 68 vs 54; FN 133 vs 147; Entity R 0.338 vs 0.269; F1 0.340 vs 0.305; F2 0.339 vs 0.282
- v1 Repeated PII values with decoys product_coverage test presidio-en-de: TP 61 vs 59; FN 178 vs 180; Entity R 0.255 vs 0.247
- v1 Repeated PII values with decoys product_coverage full presidio-strong: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation presidio-strong: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v1 Repeated PII values with decoys product_coverage test presidio-strong: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v1 Repeated PII values with decoys product_coverage full presidio-strong-high-recall: TP 139 vs 113; FN 301 vs 327; Entity R 0.316 vs 0.257; F1 0.322 vs 0.291; F2 0.318 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation presidio-strong-high-recall: TP 72 vs 54; FN 129 vs 147; Entity R 0.358 vs 0.269; F1 0.356 vs 0.305; F2 0.357 vs 0.282
- v1 Repeated PII values with decoys product_coverage test presidio-strong-high-recall: TP 67 vs 59; FN 172 vs 180; Entity R 0.280 vs 0.247; F1 0.292 vs 0.279; F2 0.285 vs 0.259
- v1 Repeated PII values with decoys full aggregate datafog-core: FP B 0 vs 419
- v1 Repeated PII values with decoys product_coverage full datafog-core: FP B 0 vs 419; Redaction load % 13.9% vs 33.5%; FP 38 vs 224; Entity P 0.648 vs 0.335
- v1 Repeated PII values with decoys product_coverage validation datafog-core: FP B 0 vs 226; Redaction load % 11.6% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v1 Repeated PII values with decoys product_coverage test datafog-core: FP B 0 vs 193; Redaction load % 16.1% vs 36.2%; FP 25 vs 125; Entity P 0.615 vs 0.321
- v1 Repeated PII values with decoys common_intersection full datafog-core: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation datafog-core: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v1 Repeated PII values with decoys common_intersection test datafog-core: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v1 Repeated PII values with decoys full aggregate datafog-regex: FP B 0 vs 419
- v1 Repeated PII values with decoys product_coverage full datafog-regex: FP B 0 vs 419; Redaction load % 18.2% vs 33.5%; FP 34 vs 224; Entity P 0.746 vs 0.335; F1 0.348 vs 0.291
- v1 Repeated PII values with decoys product_coverage validation datafog-regex: FP B 0 vs 226; Redaction load % 16.2% vs 30.5%; FP 13 vs 99; Entity P 0.776 vs 0.353; F1 0.347 vs 0.305
- v1 Repeated PII values with decoys product_coverage test datafog-regex: FP B 0 vs 193; Redaction load % 20.2% vs 36.2%; FP 21 vs 125; Entity P 0.724 vs 0.321; F1 0.349 vs 0.279; F2 0.266 vs 0.259
- v1 Repeated PII values with decoys common_intersection full datafog-regex: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation datafog-regex: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v1 Repeated PII values with decoys common_intersection test datafog-regex: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v1 Repeated PII values with decoys product_coverage full datafog-spacy: Redaction load % 32.9% vs 33.5%
- v1 Repeated PII values with decoys product_coverage validation datafog-spacy: Redaction load % 29.1% vs 30.5%
- v1 Repeated PII values with decoys full aggregate datafog-gliner: FP B 310 vs 419
- v1 Repeated PII values with decoys product_coverage full datafog-gliner: FP B 310 vs 419; Redaction load % 30.1% vs 33.5%; FP 168 vs 224; Entity P 0.373 vs 0.335
- v1 Repeated PII values with decoys product_coverage validation datafog-gliner: FP B 126 vs 226; Redaction load % 26.2% vs 30.5%; FP 67 vs 99; Entity P 0.402 vs 0.353
- v1 Repeated PII values with decoys product_coverage test datafog-gliner: FP B 184 vs 193; Redaction load % 33.7% vs 36.2%; FP 101 vs 125; Entity P 0.353 vs 0.321
- v1 Repeated PII values with decoys common_intersection full datafog-gliner: FP B 100 vs 274; Redaction load % 14.4% vs 15.8%; FP 10 vs 22; Entity P 0.875 vs 0.761; F1 0.933 vs 0.864; F2 0.972 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation datafog-gliner: FP B 40 vs 163; Redaction load % 12.4% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v1 Repeated PII values with decoys common_intersection test datafog-gliner: FP B 60 vs 111; Redaction load % 16.4% vs 17.2%; FP 6 vs 9; Entity P 0.870 vs 0.816; F1 0.930 vs 0.899; F2 0.971 vs 0.957
- v1 Repeated PII values with decoys full aggregate scrubadub-base: FP B 38 vs 419
- v1 Repeated PII values with decoys product_coverage full scrubadub-base: FP B 38 vs 419; Redaction load % 13.7% vs 33.5%; FP 34 vs 224; Entity P 0.673 vs 0.335
- v1 Repeated PII values with decoys product_coverage validation scrubadub-base: FP B 38 vs 226; Redaction load % 11.5% vs 30.5%; FP 13 vs 99; Entity P 0.698 vs 0.353
- v1 Repeated PII values with decoys product_coverage test scrubadub-base: FP B 0 vs 193; Redaction load % 15.9% vs 36.2%; FP 21 vs 125; Entity P 0.656 vs 0.321
- v1 Repeated PII values with decoys common_intersection full scrubadub-base: FP B 38 vs 274; Redaction load % 13.9% vs 15.8%; FP 4 vs 22; Entity P 0.946 vs 0.761; F1 0.972 vs 0.864; F2 0.989 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation scrubadub-base: FP B 38 vs 163; Redaction load % 12.3% vs 14.4%; FP 4 vs 13; Entity P 0.882 vs 0.698; F1 0.938 vs 0.822; F2 0.974 vs 0.920
- v1 Repeated PII values with decoys common_intersection test scrubadub-base: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v1 Repeated PII values with decoys full aggregate scrubadub-spacy: FP B 193 vs 419
- v1 Repeated PII values with decoys product_coverage full scrubadub-spacy: FP B 193 vs 419; Redaction load % 21.6% vs 33.5%; FP 140 vs 224; Entity P 0.355 vs 0.335
- v1 Repeated PII values with decoys product_coverage validation scrubadub-spacy: FP B 102 vs 226; Redaction load % 18.9% vs 30.5%; FP 60 vs 99; Entity P 0.362 vs 0.353
- v1 Repeated PII values with decoys product_coverage test scrubadub-spacy: FP B 91 vs 193; Redaction load % 24.1% vs 36.2%; FP 80 vs 125; Entity P 0.350 vs 0.321
- v1 Repeated PII values with decoys common_intersection full scrubadub-spacy: FP B 38 vs 274; Redaction load % 13.6% vs 15.8%; FP 4 vs 22; Entity P 0.944 vs 0.761; F1 0.958 vs 0.864; F2 0.966 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation scrubadub-spacy: FP B 38 vs 163; Redaction load % 12.0% vs 14.4%; FP 4 vs 13; Entity P 0.879 vs 0.698; F1 0.921 vs 0.822; F2 0.948 vs 0.920
- v1 Repeated PII values with decoys common_intersection test scrubadub-spacy: FP B 0 vs 111; Redaction load % 15.1% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 0.987 vs 0.899; F2 0.980 vs 0.957
- v1 Repeated PII values with decoys product_coverage full gliner: Redaction load % 32.0% vs 33.5%; TP 120 vs 113; FP 178 vs 224; FN 320 vs 327; Entity P 0.403 vs 0.335; Entity R 0.273 vs 0.257; F1 0.325 vs 0.291; F2 0.292 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation gliner: TP 67 vs 54; FP 75 vs 99; FN 134 vs 147; Entity P 0.472 vs 0.353; Entity R 0.333 vs 0.269; F1 0.391 vs 0.305; F2 0.354 vs 0.282
- v1 Repeated PII values with decoys product_coverage test gliner: FP B 176 vs 193; Redaction load % 33.3% vs 36.2%; FP 103 vs 125; Entity P 0.340 vs 0.321
- v1 Repeated PII values with decoys common_intersection full gliner: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation gliner: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v1 Repeated PII values with decoys common_intersection test gliner: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v1 Repeated PII values with decoys product_coverage full gliner-high-recall: TP 124 vs 113; FN 316 vs 327; Entity P 0.344 vs 0.335; Entity R 0.282 vs 0.257; F1 0.310 vs 0.291; F2 0.292 vs 0.269
- v1 Repeated PII values with decoys product_coverage validation gliner-high-recall: Leaked B 135 vs 214; TP 68 vs 54; FP 98 vs 99; FN 133 vs 147; Entity P 0.410 vs 0.353; Entity R 0.338 vs 0.269; F1 0.371 vs 0.305; F2 0.351 vs 0.282
- v1 Repeated PII values with decoys common_intersection full gliner-high-recall: FP B 0 vs 274; Redaction load % 13.6% vs 15.8%; FP 0 vs 22; Entity P 1.000 vs 0.761; F1 1.000 vs 0.864; F2 1.000 vs 0.941
- v1 Repeated PII values with decoys common_intersection validation gliner-high-recall: FP B 0 vs 163; Redaction load % 11.7% vs 14.4%; FP 0 vs 13; Entity P 1.000 vs 0.698; F1 1.000 vs 0.822; F2 1.000 vs 0.920
- v1 Repeated PII values with decoys common_intersection test gliner-high-recall: FP B 0 vs 111; Redaction load % 15.5% vs 17.2%; FP 0 vs 9; Entity P 1.000 vs 0.816; F1 1.000 vs 0.899; F2 1.000 vs 0.957
- v1 Repeated PII values with decoys full aggregate opf: Leaked B 182 vs 234
- v1 Repeated PII values with decoys product_coverage full opf: Leaked B 182 vs 234
- v1 Repeated PII values with decoys product_coverage validation opf: Leaked B 110 vs 214; Leaking docs 7 vs 9; Doc leak % 14.9% vs 19.1%; Leaking entities 13 vs 17; Entity leak % 6.5% vs 8.5%

</details>

## Gaze ablations

Rules only, rules plus NER, and full setup use the same test documents and scorer.

| Contract | Layer | Gaze configuration | Leaked B | FP B | PII docs | Leaking docs | Entity F1 | Entity F2 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| v3 | Synthetic identifiers in agentic formats | rules-only | 3,274 | 36 | 1,119 | 229 | 0.279 | 0.262 |
| v3 | Synthetic identifiers in agentic formats | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.270 | 0.259 |
| v3 | Synthetic identifiers in agentic formats | full | 2,861 | 421 | 1,119 | 205 | 0.270 | 0.260 |
| v3 | Kiji EN/DE holdout and A4 negatives | rules-only | 42,430 | 1,853 | 992 | 990 | 0.334 | 0.248 |
| v3 | Kiji EN/DE holdout and A4 negatives | rules-ner | 6,266 | 13,805 | 992 | 621 | 0.638 | 0.639 |
| v3 | Kiji EN/DE holdout and A4 negatives | full | 4,806 | 13,997 | 992 | 524 | 0.650 | 0.658 |
| v3 | Synthetic benign lookalikes | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v3 | Repeated PII values with decoys | rules-only | 89 | 76 | 53 | 12 | 0.244 | 0.219 |
| v3 | Repeated PII values with decoys | rules-ner | 20 | 89 | 53 | 3 | 0.284 | 0.261 |
| v3 | Repeated PII values with decoys | full | 20 | 193 | 53 | 3 | 0.279 | 0.259 |
| v2 | Synthetic identifiers in agentic formats | rules-only | 3,274 | 36 | 1,119 | 229 | 0.279 | 0.262 |
| v2 | Synthetic identifiers in agentic formats | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.270 | 0.259 |
| v2 | Synthetic identifiers in agentic formats | full | 2,861 | 421 | 1,119 | 205 | 0.270 | 0.260 |
| v2 | Kiji EN/DE holdout and A4 negatives | rules-only | 42,430 | 1,853 | 992 | 990 | 0.334 | 0.248 |
| v2 | Kiji EN/DE holdout and A4 negatives | rules-ner | 6,266 | 13,805 | 992 | 621 | 0.638 | 0.639 |
| v2 | Kiji EN/DE holdout and A4 negatives | full | 4,806 | 13,997 | 992 | 524 | 0.650 | 0.658 |
| v2 | Synthetic benign lookalikes | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v2 | Repeated PII values with decoys | rules-only | 89 | 76 | 53 | 12 | 0.244 | 0.219 |
| v2 | Repeated PII values with decoys | rules-ner | 20 | 89 | 53 | 3 | 0.284 | 0.261 |
| v2 | Repeated PII values with decoys | full | 20 | 193 | 53 | 3 | 0.279 | 0.259 |
| v1 | Synthetic identifiers in agentic formats | rules-only | 3,274 | 36 | 1,119 | 229 | 0.279 | 0.262 |
| v1 | Synthetic identifiers in agentic formats | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.270 | 0.259 |
| v1 | Synthetic identifiers in agentic formats | full | 2,861 | 421 | 1,119 | 205 | 0.270 | 0.260 |
| v1 | Kiji EN/DE holdout and A4 negatives | rules-only | 45,838 | 1,853 | 992 | 990 | 0.326 | 0.241 |
| v1 | Kiji EN/DE holdout and A4 negatives | rules-ner | 9,628 | 13,805 | 992 | 710 | 0.629 | 0.624 |
| v1 | Kiji EN/DE holdout and A4 negatives | full | 7,937 | 13,997 | 992 | 633 | 0.640 | 0.643 |
| v1 | Synthetic benign lookalikes | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v1 | Repeated PII values with decoys | rules-only | 89 | 76 | 53 | 12 | 0.244 | 0.219 |
| v1 | Repeated PII values with decoys | rules-ner | 20 | 89 | 53 | 3 | 0.284 | 0.261 |
| v1 | Repeated PII values with decoys | full | 20 | 193 | 53 | 3 | 0.279 | 0.259 |

Threshold choice uses validation only: gliner → gliner-high-recall; presidio-strong → presidio-strong-high-recall.
