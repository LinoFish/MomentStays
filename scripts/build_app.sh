#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/gui dist
architectures=(x86_64)
definitions=(-D MOTIONLIVE_LIBRARY)
optimization=(-O -whole-module-optimization)
app="dist/MomentStays.app"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --universal) architectures=(x86_64 arm64); shift ;;
    --arm64) architectures=(arm64); shift ;;
    --diagnostics) definitions+=(-D UI_DIAGNOSTICS); optimization=(-Onone); app=".build/gui/preview/MomentStays.app"; shift ;;
    --help) echo "Usage: $0 [--universal | --arm64] [--diagnostics] (default: Intel .app only)"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done
sources=(native/MotionLive.swift native/PhotoFormat.swift native/PhotoHEIF.swift native/PhotoHDR.swift native/BatchEngine.swift native/CoverCandidates.swift native/HEIFMetadata.swift native/AndroidHDR.swift native/AndroidExport.swift
  gui/ConversionSettings.swift gui/ConversionService.swift gui/WorkspaceModel.swift gui/DeliveryModels.swift gui/PhotoLibraryService.swift gui/SystemSharingService.swift
  gui/InterfaceSupport.swift gui/ExportSettingsView.swift gui/MotionPreview.swift gui/PhotoTimelineView.swift gui/PhotoQueueView.swift gui/WorkspaceView.swift gui/ReportView.swift
  gui/PhotosLibraryPicker.swift gui/MacPlatformServices.swift gui/UISnapshotSupport.swift gui/AppIconAppearance.swift gui/App.swift)
binaries=()
for arch in "${architectures[@]}"; do
  echo "Building macOS 12 app: $arch"
  xcrun swiftc -parse-as-library "${definitions[@]}" "${optimization[@]}" -target "$arch-apple-macosx12.0" \
    -module-cache-path "/private/tmp/motionlive-gui-swift-cache-$arch" "${sources[@]}" \
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
