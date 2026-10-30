#!/usr/bin/env python3
"""Owned-app Phase 13A-6c-1 drive: native brush coverage over the stroke's
FOOTPRINT with exact hard-edge classification, so a hard stroke the
pre-13A-6c-1 canvas-field charge (2n + sum box*64) refused past the 256M work
cap of the time (`OLD_CAP`) finishes at full resolution. The beyond-cap
PROOF (product == the retained canvas-field reference) is
core/tests/bounded_brush_footprint.rs; this drive is the app-path gate. The
work cap is now 2 billion (`CAP`); B1 and B2 keep their comparison with
the old figure and B4 refuses past the new one.

Route (every gate): `brush_stroke` with `stroke_version: 1` ->
`nativeStrokeArguments` (AgentServer+NativeBrush.swift:26-110, the session
stamps the dabs and resolves a canonical `native_stroke`) -> `precisionPaint`
(AgentServer+PrecisionPaint.swift:76-77, `capturedNativeStroke`) ->
`NativeBrushStroke.coverage` (NativeBrushStroke.swift:104-110) ->
`dr_brush_u16_plan` / `dr_brush_u16`. B3's 8-bit document replays the same
canonical dabs through the byte RZBR path (visible parity only).

Gates (fixed in designs/phase-13a-regional-status.md before measuring):
B1: a 16-bit 1600x1200 white `new_document`; `brush_stroke` stroke_version 1,
hardness 100, size 512, spacing 5, color #000000, points (300,600)->(1300,600):
40 hard dabs fully on canvas (the first point + one per 25.6 px of the
1000-px chord); old charge >= 2n + 40*512^2*64 ~ 675M > 256M, new charge <=
2n + 40*(513^2 + 64*band_bound(512)) ~ 26M (the receipt records both).
Succeeds with no "work units" in the reply; `get_document` 1600x1200;
`sample_pixel` (sample_size 1) black at (800,600) and (800,350), white at
(800,200) and (43,600).
B2: the within-cap control: the same tip, (400,600)->(900,600), spacing 50
(2 dabs, old charge ~37M < 256M): black at (650,600) and (400,400), white at
(650,200) and (100,600).
B3: the 8-bit twin of B1 (the byte RZBR replay path, r/g/b keys).
B4: the beyond-NEW-cap refusal proving the route reaches dr_brush_u16_plan: a
fresh 16-bit 1600x1200 white document, B1's tip, 96 points cycling
(300,300), (1300,300), (300,900), (1300,900) -- 95 segments, 3,964 dabs
fully on canvas, each charged >= 512^2 + 64*band_bound(512) = 554,560, sum
>= 2.19e9 > 2e9: the reply is an error containing "2 billion footprint work
units" (the plan refuses; nothing is rasterized); then (800,600) and
(300,300) still white and `get_document` 1600x1200.
A failure exits nonzero; the JSON receipt is written to --receipts either way.
"""
import argparse
import json
import math
from pathlib import Path
import sys
import traceback

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

WIDTH, HEIGHT = 1600, 1200
SIZE = 512
MARGIN = 2.0 ** -16
SENTENCE = '2 billion footprint work units'
CAP = 2_000_000_000
# The old cap, which B1 and B2's historical comparisons are written against.
OLD_CAP = 256_000_000


def rpc(app, name, **arguments):
    return app.client.rpc('tools/call', {'name': name, 'arguments': arguments})


def text_of(reply):
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def band_bound(diameter):
    a = diameter / 2
    rho = math.sqrt(2) + MARGIN * a
    return math.ceil(4 * math.pi * a * rho + math.pi * rho * rho)


def stamps(points, spacing):
    """The session's own stamp count: the first point, then one per spacing
    along each segment from the running anchor (NativeBrush's walk)."""
    count = 1
    anchor = list(points[0])
    for target in points[1:]:
        distance = math.hypot(target[0] - anchor[0], target[1] - anchor[1])
        while distance >= spacing:
            fraction = spacing / distance
            anchor[0] += (target[0] - anchor[0]) * fraction
            anchor[1] += (target[1] - anchor[1]) * fraction
            count += 1
            distance = math.hypot(target[0] - anchor[0], target[1] - anchor[1])
    return count


def rgb(sample):
    codes = sample.get('mean_rgba_codes')
    if codes is not None:
        return [round(v) for v in codes[:3]], 65535
    return [sample[k] for k in ('r', 'g', 'b')], 255


def probe(app, document, label, black, white):
    found = {}
    for (x, y) in black:
        sample = app.call('sample_pixel', document_id=document, x=x, y=y, sample_size=1)
        codes, _ = rgb(sample)
        found[f'{x},{y}'] = codes
        assert codes == [0, 0, 0], f'{label}: ({x}, {y}) is not black: {codes}'
    for (x, y) in white:
        sample = app.call('sample_pixel', document_id=document, x=x, y=y, sample_size=1)
        codes, top = rgb(sample)
        found[f'{x},{y}'] = codes
        assert codes == [top] * 3, f'{label}: ({x}, {y}) is not white: {codes}'
    return found


def document(app, depth):
    created = app.call('new_document', width=WIDTH, height=HEIGHT, depth=depth,
                       background='white')
    return created['document']['id']


def dimensions(app, label, doc):
    summary = app.call('get_document', document_id=doc)
    assert (summary['width'], summary['height']) == (WIDTH, HEIGHT), f'{label}: {summary}'
    return [summary['width'], summary['height']]


def stroke(app, doc, points, spacing):
    return rpc(app, 'brush_stroke', document_id=doc, stroke_version=1, hardness=100,
               size=SIZE, spacing=spacing, color='#000000', points=points)


def gate(app, label, depth, points, spacing, black, white):
    doc = document(app, depth)
    reply = stroke(app, doc, points, spacing)
    text = text_of(reply)
    assert not reply.get('isError'), f'{label} brush_stroke refused: {text}'
    assert 'work units' not in text, f'{label}: "work units" in {text}'
    n = WIDTH * HEIGHT
    dabs = stamps(points, SIZE * spacing / 100)
    return {'document': doc, 'depth': depth, 'points': points, 'spacing': spacing,
            'dabs': dabs,
            'old_charge_lower_bound': 2 * n + dabs * SIZE * SIZE * 64,
            'new_charge_upper_bound': 2 * n + dabs * ((SIZE + 1) ** 2 + 64 * band_bound(SIZE)),
            'reply': text[:400], 'dimensions': dimensions(app, label, doc),
            'samples': probe(app, doc, label, black, white)}


def refusal(app):
    doc = document(app, 16)
    corners = [[300, 300], [1300, 300], [300, 900], [1300, 900]]
    points = [corners[k % 4] for k in range(96)]
    dabs = stamps(points, SIZE * 5 / 100)
    per = SIZE * SIZE + 64 * band_bound(SIZE)
    assert per == 554_560, per
    assert 3_607 <= dabs < 50_000 and dabs * per > CAP, dabs
    reply = stroke(app, doc, points, 5)
    text = text_of(reply)
    assert reply.get('isError'), f'B4 was not refused: {text}'
    assert SENTENCE in text, f'B4 refused without the core sentence: {text}'
    return {'document': doc, 'points': points, 'dabs': dabs, 'per_dab_lower_bound': per,
            'charge_lower_bound': dabs * per, 'reply': text,
            'samples': probe(app, doc, 'B4', [], [(800, 600), (300, 300)]),
            'dimensions': dimensions(app, 'B4', doc)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', default=str(app_under_test.default_app_bundle()))
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    long = [[300, 600], [1300, 600]]
    failed = False
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['mcp_socket'] = app.client.socket_path
            receipts['B1'] = gate(app, 'B1', 16, long, 5, [(800, 600), (800, 350)],
                                  [(800, 200), (43, 600)])
            assert receipts['B1']['dabs'] == 40, receipts['B1']['dabs']
            assert receipts['B1']['old_charge_lower_bound'] > OLD_CAP
            assert receipts['B1']['new_charge_upper_bound'] < OLD_CAP
            print('PASS B1 a 40-dab hard 512-px 16-bit stroke past the old cap finishes at '
                  '1600x1200, black on the stroke, white off it', flush=True)
            receipts['B2'] = gate(app, 'B2', 16, [[400, 600], [900, 600]], 50,
                                  [(650, 600), (400, 400)], [(650, 200), (100, 600)])
            assert receipts['B2']['old_charge_lower_bound'] < OLD_CAP
            print('PASS B2 the within-cap 2-dab control finishes', flush=True)
            receipts['B3'] = gate(app, 'B3', 8, long, 5, [(800, 600), (800, 350)],
                                  [(800, 200), (43, 600)])
            print('PASS B3 the 8-bit twin finishes with the same visible samples', flush=True)
            receipts['B4'] = refusal(app)
            print(f'PASS B4 a {receipts["B4"]["dabs"]}-dab stroke past the NEW cap refuses '
                  'with the core sentence and leaves the document white', flush=True)
    except Exception:
        failed = True
        receipts['failure'] = traceback.format_exc()
        print('FAIL', receipts['failure'], flush=True)
    (folder / 'phase13a6c1.json').write_text(json.dumps(receipts, indent=2, sort_keys=True))
    print(f'receipt: {folder / "phase13a6c1.json"}', flush=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
