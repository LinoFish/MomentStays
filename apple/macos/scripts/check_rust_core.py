#!/usr/bin/env python3
"""ABI parity check: Rust CLI vs the production Swift-to-Rust bridge.

Diffs `momentstays-core inspect` against the bridge's `format` command
(accept/reject, error text, timestamp_us, ultra, coding_hash) on generated
fixtures and on any real sample folder passed with `--samples`. macOS only: build
the bridge driver first with apple/macos/scripts/build_backend.sh.

Usage:
  python3 apple/macos/scripts/check_rust_core.py --swift .build/backend/native-tests [--samples DIR]
"""
import argparse, json, os, struct, subprocess, sys, tempfile
from pathlib import Path
from testkit.fixtures import jpeg, motion

ROOT = str(Path(__file__).resolve().parents[3])


def box(kind, payload):
    return struct.pack(">I", len(payload) + 8) + kind + payload


def fixtures():
    long_video = box(b"ftyp", b"isom\0\0\0\0isom") + box(b"moov", b"\0" * 32) + box(b"mdat", b"\xff\xd9" * 400)
    yield "attr", motion()
    yield "element", motion(element=True)
    yield "legacy-microvideo", motion(legacy=True)
    yield "no-container", motion(container=False)
    yield "no-time", motion(timestamp=None)
    yield "negative-time", motion(timestamp="-1")
    yield "bad-time", motion(timestamp="12x")
    yield "zero-time", motion(timestamp="0")
    yield "big-time", motion(timestamp="9223372036854775807")
    yield "long-video", motion(movie=long_video)
    yield "not-jpeg", b"hello world, not a jpeg at all"
    yield "plain-jpeg", jpeg()
    good = motion()
    yield "truncated", good[:-5]
    yield "trailing-garbage", good + b"junk"
    yield "flag-zero", good.replace(b'MotionPhoto="1"', b'MotionPhoto="0"')
    yield "no-ftyp", good.replace(b"ftyp", b"ftyq")
    # vendor fixtures (Rust-only expectations are covered by cargo tests)


def run_rust(binary, path):
    out = subprocess.run([binary, "inspect", path], capture_output=True, text=True)
    return json.loads(out.stdout.strip().splitlines()[-1])


def swift_reference(driver, path):
    req = json.dumps({"mode": "format", "input": path})
    out = subprocess.run([driver], input=req, capture_output=True, text=True)
    v = json.loads(out.stdout.strip().splitlines()[-1])
    return {"ok": "error" not in v, **v}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--swift", required=True); ap.add_argument("--samples"); ap.add_argument("--binary")
    a = ap.parse_args()
    binary = a.binary or os.path.join(ROOT, "core", "target", "release", "momentstays-core")
    if not a.binary:
        subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=os.path.join(ROOT, "core"), check=True)
    failures, checked = [], 0
    with tempfile.TemporaryDirectory() as tmp:
        items = list(fixtures())
        if a.samples:
            for name in sorted(os.listdir(a.samples)):
                p = os.path.join(a.samples, name)
                if os.path.isfile(p):
                    items.append(("sample:" + name, open(p, "rb").read()))
        for name, data in items:
            path = os.path.join(tmp, name.replace("/", "_").replace(":", "_") + ".jpg")
            open(path, "wb").write(data)
            rust = run_rust(binary, path)
            sw = swift_reference(a.swift, path)
            checked += 1
            if rust["ok"] != sw["ok"]:
                failures.append((name, "swift-accept", rust, sw))
            elif rust["ok"]:
                for k in ("timestamp_us", "ultra", "coding_hash"):
                    if rust.get(k) != sw.get(k):
                        failures.append((name, "swift-" + k, rust.get(k), sw.get(k)))
            elif rust.get("error") != sw.get("error"):
                failures.append((name, "swift-error-text", rust.get("error"), sw.get("error")))
    for f in failures:
        print("MISMATCH", *f)
    print(f"checked {checked} inputs, {len(failures)} mismatches")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
