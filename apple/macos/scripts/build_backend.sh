#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
build_dir=".build/backend"
mkdir -p "$build_dir"
if [[ $# -gt 0 ]]; then
  echo "Usage: $0 (Intel backend library and headless test driver only)" >&2
  exit 2
fi
source apple/macos/scripts/rust_bridge.sh
sources=(apple/shared/RustCore.swift apple/shared/MotionLive.swift apple/shared/PhotoFormat.swift apple/shared/PhotoHEIF.swift apple/shared/PhotoHDR.swift apple/shared/BatchEngine.swift
  apple/shared/CoverCandidates.swift apple/shared/HEIFMetadata.swift apple/shared/AndroidHDR.swift apple/shared/AndroidExport.swift apple/shared/AndroidTransfer.swift
  apple/macos/Sources/ConversionSettings.swift apple/macos/Sources/DeliveryModels.swift apple/macos/Sources/PhotoLibraryService.swift)
python3 - "${sources[@]}" apple/shared/tests/NativeDriver.swift <<'PY'
import hashlib,json,sys
from pathlib import Path
values={name:hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in sys.argv[1:]}
Path('.build/backend/source-hashes.json').write_text(json.dumps(values,indent=2))
driver=Path('apple/shared/tests/NativeDriver.swift').read_text()
Path('.build/backend/NativeDriver.swift').write_text('@testable import MomentStaysBackend\n'+driver)
PY
# Library includes the engine and PhotoKit services, with no SwiftUI views or app host.
# Testing visibility lets the headless driver exercise the existing internal API.
xcrun swiftc -parse-as-library -D MOTIONLIVE_LIBRARY -enable-testing -O -target x86_64-apple-macosx12.0 \
  -module-cache-path /private/tmp/motionlive-backend-swift-cache \
  -emit-library -emit-module -module-name MomentStaysBackend \
  -emit-module-path "$build_dir/MomentStaysBackend.swiftmodule" \
  -Xlinker -install_name -Xlinker @rpath/libMomentStaysBackend.dylib \
  "${sources[@]}" "${rust_bridge_flags[@]}" -o "$build_dir/libMomentStaysBackend.dylib"
xcrun swiftc -parse-as-library -O -target x86_64-apple-macosx12.0 \
  -module-cache-path /private/tmp/motionlive-backend-swift-cache \
  -I "$build_dir" -L "$build_dir" -lMomentStaysBackend \
  -Xlinker -rpath -Xlinker @executable_path \
  "$build_dir/NativeDriver.swift" -o "$build_dir/native-tests"
python3 - <<'PY'
import hashlib,json
from pathlib import Path
values=json.loads(Path('.build/backend/source-hashes.json').read_text())
changed=[name for name,digest in values.items() if hashlib.sha256(Path(name).read_bytes()).hexdigest()!=digest]
if changed:
    raise SystemExit('Backend sources changed during build; rebuild required: '+', '.join(changed))
PY
codesign --force --sign - "$build_dir/libMomentStaysBackend.dylib"
codesign --force --sign - "$build_dir/native-tests"
codesign --verify --strict "$build_dir/libMomentStaysBackend.dylib"
codesign --verify --strict "$build_dir/native-tests"
echo "Created Intel backend library and headless test driver in $build_dir."
