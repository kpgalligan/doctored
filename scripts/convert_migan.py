# /// script
# requires-python = "==3.11.*"
# dependencies = [
#     "torch==2.5.1",
#     "coremltools==8.3.0",
#     "numpy==1.26.4",
#     "pillow==11.0.0",
# ]
# ///
"""Convert MI-GAN 512 Places2 to Core ML for the Remove tool's model mode (M3a). NETWORK.

The Remove tool fills a stroke's hole with MI-GAN (Sargsyan et al., ICCV 2023; Picsart AI
Research, MIT code and MIT weights). Its README links the pretrained `migan_512_places2.pt`
only as a Google Drive folder, which does not answer a plain HTTPS GET, so the source is
IOPaint's TorchScript export of the same generator (`MIGAN_MODEL_URL` in IOPaint's
`iopaint/model/mi_gan.py`, whose MD5 pin 76eb3b1a71c400ee3290524f7a11b89c these bytes match).
This script is the reproducible conversion:

  1. downloads that TorchScript file and refuses any other bytes (SOURCE_SHA256, SOURCE_SIZE),
     and MI-GAN's weights licence from the repository at a pinned commit (LICENSE_URL);
  2. wraps the generator in MI-GAN's own input convention, so the package takes plain planes:
     `image` [1,3,512,512] RGB 0…1 and `mask` [1,1,512,512] (1 = remove), and gives `filled`
     [1,3,512,512] 0…1:
         img = 2·image − 1; known = 1 − mask
         out = G(cat[known − 0.5, img·known]);  filled = clamp((out + 1)/2, 0, 1)
  3. traces it and converts it to an mlprogram at FLOAT16 compute precision;
  4. self-checks the package against the PyTorch wrapper on 512² crops of the repo's
     sky.jpg, product.jpg and fur.png (`~/devel/doctored-fixtures/phase6/photos`, scaled so the
     short side is 512, centre crop) and a synthetic gradient, each with one fixed
     disc-plus-stroke hole: over the hole, mean |Δ| ≤ MEAN_BOUND and p99 |Δ| ≤ P99_BOUND. A miss
     rebuilds at FLOAT32; a miss at both exits 1 and writes no release assets;
  5. measures the package: its bytes, the median of 10 predictions at 512² (compute units ALL,
     one warm-up), the machine (`sysctl hw.memsize`, the chip), and per fixture the hole's
     mean |fill − original| (how far the fill is from what the hole hid; a quality figure,
     not a bound);
  6. writes `<output>/<RELEASE_TAG>/`: the flat assets
     `migan_512_places2-{Manifest.json,model.mlmodel,weight.bin}` (the files `ModelCatalog`'s
     entry downloads from
     https://github.com/kpgalligan/doctored-models/releases/download/<RELEASE_TAG>/),
     `LICENSE` (MI-GAN's weights licence), `NOTES.md`, `receipt.json` and `catalog-entry.json`
     (the entry `ModelCatalog` pastes; `inference_bytes` is the measured figure's place-holder).

Run with `uv run scripts/convert_migan.py --output ~/devel/doctored-fixtures/models-release`;
uv reads the pins above (Python 3.11). `scripts/fill_model_fixtures.py --release-dir <output>
--only doctored/migan-512-places2` then serves the assets to the fixture store. Publishing the
release is the owner's step: this script publishes nothing. The Manifest's item identifiers are
fresh UUIDs on every save, so a second run writes different output hashes: the catalogue pins
the run whose receipt it cites.
"""

import argparse
import hashlib
from importlib import metadata
import json
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

SOURCE_URL = "https://github.com/Sanster/models/releases/download/migan/migan_traced.pt"
SOURCE_SHA256 = "038b82d90416f1d1af232486d76d07665aecdd988e0f16090b1e11a61820b800"
SOURCE_SIZE = 27048675
OFFICIAL_URL = "https://drive.google.com/drive/folders/1xNtvN2lto0p5yFKOEEg9RioMjGrYM74w"
REPOSITORY = "https://github.com/Picsart-AI-Research/MI-GAN"
REPOSITORY_COMMIT = "2b793c5ece43f4253e32d4afc257120a5deed6f5"
LICENSE_URL = (f"https://raw.githubusercontent.com/Picsart-AI-Research/MI-GAN/"
               f"{REPOSITORY_COMMIT}/LICENSE-WEIGHTS")
RELEASE_TAG = "migan-512-places2-coreml-1"
PACKAGE = "migan_512_places2.mlpackage"
ASSET_PREFIX = "migan_512_places2-"
RELEASE_BASE = f"https://github.com/kpgalligan/doctored-models/releases/download/{RELEASE_TAG}/"
SIDE = 512
# The package against the float32 PyTorch wrapper, over the hole only (outside it the app
# never reads the fill): a mean within 0.01 (2.5 codes of 255) and a 99th percentile within
# 0.05 is a fill a viewer cannot tell from the reference's.
MEAN_BOUND = 0.01
P99_BOUND = 0.05
TIMED_RUNS = 10
# The place-holder until scripts/remove-model-admission/measure.py measures the figure.
INFERENCE_BYTES = 209715200

ROOT = Path(__file__).resolve().parent.parent
PHOTOS = Path.home() / "devel/doctored-fixtures/phase6/photos"
FIXTURES = ["sky.jpg", "product.jpg", "fur.png"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=300) as response:
        return response.read()


def fetch_source(scratch: Path) -> Path:
    data = fetch(SOURCE_URL)
    if len(data) != SOURCE_SIZE or sha256(data) != SOURCE_SHA256:
        raise SystemExit(f"the source is not the pinned file: {len(data)} bytes, sha256 {sha256(data)}")
    path = scratch / "migan_traced.pt"
    path.write_bytes(data)
    return path


def wrapper(source: Path):
    import torch

    generator = torch.jit.load(str(source), map_location="cpu").eval()

    class Wrapped(torch.nn.Module):
        """MI-GAN's input convention around the generator: plain 0…1 planes in and out."""

        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, image, mask):
            img = image * 2 - 1
            known = 1 - mask
            out = self.inner(torch.cat([known - 0.5, img * known], dim=1))
            return torch.clamp((out + 1) / 2, 0, 1)

    return Wrapped(generator).eval()


def convert(wrapped, precision: str):
    import coremltools as ct
    import torch

    example = (torch.rand(1, 3, SIDE, SIDE), (torch.rand(1, 1, SIDE, SIDE) > 0.5).float())
    with torch.no_grad():
        traced = torch.jit.trace(wrapped, example)
    mlmodel = ct.convert(
        traced,
        inputs=[ct.TensorType(name="image", shape=(1, 3, SIDE, SIDE)),
                ct.TensorType(name="mask", shape=(1, 1, SIDE, SIDE))],
        outputs=[ct.TensorType(name="filled")],
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16 if precision == "float16" else ct.precision.FLOAT32,
        minimum_deployment_target=ct.target.macOS15,
    )
    mlmodel.short_description = (
        "MI-GAN 512 Places2 (Picsart AI Research, MIT), converted to Core ML by "
        "scripts/convert_migan.py: image [1,3,512,512] RGB 0-1 and mask [1,1,512,512] (1 = remove) "
        "in, filled [1,3,512,512] RGB 0-1 out.")
    mlmodel.user_defined_metadata.pop("com.github.apple.coremltools.conversion_date", None)
    return mlmodel


def hole():
    """The fixed hole: a disc of radius 70 at (300, 220) and a 24-px stroke from (80, 420) to
    (430, 360), 1 inside."""
    import numpy as np

    ys, xs = np.mgrid[0:SIDE, 0:SIDE].astype(np.float32)
    disc = (xs - 300) ** 2 + (ys - 220) ** 2 <= 70 ** 2
    ax, ay, bx, by = 80.0, 420.0, 430.0, 360.0
    dx, dy = bx - ax, by - ay
    t = np.clip(((xs - ax) * dx + (ys - ay) * dy) / (dx * dx + dy * dy), 0, 1)
    stroke = (xs - (ax + t * dx)) ** 2 + (ys - (ay + t * dy)) ** 2 <= 12 ** 2
    return (disc | stroke).astype(np.float32)


def fixtures():
    import numpy as np
    from PIL import Image

    gradient = np.zeros((SIDE, SIDE, 3), dtype=np.uint8)
    gradient[..., 0] = (np.arange(SIDE)[None, :] // 2).astype(np.uint8)
    gradient[..., 1] = (np.arange(SIDE)[:, None] // 2).astype(np.uint8)
    gradient[..., 2] = 128
    images = {"gradient": gradient}
    for name in FIXTURES:
        path = PHOTOS / name
        if not path.is_file():
            raise SystemExit(f"the self-check needs {path} (scripts/phase6_quality_fixtures.py)")
        image = Image.open(path).convert("RGB")
        scale = SIDE / min(image.size)
        size = (max(SIDE, round(image.size[0] * scale)), max(SIDE, round(image.size[1] * scale)))
        image = image.resize(size, Image.LANCZOS)
        left, top = (size[0] - SIDE) // 2, (size[1] - SIDE) // 2
        images[name] = np.asarray(image.crop((left, top, left + SIDE, top + SIDE)))
    return {name: pixels.astype(np.float32).transpose(2, 0, 1)[None] / 255.0
            for name, pixels in images.items()}


def check(mlmodel, wrapped, images, mask) -> dict:
    import numpy as np
    import torch

    inside = mask[0, 0] > 0.5
    results = {}
    for name, image in images.items():
        with torch.no_grad():
            reference = wrapped(torch.from_numpy(image), torch.from_numpy(mask)).numpy()
        got = mlmodel.predict({"image": image, "mask": mask})["filled"]
        delta = np.abs(got - reference)[0][:, inside]
        fill_error = np.abs(got - image)[0][:, inside]
        results[name] = {
            "hole_mean_abs_delta": float(delta.mean()),
            "hole_p99_abs_delta": float(np.percentile(delta, 99)),
            "hole_mean_abs_fill_minus_original": float(fill_error.mean()),
            "finite": bool(np.isfinite(got).all()),
            "in_range": bool(got.min() >= 0 and got.max() <= 1),
        }
        r = results[name]
        print(f"check {name}: hole mean |Δ| {r['hole_mean_abs_delta']:.5f}, p99 |Δ| "
              f"{r['hole_p99_abs_delta']:.5f}, mean |fill − original| "
              f"{r['hole_mean_abs_fill_minus_original']:.4f}", flush=True)
    return results


def passed(results: dict) -> bool:
    return all(r["hole_mean_abs_delta"] <= MEAN_BOUND and r["hole_p99_abs_delta"] <= P99_BOUND
               and r["finite"] and r["in_range"] for r in results.values())


def timing(package: Path, images, mask) -> dict:
    import coremltools as ct

    model = ct.models.MLModel(str(package), compute_units=ct.ComputeUnit.ALL)
    feed = {"image": images["sky.jpg"], "mask": mask}
    model.predict(feed)
    seconds = []
    for _ in range(TIMED_RUNS):
        start = time.perf_counter()
        model.predict(feed)
        seconds.append(time.perf_counter() - start)
    return {"compute_units": "ALL", "warm_up": 1, "runs": TIMED_RUNS,
            "median_seconds": statistics.median(seconds), "seconds": seconds}


def machine() -> dict:
    def sysctl(name):
        done = subprocess.run(["sysctl", "-n", name], capture_output=True, text=True, check=False)
        return done.stdout.strip()
    return {"hw.memsize": int(sysctl("hw.memsize") or 0),
            "chip": sysctl("machdep.cpu.brand_string"),
            "macos": platform.mac_ver()[0]}


def catalog_entry(outputs: dict, precision: str) -> dict:
    return {
        "id": "doctored/migan-512-places2",
        "name": "MI-GAN 512 Places2 (Core ML)",
        "capability": "inpaint",
        "version": RELEASE_TAG,
        "files": [
            {"path": f"{PACKAGE}/{inner}", "url": RELEASE_BASE + outputs[name]["asset"],
             "sha256": outputs[name]["sha256"], "size": outputs[name]["size"]}
            for name, inner in [("Manifest.json", "Manifest.json"),
                                ("model.mlmodel", "Data/com.apple.CoreML/model.mlmodel"),
                                ("weight.bin", "Data/com.apple.CoreML/weights/weight.bin")]
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {"name": "MIT", "url": f"{REPOSITORY}/blob/main/LICENSE-WEIGHTS",
                    "tier": "clean_data_encumbered"},
        "training_data": (
            "Places2 (Places365-Standard, MIT CSAIL; released for non-commercial research and "
            "education): Picsart AI Research trained MI-GAN 512 on its scenes at 512×512. "
            "Doctored converted the TorchScript export of migan_512_places2 to Core ML "
            f"({precision} compute) with scripts/convert_migan.py, retraining nothing."),
        "inference_bytes": INFERENCE_BYTES,
        "packages": [PACKAGE],
    }


def notes(receipt: dict, command: str) -> str:
    lines = [
        f"# MI-GAN 512 Places2, Core ML ({RELEASE_TAG})",
        "",
        "The Remove tool's model. Written by `scripts/convert_migan.py`; publishing the "
        f"release `{RELEASE_TAG}` on kpgalligan/doctored-models is the owner's step.",
        "",
        f"- Source: {SOURCE_URL} (IOPaint's TorchScript export of MI-GAN's `migan_512_places2.pt`; "
        f"the official file is a Google Drive folder, {OFFICIAL_URL}, which no plain HTTPS GET "
        "reaches).",
        f"- Source SHA-256: {SOURCE_SHA256} ({SOURCE_SIZE} bytes).",
        f"- Code and weights: MIT, Picsart AI Research ({REPOSITORY}, `LICENSE-WEIGHTS` at "
        f"{REPOSITORY_COMMIT}, copied here as `LICENSE`).",
        "- Training data: Places2 (Places365-Standard, MIT CSAIL), released for non-commercial "
        "research and education; the weights' own licence is MIT.",
        f"- Compute precision: {receipt['conversion']['precision']}.",
        "",
        "| asset | bytes | sha256 |",
        "|---|---|---|",
    ]
    for entry in receipt["outputs"].values():
        lines.append(f"| {entry['asset']} | {entry['size']} | {entry['sha256']} |")
    lines += ["", f"Command: `{command}`", ""]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True,
                        help="the folder that receives <RELEASE_TAG>/")
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="doctored-migan-"))
    source = fetch_source(scratch)
    license_text = fetch(LICENSE_URL).decode()
    if "MIT License" not in license_text:
        raise SystemExit(f"{LICENSE_URL} is not the MIT licence")
    print(f"source {SOURCE_SHA256}; scratch {scratch}", flush=True)
    wrapped = wrapper(source)
    images = fixtures()
    mask = hole()[None, None]
    attempts = {}
    mlmodel = None
    precision = None
    for candidate in ["float16", "float32"]:
        print(f"converting at {candidate}", flush=True)
        model = convert(wrapped, candidate)
        results = check(model, wrapped, images, mask)
        attempts[candidate] = {"results": results, "passed": passed(results)}
        if attempts[candidate]["passed"]:
            mlmodel, precision = model, candidate
            break

    versions = {name: metadata.version(name) for name in ["torch", "coremltools", "numpy", "pillow"]}
    versions["python"] = sys.version.split()[0]
    receipt = {
        "source": {"url": SOURCE_URL, "sha256": SOURCE_SHA256, "size": SOURCE_SIZE,
                   "official": OFFICIAL_URL, "repository": REPOSITORY,
                   "repository_commit": REPOSITORY_COMMIT, "license_url": LICENSE_URL},
        "tools": versions,
        "check": {"hole_mean_abs_delta_bound": MEAN_BOUND, "hole_p99_abs_delta_bound": P99_BOUND,
                  "attempts": attempts, "passed": mlmodel is not None},
        "release_tag": RELEASE_TAG,
        "machine": machine(),
    }
    if mlmodel is None:
        failed = scratch / "receipt.json"
        failed.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(f"FAIL: the self-check missed its bounds at float16 and float32; no release assets "
              f"written; receipt {failed}", flush=True)
        return 1

    package = scratch / PACKAGE
    mlmodel.save(str(package))
    files = {
        "Manifest.json": package / "Manifest.json",
        "model.mlmodel": package / "Data/com.apple.CoreML/model.mlmodel",
        "weight.bin": package / "Data/com.apple.CoreML/weights/weight.bin",
    }
    outputs = {}
    for name, path in files.items():
        data = path.read_bytes()
        outputs[name] = {"asset": ASSET_PREFIX + name, "sha256": sha256(data), "size": len(data)}
    receipt["conversion"] = {"precision": precision}
    receipt["outputs"] = outputs
    receipt["package_bytes"] = sum(entry["size"] for entry in outputs.values())
    receipt["timing"] = timing(package, images, mask)
    print(f"package {receipt['package_bytes']} B; median prediction "
          f"{receipt['timing']['median_seconds'] * 1000:.1f} ms", flush=True)

    release = output / RELEASE_TAG
    release.mkdir(parents=True, exist_ok=True)
    for name, path in files.items():
        shutil.copyfile(path, release / (ASSET_PREFIX + name))
    (release / "LICENSE").write_text(license_text)
    command = "uv run scripts/convert_migan.py --output " + str(arguments.output)
    (release / "NOTES.md").write_text(notes(receipt, command))
    (release / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    (release / "catalog-entry.json").write_text(
        json.dumps(catalog_entry(outputs, precision), indent=2, ensure_ascii=False) + "\n")
    for entry in outputs.values():
        print(f"asset {RELEASE_TAG}/{entry['asset']} {entry['size']} B sha256 {entry['sha256']}", flush=True)
    print(f"PASS: converted at {precision}; receipt {release / 'receipt.json'}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
