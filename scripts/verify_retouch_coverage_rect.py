#!/usr/bin/env python3
"""The MCP drive for 13B-9c and its coverage-brush residue: the 16-bit Clone
Stamp, Healing Brush and Spot Healing strokes, and the Dodge/Burn, Blur,
Sharpen, Sponge and History Brush strokes (`dodge_burn`, `blur_stroke`,
`sharpen_stroke`, `sponge_stroke`, `history_brush_stroke`, the last from the
state before an Invert), send their stroke's rect (`coverage_rect`), on a
16-bit RGB document and a 16-bit CMYK document, on an owned Debug instance
(`phase5_support.App`: its own scratch socket, a dummy API key, a private
directory for every preference library), terminated by its own context
manager; no Doctored process this file did not start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The fixtures are written here in
pure Python: an 8-bit PNG of hashed noise, opened and converted to 16 bits
(`convert_precision`), and a 16-bit chunky CMYK TIFF of hashed ink in macOS's
Generic CMYK Profile. Each stroke is one point at DAB; the selection, when
there is one, is the rectangle SELECT, whose left edge cuts the tip.

- **C1 the reply names the rect.** Every tool, with and without the
  selection, answers a job whose details carry `coverage_rect`: four
  integers, inside the canvas, holding the stroke's tip (DAB ± half the
  size).
- **C2 confined.** Every FAR probe (outside that rect) is exactly as it
  was; with the selection, every CUT probe (inside the tip, left of the
  selection) is exactly as it was; at least one probe inside the tip (and,
  with the selection, inside it) changes. The History Brush's "as it was" is
  the inverted state its stroke starts from.
- **C3 the hard clone copies.** A hard (100), opacity 1 `clone_stamp` whose
  source is 60 px to the left (an integer move) leaves at every INSIDE probe
  (inside the selection and the tip's interior) the sample the source held
  there before, exactly.
- **C4 one undo.** One `undo` restores every probe exactly (for the History
  Brush, the inverted state; a second Invert then gives the original back).
"""
import argparse
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402
from verify_cmyk_edit import GENERIC, cmyk_tiff  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
W, H = 200, 140
DAB = (100.5, 70.5)
SIZE = 21
SHIFT = 60
SELECT = {'x': 96, 'y': 40, 'width': 60, 'height': 60}
# Inside the selection and at most 8 px from DAB: the hard tip's interior.
INSIDE = [(100, 70), (104, 70), (100, 74), (98, 66), (105, 75)]
# Inside the tip, left of the selection's edge at x = 96.
CUT = [(92, 70), (93, 72), (94, 68)]
FAR = [(5, 5), (195, 5), (5, 135), (195, 135), (150, 120), (60, 20), (40, 70)]
DEPTHS = ['rgba16', 'cmyk16']
TOOLS = ['clone_stamp', 'heal_stroke', 'spot_heal_stroke', 'dodge_burn', 'blur_stroke',
         'sharpen_stroke', 'sponge_stroke', 'history_brush_stroke']


def hashed(x, y, salt):
    v = (x * 0x9E3779B1 + y * 0x85EBCA77 + salt * 0xC2B2AE3D) & 0xFFFFFFFF
    v ^= v >> 15
    v = (v * 0x2C1B3C6D) & 0xFFFFFFFF
    v ^= v >> 12
    return v >> 16


def rgb_pixels():
    out = bytearray()
    for y in range(H):
        for x in range(W):
            out += bytes([hashed(x, y, c) & 0xFF for c in (1, 2, 3)] + [255])
    return bytes(out)


def cmyk_samples():
    return [hashed(x, y, c) for y in range(H) for x in range(W) for c in (1, 2, 3, 4)]


def reading(reply):
    """The deepest numbers `sample_color` reports, and the alpha among them."""
    if 'ink16' in reply:
        return tuple(reply['ink16']) + (reply.get('alpha16', reply.get('a')),)
    for key in ['center_rgba32', 'center_rgba_codes']:
        if key in reply:
            return tuple(reply[key])
    return (reply['r'], reply['g'], reply['b'], reply['a'])


def coverage_rect(reply):
    details = reply.get('details') or {}
    return details.get('coverage_rect', reply.get('coverage_rect'))


def holds_tip(rect):
    if not (isinstance(rect, list) and len(rect) == 4 and all(isinstance(v, int) for v in rect)):
        return False
    x, y, w, h = rect
    half = SIZE / 2
    return (0 <= x and 0 <= y and w > 0 and h > 0 and x + w <= W and y + h <= H
            and x <= DAB[0] - half and y <= DAB[1] - half
            and x + w >= DAB[0] + half and y + h >= DAB[1] + half)


def outside(point, rect):
    x, y, w, h = rect
    return not (x <= point[0] < x + w and y <= point[1] < y + h)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    output = Path(args.receipts or tempfile.mkdtemp(prefix='retouch-coverage-rect-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_retouch_coverage_rect.py before any measurement'}
    rgb_path = folder / 'noise.png'
    png(rgb_path, W, H, rgb_pixels())
    cmyk_path = folder / 'noise.tif'
    cmyk_path.write_bytes(cmyk_tiff(W, H, cmyk_samples(), GENERIC.read_bytes()))
    failures = []

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def gate(ok, what):
            print(('ok   ' if ok else 'FAIL ') + what)
            if not ok:
                failures.append(what)

        def probe(doc, points):
            return {p: reading(call('sample_color', document_id=doc, x=p[0], y=p[1]))
                    for p in points}

        def open_depth(depth):
            if depth == 'cmyk16':
                return call('open_document', path=str(cmyk_path))['document']['id']
            doc = call('open_document', path=str(rgb_path))['document']['id']
            call('convert_precision', document_id=doc, mode=depth, structure='preserve')
            return doc

        def stroke(doc, tool, state=None):
            arguments = {'document_id': doc, 'layer': 0, 'points': [list(DAB)], 'size': SIZE}
            if tool == 'clone_stamp':
                arguments.update(source_x=DAB[0] - SHIFT, source_y=DAB[1], opacity=1,
                                 hardness=100)
            elif tool == 'heal_stroke':
                arguments.update(source_x=DAB[0] - SHIFT, source_y=DAB[1], opacity=100)
            elif tool == 'spot_heal_stroke':
                arguments.update(opacity=100, seed=7)
            elif tool == 'dodge_burn':
                arguments.update(exposure=80, hardness=100)
            elif tool == 'sponge_stroke':
                arguments.update(mode='desaturate', strength=100, hardness=100)
            elif tool == 'history_brush_stroke':
                arguments.update(opacity=100, hardness=100, state_id=state)
            else:
                arguments.update(strength=100, hardness=100)
            return call(tool, **arguments)

        everything = INSIDE + CUT + FAR
        sources = [(x - SHIFT, y) for (x, y) in INSIDE]
        for depth in DEPTHS:
            row = receipts.setdefault(depth, {})
            doc = open_depth(depth)
            original = probe(doc, everything)
            source = probe(doc, sources)
            for tool in TOOLS:
                for selected in (False, True):
                    label = f'{depth} {tool}' + (' selected' if selected else '')
                    # The History Brush paints the state before an Invert back:
                    # its stroke starts from the inverted state.
                    state, start = None, original
                    if tool == 'history_brush_stroke':
                        state = call('list_history', document_id=doc)['current_state_id']
                        call('apply_filter', document_id=doc, layer=0, filter='invert')
                        start = probe(doc, everything)
                    if selected:
                        call('select_rect', document_id=doc, **SELECT)
                    reply = stroke(doc, tool, state)
                    rect = coverage_rect(reply)
                    row[label] = {'coverage_rect': rect}
                    gate(holds_tip(rect), f'{label} C1 the reply names the rect {rect}')
                    after = probe(doc, everything)
                    if isinstance(rect, list) and len(rect) == 4:
                        far = [p for p in FAR if outside(p, rect)]
                        gate(len(far) == len(FAR) and all(after[p] == start[p] for p in far),
                             f'{label} C2 nothing outside the rect changes')
                    if selected:
                        gate(all(after[p] == start[p] for p in CUT),
                             f'{label} C2 nothing outside the selection changes')
                    changed = INSIDE if selected else INSIDE + CUT
                    gate(any(after[p] != start[p] for p in changed),
                         f'{label} C2 the stroke changes its tip')
                    if tool == 'clone_stamp':
                        copied = [after[p] == source[(p[0] - SHIFT, p[1])] for p in INSIDE]
                        gate(all(copied), f'{label} C3 the hard clone copies its source exactly')
                    call('undo', document_id=doc)
                    gate(probe(doc, everything) == start, f'{label} C4 one undo restores')
                    if selected:
                        call('deselect', document_id=doc)
                    if tool == 'history_brush_stroke':
                        # A second Invert (exact at 16 bits and on ink) gives the
                        # next case the original back, whatever the selection's
                        # own undo steps are.
                        call('apply_filter', document_id=doc, layer=0, filter='invert')
                        gate(probe(doc, everything) == original,
                             f'{label} a second Invert restores the original')
            call('close_document', document_id=doc, discard_changes=True)

    receipts['failures'] = failures
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    print('verify_retouch_coverage_rect: PASS' if not failures
          else f'verify_retouch_coverage_rect: FAIL ({len(failures)})')
    raise SystemExit(0 if not failures else 1)


if __name__ == '__main__':
    main()
