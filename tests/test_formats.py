import hashlib
import struct
import unittest
import uuid
import xml.etree.ElementTree as ET

from motionlive.jpeg import FormatError, TIFF, app, exif_identifier, image_hash, maker_note, mpf_images, segments
from motionlive.photo import CAMERA, CONTAINER, HDR, ITEM, RDF, XMP, live_still, parse, q, ultra_from_apple, xmp_packets


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
    from motionlive.jpeg import edit_header
    return edit_header(still, {}, app(0xE1, XMP + ET.tostring(root))) + movie


class FormatTests(unittest.TestCase):
    def test_jpeg_entropy_false_eoi(self):
        data = jpeg() + video()
        self.assertEqual(segments(data)[-1].end, len(jpeg()))

    def test_truncated_jpeg(self):
        with self.assertRaises(FormatError):
            segments(jpeg()[:-2])

    def test_modern_attribute_xmp(self):
        result = parse(motion())
        self.assertEqual(result.timestamp_us, 400000)
        self.assertEqual(result.video, video())

    def test_element_xmp(self):
        self.assertEqual(parse(motion(element=True)).timestamp_us, 400000)

    def test_microvideo(self):
        result = parse(motion(timestamp="1234567", legacy=True))
        self.assertEqual(result.timestamp_us, 1234567)
        self.assertEqual(result.timestamp_tag, "MicroVideoPresentationTimestampUs")

    def test_missing_negative_time(self):
        self.assertIsNone(parse(motion(timestamp=None)).timestamp_us)
        self.assertIsNone(parse(motion(timestamp="-1")).timestamp_us)

    def test_malformed_timestamp(self):
        with self.assertRaises(FormatError):
            parse(motion(timestamp="NaN"))

    def test_motion_flag_zero(self):
        data = motion().replace(b'GCamera:MotionPhoto="1"', b'GCamera:MotionPhoto="0"')
        with self.assertRaises(FormatError):
            parse(data)

    def test_bad_video_offset(self):
        data = motion().replace(str(len(video())).encode(), b"999999")
        with self.assertRaises(FormatError):
            parse(data)

    def test_live_still_primary_bitstream(self):
        original = parse(motion()).still
        output = live_still(original, str(uuid.uuid4()).upper())
        self.assertEqual(image_hash(original), image_hash(output))
        for _, root in xmp_packets(output):
            self.assertFalse(any(k.startswith("{" + CAMERA + "}") for e in root.iter() for k in e.attrib))
            self.assertFalse(any(e.get(q(ITEM, "Semantic")) == "MotionPhoto" for e in root.iter()))

    def test_element_motion_items_removed(self):
        output = live_still(parse(motion(element=True)).still, str(uuid.uuid4()))
        self.assertNotIn(b">MotionPhoto<", output)

    def test_unrelated_google_metadata_survives(self):
        from motionlive.jpeg import edit_header
        original = parse(motion()).still
        segment, root = xmp_packets(original)[0]
        root.find(".//" + q(RDF, "Description")).set(q(CAMERA, "SpecialTypeID"), "preserve-me")
        modified = edit_header(original, {segment.start: app(0xE1, XMP + ET.tostring(root))})
        output = live_still(modified, "ONE")
        self.assertIn(b'GCamera:SpecialTypeID="preserve-me"', output)

    def test_ultra_gainmap_and_mpf_preserved(self):
        original = ultra_from_apple(jpeg(), jpeg(), 2.0)
        original_gain = mpf_images(original)[1]
        result = parse(motion(original))
        self.assertEqual(result.hdr, "ultra-hdr")
        output = live_still(result.still, str(uuid.uuid4()).upper())
        self.assertEqual(mpf_images(output)[1], original_gain)
        self.assertEqual(image_hash(output), image_hash(original))

    def test_generated_ultra_metadata(self):
        data = ultra_from_apple(jpeg(), jpeg(), 1.8)
        images = mpf_images(data)
        self.assertEqual(len(images), 2)
        self.assertEqual(len(images[0]) + len(images[1]), len(data))
        self.assertEqual(image_hash(images[1]), image_hash(jpeg()))
        text = ET.tostring(xmp_packets(images[1])[0][1])
        self.assertIn(b'OffsetSDR="0"', text)

    def test_false_ultra_without_mpf_rejected(self):
        raw = motion().replace(b'GCamera:MotionPhoto="1"',
            b'xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/" hdrgm:Version="1.0" GCamera:MotionPhoto="1"')
        with self.assertRaises(FormatError):
            parse(raw)

    def test_apple_makernote_retains_hdr_float_tags(self):
        # Existing Apple float tags 33/48 plus an external value.
        entries = (struct.pack(">HHI", 33, 11, 1) + struct.pack(">f", 0.5)
                   + struct.pack(">HHI", 48, 11, 1) + struct.pack(">f", 0.1)
                   + struct.pack(">HHII", 99, 7, 8, 56))
        old = b"Apple iOS\0\0\x01MM" + struct.pack(">H", 3) + entries + b"\0" * 4 + b"12345678"
        note = maker_note("ABC", old)
        n = struct.unpack_from(">H", note, 14)[0]
        self.assertEqual(n, 4)
        fields = {struct.unpack_from(">H", note, 16 + i * 12)[0]: note[16 + i * 12:28 + i * 12] for i in range(n)}
        self.assertEqual(fields[33][-4:], struct.pack(">f", 0.5))
        offset = struct.unpack(">I", fields[99][-4:])[0]
        self.assertEqual(note[offset:offset + 8], b"12345678")

    def test_original_exif_values_retained(self):
        first = exif_identifier(None, "FIRST")
        second = exif_identifier(first, "SECOND")
        self.assertIn(b"SECOND\0", second)
        self.assertTrue(second[6:6 + len(first) - 6][8:] == first[14:])

    def test_mpf_auxiliary_hash_invariant_after_second_edit(self):
        original = ultra_from_apple(jpeg(), jpeg(), 2)
        once = live_still(original, "ONE")
        twice = live_still(once, "TWO")
        self.assertEqual(hashlib.sha256(mpf_images(original)[1]).digest(), hashlib.sha256(mpf_images(twice)[1]).digest())


if __name__ == "__main__":
    unittest.main()
