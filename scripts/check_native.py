#!/usr/bin/env python3
"""End-to-end tests using locally generated media; never opens the Photos library."""
import argparse
from argparse import Namespace
import json
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from motionlive.cli import convert_one, implementation_id, native, native_binary, verify_pair
from motionlive.jpeg import image_hash, mpf_images
from motionlive.photo import parse, ultra_from_apple
from tests.test_formats import motion


def check(binary):
    args = Namespace(missing_time="error", legacy_hdr="auto")
    impl = implementation_id()
    count = 0
    with tempfile.TemporaryDirectory(prefix="motionlive-e2e-") as tmp:
        root = Path(tmp)
        for name, options in (("regular", {}), ("offset", {"start_us": 2_000_000}),
                              ("variable", {"variable": True}), ("fractional", {"fractional": True}),
                              ("audio", {"audio": True}),
                              ("legacy", {"legacy_hdr": True})):
            fixture = root / name
            result = native(binary, {"mode": "fixture", "folder": str(fixture), **options})
            still, video = (fixture / "still.jpg").read_bytes(), (fixture / "clip.mp4").read_bytes()
            timestamp = result["cover_us"]
            source = fixture / "motion.jpg"
            source.write_bytes(motion(still, str(timestamp), movie=video))
            target = root / (name + ".livephoto")
            converted = convert_one(source, target, binary, args, impl)
            manifest = verify_pair(target)
            assert converted["status"] == "converted"
            expected_time = (timestamp - options.get("start_us", 0)) / 1e6
            assert abs(manifest["timing"]["cover_seconds"] - expected_time) < 2e-6
            assert abs(manifest["timing"]["requested_seconds"] - timestamp / 1e6) < 2e-6
            assert manifest["primary_coding_sha256"] == image_hash(still)
            assert all(t["packets"] > 0 for t in manifest["timing"]["tracks"])
            if name == "audio":
                assert len(manifest["timing"]["tracks"]) == 2
            if name == "legacy":
                assert manifest["hdr"]["status"] == "converted", manifest["hdr"]
                assert len(mpf_images((target / "photo.jpg").read_bytes())) == 2
            else:
                assert manifest["hdr"]["status"] == "unchanged", manifest["hdr"]
                assert len(mpf_images((target / "photo.jpg").read_bytes())) == 0
            assert convert_one(source, target, binary, args, impl)["status"] == "resumed"
            count += 1
            print("PASS", name, json.dumps(manifest["timing"], ensure_ascii=False))
            if name == "regular":
                # Ultra HDR bypasses HDR conversion; compressed map survives exactly.
                ultra = ultra_from_apple(still, still, 2.0)
                source_ultra = fixture / "ultra.jpg"
                source_ultra.write_bytes(motion(ultra, str(timestamp), movie=video))
                ultra_target = root / "ultra.livephoto"
                convert_one(source_ultra, ultra_target, binary, args, impl)
                m = verify_pair(ultra_target)
                assert m["hdr"]["input"] == "ultra-hdr" and m["hdr"]["status"] == "preserved"
                assert mpf_images((ultra_target / "photo.jpg").read_bytes())[1] == mpf_images(ultra)[1]
                count += 1
                # Strict missing/out-of-range timestamps must not publish a pair.
                for label, stamp in (("missing", None), ("outside", "999000000")):
                    bad_source = fixture / (label + ".jpg")
                    bad_source.write_bytes(motion(still, stamp, movie=video))
                    bad_target = root / (label + ".livephoto")
                    try:
                        convert_one(bad_source, bad_target, binary, args, impl)
                    except (ValueError, RuntimeError):
                        assert not bad_target.exists()
                    else:
                        raise AssertionError("Invalid timestamp was accepted")
                    count += 1
                # Fallback is available only through explicit user policy.
                mid_target = root / "midpoint.livephoto"
                missing = fixture / "missing.jpg"
                mid_args = Namespace(missing_time="midpoint", legacy_hdr="auto")
                convert_one(missing, mid_target, binary, mid_args, impl)
                assert verify_pair(mid_target)["timing"]["timing_policy"] == "explicit-midpoint"
                count += 1
                (target / "photo.jpg").write_bytes(b"corrupted")
                try:
                    verify_pair(target)
                except ValueError:
                    count += 1
                else:
                    raise AssertionError("Output corruption was not detected")
                # Both interpretations valid but different: auto must reject ambiguity.
                ambiguous = root / "ambiguous"
                times = native(binary, {"mode": "fixture", "folder": str(ambiguous), "start_us": 200000})
                amb_source = ambiguous / "motion.jpg"
                amb_source.write_bytes(motion((ambiguous / "still.jpg").read_bytes(), str(times["cover_us"]),
                                              movie=(ambiguous / "clip.mp4").read_bytes()))
                try:
                    convert_one(amb_source, root / "amb-auto.livephoto", binary, args, impl)
                except RuntimeError as e:
                    assert "Ambiguous" in str(e), e
                    count += 1
                else:
                    raise AssertionError("Ambiguous source timestamp was guessed")
                for domain, expected in (("presentation", 0.4), ("media", 0.6)):
                    explicit = Namespace(missing_time="error", legacy_hdr="auto", time_domain=domain)
                    folder = root / ("amb-" + domain + ".livephoto")
                    convert_one(amb_source, folder, binary, explicit, impl)
                    assert abs(verify_pair(folder)["timing"]["cover_seconds"] - expected) < 2e-6
                    count += 1
        print("PASS %d native integration checks" % count)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path)
    args = parser.parse_args()
    check(args.binary.resolve() if args.binary else native_binary())
