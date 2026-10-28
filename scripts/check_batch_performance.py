#!/usr/bin/env python3
"""Synthetic serial/automatic comparison; media integrity is checked independently.

Results describe this small local workload, not a general throughput promise.
Run after scripts/build_gui_tests.sh. macOS media services must be available.
"""
import json
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import motion

DRIVER = ROOT / '.build/gui/native-tests'

def call(request, timed=False):
    command = [str(DRIVER)]
    if timed:
        command = ['/usr/bin/time', '-l', *command]
    result = subprocess.run(command, input=json.dumps(request), text=True, capture_output=True, timeout=180)
    assert result.returncode == 0, (result.stdout, result.stderr)
    value = json.loads(result.stdout)
    match = re.search(r'(\d+)\s+maximum resident set size', result.stderr)
    return value, int(match[1]) if match else None

with tempfile.TemporaryDirectory(prefix='motionlive-performance-') as temporary:
    root = Path(temporary)
    fixture = root / 'fixture'
    stamp, _ = call({'mode': 'fixture', 'folder': str(fixture), 'audio': True})
    still = (fixture / 'still.jpg').read_bytes()
    payload = motion(still, str(stamp['cover_us']), movie=(fixture / 'clip.mp4').read_bytes())
    source = root / 'sources'
    source.mkdir()
    for index in range(8):
        (source / f'motion-{index:02d}.jpg').write_bytes(payload)
    for index in range(24):
        (source / f'static-{index:02d}.jpg').write_bytes(still)
    runs = []
    baseline = None
    for repeat in range(3):
        for name, workers in [('serial', 1), ('automatic', None)]:
            output = root / f'{name}-{repeat}'
            request = {'mode': 'batch', 'input': str(source), 'output': str(output)}
            if workers is not None:
                request['concurrency'] = workers
            value, rss = call(request, timed=True)
            assert value['counts'] == {'converted': 8, 'skipped': 24, 'failed': 0, 'resumed': 0}, value
            report = json.loads(Path(value['report']).read_text())
            assert report['performance']['static_skipped'] == 24, report
            manifests = [json.loads(path.read_text()) for path in sorted(output.glob('*.livephoto/manifest.json'))]
            # UUID/file hashes differ; compare compressed media and timing instead.
            signatures = [(m['primary_coding_sha256'], {k: v for k, v in m['timing'].items() if k != 'identifier'}) for m in manifests]
            if baseline is None:
                baseline = signatures
            else:
                assert signatures == baseline, 'serial/parallel media or timeline changed'
            runs.append({'mode': name, 'repeat': repeat + 1, 'cache': 'first pair' if repeat == 0 else 'warm',
                         'maximum_rss_bytes': rss, **report['performance']})
            print('PASS', name, repeat + 1, f"{report['performance']['total_seconds']:.3f}s", flush=True)
    result = {'platform': platform.platform(), 'architecture': platform.machine(),
              'workload': '8 synthetic one-second motion JPEGs with audio + 24 static JPEGs; optimized native driver; local temporary storage',
              'runs': runs, 'median_seconds': {name: statistics.median(r['total_seconds'] for r in runs if r['mode'] == name)
                                              for name in ('serial', 'automatic')}}
    target = ROOT / '.build/gui/batch-performance.json'
    target.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print('Results:', target)
