#!/usr/bin/env python3
"""Measure actual RAW development source at 24/48 MP with synthetic CFA DNGs.

Compiles every app source except `main.swift` against the release core archive
(`core/target/release/libdoctored_core.a`; run `make rust` first), the way
`scripts/color-metadata-probe/verify.py` does, with a scratch `main.swift` that
calls `RawImage.develop(url:values:capabilities:)` directly. Nothing is stubbed
or omitted: the core metadata-density fallback, the core profile-ingest check,
the Core Image render, CoreGraphics profile validation and unpremultiply all
run. No Doctored app window, preference library or agent server is used; the
probe runs with private preference directories all the same. The linked probe is
built by `scripts/probe_build.py` and cached under `build/probe-cache`, keyed on
the generated `main.swift` with every other input, so an unchanged re-run
compiles nothing.
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

from phase3_fixtures import synthetic_dng
from verification_isolation import preset_environment

ROOT = Path(__file__).resolve().parents[1]
# The shared cached probe build, loaded by path.
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)

MAIN = """
import AppKit
import Foundation

// One function, so the probe adds no module-scope globals beside the app's.
func measure() throws {
    let root = URL(fileURLWithPath: CommandLine.arguments[1])
    for name in ["24mp.dng", "48mp.dng"] {
        let url = root.appendingPathComponent(name)
        guard let probe = RawImage.inspect(url: url) else { fatalError("CIRAWFilter rejected " + name) }
        for run in 0..<3 {
            try autoreleasepool {
                let start = ProcessInfo.processInfo.systemUptime
                let decoded = try RawImage.develop(url: url, values: probe.capabilities.defaults,
                                                   capabilities: probe.capabilities)
                let elapsed = ProcessInfo.processInfo.systemUptime - start
                guard decoded.width * decoded.height == (name == "24mp.dng" ? 24_000_000 : 48_000_000),
                      decoded.pixels.count == decoded.width * decoded.height * 4,
                      decoded.profile != nil,
                      decoded.pixels.contains(where: { $0 != 0 }) else {
                    fatalError("RAW decode returned invalid output")
                }
                let payload: [String: Any] = ["operation": "raw_develop", "fixture": name, "run": run,
                    "width": decoded.width, "height": decoded.height, "elapsed_ms": elapsed * 1000,
                    "swift_optimization": "Onone"]
                let data = try JSONSerialization.data(withJSONObject: payload, options: .sortedKeys)
                print(String(decoding: data, as: UTF8.self))
                fflush(stdout)
            }
        }
    }
}

try measure()
"""


def run(scratch):
    archive = ROOT / "core/target/release/libdoctored_core.a"
    if not archive.is_file():
        raise RuntimeError(f"Missing {archive}; run `make rust` first.")
    environment = dict(os.environ, ANTHROPIC_API_KEY="dummy",
                       CLANG_MODULE_CACHE_PATH=str(scratch / "clang-cache"),
                       **preset_environment(scratch))
    (scratch / "main.swift").write_text(MAIN)
    sources = probe_build.app_sources()
    binary = scratch / "probe"
    probe_build.build(app_sources=sources, sources=[scratch / "main.swift"],
                      bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
                      library=archive, link_library="file", output=binary, log=scratch / "build.log",
                      env=environment, timeout=600)
    for name, width, height in [("24mp.dng", 6000, 4000), ("48mp.dng", 8000, 6000)]:
        synthetic_dng(scratch / name, width=width, height=height)
    result = subprocess.run([str(binary), str(scratch)], env=environment, capture_output=True,
                            text=True, timeout=300)
    (scratch / "run.log").write_text(result.stdout + result.stderr)
    print(result.stdout, end="", flush=True)
    result.check_returncode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true")
    options = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("Requires macOS Core Image and Apple Silicon toolchain")
    scratch = Path(tempfile.mkdtemp(prefix="rasterize-phase4-raw-"))
    print(json.dumps({"scratch": str(scratch)}), flush=True)
    try:
        run(scratch)
    except BaseException:
        print(f"Probe artifacts retained: {scratch}", flush=True)
        raise
    if not options.keep:
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
