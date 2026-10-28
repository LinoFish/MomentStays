#!/usr/bin/env python3
"""Existing-output decisions and transactional replacement failure protection."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(source, output, policy=None, **options):
    request = {'mode':'batch','input':str(source),'output':str(output),**options}
    if policy is not None:
        request['existing_output'] = policy
    p = subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps(request),text=True,capture_output=True,timeout=180)
    assert p.returncode == 0,(p.stdout,p.stderr)
    result = json.loads(p.stdout)
    return result,json.loads(Path(result['report']).read_text())


def snapshot(folder):
    return {str(p.relative_to(folder)):hashlib.sha256(p.read_bytes()).hexdigest()
            for p in folder.rglob('*') if p.is_file()}


with tempfile.TemporaryDirectory(prefix='motionlive-existing-',dir=ROOT/'.build') as directory:
    root=Path(directory);src=root/'sources';src.mkdir();out=root/'out'
    source=src/'照片.MP.jpg'
    original=(ROOT/'pictures/sample-motion.MP.jpg').read_bytes()
    source.write_bytes(original)
    result,_=run(source,out);assert result['counts']['converted']==1
    pair=out/(source.name+'.livephoto')
    initial=snapshot(pair);old=json.loads((pair/'manifest.json').read_text())
    for settings in [{},{'video_track':'secondary'}]:
        result,report=run(source,out,'skip',**settings)
        assert result['counts']['skipped']==1 and result['counts']['failed']==0,report
        assert snapshot(pair)==initial
    print('PASS skip existing output regardless of changed settings; no failure or byte changes')
    result,report=run(source,out,'cancel')
    assert result['cancelled'] and not any(result['counts'].values()) and snapshot(pair)==initial,report
    print('PASS cancel decision stops batch without changing existing files')
    result,report=run(source,out,'overwrite',video_track='secondary')
    assert result['counts']['failed']==1 and snapshot(pair)==initial,report
    assert not list(out.glob('.motionlive-*'))
    print('PASS failed replacement leaves entire old result intact and removes staging')
    result,report=run(source,out,'overwrite')
    assert result['counts']['converted']==1 and result['counts']['failed']==0,report
    fresh=json.loads((pair/'manifest.json').read_text())
    assert fresh['identifier']!=old['identifier']
    assert fresh['primary_coding_sha256']==old['primary_coding_sha256']
    assert fresh['timing']['cover_seconds']==old['timing']['cover_seconds']
    assert not list(out.glob('.motionlive-*'))
    print('PASS explicit overwrite regenerates and verifies new pair, then atomically replaces old pair')
    (pair/'manifest.json').write_text('damaged')
    damaged=snapshot(pair)
    result,report=run(source,out,'skip')
    assert result['counts']['skipped']==1 and result['counts']['failed']==0 and snapshot(pair)==damaged,report
    result,report=run(source,out,'overwrite')
    assert result['counts']['converted']==1 and result['counts']['failed']==0,report
    assert json.loads((pair/'manifest.json').read_text())['source_sha256']==hashlib.sha256(original).hexdigest()
    print('PASS corrupt or incomplete prior manifest can be skipped or replaced without blocking conversion')
    # A different export layout used to fail before conversion. It now follows
    # the explicit overwrite choice as well, including auxiliary-only outputs.
    auxiliary=src/'aux.jpg';auxiliary.write_bytes((ROOT/'pictures/sample-sparse.MP.jpg').read_bytes())
    result,_=run(auxiliary,out);assert result['counts']['converted']==1
    result,report=run(auxiliary,out,'overwrite',output_mode='livephoto+auxiliary')
    assert result['counts']['converted']==1,report
    combined=out/(auxiliary.name+'.livephoto')
    m=json.loads((combined/'manifest.json').read_text())
    assert set(m['output_files'])=={'still','video','auxiliary'}
    result,_=run(auxiliary,out,output_mode='auxiliary-only');assert result['counts']['converted']==1
    result,report=run(auxiliary,out,'overwrite',output_mode='auxiliary-only')
    assert result['counts']['converted']==1,report
    assert source.read_bytes()==original
    print('PASS mode change and auxiliary-only replacement; source bytes unchanged')
print('PASS all existing-output decision checks')
