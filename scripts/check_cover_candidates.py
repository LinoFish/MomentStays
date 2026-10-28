#!/usr/bin/env python3
"""Synthetic sparse/multi-frame covers, independent pair times and atomic export.

Uses system media decoders; never accesses the Photos library or personal media.
Build the driver with scripts/build_gui_tests.sh first.
"""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.photo import xmp_packets, values
from motionlive.jpeg import image_hash
from tests.test_formats import motion

DRIVER = ROOT / '.build/gui/native-tests'


def box(kind, payload):
    return struct.pack('>I4s', len(payload) + 8, kind) + payload


def boxes(data, start=0, end=None):
    end = len(data) if end is None else end
    while start < end:
        size, kind = struct.unpack_from('>I4s', data, start)
        header = 8
        if size == 1:
            size = struct.unpack_from('>Q', data, start + 8)[0]
            header = 16
        if size == 0:
            size = end - start
        assert header <= size <= end - start
        yield kind, start, start + header, start + size
        start += size


def child(data, parent, kind):
    return next(b for b in boxes(data, parent[2], parent[3]) if b[0] == kind)


def word(data, at):
    return struct.unpack_from('>I', data, at)[0]


def descriptor(tag, payload):
    size = len(payload)
    return bytes([tag, 0x80 | (size >> 21), 0x80 | ((size >> 14) & 127), 0x80 | ((size >> 7) & 127), size & 127]) + payload


def dual(data, sparse=True, zero=False, time_us=200000):
    moov = next(b for b in boxes(data) if b[0] == b'moov')
    assert moov[3] == len(data)
    primary = next(b for b in boxes(data, moov[2], moov[3]) if b[0] == b'trak')
    mdia = child(data, primary, b'mdia')
    table = child(data, child(data, mdia, b'minf'), b'stbl')
    stts, stsz = [child(data, table, k) for k in [b'stts', b'stsz']]
    step = 0 if zero else word(data, stts[2] + 12)
    fixed = word(data, stsz[2] + 4)
    size = fixed or word(data, stsz[2] + 12)
    offsets = next(b for b in boxes(data, table[2], table[3]) if b[0] in [b'stco', b'co64'])
    offset = int.from_bytes(data[offsets[2] + 8:offsets[2] + (16 if offsets[0] == b'co64' else 12)], 'big')

    def rebuild(parent):
        result = []
        for b in boxes(data, parent[2], parent[3]):
            kind, start, payload, end = b
            part = bytearray(data[payload:end])
            if kind == b'edts':
                continue
            if sparse and kind == b'stbl':
                sd = child(data, b, b'stsd')
                part = data[sd[1]:sd[3]]
                part += box(b'stts', struct.pack('>IIII', 0, 1, 1, step))
                part += box(b'stsz', struct.pack('>IIII', 0, 0, 1, size))
                part += box(b'stsc', struct.pack('>IIIII', 0, 1, 1, 1, 1))
                part += box(b'co64', struct.pack('>IIQ', 0, 1, offset))
                part += box(b'stss', struct.pack('>III', 0, 1, 1))
            elif kind in [b'mdia', b'minf']:
                part = rebuild(b)
            elif kind == b'tkhd':
                assert part[0] == 0
                struct.pack_into('>I', part, 12, 99)
                if sparse:
                    struct.pack_into('>I', part, 20, 0 if zero else 34)
            elif sparse and kind == b'mdhd':
                assert part[0] == 0
                struct.pack_into('>I', part, 16, step)
            result.append(box(kind, bytes(part)))
        return b''.join(result)

    auxiliary = box(b'trak', rebuild(primary))
    if not sparse:
        return data[:moov[1]] + box(b'moov', data[moov[2]:moov[3]] + auxiliary)
    # Store one independent auxiliary keyframe at sample PTS 0. Its descriptor
    # deliberately supplies a different, authoritative presentation time.
    frame = descriptor(0xc3, struct.pack('>fq', .5, time_us))
    primary_frame = descriptor(0xc3, struct.pack('>fq', .5, 400000))
    meta = descriptor(0xc0, descriptor(0xc1, struct.pack('>I', 0) + primary_frame + descriptor(0xc2, struct.pack('>I', 1) + frame)))
    meta_offset = moov[1] + 8
    entry = box(b'mett', bytes(8) + b'\0application/motionphoto-image-meta\0')
    tables = box(b'stsd', struct.pack('>II', 0, 1) + entry)
    tables += box(b'stts', struct.pack('>IIII', 0, 1, 1, 1000000))
    tables += box(b'stsc', struct.pack('>IIIII', 0, 1, 1, 1, 1))
    tables += box(b'stsz', struct.pack('>IIII', 0, 0, 1, len(meta)))
    tables += box(b'co64', struct.pack('>IIQ', 0, 1, meta_offset))
    header = bytearray(data[child(data, primary, b'tkhd')[2]:child(data, primary, b'tkhd')[3]])
    struct.pack_into('>I', header, 12, 100)
    header[-8:] = bytes(8)
    mdhd = box(b'mdhd', struct.pack('>IIIIII', 0, 0, 0, 1000000, 1000000, 0))
    hdlr = box(b'hdlr', bytes(8) + b'meta' + bytes(12))
    dinf = child(data, child(data, mdia, b'minf'), b'dinf')
    minf = box(b'minf', box(b'nmhd', bytes(4)) + data[dinf[1]:dinf[3]] + box(b'stbl', tables))
    metadata = box(b'trak', box(b'tkhd', header) + box(b'mdia', mdhd + hdlr + minf))
    return data[:moov[1]] + box(b'mdat', meta) + box(b'moov', data[moov[2]:moov[3]] + auxiliary + metadata)


def call(request):
    result = subprocess.run([str(DRIVER)], input=json.dumps(request), capture_output=True, text=True, timeout=120)
    assert result.returncode == 0, (result.stdout, result.stderr)
    return json.loads(result.stdout)


def main():
    with tempfile.TemporaryDirectory(prefix='cover-candidates-') as temporary:
        root = Path(temporary)
        fixture = root / 'fixture'
        info = call(dict(mode='fixture', folder=str(fixture)))
        still, video = [(fixture / name).read_bytes() for name in ['still.jpg', 'clip.mp4']]
        inputs = root / 'inputs'
        inputs.mkdir()
        sources = {}
        for name, options in [('sparse', {}), ('zero', {'zero': True}), ('outside', {'time_us': 9000000}), ('multi', {'sparse': False})]:
            source = inputs / (name + '.jpg')
            source.write_bytes(motion(still, str(info['cover_us']), movie=dual(video, **options)))
            sources[name] = source
        sources['single'] = inputs / 'single.jpg'
        sources['single'].write_bytes(motion(still, str(info['cover_us']), movie=video))
        originals = {name: hashlib.sha256(p.read_bytes()).hexdigest() for name, p in sources.items()}
        for name in ['sparse', 'zero', 'multi', 'single']:
            for delivery in ['static', 'livephoto']:
                for selected in [None, 0]:
                    if name == 'single' and selected is not None:
                        continue
                    output = root / f'{name}-{delivery}-{selected}'
                    req = dict(mode='batch', input=str(sources[name]), output=str(output), output_mode='livephoto+candidates', candidate_delivery=delivery)
                    if selected is not None:
                        req['cover_candidate_index'] = selected
                    result = call(req)
                    report = json.loads(Path(result['report']).read_text())
                    assert result['counts']['converted'] == 1, report
                    folder = Path(report['files'][0]['output'])
                    manifest = json.loads((folder / 'manifest.json').read_text())
                    names = manifest['output_files']
                    assert manifest['timing']['selected_video_role'] == 'primary'
                    assert manifest['timing']['tracks'][0]['max_pts_shift_seconds'] == 0
                    if selected is not None:
                        expected = 0 if name == 'multi' else .2
                        assert abs(manifest['timing']['cover_seconds'] - expected) < 2e-6
                        assert manifest['timing']['timing_policy'] == 'selected-high-res-candidate'
                    else:
                        assert image_hash((folder / names['still']).read_bytes()) == image_hash(still)
                    pairs = 1
                    for candidate in manifest['candidate_photos']:
                        role = candidate['role']
                        if role != 'original':
                            timestamps = [value for _, packet in xmp_packets((folder / names[role]).read_bytes())
                                          for value in values(packet, 'urn:momentstays:cover:1.0', 'CandidatePresentationTimestampUs')]
                            assert timestamps == [str(candidate['timestamp_us'])]
                        pair_role = 'original_video' if role == 'original' else 'candidate_video_' + role.removeprefix('candidate_')
                        assert (pair_role in names) == (delivery == 'livephoto')
                        if delivery == 'livephoto':
                            pairs += 1
                            # Native reverse scan validates unique identifiers and real marker time.
                            scanned = call(dict(mode='scan', direction='to-android', input=str(folder / names[role])))
                            assert scanned['rows'][0]['motion'], scanned
                    if delivery == 'livephoto':
                        assert len({candidate['timing']['identifier'] for candidate in manifest['candidate_photos']} | {manifest['identifier']}) == pairs
                    assert call(req)['counts']['resumed'] == 1
                    print('PASS', name, delivery, 'selected', selected, flush=True)
        for name, index in [('sparse', 1), ('single', 0), ('outside', 0)]:
            output = root / f'refuse-{name}'
            req = dict(mode='batch', input=str(sources[name]), output=str(output), cover_candidate_index=index)
            result = call(req)
            assert result['counts']['failed'] == 1 and not list(output.glob('*.livephoto'))
        assert all(hashlib.sha256(path.read_bytes()).hexdigest() == originals[name] for name, path in sources.items())
        print('PASS invalid index, missing auxiliary, unmappable time and source integrity')


if __name__ == '__main__':
    main()
