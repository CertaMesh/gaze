#!/usr/bin/env bash
# Run the full comparison against scorecards prepared for the same corpus.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

if [[ $# -gt 1 || ( $# -eq 1 && $1 != --dry-run ) ]]; then
    echo "usage: run-full.sh [--dry-run]" >&2
    exit 2
fi

: "${GAZE_COMPARE_EN_MODEL:?set GAZE_COMPARE_EN_MODEL}"
: "${GAZE_COMPARE_DE_MODEL:?set GAZE_COMPARE_DE_MODEL}"
: "${GAZE_COMPARE_NL_MODEL:?set GAZE_COMPARE_NL_MODEL}"
: "${GAZE_COMPARE_FR_MODEL:?set GAZE_COMPARE_FR_MODEL}"
: "${GAZE_COMPARE_PT_MODEL:?set GAZE_COMPARE_PT_MODEL}"
: "${GAZE_COMPARE_GLINER_MODEL:?set GAZE_COMPARE_GLINER_MODEL}"
: "${GAZE_COMPARE_PYTHON:?set GAZE_COMPARE_PYTHON to the comparison virtualenv Python}"

args=(
    --dataset "${GAZE_COMPARE_DATASET:-target/bench-data/dataiku-en-de/test.parquet}"
    --en-model "$GAZE_COMPARE_EN_MODEL"
    --de-model "$GAZE_COMPARE_DE_MODEL"
    --nl-model "$GAZE_COMPARE_NL_MODEL"
    --fr-model "$GAZE_COMPARE_FR_MODEL"
    --pt-model "$GAZE_COMPARE_PT_MODEL"
    --gliner-model "$GAZE_COMPARE_GLINER_MODEL"
    --gaze-policy "${GAZE_COMPARE_POLICY:-target/bench-data/compare-3909/policy.toml}"
    --gaze-scorecard-v1 "${GAZE_COMPARE_SCORECARD_V1:-target/bench-data/compare-3909/gaze-v1/full/scorecard-v4.json}"
    --gaze-scorecard-v2 "${GAZE_COMPARE_SCORECARD_V2:-target/bench-data/compare-3909/gaze-v2/full/scorecard-v4.json}"
    --gaze-scorecard-v3 "${GAZE_COMPARE_SCORECARD_V3:-target/bench-data/compare-3909/gaze-v3/full/scorecard-v4.json}"
    --output "${GAZE_COMPARE_OUTPUT:-target/bench-data/compare-3909/comparison.json}"
)
if [[ -n "${GAZE_COMPARE_OPF_PYTHON:-}" || -n "${GAZE_COMPARE_OPF_CHECKPOINT:-}" ]]; then
    : "${GAZE_COMPARE_OPF_PYTHON:?set GAZE_COMPARE_OPF_PYTHON}"
    : "${GAZE_COMPARE_OPF_CHECKPOINT:?set GAZE_COMPARE_OPF_CHECKPOINT}"
    args+=(--opf-python "$GAZE_COMPARE_OPF_PYTHON" --opf-checkpoint "$GAZE_COMPARE_OPF_CHECKPOINT")
fi
if [[ ${1:-} == --dry-run ]]; then
    args+=(--validate-args-only)
fi

"$GAZE_COMPARE_PYTHON" scripts/bench/compare/compare.py "${args[@]}"
