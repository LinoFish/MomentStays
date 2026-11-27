#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
work=$(mktemp -d "${TMPDIR:-/tmp}/momentstays-time-check.XXXXXX")
trap 'rm -rf "$work"' EXIT
# Filename capture-time tokens come from the shared Rust core (filename-time).
source apple/macos/scripts/rust_bridge.sh x86_64
xcrun swiftc "${rust_bridge_flags[@]}" -parse-as-library -target x86_64-apple-macosx12.0 -module-cache-path "$work/module-cache" \
  apple/shared/RustCore.swift apple/shared/MotionLive.swift apple/shared/PhotoFormat.swift apple/shared/BatchEngine.swift \
  apple/shared/CoverCandidates.swift apple/shared/AndroidExport.swift apple/shared/AndroidTransfer.swift \
  apple/macos/Sources/ConversionSettings.swift apple/macos/Sources/DeliveryModels.swift apple/macos/Sources/PhotoLibraryService.swift \
  apple/macos/Sources/PhotoTimeRepair.swift apple/macos/tests/PhotoTimeRepairTests.swift \
  -framework ImageIO -framework CoreGraphics -framework CryptoKit -o "$work/PhotoTimeRepairTests"
"$work/PhotoTimeRepairTests"
