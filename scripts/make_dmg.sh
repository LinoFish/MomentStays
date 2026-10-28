#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
app="dist/MomentStays.app"
test -d "$app"
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
archs=$(xcrun lipo -archs "$app/Contents/MacOS/MomentStays")
case "$archs" in
  x86_64) platform=intel ;;
  arm64) platform=arm64 ;;
  *x86_64*arm64*|*arm64*x86_64*) platform=universal ;;
  *) echo "Unsupported architectures: $archs" >&2; exit 2 ;;
esac
name="MomentStays-$version-$platform"
codesign --verify --deep --strict "$app"
staging=$(mktemp -d "$PWD/.build/gui/dmg-root.XXXXXX")
trap 'rm -rf "$staging"' EXIT
ditto "$app" "$staging/MomentStays.app"
cp packaging/使用说明.txt "$staging/使用说明.txt"
cp LICENSE COPYRIGHT THIRD_PARTY_NOTICES.md "$staging/"
ln -s /Applications "$staging/Applications"
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/$name.zip"
hdiutil create -volname "MomentStays $version" -srcfolder "$staging" -ov -format UDZO "dist/$name.dmg"
(cd dist && shasum -a 256 "$name.zip" "$name.dmg" > SHA256SUMS.txt)
echo "Created dist/$name.dmg and dist/$name.zip"
