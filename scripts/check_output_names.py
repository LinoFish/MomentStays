#!/usr/bin/env python3
"""Output-name preservation, safe legacy migration and collision rollback."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parent.parent


def run(source, output):
    p = subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps({'mode':'batch','input':str(source),'output':str(output)}),text=True,capture_output=True,timeout=180)
    assert p.returncode == 0, (p.stdout,p.stderr)
    result = json.loads(p.stdout)
    return result, json.loads(Path(result['report']).read_text())


with tempfile.TemporaryDirectory(prefix='motionlive-names-',dir=ROOT/'.build') as folder:
    root = Path(folder)
    src = root/'sources';src.mkdir()
    source = src/'原始 照片.MP.JPEG'
    source.write_bytes(next((ROOT/'pictures').glob('*.jpg')).read_bytes())
    result, _ = run(source,root/'out')
    assert result['counts']['converted'] == 1, result
    pair = root/'out'/(source.name+'.livephoto')
    manifest = json.loads((pair/'manifest.json').read_text())
    names = {'still':source.name,'video':source.stem+'.mov'}
    assert manifest['output_files'] == names
    assert manifest['original_filename'] == source.name
    assert set(manifest['output_sha256']) == set(names.values())
    assert all((pair/name).is_file() for name in names.values())
    assert run(source,root/'out')[0]['counts']['resumed'] == 1
    print('PASS original filename, uppercase JPEG, Unicode/spaces, matching MOV and resume')
    # Downgrade just the naming/schema to simulate a verified 1.0.3 output.
    legacy_root = root/'legacy';legacy_root.mkdir()
    legacy = legacy_root/pair.name;shutil.copytree(pair,legacy)
    old = dict(manifest);old.pop('output_files');old.pop('original_filename')
    old['implementation']='gui-native-1.0.3';old['version']='1.0.3'
    old['output_sha256']={'photo.jpg':manifest['output_sha256'][names['still']], 'photo.mov':manifest['output_sha256'][names['video']]}
    for key, before in [('still','photo.jpg'),('video','photo.mov')]:
        (legacy/names[key]).rename(legacy/before)
    (legacy/'manifest.json').write_text(json.dumps(old))
    # Keep an independent copy for the collision test.
    collision_root=root/'collision';collision_root.mkdir()
    collision=collision_root/pair.name;shutil.copytree(legacy,collision)
    result, report=run(source,legacy_root)
    assert result['counts']['resumed']==1 and result['counts']['failed']==0,report
    updated=json.loads((legacy/'manifest.json').read_text())
    assert updated['output_files']==names and updated['implementation']=='gui-native-1.1.4'
    assert updated['timing']==old['timing'] and updated['identifier']==old['identifier']
    for key,name in names.items():
        assert hashlib.sha256((legacy/name).read_bytes()).hexdigest()==manifest['output_sha256'][name]
    print('PASS legacy rename only: exact JPEG/MOV bytes, pairing ID and timestamp retained')
    (collision/names['still']).write_bytes(b'user file')
    result,_=run(source,collision_root)
    assert result['counts']['failed']==1
    assert (collision/names['still']).read_bytes()==b'user file'
    assert json.loads((collision/'manifest.json').read_text())==old
    assert (collision/'photo.jpg').is_file() and (collision/'photo.mov').is_file()
    print('PASS name collision leaves entire old pair and user file intact')
    # Manifests cannot escape the output folder via renamed keys.
    invalid_root=root/'invalid';invalid_root.mkdir()
    invalid=invalid_root/pair.name;shutil.copytree(pair,invalid)
    malicious=dict(manifest);malicious['output_files']={'still':'../outside.jpg','video':names['video']}
    malicious['output_sha256']={'../outside.jpg':manifest['output_sha256'][names['still']], names['video']:manifest['output_sha256'][names['video']]}
    (invalid/'manifest.json').write_text(json.dumps(malicious))
    assert run(source,invalid_root)[0]['counts']['failed']==1
    print('PASS invalid manifest filenames refused')
print('PASS all output naming checks')
