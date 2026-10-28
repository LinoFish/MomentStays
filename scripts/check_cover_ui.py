#!/usr/bin/env python3
"""Native candidate preview and both multi-photo settings, synthetic media only."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from scripts.check_cover_candidates import call, dual
from tests.test_formats import motion

APP = ROOT / '.build/gui/preview/MomentStays.app/Contents/MacOS/MomentStays'
OUTPUT = ROOT / '.build/gui/cover-ui'
OUTPUT.mkdir(parents=True,exist_ok=True)

with tempfile.TemporaryDirectory(prefix='cover-ui-') as temporary:
    folder = Path(temporary)
    info = call(dict(mode='fixture',folder=str(folder)))
    source = folder / 'dual.jpg'
    source.write_bytes(motion((folder/'still.jpg').read_bytes(),str(info['cover_us']),movie=dual((folder/'clip.mp4').read_bytes())))
    for name, delivery, width, flags in [('mixed','static',1120,[]),('all-live','livephoto',1120,[]),('compact','livephoto',390,['--ui-expand-settings'])]:
        path = OUTPUT / (name + '.png')
        cover_flags = ['--ui-cover-index','0'] if width >= 960 else []
        result = subprocess.run([str(APP),'--ui-snapshot',str(path),'--ui-width',str(width),'--ui-height','700',
            '--ui-motion-source',str(source),'--ui-candidate-delivery',delivery,*cover_flags,*flags],capture_output=True,text=True,timeout=30)
        assert result.returncode == 0,(result.stdout,result.stderr)
        metrics = json.loads(Path(str(path)+'.json').read_text())
        if width >= 960:
            assert metrics['candidate_count']==1 and metrics['candidate_image_loaded'],metrics
            assert abs(metrics['preview_cover']-.2)<2e-6,metrics
        else:
            assert metrics['settings_sheet_visible'],metrics
        assert metrics['primary_visible'] and metrics['queue_visible'],metrics
        assert path.stat().st_size>1000
        assert 'Unable to simultaneously satisfy constraints' not in result.stderr,result.stderr
        print('PASS',name,'candidate preview and cover time' if width >= 960 else 'compact export settings',metrics['preview_cover'],flush=True)
