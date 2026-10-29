#!/bin/bash
# Targeted append/cancellation regression, real mixed-input output, optional scan benchmark.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# -lt 1 ]]; then echo "Usage: $0 GOOGLE_MOTION_JPEG [--benchmark] [--skip-build]" >&2; exit 2; fi
scan_fixture="$1"; shift
scan_measure=0; scan_build=1
for scan_arg in "$@"; do
  case "$scan_arg" in --benchmark) scan_measure=1 ;; --skip-build) scan_build=0 ;; *) echo "Unknown option: $scan_arg" >&2; exit 2 ;; esac
done
mkdir -p .build/input-scan-fix
scan_binary="$PWD/.build/input-scan-fix/model-tests-fixed"
if [[ "$scan_build" == 1 ]]; then
  source scripts/rust_bridge.sh x86_64
  scan_sources=()
  for scan_name in RustCore MotionLive PhotoFormat PhotoHEIF PhotoHDR BatchEngine CoverCandidates HEIFMetadata AndroidHDR AndroidExport AndroidTransfer; do scan_sources+=("native/$scan_name.swift"); done
  xcrun swiftc -parse-as-library -D MOTIONLIVE_LIBRARY -O -target x86_64-apple-macosx12.0 \
    -module-cache-path "$PWD/.build/input-scan-fix/module-cache" "${scan_sources[@]}" \
    gui/ConversionSettings.swift gui/ConversionService.swift gui/WorkspaceModel.swift gui/DeliveryModels.swift gui/PhotoLibraryService.swift \
    tests/InputScanTests.swift tests/InputScanBenchmark.swift "${rust_bridge_flags[@]}" -o "$scan_binary"
fi
scan_run=$(mktemp -d "$PWD/.build/input-scan-fix/check.XXXXXX")
"$scan_binary" "$scan_run" --fixture "$scan_fixture"
if [[ "$scan_measure" == 1 ]]; then
  "$scan_binary" "$scan_run/benchmark" --benchmark "$scan_fixture"
  "$scan_binary" "$scan_run/model-benchmark" --model-benchmark "$scan_fixture"
fi
