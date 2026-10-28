#!/usr/bin/env python3
"""Targeted Google→Honor contract checks; private media/results stay ignored locally."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from tests.test_formats import motion
from motionlive.photo import parse, xmp_packets, CAMERA, ITEM, XMP, q
from motionlive.jpeg import app, edit_header, image_hash, segments, mpf_images
BINARY = Path(os.environ.get('MOTIONLIVE_TEST_BINARY', str(ROOT / '.build/gui/native-tests')))

def call(**request):
    p = subprocess.run([str(BINARY)], input=json.dumps(request), text=True, capture_output=True, timeout=180)
    assert p.returncode == 0, (p.stdout, p.stderr)
    return json.loads(p.stdout)

def mutate(data, transform):
    packet, xml = xmp_packets(data)[0]
    transform(xml)
    return edit_header(data, {packet.start: app(0xE1, XMP + ET.tostring(xml))})

def standard(still, clip, time, legacy=False, element=False):
    raw = motion(still, str(time), legacy=legacy, element=element, movie=clip)
    return mutate(raw, lambda xml: [node.set(q(ITEM, 'Mime'), 'video/mp4' if (node.get(q(ITEM, 'Semantic')) or node.findtext(q(ITEM, 'Semantic'))) == 'MotionPhoto' else 'image/jpeg') for node in xml.iter() if node.get(q(ITEM, 'Semantic')) or node.findtext(q(ITEM, 'Semantic'))])

def batch(source, output, **options):
    return call(mode='batch', input=str(source), output=str(output), direction='android-to-android', **options)

def report(result):
    return json.loads(Path(result['report']).read_text()) if result['report'] else result['report_data']

def identical(source, output):
    a, b = parse(source.read_bytes()), parse(output.read_bytes())
    assert a.video == b.video and a.timestamp_us == b.timestamp_us
    assert image_hash(a.still) == image_hash(b.still)
    assert mpf_images(a.still)[1:] == mpf_images(b.still)[1:]
    assert [s.payload for s in segments(a.still) if s.marker == 225 and s.payload.startswith(b'Exif\0\0')] == [s.payload for s in segments(b.still) if s.marker == 225 and s.payload.startswith(b'Exif\0\0')]
    assert b'MicroVideo' not in output.read_bytes() and b'LIVE_' not in output.read_bytes()[-60:]

with tempfile.TemporaryDirectory(prefix='motionlive-android-transfer-') as temp:
    root = Path(temp)
    fixture = root / 'fixture'
    info = call(mode='fixture', folder=str(fixture), audio=True)
    still, clip = (fixture/'still.jpg').read_bytes(), (fixture/'clip.mp4').read_bytes()
    valid = standard(still, clip, info['cover_us'])
    source = fixture/'modern.jpg'; source.write_bytes(valid)
    initial_hash = hashlib.sha256(valid).hexdigest()
    for label, raw in [('modern',valid),('legacy',standard(still,clip,info['cover_us'],legacy=True)),('element',standard(still,clip,info['cover_us'],element=True))]:
        src = fixture/(label+'.jpg'); src.write_bytes(raw)
        out = root/(label+'-out')
        result = batch(src,out)
        assert result['counts']['converted'] == 1, report(result)
        identical(src,next(out.rglob('*.MP.jpg')))
        assert batch(src,out)['counts']['resumed'] == 1
    both = mutate(valid,lambda xml: next(n for n in xml.iter() if n.get(q(CAMERA,'MotionPhoto'))).attrib.update({q(CAMERA,'MicroVideo'):'1',q(CAMERA,'MicroVideoOffset'):str(len(clip)),q(CAMERA,'MicroVideoPresentationTimestampUs'):str(info['cover_us'])}))
    both_src=fixture/'both.jpg'; both_src.write_bytes(both)
    assert batch(both_src,root/'both-out')['counts']['converted'] == 1
    def custom_metadata(xml):
        desc = next(n for n in xml.iter() if n.get(q(CAMERA,'MotionPhoto')))
        tags = ET.SubElement(desc,'{https://example.invalid/metadata/}Tags')
        sequence = ET.SubElement(tags,'{http://www.w3.org/1999/02/22-rdf-syntax-ns#}Seq')
        ET.SubElement(sequence,'{http://www.w3.org/1999/02/22-rdf-syntax-ns#}li').text = 'preserve array item'
    custom=fixture/'metadata.jpg'; custom.write_bytes(mutate(valid,custom_metadata))
    custom_out=root/'metadata-out'
    assert batch(custom,custom_out)['counts']['converted']==1
    assert b'preserve array item' in next(custom_out.rglob('*.MP.jpg')).read_bytes()
    # A valid gain-map source exercises preservation, rather than silently stripping HDR.
    hdr_fixture=root/'hdr-fixture'; hdr_info=call(mode='fixture',folder=str(hdr_fixture),legacy_hdr=True)
    hdr_still=hdr_fixture/'ultra.jpg'
    call(mode='android-still',input=str(hdr_fixture/'still.jpg'),output=str(hdr_still))
    hdr_input=hdr_fixture/'raw.jpg'
    hdr_input.write_bytes(motion(hdr_still.read_bytes(),str(hdr_info['cover_us']),movie=(hdr_fixture/'clip.mp4').read_bytes()))
    hdr_source=hdr_fixture/'standard.jpg'
    call(mode='android-assemble',input=str(hdr_input),output=str(hdr_source),vendor='google',time_us=hdr_info['cover_us'])
    hdr_out=root/'hdr-out'; hdr_result=batch(hdr_source,hdr_out)
    assert hdr_result['counts']['converted']==1,report(hdr_result)
    identical(hdr_source,next(hdr_out.rglob('*.MP.jpg')))
    assert len(mpf_images(next(hdr_out.rglob('*.MP.jpg')).read_bytes()))==2
    cases = {
        'missing': standard(still,clip,info['cover_us']).replace(b'MotionPhotoPresentationTimestampUs',b'UnrecognizedPresentationTimestampUs'),
        'outside': standard(still,clip,99999999999),
        'conflict': mutate(both,lambda xml: next(n for n in xml.iter() if n.get(q(CAMERA,'MicroVideo'))).set(q(CAMERA,'MicroVideoPresentationTimestampUs'),'1')),
        'offset': mutate(both,lambda xml: next(n for n in xml.iter() if n.get(q(CAMERA,'MicroVideo'))).set(q(CAMERA,'MicroVideoOffset'),'16')),
        'truncated': valid[:-100],
        'extra-tail': valid+b'undocumented',
        'gap': valid[:-len(clip)]+b'unknown'+clip,
        'other-vendor': mutate(valid,lambda xml: next(n for n in xml.iter() if n.get(q(CAMERA,'MotionPhoto'))).set('{http://ns.oplus.com/photos/1.0/camera/}VideoLength',str(len(clip)))),
        'unknown-item': valid.replace(b'MotionPhoto"',b'UnknownItem"'),
        'heic': b'\0\0\0\x18ftypheic'+b'\0'*32,
        'unknown-version': mutate(valid,lambda xml: next(n for n in xml.iter() if n.get(q(CAMERA,'MotionPhoto'))).set(q(CAMERA,'MotionPhotoVersion'),'9')),
        'unknown-directory': mutate(valid,lambda xml: next(n for n in xml.iter() if n.get(q(ITEM,'Semantic'))=='MotionPhoto').set(q(ITEM,'Unrecognized'),'value')),
        'unknown-root': mutate(valid,lambda xml: ET.SubElement(xml,'{https://example.invalid/metadata/}OutsideResource').__setattr__('text','must not disappear')),
    }
    for label,raw in cases.items():
        src=fixture/(label+'.jpg'); src.write_bytes(raw)
        result=batch(src,root/(label+'-out'))
        assert result['counts']['failed']==1 and result['counts']['converted']==0, (label,report(result))
        assert not list((root/(label+'-out')).rglob('*.MP.jpg'))
    # Existing outputs remain intact on skip, cancel and failed overwrite.
    out=root/'existing'; batch(source,out)
    published=next(out.rglob('*.MP.jpg')); before=published.read_bytes()
    assert batch(source,out,existing_output='skip')['counts']['skipped']==1
    assert batch(source,out,existing_output='cancel')['cancelled']
    assert published.read_bytes()==before
    assert batch(source,out,existing_output='overwrite')['counts']['converted']==1
    before=published.read_bytes()
    source.write_bytes(cases['missing'])
    assert batch(source,out,existing_output='overwrite')['counts']['failed']==1
    assert published.read_bytes()==before
    source.write_bytes(valid)
    # Duplicate sources, partial failure, static classification, cancellation, no JSON.
    mixed=call(mode='batch',inputs=[str(source),str(source),str(fixture/'missing.jpg'),str(fixture/'still.jpg')],output=str(root/'mixed'),direction='android-to-android',concurrency=1)
    assert mixed['counts']=={'converted':1,'resumed':0,'skipped':1,'failed':1},report(mixed)
    cancelled=call(mode='batch',inputs=[str(source),str(both_src)],output=str(root/'cancelled'),direction='android-to-android',cancel_after=1,concurrency=1)
    assert cancelled['cancelled'] and cancelled['counts']['converted']==1,cancelled
    nojson=batch(source,root/'nojson',export_data_files=False)
    assert nojson['counts']['converted']==1 and not list((root/'nojson').rglob('*.json'))
    assert hashlib.sha256(source.read_bytes()).hexdigest()==initial_hash
    scan=call(mode='scan',inputs=[str(source),str(fixture/'missing.jpg')],direction='android-to-android')['rows']
    assert sum(r['motion'] for r in scan)==1,scan
    # Optional real Google fixtures: compare implementation to the established A semantics.
    private = ROOT/'.build/honor-google-metadata/validation.json'
    if private.exists():
        records=json.loads(private.read_text())
        for i, entry in enumerate(r for r in records if '-A-' in r['output']):
            src=ROOT/entry['source']
            if not src.exists(): continue
            result=batch(src,root/f'real-{i}')
            assert result['counts']['converted']==1,report(result)
            identical(src,next((root/f'real-{i}').rglob('*.MP.jpg')))
print('PASS Google→Honor: modern/legacy/dual/element, bytes/EXIF/time, rejection, resume/overwrite/cancel/dedupe/static/report, local real fixtures')
