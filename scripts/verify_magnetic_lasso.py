#!/usr/bin/env python3
"""The Magnetic Lasso over MCP (`select_magnetic`) on an owned Debug instance: the traced outline
sits on the edge (the phase-6 ADR's "Vertex placement", designs/phase-6-selection-mask-status.md)
and the `freehand` points join the outline as given ("Magnetic Lasso input").

The expectations are FIXED here before anything is measured, and the oracle is the drawn picture:
an 8-bit 120 × 90 document, white, with a black rectangle over pixels x 30–89, y 20–69 (its sides
on x = 30, x = 90, y = 20, y = 70). The instance is owned by this file (`phase5_support.App`); no
Doctored process it did not start is signalled.

- **L1 on the edge.** `select_magnetic` round the rectangle, starting inside a left-side pixel off
  its boundary ([30.3, 45.7]) with moves 3 px outside it, default options, closed magnetically:
  every pixel whose centre lies more than 2 px from a corner of the rectangle is fully selected
  inside it and unselected outside, so no side column or row is partly covered. The corner pixels'
  coverage is reported, not judged (a corner is cut by a fraction of a pixel).
- **L2 freehand.** The same pointer path with a run of three points 7 px above the top side
  (within Width of it): listed in `freehand`, they are joined as given, so the band between y = 13
  and the top side over x 52–68 is selected and the rows above y = 13 are not; without `freehand`
  the same points snap to the top side and the band is unselected.
- **L3 refusals.** `freehand` holding index 0, a non-increasing list, an index also in `anchors`,
  or an index past the last point each answers in-band and leaves the history unchanged.

Usage: python3 -B scripts/verify_magnetic_lasso.py [--receipts <folder>]
"""
import argparse
import json
import math
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 120, 90
RECT = (30, 20, 60, 50)  # x, y, width, height in pixels
CORNERS = [(30.0, 20.0), (90.0, 20.0), (90.0, 70.0), (30.0, 70.0)]
CORNER_RADIUS = 2.0
FULL = 65535
START = [30.3, 45.7]
# Clockwise 3 px outside the rectangle, from the start up the left side and round.
MOVES = [[27, 35], [27, 25], [27, 17], [40, 17], [60, 17], [80, 17], [93, 17], [93, 30],
         [93, 45], [93, 60], [93, 73], [80, 73], [60, 73], [40, 73], [27, 73], [27, 60]]
# L2: after the move to (40, 17), three points 7 px above the top side, then on round.
BUMP = [[48, 13], [72, 13], [75, 17]]
BAND = [(x, y) for y in range(14, 20) for x in range(52, 69)]
ABOVE = [(x, y) for y in range(8, 13) for x in range(52, 69)]
REFUSALS = {
    'index 0': ({'freehand': [0]}, 'freehand must be integer point indices from 1 to'),
    'not increasing': ({'freehand': [5, 4]}, 'freehand must be strictly increasing'),
    'also an anchor': ({'freehand': [4, 5], 'anchors': [5]},
                       'a point cannot be both an anchor and freehand'),
    'past the end': ({'freehand': [99]}, 'freehand must be integer point indices from 1 to'),
}


def inside_rect(x, y):
    x0, y0, w, h = RECT
    return x0 <= x < x0 + w and y0 <= y < y0 + h


def near_corner(x, y):
    cx, cy = x + 0.5, y + 0.5
    return any(math.hypot(cx - kx, cy - ky) <= CORNER_RADIUS for kx, ky in CORNERS)


def coverage(call, doc, pixels):
    """coverage16 of each pixel, read 4096 at a time (get_selection's bound)."""
    out = []
    for at in range(0, len(pixels), 4096):
        chunk = pixels[at:at + 4096]
        reply = call('get_selection', document_id=doc,
                     points=[{'x': x, 'y': y} for x, y in chunk])
        samples = reply.get('samples', [])
        assert len(samples) == len(chunk), ('samples', len(samples), len(chunk))
        out.extend(s['coverage16'] for s in samples)
    return out


def picture(call):
    doc = call('new_document', width=W, height=H, background='white')['document']['id']
    x, y, w, h = RECT
    call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
    call('fill_selection', document_id=doc, color='#000000')
    call('deselect', document_id=doc)
    return doc


def history(call, doc):
    reply = call('list_history', document_id=doc)
    return (len(reply['states']), reply['cursor'])


def check_edge(call):
    doc = picture(call)
    reply = call('select_magnetic', document_id=doc, points=[START] + MOVES)
    every = [(x, y) for y in range(H) for x in range(W)]
    got = dict(zip(every, coverage(call, doc, every)))
    wrong = [(x, y, v) for (x, y), v in got.items()
             if not near_corner(x, y) and v != (FULL if inside_rect(x, y) else 0)]
    partial_sides = sorted({('column', x) if y in range(22, 68) else ('row', y)
                            for x, y, v in wrong if 0 < v < FULL})
    corners = {f'{x},{y}': got[(x, y)] for (x, y) in every if near_corner(x, y) and got[(x, y)]}
    call('close_document', document_id=doc, discard_changes=True)
    result = {'outline_points': reply.get('outline_points'), 'wrong': len(wrong),
              'first wrong': wrong[:12], 'partly covered sides': partial_sides[:12],
              'corner coverage': corners}
    result['verdict'] = 'PASS' if not wrong else 'FAIL: pixels away from the corners are wrong'
    return result


def bump_points():
    """[START] + MOVES with BUMP after the move to (40, 17); BUMP's indices."""
    head = MOVES[:MOVES.index([40, 17]) + 1]
    tail = MOVES[len(head):]
    points = [START] + head + BUMP + tail
    first = 1 + len(head)
    return points, list(range(first, first + len(BUMP)))


def check_freehand(call):
    points, indices = bump_points()
    out = {}
    for name, extra in (('freehand', {'freehand': indices}), ('traced', {})):
        doc = picture(call)
        reply = call('select_magnetic', document_id=doc, points=points, **extra)
        band = coverage(call, doc, BAND)
        above = coverage(call, doc, ABOVE)
        call('close_document', document_id=doc, discard_changes=True)
        out[name] = {'outline_points': reply.get('outline_points'),
                     'band full': sum(v == FULL for v in band), 'band empty': sum(v == 0 for v in band),
                     'above empty': sum(v == 0 for v in above)}
    ok = (out['freehand']['band full'] == len(BAND) and out['freehand']['above empty'] == len(ABOVE)
          and out['traced']['band empty'] == len(BAND))
    out['verdict'] = 'PASS' if ok else 'FAIL: the freehand run is not joined as given'
    return out


def check_refusals(call):
    doc = picture(call)
    points, _ = bump_points()
    before = history(call, doc)
    out = {}
    for name, (extra, sentence) in REFUSALS.items():
        text = call('select_magnetic', fail=True, document_id=doc, points=points, **extra)
        held = history(call, doc) == before
        out[name] = 'PASS' if sentence in text and held else f'FAIL: {text!r}, history held {held}'
    call('close_document', document_id=doc, discard_changes=True)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='magnetic-lasso-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_magnetic_lasso.py'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        for name, check in (('L1 on the edge', check_edge), ('L2 freehand', check_freehand),
                            ('L3 refusals', check_refusals)):
            try:
                receipts[name] = check(call)
            except AssertionError as error:
                receipts[name] = {'verdict': f'FAIL: {error}'}
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    print(json.dumps(receipts, indent=2))
    failed = [f'{section}: {case}' for section, value in receipts.items() if isinstance(value, dict)
              for case, result in value.items() if str(result).startswith('FAIL')]
    if failed:
        print('FAIL: ' + '; '.join(failed))
        sys.exit(1)
    print('PASS: select_magnetic outlines a hard-edged rectangle on its sides, joins freehand '
          'points as given, and refuses a malformed freehand list in-band')


if __name__ == '__main__':
    main()
