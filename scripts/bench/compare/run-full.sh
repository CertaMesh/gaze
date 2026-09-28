#!/usr/bin/env bash
# Measure each configuration on the same corpus and retain one aggregate report.
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

output="${GAZE_COMPARE_OUTPUT:-target/bench-data/compare-3909/comparison.json}"
args=(
    --dataset "${GAZE_COMPARE_DATASET:-target/bench-data/dataiku-en-de/test.parquet}"
    --en-model "$GAZE_COMPARE_EN_MODEL"
    --de-model "$GAZE_COMPARE_DE_MODEL"
    --nl-model "$GAZE_COMPARE_NL_MODEL"
    --fr-model "$GAZE_COMPARE_FR_MODEL"
    --pt-model "$GAZE_COMPARE_PT_MODEL"
    --gliner-model "$GAZE_COMPARE_GLINER_MODEL"
    --gliner-tokenizer "${GAZE_COMPARE_GLINER_TOKENIZER:-/synthetic/gliner_tokenizer}"
    --transformer-model "${GAZE_COMPARE_TRANSFORMER_MODEL:-/synthetic/transformer_model}"
    --gaze-policy "${GAZE_COMPARE_POLICY:-target/bench-data/compare-3909/policy.toml}"
    --output "$output"
)
if [[ -n "${GAZE_COMPARE_OPF_PYTHON:-}" || -n "${GAZE_COMPARE_OPF_CHECKPOINT:-}" ]]; then
    : "${GAZE_COMPARE_OPF_PYTHON:?set GAZE_COMPARE_OPF_PYTHON}"
    : "${GAZE_COMPARE_OPF_CHECKPOINT:?set GAZE_COMPARE_OPF_CHECKPOINT}"
    args+=(--opf-python "$GAZE_COMPARE_OPF_PYTHON" --opf-checkpoint "$GAZE_COMPARE_OPF_CHECKPOINT")
fi
if [[ ${1:-} == --dry-run ]]; then
    "$GAZE_COMPARE_PYTHON" scripts/bench/compare/compare.py "${args[@]}" \
        --tool all --validate-args-only
    exit
fi

: "${GAZE_COMPARE_SCRUB_PYTHON:?set GAZE_COMPARE_SCRUB_PYTHON to the scrubadub virtualenv Python}"
: "${GAZE_COMPARE_BINARY:?set GAZE_COMPARE_BINARY to clean_for_bench}"
: "${GAZE_COMPARE_MODEL_DIR:?set GAZE_COMPARE_MODEL_DIR to the Gaze NER model}"
: "${GAZE_COMPARE_POLICY_RULES:?set GAZE_COMPARE_POLICY_RULES}"
: "${GAZE_COMPARE_POLICY_RULES_NER:?set GAZE_COMPARE_POLICY_RULES_NER}"
: "${GAZE_COMPARE_TRANSFORMER_MODEL:?set GAZE_COMPARE_TRANSFORMER_MODEL}"
: "${GAZE_COMPARE_GLINER_TOKENIZER:?set GAZE_COMPARE_GLINER_TOKENIZER}"

"$GAZE_COMPARE_PYTHON" scripts/bench/compare/compare.py "${args[@]}" \
    --tool presidio-all --measure-gaze --gaze-binary "$GAZE_COMPARE_BINARY" \
    --gaze-model-dir "$GAZE_COMPARE_MODEL_DIR" \
    --gaze-policy-rules "$GAZE_COMPARE_POLICY_RULES" \
    --gaze-policy-rules-ner "$GAZE_COMPARE_POLICY_RULES_NER"

for tool in presidio-en presidio-en-de presidio-strong presidio-strong-high-recall \
    datafog-core datafog-regex datafog-spacy datafog-gliner gliner gliner-high-recall opf; do
    "$GAZE_COMPARE_PYTHON" scripts/bench/compare/compare.py "${args[@]}" --tool "$tool" --resume
done
for tool in scrubadub-base scrubadub-spacy; do
    "$GAZE_COMPARE_SCRUB_PYTHON" scripts/bench/compare/compare.py "${args[@]}" --tool "$tool" --resume
done
