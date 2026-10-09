# Competitor comparison on Gaze's corpus

`compare.py` loads canonical C/A/D/R documents and scores every row through
`gaze_bench_score.MetricAccumulator` and `comparison_metrics.ComparisonMetrics`.
Only aggregate JSON is published. Raw text stays in memory.
`GAZE_COMPARE_PREDICTIONS_DIR` stores native spans and labels, without document
text, outside the repository for exact offline `--rescore-predictions` runs.
Product coverage scores unsupported gold as leaked. Common
intersection scores only canonical labels claimed by every configuration; the
intersection and each native-to-canonical mapping are in the report. The full
product score has v1/v2/v3/v4 byte semantics, including v3/v4 gold-gap
credit. Additional metrics are document leak rate (PII-bearing documents with
at least one leaked byte), leaked-entity rate (gold entities with at least one
uncovered byte), redaction load (predicted bytes / scored document bytes), and
exact typed-span precision/recall/F1/F2 with raw TP/FP/FN.
The byte scorer keeps the reviewed native mapping. Exact typed scoring gives
ambiguous `custom:family:*` labels no typed credit; Gaze password/token and
OPF secret labels can match PASSWORD/SECURITYTOKEN under v1 and v4. The public
page lists every metric cell where a competitor scores better than Gaze, with
overlapping views and splits identified as such.

The SHA-256 first byte of each stable document ID defines a fixed validation
half (`<128`) and disjoint test half. Candidate thresholds are declared in
`compare.py`: Presidio strong 0.3 vs high-recall 0.0; GLiNER default 0.5 vs
high-recall 0.3. The chosen row minimizes validation v3 leaked bytes, then
validation false-positive bytes, summed over C/A/D/R. The report records both
candidate results and the choice. Comparative claims use **test** metrics and
name the corpus, contract, configuration, mapping, and split. No Gaze setting
is selected or changed using these results. Full-corpus byte rows remain for
continuity with the benchmark headline.

The root README chart uses exactly the configurations declared in
`chart-configs.json`. Its bars show full-corpus layer C under the same scored
label contract. Swept thresholds never enter that chart; their validation
choice and disjoint test results stay in `competitors.md`.

## Configurations

- Presidio 2.2.364: original English, English/German, and five-language defaults;
  strong runs English transformer NER (`dslim/bert-base-NER`) with Presidio's
  default context enhancement and all applicable predefined recognizers. The
  other four languages retain their pinned spaCy large models; German includes
  the nine documented recognizers. The raw-coordinate resolver is pinned to
  `presidio-anonymizer==2.2.364` because public anonymization can shift offsets.
- DataFog Core 0.3.0: built-in text detectors only. DataFog Python 4.8.1:
  separate regex, spaCy `en_core_web_lg`, and GLiNER
  `urchade/gliner_multi_pii-v1` engines. German regex locales are enabled for
  German documents. DataFog's spaCy model is English-only.
- scrubadub 2.0.0: autoloaded built-ins; a second row adds
  `scrubadub-spacy==2.0.0` with pinned `en_core_web_lg` for English documents.
  The plugin's own locale defaults and detector selection apply.
- Standalone GLiNER: model-card labels, default 0.5 and high-recall 0.3.
  OpenAI Privacy Filter 0.1.0: vendor default calibrated Viterbi decoder; no
  public threshold is tuned. Its source revision and checkpoint digest are
  recorded, and it is explicitly skipped only if no local runtime is supplied.
- Gaze: rules only, rules plus NER, and full setup. The policy variants are
  derived mechanically with `prepare_policies.py`. The full row is re-inferred
  once, scored through the same metric path, and its v1/v2/v3 C/A/D/R byte
  counts must exactly match the committed report when the crates tree is the
  same. `gaze_main_revision` records the matching main commit.

## Reproduce

Use Python 3.12. `requirements.lock` and `requirements-scrubadub.lock` pin
package hashes in separate environments because their Transformers constraints
conflict. Install each with `uv pip sync --require-hashes`. The spaCy model
wheel URLs and SHA-256 hashes are in `model-wheels.json`; install those wheels
in the relevant environment. The report records the model content tree hash,
wheel hash, and version. Set `HF_HUB_OFFLINE=1` after prefetched model snapshots
are present. The GLiNER checkpoint and its separate mDeBERTa tokenizer snapshot
must both be pinned by revision and tree SHA-256. The Presidio transformer
snapshot is pinned the same way. OPF is installed from a clean, recorded source
revision and uses its locally pinned checkpoint.

```sh
cargo run -p gaze-cli -- setup --non-interactive --force \
  --policy-out target/bench-data/compare-3909/policy.toml
python3.12 scripts/bench/compare/prepare_policies.py \
  target/bench-data/compare-3909/policy.toml
cargo build -q -p gaze-recognizers --example clean_for_bench \
  --features safety-net-nym --release
```

Set the paths consumed by `run-full.sh`:

```text
GAZE_COMPARE_PYTHON, GAZE_COMPARE_SCRUB_PYTHON, GAZE_COMPARE_DATASET,
GAZE_COMPARE_EN_MODEL, GAZE_COMPARE_DE_MODEL, GAZE_COMPARE_NL_MODEL,
GAZE_COMPARE_FR_MODEL, GAZE_COMPARE_PT_MODEL, GAZE_COMPARE_GLINER_MODEL,
GAZE_COMPARE_GLINER_TOKENIZER, GAZE_COMPARE_TRANSFORMER_MODEL,
GAZE_COMPARE_BINARY, GAZE_COMPARE_MODEL_DIR, GAZE_COMPARE_POLICY,
GAZE_COMPARE_POLICY_RULES, GAZE_COMPARE_POLICY_RULES_NER,
GAZE_COMPARE_OPF_PYTHON, GAZE_COMPARE_OPF_CHECKPOINT, GAZE_COMPARE_OUTPUT,
GAZE_COMPARE_PREDICTIONS_DIR (absolute path outside this repository)
```

Then run `scripts/bench/compare/run-full.sh` on a quiet CPU host. It runs each
configuration sequentially and resumes the same aggregate report across the
two Python environments. `finalize_report.py` checks the complete roster,
hashes the declared chart selection, and marks timing unverified because this
comparison is not a dedicated quiet-machine timing run. The comparator samples
CPU outside its process tree every five seconds and records the busy-process
count and load1 before/after. A sample above one core marks that configuration
contended. The public page withholds p50/p95 and timing comparisons; a separate
quiet-machine timing run can use the sampler to publish speed claims.
`--dry-run` checks arguments without loading data.
To re-derive metrics after a scorer-only fix, run `compare.py --dataset
target/bench-data/dataiku-en-de/test.parquet --output
target/bench-data/compare-3909/comparison.json --rescore-predictions
/absolute/local/predictions`. Replay verifies document order and every byte
aggregate before replacing typed metrics; it never calls a model.
Copy the report to `docs/reference/benchmarks/comparison.json`, render
`competitors.md`, and run `render.py --check`. The renderer verifies input
hashes, model pins, tool roster, and document counts. The report records split
ID digests for every layer and both halves.

## PII-Tracer

Perplexity PII-Tracer (MIT, model revision `d25c16f2e57e321f6d2527715c01df9112f956f5`)
is measured by `pii_tracer.py`, not `compare.py`: the third-party benchmark runner
pins `compare.py` byte for byte, so the adapter reuses `compare.measure` from beside
it and writes its own aggregate report. The checkpoint needs `transformers>=5.2`, so a
separate environment (`requirements-pii-tracer.lock`, Python 3.12, `uv pip sync
--require-hashes`) runs `pii_tracer_worker.py` as a JSONL subprocess.

The checkpoint's `modeling_pii_masking.py` is executed by `trust_remote_code`. It
imports only `torch` and `transformers` and opens no network connection. The worker
refuses to import it unless the SHA-256 of that file, the weights, the config and both
tokenizer files equal the pins in `pii_tracer_worker.py`, and it runs with
`HF_HUB_OFFLINE=1`. Download once with `huggingface_hub.snapshot_download(
"perplexity-ai/PII-Tracer", revision=<pin>, local_dir=...)`.

The declared configuration was fixed from the model card before any result: CPU,
stored bf16, `predict()` decoding, and non-overlapping 4080-token windows for longer
documents (the card says to chunk; truncation would score the dropped tail as leaked).
The card documents no threshold, so there is no sweep. `other_pii` and `secret` follow
the OPF treatment in `pii-tracer-label-map.json`, which explains every row.

```sh
python3.12 scripts/bench/compare/pii_tracer.py --dataset target/bench-data/dataiku-en-de/test.parquet \
  --pii-tracer-python <venv>/bin/python --pii-tracer-model <snapshot dir> \
  --predictions-dir /absolute/path/outside/repo --output target/bench-data/compare-3909/pii-tracer.json
```

`--preflight N` runs N documents per layer and language and fails on any unmapped
native label; its output is never published. The third-party sets take the same
`--pii-tracer-python` / `--pii-tracer-model` flags on `theirbench.py`.

Presidio: https://github.com/data-privacy-stack/presidio/tree/main/docs/analyzer/nlp_engines
DataFog Core: https://github.com/DataFog/datafog-core
DataFog Python: https://github.com/DataFog/datafog-python
scrubadub: https://github.com/LeapBeyond/scrubadub/blob/master/docs/usage.rst
GLiNER PII model: https://huggingface.co/urchade/gliner_multi_pii-v1
OPF: https://github.com/openai/privacy-filter
PII-Tracer: https://huggingface.co/perplexity-ai/PII-Tracer
