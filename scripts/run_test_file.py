#!/usr/bin/env python3
"""One Rust test file, whichever binary holds it: what `make t` and `make cancel-bench` run.

    python3 scripts/run_test_file.py <file> [<filter>] [<libtest argument>...]

`<file>` is a test file's stem (a `core/tests/` prefix and an `.rs` suffix are
stripped). A file with its own `[[test]]` entry in `core/Cargo.toml` runs as
`cargo test --release --test <file> -- [<filter>] ...`. A file a suite root
(`core/tests/suite_*.rs`) mounts runs as
`cargo test --release --test <suite> -- <file>::<filter> ...`, so there
`<filter>` is the start of the test's path inside the file, and every other
file of the suite whose name ends with `<file>` is skipped.

Exit status: 2 when no `[[test]]` entry names the file and no suite mounts it;
cargo's own when it fails; 1 when the run reached a test binary and ran no
test; else 0. A run that prints no `running N tests` line, such as `--list`,
is not a run of no test.

Standard library only, and it imports nothing from `scripts/`.
"""

from __future__ import annotations

import re
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "core"
TESTS = CORE / "tests"

SUITE_PREFIX = "suite_"
TEST_NAME = re.compile(r'^\[\[test\]\]\s*\nname\s*=\s*"([^"]+)"', re.MULTILINE)
MOUNT = re.compile(r'^#\[path = "([^/"]+)\.rs"\]\nmod \1;', re.MULTILINE)
RUNNING = re.compile(r"^running (\d+) tests?$")


def test_targets() -> list[str]:
    """The `[[test]]` names of `core/Cargo.toml`."""
    return TEST_NAME.findall((CORE / "Cargo.toml").read_text())


def mounted(suite: str) -> list[str]:
    """The stems a suite root mounts."""
    try:
        return MOUNT.findall((TESTS / f"{suite}.rs").read_text())
    except OSError:
        return []


def command_for(file: str, pattern: str, extra: list[str]) -> list[str] | None:
    """The cargo command for one test file, or None when nothing builds it."""
    targets = test_targets()
    if file in targets:
        return ["cargo", "test", "--release", "--test", file, "--"] + ([pattern] if pattern else []) + extra
    for suite in targets:
        if not suite.startswith(SUITE_PREFIX):
            continue
        stems = mounted(suite)
        if file not in stems:
            continue
        skips: list[str] = []
        for other in stems:
            if other != file and other.endswith(file):
                skips += ["--skip", f"{other}::"]
        return ["cargo", "test", "--release", "--test", suite, "--", f"{file}::{pattern}"] + skips + extra
    return None


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: run_test_file.py <file> [<filter>] [<libtest argument>...]", file=sys.stderr)
        return 2
    file = sys.argv[1].removeprefix("core/tests/").removesuffix(".rs")
    pattern = sys.argv[2] if len(sys.argv) > 2 else ""
    extra = sys.argv[3:]
    command = command_for(file, pattern, extra) if file else None
    if command is None:
        print(f"run_test_file: no [[test]] entry in core/Cargo.toml names '{file}' and no suite mounts it",
              file=sys.stderr)
        return 2
    print("cd core && " + shlex.join(command), flush=True)

    ran = 0
    seen = False
    with subprocess.Popen(command, cwd=CORE, stdout=subprocess.PIPE, text=True, errors="replace",
                          bufsize=1) as cargo:
        assert cargo.stdout is not None
        for line in cargo.stdout:
            sys.stdout.write(line)
            sys.stdout.flush()
            match = RUNNING.match(line.rstrip("\n"))
            if match:
                seen = True
                ran += int(match.group(1))
        status = cargo.wait()
    if status != 0:
        return status if status > 0 else 128 - status
    if seen and ran == 0:
        print(f"run_test_file: no test ran: {file}, filter '{pattern}'", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
