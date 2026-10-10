#!/usr/bin/env bash
# Mutation probe for the `mcp-tier-isolation` xtask gate.
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

MUTATED_FILES=()
SNAPSHOT_DIR=""
RESTORE_DIRS=()
CRITICAL=0
PENDING_SIGNAL=0
RESTORE_FAILED=0

on_signal() {
    # Defer exit while ownership or recovery data is changing. The first
    # signal wins; EXIT cleanup must never interrupt itself.
    if [ "$PENDING_SIGNAL" -eq 0 ]; then PENDING_SIGNAL="$1"; fi
    if [ "$CRITICAL" -eq 0 ]; then
        CRITICAL=1
        cleanup "$PENDING_SIGNAL"
    fi
}

end_critical() {
    CRITICAL=0
    if [ "$PENDING_SIGNAL" -ne 0 ]; then exit "$PENDING_SIGNAL"; fi
}

protected_command() (
    # Parent traps record INT/TERM; children must finish the critical command.
    # Ignoring signals in the parent itself would lose the requested exit code.
    trap '' INT TERM
    "$@"
)

remove_restore_dirs() {
    local directory failed=0
    for directory in ${RESTORE_DIRS[@]+"${RESTORE_DIRS[@]}"}; do
        protected_command rm -rf "$directory" || failed=1
    done
    [ "$failed" -eq 0 ] || return 1
    RESTORE_DIRS=()
}

remove_temporary_dirs() {
    remove_restore_dirs || return 1
    if [ -n "$SNAPSHOT_DIR" ]; then
        protected_command rm -rf "$SNAPSHOT_DIR" || return 1
        SNAPSHOT_DIR=""
    fi
}

restore_sources() {
    local file temporary directory failed=0
    CRITICAL=1
    # Never retry a failed restoration in EXIT cleanup or discard its backups.
    if [ "$RESTORE_FAILED" -ne 0 ]; then return 1; fi
    for file in ${MUTATED_FILES[@]+"${MUTATED_FILES[@]}"}; do
        # Read the complete snapshot into a run-owned directory on the source
        # filesystem before atomically replacing the source. A missing or
        # unreadable snapshot therefore cannot truncate the source. cp -p
        # preserves its original mode as well as its actual checkout bytes.
        directory="$(protected_command mktemp -d "${file%/*}/.gaze-tier-probe.XXXXXX")" || { failed=1; break; }
        RESTORE_DIRS+=("$directory")
        temporary="$directory/original"
        if ! protected_command cp -p "$SNAPSHOT_DIR/$file" "$temporary" ||
           ! protected_command mv -f "$temporary" "$file"; then
            failed=1
            break
        fi
    done
    if [ "$failed" -ne 0 ]; then
        RESTORE_FAILED=1
        remove_restore_dirs || echo "FATAL: could not remove restore staging directories" >&2
        echo "FATAL: failed to restore sources; recovery snapshots retained at $SNAPSHOT_DIR" >&2
        return 1
    fi
    # Ownership ends only after ALL replacements succeed. No later cleanup
    # can read a snapshot that snapshot deletion has already removed.
    MUTATED_FILES=()
    remove_temporary_dirs || return 1
    end_critical
}

ensure_snapshot_dir() {
    if [ -z "$SNAPSHOT_DIR" ]; then
        # Register mktemp's result before honoring a pending signal.
        CRITICAL=1
        SNAPSHOT_DIR="$(protected_command mktemp -d "${TMPDIR:-/tmp}/gaze-tier-probe.XXXXXX")"
        local code=$?
        end_critical
        [ "$code" -eq 0 ] || return 1
    fi
}

snapshot_source() {
    local file="$1" owned
    for owned in ${MUTATED_FILES[@]+"${MUTATED_FILES[@]}"}; do
        [ "$owned" != "$file" ] || return 0
    done
    ensure_snapshot_dir || return 1
    mkdir -p "$SNAPSHOT_DIR/${file%/*}" || return 1
    cp -p "$file" "$SNAPSHOT_DIR/$file" || return 1
    MUTATED_FILES+=("$file")
}

cleanup() {
    local code="$1" CRITICAL=1
    trap - EXIT
    # Keep recording signals until the final exit, including deletion. Do not
    # reinstate default signal handling halfway through restoration.
    restore_sources || code=1
    if [ "$code" -ne 1 ] && [ "$PENDING_SIGNAL" -ne 0 ]; then code="$PENDING_SIGNAL"; fi
    exit "$code"
}

require_clean() {
    local dirty
    dirty="$(GIT_OPTIONAL_LOCKS=0 git status --porcelain -- "$TOOLS_MOD" "$LIB_RS")" || exit 2
    if [ -n "$dirty" ]; then
        echo "FATAL: refusing to run — these sources already have uncommitted changes:"
        echo "$dirty"
        echo "The probe rewrites and then restores its mutations from working-tree snapshots."
        exit 2
    fi
}

# Refusals must not arm restoration or even start a valid earlier case.
for case_name in "${CASES[@]}"; do
    case "$case_name" in
        deep-path|full-surface) ;;
        *)
            echo "FATAL: unknown case '$case_name' (known: ${ALL_CASES[*]})" >&2
            exit 2
            ;;
    esac
done
require_clean
mkdir -p "$LOG_DIR" || exit 2
trap 'cleanup "$?"' EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

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
    local temporary
    # Own the directory before allocating intermediates so EXIT cleanup also
    # removes them when interrupted during preparation or the source write.
    ensure_snapshot_dir || exit 2
    temporary="$(mktemp "$SNAPSHOT_DIR/${file##*/}.probe-tmp.XXXXXX")" || exit 2
    # grep exits 1 when all lines were removed; that is a valid mutation.
    grep -v '#\[cfg(feature = "operator-tier")\]' "$file" >"$temporary"
    local code=$?
    if [ "$code" -gt 1 ]; then
        rm -f "$temporary"
        exit 2
    fi
    # Snapshot immediately before this path's first truncating write.
    if ! snapshot_source "$file"; then
        rm -f "$temporary"
        exit 2
    fi
    if ! cat "$temporary" >"$file"; then
        rm -f "$temporary"
        exit 2
    fi
    rm -f "$temporary"
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
    restore_sources || exit 1
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
