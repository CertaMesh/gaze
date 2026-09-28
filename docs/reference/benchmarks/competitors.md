# Competitor comparison

Same corpus and scorer; tools run with documented configurations. UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's false-positive count after its audited gold-gap credit. Presidio all runs English, German, Dutch, French, and Portuguese spaCy models with the documented German recognizers. Presidio English default is a secondary row. Latency was not measured under a quiet machine; timing comparisons are withheld. This measures detection; competitor restore and manifest behavior is not scored.

Leaked and false-positive byte counts are class-agnostic. A skipped document's scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to get leakage on processed documents. For example, Presidio English-only v3 A leaks 34,733 B, including 28,151 B of scored gold from 1,320 skipped non-English documents. The reviewed label map controls v3's repeated-gold credit and the exact typed-span metrics below.

Gaze measured at `a2f6fefd33cebd040a4fc6e5a384f318807717af` (release `v0.15.1`). Home-normalized setup policy SHA-256: `481f5df7a9b0b562bf2c2db7274cfbf2ea701231c52f1287b27de00554453ff1`.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`. GLiNER uses model-card labels, library threshold 0.5. OpenAI Privacy Filter (OPF) 0.1.0 at source `f7f00ca7fb869683eb732c010299d901457f19c3`.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published.

| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | Latency |
|---|---|---|---:|---:|---:|---:|---:|---|
| v3 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v3 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v3 | A | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v3 | C | gaze | 9,256 | 15,560 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | presidio-all | 30,615 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | presidio-en | 71,562 | 26,086 | 1,545 | 1,365 | 54,629 | not measured under a quiet machine |
| v3 | C | presidio-en-de | 30,615 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | presidio-strong | 25,314 | 40,898 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | presidio-strong-high-recall | 24,776 | 42,768 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | datafog-core | 103,617 | 5,839 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | datafog-regex | 99,234 | 6,766 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | datafog-spacy | 29,142 | 243,401 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | datafog-gliner | 26,834 | 47,142 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | scrubadub-base | 103,601 | 1,449 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | scrubadub-spacy | 84,397 | 11,942 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | gliner | 20,152 | 51,384 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | gliner-high-recall | 16,900 | 67,549 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | C | opf | 24,080 | 27,544 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v3 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v3 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v3 | D | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | D | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v3 | R | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v3 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v3 | R | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v2 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v2 | A | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v2 | C | gaze | 9,256 | 27,282 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | presidio-all | 30,615 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | presidio-en | 71,562 | 30,749 | 1,545 | 1,365 | 54,629 | not measured under a quiet machine |
| v2 | C | presidio-en-de | 30,615 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | presidio-strong | 25,314 | 49,720 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | presidio-strong-high-recall | 24,776 | 51,595 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | datafog-core | 103,617 | 5,992 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | datafog-regex | 99,234 | 6,919 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | datafog-spacy | 29,142 | 246,970 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | datafog-gliner | 26,834 | 49,434 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | scrubadub-base | 103,601 | 1,525 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | scrubadub-spacy | 84,397 | 15,310 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | gliner | 20,152 | 53,355 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | gliner-high-recall | 16,900 | 69,716 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | C | opf | 24,080 | 31,254 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v2 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v2 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v2 | D | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | D | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v2 | R | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v2 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v2 | R | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | not measured under a quiet machine |
| v1 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | not measured under a quiet machine |
| v1 | A | presidio-strong | 22,858 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | presidio-strong-high-recall | 22,498 | 9,617 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | datafog-core | 31,499 | 1,913 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | datafog-regex | 23,197 | 1,420 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | datafog-spacy | 18,989 | 9,585 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | datafog-gliner | 20,670 | 2,468 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | scrubadub-base | 41,473 | 133 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | scrubadub-spacy | 40,216 | 1,849 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | gliner-high-recall | 8,003 | 9,626 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | not measured under a quiet machine |
| v1 | C | gaze | 15,443 | 27,282 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | presidio-all | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | presidio-en | 76,658 | 30,749 | 1,545 | 1,365 | 57,835 | not measured under a quiet machine |
| v1 | C | presidio-en-de | 35,130 | 58,274 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | presidio-strong | 31,055 | 49,720 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | presidio-strong-high-recall | 30,517 | 51,595 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | datafog-core | 110,273 | 5,992 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | datafog-regex | 105,885 | 6,919 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | datafog-spacy | 33,270 | 246,970 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | datafog-gliner | 33,349 | 49,434 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | scrubadub-base | 110,166 | 1,525 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | scrubadub-spacy | 89,546 | 15,310 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | gliner | 26,515 | 53,355 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | gliner-high-recall | 22,699 | 69,716 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | C | opf | 25,485 | 31,383 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v1 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | not measured under a quiet machine |
| v1 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | not measured under a quiet machine |
| v1 | D | presidio-strong | 0 | 6,190 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | presidio-strong-high-recall | 0 | 6,208 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | datafog-core | 0 | 4,059 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | datafog-regex | 0 | 3,051 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | datafog-spacy | 0 | 5,939 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | datafog-gliner | 0 | 3,653 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | scrubadub-base | 0 | 156 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | scrubadub-spacy | 0 | 1,297 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | gliner-high-recall | 0 | 3,264 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | D | opf | 0 | 5,589 | 840 | 0 | 0 | not measured under a quiet machine |
| v1 | R | gaze | 234 | 419 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | not measured under a quiet machine |
| v1 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | presidio-strong | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | presidio-strong-high-recall | 581 | 1,165 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | datafog-core | 2,931 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | datafog-regex | 2,238 | 0 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | datafog-spacy | 585 | 674 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | datafog-gliner | 665 | 310 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | scrubadub-base | 2,992 | 38 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | scrubadub-spacy | 1,896 | 193 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | gliner | 478 | 432 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | gliner-high-recall | 244 | 1,139 | 100 | 0 | 0 | not measured under a quiet machine |
| v1 | R | opf | 182 | 956 | 100 | 0 | 0 | not measured under a quiet machine |

## Heldout safety and entity metrics

The validation/test split is fixed by document ID. Thresholds are selected on validation; the table below reports the disjoint test half. Product coverage scores unsupported gold as missed. Common intersection scores only classes claimed by every listed configuration. Entity scores require an exact UTF-8 byte span and a compatible reviewed label mapping.

Common classes: CREDITCARDNUMBER, DATEOFBIRTH, EMAIL, PHONENUMBER, ZIP.

| Contract | Layer | View | Tool | PII docs | Leaking docs | Doc leak % | Leaking entities | Entity leak % | Redaction load % | TP | FP | FN | Entity P | Entity R | F1 | F2 |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| v3 | A | product_coverage | gaze | 1,119 | 205 | 18.3 | 234 | 17.1 | 27.3 | 558 | 653 | 814 | 0.461 | 0.407 | 0.432 | 0.416 |
| v3 | A | product_coverage | presidio-all | 1,119 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v3 | A | product_coverage | presidio-en | 1,119 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v3 | A | product_coverage | presidio-en-de | 1,119 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v3 | A | product_coverage | presidio-strong | 1,119 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v3 | A | product_coverage | presidio-strong-high-recall | 1,119 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v3 | A | product_coverage | datafog-core | 1,119 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v3 | A | product_coverage | datafog-regex | 1,119 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v3 | A | product_coverage | datafog-spacy | 1,119 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v3 | A | product_coverage | datafog-gliner | 1,119 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v3 | A | product_coverage | scrubadub-base | 1,119 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v3 | A | product_coverage | scrubadub-spacy | 1,119 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v3 | A | product_coverage | gliner | 1,119 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v3 | A | product_coverage | gliner-high-recall | 1,119 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v3 | A | product_coverage | opf | 1,119 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v3 | A | common_intersection | gaze | 166 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
| v3 | A | common_intersection | presidio-all | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v3 | A | common_intersection | presidio-en | 166 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v3 | A | common_intersection | presidio-en-de | 166 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v3 | A | common_intersection | presidio-strong | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v3 | A | common_intersection | presidio-strong-high-recall | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v3 | A | common_intersection | datafog-core | 166 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v3 | A | common_intersection | datafog-regex | 166 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v3 | A | common_intersection | datafog-spacy | 166 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v3 | A | common_intersection | datafog-gliner | 166 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v3 | A | common_intersection | scrubadub-base | 166 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v3 | A | common_intersection | scrubadub-spacy | 166 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v3 | A | common_intersection | gliner | 166 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v3 | A | common_intersection | gliner-high-recall | 166 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v3 | A | common_intersection | opf | 166 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v3 | C | product_coverage | gaze | 992 | 524 | 52.8 | 693 | 9.2 | 15.0 | 4,986 | 2,846 | 2,521 | 0.637 | 0.664 | 0.650 | 0.658 |
| v3 | C | product_coverage | presidio-all | 992 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,859 | 4,350 | 4,648 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | C | product_coverage | presidio-en | 992 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,615 | 2,382 | 5,892 | 0.404 | 0.215 | 0.281 | 0.237 |
| v3 | C | product_coverage | presidio-en-de | 992 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,861 | 4,348 | 4,646 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | C | product_coverage | presidio-strong | 992 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,213 | 3,862 | 4,294 | 0.454 | 0.428 | 0.441 | 0.433 |
| v3 | C | product_coverage | presidio-strong-high-recall | 992 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,218 | 4,007 | 4,289 | 0.445 | 0.429 | 0.437 | 0.432 |
| v3 | C | product_coverage | datafog-core | 992 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v3 | C | product_coverage | datafog-regex | 992 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v3 | C | product_coverage | datafog-spacy | 992 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v3 | C | product_coverage | datafog-gliner | 992 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v3 | C | product_coverage | scrubadub-base | 992 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v3 | C | product_coverage | scrubadub-spacy | 992 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v3 | C | product_coverage | gliner | 992 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v3 | C | product_coverage | gliner-high-recall | 992 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
| v3 | C | product_coverage | opf | 992 | 649 | 65.4 | 1,368 | 18.2 | 13.9 | 1,369 | 3,260 | 6,138 | 0.296 | 0.182 | 0.226 | 0.198 |
| v3 | C | common_intersection | gaze | 781 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
| v3 | C | common_intersection | presidio-all | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v3 | C | common_intersection | presidio-en | 781 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v3 | C | common_intersection | presidio-en-de | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v3 | C | common_intersection | presidio-strong | 781 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v3 | C | common_intersection | presidio-strong-high-recall | 781 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v3 | C | common_intersection | datafog-core | 781 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v3 | C | common_intersection | datafog-regex | 781 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v3 | C | common_intersection | datafog-spacy | 781 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v3 | C | common_intersection | datafog-gliner | 781 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v3 | C | common_intersection | scrubadub-base | 781 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v3 | C | common_intersection | scrubadub-spacy | 781 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v3 | C | common_intersection | gliner | 781 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v3 | C | common_intersection | gliner-high-recall | 781 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v3 | C | common_intersection | opf | 781 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v3 | D | product_coverage | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | product_coverage | opf | 0 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | D | common_intersection | opf | 0 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | R | product_coverage | gaze | 53 | 3 | 5.7 | 3 | 1.3 | 36.2 | 74 | 110 | 165 | 0.402 | 0.310 | 0.350 | 0.325 |
| v3 | R | product_coverage | presidio-all | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v3 | R | product_coverage | presidio-en | 53 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v3 | R | product_coverage | presidio-en-de | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v3 | R | product_coverage | presidio-strong | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v3 | R | product_coverage | presidio-strong-high-recall | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v3 | R | product_coverage | datafog-core | 53 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v3 | R | product_coverage | datafog-regex | 53 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v3 | R | product_coverage | datafog-spacy | 53 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v3 | R | product_coverage | datafog-gliner | 53 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v3 | R | product_coverage | scrubadub-base | 53 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v3 | R | product_coverage | scrubadub-spacy | 53 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v3 | R | product_coverage | gliner | 53 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v3 | R | product_coverage | gliner-high-recall | 53 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v3 | R | product_coverage | opf | 53 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v3 | R | common_intersection | gaze | 40 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
| v3 | R | common_intersection | presidio-all | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v3 | R | common_intersection | presidio-en | 40 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v3 | R | common_intersection | presidio-en-de | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v3 | R | common_intersection | presidio-strong | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v3 | R | common_intersection | presidio-strong-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v3 | R | common_intersection | datafog-core | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | R | common_intersection | datafog-regex | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | R | common_intersection | datafog-spacy | 40 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v3 | R | common_intersection | datafog-gliner | 40 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v3 | R | common_intersection | scrubadub-base | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | R | common_intersection | scrubadub-spacy | 40 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v3 | R | common_intersection | gliner | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | R | common_intersection | gliner-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | R | common_intersection | opf | 40 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |
| v2 | A | product_coverage | gaze | 1,119 | 205 | 18.3 | 234 | 17.1 | 27.3 | 558 | 653 | 814 | 0.461 | 0.407 | 0.432 | 0.416 |
| v2 | A | product_coverage | presidio-all | 1,119 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v2 | A | product_coverage | presidio-en | 1,119 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v2 | A | product_coverage | presidio-en-de | 1,119 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v2 | A | product_coverage | presidio-strong | 1,119 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v2 | A | product_coverage | presidio-strong-high-recall | 1,119 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v2 | A | product_coverage | datafog-core | 1,119 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v2 | A | product_coverage | datafog-regex | 1,119 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v2 | A | product_coverage | datafog-spacy | 1,119 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v2 | A | product_coverage | datafog-gliner | 1,119 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v2 | A | product_coverage | scrubadub-base | 1,119 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v2 | A | product_coverage | scrubadub-spacy | 1,119 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v2 | A | product_coverage | gliner | 1,119 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v2 | A | product_coverage | gliner-high-recall | 1,119 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v2 | A | product_coverage | opf | 1,119 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v2 | A | common_intersection | gaze | 166 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
| v2 | A | common_intersection | presidio-all | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v2 | A | common_intersection | presidio-en | 166 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v2 | A | common_intersection | presidio-en-de | 166 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v2 | A | common_intersection | presidio-strong | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v2 | A | common_intersection | presidio-strong-high-recall | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v2 | A | common_intersection | datafog-core | 166 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v2 | A | common_intersection | datafog-regex | 166 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v2 | A | common_intersection | datafog-spacy | 166 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v2 | A | common_intersection | datafog-gliner | 166 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v2 | A | common_intersection | scrubadub-base | 166 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v2 | A | common_intersection | scrubadub-spacy | 166 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v2 | A | common_intersection | gliner | 166 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v2 | A | common_intersection | gliner-high-recall | 166 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v2 | A | common_intersection | opf | 166 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v2 | C | product_coverage | gaze | 992 | 524 | 52.8 | 693 | 9.2 | 15.0 | 4,986 | 2,846 | 2,521 | 0.637 | 0.664 | 0.650 | 0.658 |
| v2 | C | product_coverage | presidio-all | 992 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,859 | 4,350 | 4,648 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | C | product_coverage | presidio-en | 992 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,615 | 2,382 | 5,892 | 0.404 | 0.215 | 0.281 | 0.237 |
| v2 | C | product_coverage | presidio-en-de | 992 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,861 | 4,348 | 4,646 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | C | product_coverage | presidio-strong | 992 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,213 | 3,862 | 4,294 | 0.454 | 0.428 | 0.441 | 0.433 |
| v2 | C | product_coverage | presidio-strong-high-recall | 992 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,218 | 4,007 | 4,289 | 0.445 | 0.429 | 0.437 | 0.432 |
| v2 | C | product_coverage | datafog-core | 992 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v2 | C | product_coverage | datafog-regex | 992 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v2 | C | product_coverage | datafog-spacy | 992 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v2 | C | product_coverage | datafog-gliner | 992 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v2 | C | product_coverage | scrubadub-base | 992 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v2 | C | product_coverage | scrubadub-spacy | 992 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v2 | C | product_coverage | gliner | 992 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v2 | C | product_coverage | gliner-high-recall | 992 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
| v2 | C | product_coverage | opf | 992 | 649 | 65.4 | 1,368 | 18.2 | 13.9 | 1,369 | 3,260 | 6,138 | 0.296 | 0.182 | 0.226 | 0.198 |
| v2 | C | common_intersection | gaze | 781 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
| v2 | C | common_intersection | presidio-all | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v2 | C | common_intersection | presidio-en | 781 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v2 | C | common_intersection | presidio-en-de | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v2 | C | common_intersection | presidio-strong | 781 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v2 | C | common_intersection | presidio-strong-high-recall | 781 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v2 | C | common_intersection | datafog-core | 781 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v2 | C | common_intersection | datafog-regex | 781 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v2 | C | common_intersection | datafog-spacy | 781 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v2 | C | common_intersection | datafog-gliner | 781 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v2 | C | common_intersection | scrubadub-base | 781 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v2 | C | common_intersection | scrubadub-spacy | 781 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v2 | C | common_intersection | gliner | 781 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v2 | C | common_intersection | gliner-high-recall | 781 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v2 | C | common_intersection | opf | 781 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v2 | D | product_coverage | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | product_coverage | opf | 0 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | D | common_intersection | opf | 0 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | R | product_coverage | gaze | 53 | 3 | 5.7 | 3 | 1.3 | 36.2 | 74 | 110 | 165 | 0.402 | 0.310 | 0.350 | 0.325 |
| v2 | R | product_coverage | presidio-all | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v2 | R | product_coverage | presidio-en | 53 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v2 | R | product_coverage | presidio-en-de | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v2 | R | product_coverage | presidio-strong | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v2 | R | product_coverage | presidio-strong-high-recall | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v2 | R | product_coverage | datafog-core | 53 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v2 | R | product_coverage | datafog-regex | 53 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v2 | R | product_coverage | datafog-spacy | 53 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v2 | R | product_coverage | datafog-gliner | 53 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v2 | R | product_coverage | scrubadub-base | 53 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v2 | R | product_coverage | scrubadub-spacy | 53 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v2 | R | product_coverage | gliner | 53 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v2 | R | product_coverage | gliner-high-recall | 53 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v2 | R | product_coverage | opf | 53 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v2 | R | common_intersection | gaze | 40 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
| v2 | R | common_intersection | presidio-all | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v2 | R | common_intersection | presidio-en | 40 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v2 | R | common_intersection | presidio-en-de | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v2 | R | common_intersection | presidio-strong | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v2 | R | common_intersection | presidio-strong-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v2 | R | common_intersection | datafog-core | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | R | common_intersection | datafog-regex | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | R | common_intersection | datafog-spacy | 40 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v2 | R | common_intersection | datafog-gliner | 40 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v2 | R | common_intersection | scrubadub-base | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | R | common_intersection | scrubadub-spacy | 40 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v2 | R | common_intersection | gliner | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | R | common_intersection | gliner-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | R | common_intersection | opf | 40 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |
| v1 | A | product_coverage | gaze | 1,119 | 205 | 18.3 | 234 | 17.1 | 27.3 | 558 | 653 | 814 | 0.461 | 0.407 | 0.432 | 0.416 |
| v1 | A | product_coverage | presidio-all | 1,119 | 535 | 47.8 | 618 | 45.0 | 24.3 | 271 | 1,161 | 1,101 | 0.189 | 0.198 | 0.193 | 0.196 |
| v1 | A | product_coverage | presidio-en | 1,119 | 791 | 70.7 | 908 | 66.2 | 12.1 | 127 | 574 | 1,245 | 0.181 | 0.093 | 0.123 | 0.103 |
| v1 | A | product_coverage | presidio-en-de | 1,119 | 649 | 58.0 | 732 | 53.4 | 19.3 | 219 | 900 | 1,153 | 0.196 | 0.160 | 0.176 | 0.166 |
| v1 | A | product_coverage | presidio-strong | 1,119 | 541 | 48.3 | 624 | 45.5 | 22.9 | 279 | 1,042 | 1,093 | 0.211 | 0.203 | 0.207 | 0.205 |
| v1 | A | product_coverage | presidio-strong-high-recall | 1,119 | 541 | 48.3 | 624 | 45.5 | 23.1 | 279 | 1,066 | 1,093 | 0.207 | 0.203 | 0.205 | 0.204 |
| v1 | A | product_coverage | datafog-core | 1,119 | 674 | 60.2 | 758 | 55.2 | 12.3 | 124 | 599 | 1,248 | 0.172 | 0.090 | 0.118 | 0.100 |
| v1 | A | product_coverage | datafog-regex | 1,119 | 543 | 48.5 | 677 | 49.3 | 16.6 | 318 | 474 | 1,054 | 0.402 | 0.232 | 0.294 | 0.253 |
| v1 | A | product_coverage | datafog-spacy | 1,119 | 479 | 42.8 | 565 | 41.2 | 24.9 | 329 | 1,282 | 1,043 | 0.204 | 0.240 | 0.221 | 0.232 |
| v1 | A | product_coverage | datafog-gliner | 1,119 | 479 | 42.8 | 572 | 41.7 | 18.6 | 328 | 610 | 1,044 | 0.350 | 0.239 | 0.284 | 0.255 |
| v1 | A | product_coverage | scrubadub-base | 1,119 | 964 | 86.1 | 1,167 | 85.1 | 4.8 | 45 | 167 | 1,327 | 0.212 | 0.033 | 0.057 | 0.039 |
| v1 | A | product_coverage | scrubadub-spacy | 1,119 | 947 | 84.6 | 1,121 | 81.7 | 6.8 | 39 | 338 | 1,333 | 0.103 | 0.028 | 0.045 | 0.033 |
| v1 | A | product_coverage | gliner | 1,119 | 372 | 33.2 | 522 | 38.0 | 26.3 | 507 | 521 | 865 | 0.493 | 0.370 | 0.422 | 0.389 |
| v1 | A | product_coverage | gliner-high-recall | 1,119 | 214 | 19.1 | 330 | 24.1 | 31.9 | 521 | 920 | 851 | 0.362 | 0.380 | 0.370 | 0.376 |
| v1 | A | product_coverage | opf | 1,119 | 304 | 27.2 | 396 | 28.9 | 26.9 | 437 | 605 | 935 | 0.419 | 0.319 | 0.362 | 0.335 |
| v1 | A | common_intersection | gaze | 166 | 13 | 7.8 | 13 | 7.3 | 5.6 | 163 | 11 | 15 | 0.937 | 0.916 | 0.926 | 0.920 |
| v1 | A | common_intersection | presidio-all | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 125 | 235 | 53 | 0.347 | 0.702 | 0.465 | 0.583 |
| v1 | A | common_intersection | presidio-en | 166 | 62 | 37.3 | 62 | 34.8 | 5.5 | 101 | 79 | 77 | 0.561 | 0.567 | 0.564 | 0.566 |
| v1 | A | common_intersection | presidio-en-de | 166 | 38 | 22.9 | 38 | 21.3 | 8.1 | 125 | 173 | 53 | 0.419 | 0.702 | 0.525 | 0.619 |
| v1 | A | common_intersection | presidio-strong | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v1 | A | common_intersection | presidio-strong-high-recall | 166 | 38 | 22.9 | 38 | 21.3 | 9.3 | 134 | 219 | 44 | 0.380 | 0.753 | 0.505 | 0.629 |
| v1 | A | common_intersection | datafog-core | 166 | 48 | 28.9 | 48 | 27.0 | 5.6 | 124 | 73 | 54 | 0.629 | 0.697 | 0.661 | 0.682 |
| v1 | A | common_intersection | datafog-regex | 166 | 10 | 6.0 | 10 | 5.6 | 6.8 | 168 | 67 | 10 | 0.715 | 0.944 | 0.814 | 0.887 |
| v1 | A | common_intersection | datafog-spacy | 166 | 4 | 2.4 | 4 | 2.2 | 9.6 | 174 | 192 | 4 | 0.475 | 0.978 | 0.640 | 0.807 |
| v1 | A | common_intersection | datafog-gliner | 166 | 0 | 0.0 | 0 | 0.0 | 7.4 | 178 | 103 | 0 | 0.633 | 1.000 | 0.776 | 0.896 |
| v1 | A | common_intersection | scrubadub-base | 166 | 114 | 68.7 | 126 | 70.8 | 2.5 | 45 | 7 | 133 | 0.865 | 0.253 | 0.391 | 0.295 |
| v1 | A | common_intersection | scrubadub-spacy | 166 | 121 | 72.9 | 133 | 74.7 | 2.1 | 38 | 7 | 140 | 0.844 | 0.213 | 0.341 | 0.251 |
| v1 | A | common_intersection | gliner | 166 | 9 | 5.4 | 9 | 5.1 | 5.8 | 150 | 27 | 28 | 0.847 | 0.843 | 0.845 | 0.844 |
| v1 | A | common_intersection | gliner-high-recall | 166 | 5 | 3.0 | 5 | 2.8 | 6.0 | 154 | 42 | 24 | 0.786 | 0.865 | 0.824 | 0.848 |
| v1 | A | common_intersection | opf | 166 | 14 | 8.4 | 14 | 7.9 | 7.7 | 162 | 163 | 16 | 0.498 | 0.910 | 0.644 | 0.781 |
| v1 | C | product_coverage | gaze | 992 | 633 | 63.8 | 907 | 11.7 | 15.0 | 4,986 | 2,870 | 2,745 | 0.635 | 0.645 | 0.640 | 0.643 |
| v1 | C | product_coverage | presidio-all | 992 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,859 | 4,429 | 4,872 | 0.392 | 0.370 | 0.381 | 0.374 |
| v1 | C | product_coverage | presidio-en | 992 | 872 | 87.9 | 4,792 | 62.0 | 8.8 | 1,615 | 2,442 | 6,116 | 0.398 | 0.209 | 0.274 | 0.231 |
| v1 | C | product_coverage | presidio-en-de | 992 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,861 | 4,427 | 4,870 | 0.393 | 0.370 | 0.381 | 0.374 |
| v1 | C | product_coverage | presidio-strong | 992 | 890 | 89.7 | 2,284 | 29.5 | 15.7 | 3,213 | 3,901 | 4,518 | 0.452 | 0.416 | 0.433 | 0.422 |
| v1 | C | product_coverage | presidio-strong-high-recall | 992 | 886 | 89.3 | 2,265 | 29.3 | 15.9 | 3,218 | 4,046 | 4,513 | 0.443 | 0.416 | 0.429 | 0.421 |
| v1 | C | product_coverage | datafog-core | 992 | 992 | 100.0 | 6,977 | 90.2 | 2.7 | 671 | 408 | 7,060 | 0.622 | 0.087 | 0.152 | 0.105 |
| v1 | C | product_coverage | datafog-regex | 992 | 992 | 100.0 | 6,865 | 88.8 | 3.2 | 744 | 528 | 6,987 | 0.585 | 0.096 | 0.165 | 0.116 |
| v1 | C | product_coverage | datafog-spacy | 992 | 883 | 89.0 | 2,135 | 27.6 | 36.8 | 2,663 | 9,833 | 5,068 | 0.213 | 0.344 | 0.263 | 0.307 |
| v1 | C | product_coverage | datafog-gliner | 992 | 819 | 82.6 | 1,642 | 21.2 | 15.2 | 1,727 | 3,880 | 6,004 | 0.308 | 0.223 | 0.259 | 0.236 |
| v1 | C | product_coverage | scrubadub-base | 992 | 992 | 100.0 | 7,213 | 93.3 | 2.3 | 440 | 115 | 7,291 | 0.793 | 0.057 | 0.106 | 0.070 |
| v1 | C | product_coverage | scrubadub-spacy | 992 | 966 | 97.4 | 5,809 | 75.1 | 5.8 | 819 | 1,387 | 6,912 | 0.371 | 0.106 | 0.165 | 0.124 |
| v1 | C | product_coverage | gliner | 992 | 660 | 66.5 | 1,327 | 17.2 | 16.4 | 2,014 | 3,517 | 5,717 | 0.364 | 0.261 | 0.304 | 0.276 |
| v1 | C | product_coverage | gliner-high-recall | 992 | 613 | 61.8 | 1,146 | 14.8 | 18.4 | 2,040 | 4,218 | 5,691 | 0.326 | 0.264 | 0.292 | 0.274 |
| v1 | C | product_coverage | opf | 992 | 667 | 67.2 | 1,417 | 18.3 | 14.4 | 1,369 | 3,439 | 6,362 | 0.285 | 0.177 | 0.218 | 0.192 |
| v1 | C | common_intersection | gaze | 781 | 146 | 18.7 | 153 | 13.1 | 4.0 | 990 | 911 | 176 | 0.521 | 0.849 | 0.646 | 0.754 |
| v1 | C | common_intersection | presidio-all | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v1 | C | common_intersection | presidio-en | 781 | 634 | 81.2 | 802 | 68.8 | 2.7 | 325 | 716 | 841 | 0.312 | 0.279 | 0.295 | 0.285 |
| v1 | C | common_intersection | presidio-en-de | 781 | 470 | 60.2 | 523 | 44.9 | 4.9 | 597 | 1,471 | 569 | 0.289 | 0.512 | 0.369 | 0.443 |
| v1 | C | common_intersection | presidio-strong | 781 | 503 | 64.4 | 567 | 48.6 | 4.3 | 583 | 1,225 | 583 | 0.322 | 0.500 | 0.392 | 0.450 |
| v1 | C | common_intersection | presidio-strong-high-recall | 781 | 499 | 63.9 | 563 | 48.3 | 4.3 | 587 | 1,264 | 579 | 0.317 | 0.503 | 0.389 | 0.450 |
| v1 | C | common_intersection | datafog-core | 781 | 474 | 60.7 | 559 | 47.9 | 2.2 | 602 | 163 | 564 | 0.787 | 0.516 | 0.624 | 0.554 |
| v1 | C | common_intersection | datafog-regex | 781 | 454 | 58.1 | 530 | 45.5 | 2.6 | 631 | 246 | 535 | 0.719 | 0.541 | 0.618 | 0.569 |
| v1 | C | common_intersection | datafog-spacy | 781 | 391 | 50.1 | 442 | 37.9 | 7.0 | 661 | 2,323 | 505 | 0.222 | 0.567 | 0.319 | 0.432 |
| v1 | C | common_intersection | datafog-gliner | 781 | 138 | 17.7 | 147 | 12.6 | 5.3 | 751 | 1,657 | 415 | 0.312 | 0.644 | 0.420 | 0.531 |
| v1 | C | common_intersection | scrubadub-base | 781 | 663 | 84.9 | 836 | 71.7 | 1.6 | 322 | 53 | 844 | 0.859 | 0.276 | 0.418 | 0.320 |
| v1 | C | common_intersection | scrubadub-spacy | 781 | 667 | 85.4 | 844 | 72.4 | 1.5 | 314 | 43 | 852 | 0.880 | 0.269 | 0.412 | 0.313 |
| v1 | C | common_intersection | gliner | 781 | 88 | 11.3 | 104 | 8.9 | 4.3 | 664 | 1,115 | 502 | 0.373 | 0.569 | 0.451 | 0.515 |
| v1 | C | common_intersection | gliner-high-recall | 781 | 65 | 8.3 | 78 | 6.7 | 4.7 | 676 | 1,244 | 490 | 0.352 | 0.580 | 0.438 | 0.513 |
| v1 | C | common_intersection | opf | 781 | 56 | 7.2 | 56 | 4.8 | 5.1 | 704 | 1,170 | 462 | 0.376 | 0.604 | 0.463 | 0.538 |
| v1 | D | product_coverage | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 178 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 15.7 | 0 | 357 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 12.4 | 0 | 258 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 15.3 | 0 | 347 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 287 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 12.1 | 0 | 289 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 7.9 | 0 | 196 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 6.2 | 0 | 156 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 12.2 | 0 | 303 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 7.5 | 0 | 180 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 2.7 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 54 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 6.0 | 0 | 120 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | product_coverage | opf | 0 | 0 | 0.0 | 0 | 0.0 | 11.1 | 0 | 176 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | gaze | 0 | 0 | 0.0 | 0 | 0.0 | 2.1 | 0 | 63 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | presidio-all | 0 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 148 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | presidio-en | 0 | 0 | 0.0 | 0 | 0.0 | 4.5 | 0 | 92 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | presidio-en-de | 0 | 0 | 0.0 | 0 | 0.0 | 6.6 | 0 | 141 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | presidio-strong | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | presidio-strong-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 5.2 | 0 | 109 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | datafog-core | 0 | 0 | 0.0 | 0 | 0.0 | 2.9 | 0 | 74 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | datafog-regex | 0 | 0 | 0.0 | 0 | 0.0 | 3.2 | 0 | 78 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | datafog-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 6.4 | 0 | 168 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | datafog-gliner | 0 | 0 | 0.0 | 0 | 0.0 | 3.6 | 0 | 88 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | scrubadub-base | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | scrubadub-spacy | 0 | 0 | 0.0 | 0 | 0.0 | 0.3 | 0 | 6 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | gliner | 0 | 0 | 0.0 | 0 | 0.0 | 0.1 | 0 | 1 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | gliner-high-recall | 0 | 0 | 0.0 | 0 | 0.0 | 1.8 | 0 | 33 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | D | common_intersection | opf | 0 | 0 | 0.0 | 0 | 0.0 | 8.2 | 0 | 146 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | R | product_coverage | gaze | 53 | 3 | 5.7 | 3 | 1.3 | 36.2 | 74 | 110 | 165 | 0.402 | 0.310 | 0.350 | 0.325 |
| v1 | R | product_coverage | presidio-all | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v1 | R | product_coverage | presidio-en | 53 | 30 | 56.6 | 120 | 50.2 | 15.6 | 28 | 67 | 211 | 0.295 | 0.117 | 0.168 | 0.133 |
| v1 | R | product_coverage | presidio-en-de | 53 | 13 | 24.5 | 21 | 8.8 | 39.8 | 61 | 165 | 178 | 0.270 | 0.255 | 0.262 | 0.258 |
| v1 | R | product_coverage | presidio-strong | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v1 | R | product_coverage | presidio-strong-high-recall | 53 | 21 | 39.6 | 39 | 16.3 | 38.1 | 67 | 153 | 172 | 0.305 | 0.280 | 0.292 | 0.285 |
| v1 | R | product_coverage | datafog-core | 53 | 46 | 86.8 | 178 | 74.5 | 16.1 | 40 | 25 | 199 | 0.615 | 0.167 | 0.263 | 0.196 |
| v1 | R | product_coverage | datafog-regex | 53 | 41 | 77.4 | 163 | 68.2 | 20.2 | 55 | 21 | 184 | 0.724 | 0.230 | 0.349 | 0.266 |
| v1 | R | product_coverage | datafog-spacy | 53 | 21 | 39.6 | 27 | 11.3 | 36.4 | 59 | 159 | 180 | 0.271 | 0.247 | 0.258 | 0.251 |
| v1 | R | product_coverage | datafog-gliner | 53 | 18 | 34.0 | 33 | 13.8 | 33.7 | 55 | 101 | 184 | 0.353 | 0.230 | 0.278 | 0.247 |
| v1 | R | product_coverage | scrubadub-base | 53 | 46 | 86.8 | 178 | 74.5 | 15.9 | 40 | 21 | 199 | 0.656 | 0.167 | 0.267 | 0.197 |
| v1 | R | product_coverage | scrubadub-spacy | 53 | 25 | 47.2 | 86 | 36.0 | 24.1 | 43 | 80 | 196 | 0.350 | 0.180 | 0.238 | 0.199 |
| v1 | R | product_coverage | gliner | 53 | 20 | 37.7 | 27 | 11.3 | 33.3 | 53 | 103 | 186 | 0.340 | 0.222 | 0.268 | 0.238 |
| v1 | R | product_coverage | gliner-high-recall | 53 | 16 | 30.2 | 17 | 7.1 | 40.1 | 56 | 138 | 183 | 0.289 | 0.234 | 0.259 | 0.243 |
| v1 | R | product_coverage | opf | 53 | 6 | 11.3 | 9 | 3.8 | 38.5 | 58 | 135 | 181 | 0.301 | 0.243 | 0.269 | 0.252 |
| v1 | R | common_intersection | gaze | 40 | 0 | 0.0 | 0 | 0.0 | 17.2 | 40 | 9 | 0 | 0.816 | 1.000 | 0.899 | 0.957 |
| v1 | R | common_intersection | presidio-all | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v1 | R | common_intersection | presidio-en | 40 | 16 | 40.0 | 16 | 40.0 | 9.2 | 24 | 6 | 16 | 0.800 | 0.600 | 0.686 | 0.632 |
| v1 | R | common_intersection | presidio-en-de | 40 | 0 | 0.0 | 0 | 0.0 | 20.1 | 40 | 20 | 0 | 0.667 | 1.000 | 0.800 | 0.909 |
| v1 | R | common_intersection | presidio-strong | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v1 | R | common_intersection | presidio-strong-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 19.6 | 40 | 14 | 0 | 0.741 | 1.000 | 0.851 | 0.935 |
| v1 | R | common_intersection | datafog-core | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | R | common_intersection | datafog-regex | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | R | common_intersection | datafog-spacy | 40 | 0 | 0.0 | 0 | 0.0 | 18.1 | 40 | 26 | 0 | 0.606 | 1.000 | 0.755 | 0.885 |
| v1 | R | common_intersection | datafog-gliner | 40 | 0 | 0.0 | 0 | 0.0 | 16.4 | 40 | 6 | 0 | 0.870 | 1.000 | 0.930 | 0.971 |
| v1 | R | common_intersection | scrubadub-base | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | R | common_intersection | scrubadub-spacy | 40 | 1 | 2.5 | 1 | 2.5 | 15.1 | 39 | 0 | 1 | 1.000 | 0.975 | 0.987 | 0.980 |
| v1 | R | common_intersection | gliner | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | R | common_intersection | gliner-high-recall | 40 | 0 | 0.0 | 0 | 0.0 | 15.5 | 40 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | R | common_intersection | opf | 40 | 0 | 0.0 | 0 | 0.0 | 20.6 | 40 | 27 | 0 | 0.597 | 1.000 | 0.748 | 0.881 |

**Where Gaze trails under the shared v3 contract, reviewed mapping, and byte scorer:** On layer D's fixed test half, scrubadub 2.0.0 built-ins leaked 0 B and added 62 false-positive B; Gaze full leaked 0 B and added 1,634 false-positive B. On the full layer R aggregate (a diagnostic spanning both halves), OPF 0.1.0 default Viterbi leaked 182 B versus Gaze full's 234 B.

## Gaze ablations

Rules only, rules plus NER, and full setup use the same test documents and scorer.

| Contract | Layer | Gaze configuration | Leaked B | FP B | PII docs | Leaking docs | Entity F1 | Entity F2 |
|---|---|---|---:|---:|---:|---:|---:|---:|
| v3 | A | rules-only | 3,274 | 36 | 1,119 | 229 | 0.449 | 0.421 |
| v3 | A | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.434 | 0.415 |
| v3 | A | full | 2,861 | 421 | 1,119 | 205 | 0.432 | 0.416 |
| v3 | C | rules-only | 42,430 | 1,853 | 992 | 990 | 0.334 | 0.248 |
| v3 | C | rules-ner | 6,266 | 13,805 | 992 | 621 | 0.638 | 0.639 |
| v3 | C | full | 4,806 | 13,997 | 992 | 524 | 0.650 | 0.658 |
| v3 | D | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v3 | D | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v3 | D | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v3 | R | rules-only | 89 | 76 | 53 | 12 | 0.319 | 0.286 |
| v3 | R | rules-ner | 20 | 89 | 53 | 3 | 0.357 | 0.327 |
| v3 | R | full | 20 | 193 | 53 | 3 | 0.350 | 0.325 |
| v2 | A | rules-only | 3,274 | 36 | 1,119 | 229 | 0.449 | 0.421 |
| v2 | A | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.434 | 0.415 |
| v2 | A | full | 2,861 | 421 | 1,119 | 205 | 0.432 | 0.416 |
| v2 | C | rules-only | 42,430 | 1,853 | 992 | 990 | 0.334 | 0.248 |
| v2 | C | rules-ner | 6,266 | 13,805 | 992 | 621 | 0.638 | 0.639 |
| v2 | C | full | 4,806 | 13,997 | 992 | 524 | 0.650 | 0.658 |
| v2 | D | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v2 | D | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v2 | D | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v2 | R | rules-only | 89 | 76 | 53 | 12 | 0.319 | 0.286 |
| v2 | R | rules-ner | 20 | 89 | 53 | 3 | 0.357 | 0.327 |
| v2 | R | full | 20 | 193 | 53 | 3 | 0.350 | 0.325 |
| v1 | A | rules-only | 3,274 | 36 | 1,119 | 229 | 0.449 | 0.421 |
| v1 | A | rules-ner | 2,968 | 365 | 1,119 | 209 | 0.434 | 0.415 |
| v1 | A | full | 2,861 | 421 | 1,119 | 205 | 0.432 | 0.416 |
| v1 | C | rules-only | 45,838 | 1,853 | 992 | 990 | 0.326 | 0.241 |
| v1 | C | rules-ner | 9,628 | 13,805 | 992 | 710 | 0.629 | 0.624 |
| v1 | C | full | 7,937 | 13,997 | 992 | 633 | 0.640 | 0.643 |
| v1 | D | rules-only | 0 | 1,116 | 0 | 0 | 0.000 | 0.000 |
| v1 | D | rules-ner | 0 | 1,163 | 0 | 0 | 0.000 | 0.000 |
| v1 | D | full | 0 | 1,634 | 0 | 0 | 0.000 | 0.000 |
| v1 | R | rules-only | 89 | 76 | 53 | 12 | 0.319 | 0.286 |
| v1 | R | rules-ner | 20 | 89 | 53 | 3 | 0.357 | 0.327 |
| v1 | R | full | 20 | 193 | 53 | 3 | 0.350 | 0.325 |

Threshold choice uses validation only: gliner → gliner-high-recall; presidio-strong → presidio-strong-high-recall.
