#!/usr/bin/env python3
"""Input/ownership checks use tiny fabricated bytes and make no network requests."""
import hashlib
import io
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import phase6_reference_fixtures as fixture

class FixtureChecks(unittest.TestCase):
    def test_repository_output_refused(self):
        with self.assertRaises(ValueError):
            fixture.checked_root(fixture.ROOT / "must-not-create-photo-fixture")

    def test_bad_checksum_and_storage_refused(self):
        with tempfile.TemporaryDirectory() as root:
            path=Path(root)/"fake.png"
            raw=b"\x89PNG\r\n\x1a\n"+struct.pack(">I",13)+b"IHDR"+struct.pack(">IIBBBBB",1,1,8,6,0,0,0)
            path.write_bytes(raw)
            with self.assertRaisesRegex(ValueError,"checksum"):
                fixture.verify_reference(path,"0"*64,len(raw))
            with self.assertRaisesRegex(ValueError,"dimensions"):
                fixture.verify_reference(path,hashlib.sha256(raw).hexdigest(),len(raw))

    def test_failed_exclusive_download_keeps_existing_partial(self):
        with tempfile.TemporaryDirectory() as root:
            archive=Path(root)/"PhotoMatte85.zip"
            partial=archive.with_suffix(".part")
            partial.write_bytes(b"owned by another interrupted download")
            with patch.object(fixture.urllib.request,"urlopen",return_value=io.BytesIO(b"PKtest")):
                with self.assertRaises(FileExistsError): fixture.download(archive)
            self.assertEqual(partial.read_bytes(),b"owned by another interrupted download")
            self.assertFalse(archive.exists())

    def test_changed_permission_source_refused_before_archive_download(self):
        with tempfile.TemporaryDirectory() as root:
            permission=Path(root)/"permission-source.js"
            permission.write_bytes(b"changed licensing")
            with patch.object(fixture.urllib.request,"urlopen") as opened:
                with self.assertRaisesRegex(ValueError,"permission source has changed"):
                    fixture.acquire(root,verify_only=True)
                opened.assert_not_called()
            self.assertEqual(permission.read_bytes(),b"changed licensing")

    def test_legal_or_login_form_is_not_accepted(self):
        with tempfile.TemporaryDirectory() as root:
            with patch.object(fixture.urllib.request,"urlopen",return_value=io.BytesIO(b"<form>Accept license terms</form>")) as opened:
                with self.assertRaisesRegex(ValueError,"no login, captcha or terms"):
                    fixture.download(Path(root)/"archive.zip")
                self.assertEqual(opened.call_count,1)

if __name__=="__main__": unittest.main()
