#!/usr/bin/env python3
"""Compare separately built Swift-reference and integrated backend drivers.

Drivers use tests/RustBridgeDriver.swift linked to each backend library.
No media is persisted outside the temporary fixture directory.
"""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from scripts.check_rust_core import fixtures
from tests.test_formats import jpeg, video
from tests.test_formats import motion
from motionlive.jpeg import app
from motionlive.photo import ultra_from_apple


def cases():
    yield from fixtures()
    still, movie = jpeg(), video()
    yield "ultra-hdr", motion(ultra_from_apple(still, still, 2))
    for orientation in [1, 3, 6, 8]:
        tiff = b"II" + struct.pack("<HIH", 42, 8, 1)
        tiff += struct.pack("<HHIHHI", 274, 3, 1, orientation, 0, 0)
        oriented = still[:2] + app(225, b"Exif\0\0" + tiff) + still[2:]
        yield f"orientation-{orientation}", motion(oriented)
    for first in ["v2_f12", "v3_f31_c", "v9_f1"]:
        private = b""
        if first.endswith("_c"):
            private = b"".join(struct.pack(">I", 9) + struct.pack("<I", t) + b"\0"
                               for t in [5, 1, 0x0306, 2, 3, 4])
            private += struct.pack(">I", len(private) + 4)
        tail = f"{first:<20}{'0:1299':<20}{'LIVE_' + str(len(movie) + len(private) + 20):<20}".encode()
        yield first, still + movie + private + tail
    # Bounded malformed inputs must retain failure semantics across the ABI.
    for size in [0, 1, 2, 7, 20, len(still) - 1]:
        yield f"cut-{size}", still[:size]


def run(driver, paths):
    p = subprocess.run([driver, *map(str, paths)], check=True, text=True, capture_output=True)
    return [json.loads(line) for line in p.stdout.splitlines()]


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit("usage: check_rust_bridge.py SWIFT_REFERENCE_DRIVER RUST_DRIVER [SAMPLE ...]")
    with tempfile.TemporaryDirectory(prefix="rust-bridge-parity-") as tmp:
        paths = []
        for i, (name, data) in enumerate(cases()):
            path = Path(tmp) / f"{i}-{name}.jpg"
            path.write_bytes(data)
            paths.append(path)
        paths.extend(Path(p) for p in sys.argv[3:])
        expected, actual = run(sys.argv[1], paths), run(sys.argv[2], paths)
        assert len(expected) == len(actual) == len(paths)
        failures = [p.name for p, a, b in zip(paths, expected, actual) if a != b]
        if failures:
            raise SystemExit("Bridge parity mismatch: " + ", ".join(failures))
        print(f"{len(paths)} inputs: exact still/video bytes, metadata, hashes and errors agree")
