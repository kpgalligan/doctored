#!/usr/bin/env python3
"""Convert Lensfun's lens database to the compact JSON the core embeds.

NETWORK, run by hand only (`python3 -I scripts/fetch_lensfun.py`): the build
stays offline and reads the committed `core/data/lensfun/lensfun-db.json`.

The source is pinned: the tag, its commit, the tarball URL and its sha256 are
below. A new database means a new tag here, a new hash, a re-run and the data
diff it shows (designs/v1-lensfun.md, "Updating the data").

The database (`data/db/*.xml`) is licensed CC BY-SA 3.0 by the Lensfun project
and its contributors (original database: Tom Niemann's PTLens database). The
JSON this writes is an adaptation of it, licensed CC BY-SA 3.0 as well, and
says so in its `database` object. Only the database is read; no library code.

What is kept: mounts (name and compatible mounts), cameras (maker, model,
variant, mount, crop factor) and lenses (maker, model, mounts, crop factor,
aspect ratio, centre, focal and aperture ranges, and their distortion, TCA and
vignetting calibration rows as number arrays). Translations, comments, crop
and real-focal-length rows are dropped. A row outside the bounds the core's
profile parser accepts is dropped and counted, as is a lens with no row left.

Lens ids: the model string, plus ` [crop <c>]` when the model repeats, then
` #<n>` when that still repeats; the script asserts they are unique.

`--check` re-fetches and compares the result with the committed file byte for
byte (exit 1 on a difference).

Standard library only. Exit 0 on success.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import sys
import tarfile
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "core/data/lensfun/lensfun-db.json"

TAG = "v0.3.4"
COMMIT = "101c745e847a5de4a1e569a94368ce2027198598"
URL = "https://github.com/lensfun/lensfun/archive/refs/tags/v0.3.4.tar.gz"
SHA256 = "dafb39c08ef24a0e2abd00d05d7341b1bf1f0c38bfcd5a4c69cf5f0ecb6db112"

DATABASE = {
    "name": "Lensfun",
    "title": "Lensfun lens database",
    "tag": TAG,
    "commit": COMMIT,
    "url": "https://github.com/lensfun/lensfun",
    "license": "CC BY-SA 3.0",
    "license_url": "https://creativecommons.org/licenses/by-sa/3.0/",
    "credit": "The Lensfun project and its contributors (original database: Tom Niemann's "
    "PTLens database). Converted to JSON by Doctored.",
    "notice": "This JSON is an adaptation of the Lensfun lens database and is licensed "
    "under CC BY-SA 3.0 (https://creativecommons.org/licenses/by-sa/3.0/).",
}

# The bounds `lens_model::LensProfile::from_json` accepts.
CROP = (0.1, 20.0)
ASPECT = (1.0, 4.0)
DISTORTION_TERM = 1.0
TCA_SCALE = (0.9, 1.1)
TCA_TERM = 0.1
PA_TERM = 5.0


def fetch() -> bytes:
    with urllib.request.urlopen(URL, timeout=120) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SHA256:
        raise SystemExit(f"fetch_lensfun: sha256 {digest} is not the pinned {SHA256}")
    return data


def number(text: str | None) -> float | None:
    if text is None:
        return None
    try:
        value = float(text)
    except ValueError:
        return None
    return value if value == value and abs(value) != float("inf") else None


def plain(element: ET.Element, tag: str) -> str | None:
    """The first child `tag` without a `lang` attribute, stripped."""
    for child in element.findall(tag):
        if "lang" not in child.attrib and child.text is not None:
            return " ".join(child.text.split())
    return None


def aspect_ratio(text: str | None) -> float | None:
    if text is None:
        return 1.5
    text = text.strip()
    if ":" in text:
        a, b = (number(part) for part in text.split(":", 1))
        if not a or not b:
            return None
        value = a / b
    else:
        value = number(text)
        if not value:
            return None
    return max(value, 1.0 / value)


class Counts:
    def __init__(self) -> None:
        self.files = 0
        self.cameras = 0
        self.lenses = 0
        self.rows = {"distortion": 0, "tca": 0, "vignetting": 0}
        self.dropped_rows = 0
        self.dropped_lenses = 0
        self.dropped_cameras = 0


def attr(row: ET.Element, name: str, default: float) -> float | None:
    if name not in row.attrib:
        return default
    return number(row.attrib[name])


def calibration(lens: ET.Element, counts: Counts) -> dict[str, list[list[float]]]:
    out: dict[str, list[list[float]]] = {}
    for block in lens.findall("calibration"):
        for row in block:
            kind = row.tag
            if kind not in ("distortion", "tca", "vignetting"):
                continue
            counts.rows[kind] += 1
            model = row.attrib.get("model")
            focal = number(row.attrib.get("focal"))
            values: list[float | None]
            ok = True
            if kind == "distortion" and model == "poly3":
                key, values = "poly3", [attr(row, "k1", 0.0)]
            elif kind == "distortion" and model == "poly5":
                key, values = "poly5", [attr(row, "k1", 0.0), attr(row, "k2", 0.0)]
            elif kind == "distortion" and model == "ptlens":
                key = "ptlens"
                values = [attr(row, "a", 0.0), attr(row, "b", 0.0), attr(row, "c", 0.0)]
            elif kind == "tca" and model == "linear":
                key, values = "tca_linear", [attr(row, "kr", 1.0), attr(row, "kb", 1.0)]
            elif kind == "tca" and model == "poly3":
                key = "tca_poly3"
                values = [
                    attr(row, "vr", 1.0),
                    attr(row, "cr", 0.0),
                    attr(row, "br", 0.0),
                    attr(row, "vb", 1.0),
                    attr(row, "cb", 0.0),
                    attr(row, "bb", 0.0),
                ]
            elif kind == "vignetting" and model == "pa":
                key = "pa"
                values = [
                    number(row.attrib.get("aperture")),
                    number(row.attrib.get("distance")),
                    attr(row, "k1", 0.0),
                    attr(row, "k2", 0.0),
                    attr(row, "k3", 0.0),
                ]
            else:
                key, values, ok = "", [], False
            if ok and (focal is None or focal <= 0 or any(v is None for v in values)):
                ok = False
            if ok:
                ok = in_bounds(key, values)  # type: ignore[arg-type]
            if not ok:
                counts.dropped_rows += 1
                continue
            out.setdefault(key, []).append([focal] + values)  # type: ignore[operator]
    for rows in out.values():
        rows.sort()
    return out


def in_bounds(key: str, values: list[float]) -> bool:
    if key in ("poly3", "poly5", "ptlens"):
        return all(abs(v) <= DISTORTION_TERM for v in values)
    if key == "tca_linear":
        return all(TCA_SCALE[0] <= v <= TCA_SCALE[1] for v in values)
    if key == "tca_poly3":
        vr, cr, br, vb, cb, bb = values
        scales = all(TCA_SCALE[0] <= v <= TCA_SCALE[1] for v in (vr, vb))
        return scales and all(abs(v) <= TCA_TERM for v in (cr, br, cb, bb))
    if key == "pa":
        aperture, distance, k1, k2, k3 = values
        return aperture > 0 and distance > 0 and all(abs(v) <= PA_TERM for v in (k1, k2, k3))
    return False


def convert(tarball: bytes) -> tuple[bytes, Counts]:
    counts = Counts()
    mounts: dict[str, list[str]] = {}
    cameras: list[dict] = []
    lenses: list[dict] = []
    with tarfile.open(fileobj=io.BytesIO(tarball), mode="r:gz") as archive:
        members = sorted(
            (
                m
                for m in archive.getmembers()
                if m.isfile() and "/data/db/" in m.name and m.name.endswith(".xml")
            ),
            key=lambda m: m.name,
        )
        for member in members:
            handle = archive.extractfile(member)
            if handle is None:
                continue
            counts.files += 1
            root = ET.fromstring(handle.read())
            for mount in root.findall("mount"):
                name = plain(mount, "name")
                if name is None:
                    continue
                compat = [" ".join((c.text or "").split()) for c in mount.findall("compat")]
                mounts[name] = sorted({c for c in compat if c})
            for camera in root.findall("camera"):
                counts.cameras += 1
                entry = read_camera(camera)
                if entry is None:
                    counts.dropped_cameras += 1
                    continue
                cameras.append(entry)
            for lens in root.findall("lens"):
                counts.lenses += 1
                entry = read_lens(lens, counts)
                if entry is None:
                    counts.dropped_lenses += 1
                    continue
                lenses.append(entry)
    assign_ids(lenses)
    text = layout(cameras, lenses, mounts)
    return text.encode("utf-8"), counts


def read_camera(camera: ET.Element) -> dict | None:
    maker = plain(camera, "maker")
    model = plain(camera, "model")
    mount = plain(camera, "mount")
    crop = number(plain(camera, "cropfactor"))
    if not maker or not model or not mount or crop is None:
        return None
    if not CROP[0] < crop <= CROP[1]:
        return None
    entry = {"maker": maker, "model": model, "mount": mount, "crop": crop}
    variant = plain(camera, "variant")
    if variant:
        entry["variant"] = variant
    return entry


def read_lens(lens: ET.Element, counts: Counts) -> dict | None:
    model = plain(lens, "model")
    crop = number(plain(lens, "cropfactor"))
    aspect = aspect_ratio(plain(lens, "aspect-ratio"))
    if not model or crop is None or aspect is None:
        return None
    if not CROP[0] < crop <= CROP[1] or not ASPECT[0] <= aspect <= ASPECT[1]:
        return None
    rows = calibration(lens, counts)
    if not rows:
        return None
    entry: dict = {
        "maker": plain(lens, "maker") or "",
        "model": model,
        "mounts": [" ".join((m.text or "").split()) for m in lens.findall("mount")],
        "crop": crop,
    }
    if aspect != 1.5:
        entry["aspect"] = aspect
    center = lens.find("center")
    if center is not None:
        x = number(center.attrib.get("x", "0")) or 0.0
        y = number(center.attrib.get("y", "0")) or 0.0
        if (x, y) != (0.0, 0.0) and (x * x + y * y) ** 0.5 <= 1.0:
            entry["center"] = [x, y]
    focal = lens.find("focal")
    focals = sorted({row[0] for kind in rows.values() for row in kind})
    lo = hi = None
    if focal is not None:
        value = number(focal.attrib.get("value"))
        lo = number(focal.attrib.get("min")) or value
        hi = number(focal.attrib.get("max")) or value or lo
    if lo is None or hi is None:
        lo, hi = focals[0], focals[-1]
    entry["focal"] = [lo, hi]
    aperture = lens.find("aperture")
    if aperture is not None:
        amin = number(aperture.attrib.get("min"))
        amax = number(aperture.attrib.get("max"))
        if amin is not None:
            entry["aperture"] = [amin, amax if amax is not None else amin]
    entry.update(rows)
    return entry


def assign_ids(lenses: list[dict]) -> None:
    by_model: dict[str, int] = {}
    for lens in lenses:
        by_model[lens["model"]] = by_model.get(lens["model"], 0) + 1
    first = []
    for lens in lenses:
        base = lens["model"]
        if by_model[base] > 1:
            base += f" [crop {format_number(lens['crop'])}]"
        first.append(base)
    repeats: dict[str, int] = {}
    for base in first:
        repeats[base] = repeats.get(base, 0) + 1
    seen: dict[str, int] = {}
    for lens, base in zip(lenses, first):
        if repeats[base] > 1:
            seen[base] = seen.get(base, 0) + 1
            base += f" #{seen[base]}"
        lens["id"] = base
    ids = [lens["id"] for lens in lenses]
    assert len(ids) == len(set(ids)), "lens ids are not unique"


def format_number(value: float) -> str:
    if value == int(value) and abs(value) < 1e15:
        return str(int(value))
    return repr(value)


def encode(value) -> str:
    """Compact, sorted JSON with integral numbers written as integers."""
    if isinstance(value, dict):
        items = ",".join(f"{json.dumps(k)}:{encode(value[k])}" for k in sorted(value))
        return "{" + items + "}"
    if isinstance(value, list):
        return "[" + ",".join(encode(v) for v in value) + "]"
    if isinstance(value, float):
        return format_number(value)
    return json.dumps(value, ensure_ascii=False)


def layout(cameras: list[dict], lenses: list[dict], mounts: dict[str, list[str]]) -> str:
    """One camera, lens or mount per line, so a tag bump diffs by entry."""
    lines = ['{"database":' + encode(DATABASE) + ",", '"mounts":{']
    names = sorted(mounts)
    for i, name in enumerate(names):
        tail = "," if i + 1 < len(names) else ""
        lines.append(f"{json.dumps(name, ensure_ascii=False)}:{encode(mounts[name])}{tail}")
    lines.append('},"cameras":[')
    for i, camera in enumerate(cameras):
        lines.append(encode(camera) + ("," if i + 1 < len(cameras) else ""))
    lines.append('],"lenses":[')
    for i, lens in enumerate(lenses):
        lines.append(encode(lens) + ("," if i + 1 < len(lenses) else ""))
    lines.append("]}")
    text = "\n".join(lines) + "\n"
    json.loads(text)
    return text


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="compare with the committed JSON")
    args = parser.parse_args()
    data, counts = convert(fetch())
    print(
        f"fetch_lensfun: {counts.files} files, {counts.cameras} cameras "
        f"({counts.dropped_cameras} dropped), {counts.lenses} lenses "
        f"({counts.dropped_lenses} dropped), rows {counts.rows} "
        f"({counts.dropped_rows} outside the profile bounds or unreadable, dropped), "
        f"{len(data)} bytes"
    )
    if args.check:
        committed = OUTPUT.read_bytes() if OUTPUT.is_file() else b""
        if committed != data:
            print(f"fetch_lensfun: {OUTPUT.relative_to(ROOT)} differs from the conversion")
            return 1
        print(f"fetch_lensfun: {OUTPUT.relative_to(ROOT)} reproduces byte for byte")
        return 0
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_bytes(data)
    print(f"fetch_lensfun: wrote {OUTPUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
