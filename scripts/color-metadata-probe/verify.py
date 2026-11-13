#!/usr/bin/env python3
"""Compile and run the AppKit color and metadata regression probe against current app sources.

Run `make rust project` first. The linked probe is built by `scripts/probe_build.py`
and cached under `build/probe-cache`; the copy that runs, fixtures and logs
live in a temporary directory, retained on failure or with --keep.
With `DR_PROBE_BUILD_ONLY=1` the runner stops after the build, launching
nothing, and removes its scratch directory unless --keep. `make gate`'s
`color-metadata-probe` step runs it (every check) this way and then again in full.
"""

import argparse
import hashlib
import importlib.util
import json
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

sys.dont_write_bytecode = True
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
CHECKS = {"TIFF/HEIC", "RAW", "metadata limits", "PSD imports", "calibrated operations",
          "refused profiles", "core open profiles"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def dependencies():
    """The files `snapshot` captures and digests into `inputs.json`."""
    paths = {ROOT / name for name in (
        "app/Info.plist", "Makefile", "project.yml", "core/Cargo.toml", "core/Cargo.lock")}
    for directory, pattern in (("app/Sources", "*.swift"), ("app/Bridging", "*.h"),
                               ("core/include", "*.h"), ("core/src", "*.rs"),
                               ("scripts", "*.swift"), ("scripts", "*.py")):
        paths.update((ROOT / directory).rglob(pattern))
    return paths


def snapshot(scratch, archive):
    paths = dependencies()
    frozen = scratch / "inputs"
    inputs = {}
    for path in sorted(paths):
        relative = path.relative_to(ROOT)
        destination = frozen / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        expected = digest(path)
        if digest(destination) != expected:
            raise RuntimeError(f"Input changed during capture: {path}")
        inputs[str(relative)] = expected
    library = frozen / "libdoctored_core.a"
    shutil.copyfile(archive, library)
    archive_hash = digest(archive)
    if digest(library) != archive_hash:
        raise RuntimeError("Core archive changed during capture")
    manifest = {"source_root": str(ROOT), "inputs": inputs, "archive_path": str(archive),
                "archive_sha256": archive_hash}
    (scratch / "inputs.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return frozen, library, manifest


def run_logged(command, log_path, environment, timeout):
    with log_path.open("a") as log:
        print("$ " + shlex.join(map(str, command)), file=log, flush=True)
        subprocess.run(
            list(map(str, command)), cwd=ROOT, env=environment,
            stdout=log, stderr=subprocess.STDOUT, timeout=timeout, check=True,
        )


def verify(scratch, archive, checks):
    for required in [archive, ROOT / "app/Info.plist"]:
        if not required.is_file():
            raise RuntimeError(f"Missing {required}; run `make rust project` first.")
    frozen, library, manifest = snapshot(scratch, archive)
    info = frozen / "app/Info.plist"

    environment = dict(
        os.environ,
        CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
        ANTHROPIC_API_KEY="dummy",
        DR_AUTOMATION_ALERTS="record",
        DR_WORKING_LIMIT_BYTES=os.environ.get("DR_WORKING_LIMIT_BYTES", "2147483648"),
        DR_ACTIONS_DIR=str(scratch / "actions"),
        DR_RECOVERY_DIR=str(scratch / "recovery"),
        DR_COLOR_PROBE_DIR=str(scratch / "files"),
        DR_COLOR_PROBE_CHECKS=",".join(sorted(checks)),
        PYTHONDONTWRITEBYTECODE="1",
        **preset_environment(scratch),
    )
    # Keep the real document type registrations, with a private preferences
    # domain. The copy is embedded in the probe, never written into the app.
    with info.open("rb") as source:
        probe_info = plistlib.load(source)
    probe_info["CFBundleIdentifier"] = "com.kgalligan.doctored.color-metadata-probe"
    info_copy = scratch / "Info.plist"
    with info_copy.open("wb") as destination:
        plistlib.dump(probe_info, destination)

    (scratch / "files").mkdir()
    if "RAW" in checks:
        sys.path.insert(0, str(frozen / "scripts"))
        from phase3_fixtures import synthetic_dng
        synthetic_dng(scratch / "files" / "synthetic.dng")

    build_log = scratch / "build.log"
    binary = scratch / "probe"
    build_only = probe_build.take_build_only(environment)
    probe_build.build(
        app_sources=probe_build.app_sources(frozen),
        sources=probe_build.probe_sources("color-metadata-probe", frozen),
        bridging_header=frozen / "app/Bridging/Doctored-Bridging-Header.h",
        library=library, link_library="file", info_plist=info_copy,
        output=binary, log=build_log, env=environment, timeout=600)
    if build_only:
        print(f"build only: {binary}", flush=True)
        return
    binary_hash = digest(binary)

    run_log = scratch / "run.log"
    run_logged([
        binary, "-ApplePersistenceIgnoreState", "YES",
        "-ColorWorkingSpace", "srgb",
    ], run_log, environment, 180)
    print(run_log.read_text(), end="")
    evidence = None
    if "PSD imports" in checks:
        spec = importlib.util.spec_from_file_location(
            "color_probe_psd_evidence", frozen / "scripts/color-metadata-probe/psd_evidence.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        evidence = module.verify(scratch / "files")
    for relative, expected in manifest["inputs"].items():
        if digest(ROOT / relative) != expected or digest(frozen / relative) != expected:
            raise RuntimeError(f"Input changed during qualification: {relative}")
    if digest(archive) != manifest["archive_sha256"] or digest(library) != manifest["archive_sha256"]:
        raise RuntimeError("Core archive changed during qualification")
    if digest(binary) != binary_hash:
        raise RuntimeError("Probe executable changed during qualification")
    result = {"checks": sorted(checks), "inputs_unchanged": True,
              "captured_input_count": len(manifest["inputs"]),
              "inputs_manifest_sha256": digest(scratch / "inputs.json"),
              "archive_sha256": manifest["archive_sha256"], "probe_sha256": binary_hash,
              "embedded_info_sha256": digest(info_copy), "psd_evidence": evidence,
              "artifacts": {str(path.relative_to(scratch)): digest(path)
                            for path in sorted((scratch / "files").rglob("*")) if path.is_file()}}
    (scratch / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"Qualified {len(checks)} checks with {len(manifest['inputs'])} unchanged captured inputs.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true", help="Keep scratch outputs after success too.")
    parser.add_argument("--archive", type=Path, default=ROOT / "core/target/release/libdoctored_core.a",
                        help="Exact already-built core archive to capture and link.")
    parser.add_argument("--checks", default=",".join(sorted(CHECKS)),
                        help="Comma-separated check names; use 'PSD imports' for the PSD-only probe.")
    arguments = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("This probe requires macOS, AppKit and the Apple Silicon build toolchain.")
    checks = {name.strip() for name in arguments.checks.split(",")}
    if not checks or not checks <= CHECKS:
        parser.error(f"Unknown or empty checks; choose from {sorted(CHECKS)}")

    scratch = Path(tempfile.mkdtemp(prefix="rasterize-color-probe-"))
    print(f"Probe scratch directory: {scratch}", flush=True)
    try:
        verify(scratch, arguments.archive.resolve(), checks)
    except (AssertionError, OSError, RuntimeError, subprocess.SubprocessError) as error:
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
