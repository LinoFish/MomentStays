#!/usr/bin/env python3
"""Exercise the same multi-input scan/conversion API used by GUI pick/drop/open."""
import hashlib
import json
from pathlib import Path
import sys
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / '.build/gui/native-tests'
sys.path.insert(0, str(ROOT))
from motionlive.photo import parse


def call(request, ok=True):
    result = subprocess.run([str(BINARY)], input=json.dumps(request), text=True, capture_output=True, timeout=180)
    value = json.loads(result.stdout)
    assert (result.returncode == 0) == ok, (result.stderr, value)
    return value


# Reuse an existing original read-only. No Photos library or external services.
original = next((ROOT / 'pictures').glob('*.jpg'))
source_bytes = original.read_bytes()
with tempfile.TemporaryDirectory(prefix='motionlive-multi-', dir=ROOT / '.build') as folder:
    root = Path(folder)
    a, b = root / '来源 A', root / '来源 B'
    a.mkdir(); b.mkdir()
    first, second = a / 'same.MP.jpg', b / 'same.MP.jpg'
    for path in (first, second, a / '不要选中.jpg'):
        path.write_bytes(source_bytes)
    selected = [str(first), str(second), str(first)]
    scan = call({'mode':'scan', 'inputs':selected})
    assert {row['relative'] for row in scan['rows']} == {'来源 A/same.MP.jpg', '来源 B/same.MP.jpg'}, scan
    assert all(row['motion'] for row in scan['rows']), scan
    print('PASS multi-file scan, duplicate selection, unselected sibling excluded')
    mixed = call({'mode':'scan', 'inputs':[str(a), str(first), str(b)]})
    assert len(mixed['rows']) == 3, mixed
    print('PASS mixed folders/files and overlapping selection deduplicated')
    for source in (a, b):
        call({'mode':'batch', 'inputs':selected, 'output':str(source / 'unsafe')}, ok=False)
    print('PASS output validated against every selected source')
    output = root / 'output'
    request = {'mode':'batch', 'inputs':selected, 'output':str(output)}
    result = call(request)
    assert result['counts'] == {'converted':2, 'resumed':0, 'skipped':0, 'failed':0}, result
    for directory in ('来源 A', '来源 B'):
        pair = output / directory / 'same.MP.jpg.livephoto'
        manifest = json.loads((pair / 'manifest.json').read_text())
        assert manifest['implementation'] == 'gui-native-1.1.4'
        assert manifest['source_sha256'] == hashlib.sha256(source_bytes).hexdigest()
        for name, digest in manifest['output_sha256'].items():
            assert hashlib.sha256((pair/name).read_bytes()).hexdigest() == digest
        assert manifest['source_timestamp_us'] == parse(source_bytes).timestamp_us
        timing = manifest['timing']
        assert abs(timing['requested_seconds'] - timing['cover_seconds']) <= timing['source_frame_duration_seconds'] / 2 + 2e-6
    assert not list(output.rglob('*不要选中*'))
    print('PASS both same-name photos converted separately; timestamps and hashes verified')
    again = call(request)
    assert again['counts']['resumed'] == 2 and again['counts']['failed'] == 0, again
    assert first.read_bytes() == second.read_bytes() == source_bytes
    print('PASS multi-input resume; originals unchanged')
print('PASS all multi-import checks')
