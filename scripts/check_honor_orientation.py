#!/usr/bin/env python3
"""Honor's compatible layout: preserve orientation, media and cover time.

Pass a local Motion Photo path to repeat the comparison with a real sample.
No pixel encoding or decoding is needed for these container checks.
"""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.jpeg import app, image_hash, segments
from motionlive.photo import parse, xmp_packets
from tests.test_formats import jpeg, motion, video


def assemble(source, output, vendor):
    request = dict(mode='android-assemble', input=str(source), output=str(output),
                   vendor=vendor, time_us=1433333, frame=43)
    result = subprocess.run([str(ROOT/'.build/gui/native-tests')],
                            input=json.dumps(request), text=True,
                            capture_output=True, timeout=60)
    assert result.returncode == 0, (result.stdout, result.stderr)
    return json.loads(result.stdout)


def exif(data):
    return [s.payload for s in segments(data)
            if s.marker == 225 and s.payload.startswith(b'Exif\0\0')]


with tempfile.TemporaryDirectory(prefix='motionlive-honor-orientation-') as folder:
    root = Path(folder)
    sources = []
    # EXIF + tkhd rotations: this checks that the layout fix never normalizes
    # either direction field or modifies the encoded image/video payload.
    for orientation, (a, b, c, d) in [(1, (1, 0, 0, 1)), (6, (0, 1, -1, 0)),
                                    (3, (-1, 0, 0, -1)), (8, (0, -1, 1, 0))]:
        tiff = b'II' + struct.pack('<HIH', 42, 8, 1)
        tiff += struct.pack('<HHIHHI', 274, 3, 1, orientation, 0, 0)
        still = jpeg()[:2] + app(225, b'Exif\0\0' + tiff) + jpeg()[2:]
        matrix = struct.pack('>9i', a*65536, b*65536, 0, c*65536, d*65536,
                             0, 0, 0, 1073741824)
        tkhd = struct.pack('>I4s', 92, b'tkhd') + bytes(40) + matrix + struct.pack('>II', 16<<16, 16<<16)
        track = struct.pack('>I4s', len(tkhd)+8, b'trak') + tkhd
        movie = video().replace(struct.pack('>I4s', 8, b'moov'),
                                struct.pack('>I4s', len(track)+8, b'moov') + track)
        source = root/f'orientation-{orientation}.jpg'
        source.write_bytes(motion(still=still, movie=movie))
        sources.append(source)
    sources += [Path(p).resolve() for p in sys.argv[1:]]
    for index, source in enumerate(sources):
        google, honor, huawei = [root/f'{index}-{v}.jpg' for v in ('google', 'honor', 'huawei')]
        baseline = assemble(source, google, 'google')
        result = assemble(source, honor, 'honor')
        legacy = assemble(source, huawei, 'huawei')
        # XML namespace/attribute serialization order can differ across native
        # processes; compare its meaning and all original media bytes instead.
        def metadata(path):
            return [ET.canonicalize(ET.tostring(xml, encoding='unicode'), rewrite_prefixes=True)
                    for _, xml in xmp_packets(path.read_bytes())]
        assert metadata(honor) == metadata(google)
        assert result['format'] == 'google-compatible', result
        assert result['timestamp_us'] == 1433333 and result['cover_frame'] is None
        assert legacy['format'] == 'huawei-honor' and legacy['cover_frame'] == 43
        assert result['video_sha256'] == baseline['video_sha256'] == legacy['video_sha256']
        assert result['coding_hash'] == baseline['coding_hash'] == legacy['coding_hash']
        decoded = parse(honor.read_bytes())  # independent Python XMP parser
        google_photo = parse(google.read_bytes())
        assert decoded.video == google_photo.video
        assert honor.read_bytes().endswith(decoded.video)  # no private footer
        assert decoded.timestamp_us == 1433333
        assert hashlib.sha256(decoded.video).hexdigest() == result['video_sha256']
        assert image_hash(decoded.still) == image_hash(source.read_bytes())
        assert exif(decoded.still) == exif(source.read_bytes())
        if index < 4:
            assert decoded.video == parse(source.read_bytes()).video
        print('PASS Honor/Google equivalent; EXIF, compressed media, cover preserved:', index)
print('PASS Honor compatibility and Huawei frame trailer regression')
