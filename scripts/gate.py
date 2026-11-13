#!/usr/bin/env python3
"""The one Full verification command: locked, input-hashed, timed.

Steps: `wait`, `docs`, `release`, `fmt`, `core-build`, then the Rust leg
(`clippy`, `tests`, `mcp-tests`) beside the Swift leg (`app`, `launch`) and the
agent leg (`agent-tests`), then one step per probe run (`PROBE_RUNS`), then
`probes`. `docs` is `scripts/check_docs.py` and `release` is `make
release-check` (the release tools' selftests: `scripts/appcast.py` and
`scripts/updater_check.py`, about a second); both run every time. `core-build`
is `make rust project`: the release core, whose hash the Swift and probe legs
fold in, and the generated `app/Info.plist` the probe runs embed, made before
any leg reads it (the `app` step's own `make project` then leaves the unchanged
file alone). `fmt` checks both crates (`core/` and the `doctored-mcp` helper's
`mcp-proxy/`). `clippy` is `make lint`: `scripts/check_layers.py`, then cargo
clippy on both crates. `mcp-tests` is `make mcp-test`, the helper's own suite.
`agent-tests` is `make agent-test`, the assistant helper's own suite (`swift
test` in `agent/`, designs/assistant-helper.md); it needs no core, so it starts
before `core-build` and runs even when that step fails. `launch` starts the
Release app on a scratch `DR_AGENT_SOCKET` (`mcp_client.scratch_socket`), with
`verification_isolation.owned_environment`, and passes when `tools/list`
answers over it, the bundle holds an executable
`Contents/Helpers/doctored-mcp` and an executable
`Contents/Helpers/doctored-agent` signed with the hardened runtime and no
entitlements, the bundle's `Contents/Resources/agent-catalog.json` lists the
same tools as the live answer, compared as JSON values, and the Release bundle
passes the updater check's rows R1–R5 (`updater_check.release_failures`:
Sparkle.framework is embedded but not loaded, the feed and key are empty, the
updater class is absent, the seal verifies), which proves Release launches with
Sparkle embedded and not loaded. An `--app` override is any executable and is not held to
R1–R5. Once the app has stopped, `launch` also runs the bundle's own
`doctored-agent` through `scripts/verify_agent_helper.py`'s `text-turn` and
`framing` cases (a loopback API, no key, a few seconds; `helper_failure`), so
the copy that ships, built -O in batches, stripped and re-signed with the
hardened runtime, has run, where the `agent-tests` suite runs SwiftPM's debug
build. A leg whose inputs hash to the value stored at its last pass is
skipped.

The probe-run steps are `PROBE_RUNS`, each a probe runner's small run, named
after its directory: `precision-probe` (no flag; its `--large*` modes measure
time and memory on 24 MP sources and never run here), `history-probe`,
`selection-history-probe`, `selection-probe` (`--synthetic`: the checks that
read private photo folders are skipped), `save-preservation-probe`,
`color-metadata-probe`, `data-migration-probe`, `path-guard-probe` and
`updater-probe`. Such a step runs its runner twice under `DR_BACKGROUND=1`,
into one log: under `DR_PROBE_BUILD_ONLY=1` beside the Rust and Swift legs,
which compiles the probe into `build/probe-cache` and launches nothing, then
once every leg has ended, which reuses that build, runs the probe and checks
its oracles. Each build phase compiles the whole app with its probe, so at most
`PROBE_BUILD_JOBS` of them run at once; the others wait for a slot before their
runner starts, so a runner's own compile timeout never counts the wait. The
runs wait for the legs because the probes' verdicts include wall-clock bounds
(a closed window gets two seconds of event turns to release its document), and
they must not depend on the load of the gate's own legs; they run one at a
time, in `PROBE_RUNS` order. A step is skipped while its inputs
(`ProbeRun.inputs`) and the core library hash to its last pass; a failure
stores nothing, so the next gate runs it again. The gate refuses to start with
more probe runs than `build/probe-cache` keeps builds
(`probe_build.KEEP_ENTRIES`), since each build would then prune an earlier one
before its run reused it.

`probes` type-checks, through
`scripts/probe_build.py --typecheck`, each probe set whose directory under
`scripts/` has uncommitted changes or whose last type check failed, and fails
on a `scripts/` directory that holds Swift and is not registered in
`probe_build.py`. Everything it writes is under `build/`: `gate.lock`,
`gate-state.json`, `gate-logs/<step>.log`, `gate-times.tsv`.

The lock and the `wait` step are per checkout: several git worktrees (the
lanes under `.claude/worktrees/`) gate in parallel, each with its own
`core/target` and `build/`. `wait` waits only for a build tool whose working
directory is inside this checkout, or a whole-suite run or gate whose script
path resolves inside it (the build tools are cargo, rustc, clippy-driver,
xcodebuild, and SwiftPM's swift-build and swift-test); a nested worktree is
another checkout, and a process whose working directory cannot be read counts,
to stay safe.

Standard library only; from `scripts/` it imports `app_under_test`,
`mcp_client`, `probe_build`, `updater_check` and `verification_isolation`.
"""

from __future__ import annotations

import argparse
import datetime
import fcntl
import hashlib
import http.client
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from pathlib import Path
from typing import NamedTuple

import app_under_test
import mcp_client
import probe_build
import updater_check
from verification_isolation import owned_environment

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "core"
BUILD = ROOT / "build"
LOCK = BUILD / "gate.lock"
STATE = BUILD / "gate-state.json"
LOGS = BUILD / "gate-logs"
TIMES = BUILD / "gate-times.tsv"
TESTS_SUMMARY = LOGS / "tests.json"
LIBRARY = CORE / "target/release/libdoctored_core.a"

RUST_INPUTS = ("core/", "mcp-proxy/", "scripts/run_tests.py", "scripts/gate.py", "Makefile",
               "scripts/check_layers.py")
# The app bundle carries the assistant's helper, so its package and build script are Swift inputs; its
# tests are not (AGENT_INPUTS).
SWIFT_INPUTS = ("app/", "agent/Package.swift", "agent/Package.resolved", "agent/Sources/",
                "scripts/build_agent_helper.sh", "scripts/verify_agent_helper.py", "mcp-proxy/",
                "core/include/doctored_core.h", "project.yml", "Makefile", "VERSION", "scripts/gate.py",
                "scripts/app_under_test.py", "scripts/verification_isolation.py", "scripts/mcp_client.py",
                "scripts/updater_check.py")
AGENT_INPUTS = ("agent/Package.swift", "agent/Package.resolved", "agent/Sources/", "agent/Tests/", "Makefile",
                "scripts/gate.py")
MCP_HELPER = "Contents/Helpers/doctored-mcp"
AGENT_HELPER = "Contents/Helpers/doctored-agent"
# `codesign --display --verbose=2`'s line for the code directory, e.g. `flags=0x10002(adhoc,runtime)`.
CODE_DIRECTORY_FLAGS = re.compile(r"^CodeDirectory .*\bflags=0x[0-9a-fA-F]+\(([^)]*)\)", re.MULTILINE)
CATALOG_SNAPSHOT = "Contents/Resources/agent-catalog.json"
# The stdio drive's cases `launch` runs against the bundle's helper (`helper_failure`), and their bound: the
# two take a few seconds, and every wait inside the drive is bounded on its own.
AGENT_SMOKE_CASES = "text-turn,framing"
AGENT_SMOKE_TIMEOUT_S = 120
BUILD_TOOLS = ("cargo", "rustc", "clippy-driver", "xcodebuild", "swift-build", "swift-test")
GATE_SCRIPTS = ("run_tests.py", "gate.py")
TIMES_HEADER = "utc\tcommit\tstep\tseconds\texit\tswift_files\ttest_binaries\n"
# A diagnostic's `error:` / `error[E0308]:`, not xcodebuild's `...=YES_ERROR` setting lines; a probe's failed
# group or case (`FAIL: <group>: …`, `FAIL [<check>]: …`, `FAIL <case>`) and its runner's `Probe failed: …` line.
FIRST_ERRORS = re.compile(r"(?i:\berror)(?:\[\w+\])?:|panicked at|FAILED|failed binaries|^FAIL\b|Probe failed:")
LAUNCH_TIMEOUT_S = 60
# Stored under "probe:<set name>" after a failing type check; never equal to an input hash (64 hex characters).
PROBE_FAILED = "failed"
PROBE_INPUTS = ("app/Sources/", "app/Bridging/", "core/include/doctored_core.h", "scripts/probe_build.py",
                "scripts/gate.py")


class ProbeRun(NamedTuple):
    """A probe runner the gate runs as the step `step`: `argv` from the checkout's root, skipped while `inputs`
    and the core library (`swift_hash`) hash to the value stored at its last pass."""
    step: str
    argv: tuple[str, ...]
    inputs: tuple[str, ...]


# What every probe run reads besides its own directory: the app sources and the header it compiles
# (PROBE_INPUTS), the private-directory list it loads, and the `app/Info.plist` it embeds, which `make project`
# generates from `project.yml` and `VERSION`; git ignores that file, so `input_hash` could not see it.
RUNNER_INPUTS = (*PROBE_INPUTS, "scripts/verification_isolation.py", "project.yml", "VERSION")


def probe_run(step: str, *flags: str, reads: tuple[str, ...] = ()) -> ProbeRun:
    """The probe run `step`: `scripts/<step>/verify.py` with `flags`, whose inputs are RUNNER_INPUTS, its own
    directory and `reads`."""
    return ProbeRun(step, ("python3", "-B", f"scripts/{step}/verify.py", *flags),
                    (*RUNNER_INPUTS, f"scripts/{step}/", *reads))


# The probe runs that are gate steps, each a runner's small run; a mode that measures time or memory never is,
# and neither is a runner that reads private photos or fixtures outside the tree or sends synthesized events.
PROBE_RUNS = (
    probe_run("precision-probe"),
    probe_run("history-probe"),
    probe_run("selection-history-probe"),
    # It writes its fixtures with two other probes' helpers.
    probe_run("selection-probe", "--synthetic",
              reads=("scripts/retouch-probe/fixtures.py", "scripts/mask-probe/fixtures.py")),
    # The layered TIFF writer and the float fixture; its `oversized.rs` links the core's rlib, which cargo builds
    # with the archive the core library's hash covers.
    probe_run("save-preservation-probe",
              reads=("scripts/layer_block_tiff.py", "core/tests/fixtures/float_11f/scene.tiff")),
    # The DNG writer, and the native reader `psd_evidence.py` loads with the PNG reader that one loads.
    probe_run("color-metadata-probe",
              reads=("scripts/phase3_fixtures.py", "scripts/live-filter-probe/native.py",
                     "scripts/raw-probe/native.py")),
    probe_run("data-migration-probe"),
    probe_run("path-guard-probe"),
    probe_run("updater-probe"),
)
# Each build phase is a whole-app compile beside the Release build and the suite, so at most this many run at
# once; designs/build-velocity-status.md (Phase 6) has the forced gates measured at 1 and 2.
PROBE_BUILD_JOBS = 2
probe_build_slots = threading.BoundedSemaphore(PROBE_BUILD_JOBS)
STEPS = ("wait", "docs", "release", "fmt", "core-build", "clippy", "tests", "mcp-tests", "agent-tests", "app",
         "launch", *(probe.step for probe in PROBE_RUNS), "probes")

results: dict[str, dict] = {}
output_lock = threading.Lock()
state_lock = threading.Lock()


def utc_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def log_path(step: str) -> Path:
    return LOGS / f"{step}.log"


def excerpt(step: str) -> list[str]:
    """Up to 5 log lines: the first that look like errors, else the last non-empty ones."""
    try:
        lines = [line for line in log_path(step).read_text(errors="replace").splitlines() if line.strip()]
    except OSError:
        return []
    matching = [line for line in lines if FIRST_ERRORS.search(line)]
    return (matching[:5] if matching else lines[-5:])


def record(step: str, code: int | str, seconds: float) -> None:
    shown = "0" if code == "skip" else f"{seconds:.1f}"
    results[step] = {"utc": utc_now(), "exit": code, "seconds": shown}
    with output_lock:
        print(f"{step}  exit={code}  {shown}s  build/gate-logs/{step}.log")
        if code not in (0, "skip"):
            for line in excerpt(step):
                print("    " + line[:300])
        sys.stdout.flush()


def skip(step: str, reason: str) -> None:
    log_path(step).write_text(f"skipped: {reason}\n")
    record(step, "skip", 0)


def run(step: str, argv: list[str], cwd: Path) -> int:
    started = time.monotonic()
    with log_path(step).open("wb") as log:
        try:
            code = subprocess.run(argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=log,
                                  stderr=subprocess.STDOUT).returncode
        except OSError as error:
            log.write(f"error: could not start {argv[0]}: {error}\n".encode())
            code = 127
    record(step, code, time.monotonic() - started)
    return code


def other_worktrees() -> list[Path]:
    """Every other worktree of this repository nested inside ROOT (the lanes under `.claude/worktrees/`)."""
    listing = subprocess.run(["git", "worktree", "list", "--porcelain"], cwd=ROOT, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, text=True, errors="replace").stdout
    found = []
    for line in listing.splitlines():
        if line.startswith("worktree "):
            path = Path(line[len("worktree "):]).resolve()
            if path != ROOT and path.is_relative_to(ROOT):
                found.append(path)
    return found


def process_cwds(pids: list[str]) -> dict[str, str]:
    """`pid -> working directory` from one `lsof` call; a pid it cannot read is absent."""
    if not pids:
        return {}
    listing = subprocess.run(["lsof", "-a", "-d", "cwd", "-Fn", "-p", ",".join(pids)], stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, text=True, errors="replace").stdout
    cwds, pid = {}, None
    for line in listing.splitlines():
        if line.startswith("p"):
            pid = line[1:]
        elif line.startswith("n") and pid is not None:
            cwds[pid] = line[1:]
    return cwds


def foreign_builds() -> list[str]:
    """`<pid>:<name>` for every build, whole-suite run or gate in THIS checkout that this process did not start.

    A build tool counts when its working directory is inside ROOT and not inside a nested worktree; a gate or
    whole-suite script when its script path (resolved against that working directory) is. A pid whose working
    directory cannot be read counts.
    """
    listing = subprocess.run(["ps", "-axo", "pid=,command="], stdout=subprocess.PIPE, text=True,
                             errors="replace").stdout
    candidates = []
    for line in listing.splitlines():
        fields = line.split()
        if len(fields) < 2 or not fields[0].isdigit() or int(fields[0]) == os.getpid():
            continue
        name = os.path.basename(fields[1])
        if name in BUILD_TOOLS:
            candidates.append((fields[0], name, None))
        elif name.lower().startswith("python"):
            for script in GATE_SCRIPTS:
                argument = next((argument for argument in fields[2:] if argument.endswith("scripts/" + script)),
                                None)
                if argument is not None:
                    candidates.append((fields[0], script, argument))
                    break
    if not candidates:
        return []
    cwds = process_cwds([pid for pid, _, _ in candidates])
    nested = other_worktrees()

    def in_checkout(path: Path) -> bool:
        return path.is_relative_to(ROOT) and not any(path.is_relative_to(other) for other in nested)

    found = []
    for pid, name, argument in candidates:
        cwd = cwds.get(pid)
        if cwd is None:
            found.append(f"{pid}:{name}")
            continue
        path = Path(cwd) if argument is None else Path(cwd, argument)
        try:
            path = path.resolve()
        except OSError:
            found.append(f"{pid}:{name}")
            continue
        if in_checkout(path):
            found.append(f"{pid}:{name}")
    return found


def input_hash(paths: tuple[str, ...]) -> str:
    listing = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z", "--", *paths],
                             cwd=ROOT, stdout=subprocess.PIPE, check=True).stdout
    digest = hashlib.sha256()
    for name in sorted(set(listing.decode(errors="surrogateescape").split("\0"))):
        if not name or name.endswith(".md"):
            continue
        path = ROOT / name
        if path.is_symlink():
            content = hashlib.sha256(os.readlink(path).encode(errors="surrogateescape"))
        elif path.is_file():
            with path.open("rb") as handle:
                content = hashlib.file_digest(handle, "sha256")
        else:
            continue
        digest.update(name.encode(errors="surrogateescape") + b"\0" + content.hexdigest().encode() + b"\n")
    return digest.hexdigest()


def swift_hash(source_hash: str) -> str:
    digest = hashlib.sha256(source_hash.encode())
    with LIBRARY.open("rb") as handle:
        while block := handle.read(1 << 20):
            digest.update(block)
    return digest.hexdigest()


def read_state() -> dict:
    try:
        state = json.loads(STATE.read_text())
    except (OSError, ValueError):
        return {}
    return state if isinstance(state, dict) else {}


def store(leg: str, value: str) -> None:
    with state_lock:
        state = read_state()
        state[leg] = value
        scratch = STATE.with_suffix(".json.tmp")
        scratch.write_text(json.dumps(state) + "\n")
        os.replace(scratch, STATE)


def tools_listed(client: mcp_client.Client) -> list:
    """The tools `tools/list` answered over the socket; raises when there is no such reply."""
    reply = client.post({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}, timeout=10)
    tools = reply["result"]["tools"]
    if not isinstance(tools, list) or not tools:
        raise ValueError("tools/list returned no tools")
    return tools


def signing_failure(helper: Path) -> str | None:
    """Why `helper` is not signed as the Build Agent Helper phase signs it, else None: a signature whose code
    directory has the `runtime` flag (the hardened runtime), and no entitlements at all."""
    shown = subprocess.run(["codesign", "--display", "--verbose=2", str(helper)], stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, errors="replace")
    if shown.returncode != 0:
        lines = shown.stdout.strip().splitlines()
        return f"{helper} is not signed: {lines[-1] if lines else 'codesign failed'}"
    flags = CODE_DIRECTORY_FLAGS.search(shown.stdout)
    if flags is None or "runtime" not in flags.group(1).split(","):
        return f"{helper} is not signed with the hardened runtime ({flags.group(0) if flags else 'no flags line'})"
    entitlements = subprocess.run(["codesign", "--display", "--entitlements", "-", "--xml", str(helper)],
                                  stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if entitlements.returncode != 0:
        return f"could not read the entitlements of {helper}"
    if entitlements.stdout.strip():
        return f"{helper} carries entitlements: {entitlements.stdout.decode(errors='replace').strip()[:200]}"
    return None


def bundle_failure(executable: Path, live: list, release_bundle: bool) -> str | None:
    """Why the bundle around `executable` is incomplete, else None: both helpers must be executable, the
    assistant's signed with the hardened runtime and no entitlements (`signing_failure`), the catalog snapshot
    must list the live tools, compared as JSON values (key order differs between processes), and, for the
    Release bundle alone, the updater check's rows R1–R5 must pass."""
    bundle = executable.resolve().parents[2]
    for helper in (bundle / MCP_HELPER, bundle / AGENT_HELPER):
        if not os.access(helper, os.X_OK):
            return f"{helper} is missing or not executable"
    unsigned = signing_failure(bundle / AGENT_HELPER)
    if unsigned is not None:
        return unsigned
    snapshot = bundle / CATALOG_SNAPSHOT
    try:
        tools = json.loads(snapshot.read_text())["tools"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        return f"could not read the tools of {snapshot}: {type(error).__name__}: {error}"
    if tools != live:
        return f"{snapshot} lists {len(tools)} tools that differ from the live {len(live)}"
    if release_bundle:
        failures = updater_check.release_failures(bundle)
        if failures:
            return "; ".join(failures)
    return None


def helper_failure(bundle: Path, log) -> str | None:
    """Why the bundle's assistant helper fails its stdio smoke run, else None: `scripts/verify_agent_helper.py`'s
    `AGENT_SMOKE_CASES` against the drive's loopback Messages API with a dummy key, the bundle's own catalog, and
    the app's scrubbed environment for the helper. Its output goes to `log`. Freshness is not this step's
    question (the gate judges the bundle it launched), so the drive's stale-helper refusal is turned off."""
    helper = bundle / AGENT_HELPER
    log.write(f"launch: driving {helper} ({AGENT_SMOKE_CASES})\n".encode())
    log.flush()
    command = [sys.executable, str(ROOT / "scripts" / "verify_agent_helper.py"), "--helper", str(helper),
               "--catalog", str(bundle / CATALOG_SNAPSHOT), "--cases", AGENT_SMOKE_CASES]
    try:
        done = subprocess.run(command, cwd=ROOT, env=dict(os.environ, DR_ALLOW_STALE_APP="1"),
                              stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                              timeout=AGENT_SMOKE_TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return f"the stdio drive of {helper} did not finish within {AGENT_SMOKE_TIMEOUT_S} s"
    except OSError as error:
        return f"could not start the stdio drive of {helper}: {error}"
    if done.returncode != 0:
        return f"{helper} failed scripts/verify_agent_helper.py --cases {AGENT_SMOKE_CASES} (exit {done.returncode})"
    return None


def launch(executable: Path, release_bundle: bool) -> int:
    """Start `executable` as an owned instance and judge it (`bundle_failure`); `release_bundle` holds the
    bundle to R1–R5, which an `--app` override is not."""
    started = time.monotonic()
    agent_socket = mcp_client.scratch_socket("rz-gate-")
    client = mcp_client.Client(agent_socket)
    code, verdict = 1, ""
    with tempfile.TemporaryDirectory(prefix="rz-gate-", ignore_cleanup_errors=True) as scratch, \
            log_path("launch").open("wb") as log:
        folder = Path(scratch)
        env = dict(os.environ, **owned_environment(folder, agent_socket))
        log.write(f"launch: {executable} on {agent_socket}\n".encode())
        log.flush()
        try:
            process = subprocess.Popen(
                [str(executable), "-ApplePersistenceIgnoreState", "YES"],
                env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        except OSError as error:
            process, verdict = None, f"error: could not start {executable}: {error}"
        if process is not None:
            try:
                deadline = time.monotonic() + LAUNCH_TIMEOUT_S
                last = "no reply"
                while True:
                    status = process.poll()
                    if status is not None:
                        verdict = f"error: the app exited with status {status} before answering tools/list"
                        break
                    try:
                        live = tools_listed(client)
                    except (OSError, ValueError, KeyError, TypeError, RuntimeError,
                            http.client.HTTPException) as error:
                        last = f"{type(error).__name__}: {error}"
                    else:
                        incomplete = bundle_failure(executable, live, release_bundle)
                        if incomplete is None:
                            code = 0
                            verdict = (f"tools/list answered with {len(live)} tools after "
                                       f"{time.monotonic() - started:.1f} s; both helpers and the catalog "
                                       "snapshot match"
                                       + ("; R1–R5 pass" if release_bundle else ""))
                        else:
                            verdict = f"error: {incomplete}"
                        break
                    if time.monotonic() >= deadline:
                        verdict = f"error: no tools/list reply within {LAUNCH_TIMEOUT_S} s ({last})"
                        break
                    time.sleep(0.2)
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                mcp_client.remove_scratch(agent_socket)
        if code == 0:
            # The app has stopped; the helper it would start runs on its own, as the Assistant panel starts it.
            log.seek(0, os.SEEK_END)
            failed = helper_failure(executable.resolve().parents[2], log)
            if failed is None:
                verdict += f"; the bundle's doctored-agent passes {AGENT_SMOKE_CASES}"
            else:
                code, verdict = 1, f"error: {failed}"
        log.seek(0, os.SEEK_END)
        log.write(f"launch: {verdict}\n".encode())
    record("launch", code, time.monotonic() - started)
    return code


def rust_leg(run_it: bool, fmt_code: int | str, wanted: str) -> None:
    if not run_it:
        skip("clippy", "the Rust inputs are unchanged since the last pass")
        skip("tests", "the Rust inputs are unchanged since the last pass")
        skip("mcp-tests", "the Rust inputs are unchanged since the last pass")
        return
    clippy = run("clippy", ["make", "lint"], ROOT)
    tests = run("tests", ["python3", "scripts/run_tests.py", "--summary", "build/gate-logs/tests.json"], ROOT)
    mcp_tests = run("mcp-tests", ["make", "mcp-test"], ROOT)
    if fmt_code == 0 and clippy == 0 and tests == 0 and mcp_tests == 0:
        store("rust", wanted)


def agent_leg(run_it: bool, wanted: str) -> None:
    if not run_it:
        skip("agent-tests", "the assistant helper's inputs are unchanged since the last pass")
        return
    if run("agent-tests", ["make", "agent-test"], ROOT) == 0:
        store("agent", wanted)


def swift_leg(run_it: bool, wanted: str, app: Path | None) -> None:
    if not run_it:
        skip("app", "the Swift inputs and the core library are unchanged since the last pass")
        skip("launch", "the Swift inputs and the core library are unchanged since the last pass")
        return
    if run("app", ["make", "app"], ROOT) != 0:
        skip("launch", "the app build failed")
        return
    launched = launch(app if app is not None else app_under_test.RELEASE_APP / app_under_test.EXECUTABLE_IN_BUNDLE,
                      release_bundle=app is None)
    if launched == 0 and app is None:
        store("swift", wanted)


def probe_run_key(probe: ProbeRun) -> str:
    """The state key of `probe`'s step, apart from the type check's `probe:<set name>`."""
    return f"probe-run:{probe.step}"


def probe_run_phase(probe: ProbeRun, phase: str, environment: dict[str, str], log) -> tuple[int, float]:
    """Run `probe`'s runner once with `environment`, its output into `log`; its exit status and seconds.

    No timeout of the gate's own: the runner bounds its compile and its run and kills its child's process
    group, which a kill of the runner from here would orphan."""
    started = time.monotonic()
    log.write(("$ " + " ".join(probe.argv) + f"  ({phase})\n").encode())
    log.flush()
    try:
        code = subprocess.run(list(probe.argv), cwd=ROOT, env=environment, stdin=subprocess.DEVNULL, stdout=log,
                              stderr=subprocess.STDOUT).returncode
    except OSError as error:
        log.write(f"error: could not start {probe.argv[0]}: {error}\n".encode())
        code = 127
    seconds = time.monotonic() - started
    log.seek(0, os.SEEK_END)
    log.write(f"{probe.step}: {phase} exit {code} {seconds:.1f}s\n".encode())
    log.flush()
    return code, seconds


def probe_build_phase(probe: ProbeRun, outcome: dict) -> None:
    """`probe`'s first phase, beside the Rust and Swift legs: its runner under `DR_PROBE_BUILD_ONLY=1`, which
    compiles the probe into `build/probe-cache` (or finds it there) and launches nothing. It starts the step's
    log, waits for one of the `PROBE_BUILD_JOBS` build slots before the runner starts, and puts the phase's
    `code` and `seconds` (the wait left out) in `outcome`."""
    environment = dict(os.environ, DR_BACKGROUND="1", **{probe_build.BUILD_ONLY_ENV: "1"})
    with log_path(probe.step).open("wb") as log:
        waiting = time.monotonic()
        with probe_build_slots:
            log.write(f"{probe.step}: waited {time.monotonic() - waiting:.1f}s for a probe build slot\n".encode())
            outcome["code"], outcome["seconds"] = probe_run_phase(probe, "build", environment, log)


def probe_run_finish(probe: ProbeRun, wanted: str, outcome: dict) -> None:
    """`probe`'s second phase, once every leg has ended, when the first passed: its runner without
    `DR_PROBE_BUILD_ONLY`, which reuses the cached build, runs the probe and checks its oracles. Records the step
    with the failing phase's exit and both phases' seconds, and stores `wanted` only on a pass."""
    code, seconds = outcome.get("code", 1), outcome.get("seconds", 0.0)
    if code == 0:
        environment = dict(os.environ, DR_BACKGROUND="1")
        environment.pop(probe_build.BUILD_ONLY_ENV, None)
        with log_path(probe.step).open("ab") as log:
            code, ran = probe_run_phase(probe, "run", environment, log)
        seconds += ran
    record(probe.step, code, seconds)
    if code == 0:
        store(probe_run_key(probe), wanted)


def git(*argv: str) -> str:
    return subprocess.run(["git", *argv], cwd=ROOT, stdout=subprocess.PIPE, text=True).stdout


def probe_leg(force: bool) -> None:
    """Type-check each probe set whose directory changed or whose last type check failed.

    `force` only turns off the skip of a set whose inputs hash to its last pass; it adds no set.
    """
    started = time.monotonic()
    stored = read_state()
    scripts = ROOT / "scripts"
    registered = set(probe_build.PROBE_SETS) | set(probe_build.STANDALONE_DIRS)
    unregistered = sorted({path.parent.name for path in scripts.glob("*/*.swift")} - registered)
    changed = set()
    for listing in (git("diff", "--name-only", "HEAD", "--", "scripts/"),
                    git("ls-files", "-o", "--exclude-standard", "--", "scripts/")):
        for name in listing.splitlines():
            parts = name.split("/")
            if len(parts) > 2:
                changed.add(parts[1])

    ran, failed, unchanged = [], [], []
    with log_path("probes").open("w") as log:
        for name in unregistered:
            log.write(f"error: scripts/{name} holds Swift files and is in neither PROBE_SETS nor STANDALONE_DIRS "
                      "of scripts/probe_build.py\n")
        for name in sorted(probe_build.PROBE_SETS):
            directories = sorted({path.parent.name for path in probe_build.probe_sources(name)})
            key = f"probe:{name}"
            if not changed.intersection(directories) and stored.get(key) != PROBE_FAILED:
                continue
            wanted = input_hash((*PROBE_INPUTS, *(f"scripts/{directory}/" for directory in directories)))
            if not force and stored.get(key) == wanted:
                unchanged.append(name)
                continue
            argv = ["python3", "scripts/probe_build.py", "--typecheck", f"scripts/{name}"]
            log.write("$ " + " ".join(argv) + "\n")
            log.flush()
            try:
                code = subprocess.run(argv, cwd=ROOT, stdin=subprocess.DEVNULL, stdout=log,
                                      stderr=subprocess.STDOUT).returncode
            except OSError as error:
                log.write(f"error: could not start {argv[0]}: {error}\n")
                code = 127
            log.seek(0, os.SEEK_END)
            log.write(f"{name}: exit {code}\n")
            log.flush()
            store(key, wanted if code == 0 else PROBE_FAILED)
            ran.append(name)
            if code != 0:
                failed.append(name)
        if unchanged:
            log.write("unchanged since their last pass: " + " ".join(unchanged) + "\n")
    if unregistered or failed:
        record("probes", 1, time.monotonic() - started)
    elif ran:
        record("probes", 0, time.monotonic() - started)
    elif unchanged:
        skip("probes", "every changed probe set is unchanged since its last pass: " + " ".join(unchanged))
    else:
        skip("probes", "no probe directory under scripts/ has uncommitted changes and no probe set failed its "
                       "last type check")


def append_times() -> None:
    commit = git("rev-parse", "--short", "HEAD").strip()
    if git("status", "--porcelain").strip():
        commit += "-dirty"
    swift_files = len(list((ROOT / "app/Sources").glob("*.swift")))
    binaries = ""
    if results.get("tests", {}).get("exit") != "skip":
        try:
            binaries = str(json.loads(TESTS_SUMMARY.read_text())["binaries"])
        except (OSError, ValueError, KeyError):
            binaries = ""
    rows = "".join(f"{results[step]['utc']}\t{commit}\t{step}\t{results[step]['seconds']}\t{results[step]['exit']}"
                   f"\t{swift_files}\t{binaries}\n" for step in STEPS if step in results)
    fresh = not TIMES.exists()
    with TIMES.open("a") as table:
        if fresh:
            table.write(TIMES_HEADER)
        table.write(rows)


def main() -> int:
    """`gate()`, with a final `gate:` line when it raises outside the legs."""
    started = time.monotonic()
    try:
        return gate()
    except Exception:
        traceback.print_exc()
        sys.stderr.flush()
        print(f"gate: FAIL (gate) {time.monotonic() - started:.1f}s", flush=True)
        return 1


def gate() -> int:
    parser = argparse.ArgumentParser(description="Run the docs check, the release tools' selftests, fmt, the "
                                                 "core build, clippy, the whole suite, the doctored-mcp suite, "
                                                 "the assistant helper's suite, the Release app build, a launch "
                                                 "over a scratch socket with the Release bundle's updater check "
                                                 "(R1–R5), the probe runs (PROBE_RUNS: each a probe runner's "
                                                 "small run, built beside the legs at most PROBE_BUILD_JOBS at a "
                                                 "time and run once they end) and the probes step (a type check of "
                                                 "each changed probe directory); skip a leg whose inputs are "
                                                 "unchanged since its last pass.")
    parser.add_argument("--force", action="store_true",
                        help="ignore stored hashes; run every step, the probe-run steps included (the probes "
                             "step still checks only probe directories that changed or failed last time)")
    parser.add_argument("--wait", type=float, default=120, metavar="SECONDS",
                        help="seconds to wait for this checkout's build/gate.lock and for builds, whole-suite "
                             "runs and gates in this checkout (other worktrees' builds are not waited for), "
                             "default 120")
    parser.add_argument("--app", type=Path, metavar="EXECUTABLE",
                        help="executable the launch step starts instead of the Release app; "
                             "a Swift pass is then not stored, and the updater check's rows R1–R5 apply "
                             "only to the Release bundle, so they are not run")
    args = parser.parse_args()
    if len(PROBE_RUNS) > probe_build.KEEP_ENTRIES:
        raise RuntimeError(f"{len(PROBE_RUNS)} probe runs, but build/probe-cache keeps {probe_build.KEEP_ENTRIES} "
                           "builds: each build would prune an earlier one before its run reused it")

    started = time.monotonic()
    deadline = started + args.wait
    LOGS.mkdir(parents=True, exist_ok=True)
    lock = LOCK.open("a")
    while True:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            break
        except OSError:
            if time.monotonic() >= deadline:
                print("gate: BLOCKED lock build/gate.lock", flush=True)
                return 3
            time.sleep(0.5)

    while True:
        foreign = foreign_builds()
        if not foreign:
            break
        if time.monotonic() >= deadline:
            print("gate: BLOCKED " + " ".join(foreign), flush=True)
            return 3
        time.sleep(1)
    log_path("wait").write_text(f"waited {time.monotonic() - started:.1f} s for build/gate.lock and for foreign "
                                "builds\n")
    record("wait", 0, time.monotonic() - started)
    run("docs", ["python3", "scripts/check_docs.py"], ROOT)
    run("release", ["make", "release-check"], ROOT)

    state = {} if args.force else read_state()
    rust_wanted = input_hash(RUST_INPUTS)
    swift_sources = input_hash(SWIFT_INPUTS)
    run_rust = state.get("rust") != rust_wanted
    if run_rust:
        TESTS_SUMMARY.unlink(missing_ok=True)
    agent_wanted = input_hash(AGENT_INPUTS)
    agent = threading.Thread(target=agent_leg, args=(state.get("agent") != agent_wanted, agent_wanted))
    agent.start()

    fmt_code: int | str
    if run_rust:
        fmt_code = run("fmt", ["sh", "-c", "cargo fmt --check --manifest-path core/Cargo.toml && "
                                       "cargo fmt --check --manifest-path mcp-proxy/Cargo.toml"], ROOT)
    else:
        skip("fmt", "the Rust inputs are unchanged since the last pass")
        fmt_code = "skip"

    probe_runs: list[tuple[ProbeRun, str, dict]] = []
    if run("core-build", ["make", "rust", "project"], ROOT) != 0:
        for step in ("clippy", "tests", "mcp-tests", "app", "launch", *(probe.step for probe in PROBE_RUNS)):
            skip(step, "the core build failed")
    else:
        swift_wanted = swift_hash(swift_sources)
        run_swift = state.get("swift") != swift_wanted
        legs = [threading.Thread(target=rust_leg, args=(run_rust, fmt_code, rust_wanted)),
                threading.Thread(target=swift_leg, args=(run_swift, swift_wanted, args.app))]
        for probe in PROBE_RUNS:
            probe_wanted = swift_hash(input_hash(probe.inputs))
            if state.get(probe_run_key(probe)) == probe_wanted:
                skip(probe.step, "the probe run's inputs and the core library are unchanged since the last pass")
                continue
            outcome: dict = {}
            probe_runs.append((probe, probe_wanted, outcome))
            legs.append(threading.Thread(target=probe_build_phase, args=(probe, outcome)))
        for leg in legs:
            leg.start()
        for leg in legs:
            leg.join()
    agent.join()
    # Every leg has ended, so a probe's wall-clock bounds are not measured beside the gate's own load.
    for probe, wanted, outcome in probe_runs:
        try:
            probe_run_finish(probe, wanted, outcome)
        except Exception:
            traceback.print_exc()
            sys.stderr.flush()
    try:
        probe_leg(args.force)
    except Exception:
        traceback.print_exc()
        sys.stderr.flush()
    for step in STEPS:
        if step not in results:
            log_path(step).write_text("error: the gate stopped before this step finished\n")
            record(step, 1, 0)

    append_times()
    failed = [step for step in STEPS if results.get(step, {}).get("exit") not in (0, "skip")]
    seconds = f"{time.monotonic() - started:.1f}"
    if failed:
        print(f"gate: FAIL ({' '.join(failed)}) {seconds}s", flush=True)
        return 1
    print(f"gate: PASS {seconds}s", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
