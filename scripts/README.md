# Scripts

Repository helper scripts are grouped by purpose. Run paths below from the
repository root.

## Fetch

| Script | What it does | Who calls it |
|---|---|---|
| `scripts/fetch/fetch-ner-model.sh` | Fetches and verifies the pinned Davlan mBERT NER bundle. | Operators, adapters, and release validation. |
| `scripts/fetch/fetch-kiji-safetynet-model.sh` | Fetches and verifies the pinned Kiji DistilBERT safety-net bundle. | Operators and Kiji safety-net docs. |
| `scripts/fetch/fetch-openai-privacy-filter.sh` | Installs the pinned OpenAI Privacy Filter subprocess runtime. | Operators evaluating the OPF safety net. |

## Bench

| Script | What it does | Who calls it |
|---|---|---|
| `scripts/bench/openpii_gaze_bench.py` | Fetches, verifies, and scores the current pipeline on the pinned synthetic OpenPII holdout. | Maintainers measuring external multilingual leak coverage. |
| `scripts/bench/gaze-pipeline-bench.py` | Generates the end-to-end Gaze pipeline benchmark snapshot. | Maintainers refreshing benchmark evidence. |
| `scripts/bench/kiji-bench-scorer.py` | Scores Kiji direct, observer-residual, and latency benchmark cells. | Maintainers running safety-net benchmarks. |
| `scripts/bench/opf-bench-scorer.py` | Scores OpenAI Privacy Filter direct, observer-residual, and latency cells. | Maintainers running safety-net benchmarks. |
| `scripts/bench/ner-bench-scorer.py` | Runs the config-driven multi-model NER leaderboard. | Maintainers evaluating NER candidates. |
| `scripts/bench/ner-warm-latency.py` | Measures warm persistent-model latency for pinned NER candidates. | Maintainers evaluating low-latency NER options. |
| `scripts/bench/safety_net_bench_lib.py` | Shared fixtures, scoring, and snapshot helpers for benchmark scripts. | Other scripts in `scripts/bench/`. |
| `scripts/bench/kiji-runner.py` | Reference Kiji subprocess wrapper for the SafetyNet backend. | Operators and benchmark scorers. |
| `scripts/bench/onnx-token-classification-runner.py` | Generic ONNX Runtime token-classification subprocess wrapper. | NER leaderboard and warm-latency scripts. |
| `scripts/bench/transformers-runner.py` | Generic Hugging Face transformers NER subprocess wrapper. | NER leaderboard scorer. |
| `scripts/bench/quantize-kiji-int8.py` | Quantizes an already-fetched Kiji ONNX bundle to int8 and writes checksums. | Operators and Kiji benchmark setup. |
