#!/usr/bin/env python3
"""Huawei LIVE_ suffix regression, malformed tails, and optional real conversions."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import jpeg, video


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, default=ROOT / '.build/backend/native-tests')
    parser.add_argument('--samples', type=Path, default=ROOT / 'pictures/huawei')
    args = parser.parse_args()

    def run(request):
        result = subprocess.run([str(args.driver.resolve())], input=json.dumps(request),
                                text=True, capture_output=True, timeout=180)
        return json.loads(result.stdout)

    with tempfile.TemporaryDirectory(prefix='motionlive-huawei-') as folder:
        root = Path(folder)
        movie = video()
        for first, frame in [('v3_f31_c', 31), ('v2_f12', 12), ('v3_f0_c', 0),
                             ('v3_f31_cc', None), ('v3_f31_c_extra', None),
                             ('v3_f_c', None), ('v9_f31_c', None), ('v3_f-1_c', None),
                             ('v3_f9223372036854775808_c', None)]:
            source = root / 'fixture.jpg'
            private = b''
            if first.endswith('_c'):
                private = b''.join(struct.pack('>I', 9) + struct.pack('<I', t) + b'\0'
                                   for t in [5, 1, 0x0306, 2, 3, 4])
                private += struct.pack('>I', len(private) + 4)
            source.write_bytes(jpeg() + movie + private +
                               f'{first:<20}{"0:1299":<20}{"LIVE_" + str(len(movie) + len(private) + 20):<20}'.encode())
            result = run({'mode': 'format', 'input': str(source)})
            if frame is None:
                assert 'error' in result, (first, result)
            else:
                assert result['cover_frame'] == frame, result
                assert result['format'] == 'huawei-honor', result
                assert result['timestamp_tag'] == 'Huawei:frame-index', result
                assert result['video_sha256'] == hashlib.sha256(movie).hexdigest(), result
                if first.endswith('_c'):
                    good = source.read_bytes()
                    for offset, replacement in [(-64, b'\xff' * 4),
                                                (-60 - len(private), b'\0' * 4),
                                                (-60 - len(private) + 4, b'\xff' * 4)]:
                        bad = bytearray(good)
                        at = len(bad) + offset
                        bad[at:at + 4] = replacement
                        source.write_bytes(bad)
                        assert 'error' in run({'mode': 'format', 'input': str(source)})
        print('PASS Huawei suffixed/legacy frame fields and malformed-tail rejection')

        for source in sorted(args.samples.glob('*.jpg')):
            data = source.read_bytes()
            length = int(data[-20:].decode().strip()[5:])
            private_length = int.from_bytes(data[-64:-60], 'big')
            movie = data[len(data) - 40 - length:-60 - private_length]
            parsed = run({'mode': 'format', 'input': str(source)})
            assert 'error' not in parsed, parsed
            assert parsed['cover_frame'] == 31, parsed
            assert parsed['format'] == 'huawei-honor', parsed
            assert parsed['video_sha256'] == hashlib.sha256(movie).hexdigest(), parsed
            result = run({'mode': 'batch', 'input': str(source), 'output': str(root / source.stem)})
            report = json.loads(Path(result['report']).read_text())
            row = report['files'][0]
            assert row['status'] == 'converted', row
            manifest = json.loads((Path(row['output']) / 'manifest.json').read_text())
            assert manifest['source_cover_frame_index'] == 31, manifest
            assert manifest['source_timestamp_tag'] == 'Huawei:frame-index', manifest
            assert manifest['timing']['cover_seconds'] > 0, manifest['timing']
            assert all(t['max_pts_shift_seconds'] == 0 for t in manifest['timing']['tracks'])
            print('PASS', source.name, 'video bytes, conversion, source frame 31 and original PTS')


if __name__ == '__main__':
    main()
