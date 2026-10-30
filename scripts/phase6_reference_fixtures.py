#!/usr/bin/env python3
"""Acquire pinned PhotoMatte85 portrait/hair references outside the repository.

The official UW dataset page permits commercial/noncommercial use, requests
academic citation and asks commercial-product users to fill a survey. This
local evaluation neither embeds the dataset in the app nor submits that form.
Images remain under their dataset permission; no photographs are committed.
References are supplied green-screen-derived foreground/alpha, not perfect
physical measurements. Fur/glass/product photographic ground truth is absent.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DATASET_ID = "1KpHKYW986Dax9-ZIM7I-HyBoWVcLPuaQ"
ARCHIVE_SHA256 = "1d3bd0291c41668238596fa0a5a1f5da51ee213eeba26595e21d54621570ff64"
ARCHIVE_BYTES = 413_386_865
PERMISSION_PAGE = "https://grail.cs.washington.edu/projects/background-matting-v2/#/datasets"
PERMISSION_ASSET = "https://grail.cs.washington.edu/projects/background-matting-v2/static/js/main.7263c829.chunk.js"
PERMISSION_SHA256 = "58cb834cb99c6ff7d80528f616204039dcb6d1b7d5926c6b05b328feea91121a"
PERMISSION_BYTES = 42461
REFERENCES = [
    ("0051115Q_000001_0003.png", "9c5f5e0f469ee178d6cdc26ec2274daef2793ef6c8579628e8a9113ff5a9a8d1", 5_139_354),
    ("0051115Q_000001_0031.png", "7e0a553b9358d83a997a771852437ee1a4b24bf0732dbe790f44680a9217f338", 5_136_253),
    ("0051115Q_000001_0060.png", "0cf4107447ad9b13d7c124d6fc8fd9ae5c37cc24d4381af331a71d5209e4dd53", 3_802_076),
    ("0051115Q_000001_0088.png", "6553eac7cea799e82de43c1e054ab84d36ad410dbce4905966c66159431b72d8", 5_227_616),
]


def sha256(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for part in iter(lambda: source.read(1024 * 1024), b""):
            value.update(part)
    return value.hexdigest()


def checked_root(directory):
    directory = Path(directory).resolve()
    if directory == ROOT or ROOT in directory.parents:
        raise ValueError("Reference photographs must be stored outside the repository")
    directory.mkdir(parents=True, exist_ok=True)
    return directory


def download(target):
    url = "https://drive.usercontent.google.com/download?" + urllib.parse.urlencode(
        {"id": DATASET_ID, "export": "download"})
    for attempt in range(2):
        request = urllib.request.Request(url, headers={"User-Agent": "RasterizeMattingQualification/1.0"})
        with urllib.request.urlopen(request, timeout=60) as response:
            first = response.read(4096)
            if not first.startswith(b"PK"):
                text = (first + response.read(200_000)).decode("utf8", errors="replace")
                # This is Google's file-size virus-scan notice, not an account,
                # captcha or license agreement. Never follow arbitrary forms.
                if attempt or "can't scan this file for viruses" not in text:
                    raise ValueError("Public archive unavailable; no login, captcha or terms accepted")
                fields = dict(re.findall(r'<input type="hidden" name="([^"]+)" value="([^"]+)"', text))
                if fields.get("id") != DATASET_ID or fields.get("confirm") != "t":
                    raise ValueError("Unexpected public download confirmation")
                url = "https://drive.usercontent.google.com/download?" + urllib.parse.urlencode(fields)
                continue
            partial = target.with_suffix(".part")
            created = False
            try:
                total = 0
                output = partial.open("xb")
                created = True
                with output:
                    part = first
                    while part:
                        total += len(part)
                        if total > ARCHIVE_BYTES:
                            raise ValueError("Archive exceeds pinned length")
                        output.write(part)
                        part = response.read(1024 * 1024)
                if total != ARCHIVE_BYTES or sha256(partial) != ARCHIVE_SHA256:
                    raise ValueError("Archive checksum/length does not match pinned release")
                partial.replace(target)
                return
            except BaseException:
                if created:
                    partial.unlink(missing_ok=True)
                raise
    raise ValueError("Public archive was not returned")


def verify_reference(path, expected_sha, expected_bytes):
    if path.stat().st_size != expected_bytes or sha256(path) != expected_sha:
        raise ValueError(f"Reference checksum/length mismatch: {path.name}")
    with path.open("rb") as source:
        header = source.read(29)
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise ValueError("Reference is not a PNG")
    shape = struct.unpack(">IIBBBBB", header[16:29])
    if shape != (2304, 3456, 8, 6, 0, 0, 0):
        raise ValueError(f"Unexpected reference dimensions/storage: {shape}")


def acquire(directory, *, verify_only=False):
    directory = checked_root(directory)
    permission = directory / "permission-source.js"
    if not permission.exists():
        if verify_only:
            raise ValueError("Pinned permission source is missing")
        with urllib.request.urlopen(PERMISSION_ASSET, timeout=60) as response:
            text = response.read(PERMISSION_BYTES + 1)
        if len(text) != PERMISSION_BYTES or hashlib.sha256(text).hexdigest() != PERMISSION_SHA256:
            raise ValueError("Dataset permission source differs from reviewed pinned text")
        with permission.open("xb") as target:
            target.write(text)
    if permission.stat().st_size != PERMISSION_BYTES or sha256(permission) != PERMISSION_SHA256:
        raise ValueError("Pinned permission source has changed; preserving file")
    archive = directory / "PhotoMatte85.zip"
    if not archive.exists():
        if verify_only:
            raise ValueError("Pinned archive is missing")
        download(archive)
    if archive.stat().st_size != ARCHIVE_BYTES or sha256(archive) != ARCHIVE_SHA256:
        raise ValueError("Existing archive checksum/length mismatch; preserving file")
    records = []
    with zipfile.ZipFile(archive) as source:
        for name, digest, length in REFERENCES:
            target = directory / name
            if not target.exists():
                if verify_only:
                    raise ValueError(f"Reference missing: {name}")
                info = source.getinfo(name)
                if info.file_size != length or info.filename != name or Path(name).name != name:
                    raise ValueError("Unexpected archive entry")
                data = source.read(info)
                if hashlib.sha256(data).hexdigest() != digest:
                    raise ValueError("Extracted reference checksum mismatch")
                target.write_bytes(data)
            verify_reference(target, digest, length)
            records.append({"id": name.removesuffix(".png"), "file": name, "sha256": digest,
                            "size_bytes": length, "dimensions": [2304, 3456],
                            "foreground": "PNG straight RGB", "alpha": "PNG alpha channel",
                            "reference_origin": "publisher green-screen extraction",
                            "crop": [512, 384, 1280, 1280]})
    manifest = {
        "schema_version": 1, "dataset": "PhotoMatte85", "archive_sha256": ARCHIVE_SHA256,
        "archive_bytes": ARCHIVE_BYTES, "permission_page": PERMISSION_PAGE,
        "permission_page_asset": PERMISSION_ASSET,
        "permission_asset_sha256": PERMISSION_SHA256,
        "permission_excerpt": "All datasets are licensed for commercial and non-commercial purposes.",
        "download_url": f"https://drive.google.com/file/d/{DATASET_ID}/view",
        "citation": "Lin et al., Real-Time High-Resolution Background Matting, CVPR2021",
        "paper": "https://openaccess.thecvf.com/content/CVPR2021/html/Lin_Real-Time_High-Resolution_Background_Matting_CVPR_2021_paper.html",
        "permission_summary": "Publisher permits commercial/noncommercial use; requests academic citation and a commercial-product survey. No form submitted; local evaluation only.",
        "commercial_product_survey": "https://docs.google.com/forms/d/e/1FAIpQLSdR9Yhu9V1QE3pN_LvZJJyDaEpJD2cscOOqMz8N732eLDf42A/viewform",
        "limitations": ["portrait/hair only; no photographic fur/glass/product reference",
                        "provided extracted alpha/foreground are reference annotations, not perfect physical measurements",
                        "numeric scenes are controlled recompositions of supplied foreground/alpha",
                        "default evaluation uses fixed native-pixel crops; no rescaling"],
        "references": records,
    }
    output = directory / "references.json"
    if not verify_only:
        output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    elif not output.exists() or json.loads(output.read_text()) != manifest:
        raise ValueError("Manifest differs from the pinned reference contract")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    manifest = acquire(args.directory, verify_only=args.verify_only)
    print(f"Verified {len(manifest['references'])} photographic foreground/alpha references")


if __name__ == "__main__":
    main()
