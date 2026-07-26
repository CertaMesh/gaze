#!/usr/bin/env bash
# Fetch and install the pinned OpenAI Privacy Filter safety-net runtime.
#
# OPF currently publishes source plus a checkpoint downloaded by the `opf`
# command. It does not publish GitHub release binaries, so this script pins the
# source commit and installs the CLI from that content-addressed tree. The
# checkpoint bundle SHA256 matches the runtime pin in the Rust adapter. The
# downloader promotes the upstream `original/` subtree, locks permissions, and
# verifies the same ordered four-file manifest before executing the model.
#
# Usage:
#   scripts/fetch/fetch-openai-privacy-filter.sh [--install-dir <dir>] [--checkpoint-dir <dir>]
#
# Default install_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/openai-privacy-filter
# Default checkpoint_dir = ${OPF_CHECKPOINT:-$HOME/.opf/privacy_filter}

set -euo pipefail

# ---- Pinned artifact contract -----------------------------------------------

OPF_REPO="openai/privacy-filter"
OPF_COMMIT_SHA="f7f00ca7fb869683eb732c010299d901457f19c3"
OPF_CHECKPOINT_BUNDLE_SHA256="4680158333621f3f344f58366f59612d52eff67ce6f46cff7becede5be1853ae"
OPF_MODEL_REPO="openai/privacy-filter"
OPF_REQUIRED_ARTIFACTS="config.json dtypes.json model.safetensors viterbi_calibration.json"

# ---- Destination -------------------------------------------------------------

DEFAULT_INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/openai-privacy-filter"
DEFAULT_CHECKPOINT_DIR="${OPF_CHECKPOINT:-$HOME/.opf/privacy_filter}"
INSTALL_DIR=""
CHECKPOINT_DIR=""

log() { printf '[fetch-openai-privacy-filter] %s\n' "$*"; }

usage() {
  cat <<'USAGE'
Usage:
  scripts/fetch/fetch-openai-privacy-filter.sh [--install-dir <dir>] [--checkpoint-dir <dir>]

Options:
  --install-dir <dir>     Source checkout and virtualenv destination.
  --checkpoint-dir <dir>  OPF checkpoint destination passed as OPF_CHECKPOINT.
  -h, --help              Show this help.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install-dir)
      if [ "$#" -lt 2 ]; then
        log "missing value for --install-dir"
        exit 2
      fi
      INSTALL_DIR="$2"
      shift 2
      ;;
    --install-dir=*)
      INSTALL_DIR="${1#--install-dir=}"
      shift
      ;;
    --checkpoint-dir)
      if [ "$#" -lt 2 ]; then
        log "missing value for --checkpoint-dir"
        exit 2
      fi
      CHECKPOINT_DIR="$2"
      shift 2
      ;;
    --checkpoint-dir=*)
      CHECKPOINT_DIR="${1#--checkpoint-dir=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      log "unknown option: $1"
      usage
      exit 2
      ;;
    *)
      log "unexpected argument: $1"
      usage
      exit 2
      ;;
  esac
done

INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
CHECKPOINT_DIR="${CHECKPOINT_DIR:-$DEFAULT_CHECKPOINT_DIR}"
SOURCE_DIR="$INSTALL_DIR/src"
VENV_DIR="$INSTALL_DIR/venv"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

require_cmd git
require_cmd python3

mkdir -p "$INSTALL_DIR" "$CHECKPOINT_DIR"
chmod 0700 "$INSTALL_DIR" "$CHECKPOINT_DIR" 2>/dev/null || true

if [ ! -d "$SOURCE_DIR/.git" ]; then
  log "cloning ${OPF_REPO} into ${SOURCE_DIR}"
  git clone "https://github.com/${OPF_REPO}.git" "$SOURCE_DIR"
else
  log "updating existing source checkout"
  git -C "$SOURCE_DIR" fetch --tags origin
fi

git -C "$SOURCE_DIR" checkout --detach "$OPF_COMMIT_SHA"
ACTUAL_COMMIT="$(git -C "$SOURCE_DIR" rev-parse HEAD)"
if [ "$ACTUAL_COMMIT" != "$OPF_COMMIT_SHA" ]; then
  log "source commit mismatch: expected $OPF_COMMIT_SHA got $ACTUAL_COMMIT"
  exit 4
fi

if [ ! -x "$VENV_DIR/bin/python" ]; then
  log "creating virtualenv ${VENV_DIR}"
  python3 -m venv "$VENV_DIR"
fi

log "installing pinned OPF package"
"$VENV_DIR/bin/python" -m pip install --upgrade pip
"$VENV_DIR/bin/python" -m pip install -e "$SOURCE_DIR"

missing_checkpoint=0
for artifact in $OPF_REQUIRED_ARTIFACTS; do
  if [ ! -f "$CHECKPOINT_DIR/$artifact" ]; then
    missing_checkpoint=1
  fi
done
if [ "$missing_checkpoint" -eq 1 ]; then
  if [ -e "$CHECKPOINT_DIR/original" ]; then
    log "checkpoint is incomplete and original/ already exists; remove or repair ${CHECKPOINT_DIR}"
    exit 5
  fi
  log "downloading pinned-by-digest checkpoint from ${OPF_MODEL_REPO}"
  "$VENV_DIR/bin/hf" download "$OPF_MODEL_REPO" \
    --include "original/*" \
    --local-dir "$CHECKPOINT_DIR"
  for artifact in $OPF_REQUIRED_ARTIFACTS; do
    if [ ! -f "$CHECKPOINT_DIR/original/$artifact" ]; then
      log "downloaded checkpoint is missing original/${artifact}"
      exit 5
    fi
    mv "$CHECKPOINT_DIR/original/$artifact" "$CHECKPOINT_DIR/$artifact"
  done
  rmdir "$CHECKPOINT_DIR/original"
fi

chmod -R u+rwX,go-rwx "$CHECKPOINT_DIR"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

bundle_manifest=""
for artifact in $OPF_REQUIRED_ARTIFACTS; do
  artifact_sha="$(sha256_file "$CHECKPOINT_DIR/$artifact")"
  bundle_manifest="${bundle_manifest}${artifact_sha}  ${artifact}
"
done
actual_bundle_sha="$(printf '%s' "$bundle_manifest" | sha256_stdin)"
if [ "$actual_bundle_sha" != "$OPF_CHECKPOINT_BUNDLE_SHA256" ]; then
  log "checkpoint bundle SHA mismatch: expected ${OPF_CHECKPOINT_BUNDLE_SHA256} got ${actual_bundle_sha}"
  exit 5
fi

log "triggering checkpoint availability check"
printf '%s\n' 'No private data.' \
  | OPF_CHECKPOINT="$CHECKPOINT_DIR" "$VENV_DIR/bin/opf" \
      --format json --output-mode typed --device cpu >/dev/null

log "done. opf command: $VENV_DIR/bin/opf"
log "checkpoint dir: $CHECKPOINT_DIR"
log "checkpoint bundle SHA256: $OPF_CHECKPOINT_BUNDLE_SHA256"
