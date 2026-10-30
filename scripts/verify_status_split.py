#!/usr/bin/env python3
"""Check that a status doc was split into itself and a shipped companion, whole sections moved byte for byte.

Standard library only; from `scripts/` it imports `check_docs`. Per NAME it
compares `designs/NAME` (the status doc) and `designs/NAME` with `-status.md`
replaced by `-shipped.md` (the companion), both in the working tree, against
`designs/NAME` at the base revision, on bytes:

    python3 scripts/verify_status_split.py [--base REV] [--added HEADING ...] NAME-status.md [NAME-status.md ...]

A status-doc section whose heading line equals an `--added` HEADING is new
since the base and is left out of checks 1-3, as the pointer section is.
A section runs from a `##` / `###` heading line to before the next one, its
trailing newlines stripped; the head is the lines before the first section.
One `ok` or `error:` line per check. Exit 0 when every check holds, 1 otherwise.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

import check_docs

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASE = "HEAD"
STATUS_SUFFIX = "-status.md"
SHIPPED_SUFFIX = "-shipped.md"

HEADING = re.compile(rb"^#{2,3} ")
POINTER_HEADING = b"## Shipped runs"
OPEN_ITEMS_LINE = b"## Open items"


def split(data: bytes) -> tuple[list[bytes], list[bytes]]:
    """(head lines, sections) of a doc."""
    head: list[bytes] = []
    sections: list[bytes] = []
    current: list[bytes] | None = None
    for line in data.split(b"\n"):
        if HEADING.match(line):
            if current is not None:
                sections.append(b"\n".join(current).rstrip(b"\n"))
            current = [line]
        elif current is None:
            head.append(line)
        else:
            current.append(line)
    if current is not None:
        sections.append(b"\n".join(current).rstrip(b"\n"))
    return head, sections


def heading_of(section: bytes) -> str:
    """The heading line of a section, for a message."""
    return repr(section.split(b"\n", 1)[0].decode("utf-8", errors="replace")[:120])


def in_base_order(sections: list[bytes], position: dict[bytes, int]) -> bytes | None:
    """The first section that comes before its predecessor in the base, `None` when the order holds."""
    last = -1
    for section in sections:
        index = position.get(section)
        if index is None:
            continue
        if index <= last:
            return section
        last = index
    return None


def check_name(name: str, base: str, added: list[bytes]) -> list[tuple[bool, str]]:
    """(held, message) per check of one doc."""
    outcomes: list[tuple[bool, str]] = []
    status_name = f"designs/{name}"
    if not name.endswith(STATUS_SUFFIX):
        outcomes.append((False, f"{status_name}: the name does not end with {STATUS_SUFFIX}"))
        return outcomes
    companion_file = name[:-len(STATUS_SUFFIX)] + SHIPPED_SUFFIX
    companion_name = f"designs/{companion_file}"

    shown = subprocess.run(["git", "show", f"{base}:{status_name}"], cwd=ROOT, capture_output=True)
    if shown.returncode != 0:
        outcomes.append((False, f"{status_name}: cannot read it at {base}: "
                                f"{shown.stderr.decode(errors='replace').strip()}"))
        return outcomes
    original = shown.stdout

    missing_files = [each for each in (status_name, companion_name) if not (ROOT / each).is_file()]
    if missing_files:
        for each in missing_files:
            outcomes.append((False, f"{each}: the file is missing"))
        return outcomes
    status = (ROOT / status_name).read_bytes()
    companion = (ROOT / companion_name).read_bytes()

    base_head, base_sections = split(original)
    status_head, status_all = split(status)
    companion_head, companion_sections = split(companion)
    status_all = [section for section in status_all if section.split(b"\n", 1)[0] not in added]
    pointers = [index for index, section in enumerate(status_all)
                if section.split(b"\n", 1)[0] == POINTER_HEADING]
    status_sections = [section for section in status_all if section.split(b"\n", 1)[0] != POINTER_HEADING]
    position = {section: index for index, section in enumerate(base_sections)}

    # 1. The status doc's sections plus the companion's are the base's sections, each once.
    wanted = Counter(base_sections)
    found = Counter(status_sections) + Counter(companion_sections)
    faults = []
    for section in base_sections:
        if found[section] == 0:
            faults.append(f"missing {heading_of(section)}")
    for section in status_sections + companion_sections:
        if wanted[section] == 0:
            faults.append(f"extra {heading_of(section)}")
    for section, count in found.items():
        if wanted[section] and count > wanted[section]:
            faults.append(f"repeated {heading_of(section)} ({count} times)")
    if faults or len(wanted) != len(base_sections):
        if len(wanted) != len(base_sections):
            faults.append(f"the base has {len(base_sections) - len(wanted)} repeated sections of its own")
        outcomes.append((False, f"{status_name}: check 1: the sections are not the base's, each once: "
                                + "; ".join(faults)))
    else:
        outcomes.append((True, f"{status_name}: check 1: the {len(base_sections)} sections of {base} are "
                               f"each present once, byte for byte ({len(status_sections)} in the status doc, "
                               f"{len(companion_sections)} in {companion_file})"))

    # 2. Each doc keeps its sections in base order.
    disorder = []
    for label, sections in ((status_name, status_sections), (companion_name, companion_sections)):
        culprit = in_base_order(sections, position)
        if culprit is not None:
            disorder.append(f"{label}: {heading_of(culprit)} comes after a section that follows it in the base")
    if disorder:
        outcomes.append((False, f"{status_name}: check 2: not in base order: " + "; ".join(disorder)))
    else:
        outcomes.append((True, f"{status_name}: check 2: the sections of both docs are in base order"))

    # 3. One pointer section, one line linking the companion, placed where the first moved section was.
    link = b"](" + companion_file.encode("utf-8") + b")"
    moved = [position[section] for section in companion_sections if section in position]
    if len(pointers) != 1:
        outcomes.append((False, f"{status_name}: check 3: {len(pointers)} `{POINTER_HEADING.decode()}` "
                                f"sections, not one"))
    else:
        body = [line for line in status_all[pointers[0]].split(b"\n")[1:] if line]
        before = status_all[:pointers[0]]
        if len(body) != 1:
            outcomes.append((False, f"{status_name}: check 3: the pointer section's body is {len(body)} lines, "
                                    f"not one"))
        elif link not in body[0]:
            outcomes.append((False, f"{status_name}: check 3: the pointer line has no `{link.decode()}`"))
        elif not moved:
            outcomes.append((False, f"{status_name}: check 3: {companion_file} holds no section of the base"))
        elif before != base_sections[:min(moved)]:
            outcomes.append((False, f"{status_name}: check 3: the {len(before)} sections before the pointer are "
                                    f"not the {min(moved)} base sections before the first moved one, "
                                    f"{heading_of(base_sections[min(moved)])}"))
        else:
            outcomes.append((True, f"{status_name}: check 3: one pointer section, one line linking "
                                   f"{companion_file}, after the {len(before)} base sections before the first "
                                   f"moved one"))

    # 4. The status head is the base head less whole lines, each of them a line of the companion head.
    removed = []
    cursor = 0
    for line in base_head:
        if cursor < len(status_head) and status_head[cursor] == line:
            cursor += 1
        else:
            removed.append(line)
    strays = [line for line in removed if line not in companion_head]
    if cursor != len(status_head):
        outcomes.append((False, f"{status_name}: check 4: the head is not the base head with whole lines "
                                f"removed (line {cursor + 1} of it is not from the base head in order)"))
    elif strays:
        outcomes.append((False, f"{status_name}: check 4: {len(strays)} lines removed from the head are not "
                                f"lines of the head of {companion_file}: "
                                + "; ".join(repr(line.decode("utf-8", errors="replace")[:80]) for line in strays)))
    elif not companion_head or not companion_head[0].startswith(b"# "):
        outcomes.append((False, f"{companion_name}: check 4: the head's first line does not start with `# `"))
    else:
        outcomes.append((True, f"{status_name}: check 4: the head is the base head less {len(removed)} lines, "
                               f"each a line of the head of {companion_file}, which opens with a title"))

    # 5. `## Open items` stays in the status doc as often as the base has it (0 or 1), never in the companion.
    in_base = original.split(b"\n").count(OPEN_ITEMS_LINE)
    in_status = status.split(b"\n").count(OPEN_ITEMS_LINE)
    in_companion = companion.split(b"\n").count(OPEN_ITEMS_LINE)
    if in_base > 1 or in_status != in_base or in_companion != 0:
        outcomes.append((False, f"{status_name}: check 5: the line `{OPEN_ITEMS_LINE.decode()}` occurs "
                                f"{in_status} times in the status doc and {in_companion} in {companion_file}, "
                                f"not {in_base} times (as at {base}) and never"))
    else:
        outcomes.append((True, f"{status_name}: check 5: the line `{OPEN_ITEMS_LINE.decode()}` occurs "
                               f"{in_status} times in the status doc, as at {base}, and not in {companion_file}"))

    # 6. Both docs are under their caps.
    status_cap = check_docs.STATUS_DOC_CAP_BYTES
    companion_cap = check_docs.SHIPPED_DOC_CAP_BYTES
    if len(status) > status_cap:
        outcomes.append((False, f"{status_name}: check 6: the status doc is over the cap "
                                f"({len(status)} > {status_cap})"))
    elif len(companion) > companion_cap:
        outcomes.append((False, f"{companion_name}: check 6: the shipped companion is over the cap "
                                f"({len(companion)} > {companion_cap})"))
    else:
        outcomes.append((True, f"{status_name}: check 6: the base has {len(original)} bytes, the status doc "
                               f"{len(status)} (the cap is {status_cap}), {companion_file} {len(companion)} "
                               f"(the cap is {companion_cap})"))
    return outcomes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", default=DEFAULT_BASE,
                        help=f"revision holding the docs before the split (default {DEFAULT_BASE})")
    parser.add_argument("--added", action="append", default=[], metavar="HEADING",
                        help="heading line of a status-doc section new since the base (repeatable)")
    parser.add_argument("names", nargs="+", metavar="NAME-status.md", help="basename of a status doc in designs/")
    arguments = parser.parse_args()
    failed = False
    for name in arguments.names:
        for held, message in check_name(name, arguments.base,
                                     [heading.encode("utf-8") for heading in arguments.added]):
            failed = failed or not held
            print(("ok " if held else "error: ") + message)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
