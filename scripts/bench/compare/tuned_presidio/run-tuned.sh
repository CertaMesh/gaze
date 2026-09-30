#!/usr/bin/env bash
# Record, select and measure tuned Presidio on the comparison corpus. Aggregates only.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$repo_root"
here=scripts/bench/compare/tuned_presidio
export PYTHONHASHSEED=0 HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 TOKENIZERS_PARALLELISM=false

: "${TUNED_COMPARE_PYTHON:?set TUNED_COMPARE_PYTHON to the ../requirements.lock Python}"
: "${TUNED_VENDOR_PYTHON:?set TUNED_VENDOR_PYTHON to the ../theirbench/requirements-theirbench.lock Python}"
: "${TUNED_OUT:?set TUNED_OUT to a directory outside the repository}"
: "${TUNED_PRESIDIO_RESEARCH:?set TUNED_PRESIDIO_RESEARCH to the presidio-research checkout}"
: "${GAZE_COMPARE_EN_MODEL:?}" "${GAZE_COMPARE_DE_MODEL:?}" "${GAZE_COMPARE_NL_MODEL:?}"
: "${GAZE_COMPARE_FR_MODEL:?}" "${GAZE_COMPARE_PT_MODEL:?}"
: "${GAZE_COMPARE_TRANSFORMER_MODEL:?}" "${GAZE_COMPARE_GLINER_MODEL:?}" "${GAZE_COMPARE_GLINER_TOKENIZER:?}"
: "${TUNED_DAVLAN:?}" "${TUNED_OPENMED:?}"
dataset="${GAZE_COMPARE_DATASET:-target/bench-data/dataiku-en-de/test.parquet}"
spacy=(--en-model "$GAZE_COMPARE_EN_MODEL" --de-model "$GAZE_COMPARE_DE_MODEL" --nl-model "$GAZE_COMPARE_NL_MODEL"
       --fr-model "$GAZE_COMPARE_FR_MODEL" --pt-model "$GAZE_COMPARE_PT_MODEL")
models=(--dslim "$GAZE_COMPARE_TRANSFORMER_MODEL" --davlan "$TUNED_DAVLAN" --openmed "$TUNED_OPENMED"
        --gliner "$GAZE_COMPARE_GLINER_MODEL")

for pass in base dslim davlan openmed gliner; do
    "$TUNED_COMPARE_PYTHON" "$here/produce.py" pool --pass "$pass" --dataset "$dataset" --out "$TUNED_OUT/pool" \
        "${spacy[@]}" "${models[@]}" --gliner-tokenizer "$GAZE_COMPARE_GLINER_TOKENIZER"
done
"$TUNED_VENDOR_PYTHON" "$here/produce.py" authors --dataset "$dataset" --out "$TUNED_OUT/authors" \
    --checkout "$TUNED_PRESIDIO_RESEARCH" --openmed "$TUNED_OPENMED"
"$TUNED_COMPARE_PYTHON" "$here/tune.py" select --dataset "$dataset" --pool "$TUNED_OUT/pool"
"$TUNED_COMPARE_PYTHON" "$here/tune.py" measure --dataset "$dataset" --pool "$TUNED_OUT/pool" \
    --authors "$TUNED_OUT/authors" "${spacy[@]}" "${models[@]}"
python3 scripts/bench/compare/render.py docs/reference/benchmarks/comparison.json \
    --page docs/reference/benchmarks/competitors.md
python3 scripts/bench/render_benchmark_doc.py
