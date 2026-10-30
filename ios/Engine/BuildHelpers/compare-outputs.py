#!/usr/bin/env python3
"""Compare corresponding raw media, and selected manifests, without normalizing bytes."""
import hashlib, json, pathlib, sys
mac, sim = map(pathlib.Path, sys.argv[1:3])
for rel in ['apple/motion.jpg.livephoto/motion.jpg', 'apple/motion.jpg.livephoto/motion.mov', 'android/motion.jpg.motionphoto/motion.MP.jpg']:
    a, b = (root.joinpath(rel).read_bytes() for root in (mac, sim))
    first = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))
    print(f'{rel}: mac bytes={len(a)} sha256={hashlib.sha256(a).hexdigest()} simulator bytes={len(b)} sha256={hashlib.sha256(b).hexdigest()} equal={a == b} first_difference={first if a != b else "none"}')
    if rel.endswith('.jpg') and a[:4] == b[:4] == b'\xff\xd8\xff\xe1':
        na, nb = int.from_bytes(a[4:6], 'big'), int.from_bytes(b[4:6], 'big')
        aa, bb = a[4 + na:], b[4 + nb:]
        differences = sum(x != y for x, y in zip(aa, bb)) + abs(len(aa) - len(bb))
        print(f'  first APP1 lengths: mac={na} simulator={nb}; following bytes unequal={differences}')
for rel in ['apple/motion.jpg.livephoto/manifest.json', 'android/motion.jpg.motionphoto/manifest.json']:
    a, b = (json.loads(root.joinpath(rel).read_text()) for root in (mac, sim))
    print(f'{rel}: changed manifest keys={sorted(k for k in set(a) | set(b) if a.get(k) != b.get(k))}')
    for key in ['identifier', 'source_sha256', 'paired_video_sha256', 'output_sha256', 'timing', 'hdr', 'warnings']:
        if key in a or key in b:
            print(f'  {key}: mac={json.dumps(a.get(key), ensure_ascii=False, sort_keys=True)} simulator={json.dumps(b.get(key), ensure_ascii=False, sort_keys=True)}')
