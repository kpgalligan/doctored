#!/usr/bin/env python3
"""MCP drive for Select > Focus Area (`select_focus_area`, designs/focus-area.md)
at 8, 16 and 32 bits.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process.

The fixture is a 96x64 opaque picture: the left half a one-pixel checker of
two greys (sharp), the right half the same checker box-blurred over 7x7 here
(defocused). At each depth (16 and 32 bits by `convert_precision`):
- Auto selects the sharp half: coverage >= 250 left of the seam band and 0
  right of it (`get_selection` on a grid), the reply's bounds start at x 0 and
  end left of the band's far side, and the reply carries `range`;
- the same call with that `range` gives the same coverage (the recorded
  step's replay);
- one `undo` removes the selection;
- `mode add` keeps a rectangle selected first on the blurred half;
- a Grayscale document refuses.

Usage: python3 scripts/verify_focus_area.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_lens import App  # noqa: E402

W, H = 96, 64
SEAM = 24


def checker(x, y):
    return 60 if (x + y) % 2 == 0 else 190


def blurred(x, y):
    total = n = 0
    for dy in range(-3, 4):
        for dx in range(-3, 4):
            total += checker(min(max(x + dx, 0), W - 1), min(max(y + dy, 0), H - 1))
            n += 1
    return (total + n // 2) // n


def write(folder):
    data = bytearray()
    for y in range(H):
        for x in range(W):
            v = checker(x, y) if x < W // 2 else blurred(x, y)
            data += bytes([v, v, v, 255])
    path = Path(folder) / 'half-sharp.png'
    png(path, W, H, bytes(data))
    return path


GRID = [dict(x=x, y=y) for y in range(0, H, 3) for x in range(0, W, 3)]


def coverage(app, document):
    samples = app.call('get_selection', document_id=document, points=GRID)['samples']
    return {(s['x'], s['y']): s['coverage'] for s in samples}


def check_half(cover, label):
    for (x, y), value in cover.items():
        if x < W // 2 - SEAM:
            assert value >= 250, (label, 'sharp', x, y, value)
        elif x >= W // 2 + SEAM:
            assert value == 0, (label, 'blurred', x, y, value)


def depth_case(app, path, depth):
    document = app.call('open_document', path=str(path))['document']['id']
    try:
        return depth_checks(app, document, depth)
    finally:
        # Closed whatever happened, so the next case opens the file afresh.
        app.call('close_document', document_id=document, discard_changes=True)


def depth_checks(app, document, depth):
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if depth == 32:
        app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    reply = app.call('select_focus_area', document_id=document)
    assert reply['selection_empty'] is False, reply
    bounds = reply['bounds']
    assert bounds['x'] == 0 and bounds['x'] + bounds['width'] <= W // 2 + SEAM, bounds
    auto = coverage(app, document)
    check_half(auto, f'{depth} auto')
    app.call('undo', document_id=document)
    left = [point for point, v in coverage(app, document).items() if v]
    assert not left, ('undo left a selection', left[:5])
    replay = app.call('select_focus_area', document_id=document, range=reply['range'])
    assert replay['range'] == reply['range'], (reply, replay)
    assert coverage(app, document) == auto, 'the range did not replay Auto'
    app.call('select_rect', document_id=document, x=W - 10, y=0, width=10, height=10)
    app.call('select_focus_area', document_id=document, mode='add')
    combined = coverage(app, document)
    assert combined[(W - 6, 3)] == 255 and combined[(3, 3)] >= 250, 'add lost a part'
    return dict(range=reply['range'], bounds=bounds)


def refusal_case(app, path):
    document = app.call('open_document', path=str(path))['document']['id']
    app.call('convert_mode', document_id=document, mode='gray')
    text = app.refused('select_focus_area', '', document_id=document)
    app.call('close_document', document_id=document, discard_changes=True)
    return text[:80]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='keep the run folder here')
    args = parser.parse_args()
    folder = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix='rz-focus-'))
    folder.mkdir(parents=True, exist_ok=True)
    path = write(folder)
    failures = 0
    with App(folder / 'instance', 2 * 1024 ** 3) as app:
        cases = [(f'select_focus_area {depth}-bit', lambda depth=depth: depth_case(app, path, depth))
                 for depth in (8, 16, 32)]
        cases.append(('select_focus_area refused in Grayscale', lambda: refusal_case(app, path)))
        for name, run in cases:
            try:
                detail = run()
                print(f'PASS {name}: {detail}', flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports
                failures += 1
                print(f'FAIL {name}: {error!r}', flush=True)
    if failures:
        print(f'{failures} CASE(S) FAILED; the run folder is {folder}')
        return 1
    print('ALL CASES PASS')
    return 0


if __name__ == '__main__':
    sys.exit(main())
