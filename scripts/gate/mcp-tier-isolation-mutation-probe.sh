#!/usr/bin/env bash
# Mutation probe for the `mcp-tier-isolation` xtask gate (solo todo #2993).
#
# A gate that cannot fail is indistinguishable from no gate. This script proves
# the tier gate is causally connected to the thing it guards, by un-gating the
# operator-tier surface and requiring the gate to go red.
#
# For each mutation case it runs the same four steps and prints exact exit
# codes:
#
#   1. baseline   — gate on unmodified source        MUST exit 0
#   2. mutate     — un-gate an operator-tier surface
#   3. mutated    — gate on mutated source           MUST exit non-zero
#   4. reverted   — revert, rebuild, gate again      MUST exit 0
#
# Step 4 rebuilds rather than trusting a cached artifact: a stale test binary
# reports a confident, wrong result.
#
# Usage (from the repository root):
#   scripts/gate/mcp-tier-isolation-mutation-probe.sh
#   scripts/gate/mcp-tier-isolation-mutation-probe.sh deep-path
#
# Cases: `deep-path` (default set includes it) un-gates the three operator tool
# modules in src/tools/mod.rs, which `pub mod tools` exposes ungated;
# `full-surface` additionally un-gates the `operator_tools` re-export in
# src/lib.rs. Pass one or more case names to narrow the run.

set -u -o pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

TOOLS_MOD="crates/gaze-mcp-core/src/tools/mod.rs"
LIB_RS="crates/gaze-mcp-core/src/lib.rs"
GATE=(cargo run -p xtask -- mcp-tier-isolation)
LOG_DIR="${TIER_PROBE_LOG_DIR:-target/tier-isolation-mutation-probe}"

ALL_CASES=(deep-path full-surface)
CASES=("$@")
if [ "${#CASES[@]}" -eq 0 ]; then
    CASES=("${ALL_CASES[@]}")
fi

mkdir -p "$LOG_DIR"

restore_sources() {
    git checkout -- "$TOOLS_MOD" "$LIB_RS" 2>/dev/null || true
}
trap restore_sources EXIT

require_clean() {
    local dirty
    dirty="$(git status --porcelain -- "$TOOLS_MOD" "$LIB_RS")"
    if [ -n "$dirty" ]; then
        echo "FATAL: refusing to run — these sources already have uncommitted changes:"
        echo "$dirty"
        echo "The probe rewrites and then reverts them via 'git checkout --'."
        exit 2
    fi
}

# Runs the gate, echoes its exit code, keeps the full log.
run_gate() {
    local label="$1"
    local log="$LOG_DIR/$label.log"
    "${GATE[@]}" >"$log" 2>&1
    local code=$?
    echo "$code"
}

# Deletes every `#[cfg(feature = "operator-tier")]` line in a file, leaving the
# item it guarded unconditionally compiled.
ungate() {
    local file="$1"
    local before after
    before="$(grep -c '#\[cfg(feature = "operator-tier")\]' "$file")"
    if [ "$before" -eq 0 ]; then
        echo "FATAL: no operator-tier cfg gate found in $file — the probe is stale." >&2
        exit 2
    fi
    grep -v '#\[cfg(feature = "operator-tier")\]' "$file" >"$file.probe-tmp"
    mv "$file.probe-tmp" "$file"
    after="$(grep -c '#\[cfg(feature = "operator-tier")\]' "$file" || true)"
    echo "  un-gated $file: removed $before cfg attribute(s), $after remain"
}

apply_mutation() {
    case "$1" in
        deep-path)
            ungate "$TOOLS_MOD"
            ;;
        full-surface)
            ungate "$TOOLS_MOD"
            ungate "$LIB_RS"
            ;;
        *)
            echo "FATAL: unknown case '$1' (known: ${ALL_CASES[*]})" >&2
            exit 2
            ;;
    esac
}

overall=0

for case_name in "${CASES[@]}"; do
    echo "=============================================================="
    echo "case: $case_name"
    echo "=============================================================="
    require_clean

    echo "[1/4] baseline gate on unmodified source"
    baseline=$(run_gate "$case_name-1-baseline")
    echo "      exit=$baseline (expected 0)"

    echo "[2/4] applying mutation"
    apply_mutation "$case_name"

    echo "[3/4] gate on mutated source"
    mutated=$(run_gate "$case_name-3-mutated")
    echo "      exit=$mutated (expected non-zero)"

    echo "[4/4] reverting, rebuilding, re-running"
    restore_sources
    # Force a rebuild rather than trusting a cached test binary.
    touch "$TOOLS_MOD" "$LIB_RS"
    reverted=$(run_gate "$case_name-4-reverted")
    echo "      exit=$reverted (expected 0)"

    verdict="PASS"
    [ "$baseline" -eq 0 ] || verdict="FAIL"
    [ "$mutated" -ne 0 ] || verdict="FAIL"
    [ "$reverted" -eq 0 ] || verdict="FAIL"

    echo
    echo "  case=$case_name baseline=$baseline mutated=$mutated reverted=$reverted -> $verdict"
    if [ "$verdict" = "PASS" ]; then
        echo "  what the mutated run reported:"
        grep -E 'expected test case to fail to compile|mcp_tier_isolation: .*tier boundary|^error' \
            "$LOG_DIR/$case_name-3-mutated.log" | head -8 | sed 's/^/    /'
    else
        overall=1
        echo "  logs: $LOG_DIR/$case_name-*.log"
    fi
    echo
done

if [ "$overall" -eq 0 ]; then
    echo "mutation probe: PASS — the gate fails when the tier boundary is violated."
else
    echo "mutation probe: FAIL — the gate did not react to a tier violation."
fi
exit "$overall"
