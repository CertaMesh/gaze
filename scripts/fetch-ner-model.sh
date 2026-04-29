#!/usr/bin/env bash
# Fetch and verify the Gaze default NER model artifact.
#
# Pulls the pre-quantized int8 ONNX mirror for
# Davlan/bert-base-multilingual-cased-ner-hrl (mBERT, high-resource languages
# incl. German + English) at a pinned Hugging Face commit SHA. No runtime
# network and no local ONNX export happen in the gaze binary; the binary only
# consumes the pinned local artifacts produced by this script.
#
# SHA256SUMS is checked in at the repository root so adopters can copy or curl
# it from a known-stable Gaze revision and fail closed on byte drift.
#
# Usage:
#   scripts/fetch-ner-model.sh [dest_dir]
#
# Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl

set -euo pipefail

# ---- Pinned artifact contract -----------------------------------------------

HF_REPO="onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX"
HF_COMMIT_SHA="cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8"

# Files that must end up in the destination directory.
REQUIRED_FILES=(
  "model.onnx"
  "tokenizer.json"
  "config.json"
  "tokenizer_config.json"
  "special_tokens_map.json"
  "vocab.txt"
  "labels.json"
  "SHA256SUMS"
)

# ---- Destination ------------------------------------------------------------

DEFAULT_DEST="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl"
DEST="${1:-$DEFAULT_DEST}"

log() { printf '[fetch-ner-model] %s\n' "$*"; }

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

require_cmd curl

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ROOT_SHA256SUMS="${REPO_ROOT}/SHA256SUMS"
LABELS_SOURCE="${REPO_ROOT}/assets/ner/labels.davlan-mbert.json"

if [ ! -f "$ROOT_SHA256SUMS" ]; then
  log "missing repository checksum manifest: ${ROOT_SHA256SUMS}"
  exit 2
fi
if [ ! -f "$LABELS_SOURCE" ]; then
  log "missing Gaze NER labels contract: ${LABELS_SOURCE}"
  exit 2
fi

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

log "installing labels.json from assets/ner/labels.davlan-mbert.json"
cp "$LABELS_SOURCE" labels.json

log "installing SHA256SUMS from repository root"
cp "$ROOT_SHA256SUMS" SHA256SUMS

# ---- Verify SHA256SUMS ------------------------------------------------------

for f in "${REQUIRED_FILES[@]}"; do
  if [ ! -f "$f" ]; then
    log "required artifact missing: $f"
    exit 4
  fi
done

log "verifying SHA256SUMS"
verify_sha256sums

log "done. model dir: $DEST"
log "next: set [ner] model_dir = \"$DEST\" in policy.toml (or rely on the default XDG path)"
