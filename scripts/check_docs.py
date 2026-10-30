#!/usr/bin/env python3
"""Check the sizes the root CLAUDE.md's "Status docs" rule sets. A step of `make gate`.

Standard library only. It measures bytes and characters, nothing else:

    python3 scripts/check_docs.py

It fails on a status doc (`designs/*-status.md`) over 40 KB, on a shipped
companion (`designs/*-shipped.md`) over 32 KB, on a rule file over its limit,
and on an index entry line over 200 characters. One line per failure on
stdout, then a summary line. Exit 0 when every check holds, 1 otherwise.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

STATUS_DOC_GLOB = "designs/*-status.md"
STATUS_DOC_CAP_BYTES = 40 * 1024
SHIPPED_DOC_GLOB = "designs/*-shipped.md"
SHIPPED_DOC_CAP_BYTES = 32 * 1024
RULE_FILE_LIMITS: dict[str, int] = {"core/CLAUDE.md": 8000, "app/CLAUDE.md": 16000}
INDEX_FILES: tuple[str, ...] = ("core/MODULES.md", "app/FILES.md")
INDEX_HEADER_LINES = 4            # three header lines and one blank
INDEX_ENTRY_MAX_CHARS = 200


def index_entries(text: str) -> list[tuple[int, str, str]]:
    """(1-based line, key, line) per non-blank line after the header.

    The key is the text inside the first backtick pair of the line, `""` when there is none.
    """
    entries = []
    for number, line in enumerate(text.split("\n"), start=1):
        if number <= INDEX_HEADER_LINES or not line.strip():
            continue
        parts = line.split("`", 2)
        entries.append((number, parts[1] if len(parts) == 3 else "", line))
    return entries


def check(root: Path) -> list[str]:
    """One message per failure, each starting `error: `."""
    errors = []

    for path in sorted(root.glob(STATUS_DOC_GLOB)):
        name = path.relative_to(root).as_posix()
        size = path.stat().st_size
        if size > STATUS_DOC_CAP_BYTES:
            errors.append(f"error: {name}: the status doc is over the cap ({size} > {STATUS_DOC_CAP_BYTES})")

    for path in sorted(root.glob(SHIPPED_DOC_GLOB)):
        name = path.relative_to(root).as_posix()
        size = path.stat().st_size
        if size > SHIPPED_DOC_CAP_BYTES:
            errors.append(f"error: {name}: the shipped companion is over the cap "
                          f"({size} > {SHIPPED_DOC_CAP_BYTES})")

    for name, limit in RULE_FILE_LIMITS.items():
        path = root / name
        if not path.is_file():
            errors.append(f"error: {name}: the rule file is missing")
        elif path.stat().st_size > limit:
            errors.append(f"error: {name}: the rule file is over its limit ({path.stat().st_size} > {limit})")

    for name in INDEX_FILES:
        path = root / name
        if not path.is_file():
            errors.append(f"error: {name}: the index file is missing")
            continue
        for number, key, line in index_entries(path.read_text(encoding="utf-8")):
            if len(line) > INDEX_ENTRY_MAX_CHARS:
                errors.append(f"error: {name}:{number}: the entry `{key}` is over the limit "
                              f"({len(line)} > {INDEX_ENTRY_MAX_CHARS})")
    return errors


def main() -> int:
    errors = check(ROOT)
    for message in errors:
        print(message)
    if errors:
        print(f"check_docs: FAIL ({len(errors)} errors)")
        return 1
    print(f"check_docs: ok ({len(list(ROOT.glob(STATUS_DOC_GLOB)))} status docs, "
          f"{len(list(ROOT.glob(SHIPPED_DOC_GLOB)))} shipped companions)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
