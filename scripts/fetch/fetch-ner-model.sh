#!/usr/bin/env bash
# Fetch and verify the Gaze default NER model artifact.
#
# Pulls the pre-quantized int8 ONNX mirror for
# Davlan/bert-base-multilingual-cased-ner-hrl (mBERT, high-resource languages
# incl. German + English) at a pinned Hugging Face commit SHA. No runtime
# network and no local ONNX export happen in the gaze binary; the binary only
# consumes the pinned local artifacts produced by this script.
#
# Checksums are published as SHA256SUMS.ner in each Gaze GitHub release.
#
# Usage:
#   scripts/fetch/fetch-ner-model.sh [--gaze-version <tag>] [dest_dir]
#
# Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl

set -euo pipefail

# ---- Pinned artifact contract -----------------------------------------------

HF_REPO="onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX"
HF_COMMIT_SHA="cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8"
GITHUB_REPO="${GAZE_GITHUB_REPO:-CertaMesh/gaze}"

# Files that must end up in the destination directory.
REQUIRED_FILES=(
  "model.onnx"
  "tokenizer.json"
  "config.json"
  "tokenizer_config.json"
  "special_tokens_map.json"
  "vocab.txt"
  "labels.json"
)

# ---- Destination ------------------------------------------------------------

DEFAULT_DEST="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl"
DEST=""
GAZE_VERSION=""

log() { printf '[fetch-ner-model] %s\n' "$*"; }

usage() {
  cat <<'USAGE'
Usage:
  scripts/fetch/fetch-ner-model.sh [--gaze-version <tag>] [dest_dir]

Options:
  --gaze-version <tag>  Gaze GitHub release tag that provides SHA256SUMS.ner.
  -h, --help            Show this help.

Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --gaze-version)
      if [ "$#" -lt 2 ]; then
        log "missing value for --gaze-version"
        exit 2
      fi
      GAZE_VERSION="$2"
      shift 2
      ;;
    --gaze-version=*)
      GAZE_VERSION="${1#--gaze-version=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      log "unknown option: $1"
      usage
      exit 2
      ;;
    *)
      if [ -n "$DEST" ]; then
        log "unexpected extra argument: $1"
        usage
        exit 2
      fi
      DEST="$1"
      shift
      ;;
  esac
done

if [ "$#" -gt 0 ]; then
  if [ "$#" -gt 1 ]; then
    log "unexpected extra argument: $2"
    usage
    exit 2
  fi
  if [ -n "$DEST" ]; then
    log "unexpected extra argument: $1"
    usage
    exit 2
  fi
  DEST="$1"
fi

DEST="${DEST:-$DEFAULT_DEST}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

require_cmd curl

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
LABELS_SOURCE="${REPO_ROOT}/crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json"

if [ ! -f "$LABELS_SOURCE" ]; then
  log "missing Gaze NER labels contract: ${LABELS_SOURCE}"
  exit 2
fi

detect_gaze_version_from_git() {
  if command -v git >/dev/null 2>&1; then
    git -C "$REPO_ROOT" describe --tags --abbrev=0 --exclude '*-*' 2>/dev/null || true
  fi
}

detect_latest_gaze_release() {
  local api_url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
  curl -fsSL "$api_url" 2>/dev/null \
    | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n 1 \
    || true
}

resolve_gaze_version() {
  local version="$GAZE_VERSION"

  if [ -z "$version" ]; then
    version="$(detect_gaze_version_from_git)"
  fi

  if [ -z "$version" ]; then
    version="$(detect_latest_gaze_release)"
  fi

  if [ -z "$version" ]; then
    log "could not determine Gaze release version for SHA256SUMS.ner"
    log "specify one explicitly: scripts/fetch/fetch-ner-model.sh --gaze-version <tag> [dest_dir]"
    exit 2
  fi

  printf '%s\n' "$version"
}

fetch_sha256sums() {
  local version="$1"
  local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/SHA256SUMS.ner"
  log "fetching release checksums ${version} -> SHA256SUMS"
  curl -fL --retry 3 -o SHA256SUMS "$url"
}

verify_sha256sums() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c SHA256SUMS
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c SHA256SUMS
  else
    log "missing required command: shasum or sha256sum"
    exit 2
  fi
}

mkdir -p "$DEST"
cd "$DEST"
GAZE_VERSION="$(resolve_gaze_version)"

# ---- Download pinned HF artifacts ------------------------------------------

# The source int8 model lives at onnx/model_int8.onnx in the mirror. It is
# installed as model.onnx because the runtime load contract expects that file
# name in model_dir.

fetch_raw() {
  local source_file="$1"
  local dest_file="$2"
  local url="https://huggingface.co/${HF_REPO}/resolve/${HF_COMMIT_SHA}/${source_file}"
  log "fetching ${source_file} -> ${dest_file}"
  curl -fL --retry 3 -o "${dest_file}" "${url}"
}

fetch_raw "onnx/model_int8.onnx" "model.onnx"
fetch_raw "tokenizer.json" "tokenizer.json"
fetch_raw "tokenizer_config.json" "tokenizer_config.json"
fetch_raw "config.json" "config.json"
fetch_raw "special_tokens_map.json" "special_tokens_map.json"
fetch_raw "vocab.txt" "vocab.txt"

log "installing labels.json from crates/gaze-recognizers/assets/ner/labels.davlan-mbert.json"
cp "$LABELS_SOURCE" labels.json

# ---- Fetch release checksum contract ----------------------------------------

fetch_sha256sums "$GAZE_VERSION"

# ---- Verify checksums -------------------------------------------------------

for f in "${REQUIRED_FILES[@]}"; do
  if [ ! -f "$f" ]; then
    log "required artifact missing: $f"
    exit 4
  fi
done

log "verifying checksums"
verify_sha256sums

log "done. model dir: $DEST"
log "next: set [ner] model_dir = \"$DEST\" in policy.toml (or rely on the default XDG path)"
