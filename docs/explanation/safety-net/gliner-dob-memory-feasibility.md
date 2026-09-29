# GLiNER DOB judge memory feasibility

The GLiNER DOB judge stays opt-in. The tested pruning recipes did not establish
both unchanged decisions and at most 400 MiB of added peak RSS. `gaze setup`
and its bundle pin are unchanged.

## Structural attribution

The pinned ONNX model is 349,120,924 bytes. Its quantized embedding is a
250,105 × 768 byte initializer: 192,080,640 bytes (183.2 MiB). Other model
bytes total 149.8 MiB. The initializer feeds `Gather` and then
`DequantizeLinear`; ONNX Runtime does not expand the entire embedding into a
float initializer during graph optimization.

Single-threaded ONNX Runtime 1.24.2 load probes on this Apple M5 Max measured
1,004 MiB of added process RSS for the original model and 493 MiB for a
diagnostic model with only one embedding row. Thus the embedding accounts for
about 511 MiB of load RSS in this probe, while the rest of the model and
runtime account for about 493 MiB. The one-row model is load-only and cannot
run valid inputs. RSS attribution ran on a busy host, because peak RSS does not depend on CPU
load; the recorded `uptime` load average was 11.25/9.77/9.28.

## Tested candidates

All load probes used ONNX Runtime 1.24.2 with one intra-op thread and graph
optimization enabled. These are process load deltas, not the Gaze pipeline's
peak RSS. The [shipped Gaze mechanism test](../../reference/benchmarks/mechanisms/gliner-dob-judge-latency.json)
measured 1,076 → 1,740 MiB, an added 664 MiB, on a host that was quiet when the run started.

| Model | Load RSS added | Decision evidence |
| --- | ---: | --- |
| Original pinned ONNX | 1,004 MiB | Baseline |
| BASIC pre-optimized ONNX, full vocabulary | 802 MiB | Identical scores on 33 held-out spans and 11 multilingual probes |
| BASIC + 20,000-piece ranked tail cut, retaining single-character pieces | 789 MiB | Same held-out and probe decisions, but held-out scores differ |
| BASIC + 30,000-piece ranked tail cut, retaining single-character pieces | 775 MiB | Held-out P1 decision changes |
| 138,726-row broader prune without BASIC optimization | 840 MiB | Held-out P1, P3, P7 and Italian/Turkish probe decisions change |

The 20,000-piece cut retains 241,617 embedding rows and saves only about
13 MiB of load RSS beyond the full-vocabulary BASIC model. In the primary
corpus, 45 of 294 documents containing scanner-shaped dates have words whose
original token pieces this cut removes. Byte-identical observation records
are therefore unproven. The independent 30-line held-out TSV is kept outside the
repository so rules are not tuned to it; its SHA-256 is
`493ae864f8d545f490a2d6d4290ec92bf94356b6dedba1498452b67d3b412d70`.
The 11 probes cover de/fr/es/it/pt/nl/pl/tr/ar/zh/ja dates in prose.

An ORT-format file loaded directly from model bytes reduced the standalone
load delta to 644 MiB, but changed the held-out result from seven positive
emissions to zero. Loading that ORT file by path also changed the decisions
and added 985 MiB of RSS. Loading the judge on demand would defer allocation
but cannot reduce peak RSS once a date-bearing document requires the model.

The existing quiet-host latency evidence remains the only valid latency
measurement: p95 +22.5 ms and cold first document +1.6 s. No candidate
latency number was taken. A direct Gaze runtime RSS comparison and full
benchmark observation comparison remain necessary before any bundle or
default-policy change.

[The experiment script](../../../scripts/models/experiment_gliner_dob_prune.py)
reproduces the 20,000-piece candidate from the pinned source. It does not
affect installation or runtime.
