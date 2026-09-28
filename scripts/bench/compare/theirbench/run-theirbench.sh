#!/usr/bin/env bash
# Reproduce the vendors' numbers, score every row on both sets, then score
# each row's spans with the set's own evaluator. Aggregates only.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$repo_root"
here=scripts/bench/compare/theirbench

: "${THEIRBENCH_VENDOR_PYTHON:?set THEIRBENCH_VENDOR_PYTHON to the requirements-theirbench.lock Python}"
: "${THEIRBENCH_COMPARE_PYTHON:?set THEIRBENCH_COMPARE_PYTHON to the comparison Python}"
: "${THEIRBENCH_PRESIDIO_RESEARCH:?set THEIRBENCH_PRESIDIO_RESEARCH to the presidio-research checkout}"
: "${THEIRBENCH_PIIBENCH:?set THEIRBENCH_PIIBENCH to the pii-bench checkout}"
: "${THEIRBENCH_PIIBENCH_DATA:?set THEIRBENCH_PIIBENCH_DATA to piibench_commercial.py --output-dir}"
: "${THEIRBENCH_OPENMED:?set THEIRBENCH_OPENMED to the pinned OpenMed snapshot}"
out="${THEIRBENCH_OUT:-target/bench-data/theirbench}"
mkdir -p "$out"

"$THEIRBENCH_VENDOR_PYTHON" "$here/presidio_research_repro.py" --checkout "$THEIRBENCH_PRESIDIO_RESEARCH" \
    --openmed-model "$THEIRBENCH_OPENMED" --output "$out/presidio-research-repro.json"
"$THEIRBENCH_VENDOR_PYTHON" "$here/piibench_repro.py" --checkout "$THEIRBENCH_PIIBENCH" \
    --data-dir "$THEIRBENCH_PIIBENCH_DATA" --output "$out/piibench-repro.json"
# The published notebook numbers predate later evaluator fixes: reproduce each
# with the evaluator commit that produced it (REPRODUCTION_COMMITS), imported
# from a checkout at that commit.
: "${THEIRBENCH_PR_VANILLA_CHECKOUT:?set THEIRBENCH_PR_VANILLA_CHECKOUT to presidio-research at REPRODUCTION_COMMITS[vanilla]}"
: "${THEIRBENCH_PR_CUSTOM_CHECKOUT:?set THEIRBENCH_PR_CUSTOM_CHECKOUT to presidio-research at REPRODUCTION_COMMITS[custom]}"
PYTHONPATH="$THEIRBENCH_PR_VANILLA_CHECKOUT" "$THEIRBENCH_VENDOR_PYTHON" "$here/presidio_research_repro.py" \
    --reproduction --config vanilla --checkout "$THEIRBENCH_PR_VANILLA_CHECKOUT" --output "$out/historical-vanilla.json"
PYTHONPATH="$THEIRBENCH_PR_CUSTOM_CHECKOUT" "$THEIRBENCH_VENDOR_PYTHON" "$here/presidio_research_repro.py" \
    --reproduction --config custom --checkout "$THEIRBENCH_PR_CUSTOM_CHECKOUT" \
    --openmed-model "$THEIRBENCH_OPENMED" --output "$out/historical-custom.json"

# The remaining arguments are compare.py's tool flags (models, Gaze binary and policies).
"$THEIRBENCH_COMPARE_PYTHON" "$here/theirbench.py" --benchmark presidio-research \
    --presidio-research-checkout "$THEIRBENCH_PRESIDIO_RESEARCH" \
    --predictions-dir "$out/pred-presidio-research" --output "$out/presidio-research.json" "$@"
"$THEIRBENCH_COMPARE_PYTHON" "$here/theirbench.py" --benchmark piibench-commercial \
    --piibench-data "$THEIRBENCH_PIIBENCH_DATA" \
    --predictions-dir "$out/pred-piibench-commercial" --output "$out/piibench-commercial.json" "$@"

own=()
for prediction in "$out"/pred-presidio-research/*.test.jsonl; do
    system="$(basename "$prediction" .test.jsonl)"
    family="$("$THEIRBENCH_COMPARE_PYTHON" -c 'import sys; sys.path.insert(0, sys.argv[1]); import theirbench; print(theirbench.tool_family(sys.argv[2]))' "$here" "$system")"
    "$THEIRBENCH_VENDOR_PYTHON" "$here/presidio_research_repro.py" --checkout "$THEIRBENCH_PRESIDIO_RESEARCH" \
        --predictions "$prediction" --labels "$out/pred-presidio-research/labels.$family.json" \
        --system "$system" --output "$out/own-presidio-research-$system.json"
    own+=(--own "presidio-research=$out/own-presidio-research-$system.json")
done
for prediction in "$out"/pred-piibench-commercial/*.test.jsonl; do
    system="$(basename "$prediction" .test.jsonl)"
    family="$("$THEIRBENCH_COMPARE_PYTHON" -c 'import sys; sys.path.insert(0, sys.argv[1]); import theirbench; print(theirbench.tool_family(sys.argv[2]))' "$here" "$system")"
    "$THEIRBENCH_VENDOR_PYTHON" "$here/piibench_repro.py" --checkout "$THEIRBENCH_PIIBENCH" \
        --data-dir "$THEIRBENCH_PIIBENCH_DATA" --predictions "$prediction" \
        --labels "$out/pred-piibench-commercial/labels.$family.json" \
        --system "$system" --output "$out/own-piibench-commercial-$system.json"
    own+=(--own "piibench-commercial=$out/own-piibench-commercial-$system.json")
done

"$THEIRBENCH_COMPARE_PYTHON" "$here/render_theirbench.py" assemble \
    --report "$out/presidio-research.json" --report "$out/piibench-commercial.json" "${own[@]}" \
    --reproduction "presidio-research=$out/presidio-research-repro.json" \
    --reproduction "piibench-commercial=$out/piibench-repro.json" \
    --historical "$out/historical-vanilla.json" --historical "$out/historical-custom.json"
"$THEIRBENCH_COMPARE_PYTHON" "$here/render_theirbench.py" render
