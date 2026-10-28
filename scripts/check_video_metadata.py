#!/usr/bin/env python3
"""Native metadata recovery bounds tests using tiny structural MP4 fixtures."""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import jpeg, motion
from motionlive.jpeg import image_hash


def box(kind, payload):
    return struct.pack('>I4s', len(payload) + 8, kind) + payload


def descriptor(tag, payload):
    n = len(payload)
    return bytes([tag, 0x80 | (n >> 21), 0x80 | ((n >> 14) & 127), 0x80 | ((n >> 7) & 127), n & 127]) + payload


def video(timestamp=1234567, invalid_offset=False, truncated_descriptor=False, stco=False, reserved=False, duplicate=False):
    frame = descriptor(0xc3, struct.pack('>fq', .5, timestamp))
    meta = descriptor(0xc0, descriptor(0xc1, struct.pack('>I', 0) + frame + descriptor(0xc2, bytes(4))))
    if truncated_descriptor:
        meta = meta[:-1]
    ftyp = box(b'ftyp', b'isom\0\0\0\0isom')
    offset = len(ftyp) + 8 + (99999 if invalid_offset else 0)
    sample = box(b'mett', (bytes(8) if reserved else b'') + b'\0application/motionphoto-image-meta\0')
    stsd = box(b'stsd', struct.pack('>II', 0, 1) + sample)
    stsz = box(b'stsz', struct.pack('>IIII', 0, 0, 1, len(meta)))
    stsc = box(b'stsc', struct.pack('>IIIII', 0, 1, 1, 1, 1))
    offsets = box(b'stco' if stco else b'co64', struct.pack('>II', 0, 1) + struct.pack('>I' if stco else '>Q', offset))
    stbl = box(b'stbl', stsd + stsz + stsc + offsets)
    hdlr = box(b'hdlr', bytes(8) + b'meta' + bytes(12))
    track = box(b'trak', box(b'mdia', hdlr + box(b'minf', stbl)))
    return ftyp + box(b'mdat', meta) + box(b'moov', track * (2 if duplicate else 1))


def call(src, ok=True, output=None):
    request = {'mode':'format', 'input':str(src)}
    if output is not None:
        request['output'] = str(output)
    p = subprocess.run([str(ROOT/'.build/gui/native-tests')], input=json.dumps(request), text=True, capture_output=True)
    data = json.loads(p.stdout)
    assert (p.returncode == 0) == ok, data
    return data


with tempfile.TemporaryDirectory(prefix='motionlive-meta-') as folder:
    folder = Path(folder)
    src, out = folder/'source.jpg', folder/'photo.jpg'
    for name, settings in [('co64',{}), ('stco',{'stco':True}), ('reserved',{'reserved':True})]:
        src.write_bytes(jpeg() + video(**settings))
        result = call(src, output=out)
        assert result['timestamp_us'] == 1234567
        assert result['coding_hash'] == result['output_coding_hash'] == image_hash(jpeg())
        print('PASS recovered exact timestamp:', name)
    for name, settings in [('negative',{'timestamp':-1}), ('offset',{'invalid_offset':True}), ('descriptor',{'truncated_descriptor':True})]:
        src.write_bytes(jpeg() + video(**settings))
        call(src, ok=False)
        print('PASS refuses invalid metadata:', name)
    # An explicit MotionPhoto=0 must never be overridden by a valid trailer.
    src.write_bytes(motion(movie=video()).replace(b'GCamera:MotionPhoto="1"', b'GCamera:MotionPhoto="0"'))
    call(src, ok=False)
    print('PASS explicit non-motion flag respected')
    # Signatures inside arbitrary trailing bytes must never be searched for.
    src.write_bytes(jpeg() + b'junk' + video())
    call(src, ok=False)
    print('PASS rejects non-contiguous video signature')
print('PASS 8 video metadata recovery checks')
