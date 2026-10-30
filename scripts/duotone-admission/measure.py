#!/usr/bin/env python3
"""The #1 Duotone admission drive (phases 1a, 1b and the close), on a built Release app.

Every new Duotone plan is held to the measured-admission rule: plan >= ceil(1.25 x max E), where
E is `ri_interval_max_phys_footprint` - pre around ONE MCP call, read once the footprint has been
still for 2 s (so the result's display preparation — the Duotone view and its preview — is inside
the interval). The gates are FIXED here before the drive runs; a miss is recorded in
`receipt.json` and fails, and the answer to a miss is the new term that charges it
(`duotone_document::TO_DUOTONE_BYTES_PER_PIXEL`, or the view's `VIEW_BYTES_PER_PIXEL`), never this
bound and never a shared constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES` 8 GiB with
`MallocLargeCache=0`, beside a baseline twin (the same setup, then `resource_usage` instead of the
measured call), `--runs` times. The harness — `call`, `footprint`, `settle`, `owned` — is
`scripts/cmyk-admission/measure.py`'s, loaded through `scripts/gray-admission/measure.py`, whose
gray row this file reuses. The fixture is a 6000x4000 (24 MP) 16-bit sGray PNG (an `sRGB` chunk),
written HERE.

The plans, each the reply's `working_memory_plan_bytes` (the core's: canvas x
`TO_DUOTONE_BYTES_PER_PIXEL` + `BOOKKEEPING_BYTES`):

- `to-duotone-gray16`: the 16-bit sGray document; `convert_mode duotone` with `TWO` (every sample
  shared, then the result's display preparation through the view); the reply's color_mode
  duotone.
- `set-duotone`: the same document converted with `TWO` in the setup; `set_duotone` with `THREE`
  (a new description over the same samples, then the display preparation); changed true.

The close (`duotone_gray`), each on a document converted with `TWO` in the setup; the plan is the
reply's `working_memory_plan_bytes`:

- `to-gray-from-duotone`: `convert_mode gray` (each raster through the inks' luminance: the raster
  pixels x `duotone_convert::RENDER_BYTES_PER_PIXEL`, the Grayscale table
  `duotone_gray::GRAY_TABLE_BYTES`, then the result's display preparation, canvas x
  `TO_DUOTONE_BYTES_PER_PIXEL`, and bookkeeping); color_mode gray.
- `to-bitmap-from-duotone8`: on the 8-bit fixture, `convert_mode bitmap` (the default Diffusion
  Dither; the Grayscale stage without the display term, beside Gray → Bitmap's own plan);
  color_mode bitmap.

Phase 1b (#1 Duotone phase 1b), each on the same document converted with `TWO` in the setup
(`to-indexed-from-duotone8` on a 24 MP 8-bit sGray PNG written the same way, since Indexed Color
converts 8-bit documents); the plan is the reply's `working_memory_plan_bytes` (the core's render
plan, `duotone_convert::RENDER_BYTES_PER_PIXEL` a raster pixel and the view's table, beside the
route's own plan; an export's is the export plan with the view's scratch):

- `to-rgb-from-duotone`: `convert_mode rgb`; color_mode rgb.
- `to-cmyk-from-duotone`: `convert_mode cmyk` (the working CMYK); color_mode cmyk.
- `to-lab-from-duotone`: `convert_mode lab`; color_mode lab.
- `to-indexed-from-duotone8`: `convert_mode indexed` (the default Uniform palette); color_mode
  indexed.
- `export-duotone-png16`: `export_image` png, 16 bits, profile document; ok.
- `export-duotone-psd16`: `export_psd` (layers, PSD); ok; plan = the reply's
  `working_memory_plan_bytes` (`PSDExportCapture.scratchEstimate`, one more copy of the file for
  the Duotone carry).
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import struct
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gray = load('gray_measure', 'scripts/gray-admission/measure.py')
cmyk = gray.cmyk

LIMIT = 8589934592
SIZE = gray.SIZE
LINE = lambda low, high: [low] + [None] * 11 + [high]
TWO = [{'name': 'Warm Black', 'lab': [20, 2, 4], 'curve': LINE(0, 100)},
       {'name': 'Rust', 'lab': [55, 45, 55], 'curve': LINE(10, 70)}]
THREE = TWO + [{'name': 'Lime', 'lab': [80, -25, 70], 'curve': LINE(0, 50)}]

# name: (whether the setup converts to Duotone, the measured call)
CASES = {
    'to-duotone-gray16': (False, 'to-duotone'),
    'set-duotone': (True, 'set-duotone'),
    'to-gray-from-duotone': (True, 'to-gray'),
    # Phase 1b.
    'to-rgb-from-duotone': (True, 'mode:rgb'),
    'to-cmyk-from-duotone': (True, 'mode:cmyk'),
    'to-lab-from-duotone': (True, 'mode:lab'),
    'to-indexed-from-duotone8': (True, 'mode:indexed'),
    'export-duotone-png16': (True, 'export:png'),
    'export-duotone-psd16': (True, 'export:psd'),
    # The close.
    'to-bitmap-from-duotone8': (True, 'mode:bitmap'),
}
# The cases on the 8-bit fixture.
EIGHT_BIT = {'to-indexed-from-duotone8', 'to-bitmap-from-duotone8'}


def sgray_png16(path, bits=16):
    """A 24 MP 16-bit (or `bits` 8) gray PNG of the gray drive's row, tagged sGray by an `sRGB`
    chunk."""
    row = gray.gray_row(bits)
    raw = b'\0' + (bytes(row) if bits == 8 else struct.pack(f'>{len(row)}H', *row))
    compressor = zlib.compressobj(6)
    data = b''.join(compressor.compress(raw) for _ in range(SIZE[1])) + compressor.flush()

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', *SIZE, bits, 0, 0, 0, 0))
                     + chunk(b'sRGB', b'\0')
                     + chunk(b'IDAT', data) + chunk(b'IEND', b''))


def measure_case(output, executable, fixture, label, case, baseline):
    converted, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    with cmyk.owned(output, label, executable) as app:
        ok, opened = cmyk.call(app, 'open_document', path=str(fixture))
        assert ok and opened['document'].get('color_mode') == 'gray', opened
        document = opened['document']['id']
        if converted:
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode='duotone', inks=TWO)
            assert ok and reply.get('color_mode') == 'duotone', reply
        plan = None
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif measured == 'to-duotone':
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode='duotone', inks=TWO)
            if ok and reply.get('color_mode') != 'duotone':
                ok = False
        elif measured == 'set-duotone':
            ok, reply = cmyk.call(app, 'set_duotone', document_id=document, inks=THREE)
            if ok and reply.get('changed') is not True:
                ok = False
        elif measured.startswith('mode:'):
            mode = measured.split(':')[1]
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode=mode)
            if ok and reply.get('color_mode') != mode:
                ok = False
        elif measured == 'export:png':
            ok, reply = cmyk.call(app, 'export_image', document_id=document,
                                  path=str(output / f'{label}.png'),
                                  settings=dict(format='png', bit_depth=16, profile='document'))
            if ok and reply.get('ok') is not True:
                ok = False
        elif measured == 'export:psd':
            ok, reply = cmyk.call(app, 'export_psd', document_id=document,
                                  path=str(output / f'{label}.psd'))
            if ok and reply.get('ok') is not True:
                ok = False
        else:
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode='gray')
            if ok and reply.get('color_mode') != 'gray':
                ok = False
        if not baseline and ok:
            plan = reply.get('working_memory_plan_bytes')
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-duotone-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(LIMIT)
    app = arguments.app if arguments.app.is_absolute() else ROOT / arguments.app
    executable = (app / 'Contents/MacOS/Doctored' if app.suffix == '.app' else app).resolve()
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    fixture = folder / 'sgray16.png'
    if not fixture.is_file():
        sgray_png16(fixture)
    fixture8 = folder / 'sgray8.png'
    if any(case in EIGHT_BIT for case in selected) and not fixture8.is_file():
        sgray_png16(fixture8, bits=8)
    report = dict(app=str(executable), app_sha256=cmyk.digest(executable),
                  script_sha256=cmyk.digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=LIMIT,
                  fixture=dict(file=str(fixture), bytes=fixture.stat().st_size), records=[])
    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                source = fixture8 if case in EIGHT_BIT else fixture
                try:
                    record = measure_case(output, executable, source, label, case, baseline)
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
        print(f'{case:22} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-duotone: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
