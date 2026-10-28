#!/usr/bin/env python3
"""Focused third-direction layout checks using only synthetic UI fixtures."""
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parent.parent
APP=ROOT/'.build/gui/preview/留刻.app/Contents/MacOS/MomentStays'
OUT=ROOT/'.build/gui/android-transfer-ui'
OUT.mkdir(parents=True,exist_ok=True)
cases=[('narrow',720,628,[]),('wide',1440,700,[]),('settings',720,628,['--ui-expand-settings']),('share',720,628,['--ui-destination','folderAndShare','--ui-output-menu']),('source',720,628,['--ui-source-menu']),('old-forward',720,628,[]),('old-reverse',1040,628,[])]
for name,width,height,flags in cases:
    direction='to-apple' if name=='old-forward' else 'to-android' if name=='old-reverse' else 'android-to-android'
    path=OUT/(name+'.png')
    result=subprocess.run([str(APP),'--ui-snapshot',str(path),'--direction',direction,'--ui-width',str(width),'--ui-height',str(height),*flags],capture_output=True,text=True,timeout=40)
    assert result.returncode==0,(name,result.stdout,result.stderr)
    metrics=json.loads(Path(str(path)+'.json').read_text())
    assert metrics['file_menu_titles'],(name,'missing file menu diagnostic')
    assert any('扫描系统照片图库' in title for title in metrics['file_menu_titles']) == (direction != 'android-to-android'),(name,metrics['file_menu_titles'])
    for control in ['conversion-directions','settings-button','output-control','add-button','primary-action']:
        frame=metrics['frames'][control]
        assert frame['x']>=0 and frame['x']+frame['width']<=metrics['frame_width']+1,(name,control,'clipped',frame)
    if name=='settings': assert metrics['settings_sheet_visible']
    if name=='source': assert not any('图库' in title for title in metrics['source_menu_titles']),metrics['source_menu_titles']
    if name=='share':
        assert metrics['output_destination']=='folderAndShare'
        assert not any('图库' in title for title in metrics['output_menu_titles']),metrics['output_menu_titles']
    print('PASS',name,flush=True)
