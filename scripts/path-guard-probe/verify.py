#!/usr/bin/env python3
"""Compile and run the path-guard unit probe against current app sources.

Run `make rust project` first. The linked probe is built by `scripts/probe_build.py`
and cached under `build/probe-cache`; the copy that runs and its logs live in a
temporary directory, retained on failure or with --keep.

Besides the probe's own PASS/FAIL cases, this runner checks the guard's list of
folder variables for drift: the probe's `ENV_FOLDERS` line must equal
{DR_ACTIONS_DIR, DR_RECOVERY_DIR} plus `verification_isolation.PRESET_DIRECTORIES`,
and must equal every `DR_*_DIR` name the app sources mention.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import platform
import plistlib
import re
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
PRESET_DIRECTORIES = _isolation.PRESET_DIRECTORIES
# The shared cached probe build, loaded by path for the same reason.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)


def source_folder_names():
    names = set()
    for source in (ROOT / "app/Sources").glob("*.swift"):
        names.update(re.findall(r"DR_[A-Z_]*_DIR\b", source.read_text(errors="replace")))
    return names


def verify(scratch):
    library = ROOT / "core/target/release"
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
        **preset_environment(scratch),
    )
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.path-guard-probe"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    build_log = scratch / "build.log"
    binary = scratch / "probe"
    probe_build.build(
        sources=[*probe_build.app_sources(), *probe_build.probe_sources("path-guard-probe")],
        bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
        library=library / "libdoctored_core.a", info_plist=info_copy,
        output=binary, log=build_log, env=environment, timeout=600)

    run_log = scratch / "run.log"
    command = [str(binary)]
    with run_log.open("w") as log:
        print("$ " + shlex.join(command), file=log, flush=True)
        completed = subprocess.run(command, cwd=ROOT, env=environment, stdout=log,
                                   stderr=subprocess.STDOUT, timeout=120)
    output = run_log.read_text(errors="replace")
    print(output, end="")
    if completed.returncode != 0 or "\nFAIL " in "\n" + output:
        raise RuntimeError(f"the probe failed (exit {completed.returncode})")

    line = next((l for l in output.splitlines() if l.startswith("ENV_FOLDERS: ")), None)
    if line is None:
        raise RuntimeError("the probe printed no ENV_FOLDERS line")
    guarded = set(line[len("ENV_FOLDERS: "):].split())
    expected = {"DR_ACTIONS_DIR", "DR_RECOVERY_DIR"} | set(PRESET_DIRECTORIES)
    sources = source_folder_names()
    if guarded != expected:
        raise RuntimeError(f"AgentPathGuard.environmentFolders {sorted(guarded)} differs from "
                           f"Actions + recovery + PRESET_DIRECTORIES {sorted(expected)}")
    if guarded != sources:
        raise RuntimeError(f"AgentPathGuard.environmentFolders {sorted(guarded)} differs from "
                           f"the DR_*_DIR names in app/Sources {sorted(sources)}")
    print(f"PASS env folders match ({len(guarded)} names)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep scratch outputs after success too.")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS, AppKit and the Apple Silicon build toolchain.")

    scratch = Path(tempfile.mkdtemp(prefix="rasterize-path-guard-probe-"))
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
