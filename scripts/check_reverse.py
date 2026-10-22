#!/usr/bin/env python3
"""Round-trip real encoded packets and independently read Android JPEG output."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import struct

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

def apple_marker(movie):
    data=bytearray(movie.read_bytes())
    def boxes(a,z):
        result=[]
        while a<z:
            size,kind=struct.unpack_from('>I4s',data,a);h=8
            if size==1:size=struct.unpack_from('>Q',data,a+8)[0];h=16
            if size==0:size=z-a
            assert size>=h and a+size<=z
            result.append((kind,a+h,a+size));a+=size
        return result
    def child(parent,kind):return next(v for v in boxes(parent[1],parent[2]) if v[0]==kind)
    moov=next(v for v in boxes(0,len(data)) if v[0]==b'moov')
    for track in boxes(moov[1],moov[2]):
        if track[0]!=b'trak' or b'com.apple.quicktime.still-image-time' not in data[track[1]:track[2]]:continue
        table=child(child(child(track,b'mdia'),b'minf'),b'stbl')
        loc=next(v for v in boxes(table[1],table[2]) if v[0] in [b'stco',b'co64'])
        width=4 if loc[0]==b'stco' else 8
        assert int.from_bytes(data[loc[1]+4:loc[1]+8],'big')==1
        offset=int.from_bytes(data[loc[1]+8:loc[1]+8+width],'big')
        size=child(table,b'stsz');p=size[1]
        assert int.from_bytes(data[p+8:p+12],'big')==1
        length=int.from_bytes(data[p+4:p+8],'big') or int.from_bytes(data[p+12:p+16],'big')
        sample=data[offset:offset+length]
        assert sample==bytes.fromhex('000000090000000100'),sample.hex()
        data[offset+length-1]=255;movie.write_bytes(data);return
    raise AssertionError('Missing still marker track')

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
        apple_marker(pair.with_suffix(".mov"))
        before = hashlib.sha256(pair.read_bytes()).hexdigest()
        original_exif = [s.payload for s in segments(pair.read_bytes()) if s.marker == 225 and s.payload.startswith(b'Exif\0\0')]
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
            assert [s.payload for s in segments(photo.still) if s.marker == 225 and s.payload.startswith(b'Exif\0\0')] == original_exif
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
