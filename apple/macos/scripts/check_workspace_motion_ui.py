#!/usr/bin/env python3
"""Toolbar, inline task progress, real stop clicks and continuous UI interaction checks."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / '.build/gui/preview/MomentStays.app/Contents/MacOS/MomentStays'
OUT = ROOT / '.build/gui/workspace-motion-ui'
OUT.mkdir(parents=True,exist_ok=True)
working = ['--ui-task-phase','converting','--ui-interactive-task']
cases = [
    ('idle',1120,700,[]),
    ('empty',720,628,['--ui-fixtures','0']),
    ('wide',1440,700,[]),
    ('narrow',720,628,[]),
    ('dark',1120,700,['--ui-appearance','dark']),
    ('converting',1120,700,working),
    ('narrow-converting',720,628,working),
    ('stop-click',1120,700,[*working,'--ui-click-stop']),
    ('scanning',720,628,['--ui-task-phase','scanning']),
    ('delivering',1120,700,['--ui-task-phase','delivering']),
    ('stopping',720,628,['--ui-task-phase','stopping']),
    ('failed',720,628,['--ui-task-phase','failed']),
    ('stopped',720,628,['--ui-task-phase','stopped']),
    ('completed',1120,700,['--ui-task-phase','converting','--ui-finish-task']),
    ('completed-expired',1120,700,['--ui-task-phase','converting','--ui-finish-task','--ui-completion-expiry']),
    ('sharing',720,628,['--ui-sharing-preview']),
    ('monitoring',720,628,['--ui-monitoring-preview']),
    ('reduced-motion',720,628,[*working,'--ui-reduce-motion']),
    ('interactions',1120,700,['--ui-check-first','--ui-animation-exercise']),
    ('interactions-reduced',720,628,['--ui-check-first','--ui-animation-exercise','--ui-reduce-motion']),
    ('resize',1120,700,['--ui-check-first','--ui-resize-check','--ui-fixtures','1000']),
]
for name,width,height,flags in cases:
    path = OUT / (name + '.png')
    result = subprocess.run([str(APP),'--ui-snapshot',str(path),'--ui-width',str(width),'--ui-height',str(height),*flags],
        capture_output=True,text=True,timeout=30)
    assert result.returncode == 0,(name,result.stdout,result.stderr)
    m = json.loads(Path(str(path) + '.json').read_text())
    frames = m['frames']
    primary = frames['primary-action']; header = frames['command-bar']
    assert m['primary_visible'] and m['queue_visible'],(name,m)
    assert primary['y'] >= header['y'] and primary['y'] + primary['height'] <= header['y'] + header['height'] + 1,(name,'primary action outside toolbar',primary,header)
    assert primary['x'] + primary['width'] <= m['frame_width'] + 1,(name,'primary action clipped')
    assert 'workspace-more-menu' in frames and 'task-status' not in frames,(name,'missing result menu or old footer remains')
    assert m['queue_height'] >= 240,(name,'too little photo space',m)
    busy = m['task_phase'] in ['converting','scanning','delivering','stopping'] or name == 'sharing'
    if busy:
        feedback = frames['task-feedback']; bar = frames['task-progress']; scopes = frames['queue-scopes']
        assert 56 <= feedback['height'] <= 68,(name,'progress panel height',feedback)
        assert feedback['y'] >= scopes['y'] + scopes['height'],(name,'progress above photo categories')
        assert bar['y'] >= feedback['y'] and bar['y'] + bar['height'] <= feedback['y'] + feedback['height'],(name,'progress bar clipped')
    elif name in ['idle','empty','wide','narrow','dark','completed-expired','interactions','interactions-reduced','resize']:
        assert 'task-feedback' not in frames,(name,'routine status consumes photo space')
    if name in ['scanning','delivering','sharing']:
        assert m['task_progress'] is None,(name,'unknown progress should not invent a percentage')
    if name == 'stop-click':
        assert m['task_phase'] == 'stopping' and not m['can_stop'],(name,'toolbar stop click did not reach model',m)
    if name in ['converting','narrow-converting','reduced-motion']:
        assert m['can_stop'] and m['task_progress'] == 0.375,(name,m)
    if name in ['failed','stopped','completed','monitoring']:
        assert 'task-feedback' in frames and 'task-progress' not in frames,(name,'lost result or recovery feedback',m)
    if '--ui-reduce-motion' in flags:
        assert m['motion_reduced'],(name,'reduce motion ignored')
    if m['row_count'] == 0:
        assert len(m['source_button_color']) == 3,(name,'first import should be prominent')
    else:
        assert not m['source_button_color'],(name,'secondary import is competing with conversion')
    if '--ui-animation-exercise' in flags:
        assert len(m['interaction_checks']) == 6 and m['selected_count'] == 1,(name,'continuous interaction lost state',m)
        assert not m['preview_visible'] and not m['detail_expanded'] and m['queue_scope'] == 'ready',(name,'toggle did not settle',m)
    for check in m['resize_checks']:
        assert check['selection_preserved'] and check['settings_preserved'] and check['primary_visible'] and check['queue_visible'],(name,check)
        assert check['selected_count'] == 1,(name,'resize lost checked photos',check)
    assert path.stat().st_size > 1000,name
    assert 'Unable to simultaneously satisfy constraints' not in result.stderr,(name,result.stderr)
    print('PASS',name,'toolbar, task feedback and stable interaction',flush=True)
print('PASS',len(cases),'workspace motion scenarios')
