#!/usr/bin/env python3
"""Ownership checks for verified-file publication, without network access."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys
sys.dont_write_bytecode = True
import phase6_full_reference_fixtures as fixture

class FullReferenceChecks(unittest.TestCase):

    def test_exclusive_publish_preserves_existing_file_and_cleans_own_temp(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'owned.png'
            path.write_bytes(b'existing')
            with self.assertRaises(FileExistsError):
                fixture.publish(path, b'replacement')
            self.assertEqual(path.read_bytes(), b'existing')
            self.assertEqual([p.name for p in Path(root).iterdir()], ['owned.png'])

    def test_bad_download_never_publishes_unverified_reference(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / 'asset.png'
            with patch.object(fixture.urllib.request, 'urlopen', return_value=io.BytesIO(b'<form>accept</form>')):
                with self.assertRaises(ValueError):
                    fixture.acquire_file(path, 'https://example.invalid/', 4, hashlib.sha256(b'good').hexdigest(), verify_only=False)
            self.assertEqual(list(Path(root).iterdir()), [])
    def test_existing_wrong_pin_is_preserved_without_network(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "asset.png"
            path.write_bytes(b"wrong")
            with patch.object(fixture.urllib.request, "urlopen") as opened:
                with self.assertRaises(ValueError):
                    fixture.acquire_file(path, "https://example.invalid/", 4, "0" * 64, verify_only=True)
                opened.assert_not_called()
            self.assertEqual(path.read_bytes(), b"wrong")

    def test_exact_manifest_rejects_geometry_and_physical_truth_substitution(self):
        for change in ["dimensions", "transparency_reference", "reference_origin"]:
            with self.subTest(change=change), tempfile.TemporaryDirectory() as root:
                manifest = fixture.manifest()
                glass = next(row for row in manifest["references"] if row["id"] == "glass-bottle")
                replacements = {
                    "dimensions": [1, 1],
                    "transparency_reference": True,
                    "reference_origin": "physically measured alpha and foreground",
                }
                glass[change] = replacements[change]
                path = Path(root) / "full-references.json"
                modified = json.dumps(manifest, indent=2, sort_keys=True) + "\n"
                path.write_text(modified)
                with patch.object(fixture.urllib.request, "urlopen") as opened:
                    with self.assertRaisesRegex(ValueError, "pinned contract"):
                        fixture.acquire(root, verify_only=True)
                    opened.assert_not_called()
                self.assertEqual(path.read_text(), modified)

    def test_repository_output_is_refused_before_creation(self):
        target = fixture.portraits.ROOT / "must-not-create-full-reference"
        with self.assertRaisesRegex(ValueError, "outside the repository"):
            fixture.acquire(target, verify_only=True)
        self.assertFalse(target.exists())


if __name__ == '__main__':
    unittest.main()
