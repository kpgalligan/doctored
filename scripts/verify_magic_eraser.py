#!/usr/bin/env python3
"""The Magic Eraser over MCP (`magic_eraser`, `designs/magic-eraser.md`) on an owned Debug instance,
and the style read-back the Layer Style panes' stop editor relies on.

The gates are FIXED here before anything is measured, and the oracle is written from the ADR alone:
the region is the Magic Wand's — every pixel whose R, G, B, A, R−G, G−B and R−B each lie within
the tolerance of the seed's, four-connected to it when contiguous — computed here by a
breadth-first walk over the colours this file painted (8-bit codes; the typed wand reads its codes
and the tolerance × 257 back as bytes, the float wand the display copy, and every colour below sits
clear of each tolerance used); then `alpha × (1 − opacity × coverage)`, the colour kept. The
instance is owned by this file (`phase5_support.App`); no Doctored process it did not start is
signalled.

Scene (S): a 16 × 10 document, white Background (layer 0) with a black block under square B; layer
1 at 50 % opacity holds square A (x 1–4, y 1–4) and square B (x 9–12, y 1–4) in C1 and square N
(x 1–4, y 6–8) in C2, which differs from C1 by 10 codes in R, G and B; layer 1 is active.

- **M0 styles.** `set_layer_style` with a four-stop Gradient Overlay (two midpoints), a four-stop
  Outer Glow gradient and a four-stop Stroke gradient reads back all four stops of each, the
  overlay's midpoints kept and the glow's dropped (the core keeps none on a glow).
- **M1 RGB at 8, 16 and 32 bits.** On S, each from the untouched scene: tolerance 0 contiguous
  (A), non-contiguous (A and B), tolerance 20 non-contiguous (A, B and N), opacity 0.5 (A at half
  alpha: exact at 8 and 16 bits, 1e-6 at 32), a selection over A's left half with the seed inside
  it (that half) and outside it (changed false, the history unchanged), and sample_all_layers with
  non-contiguous (the composite's region: A alone, since B's composite sits over black). Every
  pixel outside the expected region is unchanged exactly; inside, the colour is unchanged where
  alpha stays above 0. One undo restores the layer exactly.
- **M2 colour modes.** Grayscale, Lab and CMYK (8 bits), a transparent layer holding A, B and N:
  contiguous erases A only, non-contiguous A and B, nothing else moves. Indexed (converted from an
  RGB scene): the erased pixels read clear (the transparent index) and nothing else moves.
- **M3 refusals.** Each answers in-band and leaves the history unchanged: Bitmap, target mask,
  blend_mode, the Quick Mask, an adjustment layer, a group, a pixels lock, a transparency lock (the
  layer's own and an enclosing group's, at 8 and 16 bits). A shape layer rasterizes and erases.
- **M4 replay.** A recorded `magic_eraser` replays to the same pixels.
- **M5 fade.** Edit ▸ Fade follows a click (`designs/v1-shortcuts-fade.md`): the state before is
  moved toward the result by the fade's opacity, premultiplied, weighed 1 on the pixels the click
  changed. So erasing A (tolerance 0, contiguous, opacity 1) and fading Normal at 0.5 leaves A at
  half the scene's alpha with the scene's colour (within one code at 8 and 16 bits, 1e-6 at 32)
  and every other pixel exactly the scene's. On S at 8, 16 and 32 bits the reply names "Fade Magic
  Eraser", the history keeps the erase's entry count with the fade current, and one undo restores
  the scene. A fade after a selection change refuses ("There is nothing to fade"). Grayscale, Lab
  and CMYK (8 bits, M2's layer) fade A to half alpha (one 8-bit code in the readout's units) and
  move nothing else. Indexed refuses `fade` for every step, so after a click it answers the
  mode's sentence and the history holds.

Usage: python3 -B scripts/verify_magic_eraser.py [--receipts <folder>] [--sections M0,M1,…]
"""
import argparse
from collections import deque
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 16, 10
C1, C2 = (200, 30, 40), (210, 40, 50)
WHITE, BLACK = (255, 255, 255), (0, 0, 0)
SQUARES = {'A': (1, 1, 4, 4, C1), 'B': (9, 1, 4, 4, C1), 'N': (1, 6, 4, 3, C2)}
UNDER_B = (9, 1, 4, 4)
DEPTHS = {8: 255.0, 16: 65535.0, 32: 1.0}
ALPHA_TOL = {8: 0.0, 16: 0.0, 32: 1e-6}
COLOUR_TOL = {8: 0.0, 16: 0.0, 32: 1e-6}
SEED = (2, 2)
MODES = (('gray', dict(color_mode='gray', depth=8)),
         ('lab', dict(color_mode='lab', depth=8)),
         ('cmyk', dict(color_mode='cmyk', depth=8)))
# M5: the fade's opacity, its tolerance in the readout's own units (one code at 8 and 16 bits),
# the reply's names and the two refusals.
FADE_OPACITY = 0.5
FADE_TOL = {8: 1.0, 16: 1.0, 32: 1e-6}
FADED, FADE_NAME = 'Magic Eraser', 'Fade Magic Eraser'
NOTHING_TO_FADE = 'There is nothing to fade'
INDEXED_FADE = 'fade is unavailable in an Indexed Color document'
NAMED = {
    'bitmap': 'magic_eraser is not available in Bitmap mode.',
    'mask': 'The Magic Eraser erases a layer\'s own pixels',
    'blend': 'The Magic Eraser has no blend mode',
    'quick_mask': 'Exit Quick Mask before editing layer pixels',
    'pixels': 'has its pixels locked',
    'transparency': 'has its transparency locked. The Magic Eraser changes transparency',
    'group_transparency': 'which has its transparency locked. The Magic Eraser changes transparency',
}


def hexcode(rgb):
    return '#%02x%02x%02x' % rgb


def rect_pixels(x, y, w, h):
    return {(px, py) for py in range(y, y + h) for px in range(x, x + w)}


def layer_scene():
    """Layer 1's colours (None = transparent) as painted."""
    scene = {(x, y): None for y in range(H) for x in range(W)}
    for (x, y, w, h, colour) in SQUARES.values():
        for p in rect_pixels(x, y, w, h):
            scene[p] = colour
    return scene


def composite_scene():
    """The composite S shows in 8-bit codes: layer 1 at 50 % over white, or black under B."""
    layer = layer_scene()
    under = rect_pixels(*UNDER_B)
    out = {}
    for p, colour in layer.items():
        base = BLACK if p in under else WHITE
        if colour is None:
            out[p] = base + (255,)
        else:
            out[p] = tuple(int(0.5 * c + 0.5 * b + 0.5) for c, b in zip(colour, base)) + (255,)
    return out


def projections(rgba):
    r, g, b, a = rgba
    return (r, g, b, a, r - g, g - b, r - b)


def region(scene, seed, tolerance, contiguous):
    """The ADR's region: every projection within ± tolerance of the seed's; 4-connected."""
    def rgba(p):
        c = scene[p]
        if c is None:
            return (0, 0, 0, 0)
        return c if len(c) == 4 else c + (255,)
    ref = projections(rgba(seed))

    def admits(p):
        return all(abs(v - r) <= tolerance for v, r in zip(projections(rgba(p)), ref))
    if not contiguous:
        return {p for p in scene if admits(p)}
    seen, queue = {seed}, deque([seed])
    while queue:
        x, y = queue.popleft()
        for q in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if q in scene and q not in seen and admits(q):
                seen.add(q)
                queue.append(q)
    return seen


def read(call, doc, x, y, layer):
    """The layer pixel's own numbers: linear f32 at 32 bits, else its codes (a CMYK pixel's ink
    and alpha, alpha last)."""
    reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=layer)
    for key in ('center_rgba32', 'center_rgba_codes', 'mean_rgba16'):
        if reply.get(key) is not None:
            return [float(v) for v in reply[key]]
    if reply.get('ink16') is not None and reply.get('alpha16') is not None:
        return [float(v) for v in reply['ink16']] + [float(reply['alpha16'])]
    raise AssertionError(('no readout', sorted(reply)))


def grid(call, doc, layer):
    return {(x, y): read(call, doc, x, y, layer) for y in range(H) for x in range(W)}


def history(call, doc):
    reply = call('list_history', document_id=doc)
    return (len(reply['states']), reply['cursor'])


def fill_rect(call, doc, rect, colour, layer):
    x, y, w, h = rect
    call('set_active_layer', document_id=doc, index=layer)
    call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
    call('fill_selection', document_id=doc, color=hexcode(colour))
    call('deselect', document_id=doc)


def scene_document(call, spec, layered=True):
    """S (module doc) in the document `spec` makes; `layered` false paints A, B and N on a
    transparent layer 0 alone (the colour modes)."""
    doc = call('new_document', width=W, height=H,
               background='white' if layered else 'transparent', **spec)['document']['id']
    layer = 0
    if layered:
        fill_rect(call, doc, UNDER_B, BLACK, 0)
        call('new_layer', document_id=doc)
        layer = 1
    for (x, y, w, h, colour) in SQUARES.values():
        fill_rect(call, doc, (x, y, w, h), colour, layer)
    if layered:
        call('set_layer_properties', document_id=doc, index=1, opacity=0.5)
    call('set_active_layer', document_id=doc, index=layer)
    return doc, layer


def check_erase(depth, before, after, erased, opacity):
    scale, atol, ctol = DEPTHS[depth], ALPHA_TOL[depth], COLOUR_TOL[depth]
    for p, b in before.items():
        a = after[p]
        if p not in erased:
            assert a == b, (depth, 'outside the region', p, b, a)
            continue
        if b[3] == 0:
            assert a == b, (depth, 'a clear pixel stays clear', p, b, a)
            continue
        want = b[3] * (1 - opacity)
        if depth in (8, 16):
            want = float(int(want + 0.5))
        assert abs(a[3] - want) <= atol, (depth, 'alpha', p, b, a, want)
        if a[3] > 0:
            assert all(abs(x - y) <= ctol for x, y in zip(a[:3], b[:3])), (depth, 'colour', p, b, a)
    return 'ok'


def erase_case(call, depth, name, erased, opacity=1.0, select=None, **arguments):
    """One M1 case on a fresh scene; the document is closed whatever the case found."""
    doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
    try:
        return erase_on(call, depth, name, doc, layer, erased, opacity, select, arguments)
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def erase_on(call, depth, name, doc, layer, erased, opacity, select, arguments):
    if select:
        call('select_rect', document_id=doc, **select)
    before = grid(call, doc, layer)
    count = history(call, doc)
    reply = call('magic_eraser', document_id=doc, x=SEED[0], y=SEED[1], opacity=opacity,
                 **arguments)
    after = grid(call, doc, layer)
    if not erased:
        assert reply.get('changed') is False, (depth, name, reply)
        assert after == before and history(call, doc) == count, (depth, name, 'moved')
        return 'unchanged'
    assert reply.get('changed') is True, (depth, name, reply)
    check_erase(depth, before, after, erased, opacity)
    assert history(call, doc)[0] == count[0] + 1, (depth, name, 'one undo step')
    call('undo', document_id=doc)
    assert grid(call, doc, layer) == before, (depth, name, 'undo restores the layer')
    return reply.get('storage')


def check_rgb(call, depth):
    """Every case runs; a failed one is recorded as `FAIL: …` (main exits non-zero), so one run
    names each case a defect breaks."""
    scene = layer_scene()
    a_region = region(scene, SEED, 0, True)
    # The composite's region and the layer's differ here: B is erased only without it.
    assert region(composite_scene(), SEED, 0, False) != region(scene, SEED, 0, False)
    cases = {
        'contiguous': (a_region, dict(tolerance=0)),
        'non-contiguous': (region(scene, SEED, 0, False), dict(tolerance=0, contiguous=False)),
        'near colour': (region(scene, SEED, 20, False), dict(tolerance=20, contiguous=False)),
        'opacity': (a_region, dict(opacity=0.5, tolerance=0)),
        'selection': (a_region & rect_pixels(1, 1, 2, 4),
                      dict(tolerance=0, select=dict(x=1, y=1, width=2, height=4))),
        'outside selection': (set(), dict(tolerance=0, select=dict(x=4, y=1, width=1, height=4))),
        'sample all layers': (
            region(composite_scene(), SEED, 0, False) & set(scene_pixels(scene)),
            dict(tolerance=0, contiguous=False, sample_all_layers=True)),
    }
    out = {}
    for name, (erased, arguments) in cases.items():
        try:
            out[name] = erase_case(call, depth, name, erased, **arguments)
        except AssertionError as failure:
            out[name] = f'FAIL: {str(failure)[:300]}'
    return out


def scene_pixels(scene):
    return [p for p, c in scene.items() if c is not None]


def check_modes(call):
    out = {}
    scene = layer_scene()
    for name, spec in MODES:
        for contiguous in (True, False):
            doc, layer = scene_document(call, spec, layered=False)
            before = grid(call, doc, layer)
            reply = call('magic_eraser', document_id=doc, x=SEED[0], y=SEED[1], tolerance=0,
                         contiguous=contiguous)
            assert reply.get('changed') is True, (name, reply)
            after = grid(call, doc, layer)
            erased = region(scene, SEED, 0, contiguous)
            for p, b in before.items():
                a = after[p]
                if p in erased:
                    assert a[-1] == 0, (name, contiguous, 'erased', p, b, a)
                else:
                    assert a == b, (name, contiguous, 'outside', p, b, a)
            out[f'{name} contiguous={contiguous}'] = reply.get('storage')
            call('close_document', document_id=doc, discard_changes=True)
    doc, layer = scene_document(call, dict(depth=8, profile='srgb'), layered=False)
    call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither='none')
    before = grid(call, doc, 0)
    reply = call('magic_eraser', document_id=doc, x=SEED[0], y=SEED[1], tolerance=0)
    after = grid(call, doc, 0)
    erased = region(scene, SEED, 0, True)
    for p, b in before.items():
        if p in erased:
            assert after[p][-1] == 0, ('indexed erased', p, b, after[p])
        else:
            assert after[p] == b, ('indexed outside', p, b, after[p])
    out['indexed'] = reply.get('storage')
    call('close_document', document_id=doc, discard_changes=True)
    return out


def refused(call, doc, key, **arguments):
    count = history(call, doc)
    text = call('magic_eraser', fail=True, document_id=doc, x=SEED[0], y=SEED[1], **arguments)
    if key is not None:
        assert NAMED[key] in text, (key, text)
    assert history(call, doc) == count, (key, 'the history moved')
    return text


def check_refusals(call):
    out = {}
    for depth in (8, 16):
        doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
        out[f'{depth} blend'] = refused(call, doc, 'blend', blend_mode='Normal')
        call('add_layer_mask', document_id=doc, layer=layer)
        out[f'{depth} mask'] = refused(call, doc, 'mask', target='mask', layer=layer)
        call('set_quick_mask', document_id=doc, active=True)
        out[f'{depth} quick mask'] = refused(call, doc, 'quick_mask', layer=layer)
        call('set_quick_mask', document_id=doc, active=False)
        call('set_layer_lock', document_id=doc, layer=layer, locks=['pixels'])
        out[f'{depth} pixels'] = refused(call, doc, 'pixels', layer=layer)
        call('set_layer_lock', document_id=doc, layer=layer, locks=['transparency'])
        out[f'{depth} transparency'] = refused(call, doc, 'transparency', layer=layer)
        call('set_layer_lock', document_id=doc, layer=layer, locks=[])
        group = call('group_layers', document_id=doc, layers=[layer])
        group_index = group.get('group', group.get('index'))
        if not isinstance(group_index, int):
            group_index = call('get_document', document_id=doc)['layers'][-1]['index']
        call('set_layer_lock', document_id=doc, layer=group_index, locks=['transparency'])
        out[f'{depth} group transparency'] = refused(call, doc, 'group_transparency', layer=layer)
        out[f'{depth} group'] = refused(call, doc, None, layer=group_index)
        call('set_layer_lock', document_id=doc, layer=group_index, locks=[])
        adjustment = call('add_adjustment_layer', document_id=doc, op='invert')
        index = adjustment.get('layer', adjustment.get('index'))
        out[f'{depth} adjustment'] = refused(call, doc, None, layer=index)
        call('close_document', document_id=doc, discard_changes=True)
        # A shape layer rasterizes and erases.
        doc, _ = scene_document(call, dict(depth=depth, profile='srgb'))
        call('add_shape_layer', document_id=doc, kind='rect', x=6, y=6, w=8, h=3, fill='#3366cc')
        shapes = call('get_document', document_id=doc)['layers']
        shape = next(i for i, row in enumerate(shapes) if row.get('shape') is not None)
        reply = call('magic_eraser', document_id=doc, layer=shape, x=9, y=7, tolerance=0)
        assert reply.get('changed') is True, (depth, 'shape', reply)
        if depth == 8:
            assert reply.get('rasterized_shape') is True, (depth, reply)
        rows = call('get_document', document_id=doc)['layers']
        assert rows[shape].get('shape') is None, (depth, 'the shape description stays', rows[shape])
        assert read(call, doc, 9, 7, shape)[3] == 0, (depth, 'the shape pixel is erased')
        out[f'{depth} shape'] = 'rasterized'
        call('close_document', document_id=doc, discard_changes=True)
    bitmap = call('new_document', width=W, height=H, background='white', color_mode='gray',
                  depth=8)['document']['id']
    call('convert_mode', document_id=bitmap, mode='bitmap', method='threshold')
    out['bitmap'] = refused(call, bitmap, 'bitmap')
    call('close_document', document_id=bitmap, discard_changes=True)
    return out


def check_replay(call):
    doc, layer = scene_document(call, dict(depth=8, profile='srgb'))
    before = grid(call, doc, layer)
    call('start_recording', discard=True)
    call('magic_eraser', document_id=doc, x=SEED[0], y=SEED[1], tolerance=0, contiguous=False,
         opacity=0.5)
    recorded = grid(call, doc, layer)
    call('stop_recording', name='magic-eraser')
    call('undo', document_id=doc)
    assert grid(call, doc, layer) == before, 'undo restores the scene'
    call('run_action', document_id=doc, name='magic-eraser')
    assert grid(call, doc, layer) == recorded, 'the recorded step replays differently'
    call('close_document', document_id=doc, discard_changes=True)
    return 'ok'


def check_faded(label, before, after, erased, tol):
    """M5's rule: inside the erased region alpha is FADE_OPACITY of the scene's and the colour is
    the scene's, within `tol`; every other pixel is the scene's exactly."""
    for p, b in before.items():
        a = after[p]
        if p not in erased:
            assert a == b, (label, 'outside the region', p, b, a)
            continue
        assert b[-1] > 0, (label, 'the scene is opaque inside the region', p, b)
        assert abs(a[-1] - b[-1] * FADE_OPACITY) <= tol, (label, 'alpha', p, b, a)
        assert all(abs(x - y) <= tol for x, y in zip(a[:-1], b[:-1])), (label, 'colour', p, b, a)


def erase_a(call, doc, label):
    reply = call('magic_eraser', document_id=doc, x=SEED[0], y=SEED[1], tolerance=0)
    assert reply.get('changed') is True, (label, 'the click erases A', reply)


def faded_reply(call, doc, label):
    reply = call('fade', document_id=doc, opacity=FADE_OPACITY)
    assert (reply.get('changed') is True and reply.get('faded') == FADED
            and reply.get('name') == FADE_NAME), (label, reply)


def fade_rgb(call, depth):
    """Erase A on S at `depth`, fade it to half, then undo."""
    doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
    try:
        before = grid(call, doc, layer)
        erase_a(call, doc, depth)
        count = history(call, doc)
        faded_reply(call, doc, depth)
        listed = call('list_history', document_id=doc)
        assert len(listed['states']) == count[0], (depth, 'the fade replaces the erase', listed)
        assert listed['states'][listed['cursor']]['name'] == FADE_NAME, (depth, listed)
        check_faded(depth, before, grid(call, doc, layer), region(layer_scene(), SEED, 0, True),
                    FADE_TOL[depth])
        call('undo', document_id=doc)
        assert grid(call, doc, layer) == before, (depth, 'one undo restores the scene')
        return 'ok'
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def fade_after_selection(call):
    """A selection change between the click and the fade leaves nothing to fade."""
    doc, _ = scene_document(call, dict(depth=8, profile='srgb'))
    try:
        erase_a(call, doc, 'selection change')
        call('select_rect', document_id=doc, x=0, y=0, width=W, height=H)
        count = history(call, doc)
        text = call('fade', fail=True, document_id=doc, opacity=FADE_OPACITY)
        assert NOTHING_TO_FADE in text, ('selection change', text)
        assert history(call, doc) == count, ('selection change', 'the history moved')
        return 'refused'
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def fade_mode(call, name, spec):
    """Erase A on M2's one-layer scene in a colour mode and fade it to half. The readout's units
    are the opaque alpha's (255 or 65535), so one 8-bit code is that over 255."""
    doc, layer = scene_document(call, spec, layered=False)
    try:
        before = grid(call, doc, layer)
        full = max(b[-1] for b in before.values())
        assert full in (255.0, 65535.0), (name, 'an opaque readout', full)
        erase_a(call, doc, name)
        faded_reply(call, doc, name)
        check_faded(name, before, grid(call, doc, layer), region(layer_scene(), SEED, 0, True),
                    full / 255)
        return 'ok'
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def fade_indexed(call):
    """Indexed Color offers Fade after no step: the click's fade answers the mode's sentence."""
    doc, _ = scene_document(call, dict(depth=8, profile='srgb'), layered=False)
    try:
        call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither='none')
        erase_a(call, doc, 'indexed')
        count = history(call, doc)
        text = call('fade', fail=True, document_id=doc, opacity=FADE_OPACITY)
        assert INDEXED_FADE in text, ('indexed', text)
        assert history(call, doc) == count, ('indexed', 'the history moved')
        return 'refused'
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def check_fade(call):
    """M5. Every case runs; a failed one is recorded as `FAIL: …`, as M1's are."""
    cases = {f'rgb {depth}': (lambda depth=depth: fade_rgb(call, depth)) for depth in DEPTHS}
    cases['selection change'] = lambda: fade_after_selection(call)
    for name, spec in MODES:
        cases[name] = lambda name=name, spec=spec: fade_mode(call, name, spec)
    cases['indexed'] = lambda: fade_indexed(call)
    out = {}
    for name, run in cases.items():
        try:
            out[name] = run()
        except AssertionError as failure:
            out[name] = f'FAIL: {str(failure)[:300]}'
    return out


def stops(count, midpoints):
    out = []
    for i in range(count):
        stop = {'position': i / (count - 1), 'color': ['#102030', '#c04080', '#40c080', '#f0e0a0'][i],
                'opacity': 1}
        if midpoints and i < count - 1:
            stop['midpoint'] = 0.3
        out.append(stop)
    return out


def check_styles(call):
    doc = call('new_document', width=W, height=H, background='white', depth=8,
               profile='srgb')['document']['id']
    call('new_layer', document_id=doc)
    fill_rect(call, doc, (2, 2, 8, 6), C1, 1)
    style = {'effects': [
        {'type': 'gradient_overlay', 'gradient': {'stops': stops(4, True)}},
        {'type': 'outer_glow', 'fill_type': 'gradient', 'gradient': {'stops': stops(4, True)}},
        {'type': 'stroke', 'fill_type': 'gradient', 'gradient': {'stops': stops(4, True)}},
    ]}
    reply = call('set_layer_style', document_id=doc, layer=1, style=style)
    effects = {e['type']: e for e in reply['style']['effects']}
    out = {}
    for kind in ('gradient_overlay', 'outer_glow', 'stroke'):
        got = effects[kind]['gradient']['stops']
        assert [s['color'] for s in got] == [s['color'] for s in stops(4, False)], (kind, got)
        kept = [s.get('midpoint') for s in got[:-1]]
        if kind == 'outer_glow':
            assert all(m is None for m in kept), (kind, 'a glow keeps no midpoint', got)
        else:
            assert all(abs(m - 0.3) < 1e-9 for m in kept), (kind, 'midpoints kept', got)
        out[kind] = len(got)
    call('close_document', document_id=doc, discard_changes=True)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--sections', default='M0,M1,M2,M3,M4,M5',
                        help='Comma-separated sections to run (default all).')
    args = parser.parse_args()
    sections = set(args.sections.split(','))
    output = Path(args.receipts or tempfile.mkdtemp(prefix='magic-eraser-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_magic_eraser.py'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        if 'M0' in sections:
            receipts['M0 styles'] = check_styles(call)
        if 'M1' in sections:
            for depth in DEPTHS:
                receipts[f'M1 rgb {depth}'] = check_rgb(call, depth)
        if 'M2' in sections:
            receipts['M2 modes'] = check_modes(call)
        if 'M3' in sections:
            receipts['M3 refusals'] = check_refusals(call)
        if 'M4' in sections:
            receipts['M4 replay'] = check_replay(call)
        if 'M5' in sections:
            receipts['M5 fade'] = check_fade(call)
    assert len(receipts) > 1, ('no section ran', args.sections)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    print(json.dumps(receipts, indent=2))
    failed = [f'{section}: {case}' for section, value in receipts.items() if isinstance(value, dict)
              for case, result in value.items() if str(result).startswith('FAIL')]
    if failed:
        print('FAIL: ' + '; '.join(failed))
        sys.exit(1)
    print('PASS: the Magic Eraser over MCP at 8, 16 and 32 bits, in Gray, Lab, CMYK and Indexed, '
          'with its refusals, undo, replay and Fade; the style panes\' stops read back whole')


if __name__ == '__main__':
    main()
