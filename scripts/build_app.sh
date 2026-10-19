#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/gui dist
sources=(native/MotionLive.swift native/PhotoFormat.swift native/PhotoHEIF.swift native/PhotoHDR.swift native/BatchEngine.swift native/HEIFMetadata.swift native/AndroidHDR.swift native/AndroidExport.swift gui/PhotosLibraryPicker.swift gui/App.swift)
for arch in x86_64 arm64; do
  echo "Building macOS 12 app: $arch"
  xcrun swiftc -parse-as-library -D MOTIONLIVE_LIBRARY -O -target "$arch-apple-macosx12.0" \
    -module-cache-path "/private/tmp/motionlive-gui-swift-cache-$arch" "${sources[@]}" \
    -o ".build/gui/Motion-Live-$arch"
done
app="dist/Motion-Live.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
xcrun lipo -create .build/gui/Motion-Live-x86_64 .build/gui/Motion-Live-arm64 -output "$app/Contents/MacOS/Motion-Live"
cp packaging/Info.plist "$app/Contents/Info.plist"
cp packaging/使用说明.txt "$app/Contents/Resources/使用说明.txt"
xcrun swiftc -module-cache-path /private/tmp/motionlive-swift-cache packaging/Icon.swift -o .build/gui/make-icon
.build/gui/make-icon .build/gui/AppIcon.iconset
iconutil -c icns .build/gui/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
ditto -c -k --sequesterRsrc --keepParent "$app" dist/Motion-Live-1.2.0-universal.zip
echo "App and shareable ZIP created."
