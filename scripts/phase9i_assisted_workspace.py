#!/usr/bin/env python3
"""Export the source-only Phase 9I assisted annotation workspace.

The workspace an annotator opens contains anonymous case directories with a
composite source PNG and its starting mask, one task card, and nothing else.
The private side — which anonymous id is which case, the salt, the exposure
declaration and every program hash — stays outside it.

Nothing here constructs a source or a prior: the frozen Rust evaluator recipe
does, through core/examples/phase9i_assisted_workspace.rs, so the bytes an
annotator sees and the bytes the later scorer re-derives come from one
implementation. This script names cases, checks what came back and audits the
workspace for anything that could unblind it.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re
import secrets
import struct
import subprocess
import sys
import uuid

sys.dont_write_bytecode = True
import phase9i_gate_v2_rescore as rescore
from verify_phase2 import read_png

ROOT = Path(__file__).resolve().parents[1]
PROTOCOL = "phase9i-assisted-edges-v1"
PROTOCOL_DOC = ROOT / "archive/phase-9i-assisted-protocol-v1.md"
TASK_CARD_DOC = ROOT / "designs/phase-9i-assisted-annotation-task-v1.md"
TEMPLATE = ROOT / "designs/phase-9i-assisted-v1.template.json"
BUDGET = {"seconds": 300, "modifying_gestures": 20,
          "brush_diameters": [3, 9, 27, 81], "max_stroke_points": 256}
PROFILE = "untagged; benchmark encoded RGB8, displayed as sRGB"
# Anything that could identify a case, its reference photograph or the truth
# plane it was derived from. The corpus prefixes are included because an
# anonymous id must not be derivable from a workspace path or field either.
BASE_FORBIDDEN = ("photo-", "full-", "truth", "alpha", "reference")
BACKGROUNDS = ("light", "dark", "checker")
STARTS = ("blurred", "binary")


def sha256(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def parse_case(case_id):
    """`photo-<reference>-<background>-<start>` / `full-<id>-<bg>-<start>`.

    Reference ids contain hyphens, so the fixed fields are read from the ends,
    exactly as the Rust exporter's own parse_case does.
    """
    parts = case_id.split("-")
    if len(parts) < 4 or parts[0] not in ("photo", "full"):
        raise ValueError(f"unrecognized case id {case_id}")
    corpus, background, start = parts[0], parts[-2], parts[-1]
    if background not in BACKGROUNDS or start not in STARTS:
        raise ValueError(f"unrecognized background or start in {case_id}")
    return corpus, "-".join(parts[1:-2]), background, start


def anonymous_ids(cases, salt):
    """A stable, collision-checked anonymous id per case, keyed by one secret
    salt that only the private mapping records."""
    mapping = {case: hashlib.sha256(f"{salt}:{case}".encode()).hexdigest()[:12] for case in cases}
    if len(set(mapping.values())) != len(mapping):
        raise ValueError("anonymous id collision; re-run for a new salt")
    return mapping


def png_header(data):
    """(width, height, depth, color, chunk_kinds) read independently of any
    decoder, so the audit can refuse an unexpected storage or ancillary chunk."""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    offset, kinds, shape = 8, [], None
    while offset < len(data):
        size = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", payload)
            if interlace != 0:
                raise ValueError("interlaced PNG")
            shape = (width, height, depth, color)
        kinds.append(kind.decode("ascii"))
        offset += size + 12
    if shape is None:
        raise ValueError("PNG without IHDR")
    return shape + (kinds,)


def _strings(value):
    if isinstance(value, dict):
        for key, item in value.items():
            yield key
            yield from _strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from _strings(item)
    elif isinstance(value, str):
        yield value


def audit_workspace(workspace, forbidden_tokens):
    """Refuse a workspace that holds anything beyond the declared file set, an
    unexpected PNG storage, or any path or JSON string that could unblind it."""
    manifest = json.loads((workspace / "manifest.json").read_text())
    expected = {Path("manifest.json"), Path("TASK-CARD.md")}
    for entry in manifest["cases"]:
        for name in ("source.png", "prior.png", "case.json"):
            expected.add(Path("cases") / entry["anonymous_id"] / name)
    present = {path.relative_to(workspace) for path in workspace.rglob("*") if path.is_file()}
    if present != expected:
        raise ValueError(f"unexpected workspace files: {sorted(map(str, present ^ expected))}")
    directories = {path.relative_to(workspace) for path in workspace.rglob("*") if path.is_dir()}
    allowed = {Path("cases"), Path("replays")} | {
        Path("cases") / entry["anonymous_id"] for entry in manifest["cases"]}
    if directories != allowed:
        raise ValueError(f"unexpected workspace directories: {sorted(map(str, directories ^ allowed))}")
    if not (workspace / "replays").is_dir():
        raise ValueError("the replays directory is missing")
    tokens = tuple(token.lower() for token in forbidden_tokens)
    for path in sorted(workspace.rglob("*")):
        text = str(path.relative_to(workspace)).lower()
        for token in tokens:
            if token in text:
                raise ValueError(f"workspace path names {token}: {path}")
    for path in sorted(workspace.rglob("*.json")):
        for value in _strings(json.loads(path.read_text())):
            lowered = value.lower()
            for token in tokens:
                if token in lowered:
                    raise ValueError(f"{path} names {token}")
    for entry in manifest["cases"]:
        directory = workspace / "cases" / entry["anonymous_id"]
        source = png_header((directory / "source.png").read_bytes())
        prior = png_header((directory / "prior.png").read_bytes())
        if source[2:4] != (8, 2) or prior[2:4] != (8, 0):
            raise ValueError(f"unexpected PNG storage in {directory}")
        if source[:2] != (entry["width"], entry["height"]) or prior[:2] != source[:2]:
            raise ValueError(f"case dimensions disagree in {directory}")
        for kinds in (source[4], prior[4]):
            if not set(kinds) <= {"IHDR", "IDAT", "IEND"}:
                raise ValueError(f"unexpected PNG chunks in {directory}: {sorted(set(kinds))}")


def checked_output(path):
    """New, outside the repository, and not a symlink — phase6's checked_root
    precedent, applied to an output the annotator will be handed."""
    resolved = Path(path).resolve()
    if resolved == ROOT or ROOT in resolved.parents:
        raise ValueError("the workspace must be stored outside the repository")
    if Path(path).is_symlink() or resolved.exists():
        raise ValueError("the output must be a new directory and not a symlink")
    resolved.mkdir(parents=True)
    return resolved


def run_stage(command, log):
    with log.open("ab") as stream:
        stream.write(f"$ {' '.join(map(str, command))}\n".encode())
        stream.flush()
        result = subprocess.run(list(map(str, command)), cwd=ROOT, stdout=stream,
                                stderr=subprocess.STDOUT, check=False)
    if result.returncode:
        raise RuntimeError(f"stage failed ({result.returncode}): {' '.join(map(str, command))}")


def case_entry(directory, anonymous):
    source_bytes = (directory / "source.png").read_bytes()
    prior_bytes = (directory / "prior.png").read_bytes()
    width, height, pixels = read_png(source_bytes)
    plane = bytes(channel for pixel in pixels for channel in pixel[:3])
    if len(plane) != width * height * 3:
        raise ValueError("decoded source plane length disagrees with its header")
    prior_width, prior_height, prior_pixels = read_png(prior_bytes)
    if (prior_width, prior_height) != (width, height):
        raise ValueError("the prior does not match its source dimensions")
    prior_plane = bytes(pixel[0] for pixel in prior_pixels)
    return {
        "anonymous_id": anonymous,
        "width": width,
        "height": height,
        "profile": PROFILE,
        "source_png_sha256": hashlib.sha256(source_bytes).hexdigest(),
        "source_plane_sha256": hashlib.sha256(plane).hexdigest(),
        "prior_png_sha256": hashlib.sha256(prior_bytes).hexdigest(),
        "prior_plane_sha256": hashlib.sha256(prior_plane).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--crops", type=Path, required=True)
    parser.add_argument("--full", type=Path, required=True)
    parser.add_argument("--annotator-id", required=True)
    parser.add_argument("--exposed", action="append", default=[],
                        help="a subset case the annotator has already inspected")
    parser.add_argument("--cases", action="append", default=[],
                        help="restrict the export to these subset cases")
    args = parser.parse_args()
    subset = sorted(rescore.ASSISTED_COMPLETION_SUBSET)
    if len(subset) != 36:
        raise ValueError(f"the completion subset is {len(subset)} cases, not 36")
    unknown = sorted(set(args.cases + args.exposed) - set(subset))
    if unknown:
        raise ValueError(f"not completion-subset cases: {unknown}")
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,64}", args.annotator_id):
        raise ValueError("annotator id must be a short plain identifier")
    cases = sorted(set(args.cases)) if args.cases else subset

    output = checked_output(args.output)
    workspace, private = output / "workspace", output / "private"
    workspace.mkdir()
    private.mkdir()
    (workspace / "replays").mkdir()
    log = private / "export.log"
    log.touch()
    run_stage([sys.executable, "-B", ROOT / "scripts/phase6_reference_fixtures.py",
               args.crops, "--verify-only"], log)
    run_stage([sys.executable, "-B", ROOT / "scripts/phase6_full_reference_fixtures.py",
               args.full, "--verify-only"], log)

    salt = secrets.token_hex(16)
    anonymous = anonymous_ids(cases, salt)
    jobs = {"schema_version": 1,
            "cases": [{"case": case, "anonymous_id": anonymous[case]} for case in cases]}
    (private / "jobs.json").write_text(json.dumps(jobs, indent=2) + "\n")
    run_stage([args.executable.resolve(), private / "jobs.json", workspace / "cases",
               args.crops, args.full], log)

    entries = sorted((case_entry(workspace / "cases" / anonymous[case], anonymous[case])
                      for case in cases), key=lambda entry: entry["anonymous_id"])
    for entry in entries:
        path = workspace / "cases" / entry["anonymous_id"] / "case.json"
        path.write_text(json.dumps(entry, indent=2, sort_keys=True) + "\n")
    task_card = workspace / "TASK-CARD.md"
    task_card.write_bytes(TASK_CARD_DOC.read_bytes())
    created = datetime.datetime.now(datetime.timezone.utc).isoformat()
    workspace_id = str(uuid.uuid4())
    manifest = {"workspace_schema_version": 1, "protocol_id": PROTOCOL,
                "workspace_id": workspace_id, "created_utc": created,
                "annotator_id": args.annotator_id, "budget": BUDGET,
                "task_card": "TASK-CARD.md", "task_card_sha256": sha256(task_card),
                "cases": entries}
    (workspace / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")

    recipes = list(json.loads(TEMPLATE.read_text())["recipe_source_pins"])
    programs = {"scripts/phase9i_assisted_workspace.py": sha256(Path(__file__).resolve()),
                "scripts/phase9i_gate_v2_rescore.py": sha256(ROOT / "scripts/phase9i_gate_v2_rescore.py"),
                "executable": sha256(args.executable)}
    programs.update({name: sha256(ROOT / name) for name in recipes})
    mapping = {
        "mapping_schema_version": 1, "protocol_id": PROTOCOL, "workspace_id": workspace_id,
        "salt": salt, "repo_head": subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                                  capture_output=True, text=True,
                                                  check=True).stdout.strip(),
        "created_utc": created, "annotator_id": args.annotator_id,
        "subset_override": bool(args.cases),
        "annotator_exposed_declared": sorted(set(args.exposed)),
        "cases": [dict(zip(("case", "corpus", "reference_id", "background", "start"),
                           (case,) + parse_case(case)),
                       anonymous_id=anonymous[case], annotator_exposed=case in set(args.exposed))
                  for case in cases],
        "program_sha256": programs,
        "protocol_sha256": sha256(PROTOCOL_DOC),
        "task_card_sha256": sha256(task_card),
        "reference_manifest_sha256": {
            "crops/references.json": sha256(args.crops / "references.json"),
            "full/full-references.json": sha256(args.full / "full-references.json")},
    }
    (private / "mapping.json").write_text(json.dumps(mapping, indent=2, sort_keys=True) + "\n")

    audit_workspace(workspace, BASE_FORBIDDEN + tuple(cases)
                    + tuple(parse_case(case)[1] for case in cases))
    print(json.dumps({"workspace": str(workspace), "workspace_id": workspace_id,
                      "cases": len(entries), "subset_override": bool(args.cases),
                      "manifest_sha256": sha256(workspace / "manifest.json"),
                      "mapping_sha256": sha256(private / "mapping.json")},
                     indent=2, sort_keys=True), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
