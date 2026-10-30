#!/usr/bin/env python3
"""Acquire seven pinned full-image photographic cutout references outside git.

Three Commons photographs extend subject coverage; their supplied edited RGBA
is an annotation reference for controlled recomposition, not physical truth.
The glass bottle has silhouette alpha and is explicitly NOT a translucent-matte
reference. Four already pinned PhotoMatte85 originals provide full portraits.
No forms, credentials, terms dialogs or restricted research datasets are used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import tempfile
import urllib.parse
import urllib.request
import sys
sys.dont_write_bytecode = True
import phase6_reference_fixtures as portraits
COMMONS = [{'id': 'fur-longhair',
  'subject': 'fur',
  'file': 'fur-longhair.png',
  'size_bytes': 439673,
  'sha256': '204a5dc854a60fa8bd1938e5d0c75ab6390852801ea19bf7693485a8892570d7',
  'dimensions': [763, 764],
  'interlace': 0,
  'url': 'https://upload.wikimedia.org/wikipedia/commons/2/26/BLH_cat_transparent.png',
  'title': 'BLH_cat_transparent.png',
  'revision': 810538237,
  'permission_bytes': 508,
  'permission_sha256': 'a55a70c4831c3deff11b04ea51f0ab7045c14a723e13f371912bd08e29619c3c',
  'author': 'Chatterie des Millenovæ; background removal by YukiKoKo',
  'license': 'CC BY-SA 4.0',
  'license_url': 'https://creativecommons.org/licenses/by-sa/4.0/',
  'original_source': 'https://commons.wikimedia.org/wiki/File:British_Longhair_-_Black_Silver_Shaded.jpg',
  'reference_limit': 'Edited cutout from an existing photograph; supplied fur boundary and RGB may '
                     'retain original-background residue.',
  'transparency_reference': False},
 {'id': 'glass-bottle',
  'subject': 'glass_silhouette',
  'file': 'glass-bottle.png',
  'size_bytes': 509065,
  'sha256': 'f8fbdde9a9f423a52b4b4a3b32de1305a4e6cf82f9688397841e65195fc1b2e3',
  'dimensions': [800, 600],
  'interlace': 1,
  'url': 'https://upload.wikimedia.org/wikipedia/commons/1/1b/Colorful_bottle.png',
  'title': 'Colorful_bottle.png',
  'revision': 943421598,
  'permission_bytes': 995,
  'permission_sha256': '853e6de32836e573b251c7a4f8343ee6123ee96890ae0a75f191e22383f1e83f',
  'author': 'Matthew Bowden; transparency postprocessing by Arc96',
  'license': 'Attribution-only permission; Wikimedia VRT ticket2007040110016185',
  'license_url': 'https://commons.wikimedia.org/wiki/File:Colorful_bottle.png#Licensing',
  'original_source': 'https://commons.wikimedia.org/wiki/File:Colorful_bottle.jpg',
  'reference_limit': 'Only452 fractional alpha pixels and190338 opaque pixels: an edited '
                     'silhouette, NOT transparent-interior or refractive-ground-truth evidence.',
  'transparency_reference': False},
 {'id': 'hard-arduboy',
  'subject': 'hard_product',
  'file': 'hard-arduboy.png',
  'size_bytes': 2013380,
  'sha256': '1617d139a5369f343ef932303a7c1856cd82edc5b81efa1163e6e099643f7909',
  'dimensions': [944, 1338],
  'interlace': 0,
  'url': 'https://upload.wikimedia.org/wikipedia/commons/3/34/Arduboy_transparent.png',
  'title': 'Arduboy_transparent.png',
  'revision': 801139476,
  'permission_bytes': 384,
  'permission_sha256': '698e96ab0eb6054e87d845e2ab6e12478258a959a8bfaf6e7f2c405514326d19',
  'author': 'Drummyfish',
  'license': 'CC0 1.0',
  'license_url': 'https://creativecommons.org/publicdomain/zero/1.0/',
  'original_source': 'https://commons.wikimedia.org/wiki/File:Arduboy_transparent.png',
  'reference_limit': 'Photograph edited by its creator; supplied hard-edge coverage is not '
                     'measured optical alpha.',
  'transparency_reference': False}]

def publish(path, data):
    """Publish verified bytes without overwriting an existing/interrupted file."""
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.reference-', delete=False) as temporary:
        temp = Path(temporary.name)
        try:
            temporary.write(data)
            temporary.flush()
            os.fsync(temporary.fileno())
        except BaseException:
            temp.unlink(missing_ok=True)
            raise
    try:
        os.link(temp, path)
    finally:
        temp.unlink(missing_ok=True)

def verify(path, length, digest):
    if not path.exists() or path.stat().st_size != length or portraits.sha256(path) != digest:
        raise ValueError(f'Pinned reference/evidence differs or is missing: {path.name}; preserving existing files')

def acquire_file(path, url, length, digest, *, verify_only):
    if not path.exists():
        if verify_only:
            raise ValueError(f'Missing pinned file: {path.name}')
        request = urllib.request.Request(url, headers={'User-Agent': 'RasterizeQualification/1.0 (local image editor evaluation)'})
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read(length + 1)
        if len(data) != length or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError(f'Public download differs from pin: {path.name}; no forms accepted')
        publish(path, data)
    verify(path, length, digest)

def manifest():
    records = []
    for source in COMMONS:
        record = dict(source)
        record["permission_file"] = source["id"] + "-permission.wiki"
        identity = {"title": "File:" + source["title"], "oldid": source["revision"]}
        record["permission_url"] = (
            "https://commons.wikimedia.org/w/index.php?"
            + urllib.parse.urlencode({**identity, "action": "raw"})
        )
        record["source_page"] = (
            "https://commons.wikimedia.org/w/index.php?" + urllib.parse.urlencode(identity)
        )
        record["evaluation_extent"] = "entire supplied original image; no crop or resize"
        record["reference_origin"] = "publisher-edited photographic cutout"
        records.append(record)
    for file, digest, length in portraits.REFERENCES:
        records.append({
            "id": "portrait-" + file.removesuffix(".png"),
            "subject": "portrait_hair",
            "file": file,
            "sha256": digest,
            "size_bytes": length,
            "dimensions": [2304, 3456],
            "interlace": 0,
            "author": "PhotoMatte85 dataset contributors; Lin et al., CVPR2021",
            "license": (
                "Publisher permission for commercial/noncommercial purposes; "
                "academic citation and commercial-product survey requested"
            ),
            "license_url": portraits.PERMISSION_PAGE,
            "source_page": portraits.PERMISSION_PAGE,
            "permission_file": "permission-source.js",
            "permission_sha256": portraits.PERMISSION_SHA256,
            "permission_bytes": portraits.PERMISSION_BYTES,
            "archive_sha256": portraits.ARCHIVE_SHA256,
            "evaluation_extent": "entire supplied original image; no crop or resize",
            "reference_origin": "publisher green-screen-extracted foreground and alpha",
            "reference_limit": (
                "Supplied portrait/hair annotations; not physically measured foreground color or alpha."
            ),
            "transparency_reference": False,
        })
    return {
        "schema_version": 1,
        "reference_contract": "full photographic cutouts, encoded controlled recompositions",
        "references": records,
        "physical_translucency_reference_available": False,
        "limitations": [
            "Edited cutouts are supplied annotation references, not optical ground truth.",
            "Glass silhouette does not qualify translucent interiors or refraction.",
            "No natural-scene alpha is inferred from cutout RGB; source composites are generated from supplied F and alpha.",
            "All cases use full native dimensions; work/memory refusals are recorded as failures.",
        ],
    }


def attribution(value):
    lines = [
        "# Photographic reference and derived-image attribution",
        "",
        "Images remain outside the code repository. Derived images change backgrounds, alpha and foreground colors for local qualification.",
        "Cat image derivatives retain CC BY-SA4.0; the software itself is not a derivative of that photograph.",
        "",
    ]
    for record in value["references"]:
        lines.extend([
            f"- **{record['id']}** — {record['author']}. "
            f"[{record['license']}]({record['license_url']}). "
            f"[Source]({record['source_page']}).",
            f"  Reference limitation: {record['reference_limit']}",
        ])
    return "\n".join(lines) + "\n"


def acquire(directory, *, photo_source=None, verify_only=False):
    directory = portraits.checked_root(directory)
    value = manifest()
    contracts = {
        "full-references.json": json.dumps(value, indent=2, sort_keys=True) + "\n",
        "ATTRIBUTION.md": attribution(value),
    }
    # Metadata is part of the pin: it cannot silently turn an edited silhouette
    # into physical transparency truth or change the full-image extent.
    for name, data in contracts.items():
        path = directory / name
        if path.exists() and path.read_text() != data:
            raise ValueError(f"{name} differs from pinned contract; preserving it")
        if verify_only and not path.exists():
            raise ValueError(f"Missing {name}")

    for record in value["references"]:
        target = directory / record["file"]
        if record["id"].startswith("portrait-"):
            files = [
                (record["file"], record["size_bytes"], record["sha256"]),
                (record["permission_file"], record["permission_bytes"], record["permission_sha256"]),
            ]
            for filename, length, digest in files:
                path = directory / filename
                if not path.exists():
                    if verify_only or photo_source is None:
                        raise ValueError(
                            "Missing PhotoMatte full reference; supply --photo-source "
                            "from the existing pinned acquisition"
                        )
                    source = Path(photo_source) / filename
                    verify(source, length, digest)
                    publish(path, source.read_bytes())
                verify(path, length, digest)
        else:
            acquire_file(
                directory / record["permission_file"], record["permission_url"],
                record["permission_bytes"], record["permission_sha256"],
                verify_only=verify_only,
            )
            acquire_file(
                target, record["url"], record["size_bytes"], record["sha256"],
                verify_only=verify_only,
            )
        with target.open("rb") as source:
            header = source.read(29)
        expected = (*record["dimensions"], 8, 6, 0, 0, record["interlace"])
        valid_png = header[:8] == b"\x89PNG\r\n\x1a\n" and header[12:16] == b"IHDR"
        if not valid_png or struct.unpack(">IIBBBBB", header[16:29]) != expected:
            raise ValueError("Unexpected reference PNG dimensions/storage")
    for name, data in contracts.items():
        path = directory / name
        if not path.exists():
            publish(path, data.encode())
    return value


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--photo-source", type=Path)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    result = acquire(args.directory, photo_source=args.photo_source, verify_only=args.verify_only)
    print(f"Verified {len(result['references'])} full-image reference photographs and their permission evidence")
