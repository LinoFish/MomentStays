#!/bin/bash
# Run from repository root on a macOS host with Xcode, Rust iOS target, and simulator.
set -euo pipefail
WORK="${1:-$PWD/cross-platform-check}"
mkdir -p "$WORK"; WORK="$(cd "$WORK" && pwd)"
source apple/macos/scripts/rust_bridge.sh x86_64
xcrun swiftc -parse-as-library "${rust_bridge_flags[@]}" apple/shared/*.swift apple/shared/tests/TestFixtures.swift apple/ios/Engine/BuildHelpers/MakeNativeFixture.swift -o "$WORK/fixture"
"$WORK/fixture" "$WORK/fixture-data"
xcrun swiftc -parse-as-library -DMOTIONLIVE_NATIVE_ENGINE "${rust_bridge_flags[@]}" apple/shared/*.swift apple/ios/Engine/ConversionEngine.swift apple/ios/Engine/NativeConversionAdapter.swift apple/ios/Engine/BuildHelpers/ContentVerification.swift apple/ios/Engine/BuildHelpers/AdapterHarness.swift -o "$WORK/mac-harness"
mkdir -p "$WORK/mac"
"$WORK/mac-harness" "$WORK/fixture-data/input/motion.jpg" "$WORK/mac"
TEST=apple/ios/MomentStaysTests/CrossPlatformAdapterCheck.swift
VERIFY=apple/ios/MomentStaysTests/CrossPlatformContentVerification.swift
if [[ -e "$TEST" || -e "$VERIFY" ]]; then echo "Refusing to replace integration test files" >&2; exit 1; fi
trap 'rm -f "$TEST" "$VERIFY"' EXIT
{ echo '@testable import MomentStays'; cat apple/ios/Engine/BuildHelpers/ContentVerification.swift; } > "$VERIFY"
python3 - "$WORK" "$TEST" <<'PY'
import base64, pathlib, sys
root = pathlib.Path(sys.argv[1]); dest = pathlib.Path(sys.argv[2])
text = pathlib.Path('apple/ios/Engine/BuildHelpers/CrossPlatformAdapterCheck.swift.template').read_text()
paths = {'MOTION':root/'fixture-data/input/motion.jpg', 'STILL':root/'mac/apple/motion.jpg.livephoto/motion.jpg', 'MOVIE':root/'mac/apple/motion.jpg.livephoto/motion.mov'}
for name,path in paths.items(): text = text.replace('__'+name+'__', base64.b64encode(path.read_bytes()).decode())
dest.write_text(text)
PY
DEVICE="${SIMULATOR_UDID:-$(xcrun simctl list devices available | sed -nE 's/.*iPhone.*\(([A-F0-9-]{36})\).*/\1/p' | sed -n 1p)}"
test -n "$DEVICE"
xcrun simctl boot "$DEVICE" || true
xcrun simctl bootstatus "$DEVICE" -b
xcodebuild -project apple/ios/MomentStays.xcodeproj -scheme MomentStays -destination "platform=iOS Simulator,id=$DEVICE" -derivedDataPath "$WORK/derived" -parallel-testing-enabled NO -only-testing:MomentStaysTests/CrossPlatformAdapterCheck test CODE_SIGNING_ALLOWED=NO
CONTAINER=$(xcrun simctl get_app_container "$DEVICE" com.app9x3k2q.momentstays data)
cp -R "$CONTAINER/Documents/CrossPlatformAdapterCheck" "$WORK/simulator"
python3 apple/ios/Engine/BuildHelpers/compare-outputs.py "$WORK/mac" "$WORK/simulator"
