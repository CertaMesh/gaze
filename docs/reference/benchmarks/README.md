# Benchmark Methodology

This is the canonical reproducibility index for v0.9 benchmark claims. Public
release notes should link here whenever they cite a benchmark number.

## Hardware Spec Template

Fill this out for every published or PR-local benchmark run:

| Field | Value |
| --- | --- |
| Date | `YYYY-MM-DD` |
| Git commit | `<full commit sha>` |
| OS / kernel | `<name and version>` |
| Architecture | `<arch>` |
| CPU | `<model>` |
| RAM | `<bytes or GB>` |
| Rust | `rustc -V` |
| Python | `python --version` |
| Model cache | `$HOME/.cache/gaze/<bundle>` or another placeholder path |
| Relevant env | `GAZE_*` values that affect runtime/model selection |
| Notes | thermal state, cgroup/VM limits, cold-cache/warm-cache status |

Do not publish absolute home paths. Use `$HOME/...`, `~/...`, or
`<model-cache>/...`.

## Coverage-Loop Corpus

Most v0.9 benchmarks use the committed synthetic coverage-loop corpus:

| Field | Value |
| --- | --- |
| Corpus path | `crates/gaze-recognizers/testdata/coverage-loop/corpus` |
| Fixture count | `150` |
| Corpus SHA256 | `c6e78cca59df550fad18e59e9877da03da82c73b80c2368e5233d76353ccfa2f` |
| Coverage report SHA256 | `760f96163a68ce5f7dbc0409aa5109aa1a3ed190001536647e1881ba9d40a49c` |
| Build manifest | `crates/gaze-recognizers/testdata/coverage-loop/build-manifest.json` |

The corpus is synthetic by design and must remain free of real PII.

## Primary English/German Synthetic Holdout

The primary product-language scorecard uses English and German rows from the
pinned test split of Dataiku's synthetic Kiji PII corpus. It covers the complete
Gaze path: deterministic recognizers, Pass 2 NER, Kiji SafetyNet discovery,
Resolve promotion, fallback, exact restore, manifest integrity, post-policy
scan, precision, and warm latency.

Runnable path:

```bash
uv run --with pyarrow scripts/bench/dataiku_en_de_gaze_bench.py --no-download
```

The first run may omit `--no-download`; the runner fetches and verifies the
pinned Parquet test file under ignored `target/bench-data/`.

Evidence paths:

| Field | Value |
| --- | --- |
| Dataset and scoring contract | `docs/reference/benchmarks/dataiku-en-de-holdout.md` |
| Current whole-pipeline baseline | `docs/reference/benchmarks/v0.12-en-de-whole-pipeline-baseline.md` |
| Warm OpenAI Privacy Filter sample | `docs/reference/benchmarks/v0.12-opf-daemon-sample.md` |
| Benchmark runner | `scripts/bench/dataiku_en_de_gaze_bench.py` |
| Dataset revision | `DataikuNLP/kiji-pii-training-data@0275550f0b1f1b8f2dc9356fd31ac1c788b8228b` |
| Test-file SHA256 | `916c63792345bf3c2e0888941b3d14526c43b7c7fe8af60e0d283fed71b1234d` |

The upstream test split is reserved from training. It has no negative-only
documents, so it remains one layer of the release gate rather than a complete
PII evaluation.

The optional OPF cell is intentionally not part of the default run. It requires
a verified 2.6 GB checkpoint and a warmed local daemon, and it currently has a
measured fail-closed invalid-output rate. Use the supplemental OPF report and
explicit `full-stack-opf-resolve` config when evaluating that backend.

## Secondary Multilingual Synthetic Holdout

The secondary multilingual holdout uses only the validation split from
Ai4Privacy's OpenPII Micro corpus. It is synthetic, CC BY 4.0 licensed,
SHA-256 pinned, and reserved from all Gaze model training and threshold tuning.
It retains Japanese and 29 other languages as Unicode-offset and out-of-scope
language stress tests; it is no longer the English/German headline dataset.

Runnable path:

```bash
python3 scripts/bench/openpii_gaze_bench.py --no-download
```

The first run may omit `--no-download`; the script fetches the pinned validation
file into ignored `target/bench-data/`, then verifies its byte size and SHA-256.

Evidence paths:

| Field | Value |
| --- | --- |
| Dataset and scoring contract | `docs/reference/benchmarks/openpii-micro-holdout.md` |
| Current v0.12 baseline | `docs/reference/benchmarks/v0.12-openpii-baseline.md` |
| Benchmark runner | `scripts/bench/openpii_gaze_bench.py` |
| Dataset revision | `ai4privacy/pii-masking-micro-100k@3cd59c65631280839f830d3ba96dcdfe1785cab1` |
| Validation SHA256 | `bb15da1b5fbb11b3cc6fd4c95eca256197573ecd066230eb3c1fe6898f27a578` |

Generated result JSON stays under `target/bench-data/` unless a reviewed,
hardware-qualified snapshot is deliberately promoted into the repository.

## Safety-Net Matrix and Perf

Measures strict span precision, recall, F1, and strict leak rate for Kiji
DistilBERT, Kiji int8, and OpenAI Privacy Filter in direct-detector and
observer-residual modes. The perf snapshot measures one-shot CLI wrapper
latency separately from in-process ORT warm latency.

Runnable paths:

```bash
python3 scripts/bench/kiji-bench-scorer.py --repo-root . --mode all --measure-latency --precision int8 --model-dir "$HOME/.cache/gaze/<kiji-int8-bundle>" --python python3
python3 scripts/bench/opf-bench-scorer.py --repo-root . --mode all --measure-latency --python python3
cargo bench -p gaze-recognizers --bench safety_net_matrix
GAZE_KIJI_DISTILBERT_MODEL_DIR="$HOME/.cache/gaze/<kiji-bundle>" GAZE_SAFETY_NET_MATRIX_KIJI_BACKEND=ort cargo bench -p gaze-recognizers --features safety-net-kiji --bench safety_net_matrix
```

Evidence paths:

| Field | Value |
| --- | --- |
| Matrix snapshot | `crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json` |
| Perf snapshot | `crates/gaze-recognizers/benches/safety_net_perf_snapshot.json` |
| Methodology doc | `docs/reference/benchmarks/v0.9-safety-net-benchmark.md` |
| Kiji source | `onnx-community/distilbert-NER-ONNX@3a19fe9404a4469d91aa3d551558a97f68872f67` |
| Kiji fp32 bundle SHA256 | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |
| Kiji int8 bundle SHA256 | `6e7f238f38c5ee7977052ec391f6a8c68bbef038091f2ecff4747cc2268210cb` |
| OPF source | `openai/privacy-filter@f7f00ca7fb869683eb732c010299d901457f19c3` |
| OPF checkpoint bundle SHA256 | `4680158333621f3f344f58366f59612d52eff67ce6f46cff7becede5be1853ae` |

Measured v0.9 release-note claims from this surface:

| Claim | Evidence |
| --- | --- |
| Kiji int8 observer-residual macro recall `0.666667` | `safety_net_matrix_snapshot.json` cells for `kiji_distilbert_int8` observer-residual locales |
| Kiji int8 F1 delta `0.000` versus fp32 Kiji | same snapshot, matching Kiji fp32/int8 direct and observer cells across locales |
| Kiji int8 one-shot cold start `271.909583ms` in the committed perf snapshot | `safety_net_perf_snapshot.json` |

The earlier rc-cycle fp32 warm-p50 and int8 cold-start headlines are not present
in the committed final snapshots. Do not cite them in public release notes
unless a runnable snapshot is added.

## NER Model Leaderboard

Measures candidate NER safety-net backends on the same 150-fixture corpus and
records model pins, license caveats, class-map behavior, and warm latency where
available.

Runnable paths:

```bash
python3 scripts/bench/ner-bench-scorer.py --repo-root . --python python3 --mode all --model kiji-distilbert --model openobscure-tinybert4l-pii-ner-int8 --model mrm8488-mobilebert-ner --model osiria-minilm-italian-ner
python3 scripts/bench/ner-warm-latency.py --repo-root .
```

Evidence paths:

| Field | Value |
| --- | --- |
| Config | `crates/gaze-recognizers/benches/ner_models.toml` |
| Snapshot | `crates/gaze-recognizers/benches/ner_models_snapshot.json` |
| Methodology doc | `docs/reference/benchmarks/v0.9-ner-model-leaderboard.md` |
| Kiji fp32 bundle SHA256 | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |
| Kiji int8 bundle SHA256 | `6e7f238f38c5ee7977052ec391f6a8c68bbef038091f2ecff4747cc2268210cb` |

Measured v0.9 release-note claims from this surface:

| Claim | Evidence |
| --- | --- |
| Kiji int8 ORT warm p50 `1.849ms` | `ner_models_snapshot.json` `kiji-distilbert-int8.warm_latency.warm_p50_ms` |
| Kiji int8 direct recall matches fp32 Kiji at `0.125` and observer macro recall is `0.667` | `ner_models_snapshot.json` and `safety_net_matrix_snapshot.json` |

## Runtime Comparison

Measures ORT, tract, and candle Kiji runtime cold start and warm p50/p95 latency.
The benchmark requires a local Kiji model directory and asserts that non-ORT
runtimes produce the same span set as the ORT baseline.

Runnable path:

```bash
GAZE_KIJI_DISTILBERT_MODEL_DIR="$HOME/.cache/gaze/<kiji-bundle>" cargo bench -p gaze-recognizers --features safety-net-kiji,runtime-tract,runtime-candle --bench runtime_comparison
```

Evidence paths:

| Field | Value |
| --- | --- |
| Bench | `crates/gaze-recognizers/benches/runtime_comparison.rs` |
| Methodology doc | `docs/reference/benchmarks/v0.9-runtime-comparison.md` |
| Kiji fp32 bundle SHA256 | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |

## End-to-End Pipeline

Measures full Gaze pipeline behavior over rule-floor, pass3 Kiji, pass3 OPF,
and locale-aware configurations. `pass1_ms` is matching rule-floor wall clock;
`pass3_ms` is full-pipeline minus rule-floor delta for Pass-3 configs.

Runnable paths:

```bash
python3 scripts/bench/gaze-pipeline-bench.py --repo-root . --no-update
cargo bench -p gaze-pii --bench pipeline_end_to_end
```

Evidence paths:

| Field | Value |
| --- | --- |
| Snapshot generator | `scripts/bench/gaze-pipeline-bench.py` |
| Bench snapshot assertion | `crates/gaze/benches/pipeline_end_to_end.rs` |
| Snapshot | `crates/gaze-recognizers/benches/gaze_pipeline_bench_snapshot.json` |
| Methodology doc | `docs/reference/benchmarks/v0.9-gaze-pipeline-benchmark.md` |

## Tier 4 Pipeline Gating

Measures opt-in observer-only skip gates, capitals heuristic, prefix cache, and
length-bucketing hooks. The benchmark asserts zero SafetyNet suspects for every
config in its synthetic fixture set.

Runnable path:

```bash
cargo bench -p gaze-pii --bench tier4_pipeline_gating --all-features
```

Evidence paths:

| Field | Value |
| --- | --- |
| Bench | `crates/gaze/benches/tier4_pipeline_gating.rs` |
| Methodology doc | `docs/explanation/pipeline/tier4-pipeline-gating.md` |

Measured v0.9 release-note claims from this surface:

| Claim | Evidence |
| --- | --- |
| SafetyNet calls can drop from `300` to `0` on the synthetic numeric fixture set | `tier4_pipeline_gating` bench output |
| Prefix cache reduces detector bytes by `52.7%` and latency by `50.8%` in the documented local run | `docs/explanation/pipeline/tier4-pipeline-gating.md` |

## Final Revalidation

The final rc revalidation report composes the benchmark surfaces above and adds
a release-readiness interpretation. It is not a separate harness.

Evidence path: `docs/reference/benchmarks/v0.9.0-rc1-combined-revalidation.md`.
