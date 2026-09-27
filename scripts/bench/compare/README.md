# Competitor comparison

Run `compare.py` whenever the benchmark corpus or a variant pack changes. It
loads the same main documents and agentic generator as Gaze, then scores raw
UTF-8 byte spans with `gaze_bench_score.MetricAccumulator`. `--pack-dir` adds
every `*.jsonl` variant pack in a directory, including its `sealed` partition.
Pack rows have `id`, `partition`, `text`, `language`, `region`, `layer`, and
`gold` (`start`, `end`, `label` in UTF-8 bytes). The runner rejects duplicate
IDs, invalid bounds, unknown partitions, and unsupported contract labels.

The three contracts are scored from **one inference pass** per tool. The output
is aggregate JSON only; no document text, prediction, or per-document result is
written. Run each tool sequentially on a quiet CPU host and retain the output
with the release. `--tool all` runs Presidio English default, Presidio English
and German, GLiNER-PII, and OPF if its local runtime is present. OPF needs
`--opf-python` and `--opf-checkpoint`; its daemon stays private to the run.

Use Python 3.12 and install `requirements.lock` into a session-local virtual
environment. Install the spaCy model wheels separately; this run uses
`en_core_web_lg` 3.7.1 and `de_core_news_lg` 3.7.0. OPF 0.1.0 uses its own
Python environment. The script accepts locally installed model paths and
records their content hashes, so a model swap cannot silently inherit an old
row. Provide each Gaze `scorecard-v4.json` with the matching
`--gaze-scorecard-v1`, `--gaze-scorecard-v2`, and `--gaze-scorecard-v3` flags;
the script checks the main document IDs, dataset hashes, agentic generator
hash, contract hash, and per-layer document counts before presenting Gaze rows.

Example:

```sh
python scripts/bench/compare/compare.py \
  --dataset target/bench-data/dataiku-en-de/test.parquet \
  --en-model /path/to/en_core_web_lg \
  --de-model /path/to/de_core_news_lg \
  --gliner-model /path/to/gliner_multi_pii-v1 \
  --opf-python /path/to/opf-venv/bin/python \
  --opf-checkpoint /path/to/privacy_filter \
  --gaze-scorecard-v1 target/bench-data/gaze-v1/scorecard-v4.json \
  --gaze-scorecard-v2 target/bench-data/gaze-v2/scorecard-v4.json \
  --gaze-scorecard-v3 target/bench-data/gaze-v3/scorecard-v4.json \
  --output target/bench-data/comparison.json
```

Copy only the aggregate JSON into `docs/reference/benchmarks/`, then render its
page with `python scripts/bench/compare/render.py
docs/reference/benchmarks/comparison.json --page
docs/reference/benchmarks/competitors.md`. The renderer requires all three Gaze
contracts. If a variant pack is present, each Gaze scorecard must also carry
the identical pack layer and document-ID digest; otherwise comparison fails.

The model paths point at installed model directories. The script records the
SHA-256 of all model files, installed package versions, dataset and pack
digests, and per-layer document-ID digests. Review `label-map.json` when a
competitor emits a new label. All prediction spans count byte-for-byte against
gold regardless of class. The mapping only controls contract v3's repeated-gold
credit, under the same rules Gaze uses.

Presidio's default NLP model and recognizers are English only. The English
default row leaves German documents unprotected. The multilingual row uses
Presidio's documented multiple-model configuration with `de_core_news_lg` and
language-specific built-in recognizers; it still runs on the same documents.
Presidio's analyzer output goes through its anonymizer with the default
replacement operator, and scoring uses the resolved original spans that the
anonymizer processes. GLiNER uses the model card's listed labels and its
library's default threshold. OPF uses its default typed output and CPU decode.

Sources and licenses: [Presidio MIT](https://github.com/data-privacy-stack/presidio/blob/main/LICENSE),
[Presidio language configuration](https://github.com/data-privacy-stack/presidio/blob/main/docs/analyzer/languages.md),
[GLiNER Apache-2.0](https://github.com/urchade/GLiNER/blob/main/pyproject.toml),
[GLiNER-PII model card Apache-2.0](https://huggingface.co/urchade/gliner_multi_pii-v1),
[OPF Apache-2.0](https://github.com/openai/privacy-filter),
and [spaCy model licenses](https://github.com/explosion/spacy-models).
