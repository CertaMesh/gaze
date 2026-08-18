#!/usr/bin/env bash
#
# Prove that a `gaze` CLI refactor left the user-facing surface unchanged.
#
# Builds the `gaze` binary at a BASE revision and at the current working tree,
# captures `--help` for the root command and every reachable subcommand from
# BOTH binaries in the same run, and diffs them. The comparison is always
# before-vs-after; it is never against a checked-in constant, so the harness
# cannot go stale and cannot pass because someone refreshed a golden file.
#
#   scripts/verify/cli-help-surface.sh                 # base = merge-base with origin/main
#   scripts/verify/cli-help-surface.sh --base <rev>    # explicit base revision
#   scripts/verify/cli-help-surface.sh --write-fixtures # refresh the committed capture
#
# Exit 0 = surfaces identical. Exit 1 = a diff (printed). Exit 2 = harness error.
#
# Known blind spot: `--help` does not render hidden commands or hidden args.
# HIDDEN_PATHS below names the hidden command paths explicitly; hidden *args*
# (for example `proxy serve --_foreground-daemon`) are covered instead by the
# clap-model parity tests in crates/gaze-cli/src/commands/mod.rs.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE_DIR="${REPO_ROOT}/crates/gaze-cli/tests/fixtures/cli-help"
FEATURES="--all-features"

# Hidden command paths, captured explicitly because they never appear under
# `Commands:` in help output. Space-separated argv, one path per line.
HIDDEN_PATHS=(
  "proxy _dashboard-child"
)

die() { printf 'cli-help-surface: %s\n' "$*" >&2; exit 2; }

BASE_REV=""
WRITE_FIXTURES=0
OUT_DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --base) BASE_REV="${2:-}"; shift 2 || die "--base needs a revision" ;;
    --out) OUT_DIR="${2:-}"; shift 2 || die "--out needs a directory" ;;
    --write-fixtures) WRITE_FIXTURES=1; shift ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

command -v cargo >/dev/null 2>&1 || die "cargo not on PATH"
cd "$REPO_ROOT" || die "cannot cd to repo root"

if [ -z "$BASE_REV" ]; then
  BASE_REV="$(git merge-base HEAD origin/main 2>/dev/null)" \
    || die "cannot resolve a base revision; pass --base <rev>"
fi
BASE_SHA="$(git rev-parse --verify "${BASE_REV}^{commit}" 2>/dev/null)" \
  || die "not a commit: ${BASE_REV}"

WORK_DIR="${OUT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/gaze-cli-help.XXXXXX")}"
mkdir -p "${WORK_DIR}/before" "${WORK_DIR}/after" || die "cannot create ${WORK_DIR}"
BASE_TREE="${WORK_DIR}/base-tree"

cleanup() {
  if [ -d "$BASE_TREE" ]; then
    git -C "$REPO_ROOT" worktree remove --force "$BASE_TREE" >/dev/null 2>&1
  fi
}
trap cleanup EXIT

# Capture root + every reachable subcommand's help into $2, using binary $1.
# Subcommands are discovered from the help text itself, so a newly added verb
# is picked up without editing this script.
capture_all() {
  local bin="$1" dest="$2"
  local -a queue=("")
  local path slug out

  while [ ${#queue[@]} -gt 0 ]; do
    path="${queue[0]}"
    queue=("${queue[@]:1}")

    if [ -z "$path" ]; then slug="root"; else slug="$(printf '%s' "$path" | tr ' ' '-')"; fi
    # shellcheck disable=SC2086
    out="$("$bin" $path --help 2>&1)" || die "'gaze $path --help' failed"
    printf '%s\n' "$out" > "${dest}/${slug}.txt"

    # Children are the first token of each indented line in the Commands: block.
    while IFS= read -r child; do
      [ -n "$child" ] || continue
      case "$child" in help) continue ;; esac
      if [ -z "$path" ]; then queue+=("$child"); else queue+=("$path $child"); fi
    done < <(printf '%s\n' "$out" | awk '
      /^Commands:/ { inblock = 1; next }
      inblock && /^[A-Za-z]+:/ { inblock = 0 }
      inblock && /^  [a-zA-Z_]/ { print $1 }
    ')
  done

  for path in "${HIDDEN_PATHS[@]}"; do
    slug="$(printf '%s' "$path" | tr ' ' '-')"
    # shellcheck disable=SC2086
    out="$("$bin" $path --help 2>&1)" || die "'gaze $path --help' (hidden) failed"
    printf '%s\n' "$out" > "${dest}/${slug}.txt"
  done
}

build_and_capture() {
  local tree="$1" dest="$2" label="$3" target_dir="$4" log bin
  log="${WORK_DIR}/build-${label}.log"
  printf 'cli-help-surface: building %s (%s)\n' "$label" "$tree" >&2
  # NOTE: exit status is captured on the very next line; never pipe cargo into
  # tail here, because the pipeline status would be tail's, not cargo's.
  ( cd "$tree" && CARGO_TARGET_DIR="$target_dir" cargo build -p gaze-cli ${FEATURES} --bin gaze ) >"$log" 2>&1
  local status=$?
  if [ "$status" -ne 0 ]; then
    tail -30 "$log" >&2
    die "build failed for ${label} (see ${log})"
  fi
  bin="${target_dir}/debug/gaze"
  [ -x "$bin" ] || die "no binary at ${bin}"
  capture_all "$bin" "$dest"
}

git worktree add --detach "$BASE_TREE" "$BASE_SHA" >/dev/null 2>&1 \
  || die "cannot create a worktree at ${BASE_SHA}"

build_and_capture "$BASE_TREE" "${WORK_DIR}/before" "before-${BASE_SHA:0:12}" "${WORK_DIR}/target-before"
build_and_capture "$REPO_ROOT" "${WORK_DIR}/after" "after-working-tree" "${WORK_DIR}/target-after"

printf '\ncli-help-surface: base %s\n' "$BASE_SHA"
printf 'cli-help-surface: captures in %s\n' "$WORK_DIR"

if diff -ru "${WORK_DIR}/before" "${WORK_DIR}/after" > "${WORK_DIR}/help.diff" 2>&1; then
  printf 'cli-help-surface: PASS — %s help captures byte-identical before and after\n' \
    "$(find "${WORK_DIR}/after" -name '*.txt' | wc -l | tr -d ' ')"
  result=0
else
  printf 'cli-help-surface: FAIL — the CLI help surface changed:\n\n'
  cat "${WORK_DIR}/help.diff"
  result=1
fi

if [ "$WRITE_FIXTURES" -eq 1 ] && [ "$result" -eq 0 ]; then
  mkdir -p "$FIXTURE_DIR"
  rm -f "${FIXTURE_DIR}"/*.txt
  cp "${WORK_DIR}"/after/*.txt "$FIXTURE_DIR"/
  printf 'cli-help-surface: refreshed fixtures in %s\n' "$FIXTURE_DIR"
elif [ "$WRITE_FIXTURES" -eq 1 ]; then
  printf 'cli-help-surface: fixtures NOT refreshed; the surface diff above must be resolved first\n'
fi

exit "$result"
