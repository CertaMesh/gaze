# Competitor comparison

Same corpus and scorer; tools run with documented configurations. UTF-8 byte counts use the Gaze scorer. For v4 and v3, FP is the scorer's false-positive count after its audited gold-gap credit. Presidio all runs English, German, Dutch, French, and Portuguese spaCy models with the documented German recognizers. Presidio English default is a secondary row. Latency was not measured under a quiet machine; timing comparisons are withheld. This measures detection; competitor restore and manifest behavior is not scored. The agentic-layer rows were measured on generator v20 (test corpus `ca2932826893…`, 14,870 documents). The current generator is v30; its added documents are not measured here.

Leaked and false-positive byte counts are class-agnostic. A skipped document's scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to get leakage on processed documents. For example, Presidio English-only v4 Synthetic identifiers in agentic formats leaks 93,458 B, including 47,715 B of scored gold from 2,545 skipped non-English documents. The reviewed label map controls repeated-gold credit and the exact typed-span metrics below.

Gaze is not listed: the comparison run measured an unreleased build, and this page shows tagged Gaze releases only. Released Gaze numbers are in the [release history](README.md#release-history) and the benchmark panels. Home-normalized setup policy SHA-256: `3bc3d0451640558e249638690c9825f6b9d8143ae595fc0cab525871750366fc`.

Fresh v0.17.0 external byte-micro scores are in the [external release panels](README.md#v017-final-candidate-evidence). They measure different datasets and do not refresh the comparator captures on this page. Only the two scrubadub rows were freshly measured for the v0.17 release; other comparator and tuned Presidio measurements keep their original identities.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`. GLiNER uses model-card labels, library threshold 0.5. OpenAI Privacy Filter (OPF) unavailable after two 600-second socket timeouts.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published. Mixed-source publication: only scrubadub-base and scrubadub-spacy were freshly measured with native Spanish support. The other eleven configurations, Gaze measurements, threshold choices and OPF unavailable receipt retain their original identities. Base snapshot: [`comparison-sources/original.json`](comparison-sources/original.json), SHA-256 `119e24993cc47ae9b6b53af052b2266a8f83a4903675f72327eb4c419602851d`. Supplement snapshot: [`comparison-sources/spanish.json`](comparison-sources/spanish.json), SHA-256 `d0b476ca86901e545211143358c9b8b04e9990cac86c6de53ec24231356d6a95`. Snapshot source revisions, adapter hashes, collection/replay times and per-row provenance remain authoritative; the row_sources aggregation time records composition only. Timing comparisons remain withheld.

| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | Latency |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| v4 | Synthetic identifiers in agentic formats | presidio-all | 75,541 | 60,586 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-en | 93,458 | 51,385 | 4,072 | 2,545 | 47,715 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-en-de | 81,804 | 57,657 | 5,883 | 734 | 14,028 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-strong | 90,045 | 13,789 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 86,274 | 18,703 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-core | 120,666 | 1,913 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-regex | 111,524 | 1,420 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-spacy | 87,945 | 69,841 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | datafog-gliner | 99,475 | 9,365 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | scrubadub-base | 128,648 | 133 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | scrubadub-spacy | 115,176 | 40,448 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | gliner | 87,287 | 13,706 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | gliner-high-recall | 58,495 | 23,105 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic identifiers in agentic formats | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-all | 35,130 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-en | 76,658 | 26,086 | 1,545 | 1,365 | 57,835 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-en-de | 35,130 | 50,429 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 31,055 | 40,898 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 30,517 | 42,768 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-core | 110,273 | 5,839 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-regex | 105,885 | 6,766 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-spacy | 33,270 | 243,401 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | datafog-gliner | 33,349 | 47,142 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | scrubadub-base | 110,166 | 1,449 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | scrubadub-spacy | 89,546 | 11,942 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | gliner | 26,515 | 51,384 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | gliner-high-recall | 22,699 | 67,549 | 2,910 | 0 | 0 | not measured under a quiet machine |
| v4 | Kiji EN/DE holdout and A4 negatives | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v4 | Synthetic benign lookalikes | presidio-all | 0 | 95,716 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-en | 0 | 87,014 | 3,783 | 1,409 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-en-de | 0 | 94,477 | 4,955 | 237 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-strong | 0 | 29,848 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 37,358 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-core | 0 | 5,577 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-regex | 0 | 5,293 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-spacy | 0 | 77,221 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | datafog-gliner | 0 | 17,145 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | scrubadub-base | 0 | 865 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 56,858 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | gliner | 0 | 14,028 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | gliner-high-recall | 0 | 35,363 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v4 | Synthetic benign lookalikes | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v4 | Repeated PII values with decoys | presidio-all | 76,862 | 46,877 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-en | 83,401 | 43,461 | 2,145 | 916 | 26,378 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-en-de | 77,805 | 46,503 | 3,013 | 48 | 1,680 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-strong | 101,076 | 5,147 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | presidio-strong-high-recall | 96,834 | 9,521 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-core | 134,869 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-regex | 134,176 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-spacy | 101,215 | 52,296 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | datafog-gliner | 123,284 | 1,132 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | scrubadub-base | 135,380 | 38 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | scrubadub-spacy | 111,787 | 36,798 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | gliner | 95,530 | 1,486 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | gliner-high-recall | 65,542 | 4,026 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v4 | Repeated PII values with decoys | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v3 | Synthetic identifiers in agentic formats | presidio-all | 75,541 | 60,586 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-en | 93,458 | 51,385 | 4,072 | 2,545 | 47,715 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-en-de | 81,804 | 57,657 | 5,883 | 734 | 14,028 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-strong | 90,045 | 13,789 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 86,274 | 18,703 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-core | 120,666 | 1,913 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-regex | 111,524 | 1,420 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-spacy | 87,945 | 69,841 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | datafog-gliner | 99,475 | 9,365 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | scrubadub-base | 128,648 | 133 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | scrubadub-spacy | 115,176 | 40,448 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | gliner | 87,287 | 13,706 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | gliner-high-recall | 58,495 | 23,105 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic identifiers in agentic formats | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
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
| v3 | Kiji EN/DE holdout and A4 negatives | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v3 | Synthetic benign lookalikes | presidio-all | 0 | 95,716 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-en | 0 | 87,014 | 3,783 | 1,409 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-en-de | 0 | 94,477 | 4,955 | 237 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-strong | 0 | 29,848 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 37,358 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-core | 0 | 5,577 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-regex | 0 | 5,293 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-spacy | 0 | 77,221 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | datafog-gliner | 0 | 17,145 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | scrubadub-base | 0 | 865 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 56,858 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | gliner | 0 | 14,028 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | gliner-high-recall | 0 | 35,363 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v3 | Synthetic benign lookalikes | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v3 | Repeated PII values with decoys | presidio-all | 76,862 | 46,877 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-en | 83,401 | 43,461 | 2,145 | 916 | 26,378 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-en-de | 77,805 | 46,503 | 3,013 | 48 | 1,680 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-strong | 101,076 | 5,147 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | presidio-strong-high-recall | 96,834 | 9,521 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-core | 134,869 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-regex | 134,176 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-spacy | 101,215 | 52,296 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | datafog-gliner | 123,284 | 1,132 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | scrubadub-base | 135,380 | 38 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | scrubadub-spacy | 111,787 | 36,798 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | gliner | 95,530 | 1,486 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | gliner-high-recall | 65,542 | 4,026 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v3 | Repeated PII values with decoys | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v2 | Synthetic identifiers in agentic formats | presidio-all | 75,541 | 60,586 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-en | 93,458 | 51,385 | 4,072 | 2,545 | 47,715 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-en-de | 81,804 | 57,657 | 5,883 | 734 | 14,028 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-strong | 90,045 | 13,789 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 86,274 | 18,703 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-core | 120,666 | 1,913 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-regex | 111,524 | 1,420 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-spacy | 87,945 | 69,841 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | datafog-gliner | 99,475 | 9,365 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | scrubadub-base | 128,648 | 133 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | scrubadub-spacy | 115,176 | 40,448 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | gliner | 87,287 | 13,706 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | gliner-high-recall | 58,495 | 23,105 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic identifiers in agentic formats | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
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
| v2 | Kiji EN/DE holdout and A4 negatives | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v2 | Synthetic benign lookalikes | presidio-all | 0 | 95,716 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-en | 0 | 87,014 | 3,783 | 1,409 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-en-de | 0 | 94,477 | 4,955 | 237 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-strong | 0 | 29,848 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 37,358 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-core | 0 | 5,577 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-regex | 0 | 5,293 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-spacy | 0 | 77,221 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | datafog-gliner | 0 | 17,145 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | scrubadub-base | 0 | 865 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 56,858 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | gliner | 0 | 14,028 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | gliner-high-recall | 0 | 35,363 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v2 | Synthetic benign lookalikes | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v2 | Repeated PII values with decoys | presidio-all | 76,862 | 46,877 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-en | 83,401 | 43,461 | 2,145 | 916 | 26,378 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-en-de | 77,805 | 46,503 | 3,013 | 48 | 1,680 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-strong | 101,076 | 5,147 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | presidio-strong-high-recall | 96,834 | 9,521 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-core | 134,869 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-regex | 134,176 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-spacy | 101,215 | 52,296 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | datafog-gliner | 123,284 | 1,132 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | scrubadub-base | 135,380 | 38 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | scrubadub-spacy | 111,787 | 36,798 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | gliner | 95,530 | 1,486 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | gliner-high-recall | 65,542 | 4,026 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v2 | Repeated PII values with decoys | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v1 | Synthetic identifiers in agentic formats | presidio-all | 75,541 | 60,586 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-en | 93,458 | 51,385 | 4,072 | 2,545 | 47,715 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-en-de | 81,804 | 57,657 | 5,883 | 734 | 14,028 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-strong | 90,045 | 13,789 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 86,274 | 18,703 | 6,587 | 30 | 330 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-core | 120,666 | 1,913 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-regex | 111,524 | 1,420 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-spacy | 87,945 | 69,841 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | datafog-gliner | 99,475 | 9,365 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | scrubadub-base | 128,648 | 133 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | scrubadub-spacy | 115,176 | 40,448 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | gliner | 87,287 | 13,706 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | gliner-high-recall | 58,495 | 23,105 | 6,617 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic identifiers in agentic formats | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
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
| v1 | Kiji EN/DE holdout and A4 negatives | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v1 | Synthetic benign lookalikes | presidio-all | 0 | 95,716 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-en | 0 | 87,014 | 3,783 | 1,409 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-en-de | 0 | 94,477 | 4,955 | 237 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-strong | 0 | 29,848 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 37,358 | 5,162 | 30 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-core | 0 | 5,577 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-regex | 0 | 5,293 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-spacy | 0 | 77,221 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | datafog-gliner | 0 | 17,145 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | scrubadub-base | 0 | 865 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | scrubadub-spacy | 0 | 56,858 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | gliner | 0 | 14,028 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | gliner-high-recall | 0 | 35,363 | 5,192 | 0 | 0 | not measured under a quiet machine |
| v1 | Synthetic benign lookalikes | opf | N/A | N/A | N/A | N/A | N/A | unavailable |
| v1 | Repeated PII values with decoys | presidio-all | 76,862 | 46,877 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-en | 83,401 | 43,461 | 2,145 | 916 | 26,378 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-en-de | 77,805 | 46,503 | 3,013 | 48 | 1,680 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-strong | 101,076 | 5,147 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | presidio-strong-high-recall | 96,834 | 9,521 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-core | 134,869 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-regex | 134,176 | 0 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-spacy | 101,215 | 52,296 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | datafog-gliner | 123,284 | 1,132 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | scrubadub-base | 135,380 | 38 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | scrubadub-spacy | 111,787 | 36,798 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | gliner | 95,530 | 1,486 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | gliner-high-recall | 65,542 | 4,026 | 3,061 | 0 | 0 | not measured under a quiet machine |
| v1 | Repeated PII values with decoys | opf | N/A | N/A | N/A | N/A | N/A | unavailable |

Skipped-language document counts: presidio-all/Synthetic identifiers in agentic formats: es=30; presidio-all/Synthetic benign lookalikes: es=30; presidio-en/Synthetic identifiers in agentic formats: de=1,811, es=30, fr=248, nl=280, pt=176; presidio-en/Kiji EN/DE holdout and A4 negatives: de=1,365; presidio-en/Synthetic benign lookalikes: de=1,172, es=30, fr=117, nl=30, pt=60; presidio-en/Repeated PII values with decoys: de=868, fr=48; presidio-en-de/Synthetic identifiers in agentic formats: es=30, fr=248, nl=280, pt=176; presidio-en-de/Synthetic benign lookalikes: es=30, fr=117, nl=30, pt=60; presidio-en-de/Repeated PII values with decoys: fr=48; presidio-strong/Synthetic identifiers in agentic formats: es=30; presidio-strong/Synthetic benign lookalikes: es=30; presidio-strong-high-recall/Synthetic identifiers in agentic formats: es=30; presidio-strong-high-recall/Synthetic benign lookalikes: es=30. Every skipped document counts as fully missed, including all its gold bytes.

**OPF: N/A (unavailable).** Two unchanged attempts reached the declared 600-second socket timeout. No partial metrics are published; the cause remains unproven. Attempt log SHA-256 receipts: `8bfd93468285a446fa9a357bc3497aa1ba89083e8ffc6e63eed6292289d45d48`, `9d1c6b5967f795baa4290892af41b7d6681147ca03e4548884c239ade3bb4a71`.

## Heldout safety and entity metrics

The validation/test split is fixed by document ID. Thresholds are selected on validation; the table below reports the disjoint test half. Product coverage scores unsupported gold as missed. Common intersection scores only classes claimed by every listed configuration. Entity scores require an exact UTF-8 byte span and a compatible reviewed label mapping.

Common classes: CREDITCARDNUMBER, DATEOFBIRTH, EMAIL, PHONENUMBER, ZIP.

| Contract | Layer | View | Tool | PII docs | Leaked B | FP B | Leaking docs | Doc leak % | Leaking entities | Entity leak % | Redaction load % | TP | FP | FN | Entity P | Entity R | F1 | F2 |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 3,274 | 38,580 | 31,142 | 2,105 | 64.3 | 2,558 | 61.5 | 21.5 | 317 | 3,402 | 3,842 | 0.085 | 0.076 | 0.080 | 0.078 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 3,274 | 47,354 | 26,899 | 2,460 | 75.1 | 3,015 | 72.5 | 17.1 | 151 | 2,596 | 4,008 | 0.055 | 0.036 | 0.044 | 0.039 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 3,274 | 41,797 | 29,797 | 2,248 | 68.7 | 2,719 | 65.4 | 19.9 | 265 | 3,100 | 3,894 | 0.079 | 0.064 | 0.070 | 0.066 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 3,274 | 45,571 | 6,823 | 2,314 | 70.7 | 2,764 | 66.5 | 10.9 | 363 | 2,353 | 3,796 | 0.134 | 0.087 | 0.106 | 0.094 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 3,274 | 43,786 | 9,355 | 2,227 | 68.0 | 2,677 | 64.4 | 12.4 | 363 | 3,039 | 3,796 | 0.107 | 0.087 | 0.096 | 0.091 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 3,274 | 61,273 | 989 | 2,743 | 83.8 | 3,389 | 81.5 | 3.6 | 127 | 798 | 4,032 | 0.137 | 0.031 | 0.050 | 0.036 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 3,274 | 57,202 | 670 | 2,593 | 79.2 | 3,289 | 79.1 | 4.9 | 323 | 715 | 3,836 | 0.311 | 0.078 | 0.124 | 0.091 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 3,274 | 45,094 | 35,574 | 2,314 | 70.7 | 2,773 | 66.7 | 20.8 | 361 | 2,898 | 3,798 | 0.111 | 0.087 | 0.097 | 0.091 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 3,274 | 50,845 | 4,387 | 2,268 | 69.3 | 2,668 | 64.2 | 8.3 | 384 | 1,337 | 3,775 | 0.223 | 0.092 | 0.131 | 0.105 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 3,274 | 64,820 | 79 | 3,027 | 92.5 | 3,862 | 92.9 | 2.1 | 71 | 248 | 4,088 | 0.223 | 0.017 | 0.032 | 0.021 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 3,274 | 58,098 | 21,071 | 2,924 | 89.3 | 3,634 | 87.4 | 11.5 | 58 | 800 | 4,101 | 0.068 | 0.014 | 0.023 | 0.017 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | gliner | 3,274 | 44,590 | 6,714 | 2,050 | 62.6 | 2,472 | 59.4 | 11.2 | 565 | 1,377 | 3,594 | 0.291 | 0.136 | 0.185 | 0.152 |
| v4 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 3,274 | 30,191 | 11,503 | 1,216 | 37.1 | 1,436 | 34.5 | 17.7 | 590 | 3,050 | 3,569 | 0.162 | 0.142 | 0.151 | 0.145 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 198 | 1,006 | 4,710 | 59 | 29.8 | 59 | 28.1 | 3.2 | 135 | 372 | 75 | 0.266 | 0.643 | 0.377 | 0.501 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 198 | 1,447 | 2,698 | 88 | 44.4 | 88 | 41.9 | 2.2 | 107 | 189 | 103 | 0.361 | 0.510 | 0.423 | 0.471 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 198 | 1,006 | 3,926 | 59 | 29.8 | 59 | 28.1 | 2.9 | 135 | 305 | 75 | 0.307 | 0.643 | 0.415 | 0.527 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 198 | 1,246 | 749 | 77 | 38.9 | 77 | 36.7 | 1.4 | 127 | 73 | 83 | 0.635 | 0.605 | 0.620 | 0.611 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 198 | 449 | 670 | 37 | 18.7 | 37 | 17.6 | 1.7 | 173 | 67 | 37 | 0.721 | 0.824 | 0.769 | 0.801 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 198 | 389 | 4,294 | 31 | 15.7 | 31 | 14.8 | 3.3 | 179 | 351 | 31 | 0.338 | 0.852 | 0.484 | 0.653 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 198 | 122 | 1,842 | 9 | 4.5 | 9 | 4.3 | 2.4 | 201 | 219 | 9 | 0.479 | 0.957 | 0.638 | 0.798 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 198 | 2,342 | 79 | 144 | 72.7 | 156 | 74.3 | 0.6 | 45 | 9 | 165 | 0.833 | 0.214 | 0.341 | 0.252 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 198 | 2,540 | 79 | 151 | 76.3 | 163 | 77.6 | 0.5 | 38 | 9 | 172 | 0.809 | 0.181 | 0.296 | 0.214 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | gliner | 198 | 281 | 1,116 | 17 | 8.6 | 17 | 8.1 | 2.0 | 174 | 145 | 36 | 0.545 | 0.829 | 0.658 | 0.751 |
| v4 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 198 | 191 | 1,885 | 12 | 6.1 | 12 | 5.7 | 2.3 | 179 | 235 | 31 | 0.432 | 0.852 | 0.574 | 0.714 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 39,889 | 15,145 | 872 | 87.9 | 4,792 | 62.0 | 8.8 | 1,613 | 2,444 | 6,118 | 0.398 | 0.209 | 0.274 | 0.231 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 16,067 | 25,514 | 890 | 89.7 | 2,284 | 29.5 | 15.7 | 3,212 | 3,902 | 4,519 | 0.452 | 0.415 | 0.433 | 0.422 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 15,855 | 26,356 | 886 | 89.3 | 2,265 | 29.3 | 15.9 | 3,216 | 4,048 | 4,515 | 0.443 | 0.416 | 0.429 | 0.421 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 57,944 | 3,073 | 992 | 100.0 | 6,977 | 90.2 | 2.7 | 671 | 408 | 7,060 | 0.622 | 0.087 | 0.152 | 0.105 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 55,630 | 3,179 | 992 | 100.0 | 6,865 | 88.8 | 3.2 | 744 | 528 | 6,987 | 0.585 | 0.096 | 0.165 | 0.116 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 17,316 | 131,449 | 883 | 89.0 | 2,135 | 27.6 | 36.8 | 2,663 | 9,833 | 5,068 | 0.213 | 0.344 | 0.263 | 0.307 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 17,681 | 24,786 | 819 | 82.6 | 1,642 | 21.2 | 15.2 | 1,727 | 3,880 | 6,004 | 0.308 | 0.223 | 0.259 | 0.236 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 57,771 | 703 | 992 | 100.0 | 7,213 | 93.3 | 2.3 | 440 | 115 | 7,291 | 0.793 | 0.057 | 0.106 | 0.070 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 47,004 | 7,610 | 966 | 97.4 | 5,809 | 75.1 | 5.8 | 819 | 1,387 | 6,912 | 0.371 | 0.106 | 0.165 | 0.124 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 13,551 | 26,371 | 660 | 66.5 | 1,327 | 17.2 | 16.4 | 2,014 | 3,517 | 5,717 | 0.364 | 0.261 | 0.304 | 0.276 |
| v4 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 11,640 | 34,318 | 613 | 61.8 | 1,146 | 14.8 | 18.4 | 2,040 | 4,218 | 5,691 | 0.326 | 0.264 | 0.292 | 0.274 |
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
| v4 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 48,902 | 0 | 0.0 | 0 | 0.0 | 19.0 | 0 | 2,416 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 44,689 | 0 | 0.0 | 0 | 0.0 | 17.4 | 0 | 2,085 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 48,224 | 0 | 0.0 | 0 | 0.0 | 18.7 | 0 | 2,368 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 14,198 | 0 | 0.0 | 0 | 0.0 | 5.5 | 0 | 1,504 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 17,854 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 2,091 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 2,676 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 297 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 2,638 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 294 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 40,270 | 0 | 0.0 | 0 | 0.0 | 15.6 | 0 | 1,339 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 8,534 | 0 | 0.0 | 0 | 0.0 | 3.3 | 0 | 708 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 443 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 34 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 30,384 | 0 | 0.0 | 0 | 0.0 | 11.8 | 0 | 444 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 6,744 | 0 | 0.0 | 0 | 0.0 | 2.6 | 0 | 515 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 17,342 | 0 | 0.0 | 0 | 0.0 | 6.7 | 0 | 1,369 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 6,238 | 0 | 0.0 | 0 | 0.0 | 2.4 | 0 | 459 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 3,362 | 0 | 0.0 | 0 | 0.0 | 1.3 | 0 | 269 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 5,695 | 0 | 0.0 | 0 | 0.0 | 2.2 | 0 | 421 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 3,979 | 0 | 0.0 | 0 | 0.0 | 1.5 | 0 | 281 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 3,994 | 0 | 0.0 | 0 | 0.0 | 1.6 | 0 | 284 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 1,484 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 175 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 1,646 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 189 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 5,067 | 0 | 0.0 | 0 | 0.0 | 2.0 | 0 | 507 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 2,391 | 0 | 0.0 | 0 | 0.0 | 0.9 | 0 | 247 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 392 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 30 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 1,256 | 0 | 0.0 | 0 | 0.0 | 0.5 | 0 | 89 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-all | 1,531 | 34,530 | 24,366 | 1,243 | 81.2 | 2,500 | 66.3 | 25.6 | 68 | 2,862 | 3,704 | 0.023 | 0.018 | 0.020 | 0.019 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-en | 1,531 | 38,264 | 22,442 | 1,279 | 83.5 | 2,831 | 75.1 | 23.0 | 30 | 2,543 | 3,742 | 0.012 | 0.008 | 0.009 | 0.008 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-en-de | 1,531 | 35,091 | 24,114 | 1,253 | 81.8 | 2,581 | 68.4 | 25.2 | 68 | 2,824 | 3,704 | 0.024 | 0.018 | 0.020 | 0.019 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-strong | 1,531 | 47,219 | 2,796 | 1,312 | 85.7 | 2,819 | 74.7 | 9.6 | 76 | 1,982 | 3,696 | 0.037 | 0.020 | 0.026 | 0.022 |
| v4 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 1,531 | 45,023 | 4,902 | 1,280 | 83.6 | 2,723 | 72.2 | 11.6 | 76 | 2,698 | 3,696 | 0.027 | 0.020 | 0.023 | 0.021 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-core | 1,531 | 63,320 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.8 | 47 | 82 | 3,725 | 0.364 | 0.012 | 0.024 | 0.015 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-regex | 1,531 | 62,982 | 0 | 1,519 | 99.2 | 3,689 | 97.8 | 1.0 | 62 | 78 | 3,710 | 0.443 | 0.016 | 0.032 | 0.020 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-spacy | 1,531 | 45,808 | 27,194 | 1,424 | 93.0 | 2,924 | 77.5 | 21.7 | 66 | 1,638 | 3,706 | 0.039 | 0.017 | 0.024 | 0.020 |
| v4 | Repeated PII values with decoys | product_coverage | datafog-gliner | 1,531 | 57,746 | 557 | 1,402 | 91.6 | 3,115 | 82.6 | 3.7 | 62 | 455 | 3,710 | 0.120 | 0.016 | 0.029 | 0.020 |
| v4 | Repeated PII values with decoys | product_coverage | scrubadub-base | 1,531 | 63,622 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.7 | 47 | 21 | 3,725 | 0.691 | 0.012 | 0.024 | 0.016 |
| v4 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 1,531 | 50,886 | 19,639 | 1,477 | 96.5 | 3,296 | 87.4 | 15.8 | 50 | 511 | 3,722 | 0.089 | 0.013 | 0.023 | 0.016 |
| v4 | Repeated PII values with decoys | product_coverage | gliner | 1,531 | 44,064 | 620 | 1,087 | 71.0 | 2,123 | 56.3 | 10.1 | 60 | 1,680 | 3,712 | 0.034 | 0.016 | 0.022 | 0.018 |
| v4 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 1,531 | 29,004 | 1,920 | 688 | 44.9 | 1,184 | 31.4 | 17.7 | 63 | 3,020 | 3,709 | 0.020 | 0.017 | 0.018 | 0.017 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-all | 47 | 0 | 1,083 | 0 | 0.0 | 0 | 0.0 | 1.5 | 47 | 66 | 0 | 0.416 | 1.000 | 0.588 | 0.781 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-en | 47 | 561 | 477 | 21 | 44.7 | 21 | 44.7 | 0.7 | 26 | 28 | 21 | 0.481 | 0.553 | 0.515 | 0.537 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-en-de | 47 | 0 | 955 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 62 | 0 | 0.431 | 1.000 | 0.603 | 0.791 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-strong | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v4 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-core | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-regex | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-spacy | 47 | 0 | 980 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 79 | 0 | 0.373 | 1.000 | 0.543 | 0.748 |
| v4 | Repeated PII values with decoys | common_intersection | datafog-gliner | 47 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 6 | 0 | 0.887 | 1.000 | 0.940 | 0.975 |
| v4 | Repeated PII values with decoys | common_intersection | scrubadub-base | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 47 | 23 | 0 | 1 | 2.1 | 1 | 2.1 | 0.8 | 46 | 0 | 1 | 1.000 | 0.979 | 0.989 | 0.983 |
| v4 | Repeated PII values with decoys | common_intersection | gliner | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v4 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 3,274 | 38,580 | 31,142 | 2,105 | 64.3 | 2,558 | 61.5 | 21.5 | 317 | 3,402 | 3,842 | 0.085 | 0.076 | 0.080 | 0.078 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 3,274 | 47,354 | 26,899 | 2,460 | 75.1 | 3,015 | 72.5 | 17.1 | 151 | 2,596 | 4,008 | 0.055 | 0.036 | 0.044 | 0.039 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 3,274 | 41,797 | 29,797 | 2,248 | 68.7 | 2,719 | 65.4 | 19.9 | 265 | 3,100 | 3,894 | 0.079 | 0.064 | 0.070 | 0.066 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 3,274 | 45,571 | 6,823 | 2,314 | 70.7 | 2,764 | 66.5 | 10.9 | 363 | 2,353 | 3,796 | 0.134 | 0.087 | 0.106 | 0.094 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 3,274 | 43,786 | 9,355 | 2,227 | 68.0 | 2,677 | 64.4 | 12.4 | 363 | 3,039 | 3,796 | 0.107 | 0.087 | 0.096 | 0.091 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 3,274 | 61,273 | 989 | 2,743 | 83.8 | 3,389 | 81.5 | 3.6 | 127 | 798 | 4,032 | 0.137 | 0.031 | 0.050 | 0.036 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 3,274 | 57,202 | 670 | 2,593 | 79.2 | 3,289 | 79.1 | 4.9 | 323 | 715 | 3,836 | 0.311 | 0.078 | 0.124 | 0.091 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 3,274 | 45,094 | 35,574 | 2,314 | 70.7 | 2,773 | 66.7 | 20.8 | 361 | 2,898 | 3,798 | 0.111 | 0.087 | 0.097 | 0.091 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 3,274 | 50,845 | 4,387 | 2,268 | 69.3 | 2,668 | 64.2 | 8.3 | 384 | 1,337 | 3,775 | 0.223 | 0.092 | 0.131 | 0.105 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 3,274 | 64,820 | 79 | 3,027 | 92.5 | 3,862 | 92.9 | 2.1 | 71 | 248 | 4,088 | 0.223 | 0.017 | 0.032 | 0.021 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 3,274 | 58,098 | 21,071 | 2,924 | 89.3 | 3,634 | 87.4 | 11.5 | 58 | 800 | 4,101 | 0.068 | 0.014 | 0.023 | 0.017 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | gliner | 3,274 | 44,590 | 6,714 | 2,050 | 62.6 | 2,472 | 59.4 | 11.2 | 565 | 1,377 | 3,594 | 0.291 | 0.136 | 0.185 | 0.152 |
| v3 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 3,274 | 30,191 | 11,503 | 1,216 | 37.1 | 1,436 | 34.5 | 17.7 | 590 | 3,050 | 3,569 | 0.162 | 0.142 | 0.151 | 0.145 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 198 | 1,006 | 4,710 | 59 | 29.8 | 59 | 28.1 | 3.2 | 135 | 372 | 75 | 0.266 | 0.643 | 0.377 | 0.501 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 198 | 1,447 | 2,698 | 88 | 44.4 | 88 | 41.9 | 2.2 | 107 | 189 | 103 | 0.361 | 0.510 | 0.423 | 0.471 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 198 | 1,006 | 3,926 | 59 | 29.8 | 59 | 28.1 | 2.9 | 135 | 305 | 75 | 0.307 | 0.643 | 0.415 | 0.527 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 198 | 1,246 | 749 | 77 | 38.9 | 77 | 36.7 | 1.4 | 127 | 73 | 83 | 0.635 | 0.605 | 0.620 | 0.611 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 198 | 449 | 670 | 37 | 18.7 | 37 | 17.6 | 1.7 | 173 | 67 | 37 | 0.721 | 0.824 | 0.769 | 0.801 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 198 | 389 | 4,294 | 31 | 15.7 | 31 | 14.8 | 3.3 | 179 | 351 | 31 | 0.338 | 0.852 | 0.484 | 0.653 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 198 | 122 | 1,842 | 9 | 4.5 | 9 | 4.3 | 2.4 | 201 | 219 | 9 | 0.479 | 0.957 | 0.638 | 0.798 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 198 | 2,342 | 79 | 144 | 72.7 | 156 | 74.3 | 0.6 | 45 | 9 | 165 | 0.833 | 0.214 | 0.341 | 0.252 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 198 | 2,540 | 79 | 151 | 76.3 | 163 | 77.6 | 0.5 | 38 | 9 | 172 | 0.809 | 0.181 | 0.296 | 0.214 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | gliner | 198 | 281 | 1,116 | 17 | 8.6 | 17 | 8.1 | 2.0 | 174 | 145 | 36 | 0.545 | 0.829 | 0.658 | 0.751 |
| v3 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 198 | 191 | 1,885 | 12 | 6.1 | 12 | 5.7 | 2.3 | 179 | 235 | 31 | 0.432 | 0.852 | 0.574 | 0.714 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 37,308 | 15,145 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,613 | 2,384 | 5,894 | 0.404 | 0.215 | 0.280 | 0.237 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 13,143 | 25,514 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,212 | 3,863 | 4,295 | 0.454 | 0.428 | 0.441 | 0.433 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 12,931 | 26,356 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,216 | 4,009 | 4,291 | 0.445 | 0.428 | 0.437 | 0.432 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 54,530 | 3,073 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 52,216 | 3,179 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 15,252 | 131,449 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 14,327 | 24,786 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 54,427 | 703 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 44,411 | 7,610 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 10,250 | 26,371 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v3 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 8,660 | 34,318 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
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
| v3 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 48,902 | 0 | 0.0 | 0 | 0.0 | 19.0 | 0 | 2,416 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 44,689 | 0 | 0.0 | 0 | 0.0 | 17.4 | 0 | 2,085 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 48,224 | 0 | 0.0 | 0 | 0.0 | 18.7 | 0 | 2,368 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 14,198 | 0 | 0.0 | 0 | 0.0 | 5.5 | 0 | 1,504 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 17,854 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 2,091 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 2,676 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 297 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 2,638 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 294 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 40,270 | 0 | 0.0 | 0 | 0.0 | 15.6 | 0 | 1,339 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 8,534 | 0 | 0.0 | 0 | 0.0 | 3.3 | 0 | 708 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 443 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 34 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 30,384 | 0 | 0.0 | 0 | 0.0 | 11.8 | 0 | 444 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 6,744 | 0 | 0.0 | 0 | 0.0 | 2.6 | 0 | 515 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 17,342 | 0 | 0.0 | 0 | 0.0 | 6.7 | 0 | 1,369 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 6,238 | 0 | 0.0 | 0 | 0.0 | 2.4 | 0 | 459 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 3,362 | 0 | 0.0 | 0 | 0.0 | 1.3 | 0 | 269 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 5,695 | 0 | 0.0 | 0 | 0.0 | 2.2 | 0 | 421 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 3,979 | 0 | 0.0 | 0 | 0.0 | 1.5 | 0 | 281 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 3,994 | 0 | 0.0 | 0 | 0.0 | 1.6 | 0 | 284 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 1,484 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 175 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 1,646 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 189 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 5,067 | 0 | 0.0 | 0 | 0.0 | 2.0 | 0 | 507 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 2,391 | 0 | 0.0 | 0 | 0.0 | 0.9 | 0 | 247 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 392 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 30 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 1,256 | 0 | 0.0 | 0 | 0.0 | 0.5 | 0 | 89 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-all | 1,531 | 34,530 | 24,366 | 1,243 | 81.2 | 2,500 | 66.3 | 25.6 | 68 | 2,862 | 3,704 | 0.023 | 0.018 | 0.020 | 0.019 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-en | 1,531 | 38,264 | 22,442 | 1,279 | 83.5 | 2,831 | 75.1 | 23.0 | 30 | 2,543 | 3,742 | 0.012 | 0.008 | 0.009 | 0.008 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-en-de | 1,531 | 35,091 | 24,114 | 1,253 | 81.8 | 2,581 | 68.4 | 25.2 | 68 | 2,824 | 3,704 | 0.024 | 0.018 | 0.020 | 0.019 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-strong | 1,531 | 47,219 | 2,796 | 1,312 | 85.7 | 2,819 | 74.7 | 9.6 | 76 | 1,982 | 3,696 | 0.037 | 0.020 | 0.026 | 0.022 |
| v3 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 1,531 | 45,023 | 4,902 | 1,280 | 83.6 | 2,723 | 72.2 | 11.6 | 76 | 2,698 | 3,696 | 0.027 | 0.020 | 0.023 | 0.021 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-core | 1,531 | 63,320 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.8 | 47 | 82 | 3,725 | 0.364 | 0.012 | 0.024 | 0.015 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-regex | 1,531 | 62,982 | 0 | 1,519 | 99.2 | 3,689 | 97.8 | 1.0 | 62 | 78 | 3,710 | 0.443 | 0.016 | 0.032 | 0.020 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-spacy | 1,531 | 45,808 | 27,194 | 1,424 | 93.0 | 2,924 | 77.5 | 21.7 | 66 | 1,638 | 3,706 | 0.039 | 0.017 | 0.024 | 0.020 |
| v3 | Repeated PII values with decoys | product_coverage | datafog-gliner | 1,531 | 57,746 | 557 | 1,402 | 91.6 | 3,115 | 82.6 | 3.7 | 62 | 455 | 3,710 | 0.120 | 0.016 | 0.029 | 0.020 |
| v3 | Repeated PII values with decoys | product_coverage | scrubadub-base | 1,531 | 63,622 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.7 | 47 | 21 | 3,725 | 0.691 | 0.012 | 0.024 | 0.016 |
| v3 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 1,531 | 50,886 | 19,639 | 1,477 | 96.5 | 3,296 | 87.4 | 15.8 | 50 | 511 | 3,722 | 0.089 | 0.013 | 0.023 | 0.016 |
| v3 | Repeated PII values with decoys | product_coverage | gliner | 1,531 | 44,064 | 620 | 1,087 | 71.0 | 2,123 | 56.3 | 10.1 | 60 | 1,680 | 3,712 | 0.034 | 0.016 | 0.022 | 0.018 |
| v3 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 1,531 | 29,004 | 1,920 | 688 | 44.9 | 1,184 | 31.4 | 17.7 | 63 | 3,020 | 3,709 | 0.020 | 0.017 | 0.018 | 0.017 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-all | 47 | 0 | 1,083 | 0 | 0.0 | 0 | 0.0 | 1.5 | 47 | 66 | 0 | 0.416 | 1.000 | 0.588 | 0.781 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-en | 47 | 561 | 477 | 21 | 44.7 | 21 | 44.7 | 0.7 | 26 | 28 | 21 | 0.481 | 0.553 | 0.515 | 0.537 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-en-de | 47 | 0 | 955 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 62 | 0 | 0.431 | 1.000 | 0.603 | 0.791 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-strong | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v3 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-core | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-regex | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-spacy | 47 | 0 | 980 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 79 | 0 | 0.373 | 1.000 | 0.543 | 0.748 |
| v3 | Repeated PII values with decoys | common_intersection | datafog-gliner | 47 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 6 | 0 | 0.887 | 1.000 | 0.940 | 0.975 |
| v3 | Repeated PII values with decoys | common_intersection | scrubadub-base | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 47 | 23 | 0 | 1 | 2.1 | 1 | 2.1 | 0.8 | 46 | 0 | 1 | 1.000 | 0.979 | 0.989 | 0.983 |
| v3 | Repeated PII values with decoys | common_intersection | gliner | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v3 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 3,274 | 38,580 | 31,142 | 2,105 | 64.3 | 2,558 | 61.5 | 21.5 | 317 | 3,402 | 3,842 | 0.085 | 0.076 | 0.080 | 0.078 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 3,274 | 47,354 | 26,899 | 2,460 | 75.1 | 3,015 | 72.5 | 17.1 | 151 | 2,596 | 4,008 | 0.055 | 0.036 | 0.044 | 0.039 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 3,274 | 41,797 | 29,797 | 2,248 | 68.7 | 2,719 | 65.4 | 19.9 | 265 | 3,100 | 3,894 | 0.079 | 0.064 | 0.070 | 0.066 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 3,274 | 45,571 | 6,823 | 2,314 | 70.7 | 2,764 | 66.5 | 10.9 | 363 | 2,353 | 3,796 | 0.134 | 0.087 | 0.106 | 0.094 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 3,274 | 43,786 | 9,355 | 2,227 | 68.0 | 2,677 | 64.4 | 12.4 | 363 | 3,039 | 3,796 | 0.107 | 0.087 | 0.096 | 0.091 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 3,274 | 61,273 | 989 | 2,743 | 83.8 | 3,389 | 81.5 | 3.6 | 127 | 798 | 4,032 | 0.137 | 0.031 | 0.050 | 0.036 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 3,274 | 57,202 | 670 | 2,593 | 79.2 | 3,289 | 79.1 | 4.9 | 323 | 715 | 3,836 | 0.311 | 0.078 | 0.124 | 0.091 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 3,274 | 45,094 | 35,574 | 2,314 | 70.7 | 2,773 | 66.7 | 20.8 | 361 | 2,898 | 3,798 | 0.111 | 0.087 | 0.097 | 0.091 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 3,274 | 50,845 | 4,387 | 2,268 | 69.3 | 2,668 | 64.2 | 8.3 | 384 | 1,337 | 3,775 | 0.223 | 0.092 | 0.131 | 0.105 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 3,274 | 64,820 | 79 | 3,027 | 92.5 | 3,862 | 92.9 | 2.1 | 71 | 248 | 4,088 | 0.223 | 0.017 | 0.032 | 0.021 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 3,274 | 58,098 | 21,071 | 2,924 | 89.3 | 3,634 | 87.4 | 11.5 | 58 | 800 | 4,101 | 0.068 | 0.014 | 0.023 | 0.017 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | gliner | 3,274 | 44,590 | 6,714 | 2,050 | 62.6 | 2,472 | 59.4 | 11.2 | 565 | 1,377 | 3,594 | 0.291 | 0.136 | 0.185 | 0.152 |
| v2 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 3,274 | 30,191 | 11,503 | 1,216 | 37.1 | 1,436 | 34.5 | 17.7 | 590 | 3,050 | 3,569 | 0.162 | 0.142 | 0.151 | 0.145 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 198 | 1,006 | 4,710 | 59 | 29.8 | 59 | 28.1 | 3.2 | 135 | 372 | 75 | 0.266 | 0.643 | 0.377 | 0.501 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 198 | 1,447 | 2,698 | 88 | 44.4 | 88 | 41.9 | 2.2 | 107 | 189 | 103 | 0.361 | 0.510 | 0.423 | 0.471 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 198 | 1,006 | 3,926 | 59 | 29.8 | 59 | 28.1 | 2.9 | 135 | 305 | 75 | 0.307 | 0.643 | 0.415 | 0.527 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 198 | 1,246 | 749 | 77 | 38.9 | 77 | 36.7 | 1.4 | 127 | 73 | 83 | 0.635 | 0.605 | 0.620 | 0.611 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 198 | 449 | 670 | 37 | 18.7 | 37 | 17.6 | 1.7 | 173 | 67 | 37 | 0.721 | 0.824 | 0.769 | 0.801 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 198 | 389 | 4,294 | 31 | 15.7 | 31 | 14.8 | 3.3 | 179 | 351 | 31 | 0.338 | 0.852 | 0.484 | 0.653 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 198 | 122 | 1,842 | 9 | 4.5 | 9 | 4.3 | 2.4 | 201 | 219 | 9 | 0.479 | 0.957 | 0.638 | 0.798 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 198 | 2,342 | 79 | 144 | 72.7 | 156 | 74.3 | 0.6 | 45 | 9 | 165 | 0.833 | 0.214 | 0.341 | 0.252 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 198 | 2,540 | 79 | 151 | 76.3 | 163 | 77.6 | 0.5 | 38 | 9 | 172 | 0.809 | 0.181 | 0.296 | 0.214 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | gliner | 198 | 281 | 1,116 | 17 | 8.6 | 17 | 8.1 | 2.0 | 174 | 145 | 36 | 0.545 | 0.829 | 0.658 | 0.751 |
| v2 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 198 | 191 | 1,885 | 12 | 6.1 | 12 | 5.7 | 2.3 | 179 | 235 | 31 | 0.432 | 0.852 | 0.574 | 0.714 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 37,308 | 15,145 | 851 | 85.8 | 4,613 | 61.4 | 8.7 | 1,613 | 2,384 | 5,894 | 0.404 | 0.215 | 0.280 | 0.237 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 15,775 | 29,918 | 821 | 82.8 | 2,234 | 29.8 | 16.0 | 2,860 | 4,349 | 4,647 | 0.397 | 0.381 | 0.389 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 13,143 | 25,514 | 862 | 86.9 | 2,096 | 27.9 | 15.7 | 3,212 | 3,863 | 4,295 | 0.454 | 0.428 | 0.441 | 0.433 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 12,931 | 26,356 | 857 | 86.4 | 2,077 | 27.7 | 15.9 | 3,216 | 4,009 | 4,291 | 0.445 | 0.428 | 0.437 | 0.432 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 54,530 | 3,073 | 992 | 100.0 | 6,753 | 90.0 | 2.7 | 671 | 408 | 6,836 | 0.622 | 0.089 | 0.156 | 0.108 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 52,216 | 3,179 | 992 | 100.0 | 6,641 | 88.5 | 3.2 | 744 | 528 | 6,763 | 0.585 | 0.099 | 0.169 | 0.119 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 15,252 | 131,449 | 855 | 86.2 | 1,985 | 26.4 | 36.8 | 2,663 | 9,760 | 4,844 | 0.214 | 0.355 | 0.267 | 0.314 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 14,327 | 24,786 | 757 | 76.3 | 1,423 | 19.0 | 15.3 | 1,727 | 3,874 | 5,780 | 0.308 | 0.230 | 0.264 | 0.242 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 54,427 | 703 | 992 | 100.0 | 6,995 | 93.2 | 2.3 | 440 | 109 | 7,067 | 0.801 | 0.059 | 0.109 | 0.072 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 44,411 | 7,610 | 961 | 96.9 | 5,632 | 75.0 | 5.7 | 819 | 1,331 | 6,688 | 0.381 | 0.109 | 0.170 | 0.127 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 10,250 | 26,371 | 564 | 56.9 | 1,110 | 14.8 | 16.5 | 2,014 | 3,510 | 5,493 | 0.365 | 0.268 | 0.309 | 0.283 |
| v2 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 8,660 | 34,318 | 526 | 53.0 | 953 | 12.7 | 18.4 | 2,040 | 4,189 | 5,467 | 0.328 | 0.272 | 0.297 | 0.281 |
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
| v2 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 48,902 | 0 | 0.0 | 0 | 0.0 | 19.0 | 0 | 2,416 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 44,689 | 0 | 0.0 | 0 | 0.0 | 17.4 | 0 | 2,085 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 48,224 | 0 | 0.0 | 0 | 0.0 | 18.7 | 0 | 2,368 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 14,198 | 0 | 0.0 | 0 | 0.0 | 5.5 | 0 | 1,504 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 17,854 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 2,091 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 2,676 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 297 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 2,638 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 294 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 40,270 | 0 | 0.0 | 0 | 0.0 | 15.6 | 0 | 1,339 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 8,534 | 0 | 0.0 | 0 | 0.0 | 3.3 | 0 | 708 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 443 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 34 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 30,384 | 0 | 0.0 | 0 | 0.0 | 11.8 | 0 | 444 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 6,744 | 0 | 0.0 | 0 | 0.0 | 2.6 | 0 | 515 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 17,342 | 0 | 0.0 | 0 | 0.0 | 6.7 | 0 | 1,369 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 6,238 | 0 | 0.0 | 0 | 0.0 | 2.4 | 0 | 459 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 3,362 | 0 | 0.0 | 0 | 0.0 | 1.3 | 0 | 269 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 5,695 | 0 | 0.0 | 0 | 0.0 | 2.2 | 0 | 421 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 3,979 | 0 | 0.0 | 0 | 0.0 | 1.5 | 0 | 281 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 3,994 | 0 | 0.0 | 0 | 0.0 | 1.6 | 0 | 284 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 1,484 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 175 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 1,646 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 189 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 5,067 | 0 | 0.0 | 0 | 0.0 | 2.0 | 0 | 507 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 2,391 | 0 | 0.0 | 0 | 0.0 | 0.9 | 0 | 247 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 392 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 30 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 1,256 | 0 | 0.0 | 0 | 0.0 | 0.5 | 0 | 89 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-all | 1,531 | 34,530 | 24,366 | 1,243 | 81.2 | 2,500 | 66.3 | 25.6 | 68 | 2,862 | 3,704 | 0.023 | 0.018 | 0.020 | 0.019 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-en | 1,531 | 38,264 | 22,442 | 1,279 | 83.5 | 2,831 | 75.1 | 23.0 | 30 | 2,543 | 3,742 | 0.012 | 0.008 | 0.009 | 0.008 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-en-de | 1,531 | 35,091 | 24,114 | 1,253 | 81.8 | 2,581 | 68.4 | 25.2 | 68 | 2,824 | 3,704 | 0.024 | 0.018 | 0.020 | 0.019 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-strong | 1,531 | 47,219 | 2,796 | 1,312 | 85.7 | 2,819 | 74.7 | 9.6 | 76 | 1,982 | 3,696 | 0.037 | 0.020 | 0.026 | 0.022 |
| v2 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 1,531 | 45,023 | 4,902 | 1,280 | 83.6 | 2,723 | 72.2 | 11.6 | 76 | 2,698 | 3,696 | 0.027 | 0.020 | 0.023 | 0.021 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-core | 1,531 | 63,320 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.8 | 47 | 82 | 3,725 | 0.364 | 0.012 | 0.024 | 0.015 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-regex | 1,531 | 62,982 | 0 | 1,519 | 99.2 | 3,689 | 97.8 | 1.0 | 62 | 78 | 3,710 | 0.443 | 0.016 | 0.032 | 0.020 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-spacy | 1,531 | 45,808 | 27,194 | 1,424 | 93.0 | 2,924 | 77.5 | 21.7 | 66 | 1,638 | 3,706 | 0.039 | 0.017 | 0.024 | 0.020 |
| v2 | Repeated PII values with decoys | product_coverage | datafog-gliner | 1,531 | 57,746 | 557 | 1,402 | 91.6 | 3,115 | 82.6 | 3.7 | 62 | 455 | 3,710 | 0.120 | 0.016 | 0.029 | 0.020 |
| v2 | Repeated PII values with decoys | product_coverage | scrubadub-base | 1,531 | 63,622 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.7 | 47 | 21 | 3,725 | 0.691 | 0.012 | 0.024 | 0.016 |
| v2 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 1,531 | 50,886 | 19,639 | 1,477 | 96.5 | 3,296 | 87.4 | 15.8 | 50 | 511 | 3,722 | 0.089 | 0.013 | 0.023 | 0.016 |
| v2 | Repeated PII values with decoys | product_coverage | gliner | 1,531 | 44,064 | 620 | 1,087 | 71.0 | 2,123 | 56.3 | 10.1 | 60 | 1,680 | 3,712 | 0.034 | 0.016 | 0.022 | 0.018 |
| v2 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 1,531 | 29,004 | 1,920 | 688 | 44.9 | 1,184 | 31.4 | 17.7 | 63 | 3,020 | 3,709 | 0.020 | 0.017 | 0.018 | 0.017 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-all | 47 | 0 | 1,083 | 0 | 0.0 | 0 | 0.0 | 1.5 | 47 | 66 | 0 | 0.416 | 1.000 | 0.588 | 0.781 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-en | 47 | 561 | 477 | 21 | 44.7 | 21 | 44.7 | 0.7 | 26 | 28 | 21 | 0.481 | 0.553 | 0.515 | 0.537 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-en-de | 47 | 0 | 955 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 62 | 0 | 0.431 | 1.000 | 0.603 | 0.791 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-strong | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v2 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-core | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-regex | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-spacy | 47 | 0 | 980 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 79 | 0 | 0.373 | 1.000 | 0.543 | 0.748 |
| v2 | Repeated PII values with decoys | common_intersection | datafog-gliner | 47 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 6 | 0 | 0.887 | 1.000 | 0.940 | 0.975 |
| v2 | Repeated PII values with decoys | common_intersection | scrubadub-base | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 47 | 23 | 0 | 1 | 2.1 | 1 | 2.1 | 0.8 | 46 | 0 | 1 | 1.000 | 0.979 | 0.989 | 0.983 |
| v2 | Repeated PII values with decoys | common_intersection | gliner | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v2 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-all | 3,274 | 38,580 | 31,142 | 2,105 | 64.3 | 2,558 | 61.5 | 21.5 | 317 | 3,402 | 3,842 | 0.085 | 0.076 | 0.080 | 0.078 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-en | 3,274 | 47,354 | 26,899 | 2,460 | 75.1 | 3,015 | 72.5 | 17.1 | 151 | 2,596 | 4,008 | 0.055 | 0.036 | 0.044 | 0.039 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-en-de | 3,274 | 41,797 | 29,797 | 2,248 | 68.7 | 2,719 | 65.4 | 19.9 | 265 | 3,100 | 3,894 | 0.079 | 0.064 | 0.070 | 0.066 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong | 3,274 | 45,571 | 6,823 | 2,314 | 70.7 | 2,764 | 66.5 | 10.9 | 363 | 2,353 | 3,796 | 0.134 | 0.087 | 0.106 | 0.094 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | presidio-strong-high-recall | 3,274 | 43,786 | 9,355 | 2,227 | 68.0 | 2,677 | 64.4 | 12.4 | 363 | 3,039 | 3,796 | 0.107 | 0.087 | 0.096 | 0.091 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-core | 3,274 | 61,273 | 989 | 2,743 | 83.8 | 3,389 | 81.5 | 3.6 | 127 | 798 | 4,032 | 0.137 | 0.031 | 0.050 | 0.036 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-regex | 3,274 | 57,202 | 670 | 2,593 | 79.2 | 3,289 | 79.1 | 4.9 | 323 | 715 | 3,836 | 0.311 | 0.078 | 0.124 | 0.091 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-spacy | 3,274 | 45,094 | 35,574 | 2,314 | 70.7 | 2,773 | 66.7 | 20.8 | 361 | 2,898 | 3,798 | 0.111 | 0.087 | 0.097 | 0.091 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | datafog-gliner | 3,274 | 50,845 | 4,387 | 2,268 | 69.3 | 2,668 | 64.2 | 8.3 | 384 | 1,337 | 3,775 | 0.223 | 0.092 | 0.131 | 0.105 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-base | 3,274 | 64,820 | 79 | 3,027 | 92.5 | 3,862 | 92.9 | 2.1 | 71 | 248 | 4,088 | 0.223 | 0.017 | 0.032 | 0.021 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | scrubadub-spacy | 3,274 | 58,098 | 21,071 | 2,924 | 89.3 | 3,634 | 87.4 | 11.5 | 58 | 800 | 4,101 | 0.068 | 0.014 | 0.023 | 0.017 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | gliner | 3,274 | 44,590 | 6,714 | 2,050 | 62.6 | 2,472 | 59.4 | 11.2 | 565 | 1,377 | 3,594 | 0.291 | 0.136 | 0.185 | 0.152 |
| v1 | Synthetic identifiers in agentic formats | product_coverage | gliner-high-recall | 3,274 | 30,191 | 11,503 | 1,216 | 37.1 | 1,436 | 34.5 | 17.7 | 590 | 3,050 | 3,569 | 0.162 | 0.142 | 0.151 | 0.145 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-all | 198 | 1,006 | 4,710 | 59 | 29.8 | 59 | 28.1 | 3.2 | 135 | 372 | 75 | 0.266 | 0.643 | 0.377 | 0.501 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-en | 198 | 1,447 | 2,698 | 88 | 44.4 | 88 | 41.9 | 2.2 | 107 | 189 | 103 | 0.361 | 0.510 | 0.423 | 0.471 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-en-de | 198 | 1,006 | 3,926 | 59 | 29.8 | 59 | 28.1 | 2.9 | 135 | 305 | 75 | 0.307 | 0.643 | 0.415 | 0.527 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | presidio-strong-high-recall | 198 | 1,063 | 3,356 | 59 | 29.8 | 59 | 28.1 | 2.6 | 144 | 288 | 66 | 0.333 | 0.686 | 0.449 | 0.566 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-core | 198 | 1,246 | 749 | 77 | 38.9 | 77 | 36.7 | 1.4 | 127 | 73 | 83 | 0.635 | 0.605 | 0.620 | 0.611 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-regex | 198 | 449 | 670 | 37 | 18.7 | 37 | 17.6 | 1.7 | 173 | 67 | 37 | 0.721 | 0.824 | 0.769 | 0.801 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-spacy | 198 | 389 | 4,294 | 31 | 15.7 | 31 | 14.8 | 3.3 | 179 | 351 | 31 | 0.338 | 0.852 | 0.484 | 0.653 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | datafog-gliner | 198 | 122 | 1,842 | 9 | 4.5 | 9 | 4.3 | 2.4 | 201 | 219 | 9 | 0.479 | 0.957 | 0.638 | 0.798 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-base | 198 | 2,342 | 79 | 144 | 72.7 | 156 | 74.3 | 0.6 | 45 | 9 | 165 | 0.833 | 0.214 | 0.341 | 0.252 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | scrubadub-spacy | 198 | 2,540 | 79 | 151 | 76.3 | 163 | 77.6 | 0.5 | 38 | 9 | 172 | 0.809 | 0.181 | 0.296 | 0.214 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | gliner | 198 | 281 | 1,116 | 17 | 8.6 | 17 | 8.1 | 2.0 | 174 | 145 | 36 | 0.545 | 0.829 | 0.658 | 0.751 |
| v1 | Synthetic identifiers in agentic formats | common_intersection | gliner-high-recall | 198 | 191 | 1,885 | 12 | 6.1 | 12 | 5.7 | 2.3 | 179 | 235 | 31 | 0.432 | 0.852 | 0.574 | 0.714 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-all | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en | 992 | 39,889 | 15,145 | 872 | 87.9 | 4,792 | 62.0 | 8.8 | 1,613 | 2,444 | 6,118 | 0.398 | 0.209 | 0.274 | 0.231 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-en-de | 992 | 18,047 | 29,918 | 851 | 85.8 | 2,391 | 30.9 | 16.2 | 2,860 | 4,428 | 4,871 | 0.392 | 0.370 | 0.381 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong | 992 | 16,067 | 25,514 | 890 | 89.7 | 2,284 | 29.5 | 15.7 | 3,212 | 3,902 | 4,519 | 0.452 | 0.415 | 0.433 | 0.422 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | presidio-strong-high-recall | 992 | 15,855 | 26,356 | 886 | 89.3 | 2,265 | 29.3 | 15.9 | 3,216 | 4,048 | 4,515 | 0.443 | 0.416 | 0.429 | 0.421 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-core | 992 | 57,944 | 3,073 | 992 | 100.0 | 6,977 | 90.2 | 2.7 | 671 | 408 | 7,060 | 0.622 | 0.087 | 0.152 | 0.105 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-regex | 992 | 55,630 | 3,179 | 992 | 100.0 | 6,865 | 88.8 | 3.2 | 744 | 528 | 6,987 | 0.585 | 0.096 | 0.165 | 0.116 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-spacy | 992 | 17,316 | 131,449 | 883 | 89.0 | 2,135 | 27.6 | 36.8 | 2,663 | 9,833 | 5,068 | 0.213 | 0.344 | 0.263 | 0.307 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | datafog-gliner | 992 | 17,681 | 24,786 | 819 | 82.6 | 1,642 | 21.2 | 15.2 | 1,727 | 3,880 | 6,004 | 0.308 | 0.223 | 0.259 | 0.236 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-base | 992 | 57,771 | 703 | 992 | 100.0 | 7,213 | 93.3 | 2.3 | 440 | 115 | 7,291 | 0.793 | 0.057 | 0.106 | 0.070 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | scrubadub-spacy | 992 | 47,004 | 7,610 | 966 | 97.4 | 5,809 | 75.1 | 5.8 | 819 | 1,387 | 6,912 | 0.371 | 0.106 | 0.165 | 0.124 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner | 992 | 13,551 | 26,371 | 660 | 66.5 | 1,327 | 17.2 | 16.4 | 2,014 | 3,517 | 5,717 | 0.364 | 0.261 | 0.304 | 0.276 |
| v1 | Kiji EN/DE holdout and A4 negatives | product_coverage | gliner-high-recall | 992 | 11,640 | 34,318 | 613 | 61.8 | 1,146 | 14.8 | 18.4 | 2,040 | 4,218 | 5,691 | 0.326 | 0.264 | 0.292 | 0.274 |
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
| v1 | Synthetic benign lookalikes | product_coverage | presidio-all | 0 | 0 | 48,902 | 0 | 0.0 | 0 | 0.0 | 19.0 | 0 | 2,416 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-en | 0 | 0 | 44,689 | 0 | 0.0 | 0 | 0.0 | 17.4 | 0 | 2,085 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-en-de | 0 | 0 | 48,224 | 0 | 0.0 | 0 | 0.0 | 18.7 | 0 | 2,368 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-strong | 0 | 0 | 14,198 | 0 | 0.0 | 0 | 0.0 | 5.5 | 0 | 1,504 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | presidio-strong-high-recall | 0 | 0 | 17,854 | 0 | 0.0 | 0 | 0.0 | 6.9 | 0 | 2,091 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-core | 0 | 0 | 2,676 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 297 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-regex | 0 | 0 | 2,638 | 0 | 0.0 | 0 | 0.0 | 1.0 | 0 | 294 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-spacy | 0 | 0 | 40,270 | 0 | 0.0 | 0 | 0.0 | 15.6 | 0 | 1,339 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | datafog-gliner | 0 | 0 | 8,534 | 0 | 0.0 | 0 | 0.0 | 3.3 | 0 | 708 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | scrubadub-base | 0 | 0 | 443 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 34 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | scrubadub-spacy | 0 | 0 | 30,384 | 0 | 0.0 | 0 | 0.0 | 11.8 | 0 | 444 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | gliner | 0 | 0 | 6,744 | 0 | 0.0 | 0 | 0.0 | 2.6 | 0 | 515 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | product_coverage | gliner-high-recall | 0 | 0 | 17,342 | 0 | 0.0 | 0 | 0.0 | 6.7 | 0 | 1,369 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-all | 0 | 0 | 6,238 | 0 | 0.0 | 0 | 0.0 | 2.4 | 0 | 459 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-en | 0 | 0 | 3,362 | 0 | 0.0 | 0 | 0.0 | 1.3 | 0 | 269 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-en-de | 0 | 0 | 5,695 | 0 | 0.0 | 0 | 0.0 | 2.2 | 0 | 421 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-strong | 0 | 0 | 3,979 | 0 | 0.0 | 0 | 0.0 | 1.5 | 0 | 281 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | presidio-strong-high-recall | 0 | 0 | 3,994 | 0 | 0.0 | 0 | 0.0 | 1.6 | 0 | 284 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-core | 0 | 0 | 1,484 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 175 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-regex | 0 | 0 | 1,646 | 0 | 0.0 | 0 | 0.0 | 0.6 | 0 | 189 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-spacy | 0 | 0 | 5,067 | 0 | 0.0 | 0 | 0.0 | 2.0 | 0 | 507 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | datafog-gliner | 0 | 0 | 2,391 | 0 | 0.0 | 0 | 0.0 | 0.9 | 0 | 247 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | scrubadub-base | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | scrubadub-spacy | 0 | 0 | 410 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 31 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | gliner | 0 | 0 | 392 | 0 | 0.0 | 0 | 0.0 | 0.2 | 0 | 30 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | common_intersection | gliner-high-recall | 0 | 0 | 1,256 | 0 | 0.0 | 0 | 0.0 | 0.5 | 0 | 89 | 0 | 0.000 | 0.000 | 0.000 | 0.000 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-all | 1,531 | 34,530 | 24,366 | 1,243 | 81.2 | 2,500 | 66.3 | 25.6 | 68 | 2,862 | 3,704 | 0.023 | 0.018 | 0.020 | 0.019 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-en | 1,531 | 38,264 | 22,442 | 1,279 | 83.5 | 2,831 | 75.1 | 23.0 | 30 | 2,543 | 3,742 | 0.012 | 0.008 | 0.009 | 0.008 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-en-de | 1,531 | 35,091 | 24,114 | 1,253 | 81.8 | 2,581 | 68.4 | 25.2 | 68 | 2,824 | 3,704 | 0.024 | 0.018 | 0.020 | 0.019 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-strong | 1,531 | 47,219 | 2,796 | 1,312 | 85.7 | 2,819 | 74.7 | 9.6 | 76 | 1,982 | 3,696 | 0.037 | 0.020 | 0.026 | 0.022 |
| v1 | Repeated PII values with decoys | product_coverage | presidio-strong-high-recall | 1,531 | 45,023 | 4,902 | 1,280 | 83.6 | 2,723 | 72.2 | 11.6 | 76 | 2,698 | 3,696 | 0.027 | 0.020 | 0.023 | 0.021 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-core | 1,531 | 63,320 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.8 | 47 | 82 | 3,725 | 0.364 | 0.012 | 0.024 | 0.015 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-regex | 1,531 | 62,982 | 0 | 1,519 | 99.2 | 3,689 | 97.8 | 1.0 | 62 | 78 | 3,710 | 0.443 | 0.016 | 0.032 | 0.020 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-spacy | 1,531 | 45,808 | 27,194 | 1,424 | 93.0 | 2,924 | 77.5 | 21.7 | 66 | 1,638 | 3,706 | 0.039 | 0.017 | 0.024 | 0.020 |
| v1 | Repeated PII values with decoys | product_coverage | datafog-gliner | 1,531 | 57,746 | 557 | 1,402 | 91.6 | 3,115 | 82.6 | 3.7 | 62 | 455 | 3,710 | 0.120 | 0.016 | 0.029 | 0.020 |
| v1 | Repeated PII values with decoys | product_coverage | scrubadub-base | 1,531 | 63,622 | 0 | 1,524 | 99.5 | 3,704 | 98.2 | 0.7 | 47 | 21 | 3,725 | 0.691 | 0.012 | 0.024 | 0.016 |
| v1 | Repeated PII values with decoys | product_coverage | scrubadub-spacy | 1,531 | 50,886 | 19,639 | 1,477 | 96.5 | 3,296 | 87.4 | 15.8 | 50 | 511 | 3,722 | 0.089 | 0.013 | 0.023 | 0.016 |
| v1 | Repeated PII values with decoys | product_coverage | gliner | 1,531 | 44,064 | 620 | 1,087 | 71.0 | 2,123 | 56.3 | 10.1 | 60 | 1,680 | 3,712 | 0.034 | 0.016 | 0.022 | 0.018 |
| v1 | Repeated PII values with decoys | product_coverage | gliner-high-recall | 1,531 | 29,004 | 1,920 | 688 | 44.9 | 1,184 | 31.4 | 17.7 | 63 | 3,020 | 3,709 | 0.020 | 0.017 | 0.018 | 0.017 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-all | 47 | 0 | 1,083 | 0 | 0.0 | 0 | 0.0 | 1.5 | 47 | 66 | 0 | 0.416 | 1.000 | 0.588 | 0.781 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-en | 47 | 561 | 477 | 21 | 44.7 | 21 | 44.7 | 0.7 | 26 | 28 | 21 | 0.481 | 0.553 | 0.515 | 0.537 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-en-de | 47 | 0 | 955 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 62 | 0 | 0.431 | 1.000 | 0.603 | 0.791 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-strong | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v1 | Repeated PII values with decoys | common_intersection | presidio-strong-high-recall | 47 | 0 | 606 | 0 | 0.0 | 0 | 0.0 | 1.2 | 47 | 38 | 0 | 0.553 | 1.000 | 0.712 | 0.861 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-core | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-regex | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-spacy | 47 | 0 | 980 | 0 | 0.0 | 0 | 0.0 | 1.4 | 47 | 79 | 0 | 0.373 | 1.000 | 0.543 | 0.748 |
| v1 | Repeated PII values with decoys | common_intersection | datafog-gliner | 47 | 0 | 60 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 6 | 0 | 0.887 | 1.000 | 0.940 | 0.975 |
| v1 | Repeated PII values with decoys | common_intersection | scrubadub-base | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | scrubadub-spacy | 47 | 23 | 0 | 1 | 2.1 | 1 | 2.1 | 0.8 | 46 | 0 | 1 | 1.000 | 0.979 | 0.989 | 0.983 |
| v1 | Repeated PII values with decoys | common_intersection | gliner | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |
| v1 | Repeated PII values with decoys | common_intersection | gliner-high-recall | 47 | 0 | 0 | 0 | 0.0 | 0 | 0.0 | 0.8 | 47 | 0 | 0 | 1.000 | 1.000 | 1.000 | 1.000 |

Threshold choice uses validation only: gliner → gliner-high-recall; presidio-strong → presidio-strong-high-recall.

## Tuned Presidio

Three more Presidio rows on the same documents, contracts and scorer as above. Every number in this section names its split; test-half numbers are the comparison.

- **Tuned by its authors:** Presidio Research's own tuned setup for its corpus (notebook 5, custom analyzer: OpenMed NER recognizer, title/year/age pattern recognizers, lemma context enhancement, 14 predefined recognizers removed, score threshold 0.3; `notebooks/5_Evaluate_Custom_Presidio_Analyzer.ipynb` at presidio-research `6db3769a`), applied unchanged. It supports English only, so every document is analyzed as English. Its entities without a label in this corpus: BIOMETRIC_IDENTIFIER, BLOOD_TYPE, COORDINATE, CVV, EDUCATION_LEVEL, EMPLOYMENT_STATUS, ETHNICITY, GENDER, ID, IP_ADDRESS, LANGUAGE, MAC_ADDRESS, MEDICAL_LICENSE, OCCUPATION, POLITICAL_VIEW, PROFESSIONAL_LICENSE, RELIGIOUS_BELIEF, SEXUALITY, SWIFT_CODE, USER_NAME, US_BANK_NUMBER, http_cookie. Scored labels here that none of its entities maps to: AADHAAR, BSN, CNPJ, CPF, CUSTOMER_ID, EMPLOYEE_ID, ETHADDRESS, GIVENNAME, IDCARDNUM, IPADDRESS, LICENSEPLATE, NATIONALID, NHSNUMBER, NIR, ORGANIZATION, PAN, RECORD_ID, TELEPHONENUM, USERNAME, VATID, ZIPCODE. Labels affect only the typed metrics; leaked and false-positive bytes ignore them.
- **Tuned for this corpus:** a search over a space declared in code before any run ([`space.py`](../../../scripts/bench/compare/tuned_presidio/space.py)): the NLP-engine NER (spaCy large, `dslim/bert-base-NER` or none), extra NER recognizers Presidio ships (the multilingual Davlan model Gaze itself installs, the OpenMed PII model Presidio Research chose, GLiNER), every predefined recognizer, 21 custom pattern and deny-list recognizers for this corpus's classes, per recognizer and entity thresholds, the context enhancer and an allow list learned from validation false positives. The search scored only the validation half (8,967 documents) and evaluated 21,803 candidate configurations by coordinate descent. Two objectives: the comparison's own rule (fewest validation v3 leaked bytes, then fewest false-positive bytes) and the panels' headline (highest validation v3 character F2). The custom recognizers were written after reading validation-half gold examples; the search never received test-half text, gold or output. The committed choice was made by an earlier loader that built the whole corpus in memory and dropped the test half before the search; rerunning selection through per-half files, with the test-half file never opened, reproduces every choice and validation score. A guard test runs the current real loader with every test-half file unreadable. The NER models' training data is not fully published, so overlap with this synthetic corpus's style cannot be ruled out for them (nor for the NER model in Gaze's own setup). Both halves are synthetic and share their generators, and the split holds out document ids only, not templates or values: 922 of 1,119 (82.4 %) of the Synthetic identifiers in agentic formats test documents reuse a validation gold value (every layer is counted beside the test-half table below). Tuning on validation therefore learns those templates and values, and the test half measures fit to this corpus, not robustness to unseen phrasing or values. Gaze's rules were developed against the same corpus.
- **Budget:** Gaze's rules received 58 rulepack commits (2026-04-24 to 2026-09-30) at the original measurement lineage `ef369b33`, made with the whole corpus visible, test half included. This budget is pinned to that lineage, not the current branch's rulepack history. The tuned Presidio search is at least as generous in iterations: 21,803 measured candidate configurations against 58 rulepack commits, on top of a hand-written recognizer for the classes Gaze commits to. It saw the validation half only during scoring, so its test-half documents are held out, by id, while Gaze's are not.

Validation choice (v3, C/A/D/R summed):

| Objective | Start | Leaked B | FP B | Char F2 | Chosen |
| --- | --- | ---: | ---: | ---: | --- |
| f2 | everything | 38,216 | 88,502 | 0.770 | yes |
| f2 | presidio-default | 40,701 | 80,756 | 0.765 |  |
| leak-first | everything | 23,723 | 180,207 | 0.761 | yes |
| leak-first | presidio-default | 23,723 | 183,242 | 0.759 |  |

Chosen configurations:

- **f2:** NLP-engine NER spacy; extra NER davlan all, gliner off, openmed all; context default; 80 pattern recognizers on; 42 raised thresholds; allow list of 864 texts. Full configuration in [`presidio-tuned-selection.json`](presidio-tuned-selection.json).
- **leak-first:** NLP-engine NER spacy; extra NER davlan off, gliner all, openmed all; context default; 76 pattern recognizers on; 122 raised thresholds; allow list of 1,378 texts. Full configuration in [`presidio-tuned-selection.json`](presidio-tuned-selection.json).

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
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 24,228 | 18,430 | 0.665 | 0.040 |
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 8,959 | 36,635 | 0.808 | 0.102 |
| v4 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 15,475 | 13,175 | 0.785 | 0.134 |
| v4 | Synthetic identifiers in agentic formats | presidio-all | 38,580 | 31,142 | 0.471 | 0.078 |
| v4 | Synthetic identifiers in agentic formats | presidio-strong | 45,571 | 6,823 | 0.407 | 0.094 |
| v4 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 43,786 | 9,355 | 0.429 | 0.091 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 3,443 | 41,396 | 0.854 | 0.015 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 1,046 | 61,904 | 0.835 | 0.078 |
| v4 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 2,379 | 30,462 | 0.891 | 0.290 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-all | 18,047 | 29,918 | 0.710 | 0.374 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 16,067 | 25,514 | 0.743 | 0.422 |
| v4 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 15,855 | 26,356 | 0.744 | 0.421 |
| v4 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 28,788 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 63,916 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 31,772 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-all | 0 | 48,902 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-strong | 0 | 14,198 | 0.000 | 0.000 |
| v4 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 17,854 | 0.000 | 0.000 |
| v4 | Repeated PII values with decoys | Presidio (tuned by its authors) | 23,195 | 15,910 | 0.656 | 0.013 |
| v4 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 8,321 | 34,073 | 0.806 | 0.011 |
| v4 | Repeated PII values with decoys | Presidio (tuned here, F2) | 18,069 | 14,111 | 0.731 | 0.015 |
| v4 | Repeated PII values with decoys | presidio-all | 34,530 | 24,366 | 0.489 | 0.019 |
| v4 | Repeated PII values with decoys | presidio-strong | 47,219 | 2,796 | 0.321 | 0.022 |
| v4 | Repeated PII values with decoys | presidio-strong-high-recall | 45,023 | 4,902 | 0.355 | 0.021 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 24,228 | 18,430 | 0.665 | 0.040 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 8,959 | 36,635 | 0.808 | 0.102 |
| v3 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 15,475 | 13,175 | 0.785 | 0.134 |
| v3 | Synthetic identifiers in agentic formats | presidio-all | 38,580 | 31,142 | 0.471 | 0.078 |
| v3 | Synthetic identifiers in agentic formats | presidio-strong | 45,571 | 6,823 | 0.407 | 0.094 |
| v3 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 43,786 | 9,355 | 0.429 | 0.091 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 2,426 | 41,396 | 0.859 | 0.016 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 99 | 61,904 | 0.838 | 0.070 |
| v3 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 662 | 30,462 | 0.906 | 0.297 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-all | 15,775 | 29,918 | 0.724 | 0.384 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 13,143 | 25,514 | 0.767 | 0.433 |
| v3 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 12,931 | 26,356 | 0.768 | 0.432 |
| v3 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 28,788 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 63,916 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 31,772 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-all | 0 | 48,902 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-strong | 0 | 14,198 | 0.000 | 0.000 |
| v3 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 17,854 | 0.000 | 0.000 |
| v3 | Repeated PII values with decoys | Presidio (tuned by its authors) | 23,195 | 15,910 | 0.656 | 0.013 |
| v3 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 8,321 | 34,073 | 0.806 | 0.011 |
| v3 | Repeated PII values with decoys | Presidio (tuned here, F2) | 18,069 | 14,111 | 0.731 | 0.015 |
| v3 | Repeated PII values with decoys | presidio-all | 34,530 | 24,366 | 0.489 | 0.019 |
| v3 | Repeated PII values with decoys | presidio-strong | 47,219 | 2,796 | 0.321 | 0.022 |
| v3 | Repeated PII values with decoys | presidio-strong-high-recall | 45,023 | 4,902 | 0.355 | 0.021 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 24,228 | 18,430 | 0.665 | 0.040 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 8,959 | 36,635 | 0.808 | 0.102 |
| v2 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 15,475 | 13,175 | 0.785 | 0.134 |
| v2 | Synthetic identifiers in agentic formats | presidio-all | 38,580 | 31,142 | 0.471 | 0.078 |
| v2 | Synthetic identifiers in agentic formats | presidio-strong | 45,571 | 6,823 | 0.407 | 0.094 |
| v2 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 43,786 | 9,355 | 0.429 | 0.091 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 2,426 | 41,396 | 0.859 | 0.016 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 99 | 61,904 | 0.838 | 0.070 |
| v2 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 662 | 30,462 | 0.906 | 0.297 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-all | 15,775 | 29,918 | 0.724 | 0.384 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 13,143 | 25,514 | 0.767 | 0.433 |
| v2 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 12,931 | 26,356 | 0.768 | 0.432 |
| v2 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 28,788 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 63,916 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 31,772 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-all | 0 | 48,902 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-strong | 0 | 14,198 | 0.000 | 0.000 |
| v2 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 17,854 | 0.000 | 0.000 |
| v2 | Repeated PII values with decoys | Presidio (tuned by its authors) | 23,195 | 15,910 | 0.656 | 0.013 |
| v2 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 8,321 | 34,073 | 0.806 | 0.011 |
| v2 | Repeated PII values with decoys | Presidio (tuned here, F2) | 18,069 | 14,111 | 0.731 | 0.015 |
| v2 | Repeated PII values with decoys | presidio-all | 34,530 | 24,366 | 0.489 | 0.019 |
| v2 | Repeated PII values with decoys | presidio-strong | 47,219 | 2,796 | 0.321 | 0.022 |
| v2 | Repeated PII values with decoys | presidio-strong-high-recall | 45,023 | 4,902 | 0.355 | 0.021 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned by its authors) | 24,228 | 18,430 | 0.665 | 0.040 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned here, leak-first) | 8,959 | 36,635 | 0.808 | 0.102 |
| v1 | Synthetic identifiers in agentic formats | Presidio (tuned here, F2) | 15,475 | 13,175 | 0.785 | 0.134 |
| v1 | Synthetic identifiers in agentic formats | presidio-all | 38,580 | 31,142 | 0.471 | 0.078 |
| v1 | Synthetic identifiers in agentic formats | presidio-strong | 45,571 | 6,823 | 0.407 | 0.094 |
| v1 | Synthetic identifiers in agentic formats | presidio-strong-high-recall | 43,786 | 9,355 | 0.429 | 0.091 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned by its authors) | 3,443 | 41,396 | 0.854 | 0.015 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, leak-first) | 1,046 | 61,904 | 0.835 | 0.078 |
| v1 | Kiji EN/DE holdout and A4 negatives | Presidio (tuned here, F2) | 2,379 | 30,462 | 0.891 | 0.290 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-all | 18,047 | 29,918 | 0.710 | 0.374 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong | 16,067 | 25,514 | 0.743 | 0.422 |
| v1 | Kiji EN/DE holdout and A4 negatives | presidio-strong-high-recall | 15,855 | 26,356 | 0.744 | 0.421 |
| v1 | Synthetic benign lookalikes | Presidio (tuned by its authors) | 0 | 28,788 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | Presidio (tuned here, leak-first) | 0 | 63,916 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | Presidio (tuned here, F2) | 0 | 31,772 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-all | 0 | 48,902 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-strong | 0 | 14,198 | 0.000 | 0.000 |
| v1 | Synthetic benign lookalikes | presidio-strong-high-recall | 0 | 17,854 | 0.000 | 0.000 |
| v1 | Repeated PII values with decoys | Presidio (tuned by its authors) | 23,195 | 15,910 | 0.656 | 0.013 |
| v1 | Repeated PII values with decoys | Presidio (tuned here, leak-first) | 8,321 | 34,073 | 0.806 | 0.011 |
| v1 | Repeated PII values with decoys | Presidio (tuned here, F2) | 18,069 | 14,111 | 0.731 | 0.015 |
| v1 | Repeated PII values with decoys | presidio-all | 34,530 | 24,366 | 0.489 | 0.019 |
| v1 | Repeated PII values with decoys | presidio-strong | 47,219 | 2,796 | 0.321 | 0.022 |
| v1 | Repeated PII values with decoys | presidio-strong-high-recall | 45,023 | 4,902 | 0.355 | 0.021 |

Against the latest Gaze release (v0.17.0) on all of Kiji EN/DE holdout and A4 negatives. This table covers layer C. The tuned-here rows include the validation half they were selected on, which can only flatter them. False positives are after v4's gold-gap credit.

| Contract | Configuration | Leaked B | FP B | Char F2 |
| --- | --- | ---: | ---: | ---: |
| v4 | Gaze v0.17.0 | 9,717 | 15,224 | 0.901 |
| v4 | Presidio (tuned by its authors) | 6,748 | 74,113 | 0.851 |
| v4 | Presidio (tuned here, leak-first) | 2,079 | 111,220 | 0.837 |
| v4 | Presidio (tuned here, F2) | 4,782 | 50,513 | 0.889 |
| v3 | Gaze v0.17.0 | 6,652 | 15,224 | n/a |
| v3 | Presidio (tuned by its authors) | 4,752 | 74,113 | n/a |
| v3 | Presidio (tuned here, leak-first) | 102 | 111,220 | n/a |
| v3 | Presidio (tuned here, F2) | 1,281 | 50,513 | n/a |
| v2 | Gaze v0.17.0 | 6,652 | 27,004 | n/a |
| v2 | Presidio (tuned by its authors) | 4,752 | 80,797 | n/a |
| v2 | Presidio (tuned here, leak-first) | 102 | 116,019 | n/a |
| v2 | Presidio (tuned here, F2) | 1,281 | 58,804 | n/a |
| v1 | Gaze v0.17.0 | 9,717 | 27,021 | n/a |
| v1 | Presidio (tuned by its authors) | 6,748 | 80,797 | n/a |
| v1 | Presidio (tuned here, leak-first) | 2,079 | 116,019 | n/a |
| v1 | Presidio (tuned here, F2) | 4,782 | 58,804 | n/a |

Where tuned Presidio beats Gaze v0.17.0 here: Presidio (tuned by its authors) leaks fewer v4 bytes; Presidio (tuned here, leak-first) leaks fewer v4 bytes; Presidio (tuned here, F2) leaks fewer v4 bytes; Presidio (tuned by its authors) leaks fewer v3 bytes; Presidio (tuned here, leak-first) leaks fewer v3 bytes; Presidio (tuned here, F2) leaks fewer v3 bytes; Presidio (tuned by its authors) leaks fewer v2 bytes; Presidio (tuned here, leak-first) leaks fewer v2 bytes; Presidio (tuned here, F2) leaks fewer v2 bytes; Presidio (tuned by its authors) leaks fewer v1 bytes; Presidio (tuned here, leak-first) leaks fewer v1 bytes; Presidio (tuned here, F2) leaks fewer v1 bytes.

Live check: each chosen configuration also ran live on a fixed sample (about one document in eight, every layer, both halves); Presidio (tuned here, F2): 2,228 documents, 4 differed on the first run, 4 on every rerun (4 with identical bytes and a different entity label, from Presidio's tie-break between equal-score results on one span); Presidio (tuned here, leak-first): 2,228 documents, 7 differed on the first run, 7 on every rerun (7 with identical bytes and a different entity label, from Presidio's tie-break between equal-score results on one span).

The panels' Presidio bar is **Presidio (tuned here, F2)**, the highest test-half v3 layer C character F2 of the three: Presidio (tuned here, F2) 0.906, Presidio (tuned here, leak-first) 0.838, Presidio (tuned by its authors) 0.859. The default rows above stay unchanged.

Aggregate source: [`presidio-tuned.json`](presidio-tuned.json); reproduce with [`tuned_presidio/README.md`](../../../scripts/bench/compare/tuned_presidio/README.md).
