#!/usr/bin/env python3
"""Real Samsung HEIC: independently compare every coded item and property."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT/'pictures/sample-device-motion.HEIC'

def call(**request):
    p = subprocess.run([str(ROOT/'.build/gui/native-tests')], input=json.dumps(request),
                       text=True,capture_output=True,timeout=180)
    assert p.returncode == 0, (p.stdout,p.stderr)
    return json.loads(p.stdout)

def batch(source, output, **settings):
    r = call(mode='batch',input=str(source),output=str(output),**settings)
    return r, r.get('report_data') if r['report'] is None else json.loads(Path(r['report']).read_text())

def boxes(d, a, z):
    result = []
    while a < z:
        size, kind = struct.unpack_from('>I4s',d,a); h = 8
        if size == 1: size = struct.unpack_from('>Q',d,a+8)[0]; h = 16
        if size == 0: size = z-a
        assert size >= h and a+size <= z
        result.append((kind,a,a+h,a+size)); a += size
    assert a == z
    return result

def layout(d):
    top = boxes(d,0,len(d)); meta = next(v for v in top if v[0] == b'meta')
    children = boxes(d,meta[2]+4,meta[3]); info = next(v for v in children if v[0] == b'iinf')
    types = {}
    for _,_,p,z in boxes(d,info[2]+6,info[3]):
        assert d[p] == 2
        types[int.from_bytes(d[p+4:p+6],'big')] = d[p+8:p+12]
    loc = next(v for v in children if v[0] == b'iloc'); p = loc[2]
    assert d[p:p+6] == b'\x01\0\0\0\x44\0'; p += 6
    count = int.from_bytes(d[p:p+2],'big'); p += 2
    items = {}; patches = {}
    for _ in range(count):
        iid,method,ref,count = struct.unpack_from('>HHHH',d,p); p += 8
        assert ref == 0 and count == 1
        patches[iid] = p
        offset, length = struct.unpack_from('>II',d,p); p += 8
        if method == 1: offset += next(v for v in children if v[0] == b'idat')[2]
        assert 0 <= offset < offset+length <= len(d)
        items[iid] = (types[iid],d[offset:offset+length])
    assert p == loc[3]
    props = {k:d[a:z] for k,a,_,z in children if k in [b'iprp',b'iref',b'pitm']}
    return items,props,patches,top

original = SOURCE.read_bytes(); digest = hashlib.sha256(original).hexdigest()
items,props,patches,top = layout(original)
scan = call(mode='scan',input=str(SOURCE.parent))['rows']
row = next(r for r in scan if r['relative'] == SOURCE.name)
assert row['motion'] and row['manufacturer'] == '三星' and row['cover'], row
out = ROOT/'converted-1.1.4-samsung'
r,report = batch(SOURCE,out,existing_output='overwrite')
assert r['counts']['converted'] == 1 and r['counts']['failed'] == 0,report
pair = Path(report['files'][0]['output']); manifest = json.loads((pair/'manifest.json').read_text())
assert manifest['version'] == '1.1.4',manifest['version']
result = (pair/SOURCE.name).read_bytes(); actual,actual_props,_,actual_top = layout(result)
assert set(actual) == set(items) and props == actual_props
coded = [i for i,(t,_) in items.items() if t not in [b'Exif',b'mime']]
assert len(coded) == 50 and all(items[i] == actual[i] for i in coded)
assert all(k not in [b'mpvd',b'moov'] for k,_,_,_ in actual_top)
assert result.count(b'ftyp') == 1 and b'SEFH' not in result and b'MotionPhoto' not in result
assert SOURCE.stem + '.mov' in manifest['output_sha256']
assert manifest['source_timestamp_us'] > 0
assert manifest['timing']['cover_seconds'] > 0
assert manifest['timing']['snap_error_seconds'] < 0.001
assert all(t['max_pts_shift_seconds'] == 0 for t in manifest['timing']['tracks'])
assert manifest['hdr']['input'] == 'heic' and manifest['hdr']['status'] == 'preserved'
print('PASS Samsung scan; 50 image/grid/thumbnail items byte-identical; properties/references unchanged')
print('PASS HEIC + MOV original names; no embedded motion video/audio or active MotionPhoto XMP')
print('PASS source cover mapped to an output frame; compressed video/audio and all PTS preserved')
inspector = ROOT/'.build/samsung-heic/inspect-still'
p = subprocess.run([str(inspector),str(SOURCE),str(pair/SOURCE.name)],capture_output=True,text=True,timeout=90)
assert p.returncode == 0,(p.stdout,p.stderr)
images = [json.loads(v) for v in p.stdout.splitlines()]
assert images[0]['pixel_sha256'] == images[1]['pixel_sha256']
assert images[0]['width'] == images[1]['width'] == 4000 and images[0]['height'] == images[1]['height'] == 3000
assert images[0]['orientation'] == images[1]['orientation']
assert images[1]['identifier'] == manifest['identifier']
(ROOT/'.build/samsung-heic/imageio-result.json').write_text(json.dumps(images,indent=2))
print('PASS real ImageIO decode: identical 4000x3000 pixels and orientation; pairing identifier readable')
with tempfile.TemporaryDirectory(prefix='motionlive-heic-',dir=ROOT/'.build') as directory:
    root = Path(directory); inputs = root/'inputs'; inputs.mkdir()
    for name, data in [('bad-extent',bytearray(original)),('bad-pointer',bytearray(original)),('bad-time',bytearray(original))]:
        if name == 'bad-extent': struct.pack_into('>I',data,patches[1],len(original)-100)
        if name == 'bad-pointer':
            at = data.rindex(b'mpv2')+4; struct.pack_into('>I',data,at,struct.unpack_from('>I',data,at)[0]+1)
        if name == 'bad-time': data = data.replace(b'TimestampUs="3000000"',b'TimestampUs="9000000"')
        source = inputs/(name+'.HEIC'); source.write_bytes(data)
        r,log = batch(source,root/(name+'-out'))
        assert r['counts']['failed'] == 1,log
        assert not list((root/(name+'-out')).glob('*.livephoto'))
    static = inputs/'static.heic'; static.write_bytes(original[:next(v[1] for v in top if v[0] == b'mpvd')])
    r,log = batch(static,root/'static-out'); assert r['counts']['skipped'] == 1,log
    lower = inputs/'lower.heic'; lower.write_bytes(original)
    r,log = batch(lower,root/'media-only',export_data_files=False)
    assert r['counts']['converted'] == 1 and not list((root/'media-only').rglob('*.json')),log
    assert {p.suffix for p in Path(log['files'][0]['output']).iterdir()} == {'.heic','.mov'}
print('PASS invalid extent/SEF pointer/cover rejected without publication; static HEIC skipped; lowercase and no-JSON supported')
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest() == digest
print('PASS original Samsung HEIC SHA256 unchanged')
