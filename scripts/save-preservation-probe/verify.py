#!/usr/bin/env python3
"""Compile and run the AppKit save regression probe against current app sources.

Run `make rust project` first. The linked probe is built by `scripts/probe_build.py`
and cached under `build/probe-cache`; the copy that runs, fixtures and logs
live in a temporary directory, retained on failure or with --keep.
With `DR_PROBE_BUILD_ONLY=1` the runner stops after the build, launching
nothing, and removes its scratch directory unless --keep. `make gate`'s
`save-preservation-probe` step runs it this way and then again in full.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import platform
import plistlib
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

# The one list of private preference directories, loaded by path so
# `scripts/` never shadows this probe's own modules on sys.path.
_isolation_spec = importlib.util.spec_from_file_location(
    "verification_isolation", ROOT / "scripts/verification_isolation.py")
_isolation = importlib.util.module_from_spec(_isolation_spec)
_isolation_spec.loader.exec_module(_isolation)
preset_environment = _isolation.preset_environment
# The shared cached probe build, loaded by path for the same reason.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)
# The hand-built layered TIFF the unread-layer-block case opens, loaded the same way.
_tiff_spec = importlib.util.spec_from_file_location("layer_block_tiff", ROOT / "scripts/layer_block_tiff.py")
layer_block_tiff = importlib.util.module_from_spec(_tiff_spec)
_tiff_spec.loader.exec_module(layer_block_tiff)


def run_logged(command, log_path, environment, timeout):
    with log_path.open("a") as log:
        print("$ " + shlex.join(map(str, command)), file=log, flush=True)
        subprocess.run(
            list(map(str, command)), cwd=ROOT, env=environment,
            stdout=log, stderr=subprocess.STDOUT, timeout=timeout, check=True,
        )


def verify(scratch):
    library = ROOT / "core/target/release"
    info = ROOT / "app/Info.plist"
    for required in [library / "libdoctored_core.a", library / "libdoctored_core.rlib", info]:
        if not required.is_file():
            raise RuntimeError(f"Missing {required}; run `make rust project` first.")

    # A little-endian 8-bit TIFF whose layer block holds one record the core does not read.
    layer_tiff = scratch / "unread-layers-fixture.tif"
    layer_tiff.write_bytes(layer_block_tiff.layered_tiff("<"))
    environment = dict(
        os.environ,
        DR_SAVE_PROBE_LAYER_TIFF=str(layer_tiff),
        CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
        ANTHROPIC_API_KEY="dummy",
        DR_AUTOMATION_ALERTS="record",
        DR_WORKING_LIMIT_BYTES=os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"),
        DR_ACTIONS_DIR=str(scratch / "actions"),
        DR_RECOVERY_DIR=str(scratch / "recovery"),
        DR_SAVE_PROBE_DIR=str(scratch / "files"),
        DR_SAVE_PROBE_FLOAT_FIXTURE=str(ROOT / "core/tests/fixtures/float_11f/scene.tiff"),
        **preset_environment(scratch),
    )
    # Keep the real document type registrations, with a private preferences
    # domain. The copy is embedded in the probe, never written into the app.
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.save-preservation-probe"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    build_log = scratch / "build.log"
    fixture_object = scratch / "oversized.o"
    run_logged([
        "rustc", HERE / "oversized.rs", "--edition", "2021", "--crate-type", "lib",
        "--emit=obj", "-O", "-L", f"dependency={library / 'deps'}",
        "--extern", f"doctored_core={library / 'libdoctored_core.rlib'}",
        "-o", fixture_object,
    ], build_log, environment, 120)

    binary = scratch / "probe"
    build_only = probe_build.take_build_only(environment)
    probe_build.build(
        app_sources=probe_build.app_sources(), sources=probe_build.probe_sources("save-preservation-probe"),
        bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
        library=library / "libdoctored_core.a", objects=[fixture_object], info_plist=info_copy,
        output=binary, log=build_log, env=environment, timeout=600)
    if build_only:
        print(f"build only: {binary}", flush=True)
        return

    run_log = scratch / "run.log"
    run_logged([
        binary, "-ApplePersistenceIgnoreState", "YES",
        "-ColorWorkingSpace", "srgb", "-RenderGroupPixelBudget", "1",
    ], run_log, environment, 60)
    print(run_log.read_text(), end="")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep scratch outputs after success too.")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS, AppKit and the Apple Silicon build toolchain.")

    scratch = Path(tempfile.mkdtemp(prefix="rasterize-save-probe-"))
    print(f"Probe scratch directory: {scratch}", flush=True)
    try:
        verify(scratch)
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Probe failed: {error}\nArtifacts retained at {scratch}", file=sys.stderr)
        for name in ["build.log", "run.log"]:
            log = scratch / name
            if log.is_file():
                print(f"\n{name} (last 40 lines):", file=sys.stderr)
                print("\n".join(log.read_text(errors="replace").splitlines()[-40:]), file=sys.stderr)
        return 1
    if arguments.keep:
        print(f"Artifacts retained at {scratch}")
    else:
        shutil.rmtree(scratch)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
