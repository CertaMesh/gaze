#!/usr/bin/env bash
# Fetch and verify the pinned Kiji DistilBERT safety-net artifact.
#
# Mirrors `scripts/fetch-ner-model.sh` exactly in shape: pulls a DistilBERT
# NER model at a pinned Hugging Face commit SHA, drops it under the same
# XDG-style cache path, and verifies via shasum against the release-pinned
# SHA256SUMS.kiji checksum file. No runtime network and no local ONNX export
# happens in the gaze binary; the binary only consumes the pinned local
# artifacts produced by this script.
#
# Checksums are published as SHA256SUMS.kiji in each Gaze GitHub release once
# the first sign-off run lands real hashes.
#
# Usage:
#   scripts/fetch-kiji-safetynet-model.sh [--gaze-version <tag>] [dest_dir]
#
# Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert

set -euo pipefail

# ---- Pinned artifact contract -----------------------------------------------

HF_REPO="onnx-community/distilbert-NER-ONNX"
HF_COMMIT_SHA="3a19fe9404a4469d91aa3d551558a97f68872f67"
KIJI_BUNDLE_SHA256="c129e135d86698e67c4836456212666f94a56ceaf995acd60532f557b3120d2f"
GITHUB_REPO="${GAZE_GITHUB_REPO:-EmpireTwo/gaze}"

# Files that must end up in the destination directory.
REQUIRED_FILES=(
  "model.onnx"
  "tokenizer.json"
  "labels.json"
  "SHA256SUMS"
)

# ---- Destination ------------------------------------------------------------

DEFAULT_DEST="${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert"
DEST=""
GAZE_VERSION=""

log() { printf '[fetch-kiji-safetynet-model] %s\n' "$*"; }

usage() {
  cat <<'USAGE'
Usage:
  scripts/fetch-kiji-safetynet-model.sh [--gaze-version <tag>] [dest_dir]

Options:
  --gaze-version <tag>  Gaze GitHub release tag that provides SHA256SUMS.kiji.
  -h, --help            Show this help.

Default dest_dir = ${XDG_DATA_HOME:-$HOME/.local/share}/gaze/models/kiji-distilbert
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

detect_gaze_version_from_git() {
  if command -v git >/dev/null 2>&1; then
    git -C "$(dirname "${BASH_SOURCE[0]}")/.." describe --tags --abbrev=0 --exclude '*-*' 2>/dev/null || true
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
    log "could not determine Gaze release version for SHA256SUMS.kiji"
    log "specify one explicitly: scripts/fetch-kiji-safetynet-model.sh --gaze-version <tag> [dest_dir]"
    exit 2
  fi

  printf '%s\n' "$version"
}

fetch_sha256sums() {
  local version="$1"
  local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/SHA256SUMS.kiji"
  log "fetching release checksums ${version} -> SHA256SUMS"
  curl -fL --retry 3 -o SHA256SUMS "$url"
}

verify_sha256sums() {
  local actual
  if command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 SHA256SUMS | awk '{print $1}')"
  elif command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum SHA256SUMS | awk '{print $1}')"
  else
    log "missing required command: shasum or sha256sum"
    exit 2
  fi
  if [ "$actual" != "$KIJI_BUNDLE_SHA256" ]; then
    log "SHA256SUMS integrity mismatch: expected $KIJI_BUNDLE_SHA256 got $actual"
    exit 4
  fi

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
chmod 0700 "$DEST" 2>/dev/null || true
cd "$DEST"
GAZE_VERSION="$(resolve_gaze_version)"

# ---- Download pinned HF artifacts ------------------------------------------

fetch_raw() {
  local source_file="$1"
  local dest_file="$2"
  local url="https://huggingface.co/${HF_REPO}/resolve/${HF_COMMIT_SHA}/${source_file}"
  log "fetching ${source_file} -> ${dest_file}"
  curl -fL --retry 3 -o "${dest_file}" "${url}"
  chmod 0600 "${dest_file}" 2>/dev/null || true
}

fetch_raw "onnx/model.onnx" "model.onnx"
fetch_raw "tokenizer.json" "tokenizer.json"
cat > labels.json <<EOF
{
  "schema_version": 1,
  "source": "${HF_REPO}",
  "source_commit": "${HF_COMMIT_SHA}",
  "labels": [
    {"id": "person", "upstream": ["B-PER", "I-PER"]},
    {"id": "location", "upstream": ["B-LOC", "I-LOC"]},
    {"id": "organization", "upstream": ["B-ORG", "I-ORG"]},
    {"id": "miscellaneous", "upstream": ["B-MISC", "I-MISC"]}
  ]
}
EOF
chmod 0600 labels.json 2>/dev/null || true

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
log "next: pass --safety-net-backend=kiji-distilbert --kiji-distilbert-model-dir=\"$DEST\" to gaze clean"
