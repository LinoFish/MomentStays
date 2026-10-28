#!/usr/bin/env python3
"""Parity check: Rust core (core/) vs the reference implementations.

Always: Rust `momentstays-core inspect` vs Python `motionlive.photo.parse` on
generated Google Motion Photo fixtures (accept/reject, timestamp, still/video bytes).
On macOS, if `--swift .build/backend/native-tests` (built by scripts/build_backend.sh) is given: also diff against the Swift
engine's `format` command (timestamp_us, ultra, coding_hash) on every fixture
and on any real sample folder passed with `--samples`.

Usage:
  python3 scripts/check_rust_core.py [--swift .build/backend/native-tests] [--samples DIR]
"""
import argparse, hashlib, json, os, struct, subprocess, sys, tempfile
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)
from tests.test_formats import jpeg, motion, video  # noqa: E402
from motionlive.jpeg import FormatError, app  # noqa: E402
from motionlive.photo import parse as py_parse  # noqa: E402


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


def py_reference(data):
    try:
        p = py_parse(data)
    except (FormatError, ValueError, Exception) as e:  # Python raises ET/struct errors too
        return {"ok": False, "error": str(e)}
    return {"ok": True, "timestamp_us": p.timestamp_us, "timestamp_tag": p.timestamp_tag,
            "still_sha256": hashlib.sha256(p.still).hexdigest(), "video_sha256": hashlib.sha256(p.video).hexdigest()}


def swift_reference(driver, path):
    req = json.dumps({"mode": "format", "input": path})
    out = subprocess.run([driver], input=req, capture_output=True, text=True)
    v = json.loads(out.stdout.strip().splitlines()[-1])
    return {"ok": "error" not in v, **v}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--swift"); ap.add_argument("--samples"); ap.add_argument("--binary")
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
            checked += 1
            if not name.startswith("sample:"):
                ref = py_reference(data)
                # Python reference is Google-only and keeps trailing bytes in the still;
                # compare acceptance always, values only when both accept.
                if rust["ok"] != ref["ok"]:
                    failures.append((name, "accept", rust, ref))
                elif rust["ok"]:
                    for k in ("timestamp_us", "timestamp_tag", "video_sha256", "still_sha256"):
                        if rust.get(k) != ref.get(k):
                            failures.append((name, k, rust.get(k), ref.get(k)))
            if a.swift:
                sw = swift_reference(a.swift, path)
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
