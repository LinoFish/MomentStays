#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
app="dist/留刻.app"
test -d "$app"
version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
archs=$(xcrun lipo -archs "$app/Contents/MacOS/MomentStays")
case "$archs" in
  x86_64) platform=intel ;;
  arm64) platform=arm64 ;;
  *x86_64*arm64*|*arm64*x86_64*) platform=universal ;;
  *) echo "Unsupported architectures: $archs" >&2; exit 2 ;;
esac
name="留刻-$version-$platform"
codesign --verify --deep --strict "$app"
mkdir -p .build/gui
export PYTHONPATH="$PWD/.build/dmg-tools${PYTHONPATH:+:$PYTHONPATH}"
if ! python3 -c 'import ds_store, mac_alias' 2>/dev/null; then
  echo "Install packaging tools: python3 -m pip install --target .build/dmg-tools -r packaging/dmg-requirements.txt" >&2
  exit 1
fi
work=$(mktemp -d "$PWD/.build/gui/dmg-work.XXXXXX")
staging="$work/root"
dmg_mount=""
cleanup() {
  if [[ -n "$dmg_mount" ]]; then hdiutil detach "$dmg_mount" >/dev/null || hdiutil detach -force "$dmg_mount" >/dev/null; fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$staging/.background"
ditto "$app" "$staging/留刻.app"
ln -s /Applications "$staging/Applications"
xcrun swiftc -module-cache-path /private/tmp/motionlive-swift-cache packaging/DmgBackground.swift -o .build/gui/dmg-background
.build/gui/dmg-background "$staging/.background/installer.png"
hdiutil create -volname "留刻 $version" -fs HFS+ -srcfolder "$staging" -ov -format UDRW "$work/installer-rw.dmg"
hdiutil attach -readwrite -noverify -noautoopen -nobrowse -plist "$work/installer-rw.dmg" > "$work/attach.plist"
dmg_mount=$(python3 - "$work/attach.plist" <<'PY'
import plistlib,sys
with open(sys.argv[1],'rb') as file:
    print(next(item['mount-point'] for item in plistlib.load(file)['system-entities'] if 'mount-point' in item))
PY
)
python3 packaging/dmg_layout.py "$dmg_mount"
cp "$app/Contents/Resources/AppIcon.icns" "$dmg_mount/.VolumeIcon.icns"
SetFile -a C "$dmg_mount"
hdiutil detach "$dmg_mount"
dmg_mount=""
hdiutil convert "$work/installer-rw.dmg" -format UDZO -imagekey zlib-level=9 -o "dist/$name.dmg" -ov
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/$name.zip"
(cd dist && LC_ALL=C shasum -a 256 "$name.zip" "$name.dmg" > SHA256SUMS.txt)
echo "Created dist/$name.dmg and dist/$name.zip"
