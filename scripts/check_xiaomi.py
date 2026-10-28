#!/usr/bin/env python3
"""Regress AAC reader regrouping using real Xiaomi inputs and raw BMFF tables."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.photo import parse
from motionlive.jpeg import image_hash


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
        yield kind, start + header, start + size
        start += size


def child(data, parent, kind):
    found = [b for b in boxes(data, parent[1], parent[2]) if b[0] == kind]
    assert len(found) == 1, (kind, found)
    return found[0]


def audio_tables(data):
    moov = next(b for b in boxes(data) if b[0] == b'moov')
    for track in boxes(data, moov[1], moov[2]):
        if track[0] != b'trak':
            continue
        media = child(data, track, b'mdia')
        handler = child(data, media, b'hdlr')
        if data[handler[1] + 8:handler[1] + 12] != b'soun':
            continue
        table = child(data, child(data, media, b'minf'), b'stbl')
        sizes = child(data, table, b'stsz')
        fixed, count = struct.unpack_from('>II', data, sizes[1] + 4)
        packet_sizes = [fixed] * count if fixed else list(struct.unpack_from(f'>{count}I', data, sizes[1] + 12))
        times = child(data, table, b'stts')
        runs = struct.unpack_from('>I', data, times[1] + 4)[0]
        deltas = []
        for i in range(runs):
            n, delta = struct.unpack_from('>II', data, times[1] + 8 + i * 8)
            deltas.extend([delta] * n)
        assert len(deltas) == count
        return packet_sizes, deltas
    raise AssertionError('Missing audio track')


result = subprocess.run([str(ROOT/'.build/gui/native-tests')], input=json.dumps({
    'mode': 'batch', 'input': str(ROOT/'pictures/vendor-samples'),
    'output': str(ROOT/'converted-1.1.0-xiaomi')}), text=True, capture_output=True, timeout=180)
assert result.returncode == 0, (result.stdout, result.stderr)
record = json.loads(result.stdout)
report = json.loads(Path(record['report']).read_text())
assert record['counts']['failed'] == 0 and record['counts']['converted'] + record['counts']['resumed'] == 3, report
for row in report['files']:
    source = Path(row['source'])
    photo = parse(source.read_bytes())
    pair = Path(row['output'])
    manifest = json.loads((pair/'manifest.json').read_text())
    names = manifest['output_files']
    assert names['still'] == source.name and names['video'] == source.stem + '.mov'
    assert image_hash(source.read_bytes()) == image_hash((pair/names['still']).read_bytes())
    original = audio_tables(photo.video)
    assert audio_tables((pair/names['video']).read_bytes()) == original
    timing = manifest['timing']
    assert manifest['source_timestamp_us'] == photo.timestamp_us
    assert timing['snap_error_seconds'] <= timing['source_frame_duration_seconds'] / 2 + .000002
    audio = next(t for t in timing['tracks'] if t['type'] == 'soun')
    assert audio['packets'] == len(original[0]) > 2
    assert all(t['max_pts_shift_seconds'] == 0 for t in timing['tracks'])
    for name, digest in manifest['output_sha256'].items():
        assert hashlib.sha256((pair/name).read_bytes()).hexdigest() == digest
    print('PASS', source.name, 'audio packets', audio['packets'], 'raw audio sizes/durations, cover and PTS preserved')
(ROOT/'.build/xiaomi-fixed-result.json').write_text(json.dumps(record))
print('PASS all 3 Xiaomi photos; AAC regrouping no longer triggers a false timeline error')
