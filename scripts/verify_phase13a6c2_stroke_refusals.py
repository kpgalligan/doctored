#!/usr/bin/env python3
"""Owned-app Phase 13A-6c-2 drive: every Swift stroke path refuses an over-cap
stroke BY NAME and never commits a truncated prefix, and UNVERSIONED typed
16-bit point strokes generate native dabs through the one existing walk
(`NativeBrushStrokeSession.generated`) and rasterize through `dr_brush_u16`,
so they now refuse by name at BOTH core limits (50,000 dabs and
2 billion footprint work units; 256 million before) -- the stated contract
delta.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; every
document is a 1600x1200 white `new_document`):
T1  16-bit unversioned `brush_stroke` size 64 hardness 100 #000000
    (300,600)->(1300,600): ok, reply `coverage` says "generated from points";
    black (800,600), white (800,400). A twin 16-bit document takes the same
    call plus `stroke_version: 1`; mean_rgba_codes at six points are EQUAL.
T2  16-bit unversioned `brush_stroke` size 4 hardness 50, 41 points
    alternating (100,600)/(1500,600) (>= 56,001 dabs at 1-px spacing):
    isError "50,000 dabs"; white at (800,600), (100,600), (1500,600);
    `get_document` 1600x1200.
T3  16-bit `add_layer_mask` layer 0, `eraser_stroke` target mask, T1's chord
    size 64: ok, "generated from points"; alpha 0 at (800,600), 65535 at
    (800,400).
T4  16-bit unversioned `brush_stroke` size 512, hardness omitted, 451 points
    alternating (100,600)/(1500,600): 4,501 dabs (<< 50,000), charge
    >= 4,501 x 474,688 ~ 2.14e9 > 2e9 (the cap): isError "2 billion
    footprint work units" and NOT "50,000"; white at (800,600), (100,600);
    1600x1200. Refused by the plan: nothing is rasterized.
T4b T4's call as `eraser_stroke` target mask after `add_layer_mask`: the same
    sentence; alpha 65535 at (800,600).
B1  8-bit, T2's call: isError "50,000 dabs"; white at T2's three points.
B2  8-bit `dodge_burn` and `spot_heal_stroke` with T2's geometry, hardness 50:
    both isError "50,000 dabs"; white unchanged.
B3  8-bit `brush_stroke` T1's chord hardness 50: ok; black (800,600), white
    (800,400).
Q1  16-bit Quick Mask on, `quick_mask_stroke` stroke_version 1, 1600x1200,
    size 4 hardness 50, T2's points: isError "50,000 dabs"; `get_selection`
    unchanged.
C1  16-bit `brush_stroke` with a built `native_stroke` of 50,001 dabs:
    isError "50,000 dabs"; white. A transport rejection of the ~2.1 MB body
    is recorded as a MISS with the transport status, never shrunk.
A failure exits nonzero; the JSON receipt is written to --receipts either way.
"""
import argparse
import base64
import json
import math
from pathlib import Path
import struct
import sys
import traceback

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

WIDTH, HEIGHT = 1600, 1200
DABS = '50,000 dabs'
WORK = '2 billion footprint work units'
GENERATED = 'generated from points'
CAP = 2_000_000_000
MARGIN = 2.0 ** -16
CHORD = [[300, 600], [1300, 600]]
ALTERNATING_41 = [[100, 600] if k % 2 == 0 else [1500, 600] for k in range(41)]
ALTERNATING_451 = [[100, 600] if k % 2 == 0 else [1500, 600] for k in range(451)]
T1_PROBES = [(269, 600), (800, 568), (800, 600), (800, 632), (1331, 600), (500, 610)]


def rpc(app, name, **arguments):
    return app.client.rpc('tools/call', {'name': name, 'arguments': arguments})


def text_of(reply):
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def band_bound(diameter):
    a = diameter / 2
    rho = math.sqrt(2) + MARGIN * a
    return math.ceil(4 * math.pi * a * rho + math.pi * rho * rho)


def stamps(points, spacing):
    """The walk's own stamp count: the first point, then one per spacing
    along each segment from the running anchor."""
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


def sample(app, doc, x, y):
    return app.call('sample_pixel', document_id=doc, x=x, y=y, sample_size=1)


def rgba(found):
    codes = found.get('mean_rgba_codes')
    if codes is not None:
        return [round(v) for v in codes], 65535
    return [found[k] for k in ('r', 'g', 'b', 'a')], 255


def probe(app, doc, label, black=(), white=()):
    found = {}
    for (x, y) in black:
        codes, _ = rgba(sample(app, doc, x, y))
        found[f'{x},{y}'] = codes
        assert codes[:3] == [0, 0, 0], f'{label}: ({x}, {y}) is not black: {codes}'
    for (x, y) in white:
        codes, top = rgba(sample(app, doc, x, y))
        found[f'{x},{y}'] = codes
        assert codes == [top] * 4, f'{label}: ({x}, {y}) is not white: {codes}'
    return found


def alpha(app, doc, label, x, y, expected):
    codes, _ = rgba(sample(app, doc, x, y))
    assert codes[3] == expected, f'{label}: ({x}, {y}) alpha {codes[3]} != {expected}'
    return codes


def document(app, depth):
    created = app.call('new_document', width=WIDTH, height=HEIGHT, depth=depth,
                       background='white')
    return created['document']['id']


def dimensions(app, label, doc):
    summary = app.call('get_document', document_id=doc)
    assert (summary['width'], summary['height']) == (WIDTH, HEIGHT), f'{label}: {summary}'
    return [summary['width'], summary['height']]


def succeeds(label, reply, generated=False):
    text = text_of(reply)
    assert not reply.get('isError'), f'{label} refused: {text}'
    if generated:
        assert GENERATED in text, f'{label}: "{GENERATED}" not in {text}'
    return text


def refuses(label, reply, sentence, absent=None):
    text = text_of(reply)
    assert reply.get('isError'), f'{label} was not refused: {text}'
    assert sentence in text, f'{label} refused without "{sentence}": {text}'
    if absent is not None:
        assert absent not in text, f'{label}: "{absent}" in {text}'
    return text


def t1(app):
    doc = document(app, 16)
    call = dict(size=64, hardness=100, color='#000000', points=CHORD)
    text = succeeds('T1', rpc(app, 'brush_stroke', document_id=doc, **call), generated=True)
    samples = probe(app, doc, 'T1', black=[(800, 600)], white=[(800, 400)])
    twin = document(app, 16)
    twin_text = succeeds('T1 twin', rpc(app, 'brush_stroke', document_id=twin,
                                        stroke_version=1, **call))
    compared = {}
    for (x, y) in T1_PROBES:
        a = sample(app, doc, x, y)['mean_rgba_codes']
        b = sample(app, twin, x, y)['mean_rgba_codes']
        compared[f'{x},{y}'] = [a, b]
        assert a == b, f'T1: ({x}, {y}) unversioned {a} != versioned {b}'
    return {'document': doc, 'twin': twin, 'reply': text[:400], 'twin_reply': twin_text[:400],
            'samples': samples, 'compared': compared}


def t2(app):
    doc = document(app, 16)
    dabs = stamps(ALTERNATING_41, 1)
    assert dabs >= 56_001, dabs
    text = refuses('T2', rpc(app, 'brush_stroke', document_id=doc, size=4, hardness=50,
                             color='#000000', points=ALTERNATING_41), DABS)
    return {'document': doc, 'walk_dabs': dabs, 'reply': text,
            'samples': probe(app, doc, 'T2', white=[(800, 600), (100, 600), (1500, 600)]),
            'dimensions': dimensions(app, 'T2', doc)}


def t3(app):
    doc = document(app, 16)
    app.call('add_layer_mask', document_id=doc, layer=0)
    text = succeeds('T3', rpc(app, 'eraser_stroke', document_id=doc, target='mask', size=64,
                              points=CHORD), generated=True)
    return {'document': doc, 'reply': text[:400],
            'hidden': alpha(app, doc, 'T3', 800, 600, 0),
            'shown': alpha(app, doc, 'T3', 800, 400, 65535)}


def t4_bound():
    dabs = stamps(ALTERNATING_451, 512 * 25 / 100)
    per = 356 * 512 + 64 * band_bound(512)
    assert band_bound(512) == 4_569, band_bound(512)
    assert per == 474_688, per
    assert dabs < 50_000 and dabs * per > CAP, (dabs, per)
    return dabs, per


def t4(app):
    dabs, per = t4_bound()
    doc = document(app, 16)
    text = refuses('T4', rpc(app, 'brush_stroke', document_id=doc, size=512, color='#000000',
                             points=ALTERNATING_451), WORK, absent='50,000')
    return {'document': doc, 'walk_dabs': dabs, 'per_dab_lower_bound': per,
            'charge_lower_bound': dabs * per, 'reply': text,
            'samples': probe(app, doc, 'T4', white=[(800, 600), (100, 600)]),
            'dimensions': dimensions(app, 'T4', doc)}


def t4b(app):
    doc = document(app, 16)
    app.call('add_layer_mask', document_id=doc, layer=0)
    text = refuses('T4b', rpc(app, 'eraser_stroke', document_id=doc, target='mask', size=512,
                              points=ALTERNATING_451), WORK, absent='50,000')
    return {'document': doc, 'reply': text, 'shown': alpha(app, doc, 'T4b', 800, 600, 65535)}


def b1(app):
    doc = document(app, 8)
    text = refuses('B1', rpc(app, 'brush_stroke', document_id=doc, size=4, hardness=50,
                             color='#000000', points=ALTERNATING_41), DABS)
    return {'document': doc, 'reply': text,
            'samples': probe(app, doc, 'B1', white=[(800, 600), (100, 600), (1500, 600)])}


def b2(app):
    doc = document(app, 8)
    white = [(800, 600), (100, 600), (1500, 600)]
    dodge = refuses('B2 dodge_burn', rpc(app, 'dodge_burn', document_id=doc, size=4,
                                         hardness=50, burn=True, points=ALTERNATING_41), DABS)
    after_dodge = probe(app, doc, 'B2 dodge_burn', white=white)
    spot = refuses('B2 spot_heal_stroke', rpc(app, 'spot_heal_stroke', document_id=doc, size=4,
                                              hardness=50, points=ALTERNATING_41), DABS)
    return {'document': doc, 'dodge_reply': dodge, 'spot_reply': spot,
            'after_dodge': after_dodge, 'samples': probe(app, doc, 'B2 spot_heal_stroke',
                                                         white=white)}


def b3(app):
    doc = document(app, 8)
    text = succeeds('B3', rpc(app, 'brush_stroke', document_id=doc, size=64, hardness=50,
                              color='#000000', points=CHORD))
    return {'document': doc, 'reply': text[:400],
            'samples': probe(app, doc, 'B3', black=[(800, 600)], white=[(800, 400)])}


def q1(app):
    doc = document(app, 16)
    app.call('set_quick_mask', document_id=doc, active=True)
    points = [{'x': 800, 'y': 600}, {'x': 100, 'y': 600}, {'x': 800, 'y': 400}]
    before = app.call('get_selection', document_id=doc, points=points)
    text = refuses('Q1', rpc(app, 'quick_mask_stroke', document_id=doc, stroke_version=1,
                             width=WIDTH, height=HEIGHT, size=4, hardness=50,
                             points=ALTERNATING_41), DABS)
    after = app.call('get_selection', document_id=doc, points=points)
    assert after == before, f'Q1: selection changed {before} -> {after}'
    return {'document': doc, 'reply': text, 'selection': after}


def c1(app):
    doc = document(app, 16)
    count = 50_001
    payload = struct.pack('<4d', 800.0, 600.0, 4.0, 1.0) * count
    canonical = {'version': 1, 'encoding': 'le_f64_xy_diameter_opacity',
                 'dabs': base64.b64encode(payload).decode('ascii'), 'hardness': 0.5,
                 'roundness': 1.0, 'angle_degrees': 0.0, 'pixel': False}
    receipt = {'document': doc, 'dabs': count, 'body_base64_bytes': len(canonical['dabs'])}
    try:
        reply = rpc(app, 'brush_stroke', document_id=doc, color='#000000', size=4,
                    native_stroke=canonical)
    except RuntimeError as error:
        receipt['miss'] = f'transport rejected the body: {error}'
        return receipt
    except OSError as error:
        receipt['miss'] = f'transport rejected the body: {error!r}'
        return receipt
    receipt['reply'] = refuses('C1', reply, DABS)
    receipt['samples'] = probe(app, doc, 'C1', white=[(800, 600)])
    return receipt


GATES = [
    ('T1', t1, 'unversioned 16-bit points equal the stroke_version 1 twin exactly'),
    ('T2', t2, 'a 16-bit unversioned over-dab-cap stroke refuses by name; document white'),
    ('T3', t3, 'an unversioned 16-bit mask eraser stroke generates native dabs'),
    ('T4', t4, 'a within-dab-cap 16-bit stroke refuses by the core work-cap sentence'),
    ('T4b', t4b, 'the mask-target twin refuses by the same sentence; mask unchanged'),
    ('B1', b1, 'the 8-bit byte walker refuses by name; document white'),
    ('B2', b2, '8-bit dodge_burn and spot_heal_stroke refuse by name; document white'),
    ('B3', b3, 'the 8-bit within-cap control paints'),
    ('Q1', q1, 'Quick Mask over the dab cap refuses by name; selection unchanged'),
    ('C1', c1, 'a 50,001-dab canonical stroke refuses by name'),
]


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
    failed = False
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['mcp_socket'] = app.client.socket_path
            for label, gate, summary in GATES:
                try:
                    receipts[label] = gate(app)
                except Exception:
                    failed = True
                    receipts[label] = {'failure': traceback.format_exc()}
                    print(f'FAIL {label}', receipts[label]['failure'], flush=True)
                    continue
                if 'miss' in receipts[label]:
                    print(f'MISS {label} {receipts[label]["miss"]}', flush=True)
                else:
                    print(f'PASS {label} {summary}', flush=True)
    except Exception:
        failed = True
        receipts['failure'] = traceback.format_exc()
        print('FAIL', receipts['failure'], flush=True)
    (folder / 'phase13a6c2.json').write_text(json.dumps(receipts, indent=2, sort_keys=True))
    print(f'receipt: {folder / "phase13a6c2.json"}', flush=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
