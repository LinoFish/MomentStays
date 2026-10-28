#!/usr/bin/env python3
"""Existing-output decisions and transactional replacement failure protection."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import motion
BINARY = Path(os.environ.get("MOTIONLIVE_NATIVE_TEST_BINARY", str(ROOT / ".build/gui/native-tests")))


def run(source, output, policy=None, **options):
    request = {'mode':'batch','input':str(source),'output':str(output),**options}
    if policy is not None:
        request['existing_output'] = policy
    p = subprocess.run([str(BINARY)],input=json.dumps(request),text=True,capture_output=True,timeout=180)
    assert p.returncode == 0,(p.stdout,p.stderr)
    result = json.loads(p.stdout)
    return result,json.loads(Path(result['report']).read_text())


def snapshot(folder):
    return {str(p.relative_to(folder)):hashlib.sha256(p.read_bytes()).hexdigest()
            for p in folder.rglob('*') if p.is_file()}


def boxes(data, start=0, end=None):
    end = len(data) if end is None else end
    while start < end:
        size, kind = struct.unpack_from('>I4s', data, start)
        header = 8
        if size == 1:
            size = struct.unpack_from('>Q', data, start + 8)[0]
            header = 16
        elif size == 0:
            size = end - start
        assert header <= size <= end - start
        yield kind, start, start + header, start + size
        start += size


def dual_video(data):
    moov = next(b for b in boxes(data) if b[0] == b'moov')
    assert moov[3] == len(data)
    track = next(b for b in boxes(data, moov[2], moov[3]) if b[0] == b'trak')
    duplicate = bytearray(data[track[1]:track[3]])
    header = next(b for b in boxes(duplicate, 8) if b[0] == b'tkhd')
    struct.pack_into('>I', duplicate, header[2] + 12, 99)
    body = bytearray(data[moov[2]:moov[3]])
    mvhd = next(b for b in boxes(body) if b[0] == b'mvhd')
    struct.pack_into('>I', body, mvhd[3] - 4, 100)
    return data[:moov[1]] + struct.pack('>I4s', len(body) + len(duplicate) + 8, b'moov') + body + duplicate


with tempfile.TemporaryDirectory(prefix='motionlive-existing-',dir=ROOT/'.build') as directory:
    root=Path(directory);src=root/'sources';src.mkdir();out=root/'out'
    source=src/'照片.MP.jpg'
    fixture = root / 'fixture'
    generated = subprocess.run([str(BINARY)], input=json.dumps({'mode':'fixture','folder':str(fixture)}), text=True, capture_output=True, timeout=60)
    assert generated.returncode == 0, (generated.stdout, generated.stderr)
    stamp = json.loads(generated.stdout)['cover_us']
    still, clip = (fixture/'still.jpg').read_bytes(), (fixture/'clip.mp4').read_bytes()
    original = motion(still,str(stamp),movie=clip)
    source.write_bytes(original)
    result,_=run(source,out);assert result['counts']['converted']==1
    pair=out/(source.name+'.livephoto')
    initial=snapshot(pair);old=json.loads((pair/'manifest.json').read_text())
    for settings in [{},{'video_track':'secondary'}]:
        result,report=run(source,out,'skip',**settings)
        assert result['counts']['skipped']==1 and result['counts']['failed']==0,report
        assert snapshot(pair)==initial
    print('PASS skip existing output regardless of changed settings; no failure or byte changes')
    result,report=run(source,out,'cancel')
    assert result['cancelled'] and not any(result['counts'].values()) and snapshot(pair)==initial,report
    print('PASS cancel decision stops batch without changing existing files')
    result,report=run(source,out,'overwrite',video_track='secondary')
    assert result['counts']['failed']==1 and snapshot(pair)==initial,report
    assert not list(out.glob('.motionlive-*'))
    print('PASS failed replacement leaves entire old result intact and removes staging')
    result,report=run(source,out,'overwrite')
    assert result['counts']['converted']==1 and result['counts']['failed']==0,report
    fresh=json.loads((pair/'manifest.json').read_text())
    assert fresh['identifier']!=old['identifier']
    assert fresh['primary_coding_sha256']==old['primary_coding_sha256']
    assert fresh['timing']['cover_seconds']==old['timing']['cover_seconds']
    assert not list(out.glob('.motionlive-*'))
    print('PASS explicit overwrite regenerates and verifies new pair, then atomically replaces old pair')
    (pair/'manifest.json').write_text('damaged')
    damaged=snapshot(pair)
    result,report=run(source,out,'skip')
    assert result['counts']['skipped']==1 and result['counts']['failed']==0 and snapshot(pair)==damaged,report
    result,report=run(source,out,'overwrite')
    assert result['counts']['converted']==1 and result['counts']['failed']==0,report
    assert json.loads((pair/'manifest.json').read_text())['source_sha256']==hashlib.sha256(original).hexdigest()
    print('PASS corrupt or incomplete prior manifest can be skipped or replaced without blocking conversion')
    # A different export layout used to fail before conversion. It now follows
    # the explicit overwrite choice as well, including auxiliary-only outputs.
    auxiliary=src/'aux.jpg';auxiliary.write_bytes(motion(still,str(stamp),movie=dual_video(clip)))
    result,_=run(auxiliary,out);assert result['counts']['converted']==1
    result,report=run(auxiliary,out,'overwrite',output_mode='livephoto+auxiliary')
    assert result['counts']['converted']==1,report
    combined=out/(auxiliary.name+'.livephoto')
    m=json.loads((combined/'manifest.json').read_text())
    assert set(m['output_files'])=={'still','video','auxiliary'}
    result,_=run(auxiliary,out,output_mode='auxiliary-only');assert result['counts']['converted']==1
    result,report=run(auxiliary,out,'overwrite',output_mode='auxiliary-only')
    assert result['counts']['converted']==1,report
    assert source.read_bytes()==original
    print('PASS mode change and auxiliary-only replacement; source bytes unchanged')
print('PASS all existing-output decision checks')
