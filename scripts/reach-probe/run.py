#!/usr/bin/env python3
"""Auto-Align's registration reach and false-accept table.

Builds `main.swift` with the app sources (-O, whole module, release core)
through `scripts/probe_build.py`, which caches the linked probe under
`build/probe-cache`; runs it once on the committed photo
(`core/tests/fixtures/registration_photo/sky.jpg`) and writes
`<output>/reach.json` (every row) and `<output>/reach.md` (the tables). The
rows, the geometry and the bounds are fixed in `main.swift`'s header.

`--before <reach.json>` checks the pass rule against that earlier run (the
HEAD baseline) and exits 1 when it fails. The rule is the controller's
ruling 2 (its pass rule is ruling 1's):
  - zero false accepts, with Geometric Distortion off and on;
  - no record is a WRONG ACCEPT (an answer past its bound), including every
    row that was one before;
  - every plain row that registered before still registers within its bound;
  - a lens row (its GD OFF record `<id>` and GD ON record `<id>-gd`, see
    main.swift's header) passes when its GD OFF record is refused with the
    lens sentence (`AutoAlign.lensDistortionReason`) or its GD ON record
    registers within its bound (MUST 8.0, FLOOR floor + 3.0); and, when it
    registered before, its GD ON record registers within its bound.
A plain row refused before may stay refused (P-collage-15's similarity
reduction).

Ruling 4 named two exceptions, RESIDUALS and NO ESTIMATE; both are empty
now, so every row is judged by the standard rule:
  - B-20-15-lens and B-30-15-lens (250 x 167 and 382 x 255 at 15 % overlap)
    gave the feature step no estimate. A frame under 512 px is now detected on
    its 2x upsampled level first, so they answer and pass the lens rule.
  - B-20-30-lens (306 x 204, a lens departure of 0.70 px at its inliers) was a
    named WRONG ACCEPT, 8.76 px with Geometric Distortion off. On the
    upsampled level its inliers reach toward the frame's edge and depart
    1.42 px, so it refuses with the lens sentence.
The structures stay, empty, for a later named exception.

Run `cd core && cargo build --release` first. Point --output outside the
repository.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
LENS_REASON = "the frames carry lens distortion; turn on Geometric Distortion"
# Named residuals: row id -> the pinned worst error, px (docstring; none now).
RESIDUALS = {}
# Rows the feature step gives no estimate (docstring; none now).
NO_ESTIMATE = set()
PHOTO = ROOT / "core/tests/fixtures/registration_photo/sky.jpg"

_isolation_spec = importlib.util.spec_from_file_location(
    "verification_isolation", ROOT / "scripts/verification_isolation.py")
_isolation = importlib.util.module_from_spec(_isolation_spec)
_isolation_spec.loader.exec_module(_isolation)
_build_spec = importlib.util.spec_from_file_location("probe_build", ROOT / "scripts/probe_build.py")
probe_build = importlib.util.module_from_spec(_build_spec)
_build_spec.loader.exec_module(probe_build)

def environment(output):
    return dict(os.environ, DR_BACKGROUND="1", DR_AUTOMATION_ALERTS="record", ANTHROPIC_API_KEY="dummy",
                CLANG_MODULE_CACHE_PATH=str(output / "clang-cache"),
                DR_ACTIONS_DIR=str(output / "isolation/actions"),
                DR_RECOVERY_DIR=str(output / "isolation/recovery"),
                **_isolation.preset_environment(output / "isolation"))


def build(output, env):
    library = ROOT / "core/target/release"
    if not (library / "libdoctored_core.a").is_file():
        raise SystemExit("Missing the release core; run `cd core && cargo build --release` first.")
    binary = output / "reach-probe"
    (output / "build.log").write_text("")
    probe_build.build(app_sources=probe_build.app_sources(), sources=probe_build.probe_sources("reach-probe"),
                      bridging_header=ROOT / "app/Bridging/Doctored-Bridging-Header.h",
                      library=library / "libdoctored_core.a", optimization="-O", output=binary,
                      log=output / "build.log", env=env, timeout=1800)
    return binary


def run(binary, env, output):
    with (output / "probe.log").open("w") as log:
        completed = subprocess.run([str(binary), str(PHOTO)], env=env, stdout=subprocess.PIPE,
                                   stderr=log, text=True, timeout=7200)
    (output / "probe.out").write_text(completed.stdout)
    rows, header, done = [], None, False
    for line in completed.stdout.splitlines():
        if not line.startswith("{"):
            continue
        record = json.loads(line)
        if "error" in record and "id" not in record:
            raise SystemExit(f"probe error: {record['error']}")
        if "photo" in record:
            header = record
        elif record.get("done"):
            done = True
        else:
            rows.append(record)
    if completed.returncode != 0 or not done:
        raise SystemExit(f"probe exited {completed.returncode} (done: {done}); see {output / 'probe.log'}")
    return header, rows


def fmt(value, digits=2):
    if value is None:
        return "—"
    if isinstance(value, float):
        return "inf" if value < 0 else f"{value:.{digits}f}"
    return str(value)


def cell(row):
    if row is None:
        return "—"
    if row["verdict"] == "refused":
        return "refused (lens)" if row.get("reason") == LENS_REASON else "refused"
    if row["class"] == "false":
        return f"FALSE ACCEPT ({row['answer']})"
    error = fmt(row.get("error"))
    if row["verdict"] == "wrong":
        return f"WRONG {error} ({row['answer']})"
    return f"{error} ({row['answer']})"


def feature_cell(row):
    """The diagnostic: the feature step alone, unvouched (main.swift's header)."""
    if "feature_usable" not in row:
        return "none"
    usable = "" if row["feature_usable"] else " unusable"
    gate = "" if row.get("feature_gate", True) else " gated out"
    if row["class"] == "false":
        return "answered" + usable + gate
    return fmt(row.get("feature_error")) + usable + gate


def judge(rows, before):
    """The pass rule's failures, one sentence each."""
    failures = []
    earlier = {row["id"]: row for row in before} if before is not None else {}
    by_id = {row["id"]: row for row in rows}
    for row in rows:
        verdict = row["verdict"]
        if row["class"] == "false":
            if verdict != "refused":
                failures.append(f"{row['id']}: false accept ({row['answer']})")
            continue
        pin = RESIDUALS.get(row["id"])
        if verdict == "wrong" and pin is not None and row.get("error") is not None \
                and row["error"] <= pin:
            pass  # the named residual, within its pin
        elif verdict == "wrong":
            failures.append(f"{row['id']}: wrong accept ({fmt(row.get('error'))} > bound "
                            f"{fmt(row['bound'] if pin is None else pin)})")
        old = earlier.get(row["id"])
        if (row["class"] == "plain" and old is not None and old["verdict"] == "registered"
                and verdict != "registered"):
            failures.append(f"{row['id']}: registered before, {verdict} now")
        if row["lens"] and not row.get("distortion"):
            on = by_id.get(row["id"] + "-gd")
            if on is None:
                failures.append(f"{row['id']}: no GD ON record")
                continue
            if row["id"] in NO_ESTIMATE:
                if verdict != "refused" or on["verdict"] != "refused":
                    failures.append(f"{row['id']}: no-estimate row answered ({verdict}, GD on "
                                    f"{on['verdict']})")
            elif row["id"] in RESIDUALS:
                if on["verdict"] != "registered":
                    failures.append(f"{row['id']}: residual row {on['verdict']} with GD on")
            elif not (row.get("reason") == LENS_REASON or on["verdict"] == "registered"):
                failures.append(f"{row['id']}: neither refused for the lens with GD off "
                                f"({verdict}) nor registered with GD on ({on['verdict']})")
            if old is not None and old["verdict"] == "registered" and on["verdict"] != "registered":
                failures.append(f"{row['id']}: registered before, {on['verdict']} with GD on")
    if before is not None:
        missing = set(earlier) - {row["id"] for row in rows}
        failures += [f"{name}: missing from this run" for name in sorted(missing)]
    return failures


def markdown(header, rows, before, failures):
    earlier = {row["id"]: row for row in before} if before is not None else {}
    lines = [f"# Auto-Align registration reach", "",
             f"Photo: `{Path(header['photo']).name}` {header['width']} × {header['height']}. "
             "Error = worst grid distance from the truth, px. Bounds: plain 2.0; lens MUST 8.0; "
             "lens FLOOR floor + 3.0 (or refused).", ""]
    columns = ["row", "size", "registration", "class", "floor", "bound"]
    columns += ["before", "after"] if before is not None else ["result"]
    columns += ["feature alone", "lens test departure px", "GD ON registration → after fit (k)"]
    lines += ["| " + " | ".join(columns) + " |", "|" + "---|" * len(columns)]
    by_id = {row["id"]: row for row in rows}
    for row in rows:
        if row.get("distortion") and row["class"] != "false":
            continue  # shown in its GD OFF row's last column
        values = [row["id"], f"{row['width']}×{row['height']}", row["registration"], row["class"],
                  fmt(row.get("floor")), fmt(row.get("bound"))]
        if before is not None:
            values += [cell(earlier.get(row["id"])), cell(row)]
        else:
            values += [cell(row)]
        values += [feature_cell(row)]
        if "feature_departure" in row:
            values += [fmt(row["feature_departure"])]
        else:
            values += ["—"]
        on = by_id.get(row["id"] + "-gd") if row["class"] != "false" else None
        if on is None:
            values += ["—"]
        elif "registration_error" in on:
            fitted = cell(on) + (f" (k {on['coefficient']:.4f})" if "coefficient" in on else "")
            values += [f"{fmt(on['registration_error'])} → {fitted}"]
        else:
            values += [cell(on)]
        lines.append("| " + " | ".join(values) + " |")
    reach = [row for row in rows if row["class"] != "false" and not row.get("distortion")]
    gd = [row for row in rows if row["class"] != "false" and row.get("distortion")]
    false_rows = [row for row in rows if row["class"] == "false"]
    lines += ["", f"Reach rows (GD OFF) registered within bound: "
              f"{sum(row['verdict'] == 'registered' for row in reach)} / {len(reach)}; "
              f"refused {sum(row['verdict'] == 'refused' for row in reach)} "
              f"(lens sentence {sum(row.get('reason') == LENS_REASON for row in reach)}); "
              f"wrong {sum(row['verdict'] == 'wrong' for row in reach)}.",
              f"Lens rows with GD ON registered within bound: "
              f"{sum(row['verdict'] == 'registered' for row in gd)} / {len(gd)}; "
              f"refused {sum(row['verdict'] == 'refused' for row in gd)}; "
              f"wrong {sum(row['verdict'] == 'wrong' for row in gd)}.",
              f"False accepts: {sum(row['verdict'] != 'refused' for row in false_rows)} / {len(false_rows)} "
              f"(GD OFF and GD ON)."]
    by_id = {row["id"]: row for row in rows}
    lines += ["", "Named by ruling 4:"]
    lines += [f"- residual {name}: GD off {cell(by_id[name])} (pin {fmt(pin)}), GD on "
              f"{cell(by_id[name + '-gd'])}" for name, pin in RESIDUALS.items() if name in by_id]
    lines += [f"- no estimate {name}: GD off {cell(by_id[name])}, GD on {cell(by_id[name + '-gd'])}"
              for name in sorted(NO_ESTIMATE) if name in by_id]
    if before is not None:
        lines += ["", "Pass rule: " + ("PASS" if not failures else "FAIL")]
        lines += [f"- {failure}" for failure in failures]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--before", type=Path)
    arguments = parser.parse_args()
    output = arguments.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = environment(output)
    binary = build(output, env)
    header, rows = run(binary, env, output)
    header["sha256"] = hashlib.sha256(PHOTO.read_bytes()).hexdigest()
    before = None
    if arguments.before is not None:
        before = json.loads(arguments.before.read_text())["rows"]
    failures = judge(rows, before) if before is not None else []
    (output / "reach.json").write_text(json.dumps({"photo": header, "rows": rows}, indent=1) + "\n")
    (output / "reach.md").write_text(markdown(header, rows, before, failures))
    print((output / "reach.md").read_text())
    if before is not None and failures:
        print("reach: FAIL")
        return 1
    print("reach: PASS" if before is not None else "reach: written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
