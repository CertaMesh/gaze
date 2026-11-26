#!/usr/bin/env bash
# Build the release `gaze` binary reproducibly.
#
# Usage: scripts/release/build-binary.sh <target> <output> [cargo build args...]
#
# Example (release features):
#   scripts/release/build-binary.sh aarch64-apple-darwin dist/gaze --features document,proxy
#
# The release workflow and anyone verifying a published digest run this same
# script. Two builds of the same commit, target, features and toolchain produce
# the same bytes, regardless of checkout path, CARGO_HOME or rustup home.
# Writes <output>, <output>.sha256 and <output>.buildinfo.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <target> <output> [cargo build args...]" >&2
  exit 2
fi
target="$1"
output="$2"
shift 2

root="$(cd "$(dirname "$0")/../.." && pwd -P)"
cd "$root"

# Refuse any compiler other than the rustup toolchain pinned in
# rust-toolchain.toml. A rustc of the same version from another distribution
# (for example Homebrew) ignores the pin and emits different code.
channel="$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)"
if ! command -v rustup >/dev/null 2>&1; then
  echo "build-binary: rustup is required to select the pinned toolchain $channel" >&2
  exit 1
fi
sysroot="$(rustc --print sysroot)"
expected_sysroot="$(rustup run "$channel" rustc --print sysroot)"
if [ "$sysroot" != "$expected_sysroot" ]; then
  echo "build-binary: rustc resolves to $sysroot, not the pinned rustup toolchain $channel ($expected_sysroot)" >&2
  exit 1
fi
release="$(rustc -vV | sed -n 's/^release: //p')"
if [ "$release" != "$channel" ]; then
  echo "build-binary: rustc reports $release; rust-toolchain.toml pins $channel" >&2
  exit 1
fi

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rustflags=(
  "--remap-path-prefix=$root=/gaze"
  "--remap-path-prefix=$cargo_home=/cargo"
  "--remap-path-prefix=$sysroot=/rustc"
)
case "$target" in
  *-apple-darwin)
    # ld64 derives LC_UUID from its output before stripping, which includes
    # debug-map entries naming object files under the build directory. Omitting
    # the debug map makes the UUID, and so the code signature, path-independent.
    rustflags+=("-Clink-arg=-Wl,-S")
    ;;
esac
# CARGO_ENCODED_RUSTFLAGS takes precedence over RUSTFLAGS; set it so an
# inherited RUSTFLAGS cannot change the build.
CARGO_ENCODED_RUSTFLAGS="$(IFS=$'\x1f'; printf '%s' "${rustflags[*]}")"
export CARGO_ENCODED_RUSTFLAGS
export CARGO_INCREMENTAL=0
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"
unset RUSTFLAGS RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER

cargo build --locked --release -p gaze-cli --bin gaze --target "$target" "$@"

mkdir -p "$(dirname "$output")"
cp "target/$target/release/gaze" "$output"
chmod +x "$output"
name="$(basename "$output")"
(cd "$(dirname "$output")" && shasum -a 256 "$name" > "$name.sha256")

{
  echo "commit: $(git rev-parse HEAD)"
  if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "dirty: yes (tracked files differ from the commit)"
  fi
  echo "target: $target"
  echo "cargo-args: $*"
  echo "sha256: $(cut -d' ' -f1 < "$output.sha256")"
  echo "rustc:"
  rustc -vV | sed 's/^/  /'
  echo "cargo: $(cargo -V)"
  case "$target" in
    *-apple-darwin)
      echo "linker: $(ld -v 2>&1 | head -1)"
      echo "xcode: $(xcodebuild -version 2>/dev/null | tr '\n' ' ')"
      ;;
    *-linux-gnu)
      echo "linker: $(ld -v 2>&1 | head -1)"
      echo "cc: $(cc --version 2>&1 | head -1)"
      echo "glibc: $(ldd --version 2>&1 | head -1)"
      ;;
  esac
} > "$output.buildinfo"

cat "$output.sha256"
