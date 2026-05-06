#!/usr/bin/env bash
# Fetch and verify the Gaze default NER model artifact.
#
# Pulls the pre-quantized int8 ONNX mirror for
# Davlan/bert-base-multilingual-cased-ner-hrl (mBERT, high-resource languages
# incl. German + English) at a pinned Hugging Face commit SHA. No runtime
# network and no local ONNX export happen in the gaze binary; the binary only
# consumes the pinned local artifacts produced by this script.
#
# Checksums are embedded below, pinned to HF_COMMIT_SHA. Update them whenever
# the model pin changes.
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
LABELS_SOURCE="${REPO_ROOT}/assets/ner/labels.davlan-mbert.json"

if [ ! -f "$LABELS_SOURCE" ]; then
  log "missing Gaze NER labels contract: ${LABELS_SOURCE}"
  exit 2
fi

write_sha256sums() {
  # Pinned to HF_COMMIT_SHA=cfe67b1c1c4c91c1b26ac192955fc0971e62d8c8
  # Update whenever the model pin changes.
  cat > SHA256SUMS <<'SHASUMS'
1213fdd405d295768b0d41d8214062f2f278f0e3acff6af67d8fd47360d2be0f  model.onnx
bf1b59b7b11c95f194f51708d918eea378e09d05f84c0e1656dc5180e8117088  tokenizer.json
470cff6e0353b08e2a6e9b4f61729ecdc47ccb3ced335fa5520e9ce334572d59  tokenizer_config.json
8e5caefadaf9923a9e7d3de42ca97780c68fc4d83519d333f141b299e40af638  config.json
b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3  special_tokens_map.json
fe0fda7c425b48c516fc8f160d594c8022a0808447475c1a7c6d6479763f310c  vocab.txt
8498e2bafc017a793571c3c2f7092390a93a757f5ca45004f21db2560a8c6fdb  labels.json
SHASUMS
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

# ---- Verify checksums -------------------------------------------------------

for f in "${REQUIRED_FILES[@]}"; do
  if [ ! -f "$f" ]; then
    log "required artifact missing: $f"
    exit 4
  fi
done

log "writing and verifying checksums"
write_sha256sums
verify_sha256sums

log "done. model dir: $DEST"
log "next: set [ner] model_dir = \"$DEST\" in policy.toml (or rely on the default XDG path)"
