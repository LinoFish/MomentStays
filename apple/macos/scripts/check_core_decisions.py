#!/usr/bin/env python3
"""Parity of the Swift UI decisions with the shared Rust core.

Builds apple/macos/tests/CoreDecisionParityTests.swift with the production
Swift sources and the Rust bridge, then runs it on deterministic synthetic
corpora (filename capture time, action blocking reason, queue state, Live Photo still
selection, input selection and output validation, per-track sample facts). macOS
only; no photo-library access.

Usage: python3 apple/macos/scripts/check_core_decisions.py [--skip-build]
"""
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
native = ["RustCore", "MotionLive", "PhotoFormat", "BatchEngine", "CoverCandidates", "AndroidExport", "AndroidTransfer"]
sources = [str(ROOT / "apple" / "shared" / (name + ".swift")) for name in native]
sources += [str(ROOT / "apple" / "macos" / "Sources" / (name + ".swift")) for name in
            ["ConversionSettings", "PhotoTimeRepair", "ConversionService", "WorkspaceModel", "DeliveryModels", "PhotoLibraryService"]]
sources.append(str(ROOT / "apple/shared/tests/TestFixtures.swift"))
sources.append(str(ROOT / "apple/macos/tests/CoreDecisionParityTests.swift"))
binary = ROOT / ".build/gui/core-decision-parity"
binary.parent.mkdir(parents=True, exist_ok=True)
if "--skip-build" not in sys.argv:
    subprocess.run(["bash", "-c", 'source apple/macos/scripts/rust_bridge.sh x86_64; exec "$@" "${rust_bridge_flags[@]}"', "bash",
                    "xcrun", "swiftc", "-parse-as-library", "-D", "MOTIONLIVE_LIBRARY", "-O", "-target", "x86_64-apple-macosx12.0",
                    "-module-cache-path", "/private/tmp/motionlive-swift-parity-cache", *sources, "-o", str(binary)],
                   cwd=ROOT, check=True)
sys.exit(subprocess.run([str(binary), str(ROOT)], cwd=ROOT, timeout=1800).returncode)
