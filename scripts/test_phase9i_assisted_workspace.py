#!/usr/bin/env python3
"""Exporter unit checks: naming, PNG storage and the unblinding audit.

Tiny fabricated PNGs only. Nothing here runs the Rust exporter, reads a
reference photograph or makes a network request.
"""
import json
from pathlib import Path
import struct
import tempfile
import unittest
import zlib

import phase9i_assisted_workspace as exporter


def png(path, width, height, color, samples, extra=None):
    """A minimal non-interlaced PNG; `extra` adds one ancillary chunk."""
    channels = {0: 1, 2: 3}[color]

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    stride = width * channels
    rows = b"".join(b"\0" + bytes(samples[y * stride:(y + 1) * stride]) for y in range(height))
    body = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, color, 0, 0, 0))
    if extra is not None:
        body += chunk(*extra)
    body += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    path.write_bytes(body)
    return body


def workspace_with(root, anonymous="0123456789ab", extra=None):
    """A complete two-file case plus manifest, as the exporter writes it."""
    workspace = Path(root) / "workspace"
    (workspace / "cases" / anonymous).mkdir(parents=True)
    (workspace / "replays").mkdir()
    directory = workspace / "cases" / anonymous
    png(directory / "source.png", 2, 2, 2, bytes(range(12)), extra)
    png(directory / "prior.png", 2, 2, 0, bytes([0, 128, 255, 7]))
    entry = exporter.case_entry(directory, anonymous)
    (directory / "case.json").write_text(json.dumps(entry, indent=2, sort_keys=True) + "\n")
    (workspace / "TASK-CARD.md").write_text("card\n")
    (workspace / "manifest.json").write_text(json.dumps(
        {"workspace_schema_version": 1, "protocol_id": exporter.PROTOCOL, "workspace_id": "w",
         "created_utc": "2026-09-15T00:00:00+00:00", "annotator_id": "tester",
         "budget": exporter.BUDGET, "task_card": "TASK-CARD.md", "task_card_sha256": "0" * 64,
         "cases": [entry]}, indent=2, sort_keys=True) + "\n")
    return workspace


class ExporterChecks(unittest.TestCase):
    def test_case_ids_parse_from_the_ends(self):
        self.assertEqual(exporter.parse_case("photo-0051115Q_000001_0003-light-blurred"),
                         ("photo", "0051115Q_000001_0003", "light", "blurred"))
        self.assertEqual(exporter.parse_case("full-portrait-0051115Q_000001_0060-checker-binary"),
                         ("full", "portrait-0051115Q_000001_0060", "checker", "binary"))
        for bad in ("photo-light-blurred", "sketch-x-light-blurred", "full-x-teal-blurred",
                    "full-x-light-sharp"):
            with self.assertRaises(ValueError):
                exporter.parse_case(bad)

    def test_completion_subset_is_the_declared_thirty_six(self):
        subset = exporter.rescore.ASSISTED_COMPLETION_SUBSET
        self.assertEqual(len(subset), 36)
        for case in subset:
            exporter.parse_case(case)

    def test_anonymous_ids_are_salted_and_stable(self):
        cases = ["photo-0051115Q_000001_0003-light-blurred", "full-fur-longhair-dark-binary"]
        first = exporter.anonymous_ids(cases, "salt")
        self.assertEqual(first, exporter.anonymous_ids(cases, "salt"))
        self.assertNotEqual(first, exporter.anonymous_ids(cases, "other"))
        self.assertEqual(sorted(len(value) for value in first.values()), [12, 12])
        for case, value in first.items():
            self.assertNotIn(exporter.parse_case(case)[1].lower(), value)

    def test_output_must_be_new_outside_the_repository_and_not_a_symlink(self):
        with self.assertRaises(ValueError):
            exporter.checked_output(exporter.ROOT / "must-not-create-workspace")
        with tempfile.TemporaryDirectory() as root:
            existing = Path(root) / "existing"
            existing.mkdir()
            with self.assertRaises(ValueError):
                exporter.checked_output(existing)
            link = Path(root) / "link"
            link.symlink_to(existing)
            with self.assertRaises(ValueError):
                exporter.checked_output(link)
            fresh = exporter.checked_output(Path(root) / "fresh")
            self.assertTrue(fresh.is_dir())

    def test_png_header_reports_storage_and_chunk_kinds(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "gray.png"
            data = png(path, 3, 2, 0, bytes(range(6)))
            self.assertEqual(exporter.png_header(data)[:4], (3, 2, 8, 0))
            self.assertEqual(exporter.png_header(data)[4], ["IHDR", "IDAT", "IEND"])
            with self.assertRaises(ValueError):
                exporter.png_header(b"not a png at all")

    def test_a_clean_workspace_passes_the_audit(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            exporter.audit_workspace(
                workspace, exporter.BASE_FORBIDDEN + ("photo-0051115Q_000001_0003-light-blurred",
                                                      "0051115Q_000001_0003"))

    def test_audit_refuses_an_extra_file_and_an_ancillary_chunk(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            (workspace / "notes.txt").write_text("left over")
            with self.assertRaisesRegex(ValueError, "unexpected workspace files"):
                exporter.audit_workspace(workspace, exporter.BASE_FORBIDDEN)
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root, extra=(b"tEXt", b"case\0photo-x-light-blurred"))
            with self.assertRaisesRegex(ValueError, "unexpected PNG chunks"):
                exporter.audit_workspace(workspace, exporter.BASE_FORBIDDEN)

    def test_audit_refuses_an_identifying_path_or_json_string(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root, anonymous="photo-0003")
            with self.assertRaisesRegex(ValueError, "names photo-"):
                exporter.audit_workspace(workspace, exporter.BASE_FORBIDDEN)
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            manifest = json.loads((workspace / "manifest.json").read_text())
            manifest["annotator_id"] = "0051115Q_000001_0003"
            (workspace / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True))
            with self.assertRaisesRegex(ValueError, "names 0051115q_000001_0003"):
                exporter.audit_workspace(workspace, exporter.BASE_FORBIDDEN + ("0051115Q_000001_0003",))

    def test_audit_refuses_the_wrong_png_storage(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            directory = workspace / "cases" / "0123456789ab"
            png(directory / "prior.png", 2, 2, 2, bytes(range(12)))
            with self.assertRaisesRegex(ValueError, "unexpected PNG storage"):
                exporter.audit_workspace(workspace, exporter.BASE_FORBIDDEN)

    def test_case_entry_hashes_the_decoded_planes(self):
        with tempfile.TemporaryDirectory() as root:
            workspace = workspace_with(root)
            entry = json.loads((workspace / "cases" / "0123456789ab" / "case.json").read_text())
            self.assertEqual(entry["width"], 2)
            self.assertEqual(entry["height"], 2)
            self.assertEqual(entry["profile"], exporter.PROFILE)
            for key in ("source_png_sha256", "source_plane_sha256",
                        "prior_png_sha256", "prior_plane_sha256"):
                self.assertRegex(entry[key], r"^[0-9a-f]{64}$")
            self.assertNotEqual(entry["source_png_sha256"], entry["source_plane_sha256"])


if __name__ == "__main__":
    unittest.main()
