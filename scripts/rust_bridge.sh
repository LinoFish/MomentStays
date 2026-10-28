#!/bin/bash
# Sourced by Swift build entry points from the repository root.
# Reference mode exists only for differential validation of the retained Swift parser.
rust_bridge_flags=()
if [[ "${MOTIONLIVE_SWIFT_REFERENCE:-0}" == 1 ]]; then
  rust_bridge_flags=(-D MOTIONLIVE_SWIFT_REFERENCE)
else
  rust_arch="${1:-x86_64}"
  case "$rust_arch" in
    x86_64) motionlive_rust_target=x86_64-apple-darwin ;;
    arm64) motionlive_rust_target=aarch64-apple-darwin ;;
    *) echo "Unsupported Rust architecture: $rust_arch" >&2; exit 2 ;;
  esac
  motionlive_rust_env=(MACOSX_DEPLOYMENT_TARGET=12.0)
  if [[ "$rust_arch" == arm64 ]]; then
    source scripts/prepare_universal_rust.sh
    motionlive_rust_env+=("RUSTC=$motionlive_rustc")
  fi
  env "${motionlive_rust_env[@]}" cargo build --offline --locked --release \
    --manifest-path core/Cargo.toml --target "$motionlive_rust_target" --lib
  rust_bridge_flags=(-import-objc-header native/RustCore.h
    -Xlinker "core/target/$motionlive_rust_target/release/libmomentstays_core.a"
    -framework Security -framework SystemConfiguration -lresolv)
fi
