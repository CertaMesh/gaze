# Gaze benchmark scripts

The one canonical local regression entry point is:

```bash
scripts/bench/run_no_opf_benchmark.py
```

The other files in this directory are scorer, producer, or specialized research
components. They are not alternate release-regression entry points.

## Setup and profiles

Create the locked Python environment once:

```bash
uv sync --project scripts/bench --locked
```

Use `quick` while developing. It takes a seeded, deterministic stratified
sample through `gaze_bench_score.stratified_sample`:

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  quick --seed 20260710 --no-download
```

Use `full` for a local release candidate. It evaluates every English/German
document selected from the pinned Dataiku test split and all 1,024 committed A4
negative fixtures:

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  full --seed 20260710 --no-download \
  --compare-baseline target/bench-data/no-opf/baseline.json
```

Omit `--no-download` on the first run to fetch and SHA-256 verify the pinned
Dataiku Parquet file. Both profiles use only `rule-floor-extended`, `pass2-ner`,
and `full-stack-kiji-resolve`. The runner removes OPF environment variables and
passes no OPF command, checkpoint, or daemon socket, even if the invoking shell
defines them.

Planning estimates on a modern laptop are roughly 2–10 minutes for the default
256-document quick profile and 30–120 minutes per measured full repetition.
Thermals, CPU runtime, and filesystem cache state can move those estimates
substantially; A6 records the authoritative observed runtime.

## Required local models

The runner validates both model bundles before it builds or starts a benchmark
cell. Missing or mismatched bytes are typed, actionable failures; no cell is
silently skipped.

| Model | Default location | Pin source | Validation |
| --- | --- | --- | --- |
| Davlan multilingual BERT NER (production pass2 ONNX) | `~/.local/share/gaze/models/davlan-mbert-ner-hrl` | `[pass2_ner]` in `scripts/bench/no_opf_models.toml` | pinned `SHA256SUMS` digest, exact seven-artifact manifest and eight-file bundle surface, then every artifact digest |
| Kiji DistilBERT | `~/.local/share/gaze/models/kiji-distilbert` | `kiji-distilbert.bundle_sha` in `crates/gaze-recognizers/benches/ner_models.toml` | pinned `SHA256SUMS` digest, then every listed artifact digest |

Override locations with `--model-dir` and `--kiji-model-dir`. The runner rejects
symlinks in validated bundle material. Davlan's canonical bundle contains exactly
`model.onnx`, `tokenizer.json`, `config.json`, `tokenizer_config.json`,
`special_tokens_map.json`, `vocab.txt`, `labels.json`, and `SHA256SUMS`. Its
production provenance is repository
`onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX`, commit
`cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8`, and canonical manifest digest
`7b0b9d0d200bf7f3a39654257f8723998316600852edff8404834eb7edfc5c16`.
The manifest must list exactly the seven runtime artifacts and match that digest;
the directory may contain no other files, directories, or symlinks. Missing
artifacts, Transformers or safetensors weights, cache metadata, and all other
extras fail closed.

This production ONNX pass2 pin is intentionally separate from
`crates/gaze-recognizers/benches/ner_models.toml`. That file remains the research
Transformers model matrix and is not the Davlan source of truth for the canonical
no-OPF runner. Kiji continues to load from its existing `ner_models.toml` entry.

At scorer initialization, stable provenance IDs are loaded from every committed
`crates/gaze-recognizers/embedded/*.toml` recognizer and from the model IDs in
`scripts/bench/no_opf_models.toml`. That file's `[builtin_source_ids]` declaration
adds built-in producer IDs only from committed ground truth; tests require every
declared ID to appear as a string literal in the read-only `gaze-recognizers`
sources. Runtime producer assertions never extend this vocabulary. An exact,
case-sensitive vocabulary match may skip only the protected-content reproduction
check; source-ID grammar, ordering, non-empty, and uniqueness validation still
apply. Missing, unreadable, malformed, or empty committed vocabulary inputs abort
scoring with a typed error.

## Outputs and verdicts

Generated artifacts are under ignored `target/bench-data/no-opf/<profile>/`:

| Artifact | Purpose |
| --- | --- |
| `scorecard-v3.json` | full schema-v3 scorecard, runner provenance, and identified scored/failed-closed populations per cell |
| `summary.md` | concise human-readable result |
| `diagnostics.json` | per-language, per-label, and per-negative-category diagnostics |
| `regression-status.json` | baseline-relative integer-count ratchet verdict |
| `release-readiness-status.json` | candidate-only absolute correctness verdict |
| `performance-status.json` | separately configured p95 `clean_ms` comparison |
| `logs/` | subprocess stderr by repetition and config |

Regression and release readiness are deliberately independent. Regression uses
integer counts with zero tolerance and fails closed on missing, empty, invalid,
or population-mismatched candidates. Release readiness requires every candidate
cell to have no pipeline, restore, manifest, telemetry-agreement, or strict
rejection failures. The production candidate cell must additionally have no
leaked labeled bytes, uncovered entities, unscanned documents, residual suspects,
or redact actions. A full command exits nonzero for either correctness failure.

Every cell records sorted `scored_population.document_ids` and
`failed_closed_population.document_ids` plus a SHA-256 digest over each list.
Failed-closed entries also name the synthetic document ID, closed failure reason,
and closed stage; their reason/stage counts must reconcile exactly to the
failed-closed total. Regression comparison requires scored-set identity before
emitting any metric or per-label delta. A mismatch names the IDs added to and
removed from the scored set even when cardinality and gold counts are unchanged.

These fields are an additive schema-v3 extension. Older schema-v3 artifacts
remain readable as historical evidence, but because they do not identify each
cell's scored set they cannot pass the current like-for-like population or
per-label evaluability gates. Generate a new baseline with the current harness
before using those gates; the committed historical artifact is not rewritten.

A quick result is always marked not release-ready because it samples the
population. Quick exits successfully when that incompleteness is its only
readiness failure, but any observed correctness failure still exits nonzero.

Performance compares `timing.clean_ms.p95` with
`--performance-tolerance-percent` and is informational by default. Add
`--performance-gating` only for an explicitly reviewed performance gate.
Warmup count, measured-repetition count, every discarded warmup sample and its
outcome, and external process/model cold-start-to-first-validated-response are
recorded in `runner_provenance`. Warmups are timing-only: their pipeline and
correctness outcomes never abort or contribute to the scorecard. Each document is
counted exactly once in the scored pass. Cold start is separate from Rust response
timing.

## Baseline acceptance

Baseline replacement has three guards: a full profile, an exact review
confirmation, and a release-ready candidate. Replacing an existing file also
requires a regression-clean comparison against that same file.

```bash
uv run --project scripts/bench python scripts/bench/run_no_opf_benchmark.py \
  full --no-download \
  --compare-baseline target/bench-data/no-opf/baseline.json \
  --accept-baseline target/bench-data/no-opf/baseline.json \
  --accept-baseline-confirm I_HAVE_REVIEWED_FULL_RESULTS
```

Review `scorecard-v3.json`, all three status files, `diagnostics.json`, and the
stderr logs before using that command. Quick results and failed candidates can
never replace a baseline.

To initialize a baseline only when the target does not yet exist, omit
`--compare-baseline` but keep the full profile and exact confirmation. The
runner refuses to overwrite an existing file through this initialization path.

## Model-free verification

Normal CI runs only the locked Python tests:

```bash
uv run --project scripts/bench python -m unittest discover -s scripts/bench
```

CI does not download models or run either benchmark profile. The authoritative
full model run and baseline/error-bucket analysis belong to A6.
