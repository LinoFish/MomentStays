#!/usr/bin/env python3
"""Real user Apple originals/edits: final JPEG XMP and byte layout, not flags alone."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.jpeg import mpf_images,segments
from motionlive.photo import xmp_packets,HDR,CONTAINER,ITEM,values
source=Path(os.environ.get('MOTIONLIVE_APPLE_SAMPLES', str(ROOT/'pictures/apple')))
out=ROOT/'.build/apple-hdr-debug/fixed'
inputs={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in source.rglob('*') if p.is_file()}

def call(**req):
 p=subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps(req),text=True,capture_output=True,timeout=300)
 assert p.returncode==0,(p.stdout,p.stderr)
 return json.loads(p.stdout)

r=call(mode='batch',input=str(source),output=str(out),direction='to-android',existing_output='overwrite')
report=json.loads(Path(r['report']).read_text());assert r['counts']['converted']==4 and r['counts']['failed']==0,report
for item in report['files']:
 name=Path(item['source']).stem;edited=name.startswith('IMG_E')
 folder=Path(item['output']);file=next(folder.glob('*.MP.jpg'));raw=file.read_bytes()
 packets=xmp_packets(raw);assert len(packets)==1,(name,len(packets))
 xml=packets[0][1];dirs=list(xml.iter('{'+CONTAINER+'}Directory'));assert len(dirs)==1
 media=[n for n in dirs[0].iter() if n.tag=='{'+CONTAINER+'}Item']
 semantics=[n.attrib['{'+ITEM+'}Semantic'] for n in media]
 expected=['Primary','MotionPhoto'] if edited else ['Primary','GainMap','MotionPhoto']
 assert semantics==expected,(name,semantics)
 images=mpf_images(raw);assert len(images)==(0 if edited else 2)
 assert values(xml,HDR,'Version')==([] if edited else ['1.0'])
 assert item['hdr']['status']==('sdr' if edited else 'converted'),item['hdr']
 p=segments(raw)[-1].end
 for n in media[1:]:
  length=int(n.attrib['{'+ITEM+'}Length'])
  if n.attrib['{'+ITEM+'}Semantic']=='GainMap':assert raw[p:p+length]==images[1]
  else:assert raw[p+4:p+8]==b'ftyp' and p+length==len(raw)
  p+=length
 assert p==len(raw)
 if not edited:
  assert (images[0]+images[1])==raw[:sum(map(len,images))]
 manifest=json.loads((folder/'manifest.json').read_text());assert manifest['implementation']=='reverse-native-1.2.1'
 assert all(t['max_pts_shift_seconds']<.000002 for t in item['timing']['tracks'])
 # Simulate old release's sidecar: do not silently resume its faulty output.
 manifest['implementation']='reverse-native-1.2.0';(folder/'manifest.json').write_text(json.dumps(manifest))
 print('PASS real Apple',name,'SDR (no source map)' if edited else 'Ultra HDR, single XMP, 3-item directory', 'cover',item['timing']['cover_timestamp_us'])
again=call(mode='batch',input=str(source),output=str(out),direction='to-android',existing_output='overwrite')
assert again['counts']['converted']==4 and again['counts']['resumed']==0,again
resume=call(mode='batch',input=str(source),output=str(out),direction='to-android')
assert resume['counts']['resumed']==4,resume
assert inputs=={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in source.rglob('*') if p.is_file()}
(ROOT/'.build/apple-hdr-debug/input-sha256.json').write_text(json.dumps(inputs,indent=2))
print('PASS old output upgrade / current resume, original files unchanged')
