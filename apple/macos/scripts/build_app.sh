#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
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
sources=(apple/shared/RustCore.swift apple/shared/MotionLive.swift apple/shared/PhotoFormat.swift apple/shared/BatchEngine.swift apple/shared/CoverCandidates.swift apple/shared/AndroidExport.swift apple/shared/AndroidTransfer.swift
  apple/macos/Sources/ConversionSettings.swift apple/macos/Sources/ConversionService.swift apple/macos/Sources/WorkspaceModel.swift apple/macos/Sources/DeliveryModels.swift apple/macos/Sources/PhotoLibraryService.swift apple/macos/Sources/SystemSharingService.swift
  apple/macos/Sources/InterfaceSupport.swift apple/macos/Sources/ExportSettingsView.swift apple/macos/Sources/MotionPreview.swift apple/macos/Sources/PhotoTimelineView.swift apple/macos/Sources/PhotoQueueView.swift apple/macos/Sources/WorkspaceView.swift apple/macos/Sources/ReportView.swift
  apple/macos/Sources/PhotosLibraryPicker.swift apple/macos/Sources/MacPlatformServices.swift apple/macos/Sources/UISnapshotSupport.swift apple/macos/Sources/AppIconAppearance.swift apple/macos/Sources/App.swift)
if [[ ${#architectures[@]} -gt 1 ]]; then
  source apple/macos/scripts/prepare_universal_rust.sh
  export RUSTC="$motionlive_rustc"
fi
binaries=()
for arch in "${architectures[@]}"; do
  source apple/macos/scripts/rust_bridge.sh "$arch"
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
cp apple/macos/packaging/Info.plist "$app/Contents/Info.plist"
rm -rf "$app/Contents/Resources/zh-Hans.lproj"
cp -R apple/macos/packaging/zh-Hans.lproj "$app/Contents/Resources/zh-Hans.lproj"
cp apple/macos/packaging/使用说明.txt "$app/Contents/Resources/使用说明.txt"
cp LICENSE COPYRIGHT THIRD_PARTY_NOTICES.md "$app/Contents/Resources/"
xcrun swiftc -module-cache-path /private/tmp/motionlive-swift-cache apple/macos/packaging/Icon.swift -o .build/gui/make-icon
.build/gui/make-icon .build/gui/AppIcon.iconset light
.build/gui/make-icon .build/gui/AppIconDark.iconset dark
iconutil -c icns .build/gui/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"
iconutil -c icns .build/gui/AppIconDark.iconset -o "$app/Contents/Resources/AppIconDark.icns"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
echo "Created $app (${architectures[*]}); no ZIP or DMG generated."
