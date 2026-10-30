#!/usr/bin/env python3
"""The #1 retouch-batch MCP drive: the painting and retouching tools and
Image ▸ Adjustments ▸ Equalize on a CMYK document's ink, over MCP, on an owned
Debug instance (`phase5_support.App`: its own scratch socket, a dummy API key,
a private directory for every preference library), terminated by its own
context manager; no Doctored process this file did not start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. Every fixture is written here in
pure Python: a 16-bit chunky CMYK TIFF in macOS's Generic CMYK Profile (an ink
ramp with hashed noise, so no stroke lands on a flat area), opened and its layer
put in a group (record 0 inside group record 1), and 8-bit PNGs for RGB.

- **R1 retouch brushes.** `dodge_burn` (shadows, midtones and highlights, each
  with protect_tones off and on), `sponge_stroke` (desaturate and saturate: the
  K ink EXACTLY as it was at every sampled stroke pixel), `blur_stroke`,
  `sharpen_stroke`, `smudge_stroke` and `history_brush_stroke` (from the state
  before an Invert) change the ink inside the stroke and leave the probes 30 px
  and more from it untouched; one `undo` restores every probe exactly.
- **R2 clone and heal.** `clone_stamp` (sample current; sample all; a
  source_transform offset; Multiply), `heal_stroke` and `patch_region` behave as
  R1 and the document stays CMYK.
- **R3 synthesis.** `spot_heal_stroke`, `content_aware_fill` (in place and
  output new_layer), `remove_stroke` and `content_aware_move` change nothing
  outside the coverage box grown by RING_REACH px; the new layer adds one layer
  and the document stays CMYK.
- **R4 modes on ink.** On a flat ink layer, `brush_stroke` (its centre),
  `fill` (the Paint Bucket), `gradient` (one ink end to end), `fill_selection`
  and `stroke_selection` (an inside strip pixel), each at opacity 1 in Multiply
  and in Color, land within 1 code of an f64 W3C statement on `L = 1 − ink`
  (Multiply: L = Lb·Ls; Color: SetLum(Ls, Lum(Lb)) on C/M/Y, K the backdrop's).
- **R5 Equalize.** On CMYK 16 and 8 and RGB 8, 16 and 32: with no selection
  the layer changes; with a selection `scope` selected leaves the outside
  exactly as it was and `entire` changes it. `undo` restores exactly. At 8
  bits, an offset layer (200 × 200 at (50, 50) on 400 × 400) under a feathered
  selection: a selected edge pixel is ONE lerp of the `entire` result by its
  coverage (within 1 code), `entire` changes pixels outside the selection, and
  a layer mask target equalizes the mask. A recorded Action replays equal.
- **R6 .rz.** After R1–R3 `save_copy rz` reopens with the same ink16 at every
  probe.
- **R7 refusals.** `depth_blur`, a brush in Pass Through, and the tools take1
  did not show enabled (`red_eye`, `red_eye_auto`, `auto_blend_layers`)
  refuse by name.
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
from verify_cmyk_edit import GENERIC, cmyk_tiff, codes  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
W, H = 120, 90
STROKE = [[30, 45], [90, 45]]
STROKE_SIZE = 16
INSIDE = [(40, 45), (60, 45), (80, 45)]
FAR = [(60, 5), (60, 85), (110, 10), (5, 80)]
RING_REACH = 16
MODE_BOUND = 1
EDGE_BOUND = 1
REFUSED = 'is unavailable in a CMYK document'


def hashed(i, salt):
    return ((i * 2654435761 + salt * 40503) >> 7) & 0xFFFF


def textured():
    samples = []
    for y in range(H):
        for x in range(W):
            i = y * W + x
            samples += [
                min(65535, 9000 + x * 300 + hashed(i, 1) % 6000),
                min(65535, 12000 + y * 400 + hashed(i, 2) % 6000),
                min(65535, 30000 + hashed(i, 3) % 9000),
                min(65535, 4000 + (x + y) * 90 + hashed(i, 4) % 5000),
            ]
    return samples


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-retouch-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_retouch.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def ink(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16']

        def probe(doc, points):
            return {p: ink(doc, *p) for p in points}

        def open_cmyk(name, samples, width=W, height=H, group=True):
            path = folder / name
            path.write_bytes(cmyk_tiff(width, height, samples, profile))
            doc = call('open_document', path=str(path))['document']['id']
            if group:
                call('group_layers', document_id=doc, layers=[0], name='Group')
            return doc

        doc = open_cmyk('textured.tif', textured())
        everything = INSIDE + FAR

        def stroke_gate(name, tool, check=None, **arguments):
            before = probe(doc, everything)
            reply = call(tool, document_id=doc, layer=0, **arguments)
            assert reply.get('changed') is not False, (name, reply)
            after = probe(doc, everything)
            assert any(after[p] != before[p] for p in INSIDE), (name, 'nothing changed', after)
            for p in FAR:
                assert after[p] == before[p], (name, 'far probe moved', p, before[p], after[p])
            if check:
                check(before, after)
            call('undo', document_id=doc)
            restored = probe(doc, everything)
            assert restored == before, (name, 'undo is not exact')
            return {str(p): after[p] for p in INSIDE}

        # ---- R1: the retouch brushes ------------------------------------------
        r1 = {}
        for range_name in ['shadows', 'midtones', 'highlights']:
            for protect in [False, True]:
                r1[f'dodge_{range_name}_{protect}'] = stroke_gate(
                    f'dodge {range_name} protect {protect}', 'dodge_burn', points=STROKE,
                    size=STROKE_SIZE, exposure=60, range=range_name, burn=range_name == 'highlights',
                    protect_tones=protect)

        def k_held(before, after):
            for p in INSIDE:
                assert after[p][3] == before[p][3], ('the Sponge moved K', p, before[p], after[p])

        for mode in ['desaturate', 'saturate']:
            r1[f'sponge_{mode}'] = stroke_gate(f'sponge {mode}', 'sponge_stroke', check=k_held,
                                               points=STROKE, size=STROKE_SIZE, mode=mode,
                                               strength=80)
        for tool in ['blur_stroke', 'sharpen_stroke']:
            r1[tool] = stroke_gate(tool, tool, points=STROKE, size=STROKE_SIZE, strength=100)
        r1['smudge_stroke'] = stroke_gate('smudge', 'smudge_stroke', points=STROKE,
                                          size=STROKE_SIZE, strength=90)
        # The History Brush paints the state before an Invert back.
        original = probe(doc, everything)
        before_invert = call('list_history', document_id=doc)['current_state_id']
        call('apply_filter', document_id=doc, layer=0, filter='invert')
        assert probe(doc, INSIDE) != {p: original[p] for p in INSIDE}
        history = call('history_brush_stroke', document_id=doc, layer=0, points=STROKE,
                       size=STROKE_SIZE, opacity=100, state_id=before_invert)
        assert history.get('changed') is not False, history
        restored = probe(doc, INSIDE)
        for p in INSIDE:
            assert all(abs(a - b) <= 1 for a, b in zip(restored[p], original[p])), (p, restored[p])
        call('undo', document_id=doc)
        call('undo', document_id=doc)
        assert probe(doc, everything) == original
        r1['history_brush'] = {str(p): restored[p] for p in INSIDE}
        receipts['r1'] = r1
        print('PASS r1: the retouch brushes edit ink inside the stroke only; undo is exact',
              flush=True)

        # ---- R2: clone and heal ---------------------------------------------------
        r2 = {}
        source = {'source_x': 60, 'source_y': 20}
        r2['clone_current'] = stroke_gate('clone current', 'clone_stamp', points=STROKE,
                                          size=STROKE_SIZE, sample='current', **source)
        r2['clone_all'] = stroke_gate('clone all', 'clone_stamp', points=STROKE, size=STROKE_SIZE,
                                      sample='all', **source)
        r2['clone_transform'] = stroke_gate('clone transform', 'clone_stamp', points=STROKE,
                                            size=STROKE_SIZE, sample='current',
                                            source_transform=[1, 0, 0, 1, 0, -25], **source)
        r2['clone_multiply'] = stroke_gate('clone multiply', 'clone_stamp', points=STROKE,
                                           size=STROKE_SIZE, sample='current',
                                           blend_mode='Multiply', **source)
        r2['heal'] = stroke_gate('heal', 'heal_stroke', points=STROKE, size=STROKE_SIZE,
                                 sample='current', **source)
        r2['patch'] = stroke_gate('patch', 'patch_region',
                                  points=[[45, 38], [75, 38], [75, 52], [45, 52]],
                                  offset_x=0, offset_y=-28)
        assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
        receipts['r2'] = r2
        print('PASS r2: clone, heal and patch edit ink; the document stays CMYK', flush=True)

        # ---- R3: synthesis ------------------------------------------------------------
        grid = [(x, y) for x in range(2, W, 9) for y in range(2, H, 9)]

        def outside(box):
            x0, y0, x1, y1 = box
            return [p for p in grid if not (x0 - RING_REACH <= p[0] < x1 + RING_REACH
                                            and y0 - RING_REACH <= p[1] < y1 + RING_REACH)]

        def undo_count():
            return int(call('list_history', document_id=doc)['undo_count'])

        def synthesis_gate(name, box, run, layers_added=0):
            count = len(call('get_document', document_id=doc)['layers'])
            steps = undo_count()
            before = probe(doc, grid)
            reply = run()
            assert reply.get('changed') is not False, (name, reply)
            after = probe(doc, grid)
            for p in outside(box):
                assert after[p] == before[p], (name, 'changed outside the box', p)
            layers = call('get_document', document_id=doc)['layers']
            assert len(layers) == count + layers_added, (name, len(layers), count)
            assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
            while undo_count() > steps:
                call('undo', document_id=doc)
            assert probe(doc, grid) == before, (name, 'undo')
            return {'outside_probes': len(outside(box))}

        r3 = {}
        r3['spot_heal'] = synthesis_gate(
            'spot heal', (52, 37, 68, 53),
            lambda: call('spot_heal_stroke', document_id=doc, layer=0, points=[[60, 45]], size=14))

        def caf(output_kind):
            call('select_rect', document_id=doc, x=55, y=40, width=10, height=10)
            reply = call('content_aware_fill', document_id=doc, layer=0, output=output_kind)
            call('deselect', document_id=doc)
            return reply

        r3['content_aware_fill'] = synthesis_gate('content-aware fill', (55, 40, 65, 50),
                                                  lambda: caf('layer'))
        r3['content_aware_fill_new_layer'] = synthesis_gate(
            'content-aware fill new layer', (55, 40, 65, 50), lambda: caf('new_layer'),
            layers_added=1)
        r3['remove'] = synthesis_gate(
            'remove', (50, 35, 72, 55),
            lambda: call('remove_stroke', document_id=doc, layer=0, points=[[55, 45], [66, 45]],
                         size=10, mode='content_aware'))
        r3['content_aware_move'] = synthesis_gate(
            'content-aware move', (40, 30, 80, 60),
            lambda: call('content_aware_move', document_id=doc, layer=0,
                         points=[[45, 40], [55, 40], [55, 50], [45, 50]], offset_x=15, offset_y=0))
        receipts['r3'] = r3
        print('PASS r3: synthesis stays inside its box; a new layer is CMYK', flush=True)

        # ---- R6: .rz round trip after retouching ---------------------------------------
        call('dodge_burn', document_id=doc, layer=0, points=STROKE, size=STROKE_SIZE,
             exposure=50, protect_tones=True)
        call('clone_stamp', document_id=doc, layer=0, points=[[20, 70], [40, 70]], size=10,
             blend_mode='Multiply', **source)
        kept = probe(doc, grid)
        rz = folder / 'retouched.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        assert probe(again, grid) == kept, 'the .rz reopens with other ink'
        call('close_document', document_id=again, discard_changes=True)
        receipts['r6'] = {'probes': len(grid)}
        print('PASS r6: the .rz reopens with the same ink', flush=True)

        # ---- R4: blend modes on ink ---------------------------------------------------
        receipts['r4'] = modes_on_ink(call, open_cmyk, ink)
        print('PASS r4: paint, fill, gradient and stroke composite through the mode on 1 − ink',
              flush=True)

        # ---- R5: Equalize -------------------------------------------------------------------
        receipts['r5'] = equalize_gates(call, folder, open_cmyk, ink)
        print('PASS r5: Equalize at CMYK 8/16 and RGB 8/16/32, scopes, offset layer, mask, Action',
              flush=True)

        # ---- R7: refusals -------------------------------------------------------------------
        refusals = {
            'depth_blur': call('depth_blur', fail=True, document_id=doc, layer=0),
            'pass_through': call('brush_stroke', fail=True, document_id=doc, layer=0,
                                 points=[[1, 1], [3, 3]], ink=[0, 0, 0, 50],
                                 blend_mode='Pass Through'),
            'red_eye': call('red_eye', fail=True, document_id=doc, layer=0, x=5, y=5, width=9,
                            height=9),
            'red_eye_auto': call('red_eye_auto', fail=True, document_id=doc, layer=0),
            'auto_blend_layers': call('auto_blend_layers', fail=True, document_id=doc),
        }
        assert REFUSED in refusals['depth_blur'], refusals
        assert 'Pass Through' in refusals['pass_through'], refusals
        for key in ['red_eye', 'red_eye_auto', 'auto_blend_layers']:
            assert REFUSED in refusals[key], (key, refusals[key])
        receipts['r7'] = refusals
        print('PASS r7: what CMYK does not take refuses by name', flush=True)
        call('close_document', document_id=doc, discard_changes=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


# ---- R4's oracle and gates --------------------------------------------------------------

def lum(c):
    return 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]


def clip_color(c):
    lightness = lum(c)
    low, high = min(c), max(c)
    out = list(c)
    if low < 0:
        out = [lightness + (v - lightness) * lightness / (lightness - low) for v in out]
    if high > 1:
        out = [lightness + (v - lightness) * (1 - lightness) / (high - lightness) for v in out]
    return out


def set_lum(c, lightness):
    d = lightness - lum(c)
    return clip_color([v + d for v in c])


def blended(mode, cb, cs):
    """`B(Cb, Cs)` on ink (W3C on L = 1 − ink)."""
    lb, ls = [1 - v for v in cb], [1 - v for v in cs]
    if mode == 'Multiply':
        return [1 - lb[c] * ls[c] for c in range(4)]
    out = set_lum(ls[:3], lum(lb[:3]))
    return [1 - out[0], 1 - out[1], 1 - out[2], cb[3]]


def modes_on_ink(call, open_cmyk, ink):
    backdrop = [20, 30, 10, 5]
    paint = [50, 10, 60, 20]
    flat = codes(backdrop) * (W * H)
    cb = [v / 65535 for v in codes(backdrop)]
    cs = [p / 100 for p in paint]
    out = {}
    for mode in ['Multiply', 'Color']:
        want = [round(v * 65535) for v in blended(mode, cb, cs)]

        def check(name, doc, points):
            for p in points:
                got = ink(doc, *p)
                assert all(abs(g - w) <= MODE_BOUND for g, w in zip(got, want)), (mode, name, p, got, want)
            out[f'{mode}_{name}'] = ink(doc, *points[0])

        doc = open_cmyk(f'flat-{mode}.tif', flat, group=False)
        call('brush_stroke', document_id=doc, layer=0, points=[[20, 20], [40, 20]], size=12,
             ink=paint, opacity=1, blend_mode=mode)
        check('brush', doc, [(30, 20)])
        call('undo', document_id=doc)
        call('fill', document_id=doc, layer=0, x=60, y=60, ink=paint, tolerance=0, blend_mode=mode)
        check('bucket', doc, [(60, 60), (5, 5)])
        call('undo', document_id=doc)
        call('gradient', document_id=doc, layer=0, x0=0, y0=10, x1=W, y1=10, start_ink=paint,
             end_ink=paint, blend_mode=mode)
        check('gradient', doc, [(10, 10), (100, 70)])
        call('undo', document_id=doc)
        call('select_rect', document_id=doc, x=30, y=30, width=40, height=30)
        call('fill_selection', document_id=doc, layer=0, ink=paint, blend_mode=mode)
        check('fill_selection', doc, [(50, 45)])
        call('undo', document_id=doc)
        call('stroke_selection', document_id=doc, layer=0, ink=paint, width=4, location='inside',
             blend_mode=mode)
        check('stroke_selection', doc, [(31, 45)])
        call('close_document', document_id=doc, discard_changes=True)
    return out


# ---- R5 -----------------------------------------------------------------------------------------

def equalize_gates(call, folder, open_cmyk, ink):
    out = {}
    # A low-key picture, so Equalize moves it.
    low = []
    for y in range(H):
        for x in range(W):
            i = y * W + x
            low += [40000 + hashed(i, 7) % 9000, 45000 + hashed(i, 8) % 8000,
                    38000 + hashed(i, 9) % 9000, 30000 + hashed(i, 10) % 6000]
    inside_points = [(10, 10), (30, 40)]
    outside_points = [(100, 10), (90, 70)]

    def scopes(doc, read, name):
        whole_before = {p: read(doc, *p) for p in inside_points + outside_points}
        reply = call('equalize', document_id=doc, layer=0)
        assert reply['changed'] is True and reply['scope'] == 'whole', (name, reply)
        assert any(read(doc, *p) != whole_before[p] for p in inside_points), name
        call('undo', document_id=doc)
        assert {p: read(doc, *p) for p in whole_before} == whole_before, (name, 'undo')
        call('select_rect', document_id=doc, x=0, y=0, width=60, height=H)
        reply = call('equalize', document_id=doc, layer=0, scope='selected')
        assert reply['scope'] == 'selected', reply
        for p in outside_points:
            assert read(doc, *p) == whole_before[p], (name, 'selected moved the outside', p)
        assert any(read(doc, *p) != whole_before[p] for p in inside_points), name
        call('undo', document_id=doc)
        call('equalize', document_id=doc, layer=0, scope='entire')
        assert any(read(doc, *p) != whole_before[p] for p in outside_points), (name, 'entire')
        call('undo', document_id=doc)
        call('deselect', document_id=doc)
        assert {p: read(doc, *p) for p in whole_before} == whole_before, (name, 'undo')
        return True

    cmyk16 = open_cmyk('low16.tif', low)
    out['cmyk16'] = scopes(cmyk16, ink, 'cmyk16')
    call('convert_precision', document_id=cmyk16, mode='cmyk8')
    out['cmyk8'] = scopes(cmyk16, ink, 'cmyk8')
    call('close_document', document_id=cmyk16, discard_changes=True)

    def rgb_read(doc, x, y):
        reply = call('sample_color', document_id=doc, x=x, y=y)
        for key in ['center_rgba32', 'center_rgba_codes']:
            if key in reply:
                return reply[key]
        return [reply['r'], reply['g'], reply['b'], reply['a']]

    pixels = bytes(v for y in range(H) for x in range(W)
                   for v in (60 + hashed(y * W + x, 11) % 70, 80 + hashed(y * W + x, 12) % 60,
                             50 + hashed(y * W + x, 13) % 90, 255))
    rgb_path = folder / 'low.png'
    png(rgb_path, W, H, pixels)
    for mode in ['rgba8', 'rgba16', 'rgba32']:
        rgb = call('open_document', path=str(rgb_path))['document']['id']
        if mode != 'rgba8':
            call('convert_precision', document_id=rgb, mode=mode, structure='preserve')
        out[mode] = scopes(rgb, rgb_read, mode)
        call('close_document', document_id=rgb, discard_changes=True)
    out['offset'] = offset_layer(call, folder, rgb_read)
    return out


def offset_layer(call, folder, read):
    side = 200
    pixels = bytes(v for y in range(side) for x in range(side)
                   for v in (40 + (x * 3 + hashed(y * side + x, 21) % 40) % 120,
                             60 + (y + hashed(y * side + x, 22) % 50) % 100,
                             30 + hashed(y * side + x, 23) % 120, 255))
    path = folder / 'offset.png'
    png(path, side, side, pixels)
    doc = call('open_document', path=str(path))['document']['id']
    call('canvas_size', document_id=doc, width=400, height=400, anchor='top-left')
    call('move_layers', document_id=doc, layers=[0], dx=50, dy=50)
    call('select_rect', document_id=doc, x=100, y=100, width=100, height=100, feather=12)
    edge = [(96, 150), (100, 150), (104, 150), (150, 98)]
    far = [(60, 60), (240, 240)]
    original = {p: read(doc, *p) for p in edge + far}
    coverage = {(s['x'], s['y']): s for s in call('get_selection', document_id=doc,
                points=[{'x': x, 'y': y} for x, y in edge])['samples']}
    call('equalize', document_id=doc, scope='entire')
    entire = {p: read(doc, *p) for p in edge + far}
    assert any(entire[p] != original[p] for p in far), 'entire changes pixels outside the selection'
    call('undo', document_id=doc)
    call('equalize', document_id=doc, scope='selected')
    selected = {p: read(doc, *p) for p in edge + far}
    for p in far:
        assert selected[p] == original[p], ('selected moved a pixel outside the selection', p)
    lerps = {}
    for p in edge:
        c = coverage[p]['coverage16'] / 65535
        for k in range(3):
            want = original[p][k] + c * (entire[p][k] - original[p][k])
            assert abs(selected[p][k] - want) <= EDGE_BOUND, ('not one lerp', p, k, selected[p],
                                                                original[p], entire[p], c)
        lerps[str(p)] = {'coverage': c, 'selected': selected[p], 'entire': entire[p]}
    call('undo', document_id=doc)
    # The mask target: a mask made from the feathered selection is equalized.
    call('add_layer_mask', document_id=doc, layer=0, kind='from_selection')
    call('deselect', document_id=doc)
    mask = call('equalize', document_id=doc, layer=0, target='mask')
    assert mask['changed'] is True and mask['target'] == 'mask', mask
    call('undo', document_id=doc)
    # A recorded Action replays equal.
    call('select_rect', document_id=doc, x=100, y=100, width=100, height=100, feather=12)
    call('start_recording', discard=True)
    call('equalize', document_id=doc, scope='selected')
    recorded = {p: read(doc, *p) for p in edge + far}
    call('stop_recording', name='equalize-replay')
    call('undo', document_id=doc)
    call('run_action', document_id=doc, name='equalize-replay')
    replayed = {p: read(doc, *p) for p in edge + far}
    assert replayed == recorded, ('the Action replays differently', replayed, recorded)
    call('close_document', document_id=doc, discard_changes=True)
    return {'edge': lerps}


if __name__ == '__main__':
    main()
