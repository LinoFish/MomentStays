#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
work=$(mktemp -d "${TMPDIR:-/tmp}/momentstays-time-check.XXXXXX")
trap 'rm -rf "$work"' EXIT
xcrun swiftc -module-cache-path "$work/module-cache" \
  apple/macos/Sources/PhotoTimeRepair.swift apple/macos/tests/PhotoTimeRepairTests.swift \
  -framework ImageIO -framework CoreGraphics -framework CryptoKit -o "$work/PhotoTimeRepairTests"
"$work/PhotoTimeRepairTests"
