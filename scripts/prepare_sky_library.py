#!/usr/bin/env python3
"""Prepare the sky library's release folder (M2a Sky Replacement). NETWORK.

Edit ▸ Sky Replacement's library is DATA in the model store: one catalogue entry
(`doctored/sky-library`, capability `sky_library`, runtime `assets`) whose files
are the sky JPEGs and their `manifest.json`, downloaded and SHA-256-verified by
the app's own `ModelDownloader` from the doctored-models GitHub release
`sky-library-1`. This script writes that release folder; publishing it is the
owner's step, and `scripts/fill_model_fixtures.py --release-dir <parent>` fills
the fixture store from it before then.

Source: 24 of Poly Haven's CC0 HDRIs (every Poly Haven asset is CC0 1.0), six per
category — blue, sunset, dramatic, night — fixed in `SKIES` below with a
per-sky crop centre. For each, the `tonemapped` JPG listed by
`https://api.polyhaven.com/files/<slug>` is downloaded (cached under
`--cache`), and the equirectangular band y ∈ [0.10H, 0.48H) — the sky above
the horizon, below the zenith's worst stretch — is cropped 0.5W wide around the
centre with `sips`, then resized to at most 3000 px wide at JPEG quality 90.

Writes into `--output` (default
~/devel/doctored-fixtures/models-release/sky-library-1):
- `<id>.jpg` per sky;
- `manifest.json`: {"version":1,"skies":[{"id","name","category","file",
  "source_url","license":"CC0-1.0","author"}]};
- `NOTES.md`: the source URL, licence and author of every sky;
- `catalog-entry.json`: the `ModelCatalog.swift` entry, with the SHA-256 and
  size of every asset and release URLs
  `https://github.com/kpgalligan/doctored-models/releases/download/sky-library-1/<file>`.
`--thumbs` also writes 600 px previews under `<cache>/thumbs` for review.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import urllib.request

VERSION = "sky-library-1"
RELEASE = f"https://github.com/kpgalligan/doctored-models/releases/download/{VERSION}/"
DEFAULT_OUTPUT = Path.home() / "devel/doctored-fixtures/models-release" / VERSION
API = "https://api.polyhaven.com"
LICENSE = "CC0-1.0"
BAND = (0.10, 0.48)
MAX_WIDTH = 3000
QUALITY = 90

# (category, Poly Haven slug, crop centre as a fraction of the width), chosen
# from a contact sheet of the sources (sun, glow or cloud mass in frame; a
# night sky with no treeline inside the band). The centre is held inside
# [0.25, 0.75] so the half-width crop never wraps.
SKIES = [
    ("blue", "kloofendal_38d_partly_cloudy_puresky", 0.55),
    ("blue", "kloofendal_43d_clear_puresky", 0.55),
    ("blue", "kloofendal_48d_partly_cloudy_puresky", 0.5),
    ("blue", "kloppenheim_05_puresky", 0.5),
    ("blue", "sunflowers_puresky", 0.5),
    ("blue", "rustig_koppie_puresky", 0.55),
    ("sunset", "belfast_sunset_puresky", 0.6),
    ("sunset", "industrial_sunset_puresky", 0.55),
    ("sunset", "qwantani_sunset_puresky", 0.6),
    ("sunset", "wasteland_clouds_puresky", 0.6),
    ("sunset", "kloppenheim_06_puresky", 0.5),
    ("sunset", "evening_road_01_puresky", 0.45),
    ("dramatic", "table_mountain_1_puresky", 0.6),
    ("dramatic", "table_mountain_2_puresky", 0.5),
    ("dramatic", "aristea_wreck_puresky", 0.4),
    ("dramatic", "farm_field_puresky", 0.45),
    ("dramatic", "kloofendal_overcast_puresky", 0.55),
    ("dramatic", "kloppenheim_01_puresky", 0.6),
    ("night", "rogland_clear_night", 0.5),
    ("night", "kloppenheim_02_puresky", 0.5),
    ("night", "qwantani_night_puresky", 0.5),
    ("night", "qwantani_moonrise_puresky", 0.5),
    ("night", "qwantani_moon_noon_puresky", 0.5),
    ("night", "kloppenheim_07_puresky", 0.5),
]


def fetch_json(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "doctored-fixture-fetch/1.0"})) as r:
        return json.load(r)


def download(url, target, size):
    if target.is_file() and target.stat().st_size == size:
        return
    partial = target.with_suffix(target.suffix + ".partial")
    request = urllib.request.Request(url, headers={"User-Agent": "doctored-fixture-fetch/1.0"})
    with urllib.request.urlopen(request) as response, partial.open("wb") as out:
        while True:
            chunk = response.read(1 << 20)
            if not chunk:
                break
            out.write(chunk)
    if partial.stat().st_size != size:
        raise RuntimeError(f"{url}: {partial.stat().st_size} bytes, expected {size}")
    partial.rename(target)


def pixel_size(path):
    out = subprocess.run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", str(path)],
                         capture_output=True, text=True, check=True).stdout
    values = {line.split(":")[0].strip(): int(line.split(":")[1]) for line in out.splitlines()[1:]}
    return values["pixelWidth"], values["pixelHeight"]


def crop(source, target, centre):
    width, height = pixel_size(source)
    top = round(BAND[0] * height)
    band_height = round(BAND[1] * height) - top
    band_width = width // 2
    centre = min(max(centre, 0.25), 0.75)
    left = round(centre * width - band_width / 2)
    run = lambda *a: subprocess.run(["sips", *a], capture_output=True, text=True, check=True)
    run("--cropToHeightWidth", str(band_height), str(band_width),
        "--cropOffset", str(top), str(left), str(source), "--out", str(target))
    if band_width > MAX_WIDTH:
        run("--resampleWidth", str(MAX_WIDTH), str(target))
    run("-s", "format", "jpeg", "-s", "formatOptions", str(QUALITY), str(target), "--out", str(target))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--cache", type=Path, default=None,
                        help="downloads (default <output>.source beside the output)")
    parser.add_argument("--thumbs", action="store_true", help="write 600 px review previews")
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    cache = (arguments.cache or output.with_name(output.name + ".source")).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    assert len(SKIES) == 24 and len({s for _, s, _ in SKIES}) == 24
    skies = []
    for category, slug, centre in SKIES:
        info = fetch_json(f"{API}/info/{slug}")
        files = fetch_json(f"{API}/files/{slug}")
        tonemapped = files["tonemapped"]
        source = cache / f"{slug}.jpg"
        print(f"{category:9} {slug}: {tonemapped['url']}", flush=True)
        download(tonemapped["url"], source, tonemapped["size"])
        identifier = slug.removesuffix("_puresky").replace("_", "-")
        target = output / f"{identifier}.jpg"
        crop(source, target, centre)
        if arguments.thumbs:
            (cache / "thumbs").mkdir(exist_ok=True)
            subprocess.run(["sips", "--resampleWidth", "600", str(target),
                            "--out", str(cache / "thumbs" / target.name)],
                           capture_output=True, check=True)
        name = info["name"].replace(" Pure Sky", "").replace(" (Pure Sky)", "")
        skies.append(dict(id=identifier, name=name, category=category, file=target.name,
                          source_url=f"https://polyhaven.com/a/{slug}", license=LICENSE,
                          author=", ".join(info["authors"])))
    manifest = output / "manifest.json"
    manifest.write_text(json.dumps(dict(version=1, skies=skies), indent=2) + "\n")
    notes = ["# Sky library 1", "",
             "Sky crops of Poly Haven HDRIs (tone-mapped JPGs), every one CC0 1.0 "
             "(https://creativecommons.org/publicdomain/zero/1.0/). Written by "
             "`scripts/prepare_sky_library.py`: the band y ∈ [0.10H, 0.48H), half the width "
             "around a fixed centre, at most 3000 px wide, JPEG quality 90. Publishing the "
             f"release `{VERSION}` on kpgalligan/doctored-models is the owner's step.", "",
             "| id | category | source | licence | author |", "|---|---|---|---|---|"]
    notes += [f"| {s['id']} | {s['category']} | {s['source_url']} | {s['license']} | {s['author']} |"
              for s in skies]
    (output / "NOTES.md").write_text("\n".join(notes) + "\n")
    assets = [output / s["file"] for s in skies] + [manifest]
    entry = dict(
        id="doctored/sky-library", name="Sky Library (Poly Haven, CC0)", capability="sky_library",
        version=VERSION,
        files=[dict(path=p.name, url=RELEASE + p.name, sha256=digest(p), size=p.stat().st_size)
               for p in assets],
        runtime="assets", minimum_macos="15.0",
        license=dict(name=LICENSE, url="https://creativecommons.org/publicdomain/zero/1.0/",
                     tier="clean"),
        training_data=("Not a model: 24 sky photographs cropped from Poly Haven's CC0 HDRIs "
                       "(tone-mapped JPGs); manifest.json names each one's source and author."),
        inference_bytes=0, packages=[])
    (output / "catalog-entry.json").write_text(json.dumps(entry, indent=2) + "\n")
    total = sum(p.stat().st_size for p in assets)
    print(f"wrote {len(assets)} assets ({total} bytes) to {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
