#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
test -d dist/Motion-Live.app
staging=".build/gui/dmg-root"
mkdir -p "$staging"
if test -d "$staging/Motion-Live.app"; then rm -rf "$staging/Motion-Live.app"; fi
ditto dist/Motion-Live.app "$staging/Motion-Live.app"
cp packaging/使用说明.txt "$staging/使用说明.txt"
if ! test -L "$staging/Applications"; then ln -s /Applications "$staging/Applications"; fi
hdiutil create -volname "Motion-Live" -srcfolder "$staging" -ov -format UDZO dist/Motion-Live-1.2.0-universal.dmg
(cd dist && shasum -a 256 Motion-Live-1.2.0-universal.zip Motion-Live-1.2.0-universal.dmg > SHA256SUMS.txt)
