#!/bin/bash
# Run from repository root on a macOS host with an available iOS simulator.
set -euo pipefail
WORK="${1:-$PWD/adapter-check}"
mkdir -p "$WORK"
WORK="$(cd "$WORK" && pwd)"
COMMON=(-DMOTIONLIVE_SWIFT_REFERENCE -DMOTIONLIVE_LIBRARY native/*.swift)
xcrun swiftc "${COMMON[@]}" ios/Engine/BuildHelpers/MakeNativeFixture.swift -o "$WORK/fixture"
"$WORK/fixture" "$WORK/fixture-data"
xcrun swiftc "${COMMON[@]}" -DMOTIONLIVE_NATIVE_ENGINE ios/Engine/ConversionEngine.swift ios/Engine/NativeConversionAdapter.swift ios/Engine/BuildHelpers/ContentVerification.swift ios/Engine/BuildHelpers/AdapterHarness.swift -o "$WORK/mac-harness"
mkdir -p "$WORK/mac"
"$WORK/mac-harness" "$WORK/fixture-data/input/motion.jpg" "$WORK/mac"
APP="$WORK/AdapterHarness.app"
mkdir -p "$APP"
cp "$WORK/fixture-data/input/motion.jpg" "$APP/motion.jpg"
cat > "$APP/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>org.momentstays.AdapterHarness</string><key>CFBundleExecutable</key><string>AdapterHarness</string><key>CFBundleName</key><string>AdapterHarness</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>1</string><key>MinimumOSVersion</key><string>16.0</string><key>LSRequiresIPhoneOS</key><true/></dict></plist>
PLIST
SDK=$(xcrun --sdk iphonesimulator --show-sdk-path)
xcrun swiftc -target arm64-apple-ios16.0-simulator -sdk "$SDK" "${COMMON[@]}" -DMOTIONLIVE_NATIVE_ENGINE ios/XMLCompat/XMLCompat.swift ios/Engine/ConversionEngine.swift ios/Engine/NativeConversionAdapter.swift ios/Engine/BuildHelpers/ContentVerification.swift ios/Engine/BuildHelpers/AdapterHarness.swift -o "$APP/AdapterHarness"
codesign --force --sign - "$APP"
DEVICE="${SIMULATOR_UDID:-$(xcrun simctl list devices available | sed -nE 's/.*iPhone.*\(([A-F0-9-]{36})\).*/\1/p' | sed -n 1p)}"
test -n "$DEVICE"
xcrun simctl boot "$DEVICE" || true
xcrun simctl bootstatus "$DEVICE" -b
xcrun simctl uninstall "$DEVICE" org.momentstays.AdapterHarness || true
xcrun simctl install "$DEVICE" "$APP"
xcrun simctl launch --console "$DEVICE" org.momentstays.AdapterHarness >"$WORK/simulator-console.txt" 2>&1 &
CONSOLE_PID=$!
CONTAINER=$(xcrun simctl get_app_container "$DEVICE" org.momentstays.AdapterHarness data)
for ((i=0;i<120;i++)); do [[ -f "$CONTAINER/Documents/status.txt" ]] && break; sleep 1; done
cp -R "$CONTAINER/Documents" "$WORK/simulator"
kill "$CONSOLE_PID" 2>/dev/null || true
cat "$WORK/simulator-console.txt"
cat "$WORK/simulator/status.txt"
grep -qx PASS "$WORK/simulator/status.txt"
echo 'Media SHA-256 (outputs may differ due to identifiers and container metadata):'
find "$WORK/mac" "$WORK/simulator" -type f \( -iname '*.mov' -o -iname '*.jpg' -o -iname '*.heic' \) -exec shasum -a 256 {} \;
