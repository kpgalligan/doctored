# /// script
# requires-python = "==3.11.*"
# dependencies = [
#     "torch==2.5.1",
#     "coremltools==8.3.0",
#     "numpy==1.26.4",
#     "pillow==11.0.0",
# ]
# ///
"""Convert DDColor to Core ML for Filter ▸ Neural Filters ▸ Colorize (M4 phase 2). NETWORK.

Colorize runs DDColor (Kang et al., ICCV 2023, "DDColor: Towards Photo-Realistic Image
Colorization via Dual Decoders"; Apache-2.0 code and weights, trained on ImageNet) once on the
layer's luminance at 512²; the Rust core carries the predicted Lab chroma onto the full-resolution
L*. This script is the reproducible conversion, in `scripts/convert_fbcnn.py`'s steps:

  1. downloads the variant's `pytorch_model.bin` from Hugging Face at a pinned commit
     (VARIANTS: URL, size and SHA-256, listed through `https://huggingface.co/api/models/...`),
     and the network files and LICENSE from github.com/piddnad/DDColor at a pinned commit
     (VENDORED, each file's SHA-256), and refuses any other bytes;
  2. builds DDColor with `scripts/export_onnx.py`'s constructor arguments for the variant's
     size (encoder convnext-t or convnext-l, MultiScaleColorDecoder, input 512², two output
     channels, Spectral last norm, no normalisation, 100 queries, 3 scales, 9 layers) and loads
     the checkpoint with strict=True; the spectral norms are then folded into their weights
     (`remove_spectral_norm`, eval mode: no power iteration), so the fp16 program does not
     recompute σ;
  3. wraps it as `ab = model(image)`: `image` [1,3,512,512] the gray sRGB 0…1 of Lab(L, 0, 0)
     in (DDColor's `ColorizationPipeline` builds exactly that), `ab` [1,2,512,512] Lab a*, b* out;
  4. traces it and converts it to an mlprogram at FLOAT16 compute precision;
  5. self-checks the package against the float32 PyTorch model (spectral norms not yet folded)
     on 512² crops of the repo's sky.jpg, product.jpg and fur.png
     (`~/devel/doctored-fixtures/phase6/photos`, scaled so the short side is 512, centre crop,
     made gray as the pipeline makes it) and a synthetic gradient: mean |Δab| ≤ MEAN_BOUND and
     p99 |Δab| ≤ P99_BOUND. A miss rebuilds at FLOAT32; a miss at both exits 1 and writes no
     release assets;
  6. measures the package: its bytes, the median of 10 predictions (compute units ALL, one
     warm-up), the machine, and its QUALITY: for each photo the mean |Δab| of the Core ML ab
     against the photo's own ab and the mean predicted chroma;
  7. writes `<output>/<release tag>/`: the flat assets
     `ddcolor-{Manifest.json,model.mlmodel,weight.bin}`, `LICENSE`, `NOTES.md`, `receipt.json`
     and `catalog-entry.json` (`inference_bytes` is the place-holder until
     `scripts/neural-filters-admission/measure.py` measures it).

The variant is chosen on those measurements by a rule fixed before measuring: the smallest
package, unless a larger one's mean |Δab| against the photos is at least 10 % lower; any package
over 1.5 GB or slower than 3 s per prediction is excluded. `ddcolor_artistic` is not a
candidate: its card names no training data beyond ImageNet plus "private data" (MODEL_ZOO.md).

Run with `uv run scripts/convert_ddcolor.py --variant paper_tiny --output <folder>`; then
`scripts/fill_model_fixtures.py --release-dir <folder> --only doctored/ddcolor` serves the
assets to the fixture store, and `scripts/verify_neural_colorize.py` drives the result.
Publishing the release is the owner's step: this script publishes nothing. The Manifest's item
identifiers are fresh UUIDs on every save, so a second run writes different output hashes: the
catalogue pins the run whose receipt it cites.
"""

import argparse
import copy
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

REPOSITORY = "https://github.com/piddnad/DDColor"
REPOSITORY_COMMIT = "2adb63f2656ac41cbdf7b894cddd94121a3faf13"
RAW = f"https://raw.githubusercontent.com/piddnad/DDColor/{REPOSITORY_COMMIT}/"
# Each vendored file at REPOSITORY_COMMIT, with its SHA-256; laid out as the repository does so
# `ddcolor/model.py`'s own `from basicsr.archs.ddcolor_arch_utils...` imports resolve unchanged.
VENDORED = {
    "ddcolor/model.py": "e8e30115aa65a9558db33641c999001d1735c444402890898b47b2cc52b37c25",
    "basicsr/archs/ddcolor_arch_utils/unet.py":
        "2b72fd6ef60d73f033cd3b2a3b4737d5371d4190b6410883e4d987d8417df1b2",
    "basicsr/archs/ddcolor_arch_utils/convnext.py":
        "f03e339488570aa79e0690719546317a375ca85bd8a9e1e5f2c797cda033db45",
    "basicsr/archs/ddcolor_arch_utils/transformer_utils.py":
        "61d99be451ef0717eaa1bf84ecfd29f9c4224ac1e5a35d68a963c4b97f4dbd0b",
    "basicsr/archs/ddcolor_arch_utils/position_encoding.py":
        "beb5b3b52f2cc4f2dfc9f312cdc5d712468ff1d2985be7471a94a0c37a8c01d6",
}
LICENSE_URL = RAW + "LICENSE"
LICENSE_SHA256 = "43070e2d4e532684de521b885f385d0841030efa2b1a20bafb76133a5e1379c1"
LICENSE_PAGE = f"{REPOSITORY}/blob/master/LICENSE"
# Hugging Face: repository, commit (the API's `sha`), file size and LFS SHA-256.
VARIANTS = {
    "paper_tiny": {"repo": "piddnad/ddcolor_paper_tiny",
                   "commit": "cf9fd99c1d7472689ec7413441c1b799a51866a3",
                   "size": 220372845,
                   "sha256": "8a1277bc90a1bfbb6d2d83933a9a6bc821931879ca93e26e4fcec12165d41fce",
                   "encoder": "convnext-t", "label": "DDColor-T paper"},
    "paper": {"repo": "piddnad/ddcolor_paper",
              "commit": "41d54a9fb98d9f3287eb26289931273181623196",
              "size": 911914869,
              "sha256": "5c77db8292398a2ec1397003f0a90266c28e97b013515d5e9a32e73a4c54a67d",
              "encoder": "convnext-l", "label": "DDColor-L paper"},
    "modelscope": {"repo": "piddnad/ddcolor_modelscope",
                   "commit": "060f67494e31883a4b13cb27f889f3154847ada4",
                   "size": 911914869,
                   "sha256": "d81711971ec59200da26d5e8a1afae8dd3778d495ea8ad7a7dadc769f403f7e7",
                   "encoder": "convnext-l", "label": "DDColor-L ModelScope"},
}
PACKAGE = "ddcolor.mlpackage"
ASSET_PREFIX = "ddcolor-"
SIDE = 512
# The package against the float32 PyTorch model over every a*, b* sample, in Lab units: half a
# unit on average (well under one just-noticeable ΔE) and 2 units at the 99th percentile.
MEAN_BOUND = 0.5
P99_BOUND = 2.0
TIMED_RUNS = 10
# The place-holder until scripts/neural-filters-admission/measure.py measures the figure.
INFERENCE_BYTES = 209715200

PHOTOS = Path.home() / "devel/doctored-fixtures/phase6/photos"
FIXTURES = ["sky.jpg", "product.jpg", "fur.png"]

# OpenCV's float sRGB ↔ Lab (D65), which DDColor's pipeline and training use.
RGB_TO_XYZ = [[0.412453, 0.357580, 0.180423],
              [0.212671, 0.715160, 0.072169],
              [0.019334, 0.119193, 0.950227]]
WHITE = (0.950456, 1.0, 1.088754)


def release_tag(variant: str) -> str:
    return f"ddcolor-{variant.replace('_', '-')}-coreml-1"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=1800) as response:
        return response.read()


def source_url(variant: str) -> str:
    entry = VARIANTS[variant]
    return f"https://huggingface.co/{entry['repo']}/resolve/{entry['commit']}/pytorch_model.bin"


def fetch_source(variant: str, scratch: Path) -> Path:
    entry = VARIANTS[variant]
    data = fetch(source_url(variant))
    if len(data) != entry["size"] or sha256(data) != entry["sha256"]:
        raise SystemExit(f"the source is not the pinned file: {len(data)} bytes, sha256 {sha256(data)}")
    path = scratch / "pytorch_model.bin"
    path.write_bytes(data)
    return path


def vendor(scratch: Path) -> Path:
    root = scratch / "ddcolor_src"
    for relative, digest in VENDORED.items():
        data = fetch(RAW + relative)
        if sha256(data) != digest:
            raise SystemExit(f"{RAW + relative} is not the pinned file: sha256 {sha256(data)}")
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    for package in ["ddcolor", "basicsr", "basicsr/archs", "basicsr/archs/ddcolor_arch_utils"]:
        (root / package / "__init__.py").touch()
    return root


def network(source: Path, variant: str, root: Path):
    import torch

    sys.path.insert(0, str(root))
    from ddcolor.model import DDColor  # noqa: E402  (the vendored file)

    model = DDColor(encoder_name=VARIANTS[variant]["encoder"],
                    decoder_name="MultiScaleColorDecoder", input_size=[SIDE, SIDE],
                    num_output_channels=2, last_norm="Spectral", do_normalize=False,
                    num_queries=100, num_scales=3, dec_layers=9)
    state = torch.load(str(source), map_location="cpu", weights_only=True)
    if isinstance(state, dict) and "params" in state:
        state = state["params"]
    model.load_state_dict(state, strict=True)
    model.eval()
    return model


def folded(model):
    import importlib

    import torch

    # `torch.nn.utils.spectral_norm` names the function; the hook class lives in its module.
    hook_class = importlib.import_module("torch.nn.utils.spectral_norm").SpectralNorm
    folded_model = copy.deepcopy(model)
    count = 0
    for module in folded_model.modules():
        for hook in list(module._forward_pre_hooks.values()):
            if isinstance(hook, hook_class):
                torch.nn.utils.remove_spectral_norm(module, name=hook.name)
                count += 1
    return folded_model.eval(), count


def convert(model, precision: str):
    import coremltools as ct
    import torch

    example = torch.rand(1, 3, SIDE, SIDE)
    with torch.no_grad():
        traced = torch.jit.trace(model, example)
    mlmodel = ct.convert(
        traced,
        inputs=[ct.TensorType(name="image", shape=(1, 3, SIDE, SIDE))],
        outputs=[ct.TensorType(name="ab")],
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16 if precision == "float16" else ct.precision.FLOAT32,
        minimum_deployment_target=ct.target.macOS15,
    )
    mlmodel.short_description = (
        "DDColor (Kang et al., Apache-2.0), converted to Core ML by scripts/convert_ddcolor.py: "
        "image [1,3,512,512] gray sRGB 0-1 in, ab [1,2,512,512] Lab a*, b* out.")
    mlmodel.user_defined_metadata.pop("com.github.apple.coremltools.conversion_date", None)
    return mlmodel


def srgb_to_lab(rgb):
    import numpy as np

    linear = np.where(rgb > 0.04045, ((rgb + 0.055) / 1.055) ** 2.4, rgb / 12.92)
    xyz = linear @ np.array(RGB_TO_XYZ).T / np.array(WHITE)
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16.0 / 116.0)
    lightness = np.where(xyz[..., 1] > 0.008856, 116.0 * f[..., 1] - 16.0, 903.3 * xyz[..., 1])
    return np.stack([lightness, 500.0 * (f[..., 0] - f[..., 1]),
                     200.0 * (f[..., 1] - f[..., 2])], axis=-1)


def gray_rgb(lightness):
    """The sRGB of Lab(L, 0, 0): a neutral, so one channel value; DDColor's model input."""
    import numpy as np

    fy = (lightness + 16.0) / 116.0
    y = np.where(fy > 6.0 / 29.0, fy ** 3, (fy - 16.0 / 116.0) / 7.787)
    value = np.where(y > 0.0031308, 1.055 * np.power(np.maximum(y, 0), 1 / 2.4) - 0.055, 12.92 * y)
    return np.clip(value, 0.0, 1.0)


def fixtures():
    import numpy as np
    from PIL import Image

    gradient = np.zeros((SIDE, SIDE, 3), dtype=np.float32)
    gradient[..., 0] = np.arange(SIDE)[None, :] / (SIDE - 1)
    gradient[..., 1] = np.arange(SIDE)[:, None] / (SIDE - 1)
    gradient[..., 2] = 0.5
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
        crop = image.crop((left, top, left + SIDE, top + SIDE))
        images[name] = np.asarray(crop).astype(np.float32) / 255.0
    prepared = {}
    for name, rgb in images.items():
        lab = srgb_to_lab(rgb.astype(np.float64))
        gray = gray_rgb(lab[..., 0]).astype(np.float32)
        plane = np.repeat(gray[None, None], 3, axis=1)
        prepared[name] = {"image": plane, "own_ab": lab[..., 1:].transpose(2, 0, 1)[None]}
    return prepared


def check(mlmodel, reference_model, images) -> dict:
    import numpy as np
    import torch

    results = {}
    for name, entry in images.items():
        with torch.no_grad():
            reference = reference_model(torch.from_numpy(entry["image"])).numpy()
        got = mlmodel.predict({"image": entry["image"]})["ab"]
        delta = np.abs(got - reference)
        result = {
            "mean_abs_delta": float(delta.mean()),
            "p99_abs_delta": float(np.percentile(delta, 99)),
            "shape": list(got.shape),
            "finite": bool(np.isfinite(got).all()),
            "max_abs_ab": float(np.abs(got).max()),
            "mean_chroma": float(np.hypot(got[0, 0], got[0, 1]).mean()),
        }
        if name != "gradient":
            result["mean_abs_ab_vs_photo"] = float(np.abs(got - entry["own_ab"]).mean())
            result["photo_mean_chroma"] = float(
                np.hypot(entry["own_ab"][0, 0], entry["own_ab"][0, 1]).mean())
        results[name] = result
        print(f"check {name}: mean |Δab| {result['mean_abs_delta']:.4f}, p99 |Δab| "
              f"{result['p99_abs_delta']:.4f}, mean chroma {result['mean_chroma']:.2f}"
              + (f", mean |ab − photo ab| {result['mean_abs_ab_vs_photo']:.3f}"
                 if "mean_abs_ab_vs_photo" in result else ""), flush=True)
    return results


def passed(results: dict) -> bool:
    shape = [1, 2, SIDE, SIDE]
    return all(r["mean_abs_delta"] <= MEAN_BOUND and r["p99_abs_delta"] <= P99_BOUND
               and r["finite"] and r["shape"] == shape for r in results.values())


def quality(results: dict) -> dict:
    photos = [r for name, r in results.items() if name != "gradient"]
    return {"mean_abs_ab_vs_photos": statistics.mean(r["mean_abs_ab_vs_photo"] for r in photos),
            "mean_chroma": statistics.mean(r["mean_chroma"] for r in photos),
            "photos_mean_chroma": statistics.mean(r["photo_mean_chroma"] for r in photos)}


def timing(package: Path, images) -> dict:
    import coremltools as ct

    model = ct.models.MLModel(str(package), compute_units=ct.ComputeUnit.ALL)
    feed = {"image": images["sky.jpg"]["image"]}
    model.predict(feed)
    seconds = []
    for _ in range(TIMED_RUNS):
        start = time.perf_counter()
        model.predict(feed)
        seconds.append(time.perf_counter() - start)
    return {"compute_units": "ALL", "warm_up": 1, "runs": TIMED_RUNS, "side": SIDE,
            "median_seconds": statistics.median(seconds), "seconds": seconds}


def machine() -> dict:
    def sysctl(name):
        done = subprocess.run(["sysctl", "-n", name], capture_output=True, text=True, check=False)
        return done.stdout.strip()
    return {"hw.memsize": int(sysctl("hw.memsize") or 0),
            "chip": sysctl("machdep.cpu.brand_string"),
            "macos": platform.mac_ver()[0]}


def catalog_entry(variant: str, outputs: dict, precision: str) -> dict:
    tag = release_tag(variant)
    base = f"https://github.com/kpgalligan/doctored-models/releases/download/{tag}/"
    return {
        "id": "doctored/ddcolor",
        "name": f"{VARIANTS[variant]['label']} (Core ML)",
        "capability": "colorize",
        "version": tag,
        "files": [
            {"path": f"{PACKAGE}/{inner}", "url": base + outputs[name]["asset"],
             "sha256": outputs[name]["sha256"], "size": outputs[name]["size"]}
            for name, inner in [("Manifest.json", "Manifest.json"),
                                ("model.mlmodel", "Data/com.apple.CoreML/model.mlmodel"),
                                ("weight.bin", "Data/com.apple.CoreML/weights/weight.bin")]
        ],
        "runtime": "coreml",
        "minimum_macos": "15.0",
        "license": {"name": "Apache-2.0", "url": LICENSE_PAGE, "tier": "clean_data_encumbered"},
        "training_data": (
            "ImageNet (MODEL_ZOO.md: trained on ImageNet), whose terms are non-commercial "
            "research. Doctored converted the Hugging Face checkpoint "
            f"{VARIANTS[variant]['repo']} to Core ML ({precision} compute) with "
            "scripts/convert_ddcolor.py, retraining nothing."),
        "inference_bytes": INFERENCE_BYTES,
        "packages": [PACKAGE],
    }


def notes(variant: str, receipt: dict, command: str) -> str:
    entry = VARIANTS[variant]
    tag = release_tag(variant)
    q = receipt["quality"]
    lines = [
        f"# {entry['label']}, Core ML ({tag})",
        "",
        "Colorize's model. Written by `scripts/convert_ddcolor.py`; publishing the release "
        f"`{tag}` on kpgalligan/doctored-models is the owner's step.",
        "",
        f"- Source: {source_url(variant)}.",
        f"- Source SHA-256: {entry['sha256']} ({entry['size']} bytes).",
        f"- Network: `ddcolor/model.py` and `basicsr/archs/ddcolor_arch_utils/*` at "
        f"{REPOSITORY_COMMIT} (SHA-256s in receipt.json), DDColor({entry['encoder']}, "
        "MultiScaleColorDecoder, 100 queries, 3 scales, 9 layers) loaded with strict=True, "
        "spectral norms folded; the package is `ab = model(image)`.",
        f"- Code and weights: Apache-2.0, Xiaoyang Kang et al. ({REPOSITORY}, `LICENSE` at "
        f"{REPOSITORY_COMMIT}, copied here as `LICENSE`).",
        "- Training data: ImageNet (MODEL_ZOO.md).",
        f"- Compute precision: {receipt['conversion']['precision']}.",
        f"- Package: {receipt['package_bytes']} bytes; median prediction at {SIDE}²: "
        f"{receipt['timing']['median_seconds'] * 1000:.1f} ms (compute units ALL, "
        f"{receipt['machine']['chip']}).",
        f"- Quality: mean |Δab| against the photos' own ab {q['mean_abs_ab_vs_photos']:.3f}; "
        f"mean predicted chroma {q['mean_chroma']:.2f} (the photos' {q['photos_mean_chroma']:.2f}).",
        "",
        "| asset | bytes | sha256 |",
        "|---|---|---|",
    ]
    for output in receipt["outputs"].values():
        lines.append(f"| {output['asset']} | {output['size']} | {output['sha256']} |")
    lines += ["", f"Command: `{command}`", ""]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--variant", choices=sorted(VARIANTS), required=True)
    parser.add_argument("--output", type=Path, required=True,
                        help="the folder that receives <release tag>/")
    arguments = parser.parse_args()
    variant = arguments.variant
    tag = release_tag(variant)
    output = arguments.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="doctored-ddcolor-"))
    source = fetch_source(variant, scratch)
    license_text = fetch(LICENSE_URL)
    if sha256(license_text) != LICENSE_SHA256:
        raise SystemExit(f"{LICENSE_URL} is not the pinned file: sha256 {sha256(license_text)}")
    root = vendor(scratch)
    print(f"source {VARIANTS[variant]['sha256']}; scratch {scratch}", flush=True)
    reference = network(source, variant, root)
    model, spectral = folded(reference)
    print(f"folded {spectral} spectral norms", flush=True)
    images = fixtures()
    attempts = {}
    mlmodel = None
    precision = None
    for candidate in ["float16", "float32"]:
        print(f"converting at {candidate}", flush=True)
        converted = convert(model, candidate)
        results = check(converted, reference, images)
        attempts[candidate] = {"results": results, "passed": passed(results)}
        if attempts[candidate]["passed"]:
            mlmodel, precision = converted, candidate
            break

    versions = {name: metadata.version(name) for name in ["torch", "coremltools", "numpy", "pillow"]}
    versions["python"] = sys.version.split()[0]
    receipt = {
        "variant": variant,
        "source": {"url": source_url(variant), "sha256": VARIANTS[variant]["sha256"],
                   "size": VARIANTS[variant]["size"], "hugging_face_commit": VARIANTS[variant]["commit"],
                   "repository": REPOSITORY, "repository_commit": REPOSITORY_COMMIT,
                   "vendored_sha256": VENDORED, "license_url": LICENSE_URL,
                   "license_sha256": LICENSE_SHA256},
        "tools": versions,
        "check": {"mean_abs_delta_bound": MEAN_BOUND, "p99_abs_delta_bound": P99_BOUND,
                  "spectral_norms_folded": spectral, "attempts": attempts,
                  "passed": mlmodel is not None},
        "release_tag": tag,
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
    receipt["quality"] = quality(attempts[precision]["results"])
    print(f"package {receipt['package_bytes']} B; median prediction "
          f"{receipt['timing']['median_seconds'] * 1000:.1f} ms; mean |Δab| vs photos "
          f"{receipt['quality']['mean_abs_ab_vs_photos']:.3f}", flush=True)

    release = output / tag
    release.mkdir(parents=True, exist_ok=True)
    for name, path in files.items():
        shutil.copyfile(path, release / (ASSET_PREFIX + name))
    (release / "LICENSE").write_bytes(license_text)
    command = f"uv run scripts/convert_ddcolor.py --variant {variant} --output {arguments.output}"
    (release / "NOTES.md").write_text(notes(variant, receipt, command))
    (release / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    (release / "catalog-entry.json").write_text(
        json.dumps(catalog_entry(variant, outputs, precision), indent=2, ensure_ascii=False) + "\n")
    for entry in outputs.values():
        print(f"asset {tag}/{entry['asset']} {entry['size']} B sha256 {entry['sha256']}", flush=True)
    print(f"PASS: converted at {precision}; receipt {release / 'receipt.json'}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
