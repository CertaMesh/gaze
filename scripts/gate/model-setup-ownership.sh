#!/usr/bin/env bash
# Release gate: real pinned NER bundle artifacts must be trusted by process owner, not cwd owner.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
receipt_dir="${1:?usage: model-setup-ownership.sh <receipt-directory>}"
mkdir -p "$receipt_dir"
receipt_dir="$(cd "$receipt_dir" && pwd)"
exec > >(tee "$receipt_dir/gate.log") 2>&1

[[ "$(uname -s)" == Linux ]] || { echo 'Requires Linux with passwordless sudo'; exit 1; }
[[ "$(id -u)" != 0 ]] || { echo 'Run as an unprivileged user, not root'; exit 1; }
sudo -n true
fixture_root="$(mktemp -d)"
cleanup() {
  local result=$?
  trap - EXIT
  sudo -n rm -rf -- "$fixture_root"
  echo "OWNERSHIP_GATE_EXIT=$result"
  exit "$result"
}
trap cleanup EXIT

set -x
git rev-parse HEAD
rustc --version --verbose
cargo --version
id
export GAZE_MODEL_SETUP_OWNED_MODEL_DIR="$fixture_root/owned"
export GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR="$fixture_root/foreign"
export GAZE_MODEL_SETUP_FOREIGN_CWD="$fixture_root/foreign-cwd"
export GAZE_MODEL_SETUP_LOOSE_MODEL_DIR="$fixture_root/loose"
bundle_files=(SHA256SUMS model.onnx tokenizer.json tokenizer_config.json config.json special_tokens_map.json vocab.txt labels.json)

# The shipped installer downloads the source-pinned bundle and performs its real verification.
cargo run --locked -p gaze-cli --features setup -- setup --non-interactive \
  --safety-net ner --model-dir "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR" \
  --policy-out "$fixture_root/policy.toml"
(
  cd "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR"
  sha256sum -c SHA256SUMS
  sha256sum "${bundle_files[@]}"
) | tee "$receipt_dir/artifact-hashes.txt"

cp -R "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR" "$GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR"
cp -R "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR" "$GAZE_MODEL_SETUP_LOOSE_MODEL_DIR"
# Setup must enumerate a non-empty foreign directory to return its ownership error.
# Keep the verifier's private foreign copy separate from this readable setup copy.
foreign_setup_dir="$fixture_root/foreign-setup"
cp -R "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR" "$foreign_setup_dir"
chmod 0755 "$foreign_setup_dir"
chmod 0644 "$foreign_setup_dir"/*
mkdir "$GAZE_MODEL_SETUP_FOREIGN_CWD"
chmod 0755 "$GAZE_MODEL_SETUP_FOREIGN_CWD" "$GAZE_MODEL_SETUP_LOOSE_MODEL_DIR"
chmod 0666 "$GAZE_MODEL_SETUP_LOOSE_MODEL_DIR"/*
sudo -n chown -R 0:0 "$GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR" "$GAZE_MODEL_SETUP_FOREIGN_CWD" "$foreign_setup_dir"
[[ "$(stat -c %u "$GAZE_MODEL_SETUP_OWNED_MODEL_DIR")" == "$(id -u)" ]]
[[ "$(stat -c %u "$GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR")" != "$(id -u)" ]]
[[ "$(stat -c %u "$GAZE_MODEL_SETUP_FOREIGN_CWD")" != "$(id -u)" ]]
# Prove rejection uses a valid foreign-owned copy, not a missing or corrupt model.
sudo -n sh -c 'cd "$1"; shift; sha256sum -c SHA256SUMS; sha256sum "$@"' \
  sh "$GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR" "${bundle_files[@]}" | tee "$receipt_dir/foreign-artifact-hashes.txt"
cmp "$receipt_dir/artifact-hashes.txt" "$receipt_dir/foreign-artifact-hashes.txt"
[[ "$(stat -c %u "$foreign_setup_dir")" != "$(id -u)" ]]
(
  cd "$foreign_setup_dir"
  sha256sum -c SHA256SUMS
  sha256sum "${bundle_files[@]}"
) | tee "$receipt_dir/foreign-setup-artifact-hashes.txt"
cmp "$receipt_dir/artifact-hashes.txt" "$receipt_dir/foreign-setup-artifact-hashes.txt"
sudo -n find "$fixture_root" -printf '%U %m %p\n' | tee "$receipt_dir/ownership.txt"

cross_directory_test=ner::pinned::tests::verify_davlan_ner_bundle_is_cwd_independent
cargo test --locked -p gaze-recognizers --lib \
  "$cross_directory_test" -- --ignored --exact --nocapture \
  | tee "$receipt_dir/cross-directory.log"
grep -F "test $cross_directory_test ... ok" "$receipt_dir/cross-directory.log"

for setup_test in loose_mode_current_user_dir_is_repaired_then_accepted foreign_owned_dir_fails_closed; do
  GAZE_MODEL_SETUP_FOREIGN_MODEL_DIR="$foreign_setup_dir" cargo test --locked -p gaze-model-setup --lib "tests::$setup_test" -- --ignored --exact --nocapture \
    | tee "$receipt_dir/$setup_test.log"
  grep -F "test tests::$setup_test ... ok" "$receipt_dir/$setup_test.log"
done
(
  cd "$GAZE_MODEL_SETUP_LOOSE_MODEL_DIR"
  sha256sum -c SHA256SUMS
  sha256sum "${bundle_files[@]}"
) | tee "$receipt_dir/repaired-artifact-hashes.txt"
cmp "$receipt_dir/artifact-hashes.txt" "$receipt_dir/repaired-artifact-hashes.txt"
find "$GAZE_MODEL_SETUP_LOOSE_MODEL_DIR" -printf '%U %m %p\n' | tee "$receipt_dir/repaired-ownership.txt"
set +x
echo 'PASS: live cross-directory euid and strict setup ownership/mode gates'
