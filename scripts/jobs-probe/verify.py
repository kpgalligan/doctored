#!/usr/bin/env python3
"""Compile and run the AppKit jobs and saved-revision regression probe against current app sources.

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

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

# The probe run's wall-clock bound. The full case list measured ~234 s after
# batch 81 (geometry follow-ups), too close to the former 300 s; 480 s keeps
# a filtered DR_JOBS_PROBE_REPEAT run and a loaded machine inside it. The
# opt-in "slider preview latency" run measured 454 s with two 16-bit Applies
# per row, 670 s once the Reduce Noise and Dust & Scratches rows were added,
# and 891 s with the cancel, immediate and busy-slot stages, so the bound is
# 1.5 × 891 s.
RUN_TIMEOUT_SECONDS = 1337

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
# The model store cases' loopback file server, loaded by path for the same reason.
_server_spec = importlib.util.spec_from_file_location(
    "range_file_server", ROOT / "scripts/range_file_server.py")
range_file_server = importlib.util.module_from_spec(_server_spec)
_server_spec.loader.exec_module(range_file_server)


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
        DR_JOBS_PROBE_DIR=str(scratch / "files"),
        DR_JOBS_PROBE_RAW_DIR=str(scratch / "raw"),
        # The round-2 UI cases' photos, read in place (never copied).
        DR_JOBS_PROBE_FIXTURES=os.environ.get(
            "DR_JOBS_PROBE_FIXTURES", str(Path.home() / "devel/doctored-fixtures")),
        DR_PASTEBOARD_NAME=f"com.kgalligan.doctored.jobs-probe.{os.getpid()}",
        **preset_environment(scratch),
    )
    # Keep the real document type registrations, with a private preferences
    # domain. The copy is embedded in the probe, never written into the app.
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.jobs-probe"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    (scratch / "files").mkdir()
    (scratch / "printers").mkdir()
    # The model store cases download through the app's own downloader from this server:
    # it serves what the probe writes under files/model-server and logs every request.
    model_log = scratch / "model-server.log"
    model_server, model_url = range_file_server.start(scratch / "files/model-server", model_log)
    environment["DR_JOBS_PROBE_MODEL_SERVER"] = model_url
    environment["DR_JOBS_PROBE_MODEL_LOG"] = str(model_log)

    # The Develop cases' camera fixtures, built by a child process so that
    # `scripts/` stays off this process's sys.path. A failure here never ends
    # the run: the child logs `SKIP <file>` and only the Develop case whose
    # file is absent fails.
    fixtures_log = scratch / "raw-fixtures.log"
    try:
        with fixtures_log.open("w") as log:
            subprocess.run(
                [sys.executable, str(HERE / "raw_fixtures.py"), str(scratch / "raw")],
                cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT,
                timeout=120, check=False)
    except (OSError, subprocess.SubprocessError) as error:
        print(f"RAW fixtures were not built: {error}", flush=True)
    if fixtures_log.is_file():
        print(fixtures_log.read_text(errors="replace"), end="", flush=True)

    build_log = scratch / "build.log"
    binary = scratch / "probe"
    probe_build.build(
        sources=[*probe_build.app_sources(), *probe_build.probe_sources("jobs-probe")],
        bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
        library=library / "libdoctored_core.a", info_plist=info_copy,
        output=binary, log=build_log, env=environment, timeout=600)

    run_log = scratch / "run.log"
    try:
        run_logged([
            binary, "-ApplePersistenceIgnoreState", "YES",
            "-ColorWorkingSpace", "srgb",
        ], run_log, environment, RUN_TIMEOUT_SECONDS)
    finally:
        model_server.shutdown()
    print(run_log.read_text(errors="replace"), end="")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep scratch outputs after success too.")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS, AppKit and the Apple Silicon build toolchain.")

    scratch = Path(tempfile.mkdtemp(prefix="rasterize-jobs-probe-"))
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
