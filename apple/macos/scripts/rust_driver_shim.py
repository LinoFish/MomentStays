#!/usr/bin/env python3
"""Stand-in for .build/gui/native-tests that runs the Rust engine.

Every request goes to core/target/release/momentstays-driver, except
`fixture` (test-input generation with AVFoundation's H.264 encoder), which
goes to the Swift driver saved next to this shim. Each request is logged to
.build/rust-shim.log with the engine that answered and whether the Rust
engine asked for host codecs ("需要原生处理"), so failures caused by steps
that only the app's native callbacks can do are easy to tell apart.

Installed and removed by run_checks_with_rust.sh; not used by the app.
"""
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
RUST = Path(os.environ.get("MOMENTSTAYS_RUST_DRIVER", ROOT / "core/target/release/momentstays-driver"))
SWIFT = Path(os.environ.get("MOMENTSTAYS_SWIFT_DRIVER", ROOT / ".build/gui/native-tests.swift"))
LOG = ROOT / ".build/rust-shim.log"


def main():
    request = sys.stdin.read()
    try:
        mode = json.loads(request).get("mode")
    except ValueError:
        mode = None
    engine = SWIFT if mode == "fixture" else RUST
    p = subprocess.run([str(engine)], input=request, text=True, capture_output=True)
    with LOG.open("a", encoding="utf-8") as log:
        log.write(json.dumps({
            "mode": mode,
            "engine": "swift" if engine == SWIFT else "rust",
            "exit": p.returncode,
            "needs_native": "需要原生处理" in p.stdout,
        }, ensure_ascii=False) + "\n")
    sys.stdout.write(p.stdout)
    sys.stderr.write(p.stderr)
    sys.exit(p.returncode)


if __name__ == "__main__":
    main()
