#!/usr/bin/env python3
"""Check that a status doc was condensed by the rule: original archived unedited, record under the cap.

Standard library only; from `scripts/` it imports `check_docs`. Per NAME it
compares `designs/RECORD` (the record; RECORD is NAME unless `--record` names
the one record every NAME was condensed into, as a status doc and its shipped
companion are), `<archive-dir>/NAME` (the moved original) and `designs/NAME`
at the base revision. The archive index must link the original by its path
relative to `archive/` (`](NAME)` when DIR is `archive` itself):

    python3 scripts/verify_status_condense.py [--base REV] [--archive-dir DIR] [--record RECORD.md] NAME.md [NAME.md ...]

One `ok` or `error:` line per check. Exit 0 when every check holds, 1 otherwise.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

import check_docs

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASE = "HEAD"
DEFAULT_ARCHIVE_DIR = "archive/2026-10"
ARCHIVE_INDEX = "archive/INDEX.md"

HEADING = re.compile(r"^#{2,3} ")
OPEN_ITEMS_PREFIX = "## Open items"
ISSUE_LINK = re.compile(r"issues/(\d+)")


def check_name(name: str, base: str, archive_dir: str, record_basename: str | None = None) -> list[tuple[bool, str]]:
    """(held, message) per check of one doc."""
    outcomes: list[tuple[bool, str]] = []
    original_name = f"designs/{name}"
    record_name = f"designs/{record_basename or name}"
    archived_name = f"{archive_dir.rstrip('/')}/{name}"

    shown = subprocess.run(["git", "show", f"{base}:{original_name}"], cwd=ROOT, capture_output=True)
    if shown.returncode != 0:
        outcomes.append((False, f"{original_name}: cannot read it at {base}: "
                                f"{shown.stderr.decode(errors='replace').strip()}"))
        return outcomes
    original = shown.stdout

    # 1. The archived original is the base's doc, byte for byte.
    archived_path = ROOT / archived_name
    if not archived_path.is_file():
        outcomes.append((False, f"{archived_name}: check 1: the archived original is missing"))
    elif archived_path.read_bytes() != original:
        outcomes.append((False, f"{archived_name}: check 1: differs from {base}:{original_name} "
                                f"({archived_path.stat().st_size} bytes, the original has {len(original)})"))
    else:
        outcomes.append((True, f"{archived_name}: check 1: byte-identical to {base}:{original_name} "
                               f"({len(original)} bytes)"))

    # 2. The record exists, is not the original, and is under the cap.
    record_path = ROOT / record_name
    if not record_path.is_file():
        outcomes.append((False, f"{record_name}: check 2: the record is missing"))
        return outcomes
    record = record_path.read_bytes()
    cap = check_docs.STATUS_DOC_CAP_BYTES
    if record == original:
        outcomes.append((False, f"{record_name}: check 2: the record is the original, unchanged"))
    elif len(record) > cap:
        outcomes.append((False, f"{record_name}: check 2: the record is over the cap ({len(record)} > {cap})"))
    else:
        outcomes.append((True, f"{record_name}: check 2: the record is {len(record)} bytes "
                               f"(the original has {len(original)}, the cap is {cap})"))

    original_lines = original.decode("utf-8", errors="replace").split("\n")
    record_text = record.decode("utf-8", errors="replace")
    record_lines = record_text.split("\n")
    record_set = set(record_lines)

    # 3. Every `##` / `###` heading of the original is a line of the record.
    headings = [(number, line) for number, line in enumerate(original_lines, start=1) if HEADING.match(line)]
    missing = []
    for number, line in headings:
        if line.startswith(OPEN_ITEMS_PREFIX):
            if sum(1 for candidate in record_lines if candidate.startswith(OPEN_ITEMS_PREFIX)) != 1:
                missing.append((number, line))
        elif line not in record_set:
            missing.append((number, line))
    if missing:
        for number, line in missing:
            outcomes.append((False, f"{record_name}: check 3: the heading on line {number} of the original "
                                    f"is not a line of the record: {line[:120]!r}"))
    else:
        outcomes.append((True, f"{record_name}: check 3: all {len(headings)} headings of the original are "
                               f"lines of the record"))

    # 4. Every linked issue of the original is linked from the record.
    wanted = set(ISSUE_LINK.findall(original.decode("utf-8", errors="replace")))
    lost = sorted(wanted - set(ISSUE_LINK.findall(record_text)), key=int)
    if lost:
        outcomes.append((False, f"{record_name}: check 4: the record links no issues/<n> for: "
                                + " ".join("#" + number for number in lost)))
    else:
        outcomes.append((True, f"{record_name}: check 4: all {len(wanted)} linked issues of the original are "
                               f"linked from the record"))

    # 5. The archive index links the moved original by its path relative to `archive/`.
    relative = os.path.relpath(archive_dir.rstrip("/"), "archive")
    entry = f"]({name})" if relative == "." else f"]({Path(relative).as_posix()}/{name})"
    index_path = ROOT / ARCHIVE_INDEX
    if not index_path.is_file():
        outcomes.append((False, f"{ARCHIVE_INDEX}: check 5: the archive index is missing"))
    elif entry not in index_path.read_text(encoding="utf-8", errors="replace"):
        outcomes.append((False, f"{ARCHIVE_INDEX}: check 5: no `{entry}`"))
    else:
        outcomes.append((True, f"{ARCHIVE_INDEX}: check 5: names `{entry}`"))
    return outcomes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", default=DEFAULT_BASE,
                        help=f"revision holding the original docs (default {DEFAULT_BASE})")
    parser.add_argument("--archive-dir", default=DEFAULT_ARCHIVE_DIR,
                        help=f"directory the originals moved to (default {DEFAULT_ARCHIVE_DIR})")
    parser.add_argument("--record", default=None, metavar="RECORD.md",
                        help="basename in designs/ of the record every NAME was condensed into "
                             "(default: each NAME's own basename)")
    parser.add_argument("names", nargs="+", metavar="NAME.md", help="basename of a doc in designs/")
    arguments = parser.parse_args()
    failed = False
    for name in arguments.names:
        for held, message in check_name(name, arguments.base, arguments.archive_dir, arguments.record):
            failed = failed or not held
            print(("ok " if held else "error: ") + message)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
