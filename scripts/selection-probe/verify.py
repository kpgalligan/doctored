#!/usr/bin/env python3
"""Compile and run the AppKit selection tools and captured-input regression probe against current app sources.

Run `make rust project` first. The linked probe is built by `scripts/probe_build.py`
and cached under `build/probe-cache`; the copy that runs, fixtures and logs
live in a temporary directory, retained on failure or with --keep.
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

# Imported fixture helpers must not leave generated files in the workspace.
sys.dont_write_bytecode = True

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


def run_logged(command, log_path, environment, timeout):
    with log_path.open("a") as log:
        print("$ " + shlex.join(map(str, command)), file=log, flush=True)
        subprocess.run(
            list(map(str, command)), cwd=ROOT, env=environment,
            stdout=log, stderr=subprocess.STDOUT, timeout=timeout, check=True,
        )


def verify(scratch, photos_dir=None, checks=None, people_dir=None):
    library = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "core/target")) / "release"
    info = ROOT / "app/Info.plist"
    for required in [library / "libdoctored_core.a", info]:
        if not required.is_file():
            raise RuntimeError(f"Missing {required}; run `make rust project` first.")

    environment = dict(
        os.environ,
        CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
        ANTHROPIC_API_KEY="dummy",
        DR_AUTOMATION_ALERTS="record",
        DR_WORKING_LIMIT_BYTES=os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"),
        DR_ACTIONS_DIR=str(scratch / "actions"),
        DR_RECOVERY_DIR=str(scratch / "recovery"),
        DR_SELECTION_PROBE_DIR=str(scratch / "files"),
        **preset_environment(scratch),
    )
    if checks:
        environment["DR_SELECTION_PROBE_CHECKS"] = checks
    if photos_dir:
        environment["DR_SELECTION_PHOTO_DIR"] = str(photos_dir.resolve())
    if people_dir:
        # Private photos, read in place; nothing derived from them is written.
        environment["DR_PEOPLE_PHOTO_DIR"] = str(people_dir.expanduser().resolve())

    # Keep the real document type registrations, with a private preferences
    # domain. The copy is embedded in the probe, never written into the app.
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.selection-probe"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    (scratch / "files").mkdir()
    spec = importlib.util.spec_from_file_location("native_cutout_fixtures", ROOT / "scripts/retouch-probe/fixtures.py")
    native = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(native)
    original = [(1003, 2005, 3007, 1)] * 6
    foreground = [(101,202,303,1),(1101,2202,3303,7),(11001,22002,33003,32769),
                  (4001,5003,6005,65535),(65001,55003,45005,0),(101,65534,513,65535)]
    for name, pixels in [("native-cutout-original", original), ("native-cutout-foreground", foreground)]:
        (scratch / "files" / f"{name}.png").write_bytes(native.png(3, 2, 16, pixels, native.srgb_profile()))

    if (not checks or any(check.strip() in ("mask outside coverage", "native layer blending") for check in checks.split(","))) and not environment.get("DR_MASK_FOUNDATION_FIXTURES"):
        helper = ROOT / "scripts/mask-probe/fixtures.py"
        spec = importlib.util.spec_from_file_location("mask_outside_fixtures", helper)
        outside = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(outside)
        inputs = scratch / "mask-fixtures"
        outside.write(inputs)
        environment["DR_MASK_FOUNDATION_FIXTURES"] = str(inputs)

    build_log = scratch / "build.log"
    binary = scratch / "probe"
    probe_build.build(
        sources=[*probe_build.app_sources(), *probe_build.probe_sources("selection-probe")],
        bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
        library=library / "libdoctored_core.a", info_plist=info_copy,
        output=binary, log=build_log, env=environment, timeout=600)

    run_log = scratch / "run.log"
    run_logged([
        binary, "-ApplePersistenceIgnoreState", "YES",
        "-ColorWorkingSpace", "srgb",
    ], run_log, environment, 300)
    print(run_log.read_text(), end="")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep scratch outputs after success too.")
    parser.add_argument("--photos-dir", type=Path, help="Pinned photos from scripts/phase6_quality_fixtures.py for Vision qualification.")
    parser.add_argument("--people-dir", type=Path, help="The group photos for the people instances check, read in place.")
    parser.add_argument("--checks", help="Comma-separated check names for diagnosis; omit for the complete suite.")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS, AppKit and the Apple Silicon build toolchain.")

    scratch = Path(tempfile.mkdtemp(prefix="doctored-selection-probe-"))
    print(f"Probe scratch directory: {scratch}", flush=True)
    try:
        verify(scratch, arguments.photos_dir, arguments.checks, arguments.people_dir)
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        reason = str(error)
        if isinstance(error, subprocess.CalledProcessError):
            # The probe build's command is `xcrun swiftc …`; name the tool it ran.
            tool = next((part for part in error.cmd if Path(part).name != "xcrun"), error.cmd[0])
            reason = f"{Path(tool).name} exited with status {error.returncode}"
        print(f"Probe failed: {reason}\nArtifacts retained at {scratch}", file=sys.stderr)
        names = ["run.log"] if (scratch / "run.log").is_file() else ["build.log"]
        for name in names:
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
