#!/usr/bin/env python3
"""The #547 spot channels 1c-1 admission drive, on a built Release app.

Every plan term 1c-1 added is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so the result's
window and display preparation are inside the interval). The gates are FIXED
here before the drive runs; a miss is recorded in `receipt.json` and fails,
and the answer to a miss is the arm's own spot constant in
`core/src/spot_typed.rs` (`RGB16_BYTES_PER_PIXEL`, `FLOAT_BYTES_PER_PIXEL`,
`LAB_BYTES_PER_PIXEL`) or, in CMYK, the existing plan that charges the
on-paper plane the overprint fills in place (`cmyk_convert::
FLATTEN_BYTES_PER_PIXEL` for the merge, the kept staged plan's own terms for
the JPEG) — never this bound and never a shared constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. The harness —
`call`, `footprint`, `settle`, `owned`, `digest`, the CMYK TIFF writer and the
16-bit RGB PNG author — is `scripts/cmyk-admission/measure.py`'s, loaded from
its file as `scripts/lab-admission/measure.py` loads it. Fixtures are
6000x4000 (24 MP), written HERE: the 16-bit RGB PNG from
`scripts/host-still-admission/author.swift` and the CMYK drive's 16-bit
Generic CMYK TIFF.

Every setup opens its fixture, makes its mode (`lab16`: `convert_mode lab`;
`float32`: `convert_precision rgba32 structure preserve`), then `select_all`,
`add_spot_channel` "Spot" lab [54.29, 80.81, 69.89] solidity 60 (full
coverage, so no tile of the overprinted image is shared with its source, the
worst case) and `deselect`. The plans, each the reply's own
`working_memory_plan_bytes`:

- `merge-rgb16`: the 16-bit RGB PNG; `merge_spot_channel` "Spot" (the
  flatten's charge plus `RGB16_BYTES_PER_PIXEL`); the reply's `channels` 0.
- `merge-lab16`: the Lab16 setup; the same (plus `LAB_BYTES_PER_PIXEL` and
  the two ProPhoto pipelines).
- `merge-cmyk16`: the 16-bit CMYK TIFF; the same (`cmyk_flatten_scratch_bytes`
  alone: the overprint fills the flatten's own on-paper plane row by row).
- `export-rgb16-png16`: the 16-bit RGB PNG; `export_image` PNG 16 bits,
  profile document (the export plan plus `RGB16_BYTES_PER_PIXEL`); the reply
  carries `spot_channels`.
- `export-float-exr32`: the float32 setup; `export_image` OpenEXR 32 bits (the
  export plan plus `FLOAT_BYTES_PER_PIXEL`); the same.
- `export-cmyk-jpeg`: the 16-bit CMYK TIFF; `export_image` JPEG, profile
  document (the kept-CMYK staged plan, whose on-paper image the overprint
  fills); the same.

1c-2a's five TIFF cases (#547, `tiff_spot`), fixed here before they first
ran. Their fixtures are written by a setup app's own saves (one owned app,
before any case): the 16-bit RGB PNG opened with `precision preserve` and the
16-bit CMYK TIFF, each with the full-coverage spot channel above, saved by
`save_copy tif` (RGB at `bit_depth` 16) as `rgb16-spot.tif` and
`cmyk16-spot.tif`, four colour samples and one spot sample a pixel. The
plans:

- `open-tiff-rgb16`: `open_document` `rgb16-spot.tif` with `precision
  preserve`; plan = the preserving open's one admission (the file's bytes +
  34 B/px + 256 MiB, `precision_codec`'s `scratch_bytes`) + `with_tiff_spots`'
  term (every decoded sample, 4 x 2 B/px, + the kept plane's 2 B/px); the
  document lists one spot channel.
- `open-tiff-cmyk16`: `open_document` `cmyk16-spot.tif`; plan = the CMYK
  open's (pixels x `cmyk_open_bytes_per_pixel` + the file's bytes + 1 MiB,
  `get_precision`'s plans after the open) + `with_tiff_spots`' term (5 x 2
  B/px + 2 B/px); the same.
- `save-tiff-rgb8`: the 16-bit RGB PNG made a byte document (`convert_precision
  rgba8` flatten) before the spot; `save_copy tif`; plan = the reply's
  `working_memory_plan_bytes` (12 B/px + `SpotChannels.tiffSaveScratch`).
- `save-tiff-rgb16`: the `png16` setup; `save_copy tif` `bit_depth` 16; plan
  = the reply's `working_memory_plan_bytes` (the typed export plan with
  `tiff_spot_scratch_bytes` in place of the overprint term).
- `save-tiff-cmyk16`: the `tiff16` setup; `save_copy tif`; plan = the reply's
  `working_memory_plan_bytes` (`cmyk_save_scratch_bytes` + the spot term).

A 1c-2a miss enlarges only the spot term it measured (`with_tiff_spots`'
admit term, `tiff_spot_scratch_bytes`, `cmyk_save_scratch_of(Tiff)`'s spot
term, `SpotChannels.tiffSaveScratch`), never a shared per-pixel constant.

Gate, for every case: plan >= ceil(1.25 x max E) over `--runs` (3) runs, each
beside its baseline twin; a case whose measured E never rises above its
baseline's is a skip, not a pass, and a run with every case skipped fails.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cmyk = load('cmyk_measure', 'scripts/cmyk-admission/measure.py')

LIMIT = 8589934592
SIZE = (6000, 4000)
RED = [54.29, 80.81, 69.89]

# name: (the setup, the measured call)
CASES = {
    'merge-rgb16': ('png16', 'merge'),
    'merge-lab16': ('lab16', 'merge'),
    'merge-cmyk16': ('tiff16', 'merge'),
    'export-rgb16-png16': ('png16', 'export:png:16'),
    'export-float-exr32': ('float32', 'export:exr:32'),
    'export-cmyk-jpeg': ('tiff16', 'export:jpeg:8'),
    # 1c-2a (module doc): the TIFF that keeps the spot channels.
    'open-tiff-rgb16': ('none', 'open:rgb16-spot'),
    'open-tiff-cmyk16': ('none', 'open:cmyk16-spot'),
    'save-tiff-rgb8': ('rgb8', 'save:8'),
    'save-tiff-rgb16': ('png16', 'save:16'),
    'save-tiff-cmyk16': ('tiff16', 'save:cmyk'),
}
MIB = 1 << 20
# `precision_codec`'s preserving-open admission: 34 B/px and DECODE_FIXED.
PRESERVE_BYTES_PER_PIXEL = 34
PRESERVE_FIXED = 256 * MIB


def fixtures(output):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {'png16': folder / 'rgb16.png', 'tiff16': folder / 'cmyk16.tif'}
    if not made['tiff16'].is_file():
        cmyk.cmyk_tiff(made['tiff16'], *SIZE, 16, cmyk.GENERIC.read_bytes())
    if not made['png16'].is_file():
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(cmyk.AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        subprocess.run([str(binary), 'png16', str(SIZE[0]), str(SIZE[1]), str(made['png16'])],
                       check=True, capture_output=True, timeout=600)
    return made


def spot_tiffs(output, executable, made):
    """1c-2a's open fixtures, written by a setup app's own saves (module doc)."""
    for name, source, extra in (('rgb16-spot', 'png16', dict(bit_depth=16)),
                                ('cmyk16-spot', 'tiff16', {})):
        path = output / 'fixtures' / f'{name}.tif'
        made[name] = path
        if path.is_file():
            continue
        with cmyk.owned(output, f'author-{name}', executable) as app:
            arguments = dict(path=str(made[source]))
            if source == 'png16':
                arguments['precision'] = 'preserve'
            ok, opened = cmyk.call(app, 'open_document', **arguments)
            assert ok, opened
            document = opened['document']['id']
            for tool, extra_arguments in (('select_all', {}),
                                          ('add_spot_channel', dict(name='Spot', lab=RED,
                                                                    solidity=60)),
                                          ('deselect', {}),
                                          ('save_copy', dict(path=str(path), format='tif',
                                                             **extra))):
                ok, reply = cmyk.call(app, tool, document_id=document, **extra_arguments)
                assert ok, (tool, reply)
    return made


def measure_open(output, executable, made, label, case, baseline, record):
    """1c-2a's open cases (module doc): the measured call is the open itself."""
    _, measured = CASES[case]
    path = made[measured.split(':')[1]]
    pixels = SIZE[0] * SIZE[1]
    with cmyk.owned(output, label, executable) as app:
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        plan = None
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        else:
            arguments = dict(path=str(path))
            if 'rgb16' in measured:
                arguments['precision'] = 'preserve'
            ok, reply = cmyk.call(app, 'open_document', **arguments)
        cmyk.settle(pid)
        post, peak = cmyk.footprint(pid)
        if ok and not baseline:
            document = reply['document']['id']
            ok_rows, rows = cmyk.call(app, 'list_channels', document_id=document)
            spots = [r for r in rows.get('channels', []) if r.get('spot')] if ok_rows else []
            if len(spots) != 1:
                ok, reply = False, f'no spot channel opened: {rows}'
            elif 'rgb16' in measured:
                plan = (path.stat().st_size + PRESERVE_BYTES_PER_PIXEL * pixels + PRESERVE_FIXED
                        + pixels * 4 * 2 + pixels * 2)
            else:
                plan = cmyk.open_plan(cmyk.plans_of(app, document), path) + pixels * 5 * 2 + pixels * 2
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if not ok:
                record['refusal'] = str(reply)[:800]
            record['plan'] = plan
    return record


def measure_case(output, executable, made, label, case, baseline):
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    if measured.startswith('open:'):
        return measure_open(output, executable, made, label, case, baseline, record)
    with cmyk.owned(output, label, executable) as app:
        def step(document, tool, **arguments):
            ok, reply = cmyk.call(app, tool, document_id=document, **arguments)
            assert ok, (tool, reply)
            return reply

        source = 'tiff16' if setup == 'tiff16' else 'png16'
        arguments = dict(path=str(made[source]))
        if source == 'png16':
            arguments['precision'] = 'preserve'
        ok, opened = cmyk.call(app, 'open_document', **arguments)
        assert ok, opened
        document = opened['document']['id']
        if setup == 'lab16':
            converted = step(document, 'convert_mode', mode='lab')
            assert converted.get('color_mode') == 'lab', converted
        if setup == 'float32':
            step(document, 'convert_precision', mode='rgba32', structure='preserve')
        if setup == 'rgb8':
            step(document, 'convert_precision', mode='rgba8', flatten=True)
        step(document, 'select_all')
        step(document, 'add_spot_channel', name='Spot', lab=RED, solidity=60)
        step(document, 'deselect')
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        plan = None
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif measured == 'merge':
            ok, reply = cmyk.call(app, 'merge_spot_channel', document_id=document, channel='Spot')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('channels') != 0:
                ok, reply = False, f'not merged: {reply}'
        elif measured.startswith('save:'):
            depth = dict(bit_depth=16) if measured == 'save:16' else {}
            ok, reply = cmyk.call(app, 'save_copy', document_id=document,
                                  path=str(output / f'{label}.tif'), format='tif', **depth)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and any('spot channels' in loss for loss in reply.get('flat_save_losses', [])):
                ok, reply = False, f'the spot channels were not kept: {reply}'
        else:
            _, name, bits = measured.split(':')
            ok, reply = cmyk.call(app, 'export_image', document_id=document,
                                  path=str(output / f'{label}.{name}'),
                                  settings=dict(format=name, bit_depth=int(bits)))
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and 'spot_channels' not in reply:
                ok, reply = False, f'no overprint disclosed: {reply}'
        cmyk.settle(pid)
        post, peak = cmyk.footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if not ok:
                record['refusal'] = str(reply)[:800]
            record['plan'] = plan
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ.get('DR_VERIFY_APP',
                                                    'build/Build/Products/Release/Doctored.app')))
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    arguments = parser.parse_args()
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-spot-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(LIMIT)
    app = arguments.app if arguments.app.is_absolute() else ROOT / arguments.app
    executable = (app / 'Contents/MacOS/Doctored' if app.suffix == '.app' else app).resolve()
    made = fixtures(output)
    if any(CASES[case][1].startswith('open:') for case in selected):
        made = spot_tiffs(output, executable, made)
    report = dict(app=str(executable), app_sha256=cmyk.digest(executable),
                  script_sha256=cmyk.digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=LIMIT,
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size)
                            for name, path in made.items()},
                  records=[])
    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                try:
                    record = measure_case(output, executable, made, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)
    table, passed, counted = {}, True, 0
    for case in selected:
        measured = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(measured) and all(r.get('ok') for r in measured)
        plans = [r.get('plan') for r in measured if r.get('plan') is not None]
        plan = min(plans) if plans and len(plans) == len(measured) else None
        max_e = max((r.get('E', 0) for r in measured), default=0)
        baseline_e = max((r.get('E') or 0 for r in baselines), default=0)
        need = math.ceil(1.25 * max_e)
        skipped = ok and plan is not None and max_e <= baseline_e
        row = dict(E=[r.get('E') for r in measured], baseline_E=[r.get('E') for r in baselines],
                   plans=plans, plan=plan, need=need, ratio=(plan / max_e) if plan and max_e else None,
                   skipped=skipped, passed=ok and plan is not None and plan >= need)
        if not skipped:
            counted += 1
            passed &= row['passed']
        table[case] = row
    if selected and not counted:
        passed = False
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        verdict = 'SKIP (no rise)' if row['skipped'] else 'PASS' if row['passed'] else 'FAIL'
        print(f'{case:20} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-spot: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
