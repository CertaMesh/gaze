#!/usr/bin/env bash
# Write the third-party licence file shipped next to each release binary.
#
# Usage: scripts/release/third-party-licenses.sh <output> [features]
#
# Lists every Rust crate linked into `gaze` with its licence text (via
# cargo-about and about.toml), then appends ONNX Runtime's licence and
# third-party notices. ONNX Runtime is linked statically from the archive that
# ort-sys downloads; its notices are vendored under scripts/release/notices
# because the upstream repository is no longer reachable on GitHub. They were
# taken from the onnxruntime 1.24.2 PyPI wheel, which ships both files.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <output> [features]" >&2
  exit 2
fi
output="$1"
features="${2:-document proxy}"

root="$(cd "$(dirname "$0")/../.." && pwd -P)"
cd "$root"

# The vendored notices describe one ONNX Runtime release. Refuse to ship them
# for another: an ort-sys bump must update the notices in the same change.
expected_ort_sys="2.0.0-rc.12"
ort_version="1.24.2"
ort_sys="$(awk '/^name = "ort-sys"$/ { getline; sub(/^version = "/, ""); sub(/"$/, ""); print }' Cargo.lock)"
if [ "$ort_sys" != "$expected_ort_sys" ]; then
  echo "third-party-licenses: Cargo.lock has ort-sys $ort_sys; vendored ONNX Runtime notices are for ort-sys $expected_ort_sys (ONNX Runtime $ort_version)" >&2
  exit 1
fi

mkdir -p "$(dirname "$output")"
cargo about generate --locked -m crates/gaze-cli/Cargo.toml \
  --features "$features" scripts/release/third-party-licenses.hbs -o "$output"

for name in LICENSE ThirdPartyNotices.txt; do
  {
    echo
    echo "================================================================================"
    echo "ONNX Runtime $ort_version: $name"
    echo "================================================================================"
    echo
    cat "scripts/release/notices/onnxruntime-$ort_version-$name"
  } >> "$output"
done
