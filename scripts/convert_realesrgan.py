# /// script
# requires-python = "==3.11.*"
# dependencies = [
#     "torch==2.5.1",
#     "coremltools==8.3.0",
#     "numpy==1.26.4",
#     "pillow==11.0.0",
# ]
# ///
"""Convert Real-ESRGAN x4plus to Core ML for Filter ▸ Neural Filters ▸ Super Zoom (M4). NETWORK.

Super Zoom's Enhance Image Details runs Real-ESRGAN x4plus (Wang et al., ICCVW 2021; Xintao
Wang, BSD-3-Clause code; the README states no separate weights licence, so the weights are
recorded under the repository's BSD-3-Clause) on overlapping 256² tiles that the Rust core cuts
and stitches. This script is the reproducible conversion, in `scripts/convert_migan.py`'s steps:

  1. downloads `RealESRGAN_x4plus.pth` from the v0.1.0 release and refuses any other bytes
     (SOURCE_SHA256, SOURCE_SIZE), and the repository's LICENSE at a pinned commit;
  2. builds RRDBNet(num_in_ch 3, num_out_ch 3, num_feat 64, num_block 23, num_grow_ch 32,
     scale 4), vendored below from BasicSR (`basicsr/archs/rrdbnet_arch.py`, Xintao Wang,
     Apache-2.0; only the scale-4 path), and loads the checkpoint's `params_ema` with
     strict=True, so a key or shape the vendored copy got wrong refuses the run;
  3. wraps it so the package takes a plain plane: `image` [1,3,256,256] RGB 0…1 in,
     `upscaled` [1,3,1024,1024] = clamp(G(image), 0, 1) out;
  4. traces it and converts it to an mlprogram at FLOAT16 compute precision;
  5. self-checks the package against the float32 PyTorch wrapper on 256² crops of the repo's
     sky.jpg, product.jpg and fur.png (`~/devel/doctored-fixtures/phase6/photos`, scaled so the
     short side is 256, centre crop) and a synthetic gradient, over the whole output:
     mean |Δ| ≤ MEAN_BOUND and p99 |Δ| ≤ P99_BOUND. A miss rebuilds at FLOAT32; a miss at both
     exits 1 and writes no release assets;
  6. measures the package: its bytes, the median of 10 predictions per 256² tile (compute units
     ALL, one warm-up), the machine;
  7. writes `<output>/<RELEASE_TAG>/`: the flat assets
     `realesrgan_x4plus-{Manifest.json,model.mlmodel,weight.bin}` (the files `ModelCatalog`'s
     entry downloads from
     https://github.com/kpgalligan/doctored-models/releases/download/<RELEASE_TAG>/),
     `LICENSE`, `NOTES.md`, `receipt.json` and `catalog-entry.json` (`inference_bytes` is the
     place-holder until `scripts/neural-filters-admission/measure.py` measures it).

Run with `uv run scripts/convert_realesrgan.py --output ~/devel/doctored-fixtures/models-release`;
then `scripts/fill_model_fixtures.py --release-dir <output> --only doctored/realesrgan-x4plus`
serves the assets to the fixture store. Publishing the release is the owner's step: this script
publishes nothing. The Manifest's item identifiers are fresh UUIDs on every save, so a second
run writes different output hashes: the catalogue pins the run whose receipt it cites.
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

SOURCE_URL = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.1.0/RealESRGAN_x4plus.pth"
SOURCE_SHA256 = "4fa0d38905f75ac06eb49a7951b426670021be3018265fd191d2125df9d682f1"
SOURCE_SIZE = 67040989
REPOSITORY = "https://github.com/xinntao/Real-ESRGAN"
REPOSITORY_COMMIT = "a4abfb2979a7bbff3f69f58f58ae324608821e27"
LICENSE_URL = (f"https://raw.githubusercontent.com/xinntao/Real-ESRGAN/"
               f"{REPOSITORY_COMMIT}/LICENSE")
RELEASE_TAG = "realesrgan-x4plus-coreml-1"
PACKAGE = "realesrgan_x4plus.mlpackage"
ASSET_PREFIX = "realesrgan_x4plus-"
RELEASE_BASE = f"https://github.com/kpgalligan/doctored-models/releases/download/{RELEASE_TAG}/"
SIDE = 256
SCALE = 4
# The package against the float32 PyTorch wrapper over every output sample: a mean within 0.01
# (2.5 codes of 255) and a 99th percentile within 0.05 is an upscale a viewer cannot tell from
# the reference's (convert_migan.py's bounds).
MEAN_BOUND = 0.01
P99_BOUND = 0.05
TIMED_RUNS = 10
# The place-holder until scripts/neural-filters-admission/measure.py measures the figure.
INFERENCE_BYTES = 209715200

PHOTOS = Path.home() / "devel/doctored-fixtures/phase6/photos"
FIXTURES = ["sky.jpg", "product.jpg", "fur.png"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=600) as response:
        return response.read()


def fetch_source(scratch: Path) -> Path:
    data = fetch(SOURCE_URL)
    if len(data) != SOURCE_SIZE or sha256(data) != SOURCE_SHA256:
        raise SystemExit(f"the source is not the pinned file: {len(data)} bytes, sha256 {sha256(data)}")
    path = scratch / "RealESRGAN_x4plus.pth"
    path.write_bytes(data)
    return path


def rrdbnet():
    """BasicSR's RRDBNet (basicsr/archs/rrdbnet_arch.py, Xintao Wang, Apache-2.0), scale 4 only."""
    import torch
    from torch import nn
    from torch.nn import functional as F

    class ResidualDenseBlock(nn.Module):
        def __init__(self, num_feat=64, num_grow_ch=32):
            super().__init__()
            self.conv1 = nn.Conv2d(num_feat, num_grow_ch, 3, 1, 1)
            self.conv2 = nn.Conv2d(num_feat + num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv3 = nn.Conv2d(num_feat + 2 * num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv4 = nn.Conv2d(num_feat + 3 * num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv5 = nn.Conv2d(num_feat + 4 * num_grow_ch, num_feat, 3, 1, 1)
            self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

        def forward(self, x):
            x1 = self.lrelu(self.conv1(x))
            x2 = self.lrelu(self.conv2(torch.cat((x, x1), 1)))
            x3 = self.lrelu(self.conv3(torch.cat((x, x1, x2), 1)))
            x4 = self.lrelu(self.conv4(torch.cat((x, x1, x2, x3), 1)))
            x5 = self.conv5(torch.cat((x, x1, x2, x3, x4), 1))
            return x5 * 0.2 + x

    class RRDB(nn.Module):
        def __init__(self, num_feat, num_grow_ch=32):
            super().__init__()
            self.rdb1 = ResidualDenseBlock(num_feat, num_grow_ch)
            self.rdb2 = ResidualDenseBlock(num_feat, num_grow_ch)
            self.rdb3 = ResidualDenseBlock(num_feat, num_grow_ch)

        def forward(self, x):
            out = self.rdb1(x)
            out = self.rdb2(out)
            out = self.rdb3(out)
            return out * 0.2 + x

    class RRDBNet(nn.Module):
        def __init__(self, num_in_ch=3, num_out_ch=3, num_feat=64, num_block=23, num_grow_ch=32):
            super().__init__()
            self.conv_first = nn.Conv2d(num_in_ch, num_feat, 3, 1, 1)
            self.body = nn.Sequential(*[RRDB(num_feat, num_grow_ch) for _ in range(num_block)])
            self.conv_body = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_up1 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_up2 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_hr = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_last = nn.Conv2d(num_feat, num_out_ch, 3, 1, 1)
            self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

        def forward(self, x):
            feat = self.conv_first(x)
            body_feat = self.conv_body(self.body(feat))
            feat = feat + body_feat
            feat = self.lrelu(self.conv_up1(F.interpolate(feat, scale_factor=2, mode="nearest")))
            feat = self.lrelu(self.conv_up2(F.interpolate(feat, scale_factor=2, mode="nearest")))
            return self.conv_last(self.lrelu(self.conv_hr(feat)))

    return RRDBNet()


def wrapper(source: Path):
    import torch

    network = rrdbnet()
    checkpoint = torch.load(str(source), map_location="cpu", weights_only=True)
    network.load_state_dict(checkpoint["params_ema"], strict=True)
    network.eval()

    class Wrapped(torch.nn.Module):
        """A plain 0…1 plane in, the ×4 plane clamped to 0…1 out."""

        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, image):
            return torch.clamp(self.inner(image), 0, 1)

    return Wrapped(network).eval()


def convert(wrapped, precision: str):
    import coremltools as ct
    import torch

    example = torch.rand(1, 3, SIDE, SIDE)
    with torch.no_grad():
        traced = torch.jit.trace(wrapped, example)
    mlmodel = ct.convert(
        traced,
        inputs=[ct.TensorType(name="image", shape=(1, 3, SIDE, SIDE))],
        outputs=[ct.TensorType(name="upscaled")],
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16 if precision == "float16" else ct.precision.FLOAT32,
        minimum_deployment_target=ct.target.macOS15,
    )
    mlmodel.short_description = (
        "Real-ESRGAN x4plus (Xintao Wang, BSD-3-Clause), converted to Core ML by "
        "scripts/convert_realesrgan.py: image [1,3,256,256] RGB 0-1 in, upscaled "
        "[1,3,1024,1024] RGB 0-1 out.")
    mlmodel.user_defined_metadata.pop("com.github.apple.coremltools.conversion_date", None)
    return mlmodel


def fixtures():
    import numpy as np
    from PIL import Image

    gradient = np.zeros((SIDE, SIDE, 3), dtype=np.uint8)
    gradient[..., 0] = np.arange(SIDE)[None, :].astype(np.uint8)
    gradient[..., 1] = np.arange(SIDE)[:, None].astype(np.uint8)
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


def check(mlmodel, wrapped, images) -> dict:
    import numpy as np
    import torch

    results = {}
    for name, image in images.items():
        with torch.no_grad():
            reference = wrapped(torch.from_numpy(image)).numpy()
        got = mlmodel.predict({"image": image})["upscaled"]
        delta = np.abs(got - reference)
        results[name] = {
            "mean_abs_delta": float(delta.mean()),
            "p99_abs_delta": float(np.percentile(delta, 99)),
            "shape": list(got.shape),
            "finite": bool(np.isfinite(got).all()),
            "in_range": bool(got.min() >= 0 and got.max() <= 1),
        }
        r = results[name]
        print(f"check {name}: mean |Δ| {r['mean_abs_delta']:.5f}, p99 |Δ| "
              f"{r['p99_abs_delta']:.5f}", flush=True)
    return results


def passed(results: dict) -> bool:
    shape = [1, 3, SIDE * SCALE, SIDE * SCALE]
    return all(r["mean_abs_delta"] <= MEAN_BOUND and r["p99_abs_delta"] <= P99_BOUND
               and r["finite"] and r["in_range"] and r["shape"] == shape for r in results.values())


def timing(package: Path, images) -> dict:
    import coremltools as ct

    model = ct.models.MLModel(str(package), compute_units=ct.ComputeUnit.ALL)
    feed = {"image": images["sky.jpg"]}
    model.predict(feed)
    seconds = []
    for _ in range(TIMED_RUNS):
        start = time.perf_counter()
        model.predict(feed)
        seconds.append(time.perf_counter() - start)
    return {"compute_units": "ALL", "warm_up": 1, "runs": TIMED_RUNS, "tile": SIDE,
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
        "id": "doctored/realesrgan-x4plus",
        "name": "Real-ESRGAN x4plus (Core ML)",
        "capability": "super_resolution",
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
        "license": {"name": "BSD-3-Clause", "url": f"{REPOSITORY}/blob/master/LICENSE",
                    "tier": "clean_data_encumbered"},
        "training_data": (
            "DF2K (DIV2K and Flickr2K) and OST, with Real-ESRGAN's synthetic second-order "
            "degradations (the repository's docs/Training.md); DIV2K is released for academic "
            "research. The README states no separate weights licence, so the weights are recorded "
            "under the repository's BSD-3-Clause. Doctored converted RealESRGAN_x4plus.pth to "
            f"Core ML ({precision} compute) with scripts/convert_realesrgan.py, retraining nothing."),
        "inference_bytes": INFERENCE_BYTES,
        "packages": [PACKAGE],
    }


def notes(receipt: dict, command: str) -> str:
    lines = [
        f"# Real-ESRGAN x4plus, Core ML ({RELEASE_TAG})",
        "",
        "Super Zoom's model. Written by `scripts/convert_realesrgan.py`; publishing the "
        f"release `{RELEASE_TAG}` on kpgalligan/doctored-models is the owner's step.",
        "",
        f"- Source: {SOURCE_URL}.",
        f"- Source SHA-256: {SOURCE_SHA256} ({SOURCE_SIZE} bytes).",
        f"- Code: BSD-3-Clause, Xintao Wang ({REPOSITORY}, `LICENSE` at {REPOSITORY_COMMIT}, "
        "copied here as `LICENSE`).",
        "- Weights licence: the README states no separate weights licence, so the weights are "
        "recorded under the repository's BSD-3-Clause.",
        "- Network: RRDBNet (23 RRDB blocks, 64 features, growth 32, ×4), vendored from BasicSR "
        "(`basicsr/archs/rrdbnet_arch.py`, Apache-2.0), loaded from `params_ema` with strict=True.",
        "- Training data: DF2K (DIV2K and Flickr2K) and OST, with synthetic degradations "
        "(docs/Training.md: \"We use DF2K (DIV2K and Flickr2K) + OST datasets for our training\").",
        f"- Compute precision: {receipt['conversion']['precision']}.",
        f"- Median prediction per {SIDE}² tile: {receipt['timing']['median_seconds'] * 1000:.1f} ms "
        f"(compute units ALL, {receipt['machine']['chip']}).",
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
    scratch = Path(tempfile.mkdtemp(prefix="doctored-realesrgan-"))
    source = fetch_source(scratch)
    license_text = fetch(LICENSE_URL).decode()
    if "BSD 3-Clause License" not in license_text:
        raise SystemExit(f"{LICENSE_URL} is not the BSD-3-Clause licence")
    print(f"source {SOURCE_SHA256}; scratch {scratch}", flush=True)
    wrapped = wrapper(source)
    images = fixtures()
    attempts = {}
    mlmodel = None
    precision = None
    for candidate in ["float16", "float32"]:
        print(f"converting at {candidate}", flush=True)
        model = convert(wrapped, candidate)
        results = check(model, wrapped, images)
        attempts[candidate] = {"results": results, "passed": passed(results)}
        if attempts[candidate]["passed"]:
            mlmodel, precision = model, candidate
            break

    versions = {name: metadata.version(name) for name in ["torch", "coremltools", "numpy", "pillow"]}
    versions["python"] = sys.version.split()[0]
    receipt = {
        "source": {"url": SOURCE_URL, "sha256": SOURCE_SHA256, "size": SOURCE_SIZE,
                   "repository": REPOSITORY, "repository_commit": REPOSITORY_COMMIT,
                   "license_url": LICENSE_URL},
        "tools": versions,
        "check": {"mean_abs_delta_bound": MEAN_BOUND, "p99_abs_delta_bound": P99_BOUND,
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
    receipt["timing"] = timing(package, images)
    print(f"package {receipt['package_bytes']} B; median prediction "
          f"{receipt['timing']['median_seconds'] * 1000:.1f} ms", flush=True)

    release = output / RELEASE_TAG
    release.mkdir(parents=True, exist_ok=True)
    for name, path in files.items():
        shutil.copyfile(path, release / (ASSET_PREFIX + name))
    (release / "LICENSE").write_text(license_text)
    command = "uv run scripts/convert_realesrgan.py --output " + str(arguments.output)
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
