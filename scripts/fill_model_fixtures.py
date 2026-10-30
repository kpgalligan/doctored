#!/usr/bin/env python3
"""Fill the shared fixture model store through the app's own downloader. NETWORK.

Builds `scripts/model-fixtures` (the app sources plus its `main.swift`) with
`scripts/probe_build.py`, prints the catalogue's licences, and runs it with
DR_MODELS_DIR=~/devel/doctored-fixtures/models (or --store): for EVERY catalogue entry it
accepts the licence for its pinned version, downloads every file with resume and SHA-256
verification, compiles the Core ML packages and runs one prediction; the CC0 sky library (an
`assets` entry) is decoded and one sky opened instead.

--release-dir serves a folder holding `<release tag>/<asset>` (the `--output` of
`scripts/convert_mediapipe_multiclass.py`, `scripts/convert_migan.py`,
`scripts/convert_realesrgan.py`, `scripts/convert_fbcnn.py` and `scripts/convert_ddcolor.py`,
and `scripts/prepare_sky_library.py`'s `sky-library-1/`) over `scripts/range_file_server.py` on loopback and
sets DR_MODEL_FIXTURE_RELEASE_BASE, so the entries hosted on the doctored-models GitHub
release are filled from our own conversion before that release is published.

--only <id> sets DR_MODEL_FIXTURE_ONLY, so the run fills that one catalogue entry (e.g.
`--only doctored/sky-library` to check the published sky library downloads end to end).

`scripts/verify_depth_blur.py`, `scripts/verify_model_selection.py`,
`scripts/verify_neural_filters.py`, `scripts/verify_neural_colorize.py`, the jobs-probe's model
cases and the admission measures read
that store and never write to it. Run
`make rust project` first.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import platform
import plistlib
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
DEFAULT_STORE = Path.home() / "devel/doctored-fixtures/models"

_build_spec = importlib.util.spec_from_file_location("probe_build", HERE / "probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)
_isolation_spec = importlib.util.spec_from_file_location(
    "verification_isolation", HERE / "verification_isolation.py")
_isolation = importlib.util.module_from_spec(_isolation_spec)
_isolation_spec.loader.exec_module(_isolation)
_server_spec = importlib.util.spec_from_file_location("range_file_server", HERE / "range_file_server.py")
range_file_server = importlib.util.module_from_spec(_server_spec)
_server_spec.loader.exec_module(range_file_server)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--store", type=Path, default=DEFAULT_STORE, help="the store to fill")
    parser.add_argument("--release-dir", type=Path,
                        help="serve <dir>/<tag>/<asset> in place of the doctored-models release")
    parser.add_argument("--only", help="fill only this catalogue entry id")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This script requires macOS and the Apple Silicon build toolchain.")
    library = ROOT / "core/target/release/libdoctored_core.a"
    info = ROOT / "app/Info.plist"
    for required in [library, info]:
        if not required.is_file():
            print(f"Missing {required}; run `make rust project` first.", file=sys.stderr)
            return 1
    scratch = Path(tempfile.mkdtemp(prefix="doctored-model-fixtures-"))
    store = arguments.store.expanduser().resolve()
    store.mkdir(parents=True, exist_ok=True)
    environment = dict(
        os.environ,
        CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
        ANTHROPIC_API_KEY="dummy",
        DR_AUTOMATION_ALERTS="record",
        **_isolation.preset_environment(scratch),
    )
    # The store is the one isolated folder this run points somewhere real.
    environment["DR_MODELS_DIR"] = str(store)
    if arguments.only:
        environment["DR_MODEL_FIXTURE_ONLY"] = arguments.only
    if arguments.release_dir is not None:
        release = arguments.release_dir.expanduser().resolve()
        if not release.is_dir():
            parser.error(f"--release-dir {release} is not a folder")
        _server, base = range_file_server.start(release, scratch / "release-requests.jsonl")
        environment["DR_MODEL_FIXTURE_RELEASE_BASE"] = base
        print(f"serving {release} at {base}", flush=True)
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.model-fixtures"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)
    binary = scratch / "probe"
    build_log = scratch / "build.log"
    print(f"building scripts/model-fixtures (log {build_log})", flush=True)
    try:
        probe_build.build(
            sources=[*probe_build.app_sources(), *probe_build.probe_sources("model-fixtures")],
            bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
            library=library, info_plist=info_copy, output=binary, log=build_log,
            env=environment, timeout=900)
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"build failed: {error}", file=sys.stderr)
        if build_log.is_file():
            print("\n".join(build_log.read_text(errors="replace").splitlines()[-40:]), file=sys.stderr)
        return 1
    print("Running this script accepts, for the fixture store at " + str(store) + ", the licence of "
          "every catalogue model at its pinned version (each printed below with its tier and "
          "training data): Apple's Core ML Depth Anything V2 Small and SAM 2.1 Large and Tiny "
          "(Apache-2.0), twistlabs' sky U2-Net small (MIT), our conversion of MediaPipe's "
          "multiclass selfie segmenter (Apache-2.0), our conversion of Picsart AI Research's "
          "MI-GAN 512 Places2 (MIT; trained on Places2, whose own terms are non-commercial "
          "research) and the sky library of Poly Haven sky crops (CC0-1.0).", flush=True)
    done = subprocess.run([str(binary), "-ApplePersistenceIgnoreState", "YES"],
                          cwd=ROOT, env=environment, timeout=2400, check=False)
    return done.returncode


if __name__ == "__main__":
    raise SystemExit(main())
