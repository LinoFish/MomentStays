#!/usr/bin/env python3
"""Native HDR hover/exit, reduced motion, and stable card layout; synthetic data only."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / '.build/gui/preview/MomentStays.app/Contents/MacOS/MomentStays'
OUTPUT = ROOT / '.build/gui/hdr-badge-ui'
OUTPUT.mkdir(parents=True,exist_ok=True)
cases = [
    ('default',[],False),
    ('hover',['--ui-hdr-hover'],True),
    ('exit',['--ui-hdr-hover','--ui-hdr-hover-exit'],False),
    ('dark',['--ui-appearance','dark'],False),
    ('dark-hover',['--ui-hdr-hover','--ui-appearance','dark'],True),
    ('reduce-motion',['--ui-hdr-hover','--ui-reduce-motion'],True),
    ('cover-after-hover',['--ui-hdr-hover','--ui-check-first','--ui-click-cover'],None),
]
baseline = None
for name,flags,expanded in cases:
    path = OUTPUT / (name + '.png')
    result = subprocess.run([str(APP),'--ui-snapshot',str(path),'--ui-width','1120','--ui-height','700',
        '--ui-fixtures','12',*flags],capture_output=True,text=True,timeout=30)
    assert result.returncode == 0,(name,result.stdout,result.stderr)
    metrics = json.loads(Path(str(path) + '.json').read_text())
    badges = metrics['hdr_badges']
    assert len(badges) == 4,(name,'SDR cards should have no HDR badge',badges)
    if expanded is not None:
        assert sum(value['expanded'] for value in badges.values()) == int(expanded),(name,badges)
        for value in badges.values():
            assert value['title'] == ('Ultra HDR' if value['expanded'] else 'HDR'),(name,value)
            assert value['animation_enabled'] == ('--ui-reduce-motion' not in flags),(name,value)
    if baseline is None:
        baseline = metrics
        cards = [frame for key,frame in metrics['frames'].items() if key.startswith('photo-card-')]
        first_y = min(frame['y'] for frame in cards)
        assert sum(abs(frame['y'] - first_y) < 1 for frame in cards) == 5,('comfortable column count',cards)
    if expanded is not None:
        for key,frame in baseline['frames'].items():
            if key.startswith('photo-card-'):
                actual = metrics['frames'][key]
                assert all(abs(actual[axis] - frame[axis]) < 1 for axis in ['x','y','width','height']),(name,'hover moved a card',key,frame,actual)
        for source,value in badges.items():
            if value['expanded']:
                key = 'photo-hdr-' + source
                assert metrics['frames'][key]['width'] > baseline['frames'][key]['width'] + 10,(name,'badge did not expand')
    if '--ui-click-cover' in flags:
        assert metrics['settings_sheet_visible'] and metrics['selected_count'] == 1,(name,'hover interfered with cover action',metrics)
    else:
        assert metrics['selected_count'] == 0 and not metrics['explicit_selection'],(name,'hover changed selection')
    assert metrics['primary_visible'] and metrics['queue_visible'],name
    assert path.stat().st_size > 1000,name
    assert 'Unable to simultaneously satisfy constraints' not in result.stderr,result.stderr
    print('PASS',name,'HDR disclosure and stable layout',flush=True)
