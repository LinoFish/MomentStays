#!/bin/bash
# Run check_*.py harnesses against the Rust engine instead of the Swift one.
#
#   apple/macos/scripts/run_checks_with_rust.sh                 # all NativeDriver checks
#   apple/macos/scripts/run_checks_with_rust.sh check_vendors.py check_reverse.py
#
# Needs the Swift test driver (.build/gui/native-tests, from build_gui_tests.sh)
# for the `fixture` mode only. The Swift driver is moved aside while the
# checks run and always restored. Per-request routing is logged in
# .build/rust-shim.log; a summary is printed at the end.
set -uo pipefail
cd "$(dirname "$0")/../../.."
gui=.build/gui/native-tests
[[ -x "$gui" ]] || { echo "Missing $gui; run apple/macos/scripts/build_gui_tests.sh first" >&2; exit 2; }
cargo build --release --quiet --manifest-path core/Cargo.toml --bin momentstays-driver || exit 2

checks=("$@")
if [[ ${#checks[@]} -eq 0 ]]; then
  checks=(check_gui_engine.py check_android_transfer.py check_apple_hdr.py check_auxiliary.py
    check_batch_performance.py check_cover_candidates.py check_existing_output.py check_extended.py
    check_honor_orientation.py check_honor_private_hdr.py check_huawei.py check_motion_hover.py
    check_multi_import.py check_optional_data_and_vendor.py check_output_names.py check_reverse.py
    check_reverse_heic.py check_samsung_heic.py check_vendors.py check_video_metadata.py
    check_workspace.py check_xiaomi.py)
fi

mv "$gui" "$gui.swift"
restore() { rm -f "$gui"; mv "$gui.swift" "$gui"; }
trap restore EXIT
cp apple/macos/scripts/rust_driver_shim.py "$gui"
chmod +x "$gui"
: > .build/rust-shim.log

passed=(); failed=()
for check in "${checks[@]}"; do
  echo "== $check"
  args=()
  [[ "$check" == check_huawei.py ]] && args=(--driver "$gui")
  if python3 "apple/macos/scripts/$check" "${args[@]}"; then passed+=("$check"); else failed+=("$check"); fi
done

echo
echo "passed (${#passed[@]}): ${passed[*]:-}"
echo "failed (${#failed[@]}): ${failed[*]:-}"
python3 - <<'PY'
import collections, json
rows = [json.loads(l) for l in open('.build/rust-shim.log', encoding='utf-8') if l.strip()]
native = collections.Counter(r['mode'] for r in rows if r['needs_native'])
print(f"requests: {len(rows)}, answered by Rust: {sum(r['engine'] == 'rust' for r in rows)}")
if native:
    print("Rust asked for host codecs (expected without the app's callbacks):", dict(native))
PY
[[ ${#failed[@]} -eq 0 ]]
