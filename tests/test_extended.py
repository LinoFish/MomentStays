"""Extended XMP regressions shared with the native format checks."""
import hashlib
import struct
import unittest
import xml.etree.ElementTree as ET
from motionlive.jpeg import FormatError, app, edit_header, image_hash, segments
from motionlive.photo import EXTENDED, NOTE, RDF, XMP, CAMERA, live_still, parse, q, xmp_packets
from tests.test_formats import motion


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


class ExtendedTests(unittest.TestCase):
    def test_unmodified_extension_identical(self):
        source = extended(reverse=True)
        output = live_still(parse(source).still, 'TEST')
        self.assertEqual(extension_payloads(source), extension_payloads(output))
        self.assertEqual(image_hash(source), image_hash(output))
        self.assertEqual(parse(source).timestamp_us, 400000)

    def test_motion_in_multichunk_extension(self):
        source = extended(True, True)
        output = live_still(parse(source).still, 'TEST')
        packets = xmp_packets(output)  # verifies re-chunked GUID and main reference
        self.assertEqual(parse(source).timestamp_us, 400000)
        self.assertEqual(image_hash(source), image_hash(output))
        self.assertTrue(any('保留' * 18000 in (e.text or '') for _, root in packets for e in root.iter()))
        self.assertFalse(any(k == q(CAMERA, 'MotionPhoto') for _, root in packets for e in root.iter() for k in e.attrib))

    def test_bad_checksum(self):
        source = extended().replace(b'private>', b'privatf>', 1)
        with self.assertRaises(FormatError):
            parse(source)

    def test_missing_chunk(self):
        source = extended()
        chunk = next(s for s in segments(source) if s.payload.startswith(EXTENDED))
        with self.assertRaises(FormatError):
            parse(edit_header(source, {chunk.start: b''}))

    def test_duplicate_chunk(self):
        source = extended()
        chunk = next(s for s in segments(source) if s.payload.startswith(EXTENDED))
        with self.assertRaises(FormatError):
            parse(edit_header(source, {chunk.start: app(0xE1, chunk.payload) * 2}))
