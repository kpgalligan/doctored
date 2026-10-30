# /// script
# requires-python = "==3.11.*"
# dependencies = [
#     "tensorflow==2.15.1",
#     "tf2onnx==1.16.1",
#     "onnx==1.16.2",
#     "onnx2torch==1.5.15",
#     "torch==2.5.1",
#     "coremltools==8.3.0",
#     "numpy==1.26.4",
#     "protobuf==3.20.3",
#     "pillow==11.0.0",
# ]
# ///
"""Convert Google's MediaPipe multiclass selfie segmenter to Core ML. NETWORK.

Select ▸ Person Parts… runs MediaPipe's `selfie_multiclass_256x256` (Apache-2.0; labels
0 background, 1 hair, 2 body-skin, 3 face-skin, 4 clothes, 5 others), which Google publishes
only as TFLite. This script is the reproducible conversion:

  1. downloads the TFLite file from Google's model bucket and refuses any other bytes
     (SOURCE_SHA256, SOURCE_SIZE);
  2. reads the label map and the input normalisation from the file's own metadata, and
     asserts the label map above;
  3. tf2onnx → onnx2torch → `torch.jit.trace` (wrapped to take and give NCHW) → coremltools
     mlprogram at FLOAT32 compute precision, with the normalisation folded into an `image`
     ImageType 256×256 and a
     softmax on the channel axis when the TFLite output is not already a distribution, so the
     output `classes` is 1×6×256×256 probabilities. FLOAT32, not FP16: the FP16 build missed
     the self-check's probability bound (max |Δp| 0.043 on the portrait and 0.078 on fur against
     0.02, measured 2026-10-14), while FLOAT32 agrees to 1e-4 at 16 MB of weights instead of 8;
  4. self-checks the package against the TFLite interpreter on the repo's portrait and fur
     fixtures and a synthetic gradient (the same 256×256 RGB to both): max |Δp| ≤ MAX_DELTA and
     argmax agreement ≥ MIN_AGREEMENT on each, else exits 1 and writes no release assets;
  5. writes `<output>/selfie_multiclass_256x256.mlpackage`, the flat release assets
     `<output>/<RELEASE_TAG>/selfie_multiclass_256x256-{Manifest.json,model.mlmodel,weight.bin}`
     (the files `ModelCatalog`'s entry downloads from
     https://github.com/kpgalligan/doctored-models/releases/download/<RELEASE_TAG>/), and
     `<output>/receipt.json` (source SHA-256, tool versions, output SHA-256s and sizes, check
     numbers).

Run with `uv run scripts/convert_mediapipe_multiclass.py --output <folder>`; uv reads the pins
above (Python 3.11). `scripts/fill_model_fixtures.py --release-dir <folder>` then serves the
assets to the fixture store. The Manifest's item identifiers are fresh UUIDs on every save, so
a second run writes different output hashes: the catalogue pins the run whose receipt it cites.
"""

import argparse
import hashlib
from importlib import metadata
import json
import shutil
import struct
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

SOURCE_URL = ("https://storage.googleapis.com/mediapipe-models/image_segmenter/"
              "selfie_multiclass_256x256/float32/1/selfie_multiclass_256x256.tflite")
SOURCE_SHA256 = "c6748b1253a99067ef71f7e26ca71096cd449baefa8f101900ea23016507e0e0"
SOURCE_SIZE = 16371837
LABELS = ["background", "hair", "body-skin", "face-skin", "clothes", "others"]
RELEASE_TAG = "mediapipe-selfie-multiclass-256x256-coreml-1"
PACKAGE = "selfie_multiclass_256x256.mlpackage"
ASSET_PREFIX = "selfie_multiclass_256x256-"
SIDE = 256
# The package against the float32 interpreter: a probability within 0.02 and the same winning
# class at 99 % of pixels is what the parts selection can tell apart at all.
MAX_DELTA = 0.02
MIN_AGREEMENT = 0.99

ROOT = Path(__file__).resolve().parent.parent
PHOTOS = Path.home() / "devel/doctored-fixtures/phase6/photos"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch_source(scratch: Path) -> Path:
    path = scratch / "selfie_multiclass_256x256.tflite"
    with urllib.request.urlopen(SOURCE_URL, timeout=120) as response:
        data = response.read()
    if len(data) != SOURCE_SIZE or sha256(data) != SOURCE_SHA256:
        raise SystemExit(f"the source is not the pinned file: {len(data)} bytes, sha256 {sha256(data)}")
    path.write_bytes(data)
    return path


# ---- TFLite metadata: a minimal flatbuffer reader for the two tables this needs ----------

def _table(buf: bytes, pos: int):
    """A flatbuffer table at `pos`: a field reader by slot index."""
    vtable = pos - struct.unpack_from("<i", buf, pos)[0]
    vsize = struct.unpack_from("<H", buf, vtable)[0]

    def field(slot: int):
        offset = 4 + 2 * slot
        if offset >= vsize:
            return None
        rel = struct.unpack_from("<H", buf, vtable + offset)[0]
        return pos + rel if rel else None
    return field


def _deref(buf: bytes, at: int) -> int:
    return at + struct.unpack_from("<I", buf, at)[0]


def _vector(buf: bytes, at: int):
    start = _deref(buf, at)
    return struct.unpack_from("<I", buf, start)[0], start + 4


def _string(buf: bytes, at):
    if at is None:
        return None
    count, start = _vector(buf, at)
    return buf[start:start + count].decode()


def metadata_buffer(model: bytes) -> bytes:
    """The bytes of the model's TFLITE_METADATA buffer (schema.fbs: Model.metadata = slot 6,
    Model.buffers = slot 4; Metadata {name, buffer}; Buffer {data})."""
    root = _table(model, _deref(model, 0))
    count, start = _vector(model, root(6))
    for index in range(count):
        entry = _table(model, _deref(model, start + 4 * index))
        if _string(model, entry(0)) == "TFLITE_METADATA":
            buffer_index = struct.unpack_from("<I", model, entry(1))[0]
            buffers, bstart = _vector(model, root(4))
            buffer = _table(model, _deref(model, bstart + 4 * buffer_index))
            size, data = _vector(model, buffer(0))
            return model[data:data + size]
    raise SystemExit("the TFLite file has no TFLITE_METADATA")


def normalisation(meta: bytes) -> tuple[list[float], list[float]]:
    """The input tensor's NormalizationOptions (metadata_schema.fbs: ModelMetadata.
    subgraph_metadata = slot 3; SubGraphMetadata.input_tensor_metadata = slot 2;
    TensorMetadata.process_units = slot 4; ProcessUnit {options_type (0), options (1)};
    NormalizationOptions = union type 1, {mean (0), std (1)})."""
    root = _table(meta, _deref(meta, 0))
    _, subgraphs = _vector(meta, root(3))
    subgraph = _table(meta, _deref(meta, subgraphs))
    _, inputs = _vector(meta, subgraph(2))
    tensor = _table(meta, _deref(meta, inputs))
    count, units = _vector(meta, tensor(4))
    for index in range(count):
        unit = _table(meta, _deref(meta, units + 4 * index))
        if meta[unit(0)] != 1:
            continue
        options = _table(meta, _deref(meta, unit(1)))

        def floats(slot):
            n, start = _vector(meta, options(slot))
            return list(struct.unpack_from(f"<{n}f", meta, start))
        return floats(0), floats(1)
    raise SystemExit("the TFLite input has no NormalizationOptions")


def labels(source: Path) -> list[str]:
    with zipfile.ZipFile(source) as archive:
        return archive.read("labels.txt").decode().split()


# ---- the conversion ----------------------------------------------------------------------

def convert(source: Path, scratch: Path, mean: list[float], std: list[float]):
    import coremltools as ct
    import numpy as np
    import onnx
    import tf2onnx
    import torch
    from onnx2torch import convert as onnx_to_torch

    model_proto, _ = tf2onnx.convert.from_tflite(
        str(source), opset=13, inputs_as_nchw=None, output_path=str(scratch / "model.onnx"))
    input_name = model_proto.graph.input[0].name
    output_name = model_proto.graph.output[0].name
    graph = onnx_to_torch(onnx.load(str(scratch / "model.onnx")))
    graph.eval()

    probe = np.random.default_rng(0).random((1, SIDE, SIDE, 3), dtype=np.float32)
    with torch.no_grad():
        raw = graph(torch.from_numpy(probe)).numpy()
    sums = raw.sum(axis=-1)
    already = bool(raw.min() >= 0 and np.abs(sums - 1).max() < 1e-3)

    class Wrapped(torch.nn.Module):
        """NCHW image in, NCHW class probabilities out."""

        def __init__(self, inner):
            super().__init__()
            self.inner = inner

        def forward(self, x):
            y = self.inner(x.permute(0, 2, 3, 1)).permute(0, 3, 1, 2)
            return y if already else torch.softmax(y, dim=1)

    wrapped = Wrapped(graph).eval()
    example = torch.rand(1, 3, SIDE, SIDE)
    traced = torch.jit.trace(wrapped, example)
    # input = (pixel − mean) / std, per channel; Core ML's ImageType applies scale · pixel + bias
    # with one scale, so the three stds must agree (MediaPipe's do).
    if max(std) - min(std) > 1e-6:
        raise SystemExit(f"per-channel stds differ: {std}")
    scale = 1.0 / std[0]
    bias = [-m / std[0] for m in (mean * 3 if len(mean) == 1 else mean)]
    mlmodel = ct.convert(
        traced,
        inputs=[ct.ImageType(name="image", shape=(1, 3, SIDE, SIDE), scale=scale, bias=bias,
                             color_layout=ct.colorlayout.RGB)],
        outputs=[ct.TensorType(name="classes")],
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT32,
        minimum_deployment_target=ct.target.macOS15,
    )
    mlmodel.short_description = (
        "MediaPipe multiclass selfie segmenter (Apache-2.0), converted to Core ML by "
        "scripts/convert_mediapipe_multiclass.py: classes 0 background, 1 hair, 2 body-skin, "
        "3 face-skin, 4 clothes, 5 others, softmax probabilities.")
    mlmodel.user_defined_metadata.pop("com.github.apple.coremltools.conversion_date", None)
    return mlmodel, {"input_name": input_name, "output_name": output_name,
                     "softmax_added": not already, "scale": scale, "bias": bias}


def check(mlmodel, source: Path, mean: list[float], std: list[float]) -> dict:
    import numpy as np
    import tensorflow as tf
    from PIL import Image

    interpreter = tf.lite.Interpreter(model_path=str(source))
    interpreter.allocate_tensors()
    tin = interpreter.get_input_details()[0]
    tout = interpreter.get_output_details()[0]
    gradient = np.zeros((SIDE, SIDE, 3), dtype=np.uint8)
    gradient[..., 0] = np.arange(SIDE)[None, :]
    gradient[..., 1] = np.arange(SIDE)[:, None]
    gradient[..., 2] = 128
    images = {"gradient": Image.fromarray(gradient)}
    for name in ["portrait.png", "fur.png"]:
        path = PHOTOS / name
        if not path.is_file():
            raise SystemExit(f"the self-check needs {path} (scripts/phase6_quality_fixtures.py)")
        images[name] = Image.open(path).convert("RGB").resize((SIDE, SIDE), Image.BILINEAR)
    results = {}
    for name, image in images.items():
        pixels = np.asarray(image, dtype=np.float32)
        normalised = (pixels - np.array(mean, dtype=np.float32)) / np.array(std, dtype=np.float32)
        interpreter.set_tensor(tin["index"], normalised[None])
        interpreter.invoke()
        reference = interpreter.get_tensor(tout["index"])[0]
        if reference.min() < 0 or np.abs(reference.sum(-1) - 1).max() > 1e-3:
            reference = tf.nn.softmax(reference, axis=-1).numpy()
        got = mlmodel.predict({"image": image})["classes"][0].transpose(1, 2, 0)
        delta = float(np.abs(got - reference).max())
        agreement = float((got.argmax(-1) == reference.argmax(-1)).mean())
        results[name] = {"max_abs_delta": delta, "argmax_agreement": agreement}
        print(f"check {name}: max |Δp| {delta:.4f}, argmax agreement {agreement:.4%}", flush=True)
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True,
                        help="the folder that receives the package, receipt.json and <tag>/ assets")
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="doctored-mediapipe-"))
    source = fetch_source(scratch)
    found = labels(source)
    if found != LABELS:
        raise SystemExit(f"the label map is not the expected one: {found}")
    mean, std = normalisation(metadata_buffer(source.read_bytes()))
    print(f"source {SOURCE_SHA256}; labels {found}; normalisation mean {mean} std {std}", flush=True)
    mlmodel, conversion = convert(source, scratch, mean, std)
    results = check(mlmodel, source, mean, std)
    failed = [name for name, r in results.items()
              if r["max_abs_delta"] > MAX_DELTA or r["argmax_agreement"] < MIN_AGREEMENT]

    package = output / PACKAGE
    if package.exists():
        shutil.rmtree(package)
    mlmodel.save(str(package))
    files = {
        "Manifest.json": package / "Manifest.json",
        "model.mlmodel": package / "Data/com.apple.CoreML/model.mlmodel",
        "weight.bin": package / "Data/com.apple.CoreML/weights/weight.bin",
    }
    outputs = {}
    for name, path in files.items():
        data = path.read_bytes()
        outputs[name] = {"package_path": str(path.relative_to(output)), "asset": ASSET_PREFIX + name,
                         "sha256": sha256(data), "size": len(data)}
    versions = {name: metadata.version(name) for name in
                ["tensorflow", "tf2onnx", "onnx", "onnx2torch", "torch", "coremltools", "numpy", "protobuf"]}
    versions["python"] = sys.version.split()[0]
    receipt = {
        "source": {"url": SOURCE_URL, "sha256": SOURCE_SHA256, "size": SOURCE_SIZE},
        "labels": found, "normalisation": {"mean": mean, "std": std},
        "conversion": conversion, "tools": versions,
        "check": {"max_abs_delta_bound": MAX_DELTA, "argmax_agreement_bound": MIN_AGREEMENT,
                  "results": results, "passed": not failed},
        "release_tag": RELEASE_TAG, "outputs": outputs,
    }
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    if failed:
        print(f"FAIL: the self-check missed its bounds on {failed}; no release assets written", flush=True)
        return 1
    release = output / RELEASE_TAG
    release.mkdir(parents=True, exist_ok=True)
    for name, path in files.items():
        shutil.copyfile(path, release / (ASSET_PREFIX + name))
    for name, entry in outputs.items():
        print(f"asset {RELEASE_TAG}/{entry['asset']} {entry['size']} B sha256 {entry['sha256']}", flush=True)
    print(f"PASS: converted; receipt {output / 'receipt.json'}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
