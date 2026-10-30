#!/usr/bin/env python3
"""The #547 Bitmap phases 1a to 2a-ii admission drive, on a built Release app.

Every new Bitmap plan is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so the result's
display projection is inside the interval). The gates are FIXED here before the
drive runs; a miss is recorded in `receipt.json` and fails, and the answer to a
miss is the plan's own per-pixel term (`bitmap_convert::TO_BITMAP_BYTES_PER_PIXEL`,
since phase 1c `bitmap_io::OPEN_BYTES_PER_PIXEL` for the opens and
`bitmap_export::EXPORT_BYTES_PER_PIXEL` for the kept export, since phase 2a-i
`bitmap_edit::BITMAP_EDIT_BYTES_PER_PIXEL` for the edits), never this bound
and never a shared constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. The harness —
`call`, `footprint`, `settle`, `owned` — is `scripts/cmyk-admission/measure.py`'s
and the gray PNG writer `scripts/gray-admission/measure.py`'s (an 8-bit gray PNG
of 6000x4000, 24 MP, whose row is a ramp with noise), each loaded from its file
as `scripts/indexed-admission/measure.py` loads them.

The plans (each the reply's `working_memory_plan_bytes`):

- `to-bitmap-threshold`: the gray PNG (an 8-bit Grayscale document);
  `convert_mode bitmap method threshold` (output pixels x
  `TO_BITMAP_BYTES_PER_PIXEL` + bookkeeping).
- `to-bitmap-diffusion`: the same with `method diffusion` (adds its error
  rows: since phase 2b Sierra Lite's two, the 32 MiB nearest-colour memo of
  1b's Floyd–Steinberg gone).
- `to-bitmap-resample`: the same, Diffusion, with `resolution` 1.25 x the
  document's own ppi (read in the setup), so the gray is resampled Bicubic to
  7500x5000 first (the LARGER of the geometry plan's scratch and the larger
  canvas's per-pixel term, not their sum: #547 Bitmap phase 1b's fix of 1a's
  2.108 overlap).
- `to-bitmap-halftone` (phase 1b): `method halftone` at 53 lpi, 45 degrees,
  round (adds the 512 KiB sorted spot grid).
- `to-bitmap-custom` (phase 1b): `method custom` with an inline 512x512 tile
  (`custom_pattern`: texel (x, y) gray `(x + 3y) mod 256`, opaque; adds the
  tile's 256 KiB threshold map; the options JSON is ~1.4 MB).
- `to-gray-from-bitmap`: the gray PNG converted to Bitmap (threshold) in the
  setup; `convert_mode gray` (Size Ratio 1: every sample shared, then the
  display preparation).
- `to-gray-ratio-4`: the same setup; `convert_mode gray size_ratio 4` (the
  1500x1000 block average).

Phase 1c's files, each 6000x4000 (24 MP) and written here: every row is the
gray fixture's ramp-with-noise row thresholded at 128 after one of 16 offsets
(row y takes offset `(y * 7) mod 16 * 8 - 64`), packed MSB first.

- `open-bitmap-png`: no setup; `open_document` of the 1-bit gray PNG. The plan
  is the open's, read from the source (`bitmap_io::open_scratch`): pixels x
  `OPEN_BYTES_PER_PIXEL` (42) + the file's bytes + bookkeeping (1 MiB).
- `open-bitmap-tiff`: the same for the uncompressed 1-bit BlackIsZero TIFF.
- `open-bitmap-psd`: `inspect_psd` in the setup; `open_document psd_mode
  composite` of the colour-mode-0 PSD (RLE, 1 is black), the same plan
  (`bitmap_psd` charges `open_scratch`).
- `export-bitmap-tiff`: the gray PNG converted to Bitmap (threshold) in the
  setup; `export_image` tiff with profile document (the kept 1-bit TIFF); the
  plan is the reply's `working_memory_plan_bytes`.
- `export-bitmap-psd`: the same setup and `inspect_psd_export` (psd,
  flattened); `export_psd` psd flattened; the plan is the report's
  `working_memory_plan_bytes`.

Phase 2a-i's edits (`bitmap_edit`: the inner op's plan on the Gray storage plus
the output canvas x `BITMAP_EDIT_BYTES_PER_PIXEL` (34; 31 as declared missed
on `crop-extend-bitmap`) + bookkeeping), each in
the `bitmap` setup. An edit case names its tool, its arguments and the reply key
its plan is read from, and must leave a Bitmap document (read after the
footprint):

- `brush-bitmap`: `brush_stroke` layer 0 along (100, 100)-(5900, 3900), size
  200 (`brush-indexed`'s stroke); plan `working_memory_plan_bytes`.
- `rotate90-bitmap`: `rotate` 90; plan `working_memory_plan_bytes`.
- `crop-extend-bitmap`: `crop` x -500, y -500, 7000 x 5000, `background_color`
  `#000000`, `delete_cropped` false (the inner straighten pads nothing, so its
  own plan is near zero and the adapter's term carries the case); plan the
  reply's `planned_bytes`.

Phase 2a-ii's two resampled-geometry cases are gone: since phase 2b's menu
walk `transform_layers` (`transform-bitmap`, 5 degrees) and `rotate` at an
arbitrary angle (`rotate5-bitmap`, 5 degrees bicubic) refuse on a Bitmap
document.
"""
import argparse
import base64
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
# Phase 1c's open plan terms, read from the source (module doc).
OPEN_BYTES_PER_PIXEL = 42
MIB = 1024 * 1024
SIZE = gray.SIZE


def custom_pattern():
    """The 512x512 inline tile of `to-bitmap-custom` (module doc)."""
    side = 512
    rgba = bytes(c for y in range(side) for x in range(side)
                 for c in ((x + 3 * y) % 256,) * 3 + (255,))
    return dict(name='Admission', width=side, height=side,
                rgba=base64.b64encode(rgba).decode())


# name: (the setup, the measured call's convert_mode arguments)
CASES = {
    'to-bitmap-threshold': ('gray8', dict(mode='bitmap', method='threshold')),
    'to-bitmap-diffusion': ('gray8', dict(mode='bitmap', method='diffusion')),
    'to-bitmap-resample': ('gray8', dict(mode='bitmap', method='diffusion', resolution=1.25)),
    'to-bitmap-halftone': ('gray8', dict(mode='bitmap', method='halftone', frequency=53, angle=45,
                                         shape='round')),
    'to-bitmap-custom': ('gray8', dict(mode='bitmap', method='custom', pattern=custom_pattern())),
    'to-gray-from-bitmap': ('bitmap', dict(mode='gray')),
    'to-gray-ratio-4': ('bitmap', dict(mode='gray', size_ratio=4)),
    # Phase 1c: the 1-bit files (module doc).
    'open-bitmap-png': ('none', 'open:bpng1'),
    'open-bitmap-tiff': ('none', 'open:btiff1'),
    'open-bitmap-psd': ('none', 'open-psd:bpsd1'),
    'export-bitmap-tiff': ('bitmap', 'export-tiff'),
    'export-bitmap-psd': ('bitmap', 'export-psd'),
    # Phase 2a-i: the edits (module doc), each (its tool, its arguments, the reply key its plan
    # is read from).
    'brush-bitmap': ('bitmap', ('brush_stroke', dict(layer=0, points=[[100, 100], [5900, 3900]],
                                                     size=200), 'working_memory_plan_bytes')),
    'rotate90-bitmap': ('bitmap', ('rotate', dict(degrees=90), 'working_memory_plan_bytes')),
    'crop-extend-bitmap': ('bitmap', ('crop', dict(x=-500, y=-500, width=7000, height=5000,
                                                   background_color='#000000',
                                                   delete_cropped=False), 'planned_bytes')),
}


def bitmap_rows():
    """The 16 packed rows of phase 1c's files (module doc), 1 white."""
    base = gray.gray_row(8)
    line = (SIZE[0] + 7) // 8
    rows = []
    for k in range(16):
        offset = k * 8 - 64
        out = bytearray(line)
        for x, g in enumerate(base):
            if g + offset >= 128:
                out[x // 8] |= 0x80 >> (x % 8)
        rows.append(bytes(out))
    return rows


def row_of(rows, y):
    return rows[(y * 7) % 16]


def bitmap_png(path, rows):
    raw = zlib.compressobj(6)
    data = b''.join(raw.compress(b'\0' + row_of(rows, y)) for y in range(SIZE[1])) + raw.flush()

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', *SIZE, 1, 0, 0, 0, 0))
                     + chunk(b'IDAT', data) + chunk(b'IEND', b''))


def bitmap_tiff(path, rows):
    length = len(rows[0]) * SIZE[1]

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [(256, 4, 1, struct.pack('<I', SIZE[0])), (257, 4, 1, struct.pack('<I', SIZE[1])),
               (258, 3, 1, short(1)), (259, 3, 1, short(1)), (262, 3, 1, short(1)),
               (273, 4, 1, struct.pack('<I', 8)), (277, 3, 1, short(1)),
               (278, 4, 1, struct.pack('<I', SIZE[1])), (279, 4, 1, struct.pack('<I', length)),
               (284, 3, 1, short(1))]
    with path.open('wb') as out:
        out.write(b'II*\x00' + struct.pack('<I', 8 + length))
        for y in range(SIZE[1]):
            out.write(row_of(rows, y))
        out.write(struct.pack('<H', len(entries)))
        for tag, kind, count, value in entries:
            out.write(struct.pack('<HHI', tag, kind, count) + value)
        out.write(struct.pack('<I', 0))


def bitmap_psd(path, rows):
    """A colour-mode-0 PSD, depth 1, RLE (literal runs), 1 is black."""
    encoded = []
    for row in rows:
        inverse = bytes(255 - b for b in row)
        encoded.append(b''.join(bytes([len(inverse[i:i + 128]) - 1]) + inverse[i:i + 128]
                                for i in range(0, len(inverse), 128)))
    header = (b'8BPS' + struct.pack('>H', 1) + b'\0' * 6
              + struct.pack('>HIIHH', 1, SIZE[1], SIZE[0], 1, 0)
              + struct.pack('>I', 0) + struct.pack('>I', 0) + struct.pack('>I', 0))
    with path.open('wb') as out:
        out.write(header + struct.pack('>H', 1))
        for y in range(SIZE[1]):
            out.write(struct.pack('>H', len(row_of(encoded, y))))
        for y in range(SIZE[1]):
            out.write(row_of(encoded, y))


def fixtures(output):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {'gpng8': folder / 'gray8.png', 'bpng1': folder / 'bitmap1.png',
            'btiff1': folder / 'bitmap1.tif', 'bpsd1': folder / 'bitmap1.psd'}
    if not made['gpng8'].is_file():
        gray.gray_png(made['gpng8'], 8)
    rows = bitmap_rows()
    for key, writer in (('bpng1', bitmap_png), ('btiff1', bitmap_tiff), ('bpsd1', bitmap_psd)):
        if not made[key].is_file():
            writer(made[key], rows)
    return made


def measure_file_case(app, output, made, label, document, fingerprint, measured):
    """Phase 1c's measured call (module doc): (ok, reply, plan); the PSD export's plan is its
    report's, read in the setup."""
    if measured.startswith('open'):
        path = made[measured.split(':')[1]]
        arguments = dict(path=str(path))
        if measured.startswith('open-psd'):
            arguments.update(psd_mode='composite', psd_fingerprint=fingerprint)
        ok, reply = cmyk.call(app, 'open_document', **arguments)
        plan = SIZE[0] * SIZE[1] * OPEN_BYTES_PER_PIXEL + path.stat().st_size + MIB
        if ok and reply['document'].get('color_mode') != 'bitmap':
            ok, reply = False, f'opened as {reply["document"].get("color_mode")}'
        return ok, reply, plan
    if measured == 'export-tiff':
        ok, reply = cmyk.call(app, 'export_image', document_id=document,
                              path=str(output / f'{label}.tif'),
                              settings=dict(format='tiff', bit_depth=8, profile='document'))
        plan = reply.get('working_memory_plan_bytes') if ok else None
        if ok and reply.get('bit_depth') != 1:
            ok, reply = False, f'not 1 bit: {reply}'
        return ok, reply, plan
    ok, reply = cmyk.call(app, 'export_psd', document_id=document, path=str(output / f'{label}.psd'),
                          mode='flattened', format='psd')
    if ok and reply.get('bit_depth') != 1:
        ok, reply = False, f'not 1 bit: {reply}'
    return ok, reply, None


def measure_case(output, executable, made, label, case, baseline):
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    with cmyk.owned(output, label, executable) as app:
        document = fingerprint = psd_plan = None
        if setup != 'none':
            ok, opened = cmyk.call(app, 'open_document', path=str(made['gpng8']))
            assert ok and opened['document'].get('color_mode') == 'gray', opened
            document = opened['document']['id']
        if setup == 'bitmap':
            ok, converted = cmyk.call(app, 'convert_mode', document_id=document, mode='bitmap',
                                      method='threshold')
            assert ok and converted.get('color_mode') == 'bitmap', converted
        if measured == 'export-psd':
            ok, inspected = cmyk.call(app, 'inspect_psd_export', document_id=document,
                                      mode='flattened', format='psd')
            assert ok and inspected.get('available'), inspected
            psd_plan = inspected['working_memory_plan_bytes']
        if isinstance(measured, str) and measured.startswith('open-psd'):
            ok, inspected = cmyk.call(app, 'inspect_psd', path=str(made[measured.split(':')[1]]))
            assert ok and inspected.get('import_precision') == 'bitmap', inspected
            fingerprint = inspected['source_fingerprint']
        arguments = dict(measured) if isinstance(measured, dict) else None
        if arguments and 'resolution' in arguments:
            ok, info = cmyk.call(app, 'get_document', document_id=document)
            assert ok, info
            arguments['resolution'] = arguments['resolution'] * info['resolution']['ppi_x']
        plan = None
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif isinstance(measured, tuple):
            tool, edit_arguments, key = measured
            ok, reply = cmyk.call(app, tool, document_id=document, **edit_arguments)
            plan = reply.get(key) if ok else None
            if ok and tool == 'brush_stroke' and reply.get(
                    'changed') is not True:
                ok = False
        elif arguments is None:
            ok, reply, plan = measure_file_case(app, output, made, label, document, fingerprint,
                                                measured)
            if plan is None and ok:
                plan = psd_plan
        else:
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, **arguments)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != arguments['mode']:
                ok = False
        cmyk.settle(pid)
        post, peak = cmyk.footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        # An edit must leave a Bitmap document, read after the footprint.
        if not baseline and ok and isinstance(measured, tuple):
            read, info = cmyk.call(app, 'get_document', document_id=document)
            if not read or info.get('color_mode') != 'bitmap':
                ok, reply = False, f'not a Bitmap document after the edit: {info}'
                record['ok'] = False
        if not baseline:
            if not ok:
                record['refusal'] = str(reply)[:800]
            record['plan'] = plan
            # The inline tile is recorded by its size, not its 1.4 MB of base64.
            record['arguments'] = measured if arguments is None else {
                key: (f"{value['width']}x{value['height']} tile" if key == 'pattern' else value)
                for key, value in arguments.items()}
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-bitmap-admission-')))
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
    print(f'gate M-bitmap: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
