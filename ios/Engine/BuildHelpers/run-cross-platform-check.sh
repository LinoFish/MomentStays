#!/bin/bash
# Run from repository root on a macOS host with Xcode, Rust iOS target, and simulator.
set -euo pipefail
WORK="${1:-$PWD/cross-platform-check}"
mkdir -p "$WORK"; WORK="$(cd "$WORK" && pwd)"
COMMON=(-DMOTIONLIVE_SWIFT_REFERENCE -DMOTIONLIVE_LIBRARY native/*.swift)
xcrun swiftc "${COMMON[@]}" ios/Engine/BuildHelpers/MakeNativeFixture.swift -o "$WORK/fixture"
"$WORK/fixture" "$WORK/fixture-data"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --manifest-path core/Cargo.toml --locked --release --lib
xcrun swiftc -DMOTIONLIVE_LIBRARY -DMOTIONLIVE_NATIVE_ENGINE -import-objc-header native/RustCore.h native/*.swift ios/Engine/ConversionEngine.swift ios/Engine/NativeConversionAdapter.swift ios/Engine/BuildHelpers/ContentVerification.swift ios/Engine/BuildHelpers/AdapterHarness.swift core/target/release/libmomentstays_core.a -o "$WORK/mac-harness"
mkdir -p "$WORK/mac"
"$WORK/mac-harness" "$WORK/fixture-data/input/motion.jpg" "$WORK/mac"
TEST=ios/MomentStaysTests/CrossPlatformAdapterCheck.swift
VERIFY=ios/MomentStaysTests/CrossPlatformContentVerification.swift
if [[ -e "$TEST" || -e "$VERIFY" ]]; then echo "Refusing to replace integration test files" >&2; exit 1; fi
trap 'rm -f "$TEST" "$VERIFY"' EXIT
{ echo '@testable import MomentStays'; cat ios/Engine/BuildHelpers/ContentVerification.swift; } > "$VERIFY"
python3 - "$WORK" "$TEST" <<'PY'
import base64, pathlib, sys
root = pathlib.Path(sys.argv[1]); dest = pathlib.Path(sys.argv[2])
text = pathlib.Path('ios/Engine/BuildHelpers/CrossPlatformAdapterCheck.swift.template').read_text()
paths = {'MOTION':root/'fixture-data/input/motion.jpg', 'STILL':root/'mac/apple/motion.jpg.livephoto/motion.jpg', 'MOVIE':root/'mac/apple/motion.jpg.livephoto/motion.mov'}
for name,path in paths.items(): text = text.replace('__'+name+'__', base64.b64encode(path.read_bytes()).decode())
dest.write_text(text)
PY
DEVICE="${SIMULATOR_UDID:-$(xcrun simctl list devices available | sed -nE 's/.*iPhone.*\(([A-F0-9-]{36})\).*/\1/p' | sed -n 1p)}"
test -n "$DEVICE"
xcrun simctl boot "$DEVICE" || true
xcrun simctl bootstatus "$DEVICE" -b
xcodebuild -project ios/MomentStays.xcodeproj -scheme MomentStays -destination "platform=iOS Simulator,id=$DEVICE" -derivedDataPath "$WORK/derived" -parallel-testing-enabled NO -only-testing:MomentStaysTests/CrossPlatformAdapterCheck test CODE_SIGNING_ALLOWED=NO
CONTAINER=$(xcrun simctl get_app_container "$DEVICE" com.app9x3k2q.momentstays data)
cp -R "$CONTAINER/Documents/CrossPlatformAdapterCheck" "$WORK/simulator"
python3 ios/Engine/BuildHelpers/compare-outputs.py "$WORK/mac" "$WORK/simulator"
