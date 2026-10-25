#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/gui
xcrun swiftc -parse-as-library -D MOTIONLIVE_LIBRARY -Onone -target x86_64-apple-macosx12.0 \
  -module-cache-path /private/tmp/motionlive-gui-swift-cache-x86_64 \
  native/MotionLive.swift native/PhotoFormat.swift native/PhotoHEIF.swift native/PhotoHDR.swift \
  native/BatchEngine.swift native/HEIFMetadata.swift native/AndroidHDR.swift native/AndroidExport.swift \
  gui/MotionPreview.swift tests/MotionPreviewTests.swift -o .build/gui/motion-preview-tests
.build/gui/motion-preview-tests
