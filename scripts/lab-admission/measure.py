#!/usr/bin/env python3
"""The #1 Lab Color phase-1, phase-1b, phase-2f, phase-3a, phase-3b, phase-4c, phase-5a and phase-5a-ii admission drive, on a built Release app.

Every new Lab plan is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so the result's
window and display projection, the core's Lab view `lab_convert::lab_display`
included, are inside the interval). The gates are FIXED here before the drive
runs; a miss is recorded in `receipt.json` and fails, and the answer to a miss
is the plan's own per-pixel term (`lab_convert::TO_LAB_BYTES_PER_PIXEL`, or for
New Lab Color `gray_io::OPEN_BYTES_PER_PIXEL`, whose charge it takes; for the
phase-1b exports `lab_rgb::CONVERT_BYTES_PER_PIXEL`, for the Lab TIFF
`lab_save::SAVE_BYTES_PER_PIXEL`, for phase 2f's filters
`lab_channel_filter::PASS_BYTES_PER_PIXEL` or `VIEW_BYTES_PER_PIXEL`, and for
the Lab TIFF open, since phase 3b, the Lab open's own
`lab_limits::OPEN_BYTES_PER_PIXEL`), never this bound and never a shared
constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. The harness —
`call`, `footprint`, `settle`, `owned`, the CMYK TIFF writer — is
`scripts/cmyk-admission/measure.py`'s, and the gray PNG writer
`scripts/gray-admission/measure.py`'s, each loaded from its file. Fixtures are
6000x4000 (24 MP), written HERE: the 16-bit RGB PNG from
`scripts/host-still-admission/author.swift`, the gray drive's 16-bit gray PNG
(a gradient with per-column noise, so no tile is uniform) and the CMYK drive's
16-bit Generic CMYK TIFF.

The plans:

- `new-lab-16`: `new_document color_mode lab depth 16` 6000x4000, white;
  plan = pixels x `OPEN_BYTES_PER_PIXEL` + 1 MiB (`lab_document::new_lab`
  charges `gray_io::open_scratch`, the constant read from
  `core/src/gray_io.rs`).
- `to-lab-16`: the 16-bit RGB PNG at 16 bits; `convert_mode lab`; plan = the
  reply's `working_memory_plan_bytes`.
- `to-lab-8`: the same PNG opened `rgba8`; `convert_mode lab` (promoted, then
  converted at depth 8); the same.
- `to-lab-gray16`: the 16-bit gray PNG (a Grayscale document); `convert_mode
  lab`; the same.
- `to-lab-cmyk16`: the 16-bit CMYK TIFF; `convert_mode lab` (through 16-bit
  ProPhoto RGB in one job); the same.
- `to-rgb-16`: the 16-bit RGB PNG converted to lab in the setup; `convert_mode
  rgb` (Lab16 -> the working RGB); the same.
- `export-lab-png16` (phase 1b): the same Lab16 setup; `export_image` PNG 16
  bits, profile srgb (the Lab -> RGB stage, `lab_rgb`); plan = the reply's
  `working_memory_plan_bytes`.
- `export-lab-jpeg8` (phase 1b): the same, JPEG 8 bits, profile srgb.
- `save-lab-tiff16` (phase 1b): the same setup; `save_copy tiff` (the Lab TIFF,
  `lab_save`); plan = the reply's `working_memory_plan_bytes`.
- `filter-emboss-16`, `filter-edge-detect-16`, `filter-lens-blur-16`,
  `filter-reduce-noise-16`, `filter-smart-sharpen-16` (phase 2f, fixed before
  the run): the same Lab16 setup; plan = `get_precision`'s
  `filter_plans[<op>]` read before the call (the core's
  `lab_channel_filter_plan` with no params on the top Lab raster: pass 0's RGB
  plan + layer x `PASS_BYTES_PER_PIXEL` + canvas x `VIEW_BYTES_PER_PIXEL` + 2
  MiB); the call is `apply_filter <op>` on layer 0 with no params. A miss
  raises `PASS_BYTES_PER_PIXEL` or `VIEW_BYTES_PER_PIXEL`.
- `open-lab-tiff16`, `open-lab-tiff8` (phase 3a, fixed before the run): the
  16-bit RGB PNG opened `preserve` (16) or `rgba8` (8), `convert_mode lab`,
  `save_copy tiff` into the case folder (opaque: three samples); the call is
  `open_document` of that file. Plan = `lab_tiff_read::open_plan` as the core
  computes it: pixels x `LAB_OPEN_BYTES_PER_PIXEL` (since phase 3b the Lab
  open's own `lab_limits::OPEN_BYTES_PER_PIXEL`; phase 3a charged Gray's
  `gray_io::OPEN_BYTES_PER_PIXEL`) + the file's bytes + `BOOKKEEPING_BYTES`
  (both constants read from the source) + pixels x (samples x 2 - 4) + (8
  bits) pixels x samples. A miss raises `lab_limits::OPEN_BYTES_PER_PIXEL`;
  Gray's `gray_io::OPEN_BYTES_PER_PIXEL` is never raised.
- `to-cmyk-lab16` (phase 5a, fixed before the run): the Lab16 setup;
  `convert_mode cmyk` (the working CMYK profile, through the core's 16-bit
  ProPhoto stage, `lab_cmyk`); plan = the reply's `working_memory_plan_bytes`
  (`cmyk_convert::to_cmyk_scratch` + `lab_cmyk::stage_scratch_bytes`), the
  reply's `color_mode` cmyk. Gate: plan >= ceil(1.25 x max E) over 3 runs,
  each beside its baseline twin. A miss raises
  `lab_cmyk::STAGE_BYTES_PER_PIXEL`, never `TO_CMYK_BYTES_PER_PIXEL`.
- `depth-lab-8` (phase 5a, fixed before the run): the Lab16 setup;
  `convert_precision mode lab8` (`lab_depth`); plan = the reply's
  `working_memory_plan_bytes`, the reply `changed` with `bits_per_channel` 8.
  Gate: plan >= ceil(1.25 x max E) over 3 runs, each beside its baseline
  twin. A miss raises `lab_depth::DEPTH_BYTES_PER_PIXEL`, never
  `TO_LAB_BYTES_PER_PIXEL`.
- `to-gray-lab16` (phase 5a-ii, fixed before the run): the Lab16 setup;
  `convert_mode gray` (the working Gray, each pixel's L* through its tone
  curve, `lab_gray`); plan = the reply's `working_memory_plan_bytes`
  (`gray_convert::to_gray_scratch` + `lab_gray::extra_scratch_bytes`), the
  reply's `color_mode` gray. Gate: plan >= ceil(1.25 x max E) over 3 runs,
  each beside its baseline twin. A miss raises
  `lab_gray::EXTRA_BYTES_PER_PIXEL`, never `TO_GRAY_BYTES_PER_PIXEL` or
  `lab_cmyk::STAGE_BYTES_PER_PIXEL`.
- `paste-rgb-into-lab-16`, `paste-lab-into-rgb-16` (phase 4c, fixed before the
  run; gray's `clip:` pattern, each case on its own private pasteboard,
  `DR_PASTEBOARD_NAME`): the setup copies a 6000x4000 window whole
  (`select_all`, `copy` layer 0) — the 16-bit RGB PNG opened `preserve`, or
  for `paste-lab-into-rgb-16` that PNG converted to lab first — and makes the
  target with `new_document` 6000x4000 white at 16 bits, color_mode lab (RGB →
  Lab) or profile srgb (Lab → RGB); the call is `paste` in_place
  (`lab_clipboard::paste`), whose reply's `color_conversion.to` must be Lab /
  RGB. Plan = pixels x `TO_LAB_BYTES_PER_PIXEL` (the conversion's term) +
  `LAYER_PASTE_MARGIN_PER_PIXEL` x pixels + 4 x `BOOKKEEPING_BYTES` (the
  converting paste's insertion term, Gray's); the pipeline's tables are
  ignored (under 1 MiB). Gate: plan >= ceil(1.25 x max E) over 3 runs, each
  beside its baseline twin. A miss raises `lab_convert::TO_LAB_BYTES_PER_PIXEL`
  alone, never `LAYER_PASTE_MARGIN_PER_PIXEL` or `BOOKKEEPING_BYTES`.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gray = load('gray_measure', 'scripts/gray-admission/measure.py')
cmyk = gray.cmyk

LIMIT = 8589934592
SIZE = (6000, 4000)
MIB = 1 << 20
OPEN_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const OPEN_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/gray_io.rs').read_text()).group(1))
LAB_OPEN_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const OPEN_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/lab_limits.rs').read_text()).group(1))
TO_LAB_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const TO_LAB_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/lab_convert.rs').read_text()).group(1))
BOOKKEEPING_BYTES = math.prod(int(v) for v in re.search(
    r'pub\(crate\) const BOOKKEEPING_BYTES: u64 = ([\d *]+);',
    (ROOT / 'core/src/precision_image.rs').read_text()).group(1).split('*'))


def lab_tiff_open_plan(path, bits, samples=3):
    """`lab_tiff_read::open_plan` of a 6000x4000 Lab TIFF at `path` (module doc)."""
    pixels = SIZE[0] * SIZE[1]
    extra = pixels * max(samples * 2 - 4, 0) + (pixels * samples if bits == 8 else 0)
    return pixels * LAB_OPEN_BYTES_PER_PIXEL + path.stat().st_size + BOOKKEEPING_BYTES + extra

# name: (the setup's fixture, the measured call)
CASES = {
    'new-lab-16': ('none', 'new:16'),
    'to-lab-16': ('png16', 'to-lab'),
    'to-lab-8': ('png8', 'to-lab'),
    'to-lab-gray16': ('gpng16', 'to-lab'),
    'to-lab-cmyk16': ('tiff16', 'to-lab'),
    'to-rgb-16': ('lab16', 'to-rgb'),
    'export-lab-png16': ('lab16', 'export:png:16'),
    'export-lab-jpeg8': ('lab16', 'export:jpeg:8'),
    'save-lab-tiff16': ('lab16', 'save-tiff'),
    # Phase 2f: the luma filters in passes (`lab_channel_filter`).
    'filter-emboss-16': ('lab16', 'filter:emboss'),
    'filter-edge-detect-16': ('lab16', 'filter:edge_detect'),
    'filter-lens-blur-16': ('lab16', 'filter:lens_blur'),
    'filter-reduce-noise-16': ('lab16', 'filter:reduce_noise'),
    'filter-smart-sharpen-16': ('lab16', 'filter:smart_sharpen'),
    # Phase 3a: the flat Lab TIFF open (`lab_tiff_read::open_plan`).
    'open-lab-tiff16': ('labtiff16', 'open-lab-tiff'),
    'open-lab-tiff8': ('labtiff8', 'open-lab-tiff'),
    # Phase 5a: Lab -> CMYK through the ProPhoto stage (`lab_cmyk`) and 16 -> 8 Bits/Channel
    # (`lab_depth`).
    'to-cmyk-lab16': ('lab16', 'to-cmyk'),
    'depth-lab-8': ('lab16', 'depth:lab8'),
    # Phase 5a-ii: Lab -> Grayscale directly (`lab_gray`).
    'to-gray-lab16': ('lab16', 'to-gray'),
    # Phase 4c: the converting pastes (`lab_clipboard`).
    'paste-rgb-into-lab-16': ('png16', 'clip:paste-rgb-into-lab'),
    'paste-lab-into-rgb-16': ('png16', 'clip:paste-lab-into-rgb'),
}


def fixtures(output):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {'gpng16': folder / 'gray16.png', 'png16': folder / 'rgb16.png',
            'tiff16': folder / 'cmyk16.tif'}
    if not made['gpng16'].is_file():
        gray.gray_png(made['gpng16'], 16)
    if not made['tiff16'].is_file():
        cmyk.cmyk_tiff(made['tiff16'], *SIZE, 16, cmyk.GENERIC.read_bytes())
    if not made['png16'].is_file():
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(cmyk.AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        subprocess.run([str(binary), 'png16', str(SIZE[0]), str(SIZE[1]), str(made['png16'])],
                       check=True, capture_output=True, timeout=600)
    return made


def measure_clip_case(output, executable, made, label, case, baseline):
    """A phase-4c paste case (module doc): the setup runs on the case's own private
    pasteboard; the plan is the module doc's figure for the 24 MP window."""
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    pixels = SIZE[0] * SIZE[1]
    into_lab = measured == 'clip:paste-rgb-into-lab'
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.lab-admission.{os.getpid()}.{label}'
    try:
        with cmyk.owned(output, label, executable) as app:
            def step(document, tool, **arguments):
                ok, reply = cmyk.call(app, tool, document_id=document, **arguments)
                assert ok, (tool, reply)
                return reply

            ok, opened = cmyk.call(app, 'open_document', path=str(made[setup]),
                                   precision='preserve')
            assert ok, opened
            source = opened['document']['id']
            if not into_lab:
                converted = step(source, 'convert_mode', mode='lab')
                assert converted.get('color_mode') == 'lab', converted
            step(source, 'select_all')
            step(source, 'copy', layer=0)
            target = dict(color_mode='lab') if into_lab else dict(profile='srgb')
            ok, made_target = cmyk.call(app, 'new_document', width=SIZE[0], height=SIZE[1],
                                        depth=16, background='white', **target)
            assert ok, made_target
            document = made_target['document']['id']
            plan = (pixels * TO_LAB_BYTES_PER_PIXEL
                    + gray.LAYER_PASTE_MARGIN_PER_PIXEL * pixels + 4 * BOOKKEEPING_BYTES)
            want = 'Lab' if into_lab else 'RGB'
            pid = app.process.pid
            cmyk.settle(pid)
            pre, _ = cmyk.footprint(pid, reset=True)
            if baseline:
                ok, reply = cmyk.call(app, 'resource_usage')
            else:
                ok, reply = cmyk.call(app, 'paste', document_id=document, placement='in_place')
                if ok and (reply.get('color_conversion') or {}).get('to') != want:
                    ok, reply = False, f'not converted into {want}: {reply}'
            cmyk.settle(pid)
            post, peak = cmyk.footprint(pid)
            record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
            if not baseline:
                if not ok:
                    record['refusal'] = str(reply)[:800]
                record['plan'] = plan
    finally:
        os.environ.pop('DR_PASTEBOARD_NAME', None)
    return record


def measure_case(output, executable, made, label, case, baseline):
    setup, measured = CASES[case]
    if measured.startswith('clip:'):
        return measure_clip_case(output, executable, made, label, case, baseline)
    record = dict(case=case, label=label, baseline=baseline)
    pixels = SIZE[0] * SIZE[1]
    with cmyk.owned(output, label, executable) as app:
        document = None
        lab_tiff = None
        if setup != 'none':
            source = {'png8': 'png16', 'lab16': 'png16', 'labtiff16': 'png16',
                      'labtiff8': 'png16'}.get(setup, setup)
            arguments = dict(path=str(made[source]))
            if source == 'png16':
                arguments['precision'] = 'rgba8' if setup in ('png8', 'labtiff8') else 'preserve'
            ok, opened = cmyk.call(app, 'open_document', **arguments)
            assert ok, opened
            document = opened['document']['id']
            if setup in ('lab16', 'labtiff16', 'labtiff8'):
                ok, converted = cmyk.call(app, 'convert_mode', document_id=document, mode='lab')
                assert ok and converted.get('color_mode') == 'lab', converted
            if setup in ('labtiff16', 'labtiff8'):
                lab_tiff = output / f'{label}-source.tif'
                ok, saved = cmyk.call(app, 'save_copy', document_id=document, path=str(lab_tiff),
                                      format='tiff')
                assert ok and saved.get('format') == 'TIFF', saved
                ok, _ = cmyk.call(app, 'close_document', document_id=document,
                                      discard_changes=True)
                assert ok, 'the setup document did not close'
                document = None
        plan = None
        if measured.startswith('filter:'):
            # The plan is read before the measured interval opens.
            ok, support = cmyk.call(app, 'get_precision', document_id=document)
            assert ok, support
            plan = (support.get('filter_plans') or {}).get(measured.split(':')[1])
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif measured.startswith('new:'):
            ok, reply = cmyk.call(app, 'new_document', width=SIZE[0], height=SIZE[1],
                                  color_mode='lab', depth=int(measured.split(':')[1]),
                                  background='white')
            plan = pixels * OPEN_BYTES_PER_PIXEL + MIB
            if ok and reply['document'].get('color_mode') != 'lab':
                ok, reply = False, f'made {reply["document"].get("color_mode")}'
        elif measured.startswith('export:'):
            _, name, bits = measured.split(':')
            ok, reply = cmyk.call(app, 'export_image', document_id=document,
                                  path=str(output / f'{label}.{name}'),
                                  settings=dict(format=name, bit_depth=int(bits), profile='srgb'))
            plan = reply.get('working_memory_plan_bytes') if ok else None
        elif measured.startswith('filter:'):
            ok, reply = cmyk.call(app, 'apply_filter', document_id=document, layer=0,
                                  filter=measured.split(':')[1])
            if not ok:
                plan = None
        elif measured == 'open-lab-tiff':
            bits = 8 if setup == 'labtiff8' else 16
            ok, reply = cmyk.call(app, 'open_document', path=str(lab_tiff))
            plan = lab_tiff_open_plan(lab_tiff, bits)
            if ok and reply['document'].get('color_mode') != 'lab':
                ok, reply = False, f'opened {reply["document"].get("color_mode")}'
        elif measured == 'save-tiff':
            ok, reply = cmyk.call(app, 'save_copy', document_id=document,
                                  path=str(output / f'{label}.tif'), format='tiff')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('format') != 'TIFF':
                ok = False
        elif measured.startswith('depth:'):
            target = measured.split(':')[1]
            ok, reply = cmyk.call(app, 'convert_precision', document_id=document, mode=target)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and (reply.get('changed') is not True
                       or reply.get('bits_per_channel') != int(target[3:])):
                ok = False
        elif measured in ('to-cmyk', 'to-gray'):
            mode = measured.split('-')[1]
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode=mode)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != mode:
                ok = False
        else:
            mode = 'lab' if measured == 'to-lab' else 'rgb'
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode=mode)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != mode:
                ok = False
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-lab-admission-')))
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
    report = dict(app=str(executable), app_sha256=cmyk.digest(executable),
                  script_sha256=cmyk.digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=LIMIT, open_bytes_per_pixel=OPEN_BYTES_PER_PIXEL,
                  lab_open_bytes_per_pixel=LAB_OPEN_BYTES_PER_PIXEL,
                  to_lab_bytes_per_pixel=TO_LAB_BYTES_PER_PIXEL,
                  bookkeeping_bytes=BOOKKEEPING_BYTES,
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
        print(f'{case:16} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-lab: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
