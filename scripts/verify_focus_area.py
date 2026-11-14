#!/usr/bin/env python3
"""MCP drive for Select > Focus Area (`select_focus_area`, designs/focus-area.md)
at 8, 16 and 32 bits, in RGB and in the colour modes that answer it.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process.

The fixture is a 96x64 opaque picture: the left half a one-pixel checker of
two greys (sharp), the right half the same checker box-blurred over 7x7 here
(defocused). Each case opens it, converts it (`prepare`: 16 and 32 bits by
`convert_precision`, then the mode by `convert_mode`; Indexed Color with
`palette exact`, `dither none`, so the indices are the picture), and:
- Auto selects the sharp half: coverage >= 250 left of the seam band and 0
  right of it (`get_selection` on a grid), the reply's bounds start at x 0 and
  end left of the band's far side, and the reply carries `range`;
- the same call with that `range` gives the same coverage (the recorded
  step's replay);
- one `undo` removes the selection;
- `mode add` keeps a rectangle selected first on the blurred half;
- in Grayscale, Lab Color and Indexed Color `get_precision` lists
  `showFocusArea:` in `supported_menu_items`, and in CMYK `select_focus_area`
  in `supported_tools`.
The cases are RGB at 8, 16 and 32 bits, Grayscale at 8, 16 and 32, Lab Color
at 8 and 16, Indexed Color at 8 and CMYK at 8 and 16. Duotone refuses with
`DUOTONE_REFUSAL` and Bitmap with `BITMAP_REFUSAL` (decision 7).

`mode_check` is the one call each colour-mode drive makes (`verify_gray_mode.py`,
`verify_lab_mode.py`, `verify_indexed_mode.py`, `verify_cmyk_mode.py`).

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
DUOTONE_REFUSAL = 'select_focus_area is not available in Duotone.'
BITMAP_REFUSAL = ('select_focus_area is unavailable in a Bitmap document: Doctored offers it on '
                  'Grayscale and colour documents only; convert to Grayscale first.')
MONOTONE = [{'name': 'Black', 'lab': [0, 0, 0], 'curve': [0] + [None] * 11 + [100]}]
# The modes whose `get_precision` lists its menu items.
MENU_MODES = ('gray', 'lab', 'indexed')


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


def color_mode(app, document):
    rows = app.call('list_documents')['documents']
    return next(row for row in rows if row['id'] == document)['color_mode']


def prepare(app, document, depth, mode):
    """The opened RGB picture at `depth`, then in `mode` (None keeps RGB)."""
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if depth == 32:
        app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    if mode == 'indexed':
        app.call('convert_mode', document_id=document, mode='indexed', palette='exact',
                 dither='none')
    elif mode == 'bitmap':
        app.call('convert_mode', document_id=document, mode='gray')
        app.call('convert_mode', document_id=document, mode='bitmap', method='threshold')
    elif mode == 'duotone':
        app.call('convert_mode', document_id=document, mode='gray')
        app.call('convert_mode', document_id=document, mode='duotone', inks=MONOTONE)
    elif mode:
        app.call('convert_mode', document_id=document, mode=mode)
    got = color_mode(app, document)
    assert got == (mode or 'rgb'), (mode, depth, got)


def answers(app, document, label):
    """One Auto call: a non-empty selection whose bounds start at x 0 and end left of the
    seam band's far side. Answers the reply."""
    reply = app.call('select_focus_area', document_id=document)
    assert reply['selection_empty'] is False, (label, reply)
    bounds = reply['bounds']
    assert bounds['x'] == 0 and bounds['x'] + bounds['width'] <= W // 2 + SEAM, (label, bounds)
    return reply


def listed(app, document, mode):
    """`get_precision` admits Focus Area by name: the menu item where the mode lists its menu
    items, else the tool."""
    precision = app.call('get_precision', document_id=document)
    if mode in MENU_MODES:
        assert 'showFocusArea:' in precision['supported_menu_items'], precision
    if mode == 'cmyk':
        assert 'select_focus_area' in precision['supported_tools'], precision


def mode_check(app, folder, mode, depth):
    """The colour-mode drives' call: the half-sharp picture in `mode` at `depth` answers
    `select_focus_area` with a non-empty selection on the sharp half, and `get_precision` lists
    it. The document is closed whatever happens. Answers the reply's bounds and range."""
    document = app.call('open_document', path=str(write(folder)))['document']['id']
    try:
        prepare(app, document, depth, mode)
        reply = answers(app, document, f'{mode} {depth}')
        listed(app, document, mode)
        return dict(bounds=reply['bounds'], range=reply['range'])
    finally:
        app.call('close_document', document_id=document, discard_changes=True)


def depth_case(app, path, depth, mode=None):
    document = app.call('open_document', path=str(path))['document']['id']
    try:
        prepare(app, document, depth, mode)
        return depth_checks(app, document, f'{mode or "rgb"} {depth}', mode)
    finally:
        # Closed whatever happened, so the next case opens the file afresh.
        app.call('close_document', document_id=document, discard_changes=True)


def depth_checks(app, document, label, mode):
    reply = answers(app, document, label)
    auto = coverage(app, document)
    check_half(auto, f'{label} auto')
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
    listed(app, document, mode)
    return dict(range=reply['range'], bounds=reply['bounds'])


def refusal_case(app, path, mode, sentence):
    document = app.call('open_document', path=str(path))['document']['id']
    try:
        prepare(app, document, 8, mode)
        text = app.call('select_focus_area', fail=True, document_id=document)
        assert text == sentence, (mode, text)
        return text
    finally:
        app.call('close_document', document_id=document, discard_changes=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='keep the run folder here')
    args = parser.parse_args()
    folder = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix='rz-focus-'))
    folder.mkdir(parents=True, exist_ok=True)
    path = write(folder)
    failures = 0
    answering = [(None, (8, 16, 32)), ('gray', (8, 16, 32)), ('lab', (8, 16)),
                 ('indexed', (8,)), ('cmyk', (8, 16))]
    with App(folder / 'instance', 2 * 1024 ** 3) as app:
        cases = [(f'select_focus_area {mode or "rgb"} {depth}-bit',
                  lambda depth=depth, mode=mode: depth_case(app, path, depth, mode))
                 for mode, depths in answering for depth in depths]
        cases += [(f'select_focus_area refused in {mode}',
                   lambda mode=mode, sentence=sentence: refusal_case(app, path, mode, sentence))
                  for mode, sentence in (('duotone', DUOTONE_REFUSAL), ('bitmap', BITMAP_REFUSAL))]
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
