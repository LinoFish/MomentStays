#!/bin/bash
# Xcode build phase. Keep Cargo output in DerivedData, away from CLI builds.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../../.." && pwd)"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
cargo_bin="${CARGO:-cargo}"
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
  echo 'error: Cargo is required. Install Rust with rustup, then reopen Xcode.' >&2
  exit 1
fi
: "${DERIVED_FILE_DIR:?This script must run as an Xcode build phase}"
: "${ARCHS:?Xcode did not provide a target architecture}"
export CARGO_TARGET_DIR="$DERIVED_FILE_DIR/rust-target"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

profile=debug
cargo_flags=(--locked --lib)
if [[ "${CONFIGURATION:-Debug}" == Release ]]; then
  profile=release
  cargo_flags+=(--release)
fi
libs=()
for arch in $ARCHS; do
  case "$arch" in
    x86_64) target=x86_64-apple-darwin ;;
    arm64) target=aarch64-apple-darwin ;;
    *) echo "error: Unsupported macOS Rust architecture: $arch" >&2; exit 1 ;;
  esac
  # Xcode exports SDK/target settings for Swift. Set the matching Cargo target
  # explicitly so a host-architecture archive cannot enter another slice.
  if ! "$cargo_bin" build --manifest-path "$repo_root/core/Cargo.toml" \
      "${cargo_flags[@]}" --target "$target"; then
    echo "error: Rust build failed. Ensure rustup target add $target has completed." >&2
    exit 1
  fi
  libs+=("$CARGO_TARGET_DIR/$target/$profile/libmomentstays_core.a")
done
if [[ ${#libs[@]} -eq 0 ]]; then
  echo 'error: No macOS Rust architecture was selected.' >&2
  exit 1
fi
archive="$DERIVED_FILE_DIR/libmomentstays_core.a"
if [[ ${#libs[@]} -eq 1 ]]; then
  cp "${libs[0]}" "$archive"
else
  xcrun lipo -create "${libs[@]}" -output "$archive"
fi
