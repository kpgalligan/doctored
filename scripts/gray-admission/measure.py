#!/usr/bin/env python3
"""The #1 phase-1a, 1b, 2e, 4 and 5a Grayscale admission drive, on a built Release app.

Every new Grayscale plan is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so a document's
window and display projection are inside the interval). The gates are FIXED
here before the drive runs; a miss is recorded in `receipt.json` and fails,
and the answer to a miss is the plan's own per-pixel term
(`gray_io::OPEN_BYTES_PER_PIXEL`, `gray_convert::TO_GRAY_BYTES_PER_PIXEL` or
`gray_export::EXPORT_BYTES_PER_PIXEL`; for phase 2e's Art History Brush the Gray
arm's `history_brush_plan` and `source_pixels·16` figures), never this bound
and never a shared constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. The harness —
`call`, `footprint`, `settle`, `owned`, the CMYK TIFF writer — is
`scripts/cmyk-admission/measure.py`'s, loaded from that file. Fixtures are
6000x4000 (24 MP), written HERE: a horizontal gray gradient with per-column
noise (so no tile is uniform), as a 16-bit and an 8-bit gray PNG, a 16-bit
BlackIsZero TIFF and (through `magick`, skipped and said so when absent) a
one-component 8-bit JPEG; the CMYK TIFF is the CMYK drive's Generic CMYK
gradient; the 16-bit RGB PNG is `scripts/host-still-admission/author.swift`'s.

The plans:

- `open-png16`, `open-png8`, `open-tiff16`, `open-jpeg8`: `open_document`;
  plan = pixels x `OPEN_BYTES_PER_PIXEL` + the file's bytes + 1 MiB
  (`gray_io::open_scratch`, the constant read from `core/src/gray_io.rs`).
- `new-gray-16`, `new-gray-32`: `new_document color_mode gray` 6000x4000,
  white; plan = pixels x `OPEN_BYTES_PER_PIXEL` + 1 MiB, twice that at 32
  bits (`new_gray`'s charge: a float pixel holds twice a u16 pixel's bytes).
- `to-gray-16`: the 16-bit RGB PNG at 16 bits; `convert_mode gray`; plan = the
  reply's `working_memory_plan_bytes`.
- `to-gray-8`: the same PNG opened `rgba8`; `convert_mode gray` (promoted, then
  converted at depth 8); the same.
- `to-gray-cmyk16`: the 16-bit CMYK TIFF; `convert_mode gray` (through 16-bit
  ProPhoto RGB in one job); the same.
- `to-rgb-16`: the 16-bit gray PNG; `convert_mode rgb`; the same.
- `export-gray-png16`, `export-gray-tiff8`, `export-gray-jpeg8` (phase 1b):
  the 16-bit gray PNG; `export_image` with profile document (a one-channel
  PNG at 16 bits, TIFF at 8, JPEG at 8, the kept-gray stages of
  `gray_export`); plan = the reply's `working_memory_plan_bytes`.
- `art-history-gray-8` (phase 2e): the 8-bit gray PNG, a Gaussian Blur 8 as
  the setup (as the CMYK harness's `tiff8-history`), then
  `art_history_brush_stroke` from the opened state along (100, 100)–(5900,
  3900), style loose_long, area 500, size 100, seed 658 (the CMYK
  `stroke:art-history` arguments); plan = the reply's
  `working_memory_plan_bytes` (`gray_art_history_plan`).
- `to-gray-16-adjustments` (phase 2e): the 16-bit RGB PNG at 16 bits,
  `add_adjustment_layer` levels (gamma 1.4) and gradient_map (#ff0000 →
  #00ffff) as the setup, then `convert_mode gray`; plan = the reply's
  `working_memory_plan_bytes`; the reply must be color_mode gray with 3
  layers (the records carried).
- `paste-rgb-into-gray-16` (phase 4): the 16-bit RGB PNG at 16 bits,
  `select_all` and `copy` layer 0 as the setup (on the case's own private
  pasteboard, `DR_PASTEBOARD_NAME`), the 16-bit gray PNG opened as the
  target; `paste` in_place into it (RGB → Gray, `gray_clipboard::paste`);
  plan = the conversion's result held through the paste, `2·P` (P = the
  window's `payload_bytes`: 8 B a pixel plus 2 of coverage;
  `ClipboardPayload::converted_to_gray`), beside the converting arm's
  insertion and display term, `LAYER_PASTE_MARGIN_PER_PIXEL·n +
  4·BOOKKEEPING_BYTES` (a CMYK paste's, `cmyk_paste_insertion`).
- `copy-gray-16` (phase 4): the 16-bit gray PNG, `select_all`; `copy` layer 0;
  plan = the typed copy's host stage (`precision_clipboard_host`:
  `3·P + PUBLIC_RENDERING_BYTES_PER_PIXEL·n`) plus the one-channel TIFF's
  admission (`GrayClipboardPayload::gray_tiff`: `BOOKKEEPING_BYTES + 8·n`).
- `assign-gray-16`, `convert-gray-16` (phase 5a): the 16-bit gray PNG (the
  working Gray assigned at open); `assign_profile` / `convert_profile` with
  profile gray_gamma_22; plan = the reply's `working_memory_plan_bytes`
  (`gray_profile`'s plan); the reply must say changed.
- `depth-gray-8`, `depth-gray-32` (phase 5a): the 16-bit gray PNG;
  `convert_precision` gray8 / gray32; the same.
- `hdr-gray-32-16`, `hdr-gray-32-8` (phase 5a): the 16-bit gray PNG made a
  32-bit one-raster Gray document by `convert_precision gray32` as the setup;
  `hdr_toning` local_adaptation (its defaults) at bit_depth 16 / 8, the whole
  job (the 8-bit one's host depth-8 child included); plan = the reply's
  `working_memory_plan_bytes` (`hdr_toning_plan`'s 32-bit Gray arm); the
  result must be a Gray document at that depth.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[2]
_SPEC = importlib.util.spec_from_file_location('cmyk_measure', ROOT / 'scripts/cmyk-admission/measure.py')
cmyk = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(cmyk)

LIMIT = 8589934592
SIZE = (6000, 4000)
MIB = 1 << 20
OPEN_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const OPEN_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/gray_io.rs').read_text()).group(1))
PUBLIC_RENDERING_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const PUBLIC_RENDERING_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/precision_clipboard_host.rs').read_text()).group(1))
LAYER_PASTE_MARGIN_PER_PIXEL = int(re.search(
    r'pub\(crate\) const LAYER_PASTE_MARGIN_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/doc_paste_layers.rs').read_text()).group(1))
BOOKKEEPING_BYTES = MIB

# name: (the setup's fixture, the measured call)
CASES = {
    'open-png16': ('none', 'open:gpng16'),
    'open-png8': ('none', 'open:gpng8'),
    'open-tiff16': ('none', 'open:gtiff16'),
    'open-jpeg8': ('none', 'open:gjpeg8'),
    'new-gray-16': ('none', 'new:16'),
    'new-gray-32': ('none', 'new:32'),
    'to-gray-16': ('png16', 'to-gray'),
    'to-gray-8': ('png8', 'to-gray'),
    'to-gray-cmyk16': ('tiff16', 'to-gray'),
    'to-rgb-16': ('gpng16', 'to-rgb'),
    'export-gray-png16': ('gpng16', 'export:png:16'),
    'export-gray-tiff8': ('gpng16', 'export:tiff:8'),
    'export-gray-jpeg8': ('gpng16', 'export:jpeg:8'),
    # Phase 2e.
    'art-history-gray-8': ('gpng8-blurred', 'stroke:art-history'),
    'to-gray-16-adjustments': ('png16-adjusted', 'to-gray'),
    # Phase 4.
    'paste-rgb-into-gray-16': ('png16', 'clip:paste-rgb-into-gray'),
    'copy-gray-16': ('gpng16', 'clip:copy-gray'),
    # Phase 5a.
    'assign-gray-16': ('gpng16', 'profile:assign'),
    'convert-gray-16': ('gpng16', 'profile:convert'),
    'depth-gray-8': ('gpng16', 'depth:8'),
    'depth-gray-32': ('gpng16', 'depth:32'),
    'hdr-gray-32-16': ('gpng16-32', 'hdr:16'),
    'hdr-gray-32-8': ('gpng16-32', 'hdr:8'),
}
# The phase-2e setups' fixture.
SETUP_FIXTURE = {'png8': 'png16', 'gpng8-blurred': 'gpng8', 'png16-adjusted': 'png16',
                 'gpng16-32': 'gpng16'}
ART_HISTORY = dict(layer=0, points=[[100, 100], [5900, 3900]], size=100, style='loose_long',
                   area=500, seed=658)
GRADIENT_STOPS = [{'position': 0, 'color': '#ff0000'}, {'position': 1, 'color': '#00ffff'}]


def gray_row(bits):
    top = 255 if bits == 8 else 65535
    seed, row = 0x9E3779B9, []
    for x in range(SIZE[0]):
        seed = (seed * 1103515245 + 12345) & 0xFFFFFFFF
        row.append(((x * top) // SIZE[0] + (seed >> 16) % 17) % (top + 1))
    return row


def gray_png(path, bits):
    row = gray_row(bits)
    raw = b'\0' + (bytes(row) if bits == 8 else struct.pack(f'>{len(row)}H', *row))
    compressor = zlib.compressobj(6)
    data = b''.join(compressor.compress(raw) for _ in range(SIZE[1])) + compressor.flush()

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', *SIZE, bits, 0, 0, 0, 0))
                     + chunk(b'IDAT', data) + chunk(b'IEND', b''))


def gray_tiff16(path):
    row = struct.pack(f'<{SIZE[0]}H', *gray_row(16))
    length = len(row) * SIZE[1]

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [(256, 4, 1, struct.pack('<I', SIZE[0])), (257, 4, 1, struct.pack('<I', SIZE[1])),
               (258, 3, 1, short(16)), (259, 3, 1, short(1)), (262, 3, 1, short(1)),
               (273, 4, 1, struct.pack('<I', 8)), (277, 3, 1, short(1)),
               (278, 4, 1, struct.pack('<I', SIZE[1])), (279, 4, 1, struct.pack('<I', length)),
               (284, 3, 1, short(1))]
    with path.open('wb') as out:
        out.write(b'II*\x00' + struct.pack('<I', 8 + length))
        for _ in range(SIZE[1]):
            out.write(row)
        out.write(struct.pack('<H', len(entries)))
        for tag, kind, count, value in entries:
            out.write(struct.pack('<HHI', tag, kind, count) + value)
        out.write(struct.pack('<I', 0))


def fixtures(output):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {'gpng16': folder / 'gray16.png', 'gpng8': folder / 'gray8.png',
            'gtiff16': folder / 'gray16.tif', 'gjpeg8': folder / 'gray8.jpg',
            'png16': folder / 'rgb16.png', 'tiff16': folder / 'cmyk16.tif'}
    if not made['gpng16'].is_file():
        gray_png(made['gpng16'], 16)
    if not made['gpng8'].is_file():
        gray_png(made['gpng8'], 8)
    if not made['gtiff16'].is_file():
        gray_tiff16(made['gtiff16'])
    magick = shutil.which('magick')
    if not made['gjpeg8'].is_file() and magick:
        subprocess.run([magick, str(made['gpng8']), '-quality', '95', str(made['gjpeg8'])],
                       check=True, capture_output=True, timeout=600)
    if not made['tiff16'].is_file():
        cmyk.cmyk_tiff(made['tiff16'], *SIZE, 16, cmyk.GENERIC.read_bytes())
    if not made['png16'].is_file():
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(cmyk.AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        subprocess.run([str(binary), 'png16', str(SIZE[0]), str(SIZE[1]), str(made['png16'])],
                       check=True, capture_output=True, timeout=600)
    return {k: v for k, v in made.items() if v.is_file()}


def measure_clip_case(output, executable, made, label, case, baseline):
    """A phase-4 case (module doc): the setup runs on the case's own private
    pasteboard; the plan is the module doc's figure for the 24 MP window."""
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    pixels = SIZE[0] * SIZE[1]
    window = pixels * 8 + pixels * 2          # payload_bytes with coverage (select_all)
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.gray-admission.{os.getpid()}.{label}'
    try:
        with cmyk.owned(output, label, executable) as app:
            def opened(key, **extra):
                ok, reply = cmyk.call(app, 'open_document', path=str(made[key]), **extra)
                assert ok, reply
                return reply['document']['id']

            def step(document, tool, **arguments):
                ok, reply = cmyk.call(app, tool, document_id=document, **arguments)
                assert ok, (tool, reply)
                return reply

            if measured == 'clip:paste-rgb-into-gray':
                source = opened('png16', precision='preserve')
                step(source, 'select_all')
                step(source, 'copy', layer=0)
                document = opened('gpng16')
                plan = 2 * window + LAYER_PASTE_MARGIN_PER_PIXEL * pixels + 4 * BOOKKEEPING_BYTES
                name, arguments = 'paste', dict(placement='in_place')
            else:
                document = opened('gpng16')
                step(document, 'select_all')
                plan = (3 * window + PUBLIC_RENDERING_BYTES_PER_PIXEL * pixels
                        + BOOKKEEPING_BYTES + 8 * pixels)
                name, arguments = 'copy', dict(layer=0)
            pid = app.process.pid
            cmyk.settle(pid)
            pre, _ = cmyk.footprint(pid, reset=True)
            if baseline:
                ok, reply = cmyk.call(app, 'resource_usage')
            else:
                ok, reply = cmyk.call(app, name, document_id=document, **arguments)
                if ok and name == 'paste' and (reply.get('color_conversion') or {}).get('to') != 'Gray':
                    ok, reply = False, f'not converted into Gray: {reply}'
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
        if setup != 'none':
            arguments = dict(path=str(made[SETUP_FIXTURE.get(setup, setup)]))
            if setup in ('png16', 'png8', 'png16-adjusted'):
                arguments['precision'] = 'rgba8' if setup == 'png8' else 'preserve'
            ok, opened = cmyk.call(app, 'open_document', **arguments)
            assert ok, opened
            document = opened['document']['id']
            if setup == 'gpng8-blurred':
                ok, blurred = cmyk.call(app, 'apply_filter', document_id=document, layer=0,
                                        filter='blur', sigma=8)
                assert ok, blurred
            if setup == 'png16-adjusted':
                ok, added = cmyk.call(app, 'add_adjustment_layer', document_id=document,
                                      op='levels', params={'gamma': 1.4}, name='Levels')
                assert ok, added
                ok, added = cmyk.call(app, 'add_adjustment_layer', document_id=document,
                                      op='gradient_map',
                                      params={'gradient': {'stops': GRADIENT_STOPS}}, name='Map')
                assert ok, added
            if setup == 'gpng16-32':
                ok, promoted = cmyk.call(app, 'convert_precision', document_id=document, mode='gray32')
                assert ok and promoted.get('changed'), promoted
        plan = None
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif measured.startswith('open:'):
            path = made[measured.split(':')[1]]
            ok, reply = cmyk.call(app, 'open_document', path=str(path))
            plan = pixels * OPEN_BYTES_PER_PIXEL + path.stat().st_size + MIB
            if ok and reply['document'].get('color_mode') != 'gray':
                ok, reply = False, f'opened as {reply["document"].get("color_mode")}'
        elif measured.startswith('new:'):
            ok, reply = cmyk.call(app, 'new_document', width=SIZE[0], height=SIZE[1],
                                  color_mode='gray', depth=int(measured.split(':')[1]),
                                  background='white')
            plan = pixels * OPEN_BYTES_PER_PIXEL + MIB
            if measured == 'new:32':
                plan *= 2       # a float pixel's bytes are twice a u16 pixel's (`new_gray`)
        elif measured == 'stroke:art-history':
            ok, reply = cmyk.call(app, 'art_history_brush_stroke', document_id=document,
                                  **ART_HISTORY)
            plan = reply.get('working_memory_plan_bytes') if ok else None
        elif measured.startswith('profile:'):
            tool = 'assign_profile' if measured == 'profile:assign' else 'convert_profile'
            ok, reply = cmyk.call(app, tool, document_id=document, profile='gray_gamma_22')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and not reply.get('changed'):
                ok, reply = False, f'changed nothing: {reply}'
        elif measured.startswith('depth:'):
            bits = measured.split(':')[1]
            ok, reply = cmyk.call(app, 'convert_precision', document_id=document, mode=f'gray{bits}')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('bits_per_channel') != int(bits):
                ok, reply = False, f'not {bits} bits: {reply}'
        elif measured.startswith('hdr:'):
            bits = int(measured.split(':')[1])
            ok, reply = cmyk.call(app, 'hdr_toning', document_id=document, method='local_adaptation',
                                  bit_depth=bits)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok:
                ok_p, precision = cmyk.call(app, 'get_precision', document_id=document)
                if not ok_p or precision.get('color_mode') != 'gray' \
                        or precision.get('bits_per_channel') != bits:
                    ok, reply = False, f'not Gray at {bits} bits: {precision}'
        elif measured.startswith('export:'):
            _, fmt, bits = measured.split(':')
            path = output / f'{label}.{fmt}'
            ok, reply = cmyk.call(app, 'export_image', document_id=document, path=str(path),
                                  settings=dict(format=fmt, bit_depth=int(bits),
                                                profile='document'))
            plan = reply.get('working_memory_plan_bytes') if ok else None
        else:
            mode = 'gray' if measured == 'to-gray' else 'rgb'
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode=mode)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != mode:
                ok = False
            if ok and setup == 'png16-adjusted' and reply['document'].get('layer_count') != 3:
                ok, reply = False, f'carried {reply["document"].get("layer_count")} layers'
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
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-gray-admission-')))
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
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size)
                            for name, path in made.items()},
                  skipped={}, records=[])
    for case in list(selected):
        setup, measured = CASES[case]
        needs = measured.split(':')[1] if measured.startswith('open:') else setup
        if measured == 'clip:paste-rgb-into-gray' and 'gpng16' not in made:
            needs = 'gpng16'
        needs = SETUP_FIXTURE.get(needs, needs)
        if needs != 'none' and needs not in made:
            report['skipped'][case] = f'fixture {needs} could not be made (magick absent)'
            selected.remove(case)
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
    for case, reason in report['skipped'].items():
        print(f'{case:16} SKIPPED: {reason}', flush=True)
    print(f'gate M-gray: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
