#!/usr/bin/env python3
"""Native macOS 12 SwiftUI layout checks with synthetic data only.

Build with `bash scripts/build_app.sh --diagnostics` first. No AppleScript,
accessibility permissions, photo library, or user files are needed.
"""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / ".build/gui/preview/Motion-Live.app/Contents/MacOS/Motion-Live"
OUTPUT = ROOT / ".build/gui/ui-checks"
OUTPUT.mkdir(parents=True, exist_ok=True)
cases = [
    ("wide", 1120, 700, []),
    ("compact", 390, 540, ["--ui-select-first"]),
    ("medium", 760, 600, []),
    ("wide-short", 1440, 540, []),
    ("dark-reverse", 1120, 700, ["--direction", "to-android", "--ui-appearance", "dark"]),
    ("compact-settings", 390, 540, ["--direction", "to-android", "--ui-expand-settings"]),
    ("compact-progress", 390, 540, ["--ui-progress-preview"]),
    ("compact-sharing", 390, 540, ["--direction", "to-android", "--ui-destination", "share"]),
    ("compact-photos", 390, 540, ["--ui-destination", "photos"]),
    ("photos-settings", 390, 540, ["--ui-destination", "folderAndPhotos", "--ui-expand-settings"]),
    ("empty", 760, 600, ["--ui-fixtures", "0"]),
    ("error", 390, 540, ["--ui-error-preview"]),
    ("report", 390, 540, ["--ui-report-preview"]),
    ("resize", 1120, 700, ["--ui-resize-check", "--ui-fixtures", "1000"]),
    ("resize-settings", 1120, 700, ["--ui-resize-check", "--ui-expand-settings"]),
]
results = []
for name, width, height, flags in cases:
    screenshot = OUTPUT / (name + ".png")
    result = subprocess.run([str(APP), "--ui-snapshot", str(screenshot), "--ui-width", str(width), "--ui-height", str(height), *flags],
        text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, (name, result.stdout, result.stderr)
    assert screenshot.exists() and screenshot.stat().st_size > 1000, (name, "missing screenshot")
    metrics = json.loads(Path(str(screenshot) + ".json").read_text())
    assert metrics["primary_visible"], (name, "primary action outside content", metrics)
    assert metrics["queue_visible"], (name, "queue outside content", metrics)
    assert metrics["queue_height"] >= 150, (name, "queue cannot show three compact rows", metrics)
    for table in metrics["native_tables"]:
        assert not table["horizontal_overflow"], (name, "native list has horizontal overflow", table)
    for check in metrics["resize_checks"]:
        assert check["primary_visible"] and check["queue_visible"], (name, "resize clipping", check)
        assert check["selection_preserved"] and check["settings_preserved"], (name, "resize lost state", check)
        assert all(table["selected_row"] == 0 for table in check["native_tables"]), (name, "native selection lost", check)
        if "--ui-expand-settings" in flags:
            assert check["settings_sheet_visible"] and not check["inspector_visible"], (name, "duplicated settings", check)
    if name == "wide":
        assert metrics["inspector_visible"] and metrics["queue_width"] >= 550, metrics
    if name == "compact-settings":
        assert metrics["settings_sheet_visible"], metrics
    if "Unable to simultaneously satisfy constraints" in result.stderr:
        raise AssertionError((name, result.stderr))
    results.append({"name": name, "queue_height": metrics["queue_height"], "queue_width": metrics["queue_width"], "resize_checks": len(metrics["resize_checks"])})
    print("PASS", name, "queue", metrics["queue_width"], "×", metrics["queue_height"], flush=True)
(OUTPUT / "results.json").write_text(json.dumps(results, indent=2))
print("PASS", len(results), "native SwiftUI scenarios; screenshots in", OUTPUT)
