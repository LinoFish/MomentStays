import math
import struct
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

from motionlive.hdr import APPLE, PIXEL, legacy_apple
from motionlive.jpeg import FormatError, app
from motionlive.photo import RDF, XMP, q
from tests.test_formats import jpeg


def legacy_fixture(version="65536", with_headroom=True, denominator=10):
    # The actual ImageIO encoder writes these scalar fields as signed rationals.
    note = b"Apple iOS\0\0\x01MM" + struct.pack(">H", 2)
    note += struct.pack(">HHII", 33, 10, 1, 44) + struct.pack(">HHII", 48, 10, 1, 52)
    note += b"\0" * 4 + struct.pack(">iiii", 1, 2, 1, denominator)
    raw = b"MM\0*\0\0\0\x08" + struct.pack(">HHHII", 1, 0x8769, 4, 1, 26) + b"\0" * 4
    raw += struct.pack(">HHHII", 1, 0x927C, 7, len(note), 44) + b"\0" * 4 + note
    primary = jpeg()
    if with_headroom:
        primary = primary[:2] + app(0xE1, b"Exif\0\0" + raw) + primary[2:]
    root = ET.Element(q("adobe:ns:meta/", "xmpmeta"))
    rdf = ET.SubElement(root, q(RDF, "RDF"))
    ET.SubElement(rdf, q(RDF, "Description"), {
        q(PIXEL, "AuxiliaryImageType"): "urn:com:apple:photo:2020:aux:hdrgainmap",
        q(APPLE, "HDRGainMapVersion"): version})
    gain = jpeg()[:2] + app(0xE1, XMP + ET.tostring(root)) + jpeg()[2:]
    return primary, gain


class HDRTests(unittest.TestCase):
    def test_sdr_never_gets_gain_map(self):
        info, reason = legacy_apple(jpeg())
        self.assertIsNone(info)
        self.assertIn("SDR is not expanded", reason)

    def test_existing_rational_headroom(self):
        primary, gain = legacy_fixture()
        with patch("motionlive.hdr.mpf_images", return_value=[primary, gain]):
            info, _ = legacy_apple(primary)
        self.assertEqual(info["gain"], gain)
        self.assertAlmostEqual(info["stops"], 1.5909)

    def test_missing_headroom_not_guessed(self):
        primary, gain = legacy_fixture(with_headroom=False)
        with patch("motionlive.hdr.mpf_images", return_value=[primary, gain]):
            info, reason = legacy_apple(primary)
        self.assertIsNone(info)
        self.assertIn("33/48", reason)

    def test_unknown_gain_map_version_not_converted(self):
        primary, gain = legacy_fixture(version="131072")
        with patch("motionlive.hdr.mpf_images", return_value=[primary, gain]):
            info, reason = legacy_apple(primary)
        self.assertIsNone(info)
        self.assertIn("Unsupported", reason)

    def test_bad_headroom_rejected(self):
        primary, gain = legacy_fixture(denominator=0)
        with patch("motionlive.hdr.mpf_images", return_value=[primary, gain]):
            with self.assertRaises(FormatError):
                legacy_apple(primary)


if __name__ == "__main__":
    unittest.main()
