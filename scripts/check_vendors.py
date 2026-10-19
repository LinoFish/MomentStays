#!/usr/bin/env python3
"""Vendor container, exact cover-frame and static-only/HDR regression checks."""
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from motionlive.jpeg import app, edit_header, image_hash, mpf_images, segments
from motionlive.photo import parse, live_still, XMP, RDF, HDR, CAMERA, q, xmp_packets, values
from tests.test_formats import motion, jpeg


def run(source, output):
    p = subprocess.run([str(ROOT/'.build/gui/native-tests')], input=json.dumps({
        'mode':'batch', 'input':str(source), 'output':str(output)}), text=True, capture_output=True, timeout=180)
    assert p.returncode == 0, (p.stdout,p.stderr)
    r = json.loads(p.stdout)
    return r, json.loads(Path(r['report']).read_text())


def pair(report):
    f = report['files'][0]
    assert f['status'] == 'converted', f
    directory = Path(f['output'])
    m = json.loads((directory/'manifest.json').read_text())
    still = (directory/m['output_files']['still']).read_bytes()
    images = mpf_images(still)
    # Every byte belongs to one complete JPEG image; no media/trailers/padding.
    assert len(still) == sum(map(len,images)) if images else len(still) == segments(still)[-1].end
    assert m['still_contains_embedded_media'] is False
    assert all(t['max_pts_shift_seconds'] == 0 for t in m['timing']['tracks'])
    return m, still


def pack(primary, gain):
    # A two-image MPF index. Test assembly is independent of native HDR writer.
    t = b'MM\0*\0\0\0\x08' + struct.pack('>H',3)
    t += struct.pack('>HHI4s',0xb000,7,4,b'0100')
    t += struct.pack('>HHII',0xb001,4,1,2)
    t += struct.pack('>HHII',0xb002,7,32,50) + b'\0'*4
    size = len(primary) + 90
    t += struct.pack('>IIIHH',0x20030000,size,0,0,0)
    t += struct.pack('>IIIHH',0,len(gain),size-10,0,0)
    return primary[:2] + app(0xe2,b'MPF\0'+t) + primary[2:] + gain


original = parse((ROOT/'pictures/sample-motion.MP.jpg').read_bytes())
clean = live_still(original.still,'VENDOR-TEST')
with tempfile.TemporaryDirectory(prefix='motionlive-vendors-',dir=ROOT/'.build') as folder:
    root = Path(folder)
    inputs = root/"inputs"; inputs.mkdir()
    for brand,ns,fields in [
        ('oppo','http://ns.oplus.com/photos/1.0/camera/',{'VideoLength':str(len(original.video)), 'OLivePhotoVersion':'2'}),
        ('vivo','http://ns.vivo.com/photos/1.0/camera/',{'VMotionPhotoVersion':'1','VMotionPhotoSource':'1'})]:
        trailer = b'PRIVATE_OPPO_TRAILER_AUDIO_VIDEO' if brand == 'oppo' else b''
        xml = ET.Element(q('adobe:ns:meta/','xmpmeta'))
        rdf = ET.SubElement(xml,q(RDF,'RDF'))
        ET.SubElement(rdf,q(RDF,'Description'),{q(ns,k):v for k,v in fields.items()})
        image = edit_header(clean,{},app(0xe1,XMP+ET.tostring(xml)))
        src=inputs/(brand+'.jpg');src.write_bytes(motion(image,str(original.timestamp_us),movie=original.video+trailer))
        result,report=run(src,root/(brand+'-out'));m,still=pair(report)
        assert m['source_format'] == ('oppo' if brand=='oppo' else 'vivo-v2')
        assert image_hash(still)==image_hash(clean) and trailer not in still if trailer else image_hash(still)==image_hash(clean)
        assert all(not values(x,ns,k) for _,x in xmp_packets(still) for k in fields)
        print('PASS',brand,'container and XMP cleanup, original PTS/cover and still-only bytes')

    src=inputs/'padding.jpg'
    src.write_bytes(motion(clean+b'EMBEDDED_AUDIO_VIDEO_SHOULD_NOT_SURVIVE',str(original.timestamp_us),movie=original.video))
    _,report=run(src,root/'padding-out');_,still=pair(report)
    assert b'EMBEDDED_AUDIO_VIDEO_SHOULD_NOT_SURVIVE' not in still
    assert not mpf_images(still) # SDR never gets a gain map.
    print('PASS extra bytes removed from SDR still; no gain map invented')

    xml=ET.Element(q('adobe:ns:meta/','xmpmeta'));rdf=ET.SubElement(xml,q(RDF,'RDF'))
    ET.SubElement(rdf,q(RDF,'Description'),{q(CAMERA,'MediaDataOffset'):str(len(original.video)),q(CAMERA,'MotionPhotoPresentationTimestampUs'):str(original.timestamp_us)})
    src=inputs/'media-offset.jpg';src.write_bytes(edit_header(clean,{},app(0xe1,XMP+ET.tostring(xml)))+original.video)
    _,report=run(src,root/'media-offset-out');m,still=pair(report)
    assert m['source_timestamp_us']==original.timestamp_us
    print('PASS MediaDataOffset without a Google enable flag, original timestamp preserved')

    # Independent SEF construction, including non-video sound-shot data before
    # the motion video. Neither record may leak into the JPEG output.
    audio_name=b'SoundShot_000';audio_bytes=b'RAW_AAC_AUDIO_MUST_NOT_LEAK'
    audio=struct.pack('<HHI',0,0x800,len(audio_name))+audio_name+audio_bytes
    tag=struct.pack('<HHI',0,0xa30,16)+b'MotionPhoto_Data'+original.video
    primary=motion(clean,str(original.timestamp_us),movie=original.video)[:-len(original.video)]
    table=b'SEFH'+struct.pack('<II',107,2)+struct.pack('<HHII',0,0x800,len(audio)+len(tag),len(audio))+struct.pack('<HHII',0,0xa30,len(tag),len(tag))
    source=primary+audio+tag+table+struct.pack('<I',len(table))+b'SEFT'
    src=inputs/'samsung-audio.jpg';src.write_bytes(source)
    _,report=run(src,root/'samsung-audio-out');m,still=pair(report)
    assert m['source_format']=='samsung-sef' and audio_bytes not in still and b'SEFT' not in still
    print('PASS Samsung SEF video+embedded-audio records removed from JPEG, paired MOV retained')
    bad=bytearray(source);struct.pack_into('<I',bad,len(source)-8-len(table)+16,0xffffffff)
    src=inputs/'bad-sef.jpg';src.write_bytes(bad)
    result,report=run(src,root/'bad-sef-out')
    assert result['counts']['failed']==1 and 'SEF' in report['files'][0]['error'],report
    print('PASS Samsung SEF out-of-bounds directory rejected')

    iso_header=b'urn:iso:std:iso:ts:21496:-1\0'
    # v0, base color space, one channel; headrooms 0/1 and 2/1;
    # gain min 0, max 2, gamma 1, offsets 0, all denominator 1.
    metadata=struct.pack('>HHB',0,0,64)+struct.pack('>14I',0,1,2,1,0,1,2,1,1,1,0,1,0,1)
    gain=jpeg()[:2]+app(0xe2,iso_header+metadata)+jpeg()[2:]
    for label,raw in [('iso',gain),('iso-invalid',gain.replace(metadata,metadata[:-4]+b'\0'*4)),('opaque',jpeg())]:
        src=inputs/(label+'.jpg');src.write_bytes(motion(pack(clean,raw),str(original.timestamp_us),movie=original.video))
        _,report=run(src,root/(label+'-out'));m,still=pair(report)
        assert image_hash(still)==image_hash(clean)
        output_gain=mpf_images(still)[1]
        assert image_hash(output_gain)==image_hash(raw)
        if label=='iso':
            assert m['hdr']['status']=='converted' and m['hdr']['output']=='ultra-hdr+iso',m['hdr']
            assert values(xmp_packets(output_gain)[0][1],HDR,'GainMapMax')==['2.0']
            assert iso_header+metadata in output_gain
        else:
            assert m['hdr']['status']!='converted' and output_gain==raw,m['hdr']
        print('PASS',label,'gain-map metadata policy; both JPEG codestreams retained')

    # Mutated Huawei tail must not fall back to a guessed timestamp.
    honor=next((ROOT/'pictures/honor').glob('*.jpg')).read_bytes()
    tail=bytearray(honor[-60:]);tail[:20]=b'v2_f999999'.ljust(20,b' ')
    src=inputs/'bad-frame.jpg';src.write_bytes(honor[:-60]+tail)
    result,report=run(src,root/'bad-frame-out')
    assert result['counts']['failed']==1 and '帧号超出' in report['files'][0]['error'],report
    print('PASS invalid Huawei/Honor cover frame rejected; no guessed time')

for src in sorted((ROOT/'pictures/honor').glob('*.jpg')):
    _,report=run(src,ROOT/'.build/honor-private-hdr/vendor-output')
    # Allow resume on repeated checks.
    row=report['files'][0];m=json.loads((Path(row['output'])/'manifest.json').read_text())
    still=(Path(row['output'])/m['output_files']['still']).read_bytes()
    assert m['source_cover_frame_index'] in [31,36]
    assert m['timing']['cover_seconds'] > 0
    if src.name == 'sample-private-hdr.jpg':
        assert m['hdr']['input']=='honor-private-ehdr' and m['hdr']['status']=='unsupported-preserved'
        assert still[segments(still)[-1].end:] == src.read_bytes()[3441337:3497410]
        print('PASS Honor private HDR extension preserved with explicit unsupported conversion warning')
        continue
    assert m['hdr']['input']=='ultra-hdr' and m['hdr']['status']=='preserved'
    assert mpf_images(still)[1:]==mpf_images(src.read_bytes())[1:]
    assert sum(map(len,mpf_images(still)))==len(still)
    print('PASS Honor original',src.name,'source frame',m['source_cover_frame_index'],'Ultra HDR preserved, no appended media')
print('PASS vendor and static-only/HDR regressions')
