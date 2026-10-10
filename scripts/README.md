# Scripts

Run these helpers from the repository root.

## Fetch

| Script | Purpose |
|---|---|
| `scripts/fetch/fetch-ner-model.sh` | Fetches and verifies the pinned Davlan mBERT NER bundle. |
| `scripts/fetch/fetch-openai-privacy-filter.sh` | Installs the pinned OpenAI Privacy Filter subprocess runtime. |

## Gate

| Script | Purpose |
|---|---|
| `scripts/gate/mcp-tier-isolation-mutation-probe.sh` | Un-gates the operator-tier surface and requires `cargo run -p xtask -- mcp-tier-isolation` to go red, then reverts, rebuilds, and requires green. |

## Bench

| Script | Purpose |
|---|---|
| `scripts/bench/openpii_gaze_bench.py` | Fetches, verifies, and scores the current pipeline on the pinned synthetic OpenPII holdout. |
| `scripts/bench/gaze-pipeline-bench.py` | Generates the end-to-end Gaze pipeline benchmark snapshot. |
| `scripts/bench/opf-bench-scorer.py` | Scores OpenAI Privacy Filter direct, observer-residual, and latency cells. |
| `scripts/bench/ner-bench-scorer.py` | Runs the config-driven multi-model NER leaderboard. |
| `scripts/bench/clean_scaling.py` | Times `gaze clean` on growing synthetic German inputs and, with `--compare`, diffs two binaries' outputs and restores. |
| `scripts/bench/ner-warm-latency.py` | Measures warm persistent-model latency for pinned NER candidates. |
| `scripts/bench/safety_net_bench_lib.py` | Shared fixtures, scoring, and snapshot helpers for benchmark scripts. |
| `scripts/bench/onnx-token-classification-runner.py` | Generic ONNX Runtime token-classification subprocess wrapper. |
| `scripts/bench/transformers-runner.py` | Generic Hugging Face transformers NER subprocess wrapper. |

## Verify

| Script | Purpose |
|---|---|
| `scripts/verify/cli-help-surface.sh` | Builds `gaze` at a base revision and at the working tree in one run and diffs `--help` for the root command and every subcommand, so a CLI refactor can be shown not to have moved the published surface. Refreshes `crates/gaze-cli/tests/fixtures/cli-help/` with `--write-fixtures`. |
