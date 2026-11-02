#!/bin/bash
# Exercise the production Rust engine and direct Apple codec backend.
# Build the production native-tests driver first.
set -o pipefail
cd "$(dirname "$0")/../../.."
[[ -x .build/gui/native-tests ]] || { echo 'Run apple/macos/scripts/build_gui_tests.sh first' >&2; exit 2; }
checks=("$@")
if [[ ${#checks[@]} -eq 0 ]]; then
  checks=(check_gui_engine.py check_android_transfer.py check_apple_hdr.py check_auxiliary.py
    check_batch_performance.py check_cover_candidates.py check_existing_output.py check_extended.py
    check_honor_orientation.py check_honor_private_hdr.py check_huawei.py
    check_multi_import.py check_optional_data_and_vendor.py check_output_names.py check_reverse.py
    check_reverse_heic.py check_samsung_heic.py check_vendors.py check_video_metadata.py
    check_workspace.py check_xiaomi.py)
fi
passed=(); failed=()
for check in "${checks[@]}"; do
  echo "== $check"
  args=()
  [[ "$check" == check_huawei.py ]] && args=(--driver .build/gui/native-tests)
  # Bash 3.2 treats an empty array as unset under nounset; this runner does
  # not use nounset and passes zero arguments for ordinary checks.
  if python3 "apple/macos/scripts/$check" "${args[@]}"; then passed+=("$check"); else failed+=("$check"); fi
done
echo "passed (${#passed[@]}): ${passed[*]}"
echo "failed (${#failed[@]}): ${failed[*]}"
[[ ${#failed[@]} -eq 0 ]]
