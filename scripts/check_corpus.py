#!/usr/bin/env python3
"""Independently verify the current corpus, including still-only JPEG boundaries."""
import hashlib
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.jpeg import image_hash,mpf_images,segments
from motionlive.photo import parse

result=json.loads((ROOT/'.build/corpus-1.1.0-result.json').read_text())
report=json.loads(Path(result['report']).read_text())
assert report['counts']=={'converted':136,'resumed':0,'failed':0,'skipped':3},report['counts']
originals=json.loads((ROOT/'.build/source-sha256-1.1.0.json').read_text())
for name,digest in originals.items():
    assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest()==digest,name
maximum=0
for row in report['files']:
    if row['status']!='converted':continue
    source,pair=Path(row['source']),Path(row['output'])
    m=json.loads((pair/'manifest.json').read_text());before=source.read_bytes()
    after=(pair/m['output_files']['still']).read_bytes()
    assert m['implementation'] in ['gui-native-1.1.0','gui-native-1.1.1'] and m['still_contains_embedded_media'] is False
    assert m['source_sha256']==hashlib.sha256(before).hexdigest()
    assert image_hash(before)==image_hash(after)==m['primary_coding_sha256']
    assert mpf_images(before)[1:]==mpf_images(after)[1:]
    images=mpf_images(after)
    assert len(after)==(sum(map(len,images)) if images else segments(after)[-1].end),source
    assert m['output_files']['still']==source.name
    for name,digest in m['output_sha256'].items():
        assert hashlib.sha256((pair/name).read_bytes()).hexdigest()==digest
    t=m['timing'];maximum=max(maximum,t['snap_error_seconds'])
    assert t['snap_error_seconds']<=t['source_frame_duration_seconds']/2+.000002
    assert all(x['max_pts_shift_seconds']==0 for x in t['tracks'])
    if m['source_format']=='huawei-honor':
        assert m['source_cover_frame_index'] in [31,36]
        assert t['timing_policy']=='source-huawei-frame'
    elif m['source_timestamp_tag']=='MP4:motionphoto-image-meta':
        assert m['source_timestamp_us']==parse(source.with_name(source.name.replace('(1)','')).read_bytes()).timestamp_us
    else:
        assert m['source_timestamp_us']==parse(before).timestamp_us
print('PASS',len(originals),'source hashes unchanged')
print('PASS 136 pairs: JPEG coding, gain maps, still-only image boundaries, filenames, output hashes, exact source timestamps/frame references and zero media PTS shifts')
print('Max cover sample alignment error:',maximum,'seconds')
