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
  The adapter selects native locales for English, German, Dutch, French,
  Portuguese and Spanish. scrubadub selects its available detectors for each
  locale; the spaCy addon remains English-only. The existing committed rows
  predate the Spanish adapter correction and count those inputs as fully
  missed. They do not establish that the native library cannot process Spanish.
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

`requirements.lock` is a reproduction-only pin for the published comparison
measurements. It retains `cryptography==48.0.1`, affected by
GHSA-m2h6-j472-rp4c, GHSA-jwv3-5hgf-82ww, and GHSA-g6cj-pr64-35w5.
Its bytes stay unchanged because the published reports record its full
SHA-256 and the renderer verifies it. Install it only in an explicitly
selected historical reproduction environment.

CI and Anvil preparation use `scripts/bench/pyproject.toml` and `uv.lock`,
which require `cryptography>=50.0.2,<51` through the documented Presidio
override. Use that patched environment for routine model-free checks:

```sh
uv sync --project scripts/bench --python 3.13 --locked --group comparison-test
```

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

Before collecting scrubadub rows, run its native regression checks with the
Python interpreter from the separately locked scrubadub environment:

```sh
"$GAZE_COMPARE_SCRUB_PYTHON" scripts/bench/compare/test_scrubadub_native.py
```

This command checks the scrubadub, plugin, spaCy and English model versions;
it fails if any are missing or differ. It checks Spanish and English positive and benign
inputs, both configurations, and full population accounting without Spanish
skips. Ordinary comparison tests skip these two native checks when scrubadub
is absent; that skip does not replace this required native run.

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

Presidio: https://github.com/data-privacy-stack/presidio/blob/2.2.364/docs/analyzer/index.md
DataFog Core: https://github.com/DataFog/datafog-core/blob/v0.3.0/README.md
DataFog Python: https://github.com/DataFog/datafog-python/blob/v4.8.1/README.md
scrubadub: https://github.com/LeapBeyond/scrubadub/blob/v2.0.0/docs/usage.rst
GLiNER PII model: https://huggingface.co/urchade/gliner_multi_pii-v1/tree/1fcf13e85f4eef5394e1fcd406cf2ca9ea82351d
OPF: https://github.com/openai/privacy-filter
PII-Tracer: https://huggingface.co/perplexity-ai/PII-Tracer/blob/d25c16f2e57e321f6d2527715c01df9112f956f5/README.md

## Publish the Spanish scrubadub supplement

The Spanish correction changes exactly `scrubadub-base` and `scrubadub-spacy`.
The original complete report remains a valid record of the original adapter,
including its skipped Spanish inputs. Preserve that file byte for byte. Its
SHA-256 is `119e24993cc47ae9b6b53af052b2266a8f83a4903675f72327eb4c419602851d`.
The renderer accepts the reviewed original adapter and finalizer hashes without
claiming those measurements used the current adapter.

After the reviewed collector source is on main, collect a fresh sparse report
in the pinned scrubadub environment. Use the original dataset and English model.
The collector still requires exact `HEAD:crates` and `origin/main:crates` equality.
This competitor-only supplement has no Gaze scorecards: the publication inherits
the original Gaze measurements from the complete snapshot.

```sh
common=(
  --dataset "$GAZE_COMPARE_DATASET"
  --pack-dir docs/reference/benchmarks/variant-packs
  --en-model "$GAZE_COMPARE_EN_MODEL"
  --output "$SUPPLEMENT_REPORT"
  --predictions-dir "$SUPPLEMENT_NATIVE"
)
"$GAZE_COMPARE_SCRUB_PYTHON" scripts/bench/compare/compare.py \
  "${common[@]}" --tool scrubadub-base
"$GAZE_COMPARE_SCRUB_PYTHON" scripts/bench/compare/compare.py \
  "${common[@]}" --tool scrubadub-spacy --resume
```

Use new private output and native-record directories. Do not resume the original
complete report. Both fresh rows must process every document under every scored
contract, with zero skipped documents. Retain the separate Spanish positive and
benign-counterweight review; a synthetic regression does not establish corpus
performance. OPF retains its original unavailable receipt.

Copy the original complete report and sparse supplement into
`docs/reference/benchmarks/comparison-sources/original.json` and
`docs/reference/benchmarks/comparison-sources/spanish.json`. Review the supplement
hash before composition. Keep these aggregate snapshots immutable and commit
them with the composed publication. Private native records stay outside the
repository. The finalizer reads them only as bytes to record file hashes and
sizes. Composition also creates `comparison.json.native-receipt.json` beside the
output report. This public-safe aggregate receipt contains only the two snapshot
hashes and native file hashes and sizes, including superseded records.
It contains no native bodies or private paths. Review and separately pin its SHA-256 alongside
the snapshot hashes, and commit all three immutable evidence files with the
publication. The renderer needs only these files and the composed report.

```sh
uv run --project scripts/bench python scripts/bench/compare/finalize_report.py \
  docs/reference/benchmarks/comparison.json \
  --compose-base docs/reference/benchmarks/comparison-sources/original.json \
  --base-sha256 119e24993cc47ae9b6b53af052b2266a8f83a4903675f72327eb4c419602851d \
  --supplement docs/reference/benchmarks/comparison-sources/spanish.json \
  --supplement-sha256 "$SUPPLEMENT_SHA256" \
  --base-predictions-dir "$ORIGINAL_NATIVE" \
  --supplement-predictions-dir "$SUPPLEMENT_NATIVE"
shasum -a 256 docs/reference/benchmarks/comparison.json.native-receipt.json
uv run --project scripts/bench python scripts/bench/compare/render.py \
  docs/reference/benchmarks/comparison.json \
  --page docs/reference/benchmarks/competitors.md
uv run --project scripts/bench python scripts/bench/compare/render.py \
  docs/reference/benchmarks/comparison.json \
  --page docs/reference/benchmarks/competitors.md --check
```

The composed report keeps the base's top-level measurement and replay metadata.
`row_sources.tool_sources` attributes each competitor to its snapshot;
`row_sources.aggregation` records only the later composition revision, dirty
state, code hashes and time. Snapshot metadata and each tool's provenance retain their own
source, adapter, collection or replay time, configuration and model pins. Native
hashes cover both original and fresh records, including superseded original
scrubadub records. `row_sources.native_receipt_sha256` pins the separately retained
receipt. Its filename is always the full report filename plus
`.native-receipt.json`. Submitted manifest fields cannot redirect it. Composition
creates it exclusively and can reuse identical bytes, but cannot replace it with
different evidence. Use a new publication filename for a new pair of sources.
Ordinary finalization and rendering compare both native maps against that file,
without reading private native records. A checksum repacked only in the submitted
report is insufficient. The trusted inputs are the independently retained,
reviewed snapshots and receipt. Simultaneous malicious rewriting of every
trusted input is outside this contract. Eleven inherited tool objects, Gaze measurements, selected
thresholds and the OPF receipt remain identical to the original snapshot.

Composition requires identical corpus, scored contracts, scorer, mappings,
dependency and model pins, heldout split and machine identity. The only accepted
adapter transition is the reviewed Spanish locale addition. Rendering checks
snapshot byte hashes and reconstructs the composition; altered inherited or
fresh objects fail validation. A rejected input leaves the prior output intact.
Tuned Presidio remains bound to the original snapshot, with the existing
byte-identity verdict and disclosed label differences. Timing stays withheld.


## Check competitor source freshness

Run the metadata check before a release or comparison report. It reads all four
comparison locks, spaCy wheel pins, tuned model/setup declarations, PII-Tracer,
and the separately recorded GLiNER tokenizer and OPF sources. It never imports
competitor runtimes, downloads model weights, installs packages, or changes pins.

```sh
uv run --project scripts/bench --locked python scripts/bench/compare/freshness.py \
  --report docs/reference/benchmarks/comparison.json \
  --output target/competitor-freshness.json
```

The aggregate follows `freshness.schema.json`. Each row records its exact pin,
pinned release date, current stable version and release date, UTC check date,
source URL, and consuming lock/setup files. PyPI dates are the first artifact
upload. GitHub release dates are publication dates. Model snapshot dates are
commit dates. Unknown dates remain explicit. Prereleases, development versions,
drafts and fully yanked PyPI releases do not count as stable. A model without
stable version tags remains `unversioned`; a moving branch is not labeled stable.
The OPF checkpoint has only a recorded digest and no versioned release source,
so that limitation remains visible. A missing pin, failed network request,
unsupported metadata or bounded pagination limit produces a visible finding
and does not abort the remaining checks. Requests use four workers, an eight
second timeout and metadata-only response limits.

A new release is `update-unverified` until a separate PR qualifies its driver,
updates the pins and changes the measured subject identity. An incompatible
latest release is `incompatible-latest`, and the measured row stays explicitly
old. The existing Presidio resolver requires its exact anonymizer version;
upstream Python requirements are checked against the locked Python 3.12
protocol. Other driver compatibility is unverified, never assumed. This sidecar
is report metadata and does not change benchmark specifications, subject keys,
measurement times or cache admission.

Add `--freshness target/competitor-freshness.json` to `render.py` to append the
inventory to the comparison page. Rendering checks the comparison hash and
source-input hashes, and rejects partial checks. Without this option, existing
pages stay unchanged. Rerun the command when source pins change. The check date
stays visible; rendering never queries the network or refreshes it implicitly.

For a bounded source check, pass an exact key such as
`--source pypi:presidio-anonymizer@2.2.364`. A partial check cannot accompany a
published aggregate. For an optional scheduled check, run the full command from
a weekly scheduler in a prepared checkout and retain its JSON and stdout. The
command exits successfully when source checks fail so findings remain reportable;
review the row statuses rather than interpreting exit zero as current pins.
## Reuse competitor document captures

`capture_compare.py` runs one competitor with private, content-addressed document
captures. Its report contains aggregate metrics and digest/count bindings. Full
subject, runtime, input and GLiNER load proofs stay in the private cache.
A public binding commits to the ordered private observation digests. Variant-pack
names become ordinal layer names in public output and prediction file paths.

Capture entrypoints support **script-path launch only**: invoke the `.py` paths
shown below. Module launch with `python -m` is refused. Script launch compiles
the entrypoint from source before its import checks run; module launch can load
that entrypoint's cached bytecode before any in-process check. Effective stdlib
and harness bytecode must match source, including with a pycache prefix. The
effective prefix is bound into the private runtime identity.

Use a clean checkout and an installed, qualified comparison runtime. The
measuring specification requires CPython 3.13 and the pinned parquet reader.
GLiNER additionally requires the source hashes and loader family declared in
`gliner_identity.py`. Unsupported runtimes, import hooks, model families and
online GLiNER resolution are refused. Use preinstalled models for qualification.

Use a private pycache prefix to avoid stale caches in a newly installed runtime.
Capture admission refuses stale caches even when Python could recompile them.
Keep the prefix unchanged when replaying a capture.

Prepare an immutable specification with `scripts/bench/benchmark_spec.py`. Keep
the dataset, installed models and cache outside the repository. Set these
variables to the scheduled qualification inputs before running the commands:

```sh
export PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1
export PYTHONPYCACHEPREFIX="$PRIVATE/pycache"
export HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1
"$QUALIFIED_PYTHON" scripts/bench/benchmark_spec.py \
	--dataset "$DATASET" --output "$PRIVATE/spec.json"
"$QUALIFIED_PYTHON" scripts/bench/compare/capture_compare.py \
	--tool gliner --dataset "$DATASET" --benchmark-spec "$PRIVATE/spec.json" \
	--gliner-model "$GLINER_MODEL" \
	--cache-dir "$PRIVATE/captures" --output "$PRIVATE/fresh.json"
"$QUALIFIED_PYTHON" scripts/bench/compare/capture_compare.py \
	--tool gliner --dataset "$DATASET" --benchmark-spec "$PRIVATE/spec.json" \
	--gliner-model "$GLINER_MODEL" \
	--cache-dir "$PRIVATE/captures" --output "$PRIVATE/reused.json"
cmp "$PRIVATE/fresh.json" "$PRIVATE/reused.json"
"$QUALIFIED_PYTHON" scripts/bench/compare/capture_compare.py \
	--tool gliner --dataset "$DATASET" --benchmark-spec "$PRIVATE/spec.json" \
	--gliner-model "$GLINER_MODEL" \
	--cache-dir "$PRIVATE/captures" --output "$PRIVATE/reused.json" \
	--verify-report "$PRIVATE/reused.json"
```

These are the installed-model qualification commands. Run them only in the
scheduled benchmark-machine window. Source tests do not establish installed-model
qualification. A full hit constructs no backend and performs no inference.
Latency and contention remain the original measurements. Cache replay does not
qualify new performance evidence.

Each unit key binds the exact text hash, document ID, language and region, the
immutable benchmark specification ID, and the actual subject/config/model/runtime
identity. Changes to current Gaze detector bytes do not change a competitor key while the
measuring specification remains unchanged.
An input change under the same specification invalidates that unit. A new
specification invalidates all units. The canonical population must match the
specification. Additional variant-pack documents also have independent unit keys.
Tampered entries cause refusal. The cache never manufactures keys for historical
predictions.

Pass `--freshness-report` to validate and reference the independently checked
sidecar from `freshness.py`. Its file hash, row count and check date remain
separate from subject execution identity. The existing historical publication commands and receipts
retain their original code bindings.

For tuned production, use `tuned_presidio/capture_produce.py` with the original
`pool` or `authors` arguments plus `--benchmark-spec` and `--cache-dir`.
`tuned_presidio/capture_tune.py` takes `--production-plans`, a private JSON object
mapping each pool pass and `authors` to its full production argument vector. It
reconstructs each plan, checks its private captures, and binds the selected live
run to the same configuration, models, findings, documents and sample used in
scoring. `--private-dir` holds the complete binding. Public output contains
digests and counts for those private inputs and live differences.
`--verify-report` replays private evidence without running models; `--render`
writes an admitted aggregate Markdown table.

The retained tuned comparison uses generator v20. It cannot be assigned the
current generator v25 specification. Rebuild matching comparison, production and
selection inputs before installed-model tuned qualification. Historical aggregate
rows remain historical and receive no reuse eligibility.

Run the model-free checks with:

```sh
anvil run -- uv run --project scripts/bench --python 3.13 --group comparison-test \
	python -m pytest -q scripts/bench --durations=30
python3 scripts/ci/check_competitor_capture_mutations.py
```
