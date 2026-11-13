#!/bin/bash
# Use the same icon source as the command-line app build; no tracked binaries.
set -euo pipefail
: "${SRCROOT:?This script must run as an Xcode build phase}"
: "${DERIVED_FILE_DIR:?Missing Xcode derived file directory}"
: "${TARGET_BUILD_DIR:?Missing Xcode target build directory}"
: "${UNLOCALIZED_RESOURCES_FOLDER_PATH:?Missing app resources path}"

work="$DERIVED_FILE_DIR/app-icons"
resources="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
mkdir -p "$work" "$resources"
xcrun --sdk macosx swiftc -module-cache-path "$work/module-cache" \
  "$SRCROOT/packaging/Icon.swift" -o "$work/make-icon"
"$work/make-icon" "$work/AppIcon.iconset" light
"$work/make-icon" "$work/AppIconDark.iconset" dark
xcrun iconutil -c icns "$work/AppIcon.iconset" -o "$resources/AppIcon.icns"
xcrun iconutil -c icns "$work/AppIconDark.iconset" -o "$resources/AppIconDark.icns"
