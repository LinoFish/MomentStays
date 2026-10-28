#!/usr/bin/env python3
"""Real Apple HDR payloads, synthesized pairing only; independent HEIF parser."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT))
from motionlive.jpeg import exif_identifier,mpf_images,image_hash
import motionlive.jpeg as jpeg_module
from motionlive.photo import xmp_packets,HDR,values
SOURCE=Path(os.environ.get('MOTIONLIVE_APPLE_REFERENCE', str(ROOT/'pictures/fixtures/apple-reference.HEIC')))
PAIR=Path(os.environ.get('MOTIONLIVE_HEIC_PAIR', str(ROOT/'pictures/fixtures/apple-pair')))

def boxes(d,a,z):
    result=[]
    while a<z:
        size,kind=struct.unpack_from('>I4s',d,a); h=8
        if size==1: size=struct.unpack_from('>Q',d,a+8)[0];h=16
        if size==0: size=z-a
        assert size>=h and a+size<=z
        result.append((kind,a,a+h,a+size));a+=size
    assert a==z
    return result

def layout(d):
    top=boxes(d,0,len(d));meta=next(b for b in top if b[0]==b'meta')
    children=boxes(d,meta[2]+4,meta[3]);info=next(b for b in children if b[0]==b'iinf')
    p=info[2]; types={}
    for _,_,q,z in boxes(d,p+4+(2 if d[p]==0 else 4),info[3]):
        width=2 if d[q]==2 else 4
        iid=int.from_bytes(d[q+4:q+4+width],'big');types[iid]=d[q+6+width:q+10+width]
    loc=next(b for b in children if b[0]==b'iloc');p=loc[2];version=d[p]
    ow,lw=d[p+4]>>4,d[p+4]&15; bw,iw=d[p+5]>>4,d[p+5]&15;p+=6
    def take(n):
        nonlocal p
        v=int.from_bytes(d[p:p+n],'big');p+=n;return v
    count=take(4 if version==2 else 2);items={};patch={}
    for _ in range(count):
        iid=take(4 if version==2 else 2);method=take(2) if version else 0
        assert take(2)==0
        base=take(bw);n=take(2);payload=b''
        for _ in range(n):
            assert take(iw)==0
            oa=p;offset=take(ow);la=p;length=take(lw)
            origin=next(b[2] for b in children if b[0]==b'idat') if method==1 else 0
            start=origin+base+offset;payload+=d[start:start+length]
            if n==1 and method==0:patch[iid]=(oa,ow,la,lw,base)
        items[iid]=(types[iid],payload)
    assert p==loc[3]
    props={k:d[a:z] for k,a,_,z in children if k in [b'iprp',b'pitm']}
    refs=[]
    for k,a,p,z in children:
        if k==b'iref':
            refs=[d[a:b] for k,a,_,b in boxes(d,p+4,z) if k!=b'cdsc']
    return items,props,refs,patch,children

def call(**request):
    p=subprocess.run([str(ROOT/'.build/gui/native-tests')],input=json.dumps(request),text=True,capture_output=True,timeout=180)
    assert p.returncode==0,(p.stdout,p.stderr)
    return json.loads(p.stdout)

assert SOURCE.exists() and PAIR.exists(),'Requires downloaded public reference and generated Samsung Apple-format pair'
original=SOURCE.read_bytes();digest=hashlib.sha256(original).hexdigest()
items,props,refs,patch,_=layout(original)
pair_manifest=json.loads((PAIR/'manifest.json').read_text())
identifier=pair_manifest['identifier']
pair_video=pair_manifest['output_files']['video']
with tempfile.TemporaryDirectory(prefix='motionlive-heic-reverse-') as folder:
    root=Path(folder);inputs=root/'input';inputs.mkdir()
    # Add only a content identifier so the unmodified public image/gain map
    # can use our existing timed MOV. This is not a real iPhone Live Photo.
    iid=next(i for i,(t,_) in items.items() if t==b'Exif');raw=items[iid][1]
    tiff=raw[4+int.from_bytes(raw[:4],'big'):]
    # Only the test pair's MakerNote is synthesized (ID + documented HDR
    # parameters); original HEVC base/gain data and image properties remain.
    def test_maker(value,old=b''):
        value=value.encode('ascii')+b'\0'
        at=56+len(value)
        entries=[struct.pack('>HHII',17,2,len(value),56),struct.pack('>HHII',33,10,1,at),struct.pack('>HHII',48,10,1,at+8)]
        return b'Apple iOS\0\0\1MM'+struct.pack('>H',3)+b''.join(entries)+b'\0'*4+value+struct.pack('>iiii',101,100,107729,400000)
    jpeg_module.maker_note=test_maker
    replacement=b'\0'*4+exif_identifier(b'Exif\0\0'+tiff,identifier)[6:]
    data=bytearray(original);oa,ow,la,lw,base=patch[iid]
    data[oa:oa+ow]=(len(data)+8-base).to_bytes(ow,'big');data[la:la+lw]=len(replacement).to_bytes(lw,'big')
    data+=struct.pack('>I4s',len(replacement)+8,b'mdat')+replacement
    source=inputs/'Apple HDR.HEIC';source.write_bytes(data)
    shutil.copyfile(PAIR/pair_video,inputs/'different-name.mov')
    for vendor in ['google','xiaomi','samsung']:
        out=root/vendor
        r=call(mode='batch',input=str(source),output=str(out),direction='to-android',vendor=vendor,container='heic')
        assert r['counts']['converted']==1 and r['counts']['failed']==0,json.loads(Path(r['report']).read_text())
        output=next(out.rglob('*.MP.heic'));payload=output.read_bytes()
        boundary=next(a for k,a,_,_ in boxes(payload,0,len(payload)) if k==b'mpvd')
        actual,ap,ar,_,children=layout(payload[:boundary])
        coded=lambda v:{i:value for i,value in v.items() if value[0] not in [b'Exif',b'mime']}
        assert coded(actual)==coded(items) and ap==props and ar==refs
        assert any(value[0]==b'mime' for value in actual.values())
        # A new metadata item must describe the primary image (cdsc).
        assert any(k==b'iref' and any(r[0]==b'cdsc' for r in boxes(payload,p+4,z)) for k,_,p,z in children)
        print('PASS real Apple HEIC coding, gain map, color/orientation and XMP association:',vendor)
    # Exercise adding a new primary XMP while retaining auxiliary XMP items.
    variant=bytearray(data)
    for kind,_,p,z in layout(variant)[4]:
        if kind==b'iref':
            width=2 if variant[p]==0 else 4
            for kind,_,q,z in boxes(variant,p+4,z):
                if kind==b'cdsc' and int.from_bytes(variant[q:q+width],'big')==51:
                    variant[q+width+2:q+width+2+width]=(52).to_bytes(width,'big')
    source.write_bytes(variant)
    out=root/'new-primary-xmp'
    result=call(mode='batch',input=str(source),output=str(out),direction='to-android',container='heic')
    assert result['counts']['converted']==1,json.loads(Path(result['report']).read_text())
    print('PASS add new primary XMP without replacing auxiliary metadata')
    source.write_bytes(data)
    encoded={}
    for quality,gain_quality in [(.35,.35),(.95,.95),(.35,.95),(.95,.35)]:
        dest=root/('hdr-'+str(quality)+'-'+str(gain_quality)+'.jpg')
        result=call(mode='android-still',input=str(source),output=str(dest),image_quality=quality,gain_quality=gain_quality)
        assert result['hdr']['status']=='converted',result
        images=mpf_images(dest.read_bytes());assert len(images)==2
        encoded[(quality,gain_quality)]=[image_hash(image) for image in images]
        metadata=[float(v) for _,xml in xmp_packets(images[1]) for v in values(xml,HDR,'GainMapMax')]
        assert len(metadata)==1 and abs(metadata[0]-(2.303-.303*.2693225))<.002,metadata
    assert (root/'hdr-0.35-0.35.jpg').stat().st_size < (root/'hdr-0.95-0.95.jpg').stat().st_size
    assert encoded[(.35,.95)][0]==encoded[(.35,.35)][0] != encoded[(.95,.95)][0]
    assert encoded[(.35,.95)][1]==encoded[(.95,.95)][1] != encoded[(.95,.35)][1]
    print('PASS real HEIC HDR → Ultra HDR model and independent JPEG quality controls')
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==digest
print('PASS public reference unchanged:',digest)
