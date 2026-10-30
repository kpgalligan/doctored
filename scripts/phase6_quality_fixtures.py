#!/usr/bin/env python3
"""Fetch five explicitly licensed photo fixtures into a supplied temporary root.

No photos are committed. Original bytes are checksum pinned; each source's own
license/attribution page is recorded next to the downloaded image. These photos
have no ground-truth alpha and therefore support inspection, not alpha scores.
Run: python3 scripts/phase6_quality_fixtures.py ~/devel/doctored-fixtures/phase6/photos
(the canonical persistent root; /private/tmp is purged after three untouched days)
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

sys.dont_write_bytecode = True

PHOTOS = [
    dict(name="portrait", file="portrait.png", url="https://raw.githubusercontent.com/scikit-image/scikit-image/v0.24.0/skimage/data/astronaut.png", author="NASA", license="Public domain; no known copyright restrictions", provenance="https://scikit-image.org/docs/0.24.x/api/skimage.data.html#skimage.data.astronaut", sha256="88431cd9653ccd539741b555fb0a46b61558b301d4110412b5bc28b5e3ea6cb5"),
    dict(name="fur", file="fur.png", url="https://raw.githubusercontent.com/scikit-image/scikit-image/v0.24.0/skimage/data/chelsea.png", author="Stefan van der Walt", license="CC0", provenance="https://scikit-image.org/docs/0.24.x/api/skimage.data.html#skimage.data.chelsea", sha256="596aa1e7cb875eb79f437e310381d26b338a81c2da23439704a73c4651e8c4bb"),
    dict(name="glass", file="glass.jpg", url="https://upload.wikimedia.org/wikipedia/commons/1/11/Glass-of-water.jpg", author="Derek Jensen (Tysto), 2005", license="Public domain dedication by photographer", provenance="https://commons.wikimedia.org/wiki/File:Glass-of-water.jpg", sha256="abeb89638e774d103a8f80730b303de033f84ae6cf438e1117384c2533c8ec14"),
    dict(name="product", file="product.jpg", url="https://upload.wikimedia.org/wikipedia/commons/e/e4/Canon_EOS_R.jpg", author="Tycho / Shansov.net, 2018", license="CC0 1.0", provenance="https://commons.wikimedia.org/wiki/File:Canon_EOS_R.jpg", sha256="60064baed5c495a54093d0b629c0210f092946ff539be8c53173e382d45f5298"),
    # Select ▸ Sky's fixture; scripts/model-drives/sky_reference.json traces its skyline.
    dict(name="sky", file="sky.jpg", url="https://upload.wikimedia.org/wikipedia/commons/c/c0/Blue_sky_behind_rooftops_and_clouds.jpg", author="Bo Jessen, 2012", license="CC0 1.0", provenance="https://commons.wikimedia.org/wiki/File:Blue_sky_behind_rooftops_and_clouds.jpg", sha256="c4f4e249cba29671e98a50ad4f911d01248b444fe3ae6c5cfb0359e71bd59375"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    records = []
    for fixture in PHOTOS:
        target = args.directory / fixture["file"]
        data = target.read_bytes() if target.exists() else urllib.request.urlopen(
            urllib.request.Request(fixture["url"], headers={"User-Agent": "RasterizeSelectionQualification/1.0 (local development fixtures)"}), timeout=30
        ).read()
        digest = hashlib.sha256(data).hexdigest()
        if fixture["sha256"] and digest != fixture["sha256"]:
            raise ValueError(f"Source bytes changed for {fixture['name']}: {digest}")
        target.write_bytes(data)
        records.append(dict(fixture, sha256=digest, size_bytes=len(data), alpha_ground_truth=False))
        print(f"{fixture['name']}: {digest} ({len(data)} bytes)")
    (args.directory / "provenance.json").write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
