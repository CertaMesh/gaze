# Gaze Benchmarks

The single benchmark document for Gaze. It carries the current release's
measured numbers as a table and as charts, the methodology behind them, and the
commands to reproduce them.

Everything under [Current release](#current-release), [Charts](#charts), and
[Release history](#release-history) is **generated** from
[`release-history.json`](release-history.json) by
[`scripts/bench/render_benchmark_doc.py`](../../../scripts/bench/render_benchmark_doc.py).
Do not hand-edit inside the `<!-- BEGIN GENERATED -->` markers; CI re-renders
and fails on drift.

| Section | Contents |
| --- | --- |
| [What we measure, and how](#what-we-measure-and-how) | corpora, arms, metrics, goals |
| [A scorecard measures the corpus, not the recognizer](#a-scorecard-measures-the-corpus-not-the-recognizer) | how to read these numbers honestly |
| [Current release](#current-release) | the headline table |
| [Charts](#charts) | per-arm and release-over-release |
| [Release history](#release-history) | one row per released version |
| [Safety-Net Matrix](#safety-net-matrix) | backend pins and matrix shape |
| [NER Model Leaderboard](#ner-model-leaderboard) | candidate backends |
| [How to reproduce](#how-to-reproduce) | commands, harness, hardware |
| [Evidence before the release gate](#evidence-before-the-release-gate) | archived per-PR reports |

Related contracts kept as separate documents:

- [`evidence-protocol-v1.md`](evidence-protocol-v1.md) — normative contract for
  the optional offline T1 evidence path (route observations, synthetic paired
  arithmetic, recursive aggregate receipt validation). Explicitly **not** an
  end-to-end paired evaluation of route observations.
- [`negative-corpus-annotation-contract.md`](negative-corpus-annotation-contract.md)
  — synthetic EN/DE hard-negative annotation and zero-PII contract.
- [`class-commitments-v1.json`](class-commitments-v1.json) — fixture-only
  class-commitment schema and its two fixture rows. Scope is
  `fixture_only_not_a_corpus_commitment`; it is not a corpus commitment.

---

## What we measure, and how

### The question

Would annotated PII bytes reach a downstream LLM? Gold spans and Gaze
prediction spans are merged before their intersection is measured in **UTF-8
bytes**. Label matching is deliberately not part of the primary score:
pseudonymization safety depends first on covering the bytes, and Gaze's class
vocabulary does not match every source taxonomy exactly. Per-label recall still
exposes class-shaped gaps.

### How to read metric headers

Result headers use one direction marker plus a written goal: `↑` **higher is
better**, `↓` **lower is better**, `↔` an **exact or invariant target**, `info`
a **descriptive field with no optimization direction**. Historical evidence
without a contractual threshold uses `goal lower` / `goal higher` /
`goal no regression`; it does not invent a numeric gate.

`info` is intentional for support counts, observed configuration columns in
mixed-metric tables, diagnostic partitions, and deltas whose favorable sign
depends on the metric. Pin, evidence, command, rationale, and category tables
stay plain because they are not optimization results.

### The three arms

Every release scorecard runs the same three configurations:

| Arm | What it is |
| --- | --- |
| `rule-floor-extended` | the shipped deterministic recognizers alone |
| `pass2-ner` | that floor plus the configured `NerRecognizer` (threshold `0.3` by default) |
| `full-stack-kiji-resolve` | **the shipped default** — Pass 2 plus the in-process Kiji SafetyNet under the shipped `Resolve`/`Redact` policy, with exact-restore checks, manifest-integrity checks, and a post-policy SafetyNet scan |

An optional `full-stack-opf-resolve` arm exercises the OpenAI Privacy Filter
through the same contract. It is excluded from the default run because it needs
a separately installed verified 2.6 GB checkpoint and a warmed daemon, and it
has a measured fail-closed invalid-output rate.

### Zero-leak production goals

Production scorecard targets, not claims about any particular historical report:

| Production metric | Direction and goal |
| --- | --- |
| Leaked labeled PII bytes | ↓ lower is better; goal 0 |
| Missed entities | ↓ lower is better; goal 0 |
| Zero-leak documents | ↑ higher is better; goal 100% |
| Exact restore | ↑ higher is better; goal 100% of completed documents |
| Valid, trace-consistent manifests | ↑ higher is better; goal 100% of completed documents |
| Final redact actions | ↓ lower is better; goal 0 |
| Silent detector skips | ↓ lower is better; goal 0 |
| Actionable residual suspects | ↓ lower is better; goal 0 |
| Production/benchmark divergence | ↔ invariant target; goal 0 |
| False-positive bytes/documents and clean-document changes | ↔ invariant ratchet; goal no regression |
| *Ratchet exception (gold noise)* | A change may raise holdout false-positive bytes only by spans that are genuine identifiers the corpus gold does not label, and only when the A4 negative corpus does not move at all (bytes and documents, every category); each such span is enumerated shape-normalised with the reason it is real, and the reviewer re-confirms the list — never tune a rule to skip real PII to keep the counter flat |

The scorecard is **non-compensating**: safety, reversibility, trust,
availability, precision, and latency are reported as separate axes. Latency is
compared only after the correctness gates pass.

### Primary corpus — English/German synthetic holdout

The primary product-language corpus is the English and German rows of the
upstream test split of
[`DataikuNLP/kiji-pii-training-data`](https://huggingface.co/datasets/DataikuNLP/kiji-pii-training-data),
paired with the complete committed A4 EN/DE hard-negative corpus. The split is
**evaluation-only**: it must never enter Gaze training, fine-tuning, prompt
examples, dictionaries, rule authoring, or threshold selection.

| Field | Pinned value |
| --- | --- |
| Repository | `DataikuNLP/kiji-pii-training-data` |
| Revision | `0275550f0b1f1b8f2dc9356fd31ac1c788b8228b` |
| File | `data/test-00000-of-00001.parquet` |
| License | `Apache-2.0` |
| Declared data kind | synthetic PII only |
| Full test rows | `5,150` |
| File bytes | `2,013,107` |
| SHA-256 | `916c63792345bf3c2e0888941b3d14526c43b7c7fe8af60e0d283fed71b1234d` |
| Selected English rows | `1,033` |
| Selected German rows | `853` |
| Selected annotations | `14,719` |
| Observed selected labels | `29` |
| Selection seed | Not applicable; the complete pinned EN/DE selection is used without sampling or shuffling |
| Negative corpus | [`crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl`](../../../crates/xtask/fixtures/negative_corpus/en_de_negative.jsonl) |

The runner refuses any file whose byte size or digest differs. It also verifies
the full row count, validates every selected annotation boundary and annotated
substring, and converts source character offsets to UTF-8 byte offsets before
scoring.

The word `kiji` in the dataset name does **not** mean this benchmark uses
Gaze's Kiji SafetyNet model. Gaze's Kiji backend is the separately pinned
`onnx-community/distilbert-NER-ONNX` bundle; the runner records both model
directories independently.

**Limits.** Every selected row contains annotated PII, so the split measures
false-positive bytes only within positive documents and cannot replace a
negative-only corpus — hence the paired A4 negatives. Synthetic templates
underrepresent OCR errors, streaming boundaries, JSON tool calls, tenant
identifiers, and ambiguous natural language. The test split is isolated from
training, but upstream train and test data may share a generator and templates:
if a future model uses another split from the same repository, that score is
**same-generator evaluation** and cannot be its only promotion gate.

### Secondary corpus — OpenPII micro, multilingual

The validation split of `ai4privacy/pii-masking-micro-100k` is retained as a
secondary multilingual masking holdout and Unicode-offset stress test,
including its Japanese slice. The same evaluation-only rule applies.

| Field | Pinned value |
| --- | --- |
| Repository | `ai4privacy/pii-masking-micro-100k` |
| Revision | `3cd59c65631280839f830d3ba96dcdfe1785cab1` |
| File | `data/validation.jsonl` |
| License | `CC-BY-4.0` |
| Declared data kind | synthetic PII only |
| Rows | `9,990` |
| Bytes | `32,536,978` |
| SHA-256 | `bb15da1b5fbb11b3cc6fd4c95eca256197573ecd066230eb3c1fe6898f27a578` |
| Annotations | `72,087` |
| Observed labels | `26` (a small tail beyond the 19 advertised on the dataset card; the runner reports observed counts rather than dropping it) |

Two additional recall slices are published for this corpus: **direct
identifiers** (names, contact details, account identifiers, addresses) and
**contextual PII** (dates, ages, titles, sex, gender, time, amount, currency).
All labels stay in the primary all-PII score; the slices do not weaken the
fail-closed contract.

**Datasets evaluated and rejected.** PIIMB has an excellent masking-oriented,
character-level methodology with negative sentences, but its assembled
benchmark is CC BY-NC 4.0 and so cannot be the default reproducible corpus for
Gaze's unrestricted adopter workflow. REDACT is useful as a future curated
secondary evaluation, but its files require access approval. Neither constraint
justifies weakening the current synthetic holdout gate.

---

## A scorecard measures the corpus, not the recognizer

These numbers score one synthetic EN/DE holdout. A perfect row here is evidence
about **this corpus**, not proof that a recognizer is complete — shapes the
corpus does not contain are unmeasured. Recall claims about a rule change need a
direct differential probe, not a scorecard row.

Two consequences worth stating plainly:

1. **Consolidating N rules into one is a widening only if the shared form is a
   superset of every original, including the loosest.** A clean scorecard across
   all three arms has previously coexisted with a real recall regression on
   whitespace shapes this corpus happens not to contain.
2. **False-positive bytes are a ratchet, not a free variable.** They are
   reported beside recall precisely so precision cannot be traded away silently
   to make a leak counter fall.

---

## Current release

<!-- BEGIN GENERATED: current-release -->

> **No release has been measured yet.** The table and charts below fill in when a release runs the harness and appends its row. Produce one with the commands in [How to reproduce](#how-to-reproduce).

<!-- END GENERATED: current-release -->

---

## Charts

<!-- BEGIN GENERATED: charts -->

> Charts render once at least one release row exists in [`release-history.json`](release-history.json).

<!-- END GENERATED: charts -->

---

## Release history

One row per released version. Every row's numbers come from the
`scorecard-vX.Y.Z.json` named in that row, which stays committed as the
machine-readable evidence.

<!-- BEGIN GENERATED: history -->

| Release | Measured | Commit | Machine | Scorecard | Surviving PII bytes ↓ |
| --- | --- | --- | --- | --- | ---: |
| *none yet* | — | — | — | — | — |

<!-- END GENERATED: history -->

Rows marked *(provisional)* were not measured on the released tree; their note
records what was measured instead.

---

## Safety-Net Matrix

The tracked benchmark snapshot lives at
[`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`](../../../crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json).
`cargo bench -p gaze-recognizers --features safety-net-kiji,safety-net-openai --bench safety_net_matrix`
validates that the snapshot pins match runtime constants and prints the JSON for
CI logs.

Current status: `opf_kiji_direct_run_v1_observer_residual_deferred` —
direct-detector cells are populated for Kiji DistilBERT and OpenAI Privacy
Filter; observer-residual cells remain deferred pending the cleaned-output
harness.

### Matrix shape

The snapshot schema is version 2, keyed by backend, locale, and mode:

| Dimension | Values |
| --- | --- |
| Backends | `kiji_distilbert`, `openai_privacy_filter` |
| Locales | `Global`, `EnUs`, `DeDe` |
| Modes | `direct_detector`, `observer_residual` |

That is 12 cells. Each `direct_detector` cell carries nullable precision,
recall, F1, and per-class metrics. Each `observer_residual` cell also carries
nullable `observer_residual_recall`, `agreement_with_rule_floor`,
`expansion_fraction`, `contradiction_fraction`, and `novel_tp_over_rule_floor`.

The top-level `strict_span_leak_rate` block is mode-independent and records one
nullable headline field per backend-locale pair. It measures end-to-end
fail-closed behavior rather than detector precision/recall.

Kiji and OPF direct-detector fields are populated from pinned local backend
runs. **Observer-residual cells remain `null`** until their separate
cleaned-output harness runs are captured. Publishing observer-residual claims
without those pins would violate the axis-4 trust contract.

### Backend integrity pins

Kiji DistilBERT:

| Pin | Value |
| --- | --- |
| Source repo | `onnx-community/distilbert-NER-ONNX` |
| Source commit | `3a19fe9404a4469d91aa3d551558a97f68872f67` |
| Bundle SHA256 (fp32) | `c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f` |
| Bundle SHA256 (int8) | `6e7f238f38c5ee7977052ec391f6a8c68bbef038091f2ecff4747cc2268210cb` |
| Model SHA256 | `b5f77096d0d9f425d34a2e263f8a2dfb845cdc757dc00c7a1e69e9cbb93115d5` |
| Tokenizer SHA256 | `cb26b43c98e8266ae3e99c2a583cf8315d73b33a17e6b20b4df7ff1f22392d34` |
| Label-map SHA256 | `d3753ce580a9d43b113d779c712494bd61341285317beec49cc1e848b86f9a97` |

OpenAI Privacy Filter:

| Pin | Value |
| --- | --- |
| Source repo | `openai/privacy-filter` |
| Source commit | `f7f00ca7fb869683eb732c010299d901457f19c3` |
| Checkpoint bundle SHA256 | `4680158333621f3f344f58366f59612d52eff67ce6f46cff7becede5be1853ae` |
| Required checkpoint artifacts | `["config.json", "dtypes.json", "model.safetensors", "viterbi_calibration.json"]` |

OPF publishes a source repository and an `opf` Python CLI that downloads its
checkpoint into `~/.opf/privacy_filter` by default, or into the directory
selected by `OPF_CHECKPOINT` / `--checkpoint`. It does not publish a GitHub
release binary, so Gaze does not pin a binary checksum — the source commit and
checkpoint bundle are the trust anchors. The bundle hash was captured from a
clean local `opf download` on 2026-05-15 and is SHA256 over the Kiji-style
line-per-file `SHA256SUMS` manifest for the required artifact list in
declaration order.

### Runnable paths

```bash
python3 scripts/bench/kiji-bench-scorer.py --repo-root . --mode all --measure-latency --precision int8 --model-dir "$HOME/.cache/gaze/<kiji-int8-bundle>" --python python3
python3 scripts/bench/opf-bench-scorer.py --repo-root . --mode all --measure-latency --python python3
cargo bench -p gaze-recognizers --bench safety_net_matrix
GAZE_KIJI_DISTILBERT_MODEL_DIR="$HOME/.cache/gaze/<kiji-bundle>" GAZE_SAFETY_NET_MATRIX_KIJI_BACKEND=ort cargo bench -p gaze-recognizers --features safety-net-kiji --bench safety_net_matrix
```

| Evidence | Path |
| --- | --- |
| Matrix snapshot | [`crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json`](../../../crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json) |
| Perf snapshot | [`crates/gaze-recognizers/benches/safety_net_perf_snapshot.json`](../../../crates/gaze-recognizers/benches/safety_net_perf_snapshot.json) |
| Bench source | [`crates/gaze-recognizers/benches/safety_net_matrix.rs`](../../../crates/gaze-recognizers/benches/safety_net_matrix.rs) |

Claims currently supported by this surface:

| Claim | Evidence |
| --- | --- |
| Kiji int8 observer-residual macro recall `0.666667` | `safety_net_matrix_snapshot.json`, `kiji_distilbert_int8` observer-residual locale cells |
| Kiji int8 F1 delta `0.000` versus fp32 Kiji | same snapshot, matching fp32/int8 direct and observer cells across locales |
| Kiji int8 one-shot cold start `271.909583ms` | `safety_net_perf_snapshot.json` |

Earlier rc-cycle fp32 warm-p50 and int8 cold-start headlines are **not** present
in the committed final snapshots. Do not cite them unless a runnable snapshot is
added.

---

## NER Model Leaderboard

Compares pinned Hugging Face NER candidates as Gaze safety-net backends on the
committed 150-fixture coverage-loop corpus. Configuration lives in
[`crates/gaze-recognizers/benches/ner_models.toml`](../../../crates/gaze-recognizers/benches/ner_models.toml);
evidence is written to
[`crates/gaze-recognizers/benches/ner_models_snapshot.json`](../../../crates/gaze-recognizers/benches/ner_models_snapshot.json).

Measured 2026-05-15 on macOS 26.5 arm64, Apple M5 Max. The scorer used Python
3.10.20 for the Tiny/Mobile/Mini candidates; Kiji warm ORT rows used the Rust
ORT backend. Corpus: 150 fixtures, `target/coverage-report.json` SHA256
`760f96163a68ce5f7dbc0409aa5109aa1a3ed190001536647e1881ba9d40a49c`.

Macro averages are across the committed `Global`, `EnUs`, and `DeDe` locale
cells. Warm p50 keeps the model/session loaded over the same 150 direct fixture
texts, except Kiji ORT warm rows, which use the live ORT bench fixture.

| Model info | HF repo @ commit info | License info | Params info | Bundle size ↓ (goal lower) | Direct recall ↑ (goal 1.000) | Observer recall ↑ (goal 1.000) | Warm p50 ↓ (goal no regression) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| **Kiji DistilBERT int8 ORT** (shipped default) | same source, local int8 artifact | Apache-2.0 | 66M | 63MB model | 0.125 | 0.667 | 1.849ms |
| Kiji DistilBERT fp32 ORT | `onnx-community/distilbert-NER-ONNX@3a19fe9` | Apache-2.0 | 66M | 249MB model | 0.125 | 0.667 | 2.562ms |
| openobscure TinyBERT4L PII NER int8 | `openobscure/tinybert4l-pii-ner-int8@f8399a9` | Apache-2.0 | 14M | 13.95MB | 0.070 | 0.271 | 1.087ms |
| mrm8488 MobileBERT NER | `mrm8488/mobilebert-finetuned-ner@3f9a1f3` | MIT | 25M | 94.29MB | 0.124 | 0.661 | 16.494ms |
| osiria MiniLM-L6-H384 Italian NER | `osiria/minilm-l6-h384-italian-cased-ner@125c646` | MIT | 22.6M | 174.30MB | 0.124 | 0.667 | 4.703ms |

Full leaderboard, including the larger multilingual candidates. `Median ms` is
the scorer's direct-mode one-shot subprocess median, **not** warm in-process
latency:

| Rank info | Model info | License info | Shippable default? info | Direct P/R/F1 ↑ (goal 1.000 each) | Observer-residual P/R/F1 ↑ (goal 1.000 each) | Median ms ↓ (goal no regression) |
| ---: | --- | --- | --- | ---: | ---: | ---: |
| 1 | Davlan multilingual BERT-NER (HRL) | AFL-3.0 | Yes | 0.558 / 0.125 / 0.200 | 0.518 / 0.661 / 0.558 | 1911.349 |
| 2 | Babelscape WikiNeural Multilingual | CC-BY-NC-SA-4.0 | No, non-commercial | 0.476 / 0.125 / 0.194 | 0.507 / 0.667 / 0.547 | 1786.361 |
| 3 | osiria MiniLM-L6-H384 Italian NER | MIT | Yes, locale caveat | 0.117 / 0.124 / 0.105 | 0.206 / 0.667 / 0.264 | 1566.991 |
| 4 | mrm8488 MobileBERT NER | MIT | Yes | 0.232 / 0.124 / 0.141 | 0.362 / 0.661 / 0.369 | 1760.778 |
| 5 | dslim/bert-base-NER (English) | MIT | Yes | 0.300 / 0.124 / 0.152 | 0.290 / 0.655 / 0.323 | 1737.297 |
| 6 | Kiji DistilBERT | Apache-2.0 | Yes | 0.247 / 0.125 / 0.140 | 0.246 / 0.667 / 0.287 | 163.161 |
| 7 | openobscure TinyBERT4L PII NER int8 | Apache-2.0 | Yes, recall caveat | 0.424 / 0.070 / 0.113 | 0.478 / 0.271 / 0.296 | 76.548 |

### Which backend to pick

| Need | Use | Reason |
| --- | --- | --- |
| `<50MB` total model bundle | openobscure TinyBERT4L PII NER int8 | Only measured candidate below 50MB and the fastest warm p50. **Not** a Kiji replacement where observer-residual recall matters. |
| `<100ms` warm p50 with recall preserved | Kiji DistilBERT int8 ORT | Warm p50 `1.849ms`, recall matches fp32 Kiji under the existing int8 gate. |
| Highest recall among permissive tiny candidates | osiria MiniLM-L6-H384 Italian NER | Ties Kiji observer recall here, but it is Italian-native and its 174MB bundle misses the low-spec storage target. |

Low-spec reference read:

| Reference profile target | Pass? | Recommendation |
| --- | ---: | --- |
| 1vCPU / 1GB RAM, `<50MB` bundle | Partial | TinyBERT is the only measured `<50MB` bundle and should fit the storage envelope, but observer recall drops to 0.271. Not a safe default. |
| 1vCPU / 1GB RAM, `<100ms` warm p50 with recall preserved | Yes by host-proxy latency; not cgroup-proven | Kiji int8 ORT: `1.849ms` warm p50 on this host, identical scorer recall to fp32. A true 1vCPU/1GB cgroup or VM run remains the final deployment proof. |

**Screening notes.** `onnx-community/TinyBERT-finetuned-NER-ONNX` and
`adel-cybral/TinyBERT-finetuned-NER` did not publish a clean permissive license
in HF metadata and were skipped; `openobscure/tinybert4l-pii-ner-int8` is
Apache-2.0 and was pinned, but it is a PII-specific TinyBERT head rather than a
strict CoNLL clone. `SKNahin/NER_MobileBert` was skipped for missing license
metadata. No permissive English or multilingual general MiniLM NER head with the
desired PER/LOC/ORG/MISC fit was found.

**Interpretation.** The tiny-candidate tier did not produce a default flip.
TinyBERT wins size and warm latency but loses too much residual recall for the
reliability axis. MobileBERT nearly preserves Kiji recall but is slower in the
subprocess scorer and larger than the low-spec storage target. Kiji DistilBERT
int8 ORT remains the shipped default; use TinyBERT only where a `<50MB` bundle
is a hard constraint and the recall drop is explicitly accepted.

### Runnable paths

```bash
python3 scripts/bench/ner-bench-scorer.py --repo-root . --python python3 --mode all --model kiji-distilbert --model openobscure-tinybert4l-pii-ner-int8 --model mrm8488-mobilebert-ner --model osiria-minilm-italian-ner
python3 scripts/bench/ner-warm-latency.py --repo-root .
```

---

## How to reproduce

### The release run

Each release measures its own tree. The two steps below are the whole contract:

```bash
# 1. Produce the scorecard on the release commit.
uv sync --project scripts/bench --locked
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py full \
  --seed 20260710 --no-download

# 2. Commit it under its release name and regenerate this document.
cp target/bench-data/no-opf/scorecard-v4.json \
   docs/reference/benchmarks/scorecard-vX.Y.Z.json
uv run --project scripts/bench python scripts/bench/render_benchmark_doc.py \
  --scorecard docs/reference/benchmarks/scorecard-vX.Y.Z.json \
  --version vX.Y.Z \
  --machine "<CPU, cores, RAM, OS and build>" \
  --append-history
```

A quick smoke run uses the scorer's seeded stratified sampler instead of the
complete corpus:

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py quick --no-download
```

The first run may omit `--no-download`; the runner fetches and verifies the
pinned 2 MB Parquet file under ignored `target/bench-data/`.

`--machine` is required because the scorecard schema does not capture the host.
It is the one hand-carried reproducibility field, and it is recorded per release
in [`release-history.json`](release-history.json). Use a placeholder for any
path: `$HOME/...`, `~/...`, `<model-cache>/...`. **Never publish an absolute
home directory.**

### Where the numbers come from

The rendered table is a projection of the schema-v4 scorecard. This mapping is
the contract, enforced by
[`scripts/bench/test_render_benchmark_doc.py`](../../../scripts/bench/test_render_benchmark_doc.py),
which mutates each source path in turn and requires the rendered value to move:

| Column | Scorecard JSON path |
| --- | --- |
| Arm | `runs[].config` |
| Gold PII bytes | `runs[].metrics.utf8_bytes.pii` |
| Surviving PII bytes | `runs[].metrics.utf8_bytes.leaked` |
| Leak rate | `runs[].metrics.utf8_bytes.leak_rate` |
| False-positive bytes | `runs[].metrics.utf8_bytes.false_positive` |
| Byte precision | `runs[].metrics.utf8_bytes.precision` |
| Zero-leak documents | `runs[].metrics.zero_leak_document_rate` |
| Restore exact | `runs[].pipeline_contract.restore_exact_rate` |
| Manifest valid | `runs[].pipeline_contract.manifest_valid_document_rate` |
| Availability | `runs[].pipeline_availability.completion_rate` |
| Failed closed | `runs[].pipeline_availability.failed_closed_documents` |
| clean p95 ms | `runs[].latency_ms.clean_ms.p95` |

Provenance rows come from `gaze.revision`, `gaze.dirty`, `generated_at`,
`dataset.repository`, `dataset.revision`, `dataset.integrity`,
`parameters.profile`, `parameters.sampling_seed`, `parameters.ner_threshold`,
and `runner_provenance.entry_point`. A scorecard with `gaze.dirty: true` is
refused: a release row has to be reproducible.

Verify that the committed document still matches its history file:

```bash
python3 scripts/bench/render_benchmark_doc.py --check
```

This runs in CI on every pull request. It is stdlib-only and needs no corpus, no
model, and no network.

### The runner's other outputs

Beyond `scorecard-v4.json`, `run_no_opf_benchmark.py` writes a Markdown summary,
per-language / per-label / per-negative-category diagnostics, and separate
machine-readable regression and release-readiness verdicts under ignored
`target/bench-data/no-opf/`. Regression uses zero-tolerance integer-count
ratchets. Release readiness is an independent candidate-only verdict.
Performance tolerance is separately configured and informational by default.

Required model bundles are verified before any cell starts. Warmups, measured
repetitions, discarded warmup samples, and external cold-start to first
validated response are Python-runner provenance; response latency consumes the
producer's honest `clean_ms`. See
[`scripts/bench/README.md`](../../../scripts/bench/README.md) for model
locations, planning runtime, and the guarded baseline-acceptance command.

### Hardware spec template

Fill this out for every published or PR-local benchmark run; the `--machine`
string should summarise it:

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

### Locked Python harness

| File | Role |
| --- | --- |
| [scripts/bench/README.md](../../../scripts/bench/README.md) | Canonical runner contract, setup, outputs, verdicts, baseline acceptance |
| [scripts/bench/pyproject.toml](../../../scripts/bench/pyproject.toml) | Locked Python project configuration |
| [scripts/bench/uv.lock](../../../scripts/bench/uv.lock) | Exact Python dependency lock |
| [scripts/bench/no_opf_models.toml](../../../scripts/bench/no_opf_models.toml) | Canonical no-OPF model and producer-ID configuration |
| [scripts/bench/run_no_opf_benchmark.py](../../../scripts/bench/run_no_opf_benchmark.py) | Canonical local no-OPF regression entry point |
| [scripts/bench/gaze_bench_score.py](../../../scripts/bench/gaze_bench_score.py) | Shared scorecard, comparator, and verdict logic |
| [scripts/bench/render_benchmark_doc.py](../../../scripts/bench/render_benchmark_doc.py) | Renders this document's generated sections from the release history |
| [scripts/bench/dataiku_en_de_gaze_bench.py](../../../scripts/bench/dataiku_en_de_gaze_bench.py) | Dataiku EN/DE whole-pipeline producer |
| [scripts/bench/openpii_gaze_bench.py](../../../scripts/bench/openpii_gaze_bench.py) | Secondary OpenPII producer and scorer |
| [scripts/bench/gaze-pipeline-bench.py](../../../scripts/bench/gaze-pipeline-bench.py) | Coverage-loop pipeline snapshot generator |
| [scripts/bench/kiji-bench-scorer.py](../../../scripts/bench/kiji-bench-scorer.py) | Kiji direct and observer-residual scorer |
| [scripts/bench/opf-bench-scorer.py](../../../scripts/bench/opf-bench-scorer.py) | OPF direct and observer-residual scorer |
| [scripts/bench/ner-bench-scorer.py](../../../scripts/bench/ner-bench-scorer.py) | NER model-matrix scorer |
| [scripts/bench/ner-warm-latency.py](../../../scripts/bench/ner-warm-latency.py) | Warm NER latency runner |
| [scripts/bench/kiji-runner.py](../../../scripts/bench/kiji-runner.py) | Kiji subprocess adapter |
| [scripts/bench/onnx-token-classification-runner.py](../../../scripts/bench/onnx-token-classification-runner.py) | Generic ONNX token-classification adapter |
| [scripts/bench/transformers-runner.py](../../../scripts/bench/transformers-runner.py) | Transformers token-classification adapter |
| [scripts/bench/opf_daemon.py](../../../scripts/bench/opf_daemon.py) | Warm OPF diagnostic daemon and client bridge |
| [scripts/bench/safety_net_bench_lib.py](../../../scripts/bench/safety_net_bench_lib.py) | Shared fixture loading and strict scoring support |
| [scripts/bench/quantize-kiji-int8.py](../../../scripts/bench/quantize-kiji-int8.py) | Pinned Kiji int8 artifact preparation helper |

### Rust benchmarks and committed snapshots

| File | Role |
| --- | --- |
| [clean_for_bench.rs](../../../crates/gaze-recognizers/examples/clean_for_bench.rs) | Long-lived pipeline producer for coverage-loop benchmarking |
| [safety_net_matrix.rs](../../../crates/gaze-recognizers/benches/safety_net_matrix.rs) | SafetyNet matrix and in-process warm benchmark source |
| [runtime_comparison.rs](../../../crates/gaze-recognizers/benches/runtime_comparison.rs) | ORT/tract/Candle comparison source |
| [pipeline_end_to_end.rs](../../../crates/gaze/benches/pipeline_end_to_end.rs) | End-to-end pipeline snapshot assertion source |
| [tier4_pipeline_gating.rs](../../../crates/gaze/benches/tier4_pipeline_gating.rs) | Tier 4 gating benchmark source |
| [ner_models.toml](../../../crates/gaze-recognizers/benches/ner_models.toml) | NER model-matrix configuration |
| [ner_models_snapshot.json](../../../crates/gaze-recognizers/benches/ner_models_snapshot.json) | Committed NER leaderboard snapshot |
| [safety_net_matrix_snapshot.json](../../../crates/gaze-recognizers/benches/safety_net_matrix_snapshot.json) | Committed SafetyNet quality matrix |
| [safety_net_perf_snapshot.json](../../../crates/gaze-recognizers/benches/safety_net_perf_snapshot.json) | Committed one-shot SafetyNet performance snapshot |
| [gaze_pipeline_bench_snapshot.json](../../../crates/gaze-recognizers/benches/gaze_pipeline_bench_snapshot.json) | Committed end-to-end pipeline snapshot |

Runtime comparison (ORT vs tract vs candle; asserts non-ORT runtimes produce the
same span set as the ORT baseline):

```bash
GAZE_KIJI_DISTILBERT_MODEL_DIR="$HOME/.cache/gaze/<kiji-bundle>" cargo bench -p gaze-recognizers --features safety-net-kiji,runtime-tract,runtime-candle --bench runtime_comparison
```

End-to-end pipeline (`pass1_ms` is matching rule-floor wall clock; `pass3_ms` is
full-pipeline minus the rule-floor delta for Pass-3 configs):

```bash
python3 scripts/bench/gaze-pipeline-bench.py --repo-root . --no-update
cargo bench -p gaze-pii --bench pipeline_end_to_end
```

Tier 4 pipeline gating (opt-in observer-only skip gates, capitals heuristic,
prefix cache, length bucketing; asserts zero SafetyNet suspects for every config
in its synthetic fixture set). Contract:
[`docs/explanation/pipeline/tier4-pipeline-gating.md`](../../explanation/pipeline/tier4-pipeline-gating.md).

```bash
cargo bench -p gaze-pii --bench tier4_pipeline_gating --all-features
```

### Coverage-loop corpus

Used by the SafetyNet matrix, the NER leaderboard, and the pipeline benches:

| Field | Value |
| --- | --- |
| Corpus path | [`crates/gaze-recognizers/testdata/coverage-loop/corpus`](../../../crates/gaze-recognizers/testdata/coverage-loop/corpus) |
| Fixture count | `150` |
| Corpus SHA256 | `c6e78cca59df550fad18e59e9877da03da82c73b80c2368e5233d76353ccfa2f` |
| Coverage report SHA256 | `760f96163a68ce5f7dbc0409aa5109aa1a3ed190001536647e1881ba9d40a49c` |
| Build manifest | [`crates/gaze-recognizers/testdata/coverage-loop/build-manifest.json`](../../../crates/gaze-recognizers/testdata/coverage-loop/build-manifest.json) |

The corpus is synthetic by design and must remain free of real PII.

---

## Evidence before the release gate

Before v0.14.0, benchmark evidence was committed per pull request rather than
per release: a BASE half, a CANDIDATE half, two-run determinism repeats, and
bisect probes for each change. Those files did their job at review time, and
none of them was measured on a released tree — several were measured on branch
heads that squash-merge has since removed from history.

They are no longer carried in the working tree. Every one remains readable at
the `v0.13.0` tag, and the links below are pinned there permanently:

| Report | Era |
| --- | --- |
| [v0.12 consolidated post-wave scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-consolidated-post-wave-scorecard.md) | v0.12 |
| [v0.12 post-wave scorecard (`a8f7182`)](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-post-wave-a8f7182-scorecard.md) | v0.12 |
| [v0.12 government-ID scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-government-id-scorecard.md) | v0.12 |
| [v0.12 Kiji decoder scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-kiji-decoder-scorecard.md) | v0.12 |
| [v0.12 locale-basis drain scorecard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-locale-basis-drain-scorecard.md) | v0.12 |
| [#3025 slice U — structured containment](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025u-bfcf264-scorecard.md) | v0.12 |
| [#3025 slice G — shared gov-ID connector](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025g-edfb167-scorecard.md) | v0.12 |
| [#3025 slice A — passport + national_id](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-3025a-cfb3aed-scorecard.md) | v0.12 |
| [v0.12 EN/DE whole-pipeline baseline](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-en-de-whole-pipeline-baseline.md) | v0.12 |
| [v0.12 no-OPF Kiji error buckets](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-no-opf-error-buckets.md) | v0.12 |
| [v0.12 OpenPII external baseline](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-openpii-baseline.md) | v0.12 |
| [v0.12 warm OPF daemon sample](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.12-opf-daemon-sample.md) | v0.12 |
| [v0.9 safety-net benchmark](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-safety-net-benchmark.md) | v0.9 |
| [v0.9 NER model leaderboard](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-ner-model-leaderboard.md) | v0.9 |
| [v0.9 runtime comparison](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-runtime-comparison.md) | v0.9 |
| [v0.9 Gaze pipeline benchmark](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9-gaze-pipeline-benchmark.md) | v0.9 |
| [v0.9.0-rc.1 combined revalidation](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.9.0-rc1-combined-revalidation.md) | v0.9 |
| [v0.8 Kiji benchmark](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-benchmark.md) | v0.8 |
| [v0.8 Kiji class-taxonomy gap](https://github.com/CertaMesh/gaze/blob/v0.13.0/docs/reference/benchmarks/v0.8-kiji-class-gap.md) | v0.8 |

Their raw schema-v3 and schema-v4 scorecard JSONs are at the same tag. Historical
reports retain their original bounded claims and do **not** imply that they meet
current production targets.
