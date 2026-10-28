#!/usr/bin/env python3
"""Native macOS 12 SwiftUI layout checks with synthetic data only.

Build with `bash scripts/build_app.sh --diagnostics` first. No AppleScript,
accessibility permissions, photo library, or user files are needed.
"""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / ".build/gui/preview/留刻.app/Contents/MacOS/MomentStays"
OUTPUT = ROOT / ".build/gui/ui-checks"
OUTPUT.mkdir(parents=True, exist_ok=True)
cases = [
    ("wide", 1120, 700, []),
    ("wide-threshold", 1040, 628, ["--direction", "to-android"]),
    ("long-locations", 720, 600, ["--direction", "to-android", "--ui-long-locations"]),
    ("cleared", 720, 600, ["--ui-clear-preview", "--ui-select-first"]),
    ("clear-busy", 720, 600, ["--ui-clear-preview", "--ui-progress-preview"]),
    ("output-menu", 720, 600, ["--ui-output-menu"]),
    ("output-menu-reverse", 1040, 628, ["--direction", "to-android", "--ui-output-menu"]),
    ("output-switch", 720, 600, ["--ui-output-menu", "--ui-output-switch"]),
    ("source-menu", 1120, 700, ["--ui-source-menu"]),
    ("compact", 390, 540, ["--ui-select-first"]),
    ("medium", 760, 600, []),
    ("medium-reverse", 760, 600, ["--direction", "to-android"]),
    ("wide-short", 1440, 540, []),
    ("dark-reverse", 1120, 700, ["--direction", "to-android", "--ui-appearance", "dark"]),
    ("compact-settings", 390, 540, ["--direction", "to-android", "--ui-expand-settings"]),
    ("compact-progress", 390, 540, ["--ui-progress-preview"]),
    ("compact-sharing", 390, 540, ["--direction", "to-android", "--ui-destination", "share"]),
    ("compact-photos", 390, 540, ["--ui-destination", "photos"]),
    ("photos-settings", 390, 540, ["--ui-destination", "folderAndPhotos", "--ui-expand-settings"]),
    ("wide-preview", 1120, 700, ["--ui-preview", "--ui-select-first"]),
    ("compact-preview", 390, 540, ["--ui-preview", "--ui-select-first"]),
    ("empty", 760, 600, ["--ui-fixtures", "0"]),
    ("compact-empty", 390, 540, ["--ui-fixtures", "0"]),
    ("compact-cover", 390, 540, ["--ui-select-first", "--ui-cover-editor"]),
    ("filtered-empty", 760, 600, ["--ui-scope", "completed"]),
    ("filtered-preview", 1120, 700, ["--ui-select-first", "--ui-preview", "--ui-scope", "completed"]),
    ("error", 390, 540, ["--ui-error-preview"]),
    ("report", 390, 540, ["--ui-report-preview"]),
    ("resize", 1120, 700, ["--ui-resize-check", "--ui-fixtures", "1000"]),
    ("resize-settings", 1120, 700, ["--ui-resize-check", "--ui-expand-settings"]),
    ("minimum-window", 200, 200, ["--direction", "to-android"]),
    ("library", 720, 600, ["--ui-library-preview"]),
    ("library-selected", 720, 600, ["--ui-library-preview", "--ui-library-selected"]),
    ("library-switch-version", 720, 600, ["--ui-library-preview", "--ui-library-selected", "--ui-library-switch-version"]),
    ("library-list", 720, 600, ["--ui-library-preview", "--ui-library-list"]),
    ("library-list-selected", 720, 600, ["--ui-library-preview", "--ui-library-list", "--ui-library-selected"]),
    ("library-switch-view", 720, 600, ["--ui-library-preview", "--ui-library-selected", "--ui-library-switch-view"]),
    ("library-resize", 720, 600, ["--ui-library-preview", "--ui-library-selected", "--ui-library-resize"]),
    ("library-empty", 720, 600, ["--ui-library-preview", "--ui-library-empty"]),
    ("library-dark", 720, 600, ["--ui-library-preview", "--direction", "to-android", "--ui-appearance", "dark"]),
    ("recent", 720, 600, ["--ui-recent-preview"]),
    ("recent-empty", 720, 600, ["--ui-recent-preview", "--ui-recent-empty"]),
]
results = []
for name, width, height, flags in cases:
    screenshot = OUTPUT / (name + ".png")
    result = subprocess.run([str(APP), "--ui-snapshot", str(screenshot), "--ui-width", str(width), "--ui-height", str(height), *flags],
        text=True, capture_output=True, timeout=30)
    assert result.returncode == 0, (name, result.stdout, result.stderr)
    assert screenshot.exists() and screenshot.stat().st_size > 1000, (name, "missing screenshot")
    metrics = json.loads(Path(str(screenshot) + ".json").read_text())
    assert metrics["minimum_width"] >= 720 and metrics["minimum_height"] >= 600, (name,"window sizing policy missing",metrics)
    assert metrics["frame_width"] >= metrics["minimum_width"] and metrics["frame_height"] >= metrics["minimum_height"], (name,"window is too small",metrics)
    assert metrics["window_controls_visible"], (name, "native window controls must be visible", metrics)
    assert metrics["titlebar_brand_visible"], (name, "titlebar brand missing", metrics)
    expected_scope = flags[flags.index("--ui-scope") + 1] if "--ui-scope" in flags else "ready"
    assert metrics["queue_scope"] == expected_scope, (name, "unexpected photo category", metrics)
    assert metrics["frames"]["conversion-directions"]["y"] >= 28, (name, "direction control overlaps titlebar", metrics)
    empty = "--ui-fixtures" in flags and flags[flags.index("--ui-fixtures") + 1] == "0" or name == "cleared"
    assert (metrics["source_button_title"] == "添加照片") == empty, (name,"source summary missing",metrics)
    assert "source-summary" not in metrics["frames"], (name,"redundant source/output row remains",metrics)
    for control in ("output-control","queue-more-menu"):
        frame = metrics["frames"][control]
        assert frame["x"] >= 0 and frame["x"] + frame["width"] <= metrics["frame_width"] + 1, (name,control,"clipped",metrics)
    if "--ui-output-menu" in flags:
        assert metrics["output_menu_selected"] == "folder" and "更改文件夹…" in metrics["output_menu_titles"],metrics
        assert metrics["output_destination"] == ("photos" if "--ui-output-switch" in flags else "folder"),metrics
        assert ("共享菜单" in metrics["output_menu_titles"]) == ("--direction" in flags),metrics
    if name == "cleared":
        assert metrics["row_count"] == 0 and metrics["output_path"].endswith("/Live Photos") and not metrics["clear_enabled"],metrics
    if name == "clear-busy":
        assert metrics["row_count"] == 24 and not metrics["clear_enabled"] and not metrics["source_button_enabled"],metrics
    expected_color = metrics["system_accent_color"]
    assert len(metrics["source_button_color"]) == len(expected_color) == 3 and all(abs(actual - expected) < 0.005 for actual, expected in zip(metrics["source_button_color"], expected_color)), (name, "system orange accent mismatch", metrics)
    assert abs(metrics["frames"]["add-button"]["height"] - metrics["frames"]["settings-button"]["height"]) < 2, (name, "source button size mismatch", metrics)
    if name == "source-menu":
        assert metrics["source_menu_opened"] and metrics["source_menu_titles"] == ["继续添加照片或文件夹…", "继续从系统图库添加…"], metrics
    if name in ("wide","compact","empty","cleared"):
        assert "task-feedback" not in metrics["frames"], (name,"routine footer status remains",metrics)
    if name in ("compact-progress","error","clear-busy"):
        assert "task-feedback" in metrics["frames"], (name,"progress/error feedback disappeared",metrics)
    assert metrics["primary_visible"], (name, "primary action outside content", metrics)
    assert metrics["queue_visible"], (name, "queue outside content", metrics)
    assert metrics["queue_height"] >= 150, (name, "queue cannot show three compact rows", metrics)
    for table in metrics["native_tables"]:
        assert not table["horizontal_overflow"], (name, "native list has horizontal overflow", table)
    for check in metrics["resize_checks"]:
        assert check["frame_width"] >= check["minimum_width"] and check["frame_height"] >= check["minimum_height"], (name,"resize bypassed minimum size",check)
        assert check["primary_visible"] and check["queue_visible"], (name, "resize clipping", check)
        assert check["selection_preserved"] and check["settings_preserved"], (name, "resize lost state", check)
        assert all(table["selected_row"] == 0 for table in check["native_tables"]), (name, "native selection lost", check)
        if "--ui-expand-settings" in flags:
            assert check["settings_sheet_visible"] and not check["inspector_visible"], (name, "duplicated settings", check)
    if name == "wide":
        assert not metrics["inspector_visible"] and metrics["queue_width"] >= 1000, metrics
        assert metrics["frames"]["queue-scopes"]["x"] < 100, (name,"photo toolbar is not aligned with the gallery",metrics)
        assert metrics["queue_height"] >= 490, (name,"compact toolbar must leave more photo space",metrics)
    if name == "compact-settings":
        assert metrics["settings_sheet_visible"], metrics
    if name == "compact-cover":
        assert metrics["settings_sheet_visible"], metrics
        assert 0 < metrics["sheet_width"] <= metrics["frame_width"], (name, "cover sheet wider than window", metrics)
    if name.startswith("library"):
        library = metrics["library_picker"]
        assert library["width"] >= 640 and library["controls_visible"], (name,"library controls clipped",metrics)
        assert library["empty_visible"] == (name == "library-empty"), metrics
        if name == "library-empty":
            assert library["empty_centered"], (name,"empty state is not centered in the photo browser",metrics)
        else:
            assert library["first_row_visible"], (name,"first photo row is clipped",metrics)
        assert library["primary_enabled"] == ("--ui-library-selected" in flags and "--ui-library-switch-version" not in flags), metrics
        if "--ui-library-selected" in flags and "--ui-library-switch-version" not in flags:
            assert library["selected"] == 3 and "3" in library["primary_title"], metrics
        assert not library["grid_overflow"], (name,"thumbnail grid overflows horizontally",metrics)
        assert library["browser_height"] >= 300, (name,"library chrome takes too much space",metrics)
        if "--ui-library-list" not in flags and name != "library-empty":
            assert library["visible_thumbnails"] >= 8, (name,"fewer than two rows of thumbnails",metrics)
        assert library["mode"] == ("list" if "--ui-library-list" in flags else "thumbnails"), metrics
        assert library["selected"] == library["table_selected"] == library["grid_selected"], (name,"grid/list selection differs",metrics)
        if name == "library-switch-version":
            assert library["selected"] == 0 and library["primary_title"] == "读取所选", metrics
    if name == "recent":
        assert metrics["recent_actions_visible"], (name,"recent result actions clipped",metrics)
    if name == "filtered-preview":
        assert "preview-photo" not in metrics["frames"], (name, "preview shows a photo outside the active category", metrics)
    if "Unable to simultaneously satisfy constraints" in result.stderr:
        raise AssertionError((name, result.stderr))
    results.append({"name": name, "queue_height": metrics["queue_height"], "queue_width": metrics["queue_width"], "resize_checks": len(metrics["resize_checks"])})
    print("PASS", name, "queue", metrics["queue_width"], "×", metrics["queue_height"], flush=True)
(OUTPUT / "results.json").write_text(json.dumps(results, indent=2))
print("PASS", len(results), "native SwiftUI scenarios; screenshots in", OUTPUT)
