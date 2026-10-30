#!/usr/bin/env python3
"""The keyed listing of every Rust test, to compare across a change of test binaries.

`--write PATH` builds the test executables through `run_tests.build()`, lists
each one from `core/` (`--list --format terse`, and again with `--ignored`) and
writes the sorted lines `<key>\t<run|ignored>`. The key is the listed path for
a suite target and `<target name>::<listed path>` for any other, so a test
keeps its key when its file moves from its own binary into a suite.

`--compare BEFORE AFTER [--new-module NAME]...` prints what differs and exits 0
only when the two listings are equal apart from the `after` keys that start
`<NAME>::`.

Standard library only.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

import run_tests

TEST_SUFFIX = ": test"


def listed(executable: str, ignored: bool) -> tuple[int, list[str]]:
    """The executable's exit status and the test paths it lists."""
    command = [executable, "--list", "--format", "terse"] + (["--ignored"] if ignored else [])
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(run_tests.CORE))
    done = subprocess.run(command, cwd=run_tests.CORE, env=env, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, text=True)
    return done.returncode, [line[:-len(TEST_SUFFIX)] for line in done.stdout.splitlines()
                             if line.endswith(TEST_SUFFIX)]


def write(path: Path) -> int:
    status, executables, rendered = run_tests.build()
    if status != 0:
        for text in rendered:
            print(text, end="" if text.endswith("\n") else "\n")
        print(f"cargo build failed: exit {status}", flush=True)
        return 2
    states: dict[str, str] = {}
    for name in sorted(executables):
        prefix = "" if name.startswith(run_tests.SUITE_PREFIX) else name + "::"
        for ignored in (False, True):
            code, paths = listed(executables[name], ignored)
            if code != 0:
                print(f"{name}: the listing exited {code}", flush=True)
                return 2
            seen: set[str] = set()
            for test in paths:
                key = prefix + test
                if key in seen or (not ignored and key in states):
                    print(f"key listed twice: {key}", flush=True)
                    return 2
                seen.add(key)
                if ignored and key not in states:
                    print(f"{name}: ignored test missing from the full listing: {key}", flush=True)
                    return 2
                states[key] = "ignored" if ignored else "run"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(f"{key}\t{states[key]}\n" for key in sorted(states)))
    ignored_count = sum(1 for state in states.values() if state == "ignored")
    print(f"{len(executables)} executables, {len(states)} tests, {ignored_count} ignored: {path}", flush=True)
    return 0


def read(path: Path) -> dict[str, str]:
    states: dict[str, str] = {}
    for line in path.read_text().splitlines():
        if not line:
            continue
        key, _, state = line.rpartition("\t")
        states[key] = state
    return states


def compare(before_path: Path, after_path: Path, new_modules: list[str]) -> int:
    before = read(before_path)
    after = read(after_path)
    prefixes = tuple(name + "::" for name in new_modules)
    only_before = sorted(key for key in before if key not in after)
    only_after = sorted(key for key in after if key not in before and not key.startswith(prefixes))
    differs = sorted(key for key in before if key in after and before[key] != after[key])
    for key in only_before:
        print(f"only in before: {key}")
    for key in only_after:
        print(f"only in after: {key}")
    for key in differs:
        print(f"state differs: {key}: {before[key]} -> {after[key]}")
    new = sum(1 for key in after if key.startswith(prefixes)) if prefixes else 0
    print(f"before {len(before)} tests, after {len(after)} tests, {new} in new modules", flush=True)
    return 1 if only_before or only_after or differs else 0


def main() -> int:
    parser = argparse.ArgumentParser(description="List every Rust test by key, or compare two listings.")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path, metavar="PATH", help="build, list every test executable, write PATH")
    mode.add_argument("--compare", type=Path, nargs=2, metavar=("BEFORE", "AFTER"), help="compare two listings")
    parser.add_argument("--new-module", action="append", default=[], metavar="NAME",
                        help="with --compare: accept the after keys that start NAME::")
    args = parser.parse_args()
    if args.write:
        return write(args.write)
    return compare(args.compare[0], args.compare[1], args.new_module)


if __name__ == "__main__":
    sys.exit(main())
