#!/usr/bin/env python3
"""Inventory preserved 9I evidence without acquisition, inference or pixel scoring.

Writes a new directory outside the repository. Existing temporary paths may be
missing: that is recorded, never silently regenerated or treated as a pass.
Gate checks recompute decisions from preserved metrics, not pixels.

The /private/tmp layout named below is the historical one this inventory was
written against. Preserved copies now live under
~/devel/doctored-fixtures/phase9i-reports/; every /private/tmp/rz-* receipt not
copied there was purged. Pass the persistent root explicitly when re-running.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
REPORT_PINS = {
    "v3/run.json": "6dec114cebd9b5609d69a8347d0e5b9da3d8e06d908dfb2c23e89c8af19a1376",
    "v3/analytic-crops/metrics.json": "8d8225440cb225d66fa35d18d204393c6a19d415d6df060fcf4ad3478aa99620",
    "v3/full/metrics.json": "e6dac625f260f180140e3ed02b32266869a0d22ac49e4137c27b224d9f8d2540",
    "v4/run.json": "2dbbda8e21b7750db3f536e66a6cc9b35476f45c92a7c711dd6ff5f2cd883c72",
    "v4/analytic-crops/metrics.json": "6333ff168ce8a50fd54236608816c1fb78dbf93e8bd3bb95296a4634977edf82",
    "v4/full/metrics.json": "be60ef2c39d0c40d1e1af8c4712f6e481daf14ae7ad20b17c737daaa3a8b5bca",
}


def sha(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def file_record(path, expected=None):
    row = {"path": str(path.resolve()), "exists": path.is_file()}
    if row["exists"]:
        row.update(bytes=path.stat().st_size, sha256=sha(path))
    if expected is not None:
        row.update(expected_sha256=expected, matches_pin=row.get("sha256") == expected)
    return row


def png_metadata(path):
    """Inspect headers/chunks only; no decoding or profile conversion."""
    chunks = []
    with path.open("rb") as source:
        if source.read(8) != b"\x89PNG\r\n\x1a\n":
            raise ValueError(f"not PNG: {path}")
        shape = None
        while header := source.read(8):
            length, kind = struct.unpack(">I4s", header)
            if kind == b"IHDR":
                shape = list(struct.unpack(">IIBBBBB", source.read(length)))
            elif kind in (b"iCCP", b"sRGB", b"gAMA", b"cHRM", b"cICP"):
                data = source.read(length)
                chunks.append({"type": kind.decode(), "bytes": length,
                               "sha256": hashlib.sha256(data).hexdigest(),
                               "hex": data.hex() if length <= 32 else None})
            else:
                source.seek(length, 1)
            source.read(4)
            if kind == b"IEND":
                break
    return {"ihdr": shape, "color_chunks": chunks}


def gate_check(row):
    alpha = ("refusal" not in row["alpha_candidate"]
             and row["after"]["boundary_mad"] <= row["before"]["boundary_mad"] + 1 / 255
             and row["after"]["soft_detail_recall"] >= .95)
    color_row = row["estimated_alpha_color"]
    before, after = color_row["before"], color_row["after"]
    color = (not color_row["alpha_candidate_refused"]
             and color_row["has_fractional_reference"]
             and after["opaque_changed_pixels"] == 0
             and after["alpha_weighted_foreground_mad"] <= before["alpha_weighted_foreground_mad"] + 1 / 255
             and after["fractional_foreground_mad"] <= .05
             and (before["fractional_foreground_mad"] <= .02
                  or after["fractional_foreground_mad"] <= .8 * before["fractional_foreground_mad"])
             and all(after["recomposite"][b]["boundary_mad"] <= .02 for b in ("white", "dark", "checker")))
    combined = alpha and row["has_fractional_reference"] and color
    return {"case": row["case"], "alpha": alpha, "color": color, "combined": combined,
            "matches_serialized_decisions": (alpha == row["passes_analytic_gate"]
                and color == color_row["passes_color_gate"]
                and combined == row["passes_end_to_end_gate"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--temporary-root", type=Path, default=Path("/private/tmp"))
    args = parser.parse_args()
    output, temp = args.output.resolve(), args.temporary_root.resolve()
    if output.is_relative_to(ROOT) or args.output.is_symlink():
        parser.error("output must be new, outside the repository, and not a symlink")
    output.mkdir(parents=True, exist_ok=False)
    report = {"schema_version": 1, "captured_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "repo_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "scope": "existing evidence inventory; no new candidate, inference, annotation, or pixel scores",
              "inventory_script": file_record(Path(__file__)), "fixture_verification": [],
              "files": [], "baselines": {}, "source_only_exports": [], "missing_or_changed": []}
    for directory, script in [(temp / "rz-phase6-quality/references", "phase6_reference_fixtures.py"),
                              (temp / "rz-phase6-full-references", "phase6_full_reference_fixtures.py")]:
        if not directory.is_dir():
            report["missing_or_changed"].append(str(directory))
            continue
        command = [sys.executable, "-B", str(ROOT / "scripts" / script), str(directory), "--verify-only"]
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        report["fixture_verification"].append({"command": command, "exit": result.returncode,
                                               "stdout": result.stdout, "stderr": result.stderr})
        if result.returncode:
            report["missing_or_changed"].append(str(directory))
        for path in sorted(directory.iterdir()):
            if path.suffix in (".png", ".json", ".js", ".wiki", ".md", ".zip"):
                record = file_record(path)
                if path.suffix == ".png":
                    record["png"] = png_metadata(path)
                report["files"].append(record)
    cases = []
    for version in ("v3", "v4"):
        directory = temp / ("rz-phase9i-soft-prior-" + version)
        baseline = {"reports": [], "gates": [], "case_count": 0}
        rows = []
        for name in ("run.json", "analytic-crops/metrics.json", "full/metrics.json"):
            path = directory / name
            record = file_record(path, REPORT_PINS[version + "/" + name])
            baseline["reports"].append(record)
            if not record["matches_pin"]:
                report["missing_or_changed"].append(str(path))
                continue
            value = json.loads(path.read_text())
            if name == "run.json":
                declaration = value["predeclared"]
                baseline["summaries"] = value["summaries"]
                baseline["original_executable_now"] = file_record(Path(declaration["executable"]), declaration["executable_sha256"])
                baseline["recorded_sources_now"] = [file_record(ROOT / p, s) for p, s in declaration["source_sha256"].items()]
                for changed in [baseline["original_executable_now"], *baseline["recorded_sources_now"]]:
                    if not changed["matches_pin"]:
                        report["missing_or_changed"].append(changed["path"])
            else:
                for corpus in ("analytic", "photographic_recompositions", "full_images"):
                    for row in value.get(corpus, []):
                        rows.append(row)
                        if version == "v3":
                            cases.append({"case": row["case"], "corpus": corpus,
                                          "dimensions": row["dimensions"], "domain": row["domain"],
                                          "reference_classes": row["reference_classes"],
                                          "harness_estimated_bytes": 256 * row["dimensions"][0] * row["dimensions"][1] + 64 * 1024**2})
        baseline["gates"] = [gate_check(row) for row in rows]
        baseline["case_count"] = len(rows)
        baseline["unique_case_count"] = len({row["case"] for row in rows})
        baseline["v4_alpha_before_after_exact"] = all(row["before"] == row["after"] for row in rows) if version == "v4" else None
        report["baselines"][version] = baseline
    for name in ("rz-phase6c-qualified/run.json", "rz-phase6c-qualified/matting/metrics.json",
                 "rz-phase6c-qualified/guided-baseline/metrics.json", "rz-phase6-full-quality/run.json",
                 "rz-phase6-full-quality/images/metrics.json", "rz-closed-form-quality/metrics.json",
                 "rz-closed-form-full/metrics.json", "rz-phase9i-stability-audit/audit.json",
                 "rz-phase9i-continue-parity/parity.json", "rz-phase9i-continue-quality-audit.json",
                 "rz-phase6-vitmatte-author-quality-final/metrics.json",
                 "rz-phase6-vitmatte-author-quality-final/guard-parity.json",
                 "rz-phase6-vitmatte-full-quality/run.json", "rz-phase6-vitmatte-full-quality/metrics.json",
                 "rz-phase6-vitmatte-output/model-report.json", "rz-phase6-vitmatte-author-output/model-report.json",
                 "rz-phase6-vitmatte-full-output/model-report.json", "rz-vitmatte-spike-yytbfsx_/requirements.lock"):
        report["files"].append(file_record(temp / name))
    model = temp / "rz-vitmatte-spike-yytbfsx_/model"
    if model.is_dir():
        for path in sorted(model.iterdir()):
            if path.is_file():
                report["files"].append(file_record(path))
    python = temp / "rz-vitmatte-spike-yytbfsx_/venv/bin/python"
    if python.is_file():
        # This imports only stdlib and repository constants; never Torch/models.
        code = ("import sys,json,platform,importlib.metadata;from pathlib import Path;"
                "sys.path.insert(0,sys.argv[1]);import phase6_vitmatte_spike as v;"
                "v.model_check(Path(sys.argv[2]));"
                "print(json.dumps({'python':platform.python_version(),"
                "'packages':{n:importlib.metadata.version(n) for n in v.VERSIONS},"
                "'pinned_model_and_packages_match':True}))")
        result = subprocess.run([str(python), "-B", "-c", code, str(ROOT / "scripts"), str(model)],
                                text=True, capture_output=True, check=False)
        report["legacy_runtime_verification"] = {"python": file_record(python), "exit": result.returncode,
                                                   "stdout": result.stdout, "stderr": result.stderr}
    for name in ("rz-phase6-vitmatte-inputs", "rz-phase6-vitmatte-full-inputs"):
        directory = temp / name
        record = file_record(directory / "inputs.json")
        if record["exists"]:
            value = json.loads((directory / "inputs.json").read_text())
            record["case_count"] = len(value["cases"])
            record["old_automatic_hard_trimap_policy"] = True
            record["not_a_new_blinded_annotation_context"] = True
        report["source_only_exports"].append(record)
    for record in [*report["files"], *report["source_only_exports"]]:
        if not record["exists"]:
            report["missing_or_changed"].append(record["path"])
    if "legacy_runtime_verification" not in report or report["legacy_runtime_verification"]["exit"]:
        report["missing_or_changed"].append(str(python))
    report["missing_or_changed"] = sorted(set(report["missing_or_changed"]))
    report["preserved_fixture_and_baseline_checks_pass"] = (
        len(report["fixture_verification"]) == 2
        and all(r["exit"] == 0 for r in report["fixture_verification"])
        and all(b["case_count"] == b["unique_case_count"] == 90
                and all(r["matches_pin"] for r in b["reports"])
                and all(g["matches_serialized_decisions"] for g in b["gates"])
                for b in report["baselines"].values()))
    manifest = {"schema_version": 1, "inference_input": False, "role": "scorer-only original90 inventory; contains reference population counts",
                "domain": "encoded RGB8; preserve source PNG metadata separately, no new color conversion",
                "prior_recipe": "original radius3 feather or threshold>=128 binary, generated by preserved Rust fixture code",
                "cases": cases}
    (output / "original90-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    report["original90_manifest"] = file_record(output / "original90-manifest.json")
    (output / "inventory.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"output": str(output), "cases": len(cases),
                      "baseline_case_counts": {v: r["case_count"] for v, r in report["baselines"].items()},
                      "inventory_sha256": sha(output / "inventory.json"),
                      "manifest_sha256": sha(output / "original90-manifest.json")}, indent=2))


if __name__ == "__main__":
    main()
