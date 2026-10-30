#!/bin/bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v cargo >/dev/null; then
  echo 'error: Rust/Cargo is required to build the native MomentStays core. Install rustup and the iOS Rust targets.' >&2
  exit 1
fi
export CARGO_TARGET_DIR="${DERIVED_FILE_DIR}/rust-target"
libs=()
for arch in $ARCHS; do
  case "${PLATFORM_NAME}:$arch" in
    iphoneos:arm64) target=aarch64-apple-ios ;;
    iphonesimulator:arm64) target=aarch64-apple-ios-sim ;;
    iphonesimulator:x86_64) target=x86_64-apple-ios ;;
    *) echo "error: Unsupported Rust iOS target ${PLATFORM_NAME}:$arch" >&2; exit 1 ;;
  esac
  cargo build --manifest-path "${SRCROOT}/../core/Cargo.toml" --locked --release --lib --target "$target"
  libs+=("$CARGO_TARGET_DIR/$target/release/libmomentstays_core.a")
done
if [ "${#libs[@]}" -eq 1 ]; then cp "${libs[0]}" "${DERIVED_FILE_DIR}/libmomentstays_core.a"; else xcrun lipo -create "${libs[@]}" -output "${DERIVED_FILE_DIR}/libmomentstays_core.a"; fi
