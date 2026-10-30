#!/usr/bin/env python3
"""Owned-app Phase 13A-6a drive: typed (16-bit) Modify Selection over the
selection's SUPPORT grown by the kernel's reach, so a 250-px feather of a small
selection finishes on a canvas past the old work cap at the canvas's original
dimensions. (The typed selection kinds now have no work cap at all: they
run as cancellable jobs. The gates below still hold; the refusal they name is
the one 13A-6a removed for a small support.)

Gates (fixed in designs/phase-13a-regional-status.md before measuring):
B1: a 16-bit 2048x1024 `new_document`, `select_rect` (500, 400, 100x100),
`modify_selection feather 250` succeeds — before 13A-6a it refused "native
selection exceeds the work limit" (2,097,152 px x 1002 > 2e9) — and
`get_document` reports width/height unchanged.
B2: the same rectangle and feather on a 16-bit 1024x1024 document (within the
cap: 1,048,576 x 1002 < 2e9): `get_selection` `coverage16` at a 64x64 point
grid over the window [250, 850) x [150, 750) is EQUAL between B1 and B2, and 0
at points outside the window on both.
B3: B1/B2 with `smooth 250`.
B4: an 8-bit 2048x1024 control: the byte `feather 250` succeeds as before and
`coverage_storage` is "u8".
A failure exits nonzero; the JSON receipt is written to --receipts either way.
"""
import argparse
import json
from pathlib import Path
import sys
import traceback

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

RECT = dict(x=500, y=400, width=100, height=100)
WINDOW = (250, 150, 850, 750)
SENTENCE = 'native selection exceeds the work limit'
# A 64x64 grid over the window (4096 points, the tool's maximum).
GRID = [{'x': WINDOW[0] + (i * (WINDOW[2] - WINDOW[0])) // 64,
         'y': WINDOW[1] + (j * (WINDOW[3] - WINDOW[1])) // 64}
        for j in range(64) for i in range(64)]
# Points outside the window that lie on both canvases.
OUTSIDE = [{'x': 249, 'y': 450}, {'x': 850, 'y': 450}, {'x': 550, 'y': 149},
           {'x': 550, 'y': 750}, {'x': 0, 'y': 0}, {'x': 100, 'y': 100},
           {'x': 900, 'y': 500}, {'x': 1000, 'y': 1000}, {'x': 1023, 'y': 1023}]


def rpc(app, name, **arguments):
    return app.client.rpc('tools/call', {'name': name, 'arguments': arguments})


def text_of(reply):
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def modified(app, width, height, depth, operation):
    """A new document, the rectangle, the modification; the raw reply."""
    created = app.call('new_document', width=width, height=height, depth=depth,
                       background='white')
    document = created['document']['id']
    app.call('select_rect', document_id=document, **RECT)
    reply = rpc(app, 'modify_selection', document_id=document, operation=operation,
                radius=250)
    return document, reply


def samples(app, document, points):
    reply = app.call('get_selection', document_id=document, points=points)
    return reply, [row['coverage16'] for row in reply['samples']]


def native(app, operation):
    """B1 (2048x1024) and B2 (1024x1024) for one operation, compared."""
    out = {}
    series = {}
    for label, (width, height) in (('B1', (2048, 1024)), ('B2', (1024, 1024))):
        document, reply = modified(app, width, height, 16, operation)
        text = text_of(reply)
        assert not reply.get('isError'), f'{label} {operation} refused: {text}'
        assert SENTENCE not in text, text
        summary = app.call('get_document', document_id=document)
        assert (summary['width'], summary['height']) == (width, height), summary
        state, grid = samples(app, document, GRID)
        _, outside = samples(app, document, OUTSIDE)
        assert state['coverage_storage'] == 'u16', state
        assert all(v == 0 for v in outside), (label, outside)
        assert any(v > 0 for v in grid), f'{label} {operation}: an empty window'
        series[label] = grid
        out[label] = {'document': document, 'reply': json.loads(text),
                      'width': summary['width'], 'height': summary['height'],
                      'coverage_storage': state['coverage_storage'],
                      'bounds': state.get('bounds'), 'outside': outside,
                      'grid_nonzero': sum(1 for v in grid if v > 0),
                      'grid_sum': sum(grid), 'center': grid[32 * 64 + 32]}
    differences = [i for i, (a, b) in enumerate(zip(series['B1'], series['B2'])) if a != b]
    assert not differences, (f'{operation}: B1 and B2 differ at {len(differences)} grid points, '
                             f'first {GRID[differences[0]]}: {series["B1"][differences[0]]} vs '
                             f'{series["B2"][differences[0]]}')
    out['equal_points'] = len(GRID)
    return out


def b4(app):
    document, reply = modified(app, 2048, 1024, 8, 'feather')
    text = text_of(reply)
    assert not reply.get('isError'), f'B4 byte feather refused: {text}'
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == (2048, 1024), summary
    state = app.call('get_selection', document_id=document)
    assert state['coverage_storage'] == 'u8', state
    return {'document': document, 'reply': json.loads(text),
            'coverage_storage': state['coverage_storage'], 'bounds': state.get('bounds')}


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
            feather = native(app, 'feather')
            receipts['B1'] = feather['B1']
            print('PASS B1 a 250-px feather of a 100x100 selection finishes on a 16-bit '
                  '2048x1024 canvas at its original dimensions', flush=True)
            receipts['B2'] = {'within_cap': feather['B2'],
                              'equal_points': feather['equal_points']}
            print('PASS B2 coverage16 over the 64x64 window grid equals the within-cap '
                  '1024x1024 twin; 0 outside the window on both', flush=True)
            receipts['B3'] = native(app, 'smooth')
            print('PASS B3 smooth 250: beyond-cap and within-cap twins equal over the window '
                  'grid; 0 outside', flush=True)
            receipts['B4'] = b4(app)
            print('PASS B4 the 8-bit 2048x1024 byte feather 250 succeeds; coverage_storage '
                  'u8', flush=True)
    except Exception:
        failed = True
        receipts['failure'] = traceback.format_exc()
        print('FAIL', receipts['failure'], flush=True)
    (folder / 'phase13a6a.json').write_text(json.dumps(receipts, indent=2, sort_keys=True))
    print(f'receipt: {folder / "phase13a6a.json"}', flush=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
