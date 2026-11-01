"""Synthetic Motion Photo fixtures for the macOS check scripts.

These builders generate small, privacy-free inputs. They are test data only and
are not a converter implementation.
"""
import hashlib
import struct
import xml.etree.ElementTree as ET

from .jpeg import app, edit_header, segments
from .photo import CAMERA, CONTAINER, EXTENDED, ITEM, NOTE, RDF, XMP, q, xmp_packets


def jpeg():
    # Two progressive-like scans, stuffed bytes and restart markers. Structural fixture.
    return (b"\xff\xd8" + app(0xDB, b"\0" * 65) + app(0xC2, b"\x08\0\x10\0\x10\x01\x01\x11\0")
            + app(0xDA, b"\x01\x01\0\0\0\0") + b"\x12\xff\0\xd9\xff\xd0\x34"
            + app(0xDA, b"\x01\x01\0\x01\x3f\0") + b"\x56\x78\xff\xd9")


def video():
    def box(kind, payload):
        return struct.pack(">I", len(payload) + 8) + kind + payload
    return box(b"ftyp", b"isom\0\0\0\0isom") + box(b"moov", b"") + box(b"mdat", b"\xff\xd9decoy")


def motion(still=None, timestamp="400000", legacy=False, element=False, container=True, movie=None):
    still = jpeg() if still is None else still
    movie = video() if movie is None else movie
    root = ET.Element(q("adobe:ns:meta/", "xmpmeta"))
    rdf = ET.SubElement(root, q(RDF, "RDF"))
    desc = ET.SubElement(rdf, q(RDF, "Description"))
    fields = {"MicroVideo" if legacy else "MotionPhoto": "1"}
    if timestamp is not None:
        fields["MicroVideoPresentationTimestampUs" if legacy else "MotionPhotoPresentationTimestampUs"] = timestamp
    if legacy or not container:
        fields["MicroVideoOffset"] = str(len(movie))
    for key, value in fields.items():
        if element:
            ET.SubElement(desc, q(CAMERA, key)).text = value
        else:
            desc.set(q(CAMERA, key), value)
    if container and not legacy:
        seq = ET.SubElement(ET.SubElement(desc, q(CONTAINER, "Directory")), q(RDF, "Seq"))
        for semantic, length in (("Primary", 0), ("MotionPhoto", len(movie))):
            li = ET.SubElement(seq, q(RDF, "li"))
            item = ET.SubElement(li, q(CONTAINER, "Item"))
            for key, value in (("Semantic", semantic), ("Length", str(length))):
                if element:
                    ET.SubElement(item, q(ITEM, key)).text = value
                else:
                    item.set(q(ITEM, key), value)
    # Use header editor so fixture MPF offsets are valid too.
    return edit_header(still, {}, app(0xE1, XMP + ET.tostring(root))) + movie


def extended(motion_in_extension=False, reverse=False):
    source = motion()
    segment, root = xmp_packets(source)[0]
    if motion_in_extension:
        extension = root
        root = ET.Element(q('adobe:ns:meta/', 'xmpmeta'))
        desc = ET.SubElement(ET.SubElement(root, q(RDF, 'RDF')), q(RDF, 'Description'))
    else:
        desc = root.find('.//' + q(RDF, 'Description'))
        extension = ET.Element(q('adobe:ns:meta/', 'xmpmeta'))
    ET.SubElement(extension, '{urn:motionlive:test}private').text = '保留' * 18000
    xml = ET.tostring(extension, encoding='utf-8')
    guid = hashlib.md5(xml).hexdigest().upper()
    desc.set(q(NOTE, 'HasExtendedXMP'), guid)
    chunks = [app(0xE1, EXTENDED + guid.encode() + struct.pack('>II', len(xml), offset) + xml[offset:offset + 30000])
              for offset in range(0, len(xml), 30000)]
    if reverse:
        chunks.reverse()
    return edit_header(source, {segment.start: app(0xE1, XMP + ET.tostring(root)) + b''.join(chunks)})


def extension_payloads(data):
    return [s.payload for s in segments(data) if s.payload.startswith(EXTENDED)]
