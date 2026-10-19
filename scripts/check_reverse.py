#!/usr/bin/env python3
"""Round-trip real encoded packets and independently read Android JPEG output."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.photo import parse,xmp_packets,integer_unique
from motionlive.jpeg import image_hash,mpf_images,segments
from tests.test_formats import motion
BINARY = ROOT/'.build/gui/native-tests'

def call(**request):
    p = subprocess.run([str(BINARY)],input=json.dumps(request),text=True,capture_output=True,timeout=180)
    assert p.returncode == 0, (p.stdout,p.stderr)
    return json.loads(p.stdout)

def batch(source,output,**options):
    result = call(mode='batch',input=str(source),output=str(output),**options)
    assert result['counts']['failed'] == 0, result
    return result

with tempfile.TemporaryDirectory(prefix='motionlive-reverse-') as folder:
    root = Path(folder)
    for legacy in [False,True]:
        fixtures = root/('hdr' if legacy else 'sdr')
        info = call(mode='fixture',folder=str(fixtures),audio=True,legacy_hdr=legacy)
        still = (fixtures/'still.jpg').read_bytes()
        source = fixtures/'中文照片.MP.jpg'
        source.write_bytes(motion(still,str(info['cover_us']),movie=(fixtures/'clip.mp4').read_bytes()))
        forward = root/('apple-hdr' if legacy else 'apple-sdr')
        batch(source,forward,legacy_hdr=False)
        pair = forward/(source.name+'.livephoto')/source.name
        before = hashlib.sha256(pair.read_bytes()).hexdigest()
        for vendor in ['google','xiaomi','samsung','huawei','honor','oppo','vivo']:
            output = root/(vendor+str(legacy))
            r = batch(pair,output,direction='to-android',vendor=vendor)
            assert r['counts']['converted'] == 1,r
            manifest = json.loads(next(output.rglob('manifest.json')).read_text())
            file = next(output.rglob('*.MP.jpg'))
            # Python parser is independent of the Swift writer / read-back path.
            raw = file.read_bytes()
            maps = mpf_images(raw)
            end = sum(map(len,maps)) if maps else segments(raw)[-1].end
            # Vendor trailers are outside the standard MP4 item. Locate using
            # independently parsed XMP length, not the Swift vendor decoder.
            import xml.etree.ElementTree as ET
            ns = '{http://ns.google.com/photos/1.0/container/item/}'
            lengths = [int(node.attrib[ns+'Length']) for _,xml in xmp_packets(raw) for node in xml.iter() if node.attrib.get(ns+'Semantic') == 'MotionPhoto']
            assert len(lengths) == 1
            photo = parse(raw[:end + lengths[0]])
            assert abs(photo.timestamp_us-info['cover_us']) <= 1, (vendor,photo)
            assert image_hash(photo.still) == image_hash(still)
            assert (photo.hdr == "ultra-hdr") == legacy,(vendor,manifest['hdr'])
            assert len(mpf_images(photo.still)) == (2 if legacy else 0)
            if vendor == 'google':
                resumed = batch(pair,output,direction='to-android',vendor=vendor)
                assert resumed['counts']['resumed'] == 1,resumed
            print('PASS reverse',vendor,'HDR' if legacy else 'SDR')
        assert hashlib.sha256(pair.read_bytes()).hexdigest() == before
        if not legacy:
            output = root/'without-json'
            batch(pair,output,direction='to-android',export_data_files=False)
            assert not list(output.rglob('*.json'))
            files = list(output.rglob('*.MP.jpg')); assert len(files) == 1
            # A missing pair must fail instead of guessing by filename.
            isolated = root/'isolated'; isolated.mkdir(); missing = isolated/'still.jpg'; missing.write_bytes(pair.read_bytes())
            failed = call(mode='batch',input=str(missing),output=str(root/'missing-out'),direction='to-android')
            assert failed['counts']['failed'] == 1,failed
print('PASS 14 vendor/HDR combinations, resume, no JSON, missing pair, original bytes')
