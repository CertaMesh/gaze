# Competitor comparison

Run `compare.py` whenever the benchmark corpus, scorer, contract, adapter, or
variant pack changes. It loads the same main documents and agentic generator as
Gaze, then scores UTF-8 byte spans with `gaze_bench_score.MetricAccumulator`.
`--pack-dir` adds every `*.jsonl` variant pack, including its sealed half. Pack
rows have `id`, `partition`, `text`, `language`, `region`, `layer`, and `gold`
(`start`, `end`, `label` in UTF-8 bytes). Invalid bounds, duplicate IDs,
unknown partitions, and unsupported labels fail closed.

Each competitor uses one inference pass for v3, v2, and v1. The output is
aggregate JSON only; no document text or per-document result is written. Run
competitors sequentially on a quiet CPU host. `presidio-all` is the headline
Presidio row: English, German, Dutch, French, and Portuguese spaCy models plus
Presidio's documented German recognizers. `presidio-en` is the English-only
default secondary row; `presidio-en-de` shows the intermediate configuration.
GLiNER uses model-card labels and the library threshold 0.5. OpenAI Privacy
Filter (OPF) runs only when its local runtime and checkpoint are configured.

Use Python 3.12. Install the hash-pinned `requirements.lock` into a session-local
virtual environment with `uv pip sync`. Install spaCy wheel models separately
at pinned versions: `en_core_web_lg` 3.7.1, `de_core_news_lg` 3.7.0,
`nl_core_news_sm` 3.7.0, `fr_core_news_sm` 3.7.0, and `pt_core_news_sm` 3.7.0.
`model-wheels.json` pins each wheel URL and SHA-256. The report records each
installed model's content SHA-256, wheel SHA-256, and version; review those
against the wheel used for the run. OPF 0.1.0 uses
its own Python environment. Its public provenance records the Python version,
source revision, source cleanliness, and checkpoint hash, never a local path.

Example:

```sh
python scripts/bench/compare/compare.py \
  --dataset target/bench-data/dataiku-en-de/test.parquet \
  --en-model /path/to/en_core_web_lg \
  --de-model /path/to/de_core_news_lg \
  --nl-model /path/to/nl_core_news_sm \
  --fr-model /path/to/fr_core_news_sm \
  --pt-model /path/to/pt_core_news_sm \
  --gliner-model /path/to/gliner_multi_pii-v1 \
  --opf-python /path/to/opf-venv/bin/python \
  --opf-checkpoint /path/to/privacy_filter \
  --gaze-policy target/gate/policy.toml \
  --gaze-scorecard-v1 target/bench-data/gaze-v1/scorecard-v4.json \
  --gaze-scorecard-v2 target/bench-data/gaze-v2/scorecard-v4.json \
  --gaze-scorecard-v3 target/bench-data/gaze-v3/scorecard-v4.json \
  --output target/bench-data/comparison.json
```

Copy only the aggregate JSON into `docs/reference/benchmarks/`, then render and
check `competitors.md` with `scripts/bench/compare/render.py`. The renderer
requires all contracts and configured competitors, model hashes, matching
per-layer document counts, clean source trees, and current competitor inputs.
Each release refreshes the Gaze rows on the same documents. If a variant pack
is present, each Gaze scorecard must carry the identical pack layer and ID
digest. The comparison stores the hash of a home-normalized policy after
checking its raw hash against every Gaze scorecard.

Presidio's anonymizer uses the public `keep` operator so resolved spans retain
their offsets. Skipped languages score as leaked gold, and only processed
documents enter latency. All predictions count byte-for-byte regardless of
class. `label-map.json` controls v3 repeated-gold credit and must be reviewed
when an adapter adds a label.

Sources and licenses: Presidio (MIT), spaCy model wheels (MIT), GLiNER and its
PII model (Apache-2.0), and OpenAI Privacy Filter (Apache-2.0). Upstream
configuration sources are Presidio's `default_recognizers.yaml` and language
documentation, the GLiNER-PII model card, and each model wheel's metadata.
