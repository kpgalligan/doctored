#!/usr/bin/env python3
"""Behind and Clear over MCP (`brush_stroke`, `fill_selection` and `stroke_selection`'s
`blend_mode`) on owned Debug instances; the rule is `core/src/paint_mode.rs`'s.

The gates are FIXED here before anything is measured. Each instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library) and ends with it; no Doctored process this file did not start is signalled.

Every case paints a 12 x 8 transparent document that holds an opaque band (x 0-3), a
half-transparent band (x 4-7) and nothing (x 8-11), at 8, 16 and 32 bits.

- **B1 Fill.** `fill_selection` Behind at opacity 0.6 over rows 2-5: an opaque pixel and every
  pixel outside the selection are unchanged exactly; an empty pixel takes (Cs, 0.6); a
  half-transparent one takes ao = ab + 0.6(1 - ab), Co = (Cb ab + Cs 0.6 (1 - ab)) / ao, with Cs
  read off the empty pixels (where the rule says Co = Cs), within a code (1e-4 at float).
  Clear: ao = ab (1 - 0.6), colour unchanged exactly, outside unchanged.
- **B2 Brush and Stroke.** A `brush_stroke` across the three bands: Behind leaves every opaque pixel
  exactly and, over the empty band, equals a Normal stroke there; Clear leaves every colour and
  lowers alpha. `stroke_selection` Behind and Clear over a selection's band: the same two facts.
- **B3 colour modes.** Grayscale and Lab (8 bits): Clear lowers alpha keeping the colour, Behind
  then leaves the opaque band and fills the cleared pixels.
- **B4 refusals.** Each refuses by name and changes nothing (the history is unchanged): a mask
  target, the Quick Mask, `preserve_transparency`, Lock Transparency (fill and brush), CMYK,
  Indexed and Bitmap documents, and `fill`, `gradient`, `clone_stamp`, `stroke_path` and
  `fill_path` naming Behind or Clear.
- **B5 undo and replay.** One undo restores a Behind fill; a recorded Behind fill and Clear brush
  stroke replay to the same pixels.

Usage: python3 -B scripts/verify_behind_clear.py [--receipts <folder>]
"""
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 12, 8
DEPTHS = {8: 255.0, 16: 65535.0, 32: 1.0}
TOLERANCE = {8: 1.0, 16: 2.0, 32: 1e-4}
OPACITY = 0.6
INK = '#0ad25a'
STROKE = [[0.5, 4.5], [11.5, 4.5]]
NAMED = {
    'mask': 'has none of its own',
    'quick_mask': 'has none of its own',
    'preserve': 'which Preserve Transparency keeps',
    'lock': "which the layer's transparency lock keeps",
    'cmyk': 'The Behind blend mode is unavailable in a CMYK document',
    'indexed': 'The Behind blend mode is unavailable in an Indexed Color document',
    'bitmap': 'The Behind blend mode is unavailable in a Bitmap document',
    'other': 'Behind and Clear apply only to brush_stroke, fill_selection and stroke_selection',
}


def read(call, doc, x, y):
    """The layer pixel's own numbers: linear f32 at 32 bits, else its codes."""
    reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=0)
    for key in ('center_rgba32', 'mean_rgba32', 'center_rgba_codes'):
        if reply.get(key) is not None:
            return [float(v) for v in reply[key]]
    raise AssertionError(('no readout', reply))


def grid(call, doc):
    return {(x, y): read(call, doc, x, y) for y in range(H) for x in range(W)}


def history(call, doc):
    reply = call('list_history', document_id=doc)
    return (len(reply['states']), reply['cursor'])


def setup(call, bits, spec=None):
    """The three-band document (module doc)."""
    spec = spec or dict(depth=bits, profile='srgb')
    doc = call('new_document', width=W, height=H, background='transparent', **spec)
    doc = doc['document']['id']
    call('select_rect', document_id=doc, x=0, y=0, width=4, height=H)
    call('fill_selection', document_id=doc, color='#c81e28')
    call('select_rect', document_id=doc, x=4, y=0, width=4, height=H)
    call('fill_selection', document_id=doc, color='#3c46e6', opacity=0.5)
    call('deselect', document_id=doc)
    return doc


def near(a, b, tol):
    return all(abs(p - q) <= tol for p, q in zip(a, b))


def check_fill(call, depth):
    scale, tol = DEPTHS[depth], TOLERANCE[depth]
    out = {}
    for mode in ('Behind', 'Clear'):
        doc = setup(call, depth)
        before = grid(call, doc)
        call('select_rect', document_id=doc, x=0, y=2, width=W, height=4)
        reply = call('fill_selection', document_id=doc, color=INK, blend_mode=mode, opacity=OPACITY)
        assert reply.get('changed', True), (depth, mode, reply)
        after = grid(call, doc)
        inside = {(x, y) for y in range(2, 6) for x in range(W)}
        cs = None
        for (x, y), b in before.items():
            a = after[(x, y)]
            if (x, y) not in inside:
                assert a == b, (depth, mode, 'outside', x, y, b, a)
                continue
            ab = b[3] / scale
            if mode == 'Clear':
                assert a[:3] == b[:3], (depth, 'Clear writes colour', x, y, b, a)
                assert near([a[3]], [ab * (1 - OPACITY) * scale], tol), (depth, x, y, b, a)
            elif b[3] >= scale:
                assert a == b, (depth, 'Behind wrote an opaque pixel', x, y, b, a)
            elif b[3] == 0:
                assert near([a[3]], [OPACITY * scale], tol), (depth, x, y, a)
                cs = cs or a[:3]
                assert near(a[:3], cs, tol), (depth, 'Co = Cs over alpha 0', x, y, a, cs)
        if mode == 'Behind':
            for (x, y), b in before.items():
                if (x, y) in inside and 0 < b[3] < scale:
                    ab = b[3] / scale
                    ao = ab + OPACITY * (1 - ab)
                    want = [(b[c] * ab + cs[c] * OPACITY * (1 - ab)) / ao for c in range(3)]
                    a = after[(x, y)]
                    assert near(a, want + [ao * scale], tol), (depth, x, y, b, a, want, ao)
        # B5: one undo restores the canvas.
        call('undo', document_id=doc)
        assert grid(call, doc) == before, (depth, mode, 'undo')
        out[mode] = 'ok'
        call('close_document', document_id=doc, discard_changes=True)
    return out


def check_brush_and_stroke(call, depth):
    scale = DEPTHS[depth]
    normal = setup(call, depth)
    call('brush_stroke', document_id=normal, points=STROKE, size=5, color=INK, opacity=0.8)
    normal_after = grid(call, normal)
    call('close_document', document_id=normal, discard_changes=True)
    tol = TOLERANCE[depth]
    for mode in ('Behind', 'Clear'):
        doc = setup(call, depth)
        before = grid(call, doc)
        call('brush_stroke', document_id=doc, points=STROKE, size=5, color=INK, opacity=0.8,
             blend_mode=mode)
        after = grid(call, doc)
        moved = [p for p in before if before[p] != after[p]]
        assert moved, (depth, mode, 'the stroke moved nothing')
        ink = None
        for p in moved:
            b, a = before[p], after[p]
            if mode == 'Clear':
                assert a[:3] == b[:3] and a[3] < b[3], (depth, 'Clear', p, b, a)
            else:
                assert b[3] < scale, (depth, 'Behind wrote an opaque pixel', p, b, a)
                if b[3] == 0:
                    # Over alpha 0 Behind takes the Normal stroke's alpha and the exact ink
                    # (Normal's colour comes back out of a premultiplied overlay).
                    assert near([a[3]], [normal_after[p][3]], tol), (depth, p, a, normal_after[p])
                    ink = ink or a[:3]
                    assert near(a[:3], ink, tol), (depth, 'Behind over alpha 0 is the ink', p, a)
        call('undo', document_id=doc)
        call('select_rect', document_id=doc, x=2, y=2, width=8, height=4)
        call('stroke_selection', document_id=doc, width=2, location='center', color=INK,
             blend_mode=mode)
        stroked = grid(call, doc)
        moved = [p for p in before if before[p] != stroked[p]]
        assert moved, (depth, mode, 'the stroke moved nothing')
        for p in moved:
            b, a = before[p], stroked[p]
            if mode == 'Clear':
                assert a[:3] == b[:3] and a[3] < b[3], (depth, 'Clear stroke', p, b, a)
            else:
                assert b[3] < scale and a[3] > b[3], (depth, 'Behind stroke', p, b, a)
        call('close_document', document_id=doc, discard_changes=True)
    return 'ok'


def check_modes(call):
    out = {}
    for name, spec in (('gray', dict(color_mode='gray', depth=8)),
                       ('lab', dict(color_mode='lab', depth=8))):
        doc = setup(call, 8, spec)
        before = grid(call, doc)
        # The opaque band's alpha is the readout's full scale (u16 codes in these modes).
        scale = before[(0, 0)][3]
        call('select_rect', document_id=doc, x=0, y=2, width=W, height=4)
        call('fill_selection', document_id=doc, color=INK, blend_mode='Clear')
        cleared = grid(call, doc)
        for p, b in before.items():
            a = cleared[p]
            assert a[:3] == b[:3], (name, 'Clear writes colour', p, b, a)
            assert (a[3] == 0) == (2 <= p[1] < 6) or b[3] == 0, (name, p, b, a)
        call('select_rect', document_id=doc, x=0, y=0, width=W, height=H)
        call('fill_selection', document_id=doc, color=INK, blend_mode='Behind')
        behind = grid(call, doc)
        for p, b in cleared.items():
            if b[3] >= scale:
                assert behind[p] == b, (name, 'Behind wrote an opaque pixel', p)
            if b[3] == 0:
                assert behind[p][3] == scale, (name, 'Behind fills a clear pixel', p, behind[p])
        out[name] = 'ok'
        call('close_document', document_id=doc, discard_changes=True)
    return out


def refused(call, doc, tool, key, **arguments):
    count = history(call, doc)
    text = call(tool, fail=True, document_id=doc, **arguments)
    assert NAMED[key] in text, (tool, key, text)
    assert history(call, doc) == count, (tool, key, 'the history moved')
    return text


def check_refusals(call):
    out = {}
    doc = setup(call, 8)
    fill = dict(color=INK, blend_mode='Behind')
    call('add_layer_mask', document_id=doc, layer=0)
    out['mask'] = refused(call, doc, 'fill_selection', 'mask', target='mask', **fill)
    out['brush mask'] = call('brush_stroke', fail=True, document_id=doc, target='mask',
                             points=STROKE, color=INK, blend_mode='Clear')
    out['preserve'] = refused(call, doc, 'fill_selection', 'preserve',
                              preserve_transparency=True, **fill)
    call('set_layer_lock', document_id=doc, layer=0, locks=['transparency'])
    out['lock fill'] = refused(call, doc, 'fill_selection', 'lock', **fill)
    out['lock brush'] = refused(call, doc, 'brush_stroke', 'lock', points=STROKE, color=INK,
                                blend_mode='Clear')
    call('set_layer_lock', document_id=doc, layer=0, locks=[])
    for tool, arguments in (
            ('fill', dict(x=1, y=1, color=INK, blend_mode='Behind')),
            ('gradient', dict(x0=0, y0=0, x1=11, y1=0, start_color='#ff0000',
                              end_color='#0000ff', blend_mode='Clear')),
            ('clone_stamp', dict(points=STROKE, source_x=1, source_y=1, blend_mode='Behind'))):
        out[tool] = refused(call, doc, tool, 'other', **arguments)
    # Fill Path and Stroke Path keep the layer modes.
    call('select_rect', document_id=doc, x=2, y=2, width=6, height=4)
    call('selection_to_path', document_id=doc)
    call('deselect', document_id=doc)
    out['fill_path'] = refused(call, doc, 'fill_path', 'other', path='Work Path', layer=0,
                               color=INK, blend_mode='Clear')
    out['stroke_path'] = refused(call, doc, 'stroke_path', 'other', path='Work Path', layer=0,
                                 color=INK, size=3, blend_mode='Behind')
    call('set_quick_mask', document_id=doc, active=True)
    out['quick_mask'] = refused(call, doc, 'fill_selection', 'quick_mask', **fill)
    call('set_quick_mask', document_id=doc, active=False)
    call('close_document', document_id=doc, discard_changes=True)
    for kind, spec in (('cmyk', dict(color_mode='cmyk', depth=8)),):
        other = setup(call, 8, spec)
        out[kind] = refused(call, other, 'fill_selection', kind, **fill)
        call('close_document', document_id=other, discard_changes=True)
    indexed = setup(call, 8)
    call('convert_mode', document_id=indexed, mode='indexed', palette='exact', dither='none')
    out['indexed'] = refused(call, indexed, 'fill_selection', 'indexed', **fill)
    call('close_document', document_id=indexed, discard_changes=True)
    bitmap = call('new_document', width=W, height=H, background='white', color_mode='gray',
                  depth=8)['document']['id']
    call('convert_mode', document_id=bitmap, mode='bitmap', method='threshold')
    out['bitmap'] = refused(call, bitmap, 'fill_selection', 'bitmap', **fill)
    call('close_document', document_id=bitmap, discard_changes=True)
    return out


def check_replay(call):
    doc = setup(call, 8)
    before = grid(call, doc)
    call('start_recording', discard=True)
    call('select_rect', document_id=doc, x=0, y=2, width=W, height=4)
    call('fill_selection', document_id=doc, color=INK, blend_mode='Behind', opacity=OPACITY)
    call('deselect', document_id=doc)
    call('brush_stroke', document_id=doc, points=STROKE, size=5, color=INK, blend_mode='Clear')
    recorded = grid(call, doc)
    call('stop_recording', name='behind-clear')
    for _ in range(4):
        call('undo', document_id=doc)
    assert grid(call, doc) == before, 'undo restores the base'
    call('run_action', document_id=doc, name='behind-clear')
    assert grid(call, doc) == recorded, 'the recorded steps replay differently'
    call('close_document', document_id=doc, discard_changes=True)
    return 'ok'


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='behind-clear-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_behind_clear.py'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        for depth in DEPTHS:
            receipts[f'B1 fill {depth}'] = check_fill(call, depth)
            receipts[f'B2 brush and stroke {depth}'] = check_brush_and_stroke(call, depth)
        receipts['B3 modes'] = check_modes(call)
        receipts['B4 refusals'] = check_refusals(call)
        receipts['B5 replay'] = check_replay(call)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    print(json.dumps(receipts, indent=2))
    print('PASS: Behind and Clear over MCP at 8, 16 and 32 bits, in Gray and Lab, with their '
          'refusals, undo and replay')


if __name__ == '__main__':
    main()
