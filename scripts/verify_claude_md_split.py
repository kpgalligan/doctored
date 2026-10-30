"""Check that the nested CLAUDE.md split moved every per-file entry byte for byte.

Standard library only. `core/CLAUDE.md` and `app/CLAUDE.md` keep their rules;
their per-file entries live in `core/MODULES.md` and `app/FILES.md`. This
compares both pairs with the unsplit files at a base revision, on bytes:

    python3 scripts/verify_claude_md_split.py [--base REV] [--head REV]

`--head` defaults to the working tree; a revision is read through `git show`.
Exit 0 when every check holds, 1 otherwise.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASE = "8384ac9"

# An entry is a bullet line, plus the one app line that never had a bullet.
ENTRY_PREFIXES = (b"- ", b"`PersistedWarp.swift` / ")

SIDES = (
    {"name": "core", "rules": "core/CLAUDE.md", "index": "core/MODULES.md",
     "heading": b"## Module map", "limit": 8000},
    {"name": "app", "rules": "app/CLAUDE.md", "index": "app/FILES.md",
     "heading": b"## File map", "limit": 16000},
)


class Failure(Exception):
    """One failed check; the message names the side, the check and the lines."""


def read(path: str, rev: str | None) -> bytes:
    """The bytes of `path` at `rev`, or in the working tree when `rev` is None."""
    if rev is None:
        return (ROOT / path).read_bytes()
    done = subprocess.run(["git", "show", f"{rev}:{path}"], cwd=ROOT,
                          capture_output=True)
    if done.returncode != 0:
        raise Failure(f"cannot read {path} at {rev}: "
                      f"{done.stderr.decode(errors='replace').strip()}")
    return done.stdout


def is_entry(line: bytes) -> bool:
    return line.startswith(ENTRY_PREFIXES)


def clip(line: bytes | None) -> str:
    """The first 120 bytes of a line, printable."""
    return "<no line>" if line is None else repr(line[:120])


def compare(side: str, check: str, got: list[tuple[int, bytes]],
            want: list[tuple[int, bytes]], got_name: str, want_name: str) -> None:
    """Fail at the first position where two numbered line lists differ."""
    for position in range(max(len(got), len(want))):
        mine = got[position] if position < len(got) else None
        theirs = want[position] if position < len(want) else None
        if mine is not None and theirs is not None and mine[1] == theirs[1]:
            continue
        raise Failure(
            f"{side}: {check}: differs at item {position + 1}\n"
            f"  {got_name} line {mine[0] if mine else '-'}: {clip(mine[1] if mine else None)}\n"
            f"  {want_name} line {theirs[0] if theirs else '-'}: {clip(theirs[1] if theirs else None)}")


def check_side(side: dict, base: str, head: str | None) -> None:
    name = side["name"]
    base_name = f"{base}:{side['rules']}"
    base_lines = read(side["rules"], base).split(b"\n")
    base_numbered = list(enumerate(base_lines, start=1))
    base_entries = [(n, line) for n, line in base_numbered if is_entry(line)]

    # 1. The index: a three-line header, an empty line, then the entries.
    check = "check 1 (index)"
    index_bytes = read(side["index"], head)
    if not index_bytes.endswith(b"\n") or index_bytes.endswith(b"\n\n"):
        raise Failure(f"{name}: {check}: {side['index']} does not end in exactly one newline\n"
                      f"  last bytes: {clip(index_bytes[-120:])}")
    index_lines = index_bytes[:-1].split(b"\n")
    for number, line in enumerate(index_lines[:3], start=1):
        if not line or is_entry(line):
            raise Failure(f"{name}: {check}: header line {number} of {side['index']} "
                          f"is empty or an entry\n  {clip(line)}")
    if len(index_lines) < 4 or index_lines[3] != b"":
        raise Failure(f"{name}: {check}: line 4 of {side['index']} is not empty\n"
                      f"  {clip(index_lines[3] if len(index_lines) > 3 else None)}")
    compare(name, check, list(enumerate(index_lines[4:], start=5)), base_entries,
            side["index"], base_name)
    print(f"ok {name} check 1: {side['index']} holds {len(base_entries)} entries "
          f"byte for byte, in order, after a 3-line header")

    # 2. The rule file: the base's other lines, plus one new line.
    check = "check 2 (rules)"
    rule_bytes = read(side["rules"], head)
    rule_numbered = list(enumerate(rule_bytes.split(b"\n"), start=1))
    base_set = set(base_lines)
    added = [(n, line) for n, line in rule_numbered if line and line not in base_set]
    kept = [(n, line) for n, line in rule_numbered if line and line in base_set]
    wanted = [(n, line) for n, line in base_numbered
              if line and not is_entry(line) and line != side["heading"]]
    compare(name, check, kept, wanted, side["rules"], base_name)
    if len(added) != 1:
        shown = "".join(f"\n  {side['rules']} line {n}: {clip(line)}" for n, line in added)
        raise Failure(f"{name}: {check}: {len(added)} lines of {side['rules']} are absent "
                      f"from the base, expected exactly 1{shown}")
    print(f"ok {name} check 2: {side['rules']} keeps {len(kept)} non-blank lines in order; "
          f"1 line is new (line {added[0][0]}): {clip(added[0][1])}")

    # 3. No entry is left in the rule file.
    check = "check 3 (no bullets)"
    for number, line in rule_numbered:
        if line.startswith(b"- "):
            raise Failure(f"{name}: {check}: {side['rules']} line {number} starts with '- '\n"
                          f"  {clip(line)}")
    print(f"ok {name} check 3: no line of {side['rules']} starts with '- ' "
          f"({len(rule_numbered) - 1} lines)")

    # 4. The rule file is small enough to load on every turn.
    check = "check 4 (size)"
    if len(rule_bytes) >= side["limit"]:
        raise Failure(f"{name}: {check}: {side['rules']} is {len(rule_bytes)} bytes, "
                      f"limit {side['limit']}")
    print(f"ok {name} check 4: {side['rules']} is {len(rule_bytes)} bytes "
          f"(limit {side['limit']})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", default=DEFAULT_BASE,
                        help=f"revision holding the unsplit files (default {DEFAULT_BASE})")
    parser.add_argument("--head", default=None,
                        help="revision holding the split files (default: the working tree)")
    arguments = parser.parse_args()
    failed = False
    for side in SIDES:
        try:
            check_side(side, arguments.base, arguments.head)
        except Failure as failure:
            failed = True
            print(f"FAIL {failure}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
