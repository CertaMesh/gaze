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
| Davlan multilingual BERT NER | `~/.local/share/gaze/models/davlan-mbert-ner-hrl` | `davlan-bert-multilingual.bundle_sha` in `crates/gaze-recognizers/benches/ner_models.toml` | SHA-256 over the complete deterministic file tree |
| Kiji DistilBERT | `~/.local/share/gaze/models/kiji-distilbert` | `kiji-distilbert.bundle_sha` in the same TOML | pinned `SHA256SUMS` digest, then every listed artifact digest |

Override locations with `--model-dir` and `--kiji-model-dir`. The runner rejects
symlinks in validated bundle material.

## Outputs and verdicts

Generated artifacts are under ignored `target/bench-data/no-opf/<profile>/`:

| Artifact | Purpose |
| --- | --- |
| `scorecard-v3.json` | full schema-v3 scorecard and runner provenance |
| `summary.md` | concise human-readable result |
| `diagnostics.json` | per-language, per-label, and per-negative-category diagnostics |
| `regression-status.json` | baseline-relative integer-count ratchet verdict |
| `release-readiness-status.json` | candidate-only absolute correctness verdict |
| `performance-status.json` | separately configured p95 `clean_ms` comparison |
| `logs/` | subprocess stderr by repetition and config |

Regression and release readiness are deliberately independent. Regression uses
integer counts with zero tolerance and fails closed on missing, empty, invalid,
or population-mismatched candidates. Release readiness checks the production
candidate cell itself: no leaked labeled bytes, uncovered entities, pipeline
failures, restore failures, invalid manifests, telemetry disagreement,
unscanned documents, residual suspects, or redact actions. A full command exits
nonzero for either correctness failure.

A quick result is always marked not release-ready because it samples the
population. Quick exits successfully when that incompleteness is its only
readiness failure, but any observed correctness failure still exits nonzero.

Performance compares `timing.clean_ms.p95` with
`--performance-tolerance-percent` and is informational by default. Add
`--performance-gating` only for an explicitly reviewed performance gate.
Warmup count, measured-repetition count, every discarded warmup sample, and
external process/model cold-start-to-first-validated-response are recorded in
`runner_provenance`. Cold start is separate from Rust response timing.

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
