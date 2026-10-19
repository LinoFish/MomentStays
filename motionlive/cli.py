import argparse
import base64
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

from . import __version__
from .jpeg import FormatError, image_hash, mpf_images
from .photo import parse, live_still, ultra_from_apple, legacy_color_profile
from .hdr import legacy_apple

ROOT = Path(__file__).resolve().parent.parent


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def implementation_id():
    h = hashlib.sha256()
    for path in sorted((ROOT / "motionlive").glob("*.py")) + [ROOT / "native/MotionLive.swift"]:
        h.update(path.read_bytes())
    return h.hexdigest()


def native_binary():
    if sys.platform != "darwin":
        raise RuntimeError("Conversion requires macOS 12 or later")
    source = ROOT / "native/MotionLive.swift"
    build = ROOT / ".build"
    build.mkdir(exist_ok=True)
    fingerprint = hashlib.sha256(source.read_bytes() + platform.machine().encode()).hexdigest()[:16]
    target = build / ("motionlive-native-" + fingerprint)
    if target.is_file():
        return target
    print("首次运行：编译 macOS 12 原生组件…", file=sys.stderr)
    with tempfile.TemporaryDirectory(prefix="motionlive-build-", dir=str(build)) as temp:
        executable = Path(temp) / "native"
        cache = build / "swift-cache"
        proc = subprocess.run(["xcrun", "swiftc", "-parse-as-library", "-O", "-target", platform.machine() + "-apple-macosx12.0",
            "-module-cache-path", str(cache), str(source), "-o", str(executable)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=300)
        if proc.returncode:
            raise RuntimeError("Swift 编译失败；需要 Xcode Command Line Tools。\n" + proc.stderr)
        os.replace(executable, target)
    return target


def native(binary, request):
    proc = subprocess.run([str(binary)], input=json.dumps(request), text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
    try:
        result = json.loads(proc.stdout)
    except ValueError as e:
        raise RuntimeError("原生组件未返回 JSON: " + proc.stderr[-2000:]) from e
    if proc.returncode or "error" in result:
        raise RuntimeError(result.get("error", proc.stderr[-2000:]))
    return result


def publish(stage, target):
    # macOS renamex_np(RENAME_EXCL): one atomic directory publication, no overwrite.
    lib = ctypes.CDLL(None, use_errno=True)
    rename = lib.renamex_np
    rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(os.fsencode(stage), os.fsencode(target), 0x00000004):
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(target))


def verify_pair(folder):
    folder = Path(folder)
    manifest = json.loads((folder / "manifest.json").read_text())
    if (not isinstance(manifest, dict) or not isinstance(manifest.get("output_sha256"), dict)
            or set(manifest["output_sha256"]) != {"photo.jpg", "photo.mov"}
            or not isinstance(manifest.get("timing"), dict) or not isinstance(manifest.get("hdr"), dict)):
        raise FormatError("Missing or malformed conversion manifest")
    for name, expected in manifest["output_sha256"].items():
        if Path(name).name != name or digest(folder / name) != expected:
            raise FormatError("Output integrity check failed: " + name)
    return manifest


def convert_one(source, target, binary, args, impl):
    source_hash = digest(source)
    options = {"missing_time": args.missing_time, "legacy_hdr": args.legacy_hdr,
               "time_domain": getattr(args, "time_domain", "auto")}
    if target.exists():
        manifest = verify_pair(target)
        if (manifest.get("source_sha256") == source_hash and manifest.get("implementation") == impl
                and manifest.get("options") == options):
            return {"status": "resumed", "source": str(source), "output": str(target)}
        raise FileExistsError("输出已存在且与当前输入/设置不一致；请选择新的输出目录: " + str(target))
    photo = parse(source.read_bytes())
    if photo.timestamp_us is None and args.missing_time == "error":
        raise FormatError("原片没有有效封面时间戳；不自动使用视频中点")
    target.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".motionlive-", dir=str(target.parent)))
    try:
        with tempfile.TemporaryDirectory(prefix="motionlive-work-") as work:
            work = Path(work)
            image = photo.still
            video_path = work / "source.mp4"
            video_path.write_bytes(photo.video)
            hdr = {"input": photo.hdr, "status": "preserved", "reason": "Ultra HDR bytes retained"}
            warnings = list(photo.warnings)
            if photo.hdr != "ultra-hdr":
                if args.legacy_hdr == "auto":
                    try:
                        legacy, reason = legacy_apple(image)
                        result = {"status": "unchanged", "reason": reason}
                        if legacy:
                            (work / "old-gain.jpg").write_bytes(legacy["gain"])
                            result = native(binary, {"mode": "legacy-gain", "input": str(work / "old-gain.jpg"),
                                "gain_output": str(work / "gain.jpg"), "stops": legacy["stops"]})
                        icc = result.pop("srgb_icc_base64", None)
                        hdr.update(result)
                        if result["status"] == "converted":
                            tagged = legacy_color_profile(image, base64.b64decode(icc, validate=True))
                            image = ultra_from_apple(tagged, (work / "gain.jpg").read_bytes(), result["stops"])
                            hdr["output"] = "ultra-hdr"
                    except (RuntimeError, ValueError) as e:
                        hdr.update(status="unchanged", reason="Legacy HDR conversion could not be completed")
                        warnings.append("Legacy HDR conversion: " + str(e))
                else:
                    hdr.update(status="unchanged", reason="Legacy HDR conversion disabled")
            identifier = str(uuid.uuid4()).upper()
            still = live_still(image, identifier)
            if image_hash(photo.still) != image_hash(still):
                raise FormatError("Static JPEG compressed coding data changed")
            (stage / "photo.jpg").write_bytes(still)
            timing = native(binary, {"mode": "movie", "input": str(video_path),
                "output": str(stage / "photo.mov"), "still": str(stage / "photo.jpg"), "identifier": identifier,
                "timestamp_us": photo.timestamp_us, "missing_time": args.missing_time,
                "time_domain": options["time_domain"]})
            stat = source.stat()
            for name in ("photo.jpg", "photo.mov"):
                os.utime(stage / name, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            manifest = {"version": __version__, "implementation": impl, "source": str(source),
                "source_sha256": source_hash, "identifier": identifier,
                "options": options,
                "source_timestamp_us": photo.timestamp_us, "source_timestamp_tag": photo.timestamp_tag,
                "timing": timing, "hdr": hdr, "warnings": warnings,
                "primary_coding_sha256": image_hash(still),
                "auxiliary_sha256": [hashlib.sha256(x).hexdigest() for x in mpf_images(still)[1:]],
                "output_sha256": {name: digest(stage / name) for name in ("photo.jpg", "photo.mov")}}
            (stage / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
            verify_pair(stage)
            # Catch an input changed by another application during conversion.
            if digest(source) != source_hash:
                raise FormatError("Source changed during conversion; result not published")
            publish(stage, target)
            return {"status": "converted", "source": str(source), "output": str(target),
                    "timing": timing, "hdr": hdr, "warnings": warnings}
    finally:
        if stage.exists():
            shutil.rmtree(stage)


def inputs(path):
    if path.is_file():
        if path.suffix.lower() not in (".jpg", ".jpeg"):
            raise ValueError("目前输入支持 Google Motion Photo JPEG (.jpg/.jpeg)")
        return [path]
    if not path.is_dir():
        raise ValueError("输入不存在: " + str(path))
    return sorted(p for p in path.rglob("*") if p.is_file() and not p.is_symlink()
                  and p.suffix.lower() in (".jpg", ".jpeg"))


def main(argv=None):
    parser = argparse.ArgumentParser(description="Motion-Live — Google Motion Photo → Apple Live Photo (macOS 12+)")
    parser.add_argument("--version", action="version", version=__version__)
    commands = parser.add_subparsers(dest="command", required=True)
    scan = commands.add_parser("scan", help="只读扫描：封面时间、HDR、格式错误")
    scan.add_argument("input", type=Path)
    convert = commands.add_parser("convert", help="递归批量转换，自动跳过已验证的相同结果")
    convert.add_argument("input", type=Path)
    convert.add_argument("-o", "--output", required=True, type=Path)
    convert.add_argument("--missing-time", choices=("error", "midpoint"), default="error",
                         help="默认缺少封面时间时报错；midpoint 必须主动选择")
    convert.add_argument("--legacy-hdr", choices=("auto", "preserve"), default="auto",
                         help="auto 尝试转换已存在的旧 Apple gain map；普通 SDR 不生成 HDR")
    convert.add_argument("--time-domain", choices=("auto", "media", "presentation"), default="auto",
                         help="默认检查样本与 edit list 时间轴，存在歧义时报错")
    verify = commands.add_parser("verify", help="校验输出文件与转换时的完整校验结果一致")
    verify.add_argument("folder", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "verify":
            manifest = verify_pair(args.folder)
            print(json.dumps({"status": "verified", "timing": manifest["timing"], "hdr": manifest["hdr"]}, ensure_ascii=False, indent=2))
            return 0
        source = args.input.expanduser().resolve()
        files = inputs(source)
        if args.command == "scan":
            results = []
            for file in files:
                try:
                    photo = parse(file.read_bytes())
                    results.append({"source": str(file), "status": "motion-photo", "hdr": photo.hdr,
                        "timestamp_us": photo.timestamp_us, "timestamp_tag": photo.timestamp_tag,
                        "warnings": photo.warnings})
                except (ValueError, OSError) as e:
                    results.append({"source": str(file), "status": "not-convertible", "reason": str(e)})
            print(json.dumps(results, ensure_ascii=False, indent=2))
            return 0
        destination = args.output.expanduser().resolve()
        input_root = source if source.is_dir() else source.parent
        if destination == input_root or input_root in destination.parents:
            raise ValueError("输出目录必须位于输入目录之外")
        destination.mkdir(parents=True, exist_ok=True)
        binary, impl = native_binary(), implementation_id()
        results = []
        for index, file in enumerate(files, 1):
            relative = file.relative_to(input_root)
            target = destination / relative.parent / (relative.name + ".livephoto")
            try:
                result = convert_one(file, target, binary, args, impl)
            except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as e:
                # A regular JPEG is a skip; broken motion photos are failures.
                status = "skipped" if str(e) == "Not a marked Google Motion Photo" else "failed"
                result = {"status": status, "source": str(file), "error": str(e)}
            results.append(result)
            print("[%d/%d] %s %s" % (index, len(files), result["status"], relative), file=sys.stderr)
        counts = {status: sum(r["status"] == status for r in results) for status in ("converted", "resumed", "skipped", "failed")}
        report = destination / ("report-" + time.strftime("%Y%m%d-%H%M%S") + "-" + uuid.uuid4().hex[:6] + ".json")
        report.write_text(json.dumps({"counts": counts, "files": results}, indent=2, ensure_ascii=False) + "\n")
        print(json.dumps({"counts": counts, "report": str(report)}, ensure_ascii=False, indent=2))
        return 1 if counts["failed"] else 0
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as e:
        print("错误: " + str(e), file=sys.stderr)
        return 1
