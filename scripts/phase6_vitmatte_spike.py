#!/usr/bin/env python3
"""Bounded, offline ViTMatte HF-conversion qualification, outside the app.

Requires a separate pinned Python environment and independently downloaded model
files. Does not install packages, download content, read credentials or load
pickle. Inference sees only the source/trimap/prior export manifest; the separate
Rust scorer owns all reference alpha/foreground data and quality thresholds.
"""
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import time

sys.dont_write_bytecode = True
MODEL_ID = "hustvl/vitmatte-small-composition-1k"
REVISION = "6a58ad7646403c1df626fbd746900aec7361ea1d"
FILES = {
    "model.safetensors": (103294572, "bda9289db1bb6762d978b42d1c62ae3f34daf7497171a347a1d09657efd788cb"),
    "config.json": (837, "ae1006f5a83227048b563b2e60709d4203e432b2276949ebef41a8cfeeeaf45f"),
    "preprocessor_config.json": (284, "0db558038b96a3f5c97e46d4ec8966fcc479e9aa58a391bca60b5094a5f7fee0"),
    "README.md": (1716, "5545af4bf41cc017cd1a75ff096d27ff4e6e6ea0bb41c9a272dd4072a0fefaaf"),
}
VERSIONS = {"torch": "2.8.0", "transformers": "4.57.6", "numpy": "2.3.2", "Pillow": "11.3.0", "safetensors": "0.6.2"}
NORMALIZATIONS = {
    "packaged": ([0.5] * 3, [0.5] * 3),
    "author": ([123.675 / 255, 116.280 / 255, 103.530 / 255],
               [58.395 / 255, 57.120 / 255, 57.375 / 255]),
}
RSS_LIMIT = 2 * 1024**3
CASE_SECONDS = 120
POLICY = {
    "candidate": "ViTMatte-S Composition-1k Hugging Face conversion; no original-author implementation parity claim",
    "device": "cpu", "dtype": "float32", "cpu_threads": 4, "batch": 1,
    "processor": "pinned checkpoint processor: RGB mean/std0.5; RGB and trimap rescale1/255",
    "geometry": "native resolution; bottom/right zero-pad to32 and crop that padding; never resize",
    "output": "floor(clamp(raw_sigmoid,0,1)*255+0.5), then exact supplied trimap0/255 constraints; unknown128 unchanged",
    "reference_input": False, "prior_input": "hash/provenance only; model receives source RGB and trimap",
    "color_estimation": "none in this script; unchanged Rust scorer estimates color independently",
    "preflight": "1GiB runtime allowance +256bytes/padded-pixel +6 float32 global-attention matrices (6heads,patch16)",
    "rss_limit_bytes": RSS_LIMIT, "case_timeout_seconds": CASE_SECONDS,
}


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


def write_json(path, value):
    temporary = Path(str(path) + ".part")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temporary.replace(path)


def model_check(root):
    for name, (size, sha) in FILES.items():
        path = root / name
        if not path.is_file() or path.stat().st_size != size or digest(path) != sha:
            raise ValueError("Pinned model file checksum/length mismatch: " + name)
    for package, expected in VERSIONS.items():
        actual = importlib.metadata.version(package)
        if actual != expected:
            raise ValueError(f"Expected {package}=={expected}, found {actual}")


def working_bytes(width, height):
    padded_w = (width + 31) // 32 * 32
    padded_h = (height + 31) // 32 * 32
    tokens = padded_w // 16 * (padded_h // 16)
    attention = 6 * tokens * tokens * 4
    return 1024**3 + 256 * padded_w * padded_h + 6 * attention


def worker(model_root, input_root, output_root, normalization="packaged"):
    from PIL import Image
    # Refuse geometry before decoding large pixel arrays or loading the model.
    with Image.open(input_root / "source.png") as image:
        width, height = image.size
        if image.mode != "RGB":
            raise ValueError("Input source must be native RGB PNG")
    if width < 1 or height < 1 or working_bytes(width, height) >= RSS_LIMIT:
        raise ValueError("Native input exceeds the frozen2GiB inference estimate; no resizing is permitted")
    import numpy as np
    import torch
    from transformers import VitMatteForImageMatting, VitMatteImageProcessor

    torch.set_num_threads(4)
    torch.set_num_interop_threads(1)
    processor = VitMatteImageProcessor.from_pretrained(model_root, local_files_only=True)
    model = VitMatteForImageMatting.from_pretrained(model_root, local_files_only=True,
                                                  use_safetensors=True, trust_remote_code=False)
    model = model.to(device="cpu", dtype=torch.float32).eval()
    source = Image.open(input_root / "source.png")
    trimap = Image.open(input_root / "trimap.png")
    prior = Image.open(input_root / "prior.png")
    if source.size != trimap.size or source.size != prior.size or trimap.mode != "L" or prior.mode != "L":
        raise ValueError("Input dimensions or categorical plane modes are invalid")
    labels = np.asarray(trimap)
    if not np.isin(labels, [0, 128, 255]).all():
        raise ValueError("Trimap must be categorical0/128/255")
    mean, std = NORMALIZATIONS[normalization]
    inputs = processor(images=source, trimaps=trimap, return_tensors="pt", do_pad=True, size_divisor=32,
                       image_mean=mean, image_std=std)
    padded = ((height + 31) // 32 * 32, (width + 31) // 32 * 32)
    if tuple(inputs.pixel_values.shape) != (1, 4, *padded):
        raise ValueError("Processor changed native geometry unexpectedly")
    if processor.image_mean != [0.5] * 3 or processor.image_std != [0.5] * 3:
        raise ValueError("Processor normalization differs from frozen HF conversion")
    started = time.monotonic()
    with torch.inference_mode():
        predicted = model(**inputs).alphas
    elapsed = time.monotonic() - started
    if tuple(predicted.shape) != (1, 1, *padded):
        raise ValueError("Model output dimensions differ from padded native input")
    raw = predicted[0, 0, :height, :width].cpu().numpy()
    if not np.isfinite(raw).all():
        raise ValueError("Model produced nonfinite alpha")
    raw_bytes = np.floor(np.clip(raw, 0, 1) * 255 + 0.5).astype(np.uint8)
    alpha = np.where(labels == 0, 0, np.where(labels == 255, 255, raw_bytes)).astype(np.uint8)
    # Parent removes every output if the process later breaches a guard.
    Image.fromarray(raw_bytes).save(output_root / "raw-alpha.png")
    Image.fromarray(alpha).save(output_root / "alpha.png")
    write_json(output_root / "case.json", {
        "width": width, "height": height, "padded_width": padded[1], "padded_height": padded[0],
        "native_geometry_preserved": True, "inference_seconds": elapsed,
        "normalization": normalization, "image_mean": mean, "image_std": std,
        "preflight_bytes": working_bytes(width, height),
        "worker_peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "model_parameters": sum(p.numel() for p in model.parameters()),
        "known_constraint_bytes_replaced": int(np.count_nonzero(alpha != raw_bytes)),
        "mps_available_but_unused": torch.backends.mps.is_available(),
    })


def guarded_case(model_root, input_root, output_root, dimensions, normalization="packaged"):
    output_root.mkdir()
    environment = dict(os.environ, HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1",
                       HF_HUB_DISABLE_TELEMETRY="1", TOKENIZERS_PARALLELISM="false",
                       PYTHONDONTWRITEBYTECODE="1", OMP_NUM_THREADS="4")
    started = time.monotonic()
    peak = 0
    refusal = None
    with (output_root / "worker.log").open("w") as log:
        process = subprocess.Popen([sys.executable, "-B", str(Path(__file__).resolve()),
                                    "--worker", "--model", str(model_root),
                                    "--inputs", str(input_root), "--outputs", str(output_root),
                                    "--normalization", normalization],
                                   env=environment, stdout=log, stderr=subprocess.STDOUT)
        try:
            while process.poll() is None:
                if time.monotonic() - started > CASE_SECONDS:
                    refusal = "case exceeded120s including load/preparation"
                    break
                measured = subprocess.run(["ps", "-o", "rss=", "-p", str(process.pid)],
                                          capture_output=True, text=True, timeout=2)
                if measured.returncode not in (0, 1) or (measured.returncode == 1 and measured.stderr.strip() and process.poll() is None):
                    refusal = "could not monitor owned inference process RSS"
                    break
                if measured.stdout.strip():
                    peak = max(peak, int(measured.stdout.strip()) * 1024)
                    if peak > RSS_LIMIT:
                        refusal = "case exceeded2GiB measured RSS"
                        break
                time.sleep(0.05)
            if refusal is not None:
                process.kill()
            process.wait(timeout=10)
        finally:
            if process.poll() is None:
                process.kill(); process.wait(timeout=10)
        if process.returncode != 0 and refusal is None:
            reason = output_root / "refusal.json"
            refusal = (json.loads(reason.read_text())["refusal"] if reason.is_file()
                       else "inference process failed; see worker.log")
    elapsed = time.monotonic() - started
    metadata = None
    if refusal is None:
        metadata = json.loads((output_root / "case.json").read_text())
        if elapsed > CASE_SECONDS:
            refusal = "case exceeded120s including load/preparation"
        elif metadata["worker_peak_rss_bytes"] > RSS_LIMIT or peak > RSS_LIMIT:
            refusal = "case exceeded2GiB measured RSS"
        elif (metadata["width"], metadata["height"]) != dimensions:
            refusal = "worker dimensions differ from the captured manifest"
    report = {"completed": refusal is None, "wall_seconds": elapsed,
              "observed_peak_rss_bytes": peak}
    if metadata is not None:
        report["worker_peak_rss_bytes"] = metadata["worker_peak_rss_bytes"]
    if refusal:
        report["refusal"] = refusal
        for name in ["alpha.png", "raw-alpha.png", "case.json", "case.json.part"]:
            (output_root / name).unlink(missing_ok=True)
    else:
        report.update(metadata)
        report["outputs"] = {name: digest(output_root / name) for name in ["alpha.png", "raw-alpha.png"]}
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--outputs", type=Path, required=True)
    parser.add_argument("--normalization", choices=list(NORMALIZATIONS), default="packaged",
                        help="Frozen packaged processor or author-config ImageNet normalization variant")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    arguments.model = arguments.model.resolve()
    arguments.inputs = arguments.inputs.resolve()
    arguments.outputs = arguments.outputs.resolve()
    repository = Path(__file__).resolve().parents[1]
    if arguments.outputs == repository or repository in arguments.outputs.parents:
        raise ValueError("Inference artifacts must be outside the repository")
    if arguments.worker:
        try:
            worker(arguments.model, arguments.inputs, arguments.outputs, arguments.normalization)
        except Exception as error:
            write_json(arguments.outputs / "refusal.json", {"refusal": str(error),
                "error_type": type(error).__name__,
                "worker_peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss})
            raise
        return
    if sys.platform != "darwin":
        raise ValueError("This bounded CPU/RSS spike is qualified only on macOS")
    model_check(arguments.model)
    manifest_path = arguments.inputs / "inputs.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema_version") != 1 or manifest.get("reference_alpha_exported") is not False:
        raise ValueError("Inference requires the separate source-only export manifest")
    arguments.outputs.mkdir()
    policy = dict(POLICY)
    policy["normalization"] = arguments.normalization
    policy["image_mean"], policy["image_std"] = NORMALIZATIONS[arguments.normalization]
    if arguments.normalization == "author":
        policy["processor"] = "author-config ImageNet RGB mean/std; otherwise pinned HF processor and1/255 rescaling"
    report = {"policy": policy, "model": MODEL_ID, "revision": REVISION,
              "model_files": {name: {"bytes": size, "sha256": sha} for name, (size, sha) in FILES.items()},
              "model_license_metadata": "Apache-2.0 (HF model card; original author repo MIT)",
              "input_manifest_sha256": digest(manifest_path), "python": sys.version,
              "environment": {dist.metadata["Name"]: dist.version for dist in importlib.metadata.distributions()},
              "cases": []}
    write_json(arguments.outputs / "model-report.json", report)
    for case in manifest["cases"]:
        name = case["case"]
        if Path(name).name != name or name in (".", "..") or case.get("domain") != "encoded RGB":
            raise ValueError("Invalid source-only case manifest")
        input_root = arguments.inputs / name
        inputs = {kind: digest(input_root / (kind + ".png")) for kind in ["source", "trimap", "prior"]}
        result = guarded_case(arguments.model, input_root, arguments.outputs / name,
                              (case["width"], case["height"]), arguments.normalization)
        for kind, sha in inputs.items():
            if digest(input_root / (kind + ".png")) != sha:
                raise ValueError("Inference altered its captured input")
        result.update(case=name, inputs=inputs)
        report["cases"].append(result)
        write_json(arguments.outputs / "model-report.json", report)
        print(json.dumps(result), flush=True)
    print(json.dumps({"completed": sum(case["completed"] for case in report["cases"]),
                      "refused": sum(not case["completed"] for case in report["cases"])}), flush=True)


if __name__ == "__main__":
    main()
