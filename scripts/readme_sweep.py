#!/usr/bin/env python3
"""List the commits a README sweep has to read. The root CLAUDE.md's "README" rule.

Standard library only. README.md's last line is the sweep marker,
`<!-- readme-swept-to: <sha> -->`: README reflects every commit up to and
including that sha. This lists the commits after it, oldest first:

    python3 scripts/readme_sweep.py [--since REV] [--count]

Each commit is one `<short sha>  <subject>` line, then one indented line per
`designs/` doc it changed (and `README.md` when it changed README), and
`(sets the sweep marker)` under the commit that set the current marker.
`--since REV` lists the commits after REV instead of after the marker.
`--count` prints only the number of commits.

Exit 0 on a listing; 2 on a duplicate marker in README, a missing marker
when the marker is the base (no `--since`), or a revision git cannot resolve.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
README = ROOT / "README.md"
MARKER_RE = re.compile(r"<!-- readme-swept-to: ([0-9a-f]{7,40}) -->")
SWEEP_DUE_AT = 20


class GitError(Exception):
    """A git command failed; the message is git's stderr."""


def git(*args: str) -> str:
    """Stdout of `git <args>` run in ROOT; raises GitError on a non-zero exit."""
    result = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    if result.returncode != 0:
        raise GitError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout


def fail(message: str) -> None:
    """Print `error: <message>` to stderr and exit 2."""
    print(f"error: {message}", file=sys.stderr)
    sys.exit(2)


def swept_to(required: bool = True) -> str | None:
    """The marker's sha; None when README has none and `required` is false."""
    matches = MARKER_RE.findall(README.read_text(encoding="utf-8"))
    if len(matches) > 1:
        fail(f"README.md holds {len(matches)} sweep markers; it must hold one")
    if not matches:
        if required:
            fail("README.md holds no sweep marker; pass --since REV")
        return None
    return matches[0]


def commits_since(rev: str) -> list[tuple[str, str]]:
    """(sha, subject) for each commit in `rev..HEAD`, oldest first."""
    out = git("log", "--reverse", "--format=%H%x09%s", f"{rev}..HEAD")
    commits = []
    for line in out.splitlines():
        if line:
            sha, _, subject = line.partition("\t")
            commits.append((sha, subject))
    return commits


def touched(sha: str) -> list[str]:
    """The `designs/` paths commit `sha` changed, plus `README.md` when it changed README."""
    out = git("show", "--format=", "--name-only", sha, "--", "designs", "README.md")
    paths = [path for path in out.splitlines() if path]
    return [p for p in paths if p.startswith("designs/")] + [p for p in paths if p == "README.md"]


def marker_commit(sha: str) -> str | None:
    """The commit that wrote the marker naming `sha` into README, or None."""
    out = git("log", "-1", "--format=%H", f"-Sreadme-swept-to: {sha}", "--", "README.md").strip()
    return out or None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="List the commits since the README sweep marker.")
    parser.add_argument("--since", metavar="REV", help="list the commits after REV instead of the marker")
    parser.add_argument("--count", action="store_true", help="print only the number of commits")
    args = parser.parse_args(argv)

    marker = swept_to(required=args.since is None)
    base = args.since if args.since is not None else marker
    try:
        git("rev-parse", "--verify", "--quiet", f"{base}^{{commit}}")
    except GitError:
        fail(f"git cannot resolve {base!r}")
    try:
        commits = commits_since(base)
        if args.count:
            print(len(commits))
            return 0
        setter = marker_commit(marker) if marker is not None else None
        for sha, subject in commits:
            print(f"{sha[:7]}  {subject}")
            for path in touched(sha):
                print(f"    {path}")
            if setter == sha:
                print("    (sets the sweep marker)")
    except GitError as error:
        fail(str(error))
    print(f"{len(commits)} commits since {base[:7]}; sweep before a push to publish and at {SWEEP_DUE_AT}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
