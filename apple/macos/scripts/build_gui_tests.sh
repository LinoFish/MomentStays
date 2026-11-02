#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
mkdir -p .build/gui
source apple/macos/scripts/rust_bridge.sh
xcrun swiftc "${rust_bridge_flags[@]}" -parse-as-library -D MOTIONLIVE_LIBRARY -O -whole-module-optimization -target x86_64-apple-macosx12.0 \
  -module-cache-path /private/tmp/motionlive-swift-cache \
  apple/shared/RustCore.swift apple/shared/MotionLive.swift apple/shared/PhotoFormat.swift apple/shared/PhotoHEIF.swift apple/shared/PhotoHDR.swift apple/shared/BatchEngine.swift apple/shared/CoverCandidates.swift apple/shared/HEIFMetadata.swift apple/shared/AndroidHDR.swift apple/shared/AndroidExport.swift apple/shared/AndroidTransfer.swift \
  apple/macos/Sources/ConversionSettings.swift apple/macos/Sources/DeliveryModels.swift apple/macos/Sources/PhotoLibraryService.swift \
  apple/shared/tests/NativeDriver.swift -o .build/gui/native-tests
