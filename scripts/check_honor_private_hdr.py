#!/usr/bin/env python3
"""Real Honor private/Ultra HDR pair: static metadata, cover, rollback."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.jpeg import image_hash, mpf_images, segments


def call(**request):
    result = subprocess.run([str(ROOT/'.build/gui/native-tests')], input=json.dumps(request),
                            capture_output=True, text=True, timeout=180)
    assert result.returncode == 0, (result.stdout, result.stderr)
    return json.loads(result.stdout)


def batch(source, output, **settings):
    result = call(mode='batch', input=str(source), output=str(output), **settings)
    return result, json.loads(Path(result['report']).read_text())


def private_range(data):
    length = int(data[-20:].strip().removeprefix(b'LIVE_')) - 20
    return segments(data)[-1].end, len(data) - 60 - length


def cover_pts(data):
    # Independent ISO BMFF sample-table reader: frame index is presentation
    # order, not frame/nominal-fps (the private sample starts at nonzero PTS).
    length = int(data[-20:].strip().removeprefix(b'LIVE_')) - 20
    movie = data[len(data)-60-length:len(data)-60]
    def boxes(start, end):
        result = {}; pos = start
        while pos < end:
            size = int.from_bytes(movie[pos:pos+4], 'big'); header = 8
            if size == 1: size = int.from_bytes(movie[pos+8:pos+16], 'big'); header = 16
            if size == 0: size = end-pos
            assert size >= header and pos+size <= end
            result.setdefault(movie[pos+4:pos+8], []).append((pos+header,pos+size))
            pos += size
        return result
    moov = boxes(0,len(movie))[b'moov'][0]
    for trak in boxes(*moov)[b'trak']:
        mdia = boxes(*trak)[b'mdia'][0]; parts = boxes(*mdia)
        hdlr = parts[b'hdlr'][0][0]
        if movie[hdlr+8:hdlr+12] != b'vide': continue
        mdhd = parts[b'mdhd'][0][0]
        scale_at = mdhd + (20 if movie[mdhd] == 1 else 12)
        scale = int.from_bytes(movie[scale_at:scale_at+4],'big')
        stbl = boxes(*boxes(*parts[b'minf'][0])[b'stbl'][0])
        stts = stbl[b'stts'][0][0]
        def entries(at):
            count = int.from_bytes(movie[at+4:at+8], 'big')
            return [struct.unpack_from('>II',movie,at+8+i*8) for i in range(count)]
        dts = []; now = 0
        for count, delta in entries(stts):
            for _ in range(count): dts.append(now); now += delta
        offsets = [0] * len(dts)
        if b'ctts' in stbl:
            at = stbl[b'ctts'][0][0]; offsets = []
            for count, value in entries(at):
                if movie[at] == 1 and value >= 2**31: value -= 2**32
                offsets.extend([value]*count)
        assert len(offsets) == len(dts)
        frame = int(data[-60:-40].strip().split(b'_f')[1])
        return sorted(a+b for a,b in zip(dts,offsets))[frame]/scale
    raise AssertionError('missing source video')


private = ROOT/'pictures/honor/sample-private-hdr.jpg'
ultra = ROOT/'pictures/honor/sample-ultra-hdr.jpg'
before = {p: hashlib.sha256(p.read_bytes()).hexdigest() for p in [private, ultra]}
rows = call(mode='scan', input=str(private.parent))['rows']
assert [(r['hdr'],r['manufacturer']) for r in rows] == [('荣耀私有 HDR','荣耀'),('Ultra HDR','荣耀')], rows
print('PASS private and standard HDR identified separately; manufacturer Honor')

out = ROOT/'.build/honor-private-hdr/output'
r, report = batch(private.parent, out, existing_output='overwrite')
assert r['counts']['converted'] == 2 and r['counts']['failed'] == 0, report
for row in report['files']:
    source = Path(row['source']); pair = Path(row['output'])
    manifest = json.loads((pair/'manifest.json').read_text())
    data = (pair/manifest['output_files']['still']).read_bytes()
    original = source.read_bytes()
    assert image_hash(data) == image_hash(original)
    assert manifest['source_cover_frame_index'] == 36
    assert abs(manifest['timing']['cover_seconds'] - cover_pts(original)) < 0.000002
    assert all(t['max_pts_shift_seconds'] == 0 for t in manifest['timing']['tracks'])
    assert manifest['still_contains_embedded_media'] is False
    if source == private:
        start, end = private_range(original)
        extension = original[start:end]
        assert len(extension) == 56073
        assert data[segments(data)[-1].end:] == extension
        assert manifest['hdr']['private_extension_sha256'] == hashlib.sha256(extension).hexdigest()
        assert manifest['hdr']['status'] == 'unsupported-preserved'
        assert 'SDR' in manifest['hdr']['reason'] and row['warnings']
        assert not mpf_images(data), 'private format must not acquire a guessed gain map'
        assert b'ftyp' not in extension and original[end:end+128] not in data
    else:
        assert manifest['hdr']['input'] == 'ultra-hdr'
        assert manifest['hdr']['status'] == 'preserved'
        assert mpf_images(data)[1:] == mpf_images(original)[1:]
        assert len(data) == sum(map(len, mpf_images(data)))
print('PASS both real samples: primary coding unchanged, exact frame 36 mapped through source stts/ctts; video/audio removed from JPEG')
print('PASS private static extension byte-for-byte; unsupported warning; no invented map; Ultra HDR gain JPEG byte-for-byte')

with tempfile.TemporaryDirectory(prefix='motionlive-honor-', dir=ROOT/'.build') as directory:
    root = Path(directory)
    inputs = root/'inputs'; inputs.mkdir()
    original = private.read_bytes(); start, end = private_range(original)
    for name, position, value in [('version',end-24,2),('record-length',end-49,133),('prefix-length',start+55880,99999)]:
        bad = bytearray(original); struct.pack_into('<I',bad,position,value)
        source = inputs/(name+'.jpg'); source.write_bytes(bad)
        result, log = batch(source,root/(name+'-out'))
        assert result['counts']['failed'] == 1 and '荣耀私有 HDR' in log['files'][0]['error'],log
        assert not list((root/(name+'-out')).glob('*.livephoto'))
    result, log = batch(private,root/'preserve',legacy_hdr=False)
    assert result['counts']['converted'] == 1
    assert log['files'][0]['hdr']['status'] == 'unsupported-preserved'
print('PASS malformed/unknown HDR extension rejected without publishing; disabling HDR conversion still preserves private data')

# An old successful output must not be silently resumed as if HDR had survived.
pair = out/(private.name+'.livephoto'); manifest_path = pair/'manifest.json'
manifest = json.loads(manifest_path.read_text())
manifest['implementation'] = 'gui-native-1.1.2'
manifest['hdr'].pop('private_extension_sha256')
manifest_path.write_text(json.dumps(manifest))
result, log = batch(private,out)
assert result['counts']['failed'] == 1 and '旧版本输出' in log['files'][0]['error'],log
result, log = batch(private,out,existing_output='overwrite')
assert result['counts']['converted'] == 1,log
print('PASS old unverified private HDR output requires explicit overwrite; successful replacement verified')
assert all(hashlib.sha256(p.read_bytes()).hexdigest() == digest for p,digest in before.items())
print('PASS original sample SHA256 unchanged')
