#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/gui dist
architectures=(x86_64 arm64)
definitions=(-D MOTIONLIVE_LIBRARY)
optimization=(-O -whole-module-optimization)
app="dist/留刻.app"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --universal) architectures=(x86_64 arm64); shift ;;
    --arm64) architectures=(arm64); shift ;;
    --diagnostics) definitions+=(-D UI_DIAGNOSTICS); optimization=(-Onone); app=".build/gui/preview/留刻.app"; shift ;;
    --help) echo "Usage: $0 [--universal | --arm64] [--diagnostics] (default: Universal .app)"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
sources=(native/RustCore.swift native/MotionLive.swift native/PhotoFormat.swift native/PhotoHEIF.swift native/PhotoHDR.swift native/BatchEngine.swift native/CoverCandidates.swift native/HEIFMetadata.swift native/AndroidHDR.swift native/AndroidExport.swift native/AndroidTransfer.swift
  gui/ConversionSettings.swift gui/ConversionService.swift gui/WorkspaceModel.swift gui/DeliveryModels.swift gui/PhotoLibraryService.swift gui/SystemSharingService.swift
  gui/InterfaceSupport.swift gui/ExportSettingsView.swift gui/MotionPreview.swift gui/PhotoTimelineView.swift gui/PhotoQueueView.swift gui/WorkspaceView.swift gui/ReportView.swift
  gui/PhotosLibraryPicker.swift gui/MacPlatformServices.swift gui/UISnapshotSupport.swift gui/AppIconAppearance.swift gui/App.swift)
if [[ ${#architectures[@]} -gt 1 ]]; then
  source scripts/prepare_universal_rust.sh
  export RUSTC="$motionlive_rustc"
fi
binaries=()
for arch in "${architectures[@]}"; do
  source scripts/rust_bridge.sh "$arch"
  echo "Building macOS 12 app: $arch"
  xcrun swiftc -parse-as-library "${definitions[@]}" "${optimization[@]}" -target "$arch-apple-macosx12.0" \
    -module-cache-path "/private/tmp/motionlive-gui-swift-cache-$arch" "${sources[@]}" "${rust_bridge_flags[@]}" \
    -o ".build/gui/MomentStays-$arch"
  binaries+=(".build/gui/MomentStays-$arch")
done
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
if [[ ${#binaries[@]} -eq 1 ]]; then
  cp "${binaries[0]}" "$app/Contents/MacOS/MomentStays"
else
  xcrun lipo -create "${binaries[@]}" -output "$app/Contents/MacOS/MomentStays"
fi
cp packaging/Info.plist "$app/Contents/Info.plist"
rm -rf "$app/Contents/Resources/zh-Hans.lproj"
cp -R packaging/zh-Hans.lproj "$app/Contents/Resources/zh-Hans.lproj"
cp packaging/使用说明.txt "$app/Contents/Resources/使用说明.txt"
cp LICENSE COPYRIGHT THIRD_PARTY_NOTICES.md "$app/Contents/Resources/"
xcrun swiftc -module-cache-path /private/tmp/motionlive-swift-cache packaging/Icon.swift -o .build/gui/make-icon
.build/gui/make-icon .build/gui/AppIcon.iconset light
.build/gui/make-icon .build/gui/AppIconDark.iconset dark
iconutil -c icns .build/gui/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
iconutil -c icns .build/gui/AppIconDark.iconset -o "$app/Contents/Resources/AppIconDark.icns"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
echo "Created $app (${architectures[*]}); no ZIP or DMG generated."
