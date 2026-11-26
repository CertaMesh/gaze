#!/usr/bin/env bash
# Write the third-party licence file shipped next to each release binary.
#
# Usage: scripts/release/third-party-licenses.sh <output> [features]
#
# Lists every Rust crate linked into `gaze` with its licence text (via
# cargo-about and about.toml), then appends the notices of C/C++ and ported
# code vendored inside crates, which cargo-about does not collect, and ONNX
# Runtime's licence and third-party notices. ONNX Runtime is linked statically from the archive that
# ort-sys downloads; its notices are vendored under scripts/release/notices
# because the upstream repository is no longer reachable on GitHub. They were
# taken from the onnxruntime 1.24.2 PyPI wheel, which ships both files.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <output> [features]" >&2
  exit 2
fi
output="$1"
# Keep in step with GAZE_RELEASE_FEATURES in .github/workflows/release-build.yml.
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
cargo about generate --locked --fail -m crates/gaze-cli/Cargo.toml \
  --features "$features" scripts/release/third-party-licenses.hbs -o "$output"

# Notices for code vendored inside a crate, read from the locked crate source.
# Each line: crate, file inside the crate, and `file` (whole file) or `header`
# (the leading /* ... */ comment). A listed file that is missing fails the run.
metadata="$(mktemp)"
trap 'rm -f "$metadata"' EXIT
cargo metadata --locked --format-version 1 > "$metadata"
python3 -I - "$metadata" "$output" <<'PYTHON'
import json
import sys
from pathlib import Path

VENDORED = [
    ("onig_sys", "oniguruma/COPYING", "file", "Oniguruma (compiled by onig_sys)"),
    ("esaxx-rs", "src/esa.hxx", "header", "esaxx (compiled by esaxx-rs)"),
    ("esaxx-rs", "src/sais.hxx", "header", "sais-lite (compiled by esaxx-rs)"),
    ("crossbeam-channel", "LICENSE-THIRD-PARTY", "file", "Code ported into crossbeam-channel"),
    ("atomic-waker", "LICENSE-THIRD-PARTY", "file", "Code ported into atomic-waker"),
]

packages = {}
for package in json.load(open(sys.argv[1]))["packages"]:
    packages.setdefault(package["name"], []).append(package)

with open(sys.argv[2], "a", encoding="utf-8") as out:
    for crate, relative, mode, title in VENDORED:
        found = packages.get(crate)
        if not found or len(found) != 1:
            sys.exit(f"third-party-licenses: expected exactly one locked {crate}, found {len(found or [])}")
        package = found[0]
        path = Path(package["manifest_path"]).parent / relative
        if not path.is_file():
            sys.exit(f"third-party-licenses: {crate} {package['version']} has no {relative}")
        text = path.read_text(encoding="utf-8")
        if mode == "header":
            end = text.find("*/")
            if not text.startswith("/*") or end < 0:
                sys.exit(f"third-party-licenses: {crate} {relative} has no leading licence comment")
            text = text[: end + 2]
        out.write("\n" + "=" * 80 + "\n")
        out.write(f"{title}: {crate} {package['version']}, {relative}\n")
        out.write("=" * 80 + "\n\n" + text.rstrip("\n") + "\n")
PYTHON

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
