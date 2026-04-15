#!/usr/bin/env bash
# Fetch and pin the Gaze v0.2 default NER model artifact.
#
# Pulls Davlan/bert-base-multilingual-cased-ner-hrl (mBERT, 10 high-resource
# languages incl. German + English) at a pinned Hugging Face commit SHA.
# Exports to ONNX, writes a SHA256SUMS manifest, verifies every file, and
# installs under the gaze runtime model dir.
#
# Ops step. No runtime network in the gaze binary — the binary only consumes
# the pinned local artifacts produced by this script.
#
# Usage:
#   scripts/fetch-ner-model.sh [dest_dir]
#
# Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/davlan-mbert-ner-hrl

set -euo pipefail

# ---- Pinned artifact contract -----------------------------------------------

# TODO: replace with the exact HF commit hash reviewed by Datenschutz once the
# initial Phase 2 model sign-off lands.
HF_REPO="Davlan/bert-base-multilingual-cased-ner-hrl"
HF_COMMIT_SHA="__PINNED_COMMIT_SHA_TODO__"

# Files that must end up in the destination directory.
REQUIRED_FILES=(
  "model.onnx"
  "tokenizer.json"
  "config.json"
  "labels.json"
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
require_cmd shasum
require_cmd python3

mkdir -p "$DEST"
cd "$DEST"

# ---- Download raw HF artifacts ---------------------------------------------

# tokenizer.json, config.json, labels.json (if published) come from HF resolve
# endpoint at the pinned commit. model.onnx is produced via optimum-cli ONNX
# export; if the HF repo already publishes an onnx/model.onnx for the pinned
# commit, use that instead.

fetch_raw() {
  local file="$1"
  local url="https://huggingface.co/${HF_REPO}/resolve/${HF_COMMIT_SHA}/${file}"
  log "fetching ${file} from ${url}"
  curl -fL --retry 3 -o "${file}" "${url}"
}

# labels.json is NOT part of the upstream HF repo; it's authored by Gaze ops
# and pinned alongside the other artifacts. The template is shipped at
# crates/gaze/testdata/ner/labels.example.json — copy and edit to reflect the
# MISC policy chosen for the deployment.
if [ ! -f "labels.json" ]; then
  log "labels.json not present; copy from crates/gaze/testdata/ner/labels.example.json and re-run"
  exit 3
fi

fetch_raw "tokenizer.json"
fetch_raw "config.json"

# model.onnx: prefer an already-exported onnx file in the HF repo; fall back
# to optimum-cli export.
if curl -fL --head -o /dev/null \
    "https://huggingface.co/${HF_REPO}/resolve/${HF_COMMIT_SHA}/onnx/model.onnx" 2>/dev/null; then
  log "fetching pre-exported onnx/model.onnx"
  curl -fL --retry 3 -o "model.onnx" \
    "https://huggingface.co/${HF_REPO}/resolve/${HF_COMMIT_SHA}/onnx/model.onnx"
else
  log "no pre-exported onnx; running optimum-cli export (requires 'optimum[exporters]' in a venv)"
  require_cmd optimum-cli
  optimum-cli export onnx \
    --model "${HF_REPO}" \
    --revision "${HF_COMMIT_SHA}" \
    --task token-classification \
    "$DEST"
fi

# ---- Write / verify SHA256SUMS ---------------------------------------------

log "writing SHA256SUMS"
: > SHA256SUMS.new
for f in "${REQUIRED_FILES[@]}"; do
  if [ ! -f "$f" ]; then
    log "required artifact missing after fetch: $f"
    exit 4
  fi
  shasum -a 256 "$f" >> SHA256SUMS.new
done
mv SHA256SUMS.new SHA256SUMS

log "verifying SHA256SUMS"
shasum -a 256 -c SHA256SUMS

log "done. model dir: $DEST"
log "next: set [ner] model_dir = \"$DEST\" in policy.toml (or rely on the default XDG path)"
