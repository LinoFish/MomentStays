#!/usr/bin/env python3
"""Cross-check Swift Extended XMP using structural fixtures and optional local originals."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.jpeg import FormatError, image_hash, mpf_images
from motionlive.photo import CAMERA, live_still, parse, q, xmp_packets
from tests.test_extended import extended, extension_payloads


def call(request, ok=True):
    result = subprocess.run([str(ROOT / '.build/gui/native-tests')], input=json.dumps(request), text=True, capture_output=True)
    value = json.loads(result.stdout)
    assert (result.returncode == 0) == ok, value
    return value


count = 0
with tempfile.TemporaryDirectory(prefix='motionlive-extended-') as folder:
    root = Path(folder)
    fixtures = [(f'fixture-{mode}-{reverse}', extended(mode, reverse)) for mode in (False, True) for reverse in (False, True)]
    originals = list((ROOT / 'pictures').glob('*.jpg')) + list((ROOT / '.build/samples/Google').glob('*.jpg'))
    selected = 0
    for path in sorted(originals):
        data = path.read_bytes()
        if not any(packet.guid for packet in xmp_packets(data)):
            continue
        try:
            parse(data)  # This cross-check uses the Google-only Python parser.
        except FormatError:
            continue
        fixtures.append((path.stem,data)); selected += 1
        if selected == 3: break
    for index, (name, data) in enumerate(fixtures):
        src, out = root / f'{index}.jpg', root / f'{index}-out.jpg'
        src.write_bytes(data)
        original = parse(data)
        result = call({'mode':'format', 'input':str(src), 'output':str(out)})
        actual = out.read_bytes()
        assert result['timestamp_us'] == original.timestamp_us
        assert result['coding_hash'] == result['output_coding_hash'] == image_hash(original.still)
        assert mpf_images(actual)[1:] == mpf_images(original.still)[1:]
        packets = xmp_packets(actual)
        assert not any(q(CAMERA, "MotionPhoto") in e.attrib for _, packet in packets for e in packet.iter())
        if name.startswith("fixture-"):
            assert any("保留" * 18000 in (e.text or "") for _, packet in packets for e in packet.iter())
        expected = live_still(original.still, 'TEST')
        assert extension_payloads(actual) == extension_payloads(data) if not name.startswith('fixture-True') else True
        assert image_hash(expected) == image_hash(actual)
        assert hashlib.sha256(src.read_bytes()).digest() == hashlib.sha256(data).digest()
        print(f'PASS {name}: cover={original.timestamp_us}, ultra={original.hdr}, unchanged primary/auxiliary JPEG')
        count += 1
    invalid = root / 'invalid.jpg'
    invalid.write_bytes(extended().replace(b'private>', b'privatf>', 1))
    call({'mode':'format', 'input':str(invalid)}, ok=False)
    count += 1
print(f'{count} Extended XMP native checks passed')
