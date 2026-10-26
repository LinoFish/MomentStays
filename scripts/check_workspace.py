#!/usr/bin/env python3
"""Exercise task state + real bidirectional engine through the new workspace model.

Only synthetic media in a temporary directory is used. No photo-library access.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import motion

native = ["MotionLive", "PhotoFormat", "PhotoHEIF", "PhotoHDR", "BatchEngine", "HEIFMetadata", "AndroidHDR", "AndroidExport"]
sources = [str(ROOT / "native" / (name + ".swift")) for name in native]
sources += [str(ROOT / "gui" / (name + ".swift")) for name in ["ConversionSettings", "ConversionService", "WorkspaceModel", "DeliveryModels", "PhotoLibraryService"]]
sources.append(str(ROOT / "tests/WorkspaceModelTests.swift"))
binary = ROOT / ".build/gui/workspace-tests"
binary.parent.mkdir(parents=True, exist_ok=True)
if "--skip-build" not in sys.argv:
    subprocess.run(["xcrun", "swiftc", "-parse-as-library", "-D", "MOTIONLIVE_LIBRARY", "-Onone", "-target", "x86_64-apple-macosx12.0",
        "-module-cache-path", "/private/tmp/motionlive-swift-model-cache", *sources, "-o", str(binary)], check=True)
driver = ROOT / ".build/gui/native-tests"
if not driver.exists():
    subprocess.run(["bash", "scripts/build_gui_tests.sh"], cwd=ROOT, check=True)
with tempfile.TemporaryDirectory(prefix="motionlive-workspace-fixture-") as folder:
    root = Path(folder)
    fixture = subprocess.run([str(driver)], input=json.dumps({"mode": "fixture", "folder": str(root)}), text=True, capture_output=True, timeout=60)
    assert fixture.returncode == 0, (fixture.stdout, fixture.stderr)
    stamp = json.loads(fixture.stdout)["cover_us"]
    source = root / "workspace-sample.jpg"
    source.write_bytes(motion((root / "still.jpg").read_bytes(), str(stamp), movie=(root / "clip.mp4").read_bytes()))
    subprocess.run([str(binary), str(source)], check=True, timeout=180)
