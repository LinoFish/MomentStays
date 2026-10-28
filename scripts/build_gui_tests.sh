#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/gui
xcrun swiftc -parse-as-library -D MOTIONLIVE_LIBRARY -O \
  -module-cache-path /private/tmp/motionlive-swift-cache \
  native/MotionLive.swift native/PhotoFormat.swift native/PhotoHEIF.swift native/PhotoHDR.swift native/BatchEngine.swift native/HEIFMetadata.swift native/AndroidHDR.swift native/AndroidExport.swift \
  tests/NativeDriver.swift -o .build/gui/native-tests
