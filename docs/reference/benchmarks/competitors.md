# Competitor comparison

Same corpus and scorer; tools run with documented configurations. UTF-8 byte counts use the Gaze scorer. For v3, FP is the scorer's false-positive count after its audited gold-gap credit. CPU-host p50/p95 is warm per-document wall-clock inference/clean time on the same machine. Presidio all runs English, German, Dutch, French, and Portuguese spaCy models with the documented German recognizers. Presidio English default is a secondary row. Latency includes processed documents only. Gaze p50 exceeds Presidio all in 12/12 comparable layer-contract rows on this host. This measures detection; competitor restore and manifest behavior is not scored.

Leaked and false-positive byte counts are class-agnostic. A skipped document's scored gold counts in full as leaked. Subtract Skipped gold B from Leaked B to get leakage on processed documents. For example, Presidio English-only v3 A leaks 34,733 B, including 28,151 B of scored gold from 1,320 skipped non-English documents. The reviewed label map affects only v3's repeated-gold credit.

Gaze measured at `b77dce0e42b9be464d369d1984215792d6133d1a` (release `v0.15.1`). Home-normalized setup policy SHA-256: `481f5df7a9b0b562bf2c2db7274cfbf2ea701231c52f1287b27de00554453ff1`. The measured call scopes differ by tool, so latency is descriptive.

Competitor runtimes: Presidio 2.2.364 with spaCy 3.7.5; GLiNER 0.2.29 at model snapshot `1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d`. GLiNER uses model-card labels, library threshold 0.5. OpenAI Privacy Filter (OPF) 0.1.0 at source `f7f00ca7fb869683eb732c010299d901457f19c3`.

Aggregate source: [`comparison.json`](comparison.json). Raw document outputs are not published.

| Contract | Layer | Tool | Leaked B | FP B | Processed | Skipped | Skipped gold B | CPU-host p50 ms | CPU-host p95 ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| v3 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | 22.5 | 29.1 |
| v3 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | 5.3 | 7.4 |
| v3 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | 5.1 | 7.2 |
| v3 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | 5.3 | 7.3 |
| v3 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | 128.0 | 132.7 |
| v3 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | 175.4 | 245.8 |
| v3 | C | gaze | 9,256 | 15,560 | 2,910 | 0 | 0 | 59.3 | 127.2 |
| v3 | C | presidio-all | 30,615 | 50,429 | 2,910 | 0 | 0 | 13.7 | 24.9 |
| v3 | C | presidio-en | 71,562 | 26,086 | 1,545 | 1,365 | 54,629 | 12.9 | 24.8 |
| v3 | C | presidio-en-de | 30,615 | 50,429 | 2,910 | 0 | 0 | 13.7 | 25.0 |
| v3 | C | gliner | 20,152 | 51,384 | 2,910 | 0 | 0 | 152.6 | 194.2 |
| v3 | C | opf | 24,080 | 27,544 | 2,910 | 0 | 0 | 554.1 | 1060.5 |
| v3 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | 18.2 | 26.7 |
| v3 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | 4.9 | 7.5 |
| v3 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | 4.7 | 7.0 |
| v3 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | 4.8 | 7.4 |
| v3 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | 128.1 | 133.2 |
| v3 | D | opf | 0 | 5,589 | 840 | 0 | 0 | 152.9 | 223.7 |
| v3 | R | gaze | 234 | 419 | 100 | 0 | 0 | 37.7 | 67.8 |
| v3 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | 8.0 | 14.5 |
| v3 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | 7.7 | 9.1 |
| v3 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | 7.9 | 14.3 |
| v3 | R | gliner | 478 | 432 | 100 | 0 | 0 | 138.6 | 153.9 |
| v3 | R | opf | 182 | 956 | 100 | 0 | 0 | 263.6 | 631.5 |
| v2 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | 22.5 | 29.1 |
| v2 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | 5.3 | 7.4 |
| v2 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | 5.1 | 7.2 |
| v2 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | 5.3 | 7.3 |
| v2 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | 128.0 | 132.7 |
| v2 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | 175.4 | 245.8 |
| v2 | C | gaze | 9,256 | 27,282 | 2,910 | 0 | 0 | 59.2 | 126.5 |
| v2 | C | presidio-all | 30,615 | 58,274 | 2,910 | 0 | 0 | 13.7 | 24.9 |
| v2 | C | presidio-en | 71,562 | 30,749 | 1,545 | 1,365 | 54,629 | 12.9 | 24.8 |
| v2 | C | presidio-en-de | 30,615 | 58,274 | 2,910 | 0 | 0 | 13.7 | 25.0 |
| v2 | C | gliner | 20,152 | 53,355 | 2,910 | 0 | 0 | 152.6 | 194.2 |
| v2 | C | opf | 24,080 | 31,254 | 2,910 | 0 | 0 | 554.1 | 1060.5 |
| v2 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | 18.2 | 26.9 |
| v2 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | 4.9 | 7.5 |
| v2 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | 4.7 | 7.0 |
| v2 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | 4.8 | 7.4 |
| v2 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | 128.1 | 133.2 |
| v2 | D | opf | 0 | 5,589 | 840 | 0 | 0 | 152.9 | 223.7 |
| v2 | R | gaze | 234 | 419 | 100 | 0 | 0 | 37.6 | 68.0 |
| v2 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | 8.0 | 14.5 |
| v2 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | 7.7 | 9.1 |
| v2 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | 7.9 | 14.3 |
| v2 | R | gliner | 478 | 432 | 100 | 0 | 0 | 138.6 | 153.9 |
| v2 | R | opf | 182 | 956 | 100 | 0 | 0 | 263.6 | 631.5 |
| v1 | A | gaze | 5,738 | 882 | 2,310 | 0 | 0 | 22.4 | 29.0 |
| v1 | A | presidio-all | 21,837 | 10,760 | 2,310 | 0 | 0 | 5.3 | 7.4 |
| v1 | A | presidio-en | 34,733 | 4,075 | 990 | 1,320 | 28,151 | 5.1 | 7.2 |
| v1 | A | presidio-en-de | 27,307 | 8,373 | 1,750 | 560 | 11,520 | 5.3 | 7.3 |
| v1 | A | gliner | 13,085 | 5,934 | 2,310 | 0 | 0 | 128.0 | 132.7 |
| v1 | A | opf | 10,508 | 5,525 | 2,310 | 0 | 0 | 175.4 | 245.8 |
| v1 | C | gaze | 15,443 | 27,282 | 2,910 | 0 | 0 | 59.2 | 126.3 |
| v1 | C | presidio-all | 35,130 | 58,274 | 2,910 | 0 | 0 | 13.7 | 24.9 |
| v1 | C | presidio-en | 76,658 | 30,749 | 1,545 | 1,365 | 57,835 | 12.9 | 24.8 |
| v1 | C | presidio-en-de | 35,130 | 58,274 | 2,910 | 0 | 0 | 13.7 | 25.0 |
| v1 | C | gliner | 26,515 | 53,355 | 2,910 | 0 | 0 | 152.6 | 194.2 |
| v1 | C | opf | 25,485 | 31,383 | 2,910 | 0 | 0 | 554.1 | 1060.5 |
| v1 | D | gaze | 0 | 3,544 | 840 | 0 | 0 | 18.2 | 26.7 |
| v1 | D | presidio-all | 0 | 7,983 | 840 | 0 | 0 | 4.9 | 7.5 |
| v1 | D | presidio-en | 0 | 6,346 | 660 | 180 | 0 | 4.7 | 7.0 |
| v1 | D | presidio-en-de | 0 | 7,752 | 810 | 30 | 0 | 4.8 | 7.4 |
| v1 | D | gliner | 0 | 1,684 | 840 | 0 | 0 | 128.1 | 133.2 |
| v1 | D | opf | 0 | 5,589 | 840 | 0 | 0 | 152.9 | 223.7 |
| v1 | R | gaze | 234 | 419 | 100 | 0 | 0 | 37.4 | 67.3 |
| v1 | R | presidio-all | 442 | 1,278 | 100 | 0 | 0 | 8.0 | 14.5 |
| v1 | R | presidio-en | 2,961 | 222 | 45 | 55 | 2,909 | 7.7 | 9.1 |
| v1 | R | presidio-en-de | 442 | 1,278 | 100 | 0 | 0 | 7.9 | 14.3 |
| v1 | R | gliner | 478 | 432 | 100 | 0 | 0 | 138.6 | 153.9 |
| v1 | R | opf | 182 | 956 | 100 | 0 | 0 | 263.6 | 631.5 |

**Where Gaze leaks more under v3:** R: opf leaks 182 B versus Gaze 234 B.

**Where a competitor has fewer false positives at equal v3 leakage:** D: gliner has 1,684 FP B versus Gaze 3,544 FP B.
