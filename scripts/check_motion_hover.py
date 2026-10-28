#!/usr/bin/env python3
"""Exercise the native tracking view and real frame decoding with synthetic media."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import motion

APP = ROOT / '.build/gui/preview/MomentStays.app/Contents/MacOS/MomentStays'
DRIVER = ROOT / '.build/gui/native-tests'
OUT = ROOT / '.build/gui/ui-checks'
OUT.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='motion-hover-') as temporary:
    folder = Path(temporary)
    fixture = subprocess.run([str(DRIVER)], input=json.dumps({'mode': 'fixture', 'folder': str(folder)}), text=True, capture_output=True, check=True)
    stamp = json.loads(fixture.stdout)['cover_us']
    source = folder / 'hover.jpg'
    source.write_bytes(motion((folder / 'still.jpg').read_bytes(), str(stamp), movie=(folder / 'clip.mp4').read_bytes()))
    for name, flags in [('hover', []), ('hover-exit', ['--ui-hover-exit']), ('hover-dark', ['--ui-appearance', 'dark'])]:
        screenshot = OUT / (name + '.png')
        result = subprocess.run([str(APP), '--ui-snapshot', str(screenshot), '--ui-width', '1120', '--ui-height', '700',
                                 '--ui-motion-source', str(source), '--ui-hover-preview', *flags], text=True, capture_output=True, timeout=30)
        assert result.returncode == 0, (name, result.stdout, result.stderr)
        metrics = json.loads(Path(str(screenshot) + '.json').read_text())
        assert abs(metrics['preview_cover'] - .4) < .00001, metrics
        assert abs(metrics['preview_duration'] - 1) < .00001, metrics
        if name == 'hover-exit':
            assert metrics['preview_frame'] is None and not metrics['preview_hovering'], metrics
        else:
            assert .76 <= metrics['preview_frame'] <= .81 and metrics['preview_hovering'], metrics
        assert metrics['primary_visible'] and metrics['queue_visible'], metrics
        assert screenshot.stat().st_size > 1000, name
        assert 'Unable to simultaneously satisfy constraints' not in result.stderr, result.stderr
        print('PASS', name, 'cover', metrics['preview_cover'], 'hover frame', metrics['preview_frame'], flush=True)
