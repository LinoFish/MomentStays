#!/bin/bash
# Production Rust engine and direct Apple codec backend.
rust_arch="${1:-x86_64}"
case "$rust_arch" in
  x86_64) motionlive_rust_target=x86_64-apple-darwin ;;
  arm64) motionlive_rust_target=aarch64-apple-darwin ;;
  *) echo "Unsupported Rust architecture: $rust_arch" >&2; exit 2 ;;
esac
motionlive_rust_env=(MACOSX_DEPLOYMENT_TARGET=12.0)
if [[ "$rust_arch" == arm64 ]]; then
  source apple/macos/scripts/prepare_universal_rust.sh
  motionlive_rust_env+=("RUSTC=$motionlive_rustc")
fi
env "${motionlive_rust_env[@]}" cargo build --offline --locked --release \
  --manifest-path core/Cargo.toml --target "$motionlive_rust_target" --lib
rust_bridge_flags=(-import-objc-header apple/shared/RustCore.h
  -Xlinker "core/target/$motionlive_rust_target/release/libmomentstays_core.a"
  -framework Security -framework SystemConfiguration -lresolv
  -framework CoreFoundation -framework CoreGraphics -framework ImageIO
  -framework CoreMedia -framework AVFoundation -lobjc)
