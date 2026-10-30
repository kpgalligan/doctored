# /// script
# requires-python = "==3.11.*"
# dependencies = [
#     "torch==2.5.1",
#     "coremltools==8.3.0",
#     "numpy==1.26.4",
#     "pillow==11.0.0",
# ]
# ///
"""Convert FBCNN (colour) to Core ML for Filter ▸ Neural Filters ▸ JPEG Artifacts Removal (M4). NETWORK.

JPEG Artifacts Removal, and Super Zoom's Remove JPEG Artifacts, run FBCNN (Jiang et al., ICCV
2021, "Towards Flexible Blind JPEG Artifacts Removal"; Apache-2.0 code and weights) on
overlapping 512² tiles that the Rust core cuts and stitches. FBCNN takes an adjustable quality
factor: its test script feeds `qf_input = 1 − QF/100`, which is what Strength Low / Medium /
High maps to. This script is the reproducible conversion, in `scripts/convert_migan.py`'s steps:

  1. downloads `fbcnn_color.pth` from the v1.0 release (SOURCE_SHA256, SOURCE_SIZE), the
     network definition `models/network_fbcnn.py` and the LICENSE at a pinned commit
     (NETWORK_SHA256), and refuses any other bytes;
  2. builds FBCNN(in_nc 3, out_nc 3, nc [64,128,256,512], nb 4) from that file (its unused
     `import torchvision.models` line is dropped, so torchvision is not needed) and loads the
     checkpoint with strict=True;
  3. wraps it as `model(image, qf)[0]`: `image` [1,3,512,512] RGB 0…1 and `qf` [1,1]
     (1 − QF/100) in, `restored` [1,3,512,512] out (the Rust stitch clamps it to 0…1);
  4. traces it and converts it to an mlprogram at FLOAT16 compute precision;
  5. self-checks the package against the float32 PyTorch wrapper on 512² crops of the repo's
     sky.jpg, product.jpg and fur.png (`~/devel/doctored-fixtures/phase6/photos`, scaled so the
     short side is 512, centre crop, then JPEG-compressed by Pillow at quality 30) and a
     synthetic gradient, each at qf 0.3, 0.6 and 0.9 (Low, Medium, High), over the whole output
     clamped to 0…1: mean |Δ| ≤ MEAN_BOUND and p99 |Δ| ≤ P99_BOUND. A miss rebuilds at
     FLOAT32; a miss at both exits 1 and writes no release assets;
  6. measures the package: its bytes, the median of 10 predictions per 512² tile (compute units
     ALL, one warm-up), the machine;
  7. writes `<output>/<RELEASE_TAG>/`: the flat assets
     `fbcnn_color-{Manifest.json,model.mlmodel,weight.bin}`, `LICENSE`, `NOTES.md`,
     `receipt.json` and `catalog-entry.json` (`inference_bytes` is the place-holder until
     `scripts/neural-filters-admission/measure.py` measures it).

Run with `uv run scripts/convert_fbcnn.py --output ~/devel/doctored-fixtures/models-release`;
then `scripts/fill_model_fixtures.py --release-dir <output> --only doctored/fbcnn-color` serves
the assets to the fixture store. Publishing the release is the owner's step: this script
publishes nothing. The Manifest's item identifiers are fresh UUIDs on every save, so a second
run writes different output hashes: the catalogue pins the run whose receipt it cites.
"""

import argparse
import hashlib
from importlib import metadata
import io
import json
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import types
import urllib.request
from pathlib import Path

SOURCE_URL = "https://github.com/jiaxi-jiang/FBCNN/releases/download/v1.0/fbcnn_color.pth"
SOURCE_SHA256 = "8b0e4ef23d59cf7ac934a342cb31a17619e4fa4a0b3374a9d78c5174312387e8"
SOURCE_SIZE = 287755111
REPOSITORY = "https://github.com/jiaxi-jiang/FBCNN"
REPOSITORY_COMMIT = "54d1831927506b3247e2d4d245abb4f4dab1a1cd"
NETWORK_URL = (f"https://raw.githubusercontent.com/jiaxi-jiang/FBCNN/"
               f"{REPOSITORY_COMMIT}/models/network_fbcnn.py")
NETWORK_SHA256 = "e1afc1f6483d57b774365467393216508f29a708b576dc973bad41bae67e8d03"
LICENSE_URL = f"https://raw.githubusercontent.com/jiaxi-jiang/FBCNN/{REPOSITORY_COMMIT}/LICENSE"
RELEASE_TAG = "fbcnn-color-coreml-1"
PACKAGE = "fbcnn_color.mlpackage"
ASSET_PREFIX = "fbcnn_color-"
RELEASE_BASE = f"https://github.com/kpgalligan/doctored-models/releases/download/{RELEASE_TAG}/"
SIDE = 512
# The package against the float32 PyTorch wrapper over every output sample clamped to 0…1:
# convert_migan.py's bounds (a mean within 2.5 codes of 255, a p99 within 0.05).
MEAN_BOUND = 0.01
P99_BOUND = 0.05
QF_INPUTS = [0.3, 0.6, 0.9]
JPEG_QUALITY = 30
TIMED_RUNS = 10
# The place-holder until scripts/neural-filters-admission/measure.py measures the figure.
INFERENCE_BYTES = 209715200

PHOTOS = Path.home() / "devel/doctored-fixtures/phase6/photos"
FIXTURES = ["sky.jpg", "product.jpg", "fur.png"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=900) as response:
        return response.read()


def fetch_source(scratch: Path) -> Path:
    data = fetch(SOURCE_URL)
    if len(data) != SOURCE_SIZE or sha256(data) != SOURCE_SHA256:
        raise SystemExit(f"the source is not the pinned file: {len(data)} bytes, sha256 {sha256(data)}")
    path = scratch / "fbcnn_color.pth"
    path.write_bytes(data)
    return path


def network_module():
    data = fetch(NETWORK_URL)
    if sha256(data) != NETWORK_SHA256:
        raise SystemExit(f"{NETWORK_URL} is not the pinned file: sha256 {sha256(data)}")
    text = data.decode()
    unused = "import torchvision.models as models\n"
    if unused not in text:
        raise SystemExit("network_fbcnn.py no longer has the torchvision import this script drops")
    module = types.ModuleType("network_fbcnn")
    exec(compile(text.replace(unused, ""), "network_fbcnn.py", "exec"), module.__dict__)
    return module


def wrapper(source: Path):
    import torch

    network = network_module().FBCNN(in_nc=3, out_nc=3, nc=[64, 128, 256, 512], nb=4,
                                     act_mode="R")
    state = torch.load(str(source), map_location="cpu", weights_only=True)
    network.load_state_dict(state, strict=True)
    network.eval()

    class Wrapped(torch.nn.Module):
        """FBCNN's restored image at a given quality input (1 − QF/100)."""

        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, image, qf):
            return self.inner(image, qf)[0]

    return Wrapped(network).eval()


def convert(wrapped, precision: str):
    import coremltools as ct
    import torch

    example = (torch.rand(1, 3, SIDE, SIDE), torch.tensor([[0.6]]))
    with torch.no_grad():
        traced = torch.jit.trace(wrapped, example)
    mlmodel = ct.convert(
        traced,
        inputs=[ct.TensorType(name="image", shape=(1, 3, SIDE, SIDE)),
                ct.TensorType(name="qf", shape=(1, 1))],
        outputs=[ct.TensorType(name="restored")],
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16 if precision == "float16" else ct.precision.FLOAT32,
        minimum_deployment_target=ct.target.macOS15,
    )
    mlmodel.short_description = (
        "FBCNN colour (Jiang et al., Apache-2.0), converted to Core ML by scripts/convert_fbcnn.py: "
        "image [1,3,512,512] RGB 0-1 and qf [1,1] (1 - QF/100) in, restored [1,3,512,512] out.")
    mlmodel.user_defined_metadata.pop("com.github.apple.coremltools.conversion_date", None)
    return mlmodel


def fixtures():
    import numpy as np
    from PIL import Image

    gradient = np.zeros((SIDE, SIDE, 3), dtype=np.uint8)
    gradient[..., 0] = (np.arange(SIDE)[None, :] // 2).astype(np.uint8)
    gradient[..., 1] = (np.arange(SIDE)[:, None] // 2).astype(np.uint8)
    gradient[..., 2] = 128
    images = {"gradient": Image.fromarray(gradient)}
    for name in FIXTURES:
        path = PHOTOS / name
        if not path.is_file():
            raise SystemExit(f"the self-check needs {path} (scripts/phase6_quality_fixtures.py)")
        image = Image.open(path).convert("RGB")
        scale = SIDE / min(image.size)
        size = (max(SIDE, round(image.size[0] * scale)), max(SIDE, round(image.size[1] * scale)))
        image = image.resize(size, Image.LANCZOS)
        left, top = (size[0] - SIDE) // 2, (size[1] - SIDE) // 2
        images[name] = image.crop((left, top, left + SIDE, top + SIDE))
    planes = {}
    for name, image in images.items():
        encoded = io.BytesIO()
        image.save(encoded, format="JPEG", quality=JPEG_QUALITY)
        decoded = np.asarray(Image.open(io.BytesIO(encoded.getvalue())).convert("RGB"))
        planes[name] = decoded.astype(np.float32).transpose(2, 0, 1)[None] / 255.0
    return planes


def check(mlmodel, wrapped, images) -> dict:
    import numpy as np
    import torch

    results = {}
    for name, image in images.items():
        for qf in QF_INPUTS:
            q = np.array([[qf]], dtype=np.float32)
            with torch.no_grad():
                reference = wrapped(torch.from_numpy(image), torch.from_numpy(q)).numpy()
            got = mlmodel.predict({"image": image, "qf": q})["restored"]
            delta = np.abs(np.clip(got, 0, 1) - np.clip(reference, 0, 1))
            key = f"{name}@qf{qf}"
            results[key] = {
                "mean_abs_delta": float(delta.mean()),
                "p99_abs_delta": float(np.percentile(delta, 99)),
                "mean_abs_restored_minus_input": float(np.abs(np.clip(got, 0, 1) - image).mean()),
                "shape": list(got.shape),
                "finite": bool(np.isfinite(got).all()),
            }
            r = results[key]
            print(f"check {key}: mean |Δ| {r['mean_abs_delta']:.5f}, p99 |Δ| "
                  f"{r['p99_abs_delta']:.5f}, mean |restored − input| "
                  f"{r['mean_abs_restored_minus_input']:.4f}", flush=True)
    return results


def passed(results: dict) -> bool:
    shape = [1, 3, SIDE, SIDE]
    return all(r["mean_abs_delta"] <= MEAN_BOUND and r["p99_abs_delta"] <= P99_BOUND
               and r["finite"] and r["shape"] == shape for r in results.values())


def timing(package: Path, images) -> dict:
    import coremltools as ct
    import numpy as np

    model = ct.models.MLModel(str(package), compute_units=ct.ComputeUnit.ALL)
    feed = {"image": images["sky.jpg"], "qf": np.array([[0.6]], dtype=np.float32)}
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
        "id": "doctored/fbcnn-color",
        "name": "FBCNN colour (Core ML)",
        "capability": "jpeg_artifacts",
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
        "license": {"name": "Apache-2.0", "url": f"{REPOSITORY}/blob/main/LICENSE",
                    "tier": "clean_data_encumbered"},
        "training_data": (
            "DIV2K and Flickr2K (options/train_fbcnn_color.json's Data_DIV2K_Flickr2K), "
            "JPEG-compressed at random quality factors during training (the paper); DIV2K is "
            "released for academic research. Doctored converted fbcnn_color.pth to Core ML "
            f"({precision} compute) with scripts/convert_fbcnn.py, retraining nothing."),
        "inference_bytes": INFERENCE_BYTES,
        "packages": [PACKAGE],
    }


def notes(receipt: dict, command: str) -> str:
    lines = [
        f"# FBCNN colour, Core ML ({RELEASE_TAG})",
        "",
        "JPEG Artifacts Removal's model. Written by `scripts/convert_fbcnn.py`; publishing "
        f"the release `{RELEASE_TAG}` on kpgalligan/doctored-models is the owner's step.",
        "",
        f"- Source: {SOURCE_URL}.",
        f"- Source SHA-256: {SOURCE_SHA256} ({SOURCE_SIZE} bytes).",
        f"- Network: `models/network_fbcnn.py` at {REPOSITORY_COMMIT} (SHA-256 {NETWORK_SHA256}), "
        "FBCNN(nc [64,128,256,512], nb 4), loaded with strict=True; the package is "
        "`model(image, qf)[0]`, qf = 1 − QF/100.",
        f"- Code and weights: Apache-2.0, Jiaxi Jiang ({REPOSITORY}, `LICENSE` at "
        f"{REPOSITORY_COMMIT}, copied here as `LICENSE`).",
        "- Training data: DIV2K and Flickr2K (`options/train_fbcnn_color.json`, "
        "`Data_DIV2K_Flickr2K`), compressed with JPEG at random quality factors (the paper).",
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
    scratch = Path(tempfile.mkdtemp(prefix="doctored-fbcnn-"))
    source = fetch_source(scratch)
    license_text = fetch(LICENSE_URL).decode()
    if "Apache License" not in license_text or "Version 2.0" not in license_text:
        raise SystemExit(f"{LICENSE_URL} is not the Apache-2.0 licence")
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
                   "network_url": NETWORK_URL, "network_sha256": NETWORK_SHA256,
                   "license_url": LICENSE_URL},
        "tools": versions,
        "check": {"mean_abs_delta_bound": MEAN_BOUND, "p99_abs_delta_bound": P99_BOUND,
                  "jpeg_quality": JPEG_QUALITY, "qf_inputs": QF_INPUTS,
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
    command = "uv run scripts/convert_fbcnn.py --output " + str(arguments.output)
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
