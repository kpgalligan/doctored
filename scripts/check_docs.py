#!/usr/bin/env python3
"""Check the sizes the root CLAUDE.md's "Status docs" rule sets, and its home-path
rule. A step of `make gate`.

Standard library only. It measures bytes and characters and greps for home paths:

    python3 scripts/check_docs.py

It fails on a status doc (`designs/*-status.md`) over 40 KB, on a shipped
companion (`designs/*-shipped.md`) over 32 KB, on a rule file over its limit,
on an index entry line over 200 characters, and on a text file in the tree
(tracked and untracked, not ignored) that holds an absolute home path (`/Users/`
or `/home/` followed by a user name and a `/`) or its dash-encoded form
(`-Users-<name>-`), named by file and line; such a path is written `~/...` or a
placeholder instead, and no allow-list exists. One line per failure on stdout,
then a summary line. Exit 0 when every check holds, 1 otherwise.
"""

from __future__ import annotations

import re
import subprocess
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

# A home folder's absolute path: `/Users/` or `/home/`, a user name, then `/`, not
# preceded by a path or name character (so a relative `x/Users/` does not count).
# Its dash-encoded form, `-Users-<name>-`, is how Claude Code names a project folder:
# `-Users-` or `-home-`, a user name of word characters, then `-`, likewise not
# preceded by a path or name character. Both are POSIX ERE, for `git grep -E`.
HOME_PATH_PATTERN = r'(^|[^A-Za-z0-9._-])/(Users|home)/[^/[:space:]]+/'
ENCODED_HOME_PATTERN = r'(^|[^A-Za-z0-9._-])-(Users|home)-[A-Za-z0-9_]+-'


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


def home_path_errors(root: Path) -> list[str]:
    """One message per (path, line) of the working tree's text files, tracked and
    untracked, not ignored, that hold an absolute home path or its dash-encoded form
    (the absolute one named when a line holds both). Binary files are skipped."""
    done = subprocess.run(['git', '-C', str(root), 'grep', '--untracked', '-I', '-n', '-o', '-z',
                           '-E', '-e', HOME_PATH_PATTERN, '-e', ENCODED_HOME_PATTERN],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if done.returncode == 1:
        return []
    if done.returncode != 0:
        return [f"error: git grep for home paths failed: "
                f"{done.stderr.decode('utf-8', 'replace').strip()}"]
    found: dict[tuple[str, int], tuple[str, str]] = {}
    for record in done.stdout.split(b'\n'):
        if not record:
            continue
        parts = record.split(b'\0', 2)
        if len(parts) != 3:
            continue
        name, line, match = parts
        path = name.decode('utf-8', 'surrogateescape')
        # The match may start with its boundary character, or with the `/` or `-`
        # itself at column 1; search for the path part rather than slicing it.
        text = match.decode('utf-8', 'replace')
        key = (path, int(line))
        absolute = re.search(r'/(Users|home)/[^/\s]+/', text)
        if absolute:
            if found.get(key, ('a dash-encoded',))[0] == 'a dash-encoded':
                found[key] = ('an absolute', absolute.group(0))
        else:
            encoded = re.search(r'-(Users|home)-[A-Za-z0-9_]+-', text)
            found.setdefault(key, ('a dash-encoded', encoded.group(0) if encoded else text))
    return [f"error: {path}:{line}: holds {kind} home path {home}"
            for (path, line), (kind, home) in sorted(found.items())]


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

    errors.extend(home_path_errors(root))
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
