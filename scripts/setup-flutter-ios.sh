#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
version=3.47.9
sha=0dc38b4038a0b002a6258696a5b327a58473c175c9e334b9f329ef7d79a4fdcd
sdk="$HOME/flutter-$version"
archive="${TMPDIR:-/tmp}/flutter_macos_${version}-stable.zip"
if [ ! -x "$sdk/bin/flutter" ]; then
  curl -fL --retry 3 "https://storage.googleapis.com/flutter_infra_release/releases/stable/macos/flutter_macos_${version}-stable.zip" -o "$archive"
  echo "$sha  $archive" | shasum -a 256 -c -
  # The requested checksum identifies the x64 distribution. Builders are arm64
  # and cannot execute it without Rosetta, so verify the official arm64 sibling too.
  if [ "$(uname -m)" = arm64 ]; then
    rm -f "$archive"
    archive="${TMPDIR:-/tmp}/flutter_macos_arm64_${version}-stable.zip"
    curl -fL --retry 3 "https://storage.googleapis.com/flutter_infra_release/releases/stable/macos/flutter_macos_arm64_${version}-stable.zip" -o "$archive"
    echo "6545b85938745dc5070e9faf2adf70233c1811009d87fe6d27bfcda5a04b7698  $archive" | shasum -a 256 -c -
  fi
  rm -rf "$sdk" "${sdk}.tmp"
  mkdir -p "${sdk}.tmp"
  unzip -q "$archive" -d "${sdk}.tmp"
  mv "${sdk}.tmp/flutter" "$sdk"
  rmdir "${sdk}.tmp"
  rm -f "$archive"
fi
export PATH="$sdk/bin:$HOME/.cargo/bin:$PATH"
flutter --version
(cd mobile && flutter create --template module --no-pub . && flutter pub get)
if ! command -v pod >/dev/null 2>&1; then
  echo 'CocoaPods is required (pod command missing)' >&2
  exit 1
fi
(cd ios && pod install)
