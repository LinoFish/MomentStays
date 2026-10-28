#!/usr/bin/env python3
"""Optional data export keeps verification; vendor detection uses evidence."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.jpeg import image_hash
from tests.test_formats import motion


def call(request):
    p=subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps(request),text=True,capture_output=True,timeout=180)
    assert p.returncode==0,(p.stdout,p.stderr)
    return json.loads(p.stdout)


def batch(source,output,**options):
    r=call({'mode':'batch','input':str(source),'output':str(output),**options})
    data=r.get('report_data') if r['report'] is None else json.loads(Path(r['report']).read_text())
    return r,data


def snapshot(folder):
    return {str(p.relative_to(folder)):hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.rglob('*') if p.is_file()}


source=ROOT/'pictures/sample-motion.MP.jpg'
with tempfile.TemporaryDirectory(prefix='motionlive-data-',dir=ROOT/'.build') as directory:
    root=Path(directory);out=root/'media-only'
    r,data=batch(source,out,export_data_files=False)
    assert r['report'] is None and r['counts']['converted']==1,data
    pair=Path(data['files'][0]['output'])
    assert {p.name for p in pair.iterdir()}=={source.name,source.stem+'.mov'}
    assert not list(out.rglob('*.json'))
    assert image_hash((pair/source.name).read_bytes())==image_hash(source.read_bytes())
    assert all(t['max_pts_shift_seconds']==0 for t in data['files'][0]['timing']['tracks'])
    old=snapshot(pair)
    r,data=batch(source,out,export_data_files=False,existing_output='skip')
    assert r['counts']['skipped']==1 and r['counts']['failed']==0 and snapshot(pair)==old,data
    r,data=batch(source,out,export_data_files=False,existing_output='overwrite',video_track='secondary')
    assert r['counts']['failed']==1 and snapshot(pair)==old,data
    r,data=batch(source,out,export_data_files=False,existing_output='overwrite')
    assert r['counts']['converted']==1 and not list(out.rglob('*.json')),data
    print('PASS media-only output: no JSON; timestamps/coding verified; skip and failed-overwrite protection retained')
    regular=root/'regular'
    r,data=batch(source,regular)
    assert r['counts']['converted']==1 and Path(r['report']).is_file()
    target=Path(data['files'][0]['output']);assert (target/'manifest.json').is_file()
    json_before=set(regular.glob('report-*.json'))
    r,data=batch(source,regular,export_data_files=False,existing_output='overwrite')
    assert r['counts']['converted']==1 and not (target/'manifest.json').exists()
    assert set(regular.glob('report-*.json'))==json_before
    print('PASS default exports manifest/report; disabling removes replaced pair sidecar but does not delete older root reports')
    auxiliary=ROOT/'pictures/sample-sparse.MP.jpg'
    r,data=batch(auxiliary,root/'aux',export_data_files=False,output_mode='auxiliary-only')
    assert r['counts']['converted']==1 and not list((root/'aux').rglob('*.json')),data
    assert len(list(Path(data['files'][0]['output']).iterdir()))==1
    print('PASS auxiliary-only export without any data files; in-memory report available')
    inputs=root/'inputs';inputs.mkdir()
    unknown=inputs/'unknown.jpg';unknown.write_bytes(motion())
    scan=call({'mode':'scan','input':str(unknown)})['rows'][0]
    assert scan['motion'] and scan['manufacturer']=='未知',scan
    assert 'Google 兼容格式不代表 Google 厂商' in scan['manufacturer_detail']
    print('PASS Google-compatible format without EXIF Make remains unknown, not misidentified as Google')
for directory,expected,count in [('xiaomi','小米',3),('honor','荣耀',2)]:
    rows=call({'mode':'scan','input':str(ROOT/'pictures'/directory)})['rows']
    assert len(rows)==count and all(r['manufacturer']==expected and 'EXIF Make' in r['manufacturer_detail'] for r in rows),rows
    print('PASS',directory,'real originals:',count,'EXIF-based manufacturer identifications')
print('PASS optional data files and manufacturer detection')
