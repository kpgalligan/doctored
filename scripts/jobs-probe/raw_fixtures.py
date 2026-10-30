#!/usr/bin/env python3
"""Build the camera fixtures the jobs-probe's Develop cases open.

    python3 scripts/jobs-probe/raw_fixtures.py <folder>

Creates <folder> and writes three synthetic DNGs into it:

  develop.dng   `phase3_fixtures.synthetic_dng` at its defaults (512 x 384,
                orientation 6): "develop range rows" and "develop output space".
  flat.dng      `verify_v1_raw_subject.flat_dng`: one grey, nothing for Vision
                to find ("develop generated masks").
  portrait.dng  `verify_v1_raw_subject.portrait_dng`: the portrait at
                ~/devel/doctored-fixtures/phase6/photos/portrait.png mosaicked
                as a camera file, read through `sips` ("develop generated
                masks").

Each file is built in its own step. A step that fails prints
`SKIP <file>: <reason>`, removes a partial file and the rest still run; the
exit status is always 0, because only the Develop case whose file is absent
should fail (`ui_develop.swift` names the file and this log). This process puts
`scripts/` first on its own `sys.path`; `verify.py` keeps it off its own. It
launches nothing.
"""

from pathlib import Path
import sys

sys.dont_write_bytecode = True

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def skip(folder, name, error):
    print(f"SKIP {name}: {type(error).__name__}: {error}", flush=True)
    try:
        (folder / name).unlink()
    except OSError:
        pass


def main():
    if len(sys.argv) != 2:
        print("usage: raw_fixtures.py <folder>", file=sys.stderr)
        return 0
    folder = Path(sys.argv[1])
    try:
        folder.mkdir(parents=True, exist_ok=True)
    except Exception as error:  # noqa: BLE001 - every failure is a SKIP line
        for name in ("develop.dng", "flat.dng", "portrait.dng"):
            skip(folder, name, error)
        return 0
    sys.path.insert(0, str(ROOT / "scripts"))

    try:
        import phase3_fixtures
        phase3_fixtures.synthetic_dng(folder / "develop.dng")
        print("BUILT develop.dng", flush=True)
    except Exception as error:  # noqa: BLE001
        skip(folder, "develop.dng", error)

    try:
        import verify_v1_raw_subject as subject
    except Exception as error:  # noqa: BLE001
        skip(folder, "flat.dng", error)
        skip(folder, "portrait.dng", error)
        return 0

    try:
        subject.flat_dng(folder)
        print("BUILT flat.dng", flush=True)
    except Exception as error:  # noqa: BLE001
        skip(folder, "flat.dng", error)

    try:
        subject.portrait_dng(folder)
        print("BUILT portrait.dng", flush=True)
    except Exception as error:  # noqa: BLE001
        skip(folder, "portrait.dng", error)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
