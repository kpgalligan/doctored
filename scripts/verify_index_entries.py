#!/usr/bin/env python3
"""Check that an index's long entries were cut to one line each, their detail kept, moved or dropped by a ledger.

Standard library only; from `scripts/` it imports `check_docs`. Per SIDE it
compares the index (`core/MODULES.md` or `app/FILES.md`) and the source files
under its directory in the working tree against the base revision, record by
record of the side's ledger (`DIR/core.jsonl` or `DIR/app.jsonl`):

    python3 scripts/verify_index_entries.py --base REV --ledger DIR [--comment-only PATH ...] SIDE [SIDE ...]

A ledger record names a base entry (`line`, `key`), how many lines it became
(`lines`), and the entry's text cut into contiguous pieces, each `kept` (the
new lines say it), `moved` (into the header comment of `to`, worded `as` when
given), `present` (the header of `to`, or a whole `.md` file, already said
`quote` at the base) or `dropped` (with the `reason` the code makes it stale).
For a `.swift` destination the base text searched is every comment line of
the whole file, since Swift keeps file detail in type and member docs. A
`--comment-only` path (repo-relative, repeatable) may change outside the
moved destinations, by comment lines only.
One `ok`, `error:` or `drop` line per check or dropped piece. Exit 0 when every
check holds, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

import check_docs

ROOT = Path(__file__).resolve().parents[1]
SIDES: dict[str, tuple[str, str, str, str]] = {
    "core": ("core/MODULES.md", "core/src", ".rs", "core.jsonl"),
    "app": ("app/FILES.md", "app/Sources", ".swift", "app.jsonl"),
}
SPAN = re.compile(r"`([^`]+)`")
SOURCE_NAME = re.compile(r"^[\w*./+-]+\.(?:rs|swift)$")
SWIFT_IMPORT = re.compile(r"^(?:@\S+\s+)*import\s")


def normalize(text: str) -> str:
    """Whitespace runs collapsed to one space, the ends stripped."""
    return " ".join(text.split())


def header(text: str, suffix: str) -> tuple[list[str], list[str]]:
    """(header lines, rest) of a source file.

    The header is the longest prefix of lines that are blank or `//` comments, or
    `#!` attributes for `.rs`, or `import` / `@… import` lines for `.swift`.
    """
    lines = text.split("\n")
    count = 0
    for line in lines:
        stripped = line.strip()
        if (not stripped or stripped.startswith("//")
                or (suffix == ".rs" and stripped.startswith("#!"))
                or (suffix == ".swift" and SWIFT_IMPORT.match(stripped))):
            count += 1
        else:
            break
    return lines[:count], lines[count:]


def is_comment(line: str) -> bool:
    return line.strip().startswith("//")


def header_text(lines: list[str]) -> str:
    """The comment lines of a header, markers and one space stripped, joined with a space, normalized."""
    texts = []
    for line in lines:
        stripped = line.strip()
        for marker in ("//!", "///", "//"):
            if stripped.startswith(marker):
                body = stripped[len(marker):]
                texts.append(body[1:] if body.startswith(" ") else body)
                break
    return normalize(" ".join(texts))


def git_show(base: str, path: str) -> str | None:
    shown = subprocess.run(["git", "show", f"{base}:{path}"], cwd=ROOT, capture_output=True)
    if shown.returncode != 0:
        return None
    return shown.stdout.decode("utf-8")


def read_ledger(side: str, ledger: Path) -> tuple[list[dict], str | None]:
    path = ledger / SIDES[side][3]
    if not path.is_file():
        return [], f"{path}: the ledger is missing"
    records = []
    for number, line in enumerate(path.read_text(encoding="utf-8").split("\n"), start=1):
        if not line.strip():
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError as error:
            return records, f"{path}:{number}: not JSON: {error}"
    return records, None


def drops(side: str, ledger: Path) -> list[str]:
    """One `drop` line per dropped piece of the side's ledger."""
    records, _ = read_ledger(side, ledger)
    return [f"drop {side}:{record.get('line')} {piece.get('reason', '')}"
            for record in records for piece in record.get("pieces", []) if piece.get("how") == "dropped"]


def check_side(side: str, base: str, ledger: Path, comment_only: list[str]) -> list[tuple[bool, str]]:
    """(held, message) per check of one side."""
    index_name, source_dir, suffix, _ = SIDES[side]
    outcomes: list[tuple[bool, str]] = []

    def report(number: int, faults: list[str], fine: str) -> None:
        if faults:
            for fault in faults:
                outcomes.append((False, f"{side}: check {number}: {fault}"))
        else:
            outcomes.append((True, f"{side}: check {number}: {fine}"))

    base_text = git_show(base, index_name)
    if base_text is None:
        return [(False, f"{side}: cannot read {index_name} at {base}")]
    index_path = ROOT / index_name
    if not index_path.is_file():
        return [(False, f"{side}: {index_name} is missing")]
    new_text = index_path.read_text(encoding="utf-8")
    records, ledger_error = read_ledger(side, ledger)
    if ledger_error:
        return [(False, f"{side}: {ledger_error}")]

    header_count = check_docs.INDEX_HEADER_LINES
    base_lines = base_text.split("\n")
    new_lines = new_text.split("\n")

    # 1. The index header is unchanged.
    report(1, [] if new_lines[:header_count] == base_lines[:header_count]
           else [f"{index_name} lines 1-{header_count} differ from {base}"],
           f"{index_name} lines 1-{header_count} are unchanged from {base}")

    # 2. One record per base entry, in order, with its key.
    base_entries = check_docs.index_entries(base_text)
    faults = []
    if len(records) != len(base_entries):
        faults.append(f"{len(records)} records for {len(base_entries)} base entries")
    for record, (number, key, _) in zip(records, base_entries):
        if record.get("line") != number or record.get("key") != key:
            faults.append(f"record {record.get('line')} `{record.get('key')}` is not base line {number} `{key}`")
    report(2, faults, f"{len(records)} records, one per base entry of {index_name}, in order, keys matching")

    # 3. The new entries are the records' groups, in order.
    body = new_lines[header_count:]
    if body and body[-1] == "":
        body = body[:-1]
    faults = []
    groups: list[list[str]] = []
    cursor = 0
    for record, (number, key, line) in zip(records, base_entries):
        count = record.get("lines")
        if not isinstance(count, int) or count < 1:
            faults.append(f"record {number}: `lines` is {count!r}, not an int >= 1")
            groups.append([])
            continue
        group = body[cursor:cursor + count]
        cursor += count
        groups.append(group)
        if len(group) != count:
            faults.append(f"record {number}: the index ends before its {count} lines")
            continue
        if any(not each.strip() for each in group):
            faults.append(f"record {number}: its group has a blank line")
            continue
        first_key = group[0].split("`", 2)
        if len(first_key) != 3 or first_key[1] != key:
            faults.append(f"record {number}: the group's first line does not have the key `{key}`")
        for each in group[1:]:
            if not each.startswith("- `"):
                faults.append(f"record {number}: a continuation line does not start with ``- ` ``: {each[:60]!r}")
        group_spans = set(SPAN.findall("\n".join(group)))
        entry = line[2:] if line.startswith("- ") else line
        names = [span for span in SPAN.findall(entry.split(" — ", 1)[0]) if SOURCE_NAME.match(span)]
        for name in names:
            if name not in group_spans:
                faults.append(f"record {number}: the name `{name}` is not in its group")
    if cursor < len(body):
        faults.append(f"{len(body) - cursor} lines of {index_name} after the last group")
    report(3, faults, f"{index_name} is the {len(records)} records' groups ({cursor} lines) in order, "
                      f"first keys, continuation lines and file names in place")

    # 4. Every new line is within the limit.
    limit = check_docs.INDEX_ENTRY_MAX_CHARS
    faults = [f"{index_name}:{header_count + offset + 1}: {len(each)} > {limit} characters"
              for offset, each in enumerate(body) if len(each) > limit]
    report(4, faults, f"all {len(body)} entry lines of {index_name} are at most {limit} characters")

    # 5. The pieces are the entry text.
    faults = []
    for record, (number, _, line) in zip(records, base_entries):
        joined = " ".join(piece.get("text", "") for piece in record.get("pieces", []))
        if normalize(joined) != normalize(line[2:] if line.startswith("- ") else line):
            faults.append(f"record {number}: the pieces do not join to the entry text")
    report(5, faults, f"the pieces of each of the {len(records)} records join to its entry text")

    # 6. Kept spans are in the group.
    faults = []
    kept = 0
    for record, group in zip(records, groups):
        group_text = "\n".join(group)
        for piece in record.get("pieces", []):
            if piece.get("how") != "kept":
                continue
            kept += 1
            for span in SPAN.findall(piece.get("text", "")):
                if f"`{span}`" not in group_text:
                    faults.append(f"record {record['line']}: the kept span `{span}` is not in its group")
    report(6, faults, f"the backtick spans of all {kept} kept pieces are in their groups")

    # 7. Moved pieces are in their destination headers once, and were not at the base.
    faults = []
    moved = 0
    targets: set[str] = set()
    work_headers: dict[str, str | None] = {}
    base_headers: dict[str, str | None] = {}

    def text_of(path: str, at_base: bool) -> str | None:
        cache = base_headers if at_base else work_headers
        if path not in cache:
            if at_base:
                content = git_show(base, path)
            else:
                file = ROOT / path
                content = file.read_text(encoding="utf-8") if file.is_file() else None
            if content is None:
                cache[path] = None
            elif path.endswith(".md"):
                cache[path] = normalize(content)
            elif at_base and path.endswith(".swift"):
                cache[path] = header_text(content.split("\n"))
            else:
                cache[path] = header_text(header(content, Path(path).suffix)[0])
        return cache[path]

    for record in records:
        for piece in record.get("pieces", []):
            if piece.get("how") != "moved":
                continue
            moved += 1
            to = piece.get("to", "")
            targets.add(to)
            wanted = normalize(piece.get("as") or piece.get("text", ""))
            if not to.startswith(source_dir + "/") or not to.endswith(suffix):
                faults.append(f"record {record['line']}: `{to}` is not a {suffix} file under {source_dir}")
                continue
            now = text_of(to, False)
            then = text_of(to, True)
            if now is None:
                faults.append(f"record {record['line']}: `{to}` does not exist")
                continue
            if now.count(wanted) != 1:
                faults.append(f"record {record['line']}: the moved text occurs {now.count(wanted)} times in the "
                              f"header of `{to}`, not once: {wanted[:80]!r}")
            elif then is not None and wanted in then:
                faults.append(f"record {record['line']}: the moved text was already in the header of `{to}` at "
                              f"{base}: {wanted[:80]!r}")
    report(7, faults, f"all {moved} moved pieces occur once in the headers of {len(targets)} files, "
                      f"and in none of them at {base}")

    # 8. Present pieces were in their destination at the base.
    faults = []
    present = 0
    for record in records:
        for piece in record.get("pieces", []):
            if piece.get("how") != "present":
                continue
            present += 1
            to = piece.get("to", "")
            quote = normalize(piece.get("quote", ""))
            then = text_of(to, True)
            if len(quote) < 20:
                faults.append(f"record {record['line']}: the quote for `{to}` is under 20 characters")
            elif then is None:
                faults.append(f"record {record['line']}: `{to}` does not exist at {base}")
            elif quote not in then:
                faults.append(f"record {record['line']}: the quote is not in `{to}` at {base}: {quote[:80]!r}")
    report(8, faults, f"all {present} present quotes are in their destinations at {base}")

    # 9. Dropped pieces have a reason.
    faults = []
    dropped = 0
    for record in records:
        for piece in record.get("pieces", []):
            if piece.get("how") == "dropped":
                dropped += 1
                if not str(piece.get("reason", "")).strip():
                    faults.append(f"record {record['line']}: a dropped piece has no reason")
            elif piece.get("how") not in ("kept", "moved", "present"):
                faults.append(f"record {record['line']}: a piece's `how` is {piece.get('how')!r}")
    report(9, faults, f"all {dropped} dropped pieces have a reason")

    # 10. Only moved targets changed, and only by comment lines added to the header;
    # a comment-only path changed by comment lines anywhere.
    listed = subprocess.run(["git", "diff", "--name-only", base, "--", source_dir], cwd=ROOT,
                            capture_output=True, text=True)
    untracked = subprocess.run(["git", "ls-files", "--others", "--exclude-standard", "--", source_dir],
                               cwd=ROOT, capture_output=True, text=True)
    changed = sorted(set(listed.stdout.split()) | set(untracked.stdout.split()))
    faults = []
    for path in changed:
        if path in comment_only:
            old = git_show(base, path)
            file = ROOT / path
            if old is None or not file.is_file():
                faults.append(f"`{path}` was added or deleted")
            elif ([each for each in old.split("\n") if each.strip() and not is_comment(each)]
                  != [each for each in file.read_text(encoding="utf-8").split("\n")
                      if each.strip() and not is_comment(each)]):
                faults.append(f"`{path}`: a non-comment line changed")
            continue
        if path not in targets:
            faults.append(f"`{path}` differs from {base} but is no moved piece's destination")
            continue
        old = git_show(base, path)
        file = ROOT / path
        if old is None or not file.is_file():
            faults.append(f"`{path}` was added or deleted")
            continue
        old_head, old_rest = header(old, suffix)
        new_head, new_rest = header(file.read_text(encoding="utf-8"), suffix)
        if old_rest != new_rest:
            faults.append(f"`{path}`: the code after the header changed")
        if ([each for each in old_head if each.strip() and not is_comment(each)]
                != [each for each in new_head if each.strip() and not is_comment(each)]):
            faults.append(f"`{path}`: the header's non-comment lines changed")
        new_comments = iter([each for each in new_head if is_comment(each)])
        if not all(any(each == candidate for candidate in new_comments)
                   for each in [line for line in old_head if is_comment(line)]):
            faults.append(f"`{path}`: the base header's comment lines are not kept, unchanged and in order")
    report(10, faults, f"the {len(changed)} changed files under {source_dir} are moved destinations, "
                       f"changed only by header comment lines, or comment-only paths changed only by comments")
    return outcomes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", required=True, help="revision holding the index before the cut")
    parser.add_argument("--ledger", required=True, type=Path, help="directory holding core.jsonl / app.jsonl")
    parser.add_argument("--comment-only", action="append", default=[], metavar="PATH",
                        help="repo-relative file that may change by comment lines only (repeatable)")
    parser.add_argument("sides", nargs="+", choices=sorted(SIDES), metavar="SIDE", help="core or app")
    arguments = parser.parse_args()
    failed = False
    for side in arguments.sides:
        for held, message in check_side(side, arguments.base, arguments.ledger, arguments.comment_only):
            failed = failed or not held
            print(("ok " if held else "error: ") + message)
        for line in drops(side, arguments.ledger):
            print(line)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
