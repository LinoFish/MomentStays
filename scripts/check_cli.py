#!/usr/bin/env python3
"""CLI smoke test: scanning, batch reporting, resume, verify, source protection."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.cli import digest, native, native_binary
from tests.test_formats import motion


def command(*args):
    p = subprocess.run([sys.executable, "-m", "motionlive", *map(str, args)], cwd=ROOT,
                       text=True, capture_output=True, timeout=300)
    return p, json.loads(p.stdout) if p.stdout else None


binary = native_binary()
with tempfile.TemporaryDirectory(prefix="motionlive-cli-") as folder:
    root = Path(folder)
    source, output = root / "input", root / "output"
    source.mkdir()
    fixture = native(binary, {"mode": "fixture", "folder": str(source)})
    still, movie = (source / "still.jpg").read_bytes(), (source / "clip.mp4").read_bytes()
    (source / "中文名称.jpg").write_bytes(motion(still, str(fixture["cover_us"]), movie=movie))
    (source / "no-time.jpg").write_bytes(motion(still, None, movie=movie))
    hashes = {p.name: digest(p) for p in source.iterdir()}
    p, scan = command("scan", source)
    assert p.returncode == 0 and len(scan) == 3, (p.stdout, p.stderr)
    p, result = command("convert", source, "-o", output)
    assert p.returncode == 1 and result["counts"] == {"converted": 1, "resumed": 0, "skipped": 1, "failed": 1}, (p.stdout, p.stderr)
    p, result = command("convert", source, "-o", output)
    assert p.returncode == 1 and result["counts"]["resumed"] == 1, (p.stdout, p.stderr)
    p, result = command("verify", output / "中文名称.jpg.livephoto")
    assert p.returncode == 0 and result["status"] == "verified", (p.stdout, p.stderr)
    p, _ = command("convert", source, "-o", source / "nested-output")
    assert p.returncode == 1 and not (source / "nested-output").exists()
    assert {p.name: digest(p) for p in source.iterdir()} == hashes
print("PASS 6 CLI checks (scan, batch counts, resume, verify, output guard, source hashes)")
