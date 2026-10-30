#!/usr/bin/env python3
"""Owned-app Phase 13A-6b drive: native artwork coverage by an active-edge
sweep bounded by the ink's rows, so a tall text layer whose outline the
pre-13A-6b full scan charged past the 256M work cap (E x H) finishes at full
resolution with every cap unchanged. The beyond-cap PROOF is
core/tests/bounded_artwork_coverage.rs; this drive is the app-path gate.

Gates (fixed in designs/phase-13a-regional-status.md before measuring):
B1: a 16-bit 240x16400 white `new_document`; `add_text_layer` of 40 lines of
"S" joined by "\\n" at x 40, y 100, size 40, Helvetica, leading 400 (design:
~40 x 2,500 flattened edges ~ 1e5 <= 262,144 in ONE part over a ~16,000-row
extent, E x H ~ 1.6e9 > 256M under the old charge) succeeds with no
"work limit"/"edge limit" in the reply; `get_document` is 240x16400; the new
layer's height >= 15,800; for lines 1, 20 and 40, `sample_pixel`
(sample_size 5) every 8 px down the line's 400-px box at x 51 finds at least
one non-white sample.
B2: the 8-bit twin (the byte fallback), same gates.
B3: within-cap control: 16-bit 240x2600, 6 lines, lines 1, 3 and 6 probed.
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

WIDTH = 240
TOP = 100
LEADING = 400
PROBE_X = 51
REFUSALS = ('work limit', 'edge limit')


def rpc(app, name, **arguments):
    return app.client.rpc('tools/call', {'name': name, 'arguments': arguments})


def text_of(reply):
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def non_white(sample):
    """True when the averaged sample is not pure white at its own depth."""
    codes = sample.get('mean_rgba_codes')
    if codes is not None:
        return any(v < 65535 for v in codes[:3])
    return any(sample[k] < 255 for k in ('r', 'g', 'b'))


def gate(app, label, height, depth, lines, probed, minimum=None):
    created = app.call('new_document', width=WIDTH, height=height, depth=depth,
                       background='white')
    document = created['document']['id']
    reply = rpc(app, 'add_text_layer', document_id=document, text='\n'.join(['S'] * lines),
                x=40, y=TOP, size=40, font='Helvetica', leading=LEADING)
    text = text_of(reply)
    assert not reply.get('isError'), f'{label} add_text_layer refused: {text}'
    for phrase in REFUSALS:
        assert phrase not in text, f'{label}: {phrase!r} in {text}'
    added = json.loads(text)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == (WIDTH, height), summary
    layer_height = added['bounds']['height']
    if minimum is not None:
        assert layer_height >= minimum, f'{label}: layer height {layer_height} < {minimum}'
    ink = {}
    for line in probed:
        box = TOP + (line - 1) * LEADING
        hits = []
        for y in range(box, box + LEADING, 8):
            sample = app.call('sample_pixel', document_id=document, x=PROBE_X, y=y,
                              sample_size=5)
            if non_white(sample):
                hits.append(y)
        assert hits, f'{label}: no ink found down line {line} ({box}..{box + LEADING}) at x {PROBE_X}'
        ink[str(line)] = {'box': [box, box + LEADING], 'non_white_rows': hits}
    return {'document': document, 'depth': depth, 'width': summary['width'],
            'height': summary['height'], 'layer': added['layer'], 'bounds': added['bounds'],
            'lines': lines, 'ink': ink}


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
            receipts['B1'] = gate(app, 'B1', 16400, 16, 40, (1, 20, 40), 15800)
            print('PASS B1 a 40-line 16-bit 240x16400 text layer finishes at full resolution '
                  'with ink on lines 1, 20 and 40', flush=True)
            receipts['B2'] = gate(app, 'B2', 16400, 8, 40, (1, 20, 40), 15800)
            print('PASS B2 the 8-bit twin finishes with ink on lines 1, 20 and 40', flush=True)
            receipts['B3'] = gate(app, 'B3', 2600, 16, 6, (1, 3, 6), 2400)
            print('PASS B3 the within-cap 16-bit 240x2600 control finishes with ink on lines '
                  '1, 3 and 6', flush=True)
    except Exception:
        failed = True
        receipts['failure'] = traceback.format_exc()
        print('FAIL', receipts['failure'], flush=True)
    (folder / 'phase13a6b.json').write_text(json.dumps(receipts, indent=2, sort_keys=True))
    print(f'receipt: {folder / "phase13a6b.json"}', flush=True)
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
