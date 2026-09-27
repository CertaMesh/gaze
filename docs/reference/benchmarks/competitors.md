# Competitor comparison

Same corpus and scorer; tools run with their documented defaults. UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's false-positive count after its audited gold-gap credit. CPU-host p50/p95 is warm per-document wall-clock inference/clean time on the same machine. Presidio English default leaves non-English documents unprocessed; the English/German row leaves Dutch, French, and Portuguese documents unprocessed. A 0.0 ms median can therefore mean a skipped language. This measures detection; competitor restore and manifest behavior is not scored.

Gaze revision: `9a0016bd15afa83b2cec9fe9a14befa8f454efae`; setup policy SHA-256: `f909a23aecacc5695388223be5e71bc1e303c845563396d6658448396a0a9ebe`. The measured call scopes differ by tool, so latency is descriptive.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`; OPF 0.1.0 at source `f7f00ca7fb869683eb732c010299d901457f19c3`.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published.

| Contract | Layer | Tool | Leaked B | FP B | CPU-host p50 ms | CPU-host p95 ms |
|---|---|---|---:|---:|---:|---:|
| v3 | A | gaze | 16,815 | 902 | 23.7 | 32.7 |
| v3 | A | presidio-en | 34,733 | 4,075 | 0.0 | 6.6 |
| v3 | A | presidio-en-de | 27,307 | 8,373 | 4.9 | 7.3 |
| v3 | A | gliner | 13,085 | 5,934 | 103.2 | 109.5 |
| v3 | A | opf | 10,508 | 5,525 | 173.5 | 246.0 |
| v3 | C | gaze | 11,804 | 13,832 | 66.5 | 154.9 |
| v3 | C | presidio-en | 71,562 | 26,086 | 6.5 | 22.9 |
| v3 | C | presidio-en-de | 31,944 | 49,784 | 13.8 | 25.1 |
| v3 | C | gliner | 20,152 | 51,384 | 133.7 | 227.9 |
| v3 | C | opf | 24,080 | 27,544 | 555.3 | 1062.6 |
| v3 | D | gaze | 0 | 3,544 | 19.3 | 32.0 |
| v3 | D | presidio-en | 0 | 6,346 | 4.5 | 7.0 |
| v3 | D | presidio-en-de | 0 | 7,752 | 4.9 | 7.4 |
| v3 | D | gliner | 0 | 1,684 | 103.2 | 108.8 |
| v3 | D | opf | 0 | 5,589 | 155.2 | 221.7 |
| v3 | R | gaze | 234 | 419 | 37.9 | 71.5 |
| v3 | R | presidio-en | 2,961 | 222 | 0.0 | 9.1 |
| v3 | R | presidio-en-de | 618 | 1,278 | 8.0 | 14.4 |
| v3 | R | gliner | 478 | 432 | 114.6 | 129.2 |
| v3 | R | opf | 182 | 956 | 266.3 | 624.9 |
| v2 | A | gaze | 16,815 | 902 | 44.7 | 97.3 |
| v2 | A | presidio-en | 34,733 | 4,075 | 0.0 | 6.6 |
| v2 | A | presidio-en-de | 27,307 | 8,373 | 4.9 | 7.3 |
| v2 | A | gliner | 13,085 | 5,934 | 103.2 | 109.5 |
| v2 | A | opf | 10,508 | 5,525 | 173.5 | 246.0 |
| v2 | C | gaze | 11,804 | 25,554 | 61.4 | 130.1 |
| v2 | C | presidio-en | 71,562 | 30,749 | 6.5 | 22.9 |
| v2 | C | presidio-en-de | 31,944 | 57,590 | 13.8 | 25.1 |
| v2 | C | gliner | 20,152 | 53,355 | 133.7 | 227.9 |
| v2 | C | opf | 24,080 | 31,254 | 555.3 | 1062.6 |
| v2 | D | gaze | 0 | 3,544 | 18.6 | 27.1 |
| v2 | D | presidio-en | 0 | 6,346 | 4.5 | 7.0 |
| v2 | D | presidio-en-de | 0 | 7,752 | 4.9 | 7.4 |
| v2 | D | gliner | 0 | 1,684 | 103.2 | 108.8 |
| v2 | D | opf | 0 | 5,589 | 155.2 | 221.7 |
| v2 | R | gaze | 234 | 419 | 38.1 | 69.0 |
| v2 | R | presidio-en | 2,961 | 222 | 0.0 | 9.1 |
| v2 | R | presidio-en-de | 618 | 1,278 | 8.0 | 14.4 |
| v2 | R | gliner | 478 | 432 | 114.6 | 129.2 |
| v2 | R | opf | 182 | 956 | 266.3 | 624.9 |
| v1 | A | gaze | 16,815 | 902 | 23.1 | 29.9 |
| v1 | A | presidio-en | 34,733 | 4,075 | 0.0 | 6.6 |
| v1 | A | presidio-en-de | 27,307 | 8,373 | 4.9 | 7.3 |
| v1 | A | gliner | 13,085 | 5,934 | 103.2 | 109.5 |
| v1 | A | opf | 10,508 | 5,525 | 173.5 | 246.0 |
| v1 | C | gaze | 17,991 | 25,554 | 61.9 | 135.5 |
| v1 | C | presidio-en | 76,658 | 30,749 | 6.5 | 22.9 |
| v1 | C | presidio-en-de | 36,459 | 57,590 | 13.8 | 25.1 |
| v1 | C | gliner | 26,515 | 53,355 | 133.7 | 227.9 |
| v1 | C | opf | 25,485 | 31,383 | 555.3 | 1062.6 |
| v1 | D | gaze | 0 | 3,544 | 18.8 | 27.4 |
| v1 | D | presidio-en | 0 | 6,346 | 4.5 | 7.0 |
| v1 | D | presidio-en-de | 0 | 7,752 | 4.9 | 7.4 |
| v1 | D | gliner | 0 | 1,684 | 103.2 | 108.8 |
| v1 | D | opf | 0 | 5,589 | 155.2 | 221.7 |
| v1 | R | gaze | 234 | 419 | 39.3 | 69.9 |
| v1 | R | presidio-en | 2,961 | 222 | 0.0 | 9.1 |
| v1 | R | presidio-en-de | 618 | 1,278 | 8.0 | 14.4 |
| v1 | R | gliner | 478 | 432 | 114.6 | 129.2 |
| v1 | R | opf | 182 | 956 | 266.3 | 624.9 |

**Where Gaze leaks more under v3:** A: gliner leaks 13,085 B versus Gaze 16,815 B; A: opf leaks 10,508 B versus Gaze 16,815 B; R: opf leaks 182 B versus Gaze 234 B.

**Where a competitor has fewer false positives at equal v3 leakage:** D: gliner has 1,684 FP B versus Gaze 3,544 FP B.
