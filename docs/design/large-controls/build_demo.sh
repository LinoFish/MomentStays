#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
output=".build/gui/large-controls-demo"
app="$output/MomentStays Large Controls Demo.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for arch in x86_64 arm64; do
  xcrun swiftc -parse-as-library -Onone -target "$arch-apple-macosx12.0" \
    -module-cache-path "/private/tmp/motionlive-gui-swift-cache-$arch" \
    docs/design/large-controls/LargeControlsDemo.swift -o "$output/LargeControlsDemo-$arch"
done
xcrun lipo -create "$output/LargeControlsDemo-x86_64" "$output/LargeControlsDemo-arm64" -output "$app/Contents/MacOS/LargeControlsDemo"
cp dist/MomentStays.app/Contents/Resources/AppIcon.icns "$app/Contents/Resources/"
cp dist/MomentStays.app/Contents/Resources/AppIconDark.icns "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>MomentStays Large Controls Demo</string>
<key>CFBundleIdentifier</key><string>app.motionlive.converter.large-controls-demo</string>
<key>CFBundleExecutable</key><string>LargeControlsDemo</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" "$output/MomentStays-Large-Controls-Demo.zip"
echo "Created $output/MomentStays-Large-Controls-Demo.zip"
