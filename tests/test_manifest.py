import json
from pathlib import Path
import tempfile
import unittest

from motionlive.cli import verify_pair
from motionlive.jpeg import FormatError


class ManifestTests(unittest.TestCase):
    def test_existing_empty_manifest_is_reported_as_format_error(self):
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / "manifest.json").write_text("{}")
            with self.assertRaises(FormatError):
                verify_pair(folder)

    def test_manifest_cannot_omit_both_output_files(self):
        with tempfile.TemporaryDirectory() as folder:
            (Path(folder) / "manifest.json").write_text(json.dumps({"output_sha256": {}, "timing": {}, "hdr": {}}))
            with self.assertRaises(FormatError):
                verify_pair(folder)
