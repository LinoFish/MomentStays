#!/usr/bin/env python3
"""Selected video, sparse secondary rejection and independent auxiliary export."""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.photo import parse, live_still
from motionlive.jpeg import image_hash
from tests.test_formats import motion


def boxes(data,start=0,end=None):
    end=len(data) if end is None else end
    while start<end:
        size,kind=struct.unpack_from('>I4s',data,start);header=8
        if size==1:size=struct.unpack_from('>Q',data,start+8)[0];header=16
        elif size==0:size=end-start
        assert header<=size<=end-start
        yield kind,start,start+header,start+size
        start+=size


def dual_video(data):
    moov=next(b for b in boxes(data) if b[0]==b'moov')
    assert moov[3]==len(data), 'Fixture expects moov after mdat, so chunk offsets remain unchanged'
    duplicate=None
    for kind,start,payload,end in boxes(data,moov[2],moov[3]):
        if kind!=b'trak':continue
        mdia=next(b for b in boxes(data,payload,end) if b[0]==b'mdia')
        hdlr=next(b for b in boxes(data,mdia[2],mdia[3]) if b[0]==b'hdlr')
        if data[hdlr[2]+8:hdlr[2]+12]!=b'vide':continue
        duplicate=bytearray(data[start:end])
        tkhd=next(b for b in boxes(duplicate,8,len(duplicate)) if b[0]==b'tkhd')
        at=tkhd[2]+(20 if duplicate[tkhd[2]]==1 else 12)
        struct.pack_into('>I',duplicate,at,99)
        break
    assert duplicate
    body=bytearray(data[moov[2]:moov[3]])
    mvhd=next(b for b in boxes(body) if b[0]==b'mvhd')
    struct.pack_into('>I',body,mvhd[3]-4,100)
    new=struct.pack('>I4s',len(body)+len(duplicate)+8,b'moov')+body+duplicate
    return data[:moov[1]]+new


def run(source,output,**settings):
    p=subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps({'mode':'batch','input':str(source),'output':str(output),**settings}),text=True,capture_output=True,timeout=180)
    assert p.returncode==0,(p.stdout,p.stderr)
    result=json.loads(p.stdout)
    report=json.loads(Path(result['report']).read_text())
    return result,report


def manifest(report):
    return json.loads((Path(report['files'][0]['output'])/'manifest.json').read_text())


sparse=ROOT/'pictures/sample-sparse.MP.jpg'
original=ROOT/'pictures/sample-motion.MP.jpg'
photo=parse(original.read_bytes())
with tempfile.TemporaryDirectory(prefix='motionlive-aux-',dir=ROOT/'.build') as folder:
    root=Path(folder)
    result,report=run(sparse,root/'aux-only',output_mode='auxiliary-only')
    assert result['counts']['converted']==1,report
    m=manifest(report)
    assert set(m['output_files'])=={'auxiliary'} and len(m['output_sha256'])==1
    assert m['auxiliary_video']['frames']==1 and m['auxiliary_video']['width']==2048
    assert m['auxiliary_video']['audio_included'] is False and m['auxiliary_video']['max_pts_shift_seconds']==0
    assert run(sparse,root/'aux-only',output_mode='auxiliary-only')[0]['counts']['resumed']==1
    print('PASS single-frame 2048px auxiliary-only export and verified resume')
    zero=ROOT/'pictures/sample-zero-duration.MP.jpg'
    result,report=run(zero,root/'zero-default',output_mode='auxiliary-only')
    assert result['counts']['failed']==1,report
    result,report=run(zero,root/'zero-repaired',output_mode='auxiliary-only',repair_auxiliary_duration=True)
    assert result['counts']['converted']==1,report
    m=manifest(report);info=m['auxiliary_video'];repair=info['duration_repair']
    assert info['frames']==1 and info['max_pts_shift_seconds']==0
    assert repair['original_duration_seconds']==0 and repair['repaired_duration_seconds']>0
    assert repair['original_sample_sha256']==info['compressed_sha256']
    print('PASS zero-duration sample only repaired with explicit opt-in; exact compressed frame and PTS retained')
    result,report=run(sparse,root/'both',output_mode='livephoto+auxiliary')
    assert result['counts']['converted']==1,report
    m=manifest(report)
    assert set(m['output_files'])=={'still','video','auxiliary'} and len(m['output_sha256'])==3
    assert m['timing']['selected_video_role']=='primary'
    assert image_hash((Path(report['files'][0]['output'])/m['output_files']['still']).read_bytes())==image_hash(sparse.read_bytes())
    assert run(sparse,root/'both',output_mode='livephoto+auxiliary')[0]['counts']['resumed']==1
    print('PASS primary Live Photo plus separate auxiliary MOV; still coding preserved')
    result,report=run(sparse,root/'sparse',video_track='secondary')
    assert result['counts']['failed']==1 and '只有 1 帧' in report['files'][0]['error'],report
    assert not list((root/'sparse').glob('*.livephoto'))
    print('PASS refuses single-frame secondary as Live Photo; no fabricated motion')
    src=root/'synthetic';src.mkdir()
    clean=live_still(photo.still,'FIXTURE')
    dual=dual_video(photo.video)
    valid=src/'dual.jpg';valid.write_bytes(motion(clean,str(photo.timestamp_us),movie=dual))
    result,report=run(valid,root/'selected',video_track='secondary')
    assert result['counts']['converted']==1,report
    m=manifest(report)
    assert m['timing']['selected_source_track_id']==99 and m['timing']['selected_video_role']=='secondary'
    assert m['timing']['selected_frame_count']>2 and m['source_timestamp_us']==photo.timestamp_us
    assert m['timing']['tracks'][0]['max_pts_shift_seconds']==0
    print('PASS continuous secondary selected by role with original cover and unchanged media PTS')
    invalid=src/'outside.jpg';invalid.write_bytes(motion(clean,'999999999',movie=dual))
    result,report=run(invalid,root/'outside',video_track='secondary')
    assert result['counts']['failed']==1 and '辅助轨无法对应原始封面时间' in report['files'][0]['error'],report
    print('PASS rejects auxiliary cover outside its timeline, without falling back to primary')
    for settings in [{'video_track':'secondary'},{'output_mode':'auxiliary-only'}]:
        result,report=run(original,root/('missing-'+str(len(settings))+str(settings.get('video_track','export'))),**settings)
        assert result['counts']['failed']==1 and '辅助' in report['files'][0]['error'],report
    print('PASS missing secondary reported for both selection and export')
print('PASS all auxiliary video checks')
