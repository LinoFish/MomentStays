#!/usr/bin/env python3
"""Cross-check the self-contained Swift GUI engine against the original format tests."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.jpeg import image_hash,mpf_images
from motionlive.photo import ultra_from_apple
from tests.test_formats import motion,jpeg

BINARY = ROOT / ".build/gui/native-tests"


def call(request, ok=True):
    p = subprocess.run([str(BINARY)],input=json.dumps(request),text=True,capture_output=True,timeout=300)
    try:
        result = json.loads(p.stdout)
    except ValueError:
        raise AssertionError((p.returncode,p.stdout,p.stderr))
    if ok:
        assert p.returncode == 0 and "error" not in result, result
    else:
        assert p.returncode != 0 and "error" in result, result
    return result


count = 0
with tempfile.TemporaryDirectory(prefix="motionlive-swift-gui-") as folder:
    root = Path(folder)
    # Namespace handling, progressive JPEG and MPF offsets are cross-checked by Python.
    ultra = ultra_from_apple(jpeg(),jpeg(),2)
    for name,data,stamp in (("modern",motion(),400000),("elements",motion(element=True),400000),
                            ("old",motion(timestamp="1234567",legacy=True),1234567),
                            ("missing",motion(timestamp=None),None),("ultra",motion(ultra),400000)):
        src,out = root/(name+".jpg"),root/(name+"-out.jpg")
        src.write_bytes(data)
        result = call({"mode":"format","input":str(src),"output":str(out)})
        assert result["timestamp_us"] == stamp,result
        assert result["coding_hash"] == result["output_coding_hash"] == image_hash(data)
        assert mpf_images(out.read_bytes())[1:] == mpf_images(data)[1:]
        count += 1
    bad = root/"invalid.jpg"
    bad.write_bytes(motion(timestamp="NaN"))
    call({"mode":"format","input":str(bad)},ok=False); count += 1
    for name,settings in (("regular",{}),("offset",{"start_us":2000000}),("vfr",{"variable":True}),
                          ("fractional",{"fractional":True}),("audio",{"audio":True}),("legacy",{"legacy_hdr":True})):
        src = root/name
        fixture = call({"mode":"fixture","folder":str(src),**settings})
        still,video = (src/"still.jpg").read_bytes(),(src/"clip.mp4").read_bytes()
        (src/"中文动态.jpg").write_bytes(motion(still,str(fixture["cover_us"]),movie=video))
        (src/"missing.jpg").write_bytes(motion(still,None,movie=video))
        output = root/(name+"-out")
        result = call({"mode":"batch","input":str(src),"output":str(output)})
        assert result["counts"] == {"converted":1,"failed":1,"skipped":1,"resumed":0},result
        manifest = json.loads((output/"中文动态.jpg.livephoto/manifest.json").read_text())
        expected = (fixture["cover_us"] - settings.get("start_us",0))/1e6
        assert abs(manifest["timing"]["cover_seconds"] - expected) < 2e-6
        assert manifest["primary_coding_sha256"] == image_hash(still)
        if name == "legacy":
            assert manifest["hdr"]["status"] == "converted",manifest["hdr"]
        if name == "audio":
            assert len(manifest["timing"]["tracks"]) == 2
        again = call({"mode":"batch","input":str(src),"output":str(output)})
        assert again["counts"]["resumed"] == 1,again
        scan = call({"mode":"scan","input":str(src)})
        assert sum(row["motion"] for row in scan["rows"]) == 2,scan
        call({"mode":"batch","input":str(src),"output":str(src/"unsafe-output")},ok=False)
        count += 4
        print("PASS native GUI engine",name)
        if name == "regular":
            # Existing Ultra HDR must bypass the HDR conversion path in the GUI too.
            ultra_image = ultra_from_apple(still,still,2)
            ultra_source = src / "ultra.jpg"
            ultra_source.write_bytes(motion(ultra_image,str(fixture["cover_us"]),movie=video))
            one = call({"mode":"batch","input":str(ultra_source),"output":str(root/"ultra-out")})
            assert one["counts"]["converted"] == 1,one
            pair = root/"ultra-out/ultra.jpg.livephoto"
            manifest = json.loads((pair/"manifest.json").read_text())
            assert manifest["hdr"]["input"] == "ultra-hdr" and manifest["hdr"]["status"] == "preserved"
            assert mpf_images((pair/manifest["output_files"]["still"]).read_bytes())[1:] == mpf_images(ultra_image)[1:]
            count += 1
    # Stop between files; the successful first pair and report remain valid.
    stopped = call({"mode":"batch","input":str(root/"regular"),"output":str(root/"stop-out"),"cancel_after":1})
    assert stopped["cancelled"] and sum(stopped["counts"].values()) == 1,stopped
    count += 1
print("PASS",count,"native GUI engine checks")
