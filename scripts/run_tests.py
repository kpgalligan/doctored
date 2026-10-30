#!/usr/bin/env python3
"""The whole Rust suite: build the test executables once, run them side by side.

`cargo test --release --no-run` builds; every test executable then runs from
`core/`: the standalone ones `--jobs` at a time, then the other suites
(`core/tests/suite_*.rs`, each of which runs its own tests in parallel) one at
a time, with the serial suite (`SERIAL_SUITE`) in a lane of its own beside the
pool and the other suites, joined before the `SOLO` list runs one at a time.
Every executable runs whatever failed before it; the last line names the
failing ones. Ignored tests stay ignored.

Standard library only, and it imports nothing from `scripts/`.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "core"

# Run one at a time after the pool and the suites: the large-document binaries.
SOLO: tuple[str, ...] = ("float_adjust_apply_24mp",
    "float_master_24mp", "conversion_bake_24mp", "float_selected_pixels_24mp",
    "selected_color_planes_24mp", "float_interchange_24mp")
# A test target whose name starts so mounts many test files; it runs alone.
SUITE_PREFIX = "suite_"
# Every test in this suite takes one lock, so it uses about one core: it runs in
# its own lane beside the pool and the other suites, and is joined before the
# solo list, the large-document binaries.
SERIAL_SUITE = "suite_serial"
BINARY_TIMEOUT_S = 600


def build() -> tuple[int, dict[str, str], list[str]]:
    """Cargo's exit status, the test executables by name, and the rendered diagnostics."""
    done = subprocess.run(["cargo", "test", "--release", "--no-run", "--message-format=json"],
                          cwd=CORE, stdout=subprocess.PIPE, text=True)
    executables: dict[str, str] = {}
    rendered: list[str] = []
    for line in done.stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        reason = message.get("reason")
        if reason == "compiler-artifact":
            if message.get("executable") and message.get("profile", {}).get("test") is True:
                executables[message["target"]["name"]] = message["executable"]
        elif reason == "compiler-message":
            text = message.get("message", {}).get("rendered")
            if text:
                rendered.append(text)
    return done.returncode, executables, rendered


def run_one(name: str, executable: str, log_dir: Path, solo: bool) -> dict:
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(CORE))
    started = time.monotonic()
    with (log_dir / f"{name}.log").open("wb") as log:
        try:
            code: int | str = subprocess.run([executable], cwd=CORE, env=env, stdin=subprocess.DEVNULL,
                                             stdout=log, stderr=subprocess.STDOUT,
                                             timeout=BINARY_TIMEOUT_S).returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
        except OSError as error:
            log.write(f"error: could not start {executable}: {error}\n".encode())
            code = 127
    return {"name": name, "exit": code, "seconds": round(time.monotonic() - started, 2), "solo": solo}


def failure_excerpt(log: Path) -> list[str]:
    """The log from its first `failures:` line (at most 60 lines), else its last 40 lines."""
    try:
        lines = log.read_text(errors="replace").splitlines()
    except OSError:
        return []
    for index, line in enumerate(lines):
        if line.strip() == "failures:":
            return lines[index:index + 60]
    return lines[-40:]


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the Rust test executables once, run the standalone ones "
                                                 "--jobs at a time, then the other suites one at a time, with the "
                                                 "serial suite beside the pool and the other suites, joined before "
                                                 "the solo list runs one at a time.")
    parser.add_argument("--jobs", type=int, default=4, metavar="N", help="pool width, default 4")
    parser.add_argument("--summary", type=Path, metavar="PATH", help="write the JSON summary there")
    parser.add_argument("--log-dir", type=Path, default=ROOT / "build/test-logs", metavar="DIR",
                        help="per-binary output, default build/test-logs")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be at least 1")

    started = time.monotonic()
    status, executables, rendered = build()
    if status != 0:
        for text in rendered:
            print(text, end="" if text.endswith("\n") else "\n")
        print(f"cargo build failed: exit {status}", flush=True)
        return 2
    missing = [name for name in SOLO if name not in executables]
    if missing:
        print("solo names with no built executable: " + " ".join(missing), flush=True)
        return 2
    if SERIAL_SUITE not in executables:
        print(f"serial suite has no built executable: {SERIAL_SUITE}", flush=True)
        return 2

    log_dir = args.log_dir.resolve()
    log_dir.mkdir(parents=True, exist_ok=True)
    suites = sorted(name for name in executables
                    if name.startswith(SUITE_PREFIX) and name not in SOLO and name != SERIAL_SUITE)
    pooled = sorted(name for name in executables
                    if name not in SOLO and name not in suites and name != SERIAL_SUITE)
    with ThreadPoolExecutor(max_workers=1) as serial_lane:
        serial = serial_lane.submit(run_one, SERIAL_SUITE, executables[SERIAL_SUITE], log_dir, False)
        with ThreadPoolExecutor(max_workers=args.jobs) as pool:
            results = list(pool.map(lambda name: run_one(name, executables[name], log_dir, False), pooled))
        for name in suites:
            results.append(run_one(name, executables[name], log_dir, False))
        results.append(serial.result())
    for name in SOLO:
        results.append(run_one(name, executables[name], log_dir, True))

    failed = [result["name"] for result in results if result["exit"] != 0]
    seconds = round(time.monotonic() - started, 1)
    for result in results:
        if result["exit"] == 0:
            continue
        print(f"{result['name']}: exit {result['exit']}  {log_dir / (result['name'] + '.log')}")
        for line in failure_excerpt(log_dir / f"{result['name']}.log"):
            print("    " + line)
    if args.summary:
        args.summary.parent.mkdir(parents=True, exist_ok=True)
        args.summary.write_text(json.dumps({"binaries": len(results), "failed": failed, "seconds": seconds,
                                            "results": results}, indent=1) + "\n")
    if failed:
        print("failed binaries: " + " ".join(failed), flush=True)
        return 1
    print(f"ok: {len(results)} binaries, {seconds} s", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
