#!/usr/bin/env python3
"""The Background Eraser over MCP (`background_eraser_stroke`, `designs/background-eraser.md`) on
an owned Debug instance.

The gates are FIXED here before anything is measured, and the oracle is written from the ADR alone.
Every stroke is explicit `native_stroke` dabs, so the oracle knows each centre. Most dabs are
pixel-mode squares of odd diameter on a pixel centre, which cover whole pixels at coverage 1: a
reached pixel's alpha becomes exactly 0 and every other pixel stays exactly as it was. A dab's
reference is the colour under its centre (continuous), the stroke's first such colour (once) or
`sample_color` (background_swatch); a pixel is like it when R, G, B, R−G, G−B and R−B each lie
within the tolerance (8-bit codes here; every colour below sits clear of each tolerance used);
contiguous walks four-connected from the centre through like pixels under the tip; a pixel like
`protect_color` is never reached. The instance is owned by this file (`phase5_support.App`); no
Doctored process it did not start is signalled.

Scene (S): a 16 × 12 document, a white Background (layer 0) and layer 1, opaque: green G
everywhere, a red wall R at x = 7 the full height, blue B at x 12–15, and one near-green N at
(2, 5), 10 codes from G in R, G and B. Layer 1 is active.

- **B1 RGB at 8, 16 and 32 bits.** On S, each from the untouched scene: contiguous and
  discontiguous (the green behind the wall is reached only discontiguous), tolerance 20 (N too),
  once and background_swatch against continuous across the green and the blue, Protect Foreground
  (N, like G at tolerance 12 and like the protected colour, stays; G does not), a dab centred on a
  protected pixel (contiguous: nothing), a selection, a swatch nothing matches (changed false, the
  history unchanged), and a hard round tip (a like pixel within radius − 1 of the centre cleared,
  every pixel beyond radius + 1 and every unlike pixel unchanged, the band between it in between).
  One undo restores the layer exactly.
- **B2 colour modes.** Grayscale, Lab and CMYK (8 bits), the scene's layer alone on a transparent
  background, its last row left clear: contiguous and discontiguous at tolerance 0 (a clear pixel
  is never reached). Indexed (converted from an RGB scene):
  the reached pixels read clear and nothing else moves.
- **B3 refusals.** Each answers in-band and leaves the history unchanged: Bitmap, target mask,
  blend_mode, flow, the Quick Mask, a pixels lock, a transparency lock (the layer's own and an
  enclosing group's), a group, an adjustment layer, sample_color without background_swatch,
  background_swatch without sample_color, and the 256-million-work-unit bound, at 8 and 16 bits.
  A shape layer rasterizes and erases.
- **B4 replay.** A recorded `background_eraser_stroke` replays to the same pixels.
- **B5 fade.** Edit ▸ Fade follows a stroke: erasing the green and fading Normal at 0.5 leaves it at
  half alpha with its colour (one code at 8 and 16 bits) and every other pixel exactly the scene's;
  the reply names "Fade Background Eraser". At 8 and 16 bits.
- **B6 default size.** `points` with no `size` lays the tool's 40 px tip with or without
  `stroke_version: 1`: one point at (4.5, 4.5) on an opaque green 48 × 24 layer clears row 4's
  pixels within radius − 1 (19 px) of the centre and leaves those beyond radius + 1 (21 px), and
  the versioned call's row equals the unversioned call's. At 8 and 16 bits.

Usage: python3 -B scripts/verify_background_eraser.py [--receipts <folder>] [--sections B1,B2,…]
"""
import argparse
import base64
from collections import deque
import json
import math
import os
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 16, 12
G, R, B, N = (40, 160, 60), (200, 40, 40), (30, 60, 200), (50, 170, 70)
PROTECT = (60, 180, 80)  # like N at 12, not like G
WHITE = (255, 255, 255)
DEPTHS = {8: 255.0, 16: 65535.0, 32: 1.0}
FADE_TOL = {8: 1.0, 16: 1.0, 32: 1e-6}
MODES = (('gray', dict(color_mode='gray', depth=8)),
         ('lab', dict(color_mode='lab', depth=8)),
         ('cmyk', dict(color_mode='cmyk', depth=8)))
NAMED = {
    'bitmap': 'background_eraser_stroke is not available in Bitmap mode.',
    'mask': 'The Background Eraser erases a layer\'s own pixels',
    'blend': 'The Background Eraser has no blend mode',
    'flow': 'background_eraser_stroke has no flow, opacity',
    'quick_mask': 'Exit Quick Mask before editing layer pixels',
    'pixels': 'has its pixels locked',
    'transparency': 'has its transparency locked. The Background Eraser changes transparency',
    'group_transparency': 'which has its transparency locked. The Background Eraser changes',
    'swatch_unused': 'sample_color and sample_ink name the colour background_swatch',
    'swatch_missing': 'background_swatch sampling erases sample_color',
    'bound': 'background eraser exceeds 256 million work units',
}
FADE_NAME = 'Fade Background Eraser'
DEFAULT_SIZE = 40  # the tool's default diameter, the catalog's `size` default


def hexcode(rgb):
    return '#%02x%02x%02x' % rgb


def scene_colour(x, y):
    if (x, y) == (2, 5):
        return N
    if x == 7:
        return R
    if x >= 12:
        return B
    return G


def mode_colour(x, y):
    """The one-layer scene (B2): S's layer with its last row transparent (None), so an Indexed
    conversion keeps a transparent index for the erased pixels to take."""
    return None if y == H - 1 else scene_colour(x, y)


def native(dabs, hardness=1.0, pixel=True):
    data = b''.join(struct.pack('<4d', *d) for d in dabs)
    return {'version': 1, 'encoding': 'le_f64_xy_diameter_opacity',
            'dabs': base64.b64encode(data).decode(), 'hardness': hardness, 'roundness': 1.0,
            'angle_degrees': 0.0, 'pixel': pixel}


def like(a, b, t):
    p = lambda c: (c[0], c[1], c[2], c[0] - c[1], c[1] - c[2], c[0] - c[2])
    return all(abs(x - y) <= t for x, y in zip(p(a), p(b)))


def square(d):
    """A pixel-mode dab's covered pixels: those overlapping its square."""
    x, y, size = d[0], d[1], d[2] / 2
    return {(px, py) for py in range(H) for px in range(W)
            if px + 1 > x - size and px < x + size and py + 1 > y - size and py < y + size}


def reached(dabs, t, contiguous=True, sampling='continuous', swatch=None, protect=None,
            colour=scene_colour):
    """The ADR's rule over the scene, pixel-mode dabs."""
    out, once = set(), None
    for d in dabs:
        hot = (math.floor(d[0]), math.floor(d[1]))
        on = 0 <= hot[0] < W and 0 <= hot[1] < H
        hot_colour = colour(*hot) if on else None
        if sampling == 'continuous':
            reference = hot_colour
        elif sampling == 'once':
            once = once or hot_colour
            reference = once
        else:
            reference = swatch
        if reference is None:
            continue
        tip = square(d)

        def admits(p):
            c = colour(*p)
            return (p in tip and c is not None and like(reference, c, t)
                    and not (protect and like(protect, c, t)))
        if not contiguous:
            out |= {p for p in tip if admits(p)}
            continue
        if not on or not admits(hot):
            continue
        seen, queue = {hot}, deque([hot])
        while queue:
            x, y = queue.popleft()
            for q in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                if q not in seen and admits(q):
                    seen.add(q)
                    queue.append(q)
        out |= seen
    return out


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
    """S (module doc); `layered` false paints `mode_colour`'s scene on a transparent layer 0
    alone."""
    doc = call('new_document', width=W, height=H,
               background='white' if layered else 'transparent', **spec)['document']['id']
    layer, rows = 0, H if layered else H - 1
    if layered:
        call('new_layer', document_id=doc)
        layer = 1
    fill_rect(call, doc, (0, 0, W, rows), G, layer)
    fill_rect(call, doc, (7, 0, 1, rows), R, layer)
    fill_rect(call, doc, (12, 0, 4, rows), B, layer)
    fill_rect(call, doc, (2, 5, 1, 1), N, layer)
    call('set_active_layer', document_id=doc, index=layer)
    return doc, layer


def stroke(call, doc, dabs, fail=False, hardness=1.0, pixel=True, **arguments):
    return call('background_eraser_stroke', fail=fail, document_id=doc,
                native_stroke=native(dabs, hardness, pixel),
                points=[[d[0], d[1]] for d in dabs], **arguments)


def check_erased(label, before, after, erased):
    for p, b in before.items():
        a = after[p]
        if p in erased:
            assert a[-1] == 0, (label, 'reached', p, b, a)
        else:
            assert a == b, (label, 'not reached', p, b, a)


# The dabs B1 strokes with: one wide square over the left green, the wall and the green behind it;
# a row across the green and the blue; one square centred on N.
WIDE = [(3.5, 5.5, 13.0, 1.0)]
ROW = [(3.5, 5.5, 5.0, 1.0), (10.5, 5.5, 5.0, 1.0), (13.5, 5.5, 5.0, 1.0)]
ON_N = [(2.5, 5.5, 3.0, 1.0)]


def rgb_case(call, depth, name, dabs, erased, select=None, **arguments):
    """One B1 case on a fresh scene; the document is closed whatever the case found."""
    doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
    try:
        if select:
            call('select_rect', document_id=doc, **select)
        before = grid(call, doc, layer)
        count = history(call, doc)
        reply = stroke(call, doc, dabs, **arguments)
        after = grid(call, doc, layer)
        if not erased:
            assert reply.get('changed') is False, (depth, name, reply)
            assert after == before and history(call, doc) == count, (depth, name, 'moved')
            return 'unchanged'
        assert reply.get('changed') is not False, (depth, name, reply)
        check_erased((depth, name), before, after, erased)
        assert history(call, doc)[0] == count[0] + 1, (depth, name, 'one undo step')
        call('undo', document_id=doc)
        assert grid(call, doc, layer) == before, (depth, name, 'undo restores the layer')
        return reply.get('storage')
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def round_tip(call, depth):
    """A hard round dab of diameter 9 at (3.5, 5.5): like pixels within radius − 1 cleared, every
    pixel beyond radius + 1 and every unlike pixel unchanged, the band in between."""
    doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
    try:
        before = grid(call, doc, layer)
        stroke(call, doc, [(3.5, 5.5, 9.0, 1.0)], pixel=False, tolerance=0,
               limits='discontiguous')
        after = grid(call, doc, layer)
        radius, partial = 4.5, 0
        for (x, y), b in before.items():
            a = after[(x, y)]
            distance = math.hypot(x + 0.5 - 3.5, y + 0.5 - 5.5)
            if scene_colour(x, y) != G or distance > radius + 1:
                assert a == b, (depth, 'round: unchanged', (x, y), b, a)
            elif distance <= radius - 1:
                assert a[-1] == 0, (depth, 'round: cleared', (x, y), b, a)
            else:
                assert 0 <= a[-1] <= b[-1], (depth, 'round: band', (x, y), b, a)
                partial += 0 < a[-1] < b[-1]
        assert partial > 0, (depth, 'a hard round tip leaves a partial band')
        return 'ok'
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def check_rgb(call, depth):
    """Every case runs; a failed one is recorded as `FAIL: …` (main exits non-zero)."""
    gate = dict(x=0, y=0, width=4, height=H)
    selected = {(x, y) for y in range(H) for x in range(4)}
    cases = {
        'contiguous': (WIDE, reached(WIDE, 0), dict(tolerance=0)),
        'discontiguous': (WIDE, reached(WIDE, 0, contiguous=False),
                          dict(tolerance=0, limits='discontiguous')),
        'tolerance 20': (WIDE, reached(WIDE, 20), dict(tolerance=20)),
        'continuous row': (ROW, reached(ROW, 0, contiguous=False),
                           dict(tolerance=0, limits='discontiguous')),
        'once row': (ROW, reached(ROW, 0, contiguous=False, sampling='once'),
                     dict(tolerance=0, limits='discontiguous', sampling='once')),
        'swatch row': (ROW, reached(ROW, 0, contiguous=False, sampling='swatch', swatch=B),
                       dict(tolerance=0, limits='discontiguous', sampling='background_swatch',
                            sample_color=hexcode(B))),
        'protect': (WIDE, reached(WIDE, 12, contiguous=False, protect=PROTECT),
                    dict(tolerance=12, limits='discontiguous', protect_color=hexcode(PROTECT))),
        'protected centre': (ON_N, reached(ON_N, 12, protect=PROTECT),
                             dict(tolerance=12, protect_color=hexcode(PROTECT))),
        'selection': (WIDE, reached(WIDE, 0) & selected, dict(tolerance=0, select=gate)),
        'nothing like the swatch': (
            [(3.5, 5.5, 5.0, 1.0)], set(),
            dict(tolerance=0, sampling='background_swatch', sample_color=hexcode(B))),
    }
    # The oracle's cases differ where the rule says they must.
    assert (9, 5) in cases['discontiguous'][1] and (9, 5) not in cases['contiguous'][1]
    assert (2, 5) in cases['tolerance 20'][1] and (2, 5) not in cases['contiguous'][1]
    assert cases['once row'][1] != cases['continuous row'][1] != cases['swatch row'][1]
    assert (2, 5) not in cases['protect'][1] and (5, 5) in cases['protect'][1]
    assert not cases['protected centre'][1]
    out = {}
    for name, (dabs, erased, arguments) in cases.items():
        try:
            out[name] = rgb_case(call, depth, name, dabs, erased, **arguments)
        except AssertionError as failure:
            out[name] = f'FAIL: {str(failure)[:300]}'
    try:
        out['round tip'] = round_tip(call, depth)
    except AssertionError as failure:
        out['round tip'] = f'FAIL: {str(failure)[:300]}'
    return out


def check_modes(call):
    out = {}
    for name, spec in MODES:
        for contiguous in (True, False):
            doc, layer = scene_document(call, spec, layered=False)
            try:
                before = grid(call, doc, layer)
                reply = stroke(call, doc, WIDE, tolerance=0,
                               limits='contiguous' if contiguous else 'discontiguous')
                check_erased((name, contiguous), before, grid(call, doc, layer),
                             reached(WIDE, 0, contiguous=contiguous, colour=mode_colour))
                out[f'{name} contiguous={contiguous}'] = reply.get('storage')
            except AssertionError as failure:
                out[f'{name} contiguous={contiguous}'] = f'FAIL: {str(failure)[:300]}'
            finally:
                call('close_document', document_id=doc, discard_changes=True)
    doc, layer = scene_document(call, dict(depth=8, profile='srgb'), layered=False)
    try:
        call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither='none')
        before = grid(call, doc, 0)
        reply = stroke(call, doc, WIDE, tolerance=0, limits='discontiguous')
        check_erased('indexed', before, grid(call, doc, 0),
                     reached(WIDE, 0, contiguous=False, colour=mode_colour))
        out['indexed'] = reply.get('storage')
    except AssertionError as failure:
        out['indexed'] = f'FAIL: {str(failure)[:300]}'
    finally:
        call('close_document', document_id=doc, discard_changes=True)
    return out


def refused(call, doc, key, dabs=WIDE, **arguments):
    count = history(call, doc)
    text = stroke(call, doc, dabs, fail=True, **arguments)
    if key is not None:
        assert NAMED[key] in text, (key, text)
    assert history(call, doc) == count, (key, 'the history moved')
    return text[:120]


def check_refusals(call):
    out = {}
    for depth in (8, 16):
        doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
        out[f'{depth} blend'] = refused(call, doc, 'blend', blend_mode='Normal')
        out[f'{depth} flow'] = refused(call, doc, 'flow', flow=50)
        out[f'{depth} swatch unused'] = refused(call, doc, 'swatch_unused',
                                                sample_color=hexcode(B))
        out[f'{depth} swatch missing'] = refused(call, doc, 'swatch_missing',
                                                 sampling='background_swatch')
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
        # 110 hard 1000 px dabs on a 1200 px canvas: each charges its box (10⁶) and its tip's
        # band work, about 2.6 million, so the stroke plans past 256 million before any pixel.
        doc = call('new_document', width=1200, height=1200, background='white', depth=depth,
                   profile='srgb')['document']['id']
        out[f'{depth} bound'] = refused(call, doc, 'bound', dabs=[(600.5, 600.5, 1000.0, 1.0)] * 110,
                                        pixel=False, size=1000)
        call('close_document', document_id=doc, discard_changes=True)
        # A shape layer rasterizes and erases.
        doc, _ = scene_document(call, dict(depth=depth, profile='srgb'))
        call('add_shape_layer', document_id=doc, kind='rect', x=2, y=2, w=12, h=8, fill='#3366cc')
        shapes = call('get_document', document_id=doc)['layers']
        shape = next(i for i, row in enumerate(shapes) if row.get('shape') is not None)
        reply = stroke(call, doc, [(8.5, 5.5, 3.0, 1.0)], layer=shape, tolerance=0)
        assert reply.get('changed') is not False, (depth, 'shape', reply)
        rows = call('get_document', document_id=doc)['layers']
        assert rows[shape].get('shape') is None, (depth, 'the shape description stays', rows[shape])
        assert read(call, doc, 8, 5, shape)[3] == 0, (depth, 'the shape pixel is erased')
        out[f'{depth} shape'] = 'rasterized'
        call('close_document', document_id=doc, discard_changes=True)
    bitmap = call('new_document', width=W, height=H, background='white', color_mode='gray',
                  depth=8)['document']['id']
    call('convert_mode', document_id=bitmap, mode='bitmap', method='threshold')
    out['bitmap'] = refused(call, bitmap, 'bitmap')
    call('close_document', document_id=bitmap, discard_changes=True)
    return out


def check_replay(call):
    out = {}
    for depth in (8, 16):
        doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
        before = grid(call, doc, layer)
        call('start_recording', discard=True)
        stroke(call, doc, ROW, tolerance=0, limits='discontiguous', sampling='once')
        recorded = grid(call, doc, layer)
        call('stop_recording', name=f'background-eraser-{depth}')
        assert recorded != before, (depth, 'the stroke erased')
        call('undo', document_id=doc)
        assert grid(call, doc, layer) == before, (depth, 'undo restores the scene')
        call('run_action', document_id=doc, name=f'background-eraser-{depth}')
        assert grid(call, doc, layer) == recorded, (depth, 'the recorded step replays differently')
        call('close_document', document_id=doc, discard_changes=True)
        out[depth] = 'ok'
    return out


def check_fade(call):
    out = {}
    for depth in (8, 16):
        doc, layer = scene_document(call, dict(depth=depth, profile='srgb'))
        try:
            before = grid(call, doc, layer)
            stroke(call, doc, WIDE, tolerance=0)
            reply = call('fade', document_id=doc, opacity=0.5)
            assert reply.get('changed') is True and reply.get('name') == FADE_NAME, (depth, reply)
            after = grid(call, doc, layer)
            erased = reached(WIDE, 0)
            for p, b in before.items():
                a = after[p]
                if p not in erased:
                    assert a == b, (depth, 'fade: outside', p, b, a)
                    continue
                assert abs(a[-1] - b[-1] * 0.5) <= FADE_TOL[depth], (depth, 'fade alpha', p, b, a)
                assert all(abs(x - y) <= FADE_TOL[depth] for x, y in zip(a[:-1], b[:-1])), (
                    depth, 'fade colour', p, b, a)
            out[depth] = 'ok'
        except AssertionError as failure:
            out[depth] = f'FAIL: {str(failure)[:300]}'
        finally:
            call('close_document', document_id=doc, discard_changes=True)
    return out


def default_size_row(call, depth, versioned):
    """B6: one `points` call with no `size` on a fresh green layer; row 4's alphas after it."""
    width, height = 48, 24
    doc = call('new_document', width=width, height=height, background='white', depth=depth,
               profile='srgb')['document']['id']
    try:
        call('new_layer', document_id=doc)
        fill_rect(call, doc, (0, 0, width, height), G, 1)
        before = [read(call, doc, x, 4, 1) for x in range(width)]
        extra = dict(stroke_version=1) if versioned else {}
        reply = call('background_eraser_stroke', document_id=doc, layer=1, points=[[4.5, 4.5]],
                     **extra)
        assert reply.get('changed') is not False, (depth, versioned, reply)
        after = [read(call, doc, x, 4, 1) for x in range(width)]
        radius = DEFAULT_SIZE / 2
        for x, (b, a) in enumerate(zip(before, after)):
            distance = abs(x + 0.5 - 4.5)
            if distance <= radius - 1:
                assert a[-1] == 0, (depth, versioned, 'cleared', x, b, a)
            elif distance > radius + 1:
                assert a == b, (depth, versioned, 'unchanged', x, b, a)
        return [a[-1] for a in after]
    finally:
        call('close_document', document_id=doc, discard_changes=True)


def check_default_size(call):
    out = {}
    for depth in (8, 16):
        try:
            plain = default_size_row(call, depth, versioned=False)
            versioned = default_size_row(call, depth, versioned=True)
            assert versioned == plain, (depth, 'stroke_version changed the tip', plain, versioned)
            out[depth] = 'ok'
        except AssertionError as failure:
            out[depth] = f'FAIL: {str(failure)[:300]}'
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--sections', default='B1,B2,B3,B4,B5,B6',
                        help='Comma-separated sections to run (default all).')
    args = parser.parse_args()
    sections = set(args.sections.split(','))
    output = Path(args.receipts or tempfile.mkdtemp(prefix='background-eraser-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_background_eraser.py'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        if 'B1' in sections:
            for depth in DEPTHS:
                receipts[f'B1 rgb {depth}'] = check_rgb(call, depth)
        if 'B2' in sections:
            receipts['B2 modes'] = check_modes(call)
        if 'B3' in sections:
            receipts['B3 refusals'] = check_refusals(call)
        if 'B4' in sections:
            receipts['B4 replay'] = check_replay(call)
        if 'B5' in sections:
            receipts['B5 fade'] = check_fade(call)
        if 'B6' in sections:
            receipts['B6 default size'] = check_default_size(call)
    assert len(receipts) > 1, ('no section ran', args.sections)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    print(json.dumps(receipts, indent=2))
    failed = [f'{section}: {case}' for section, value in receipts.items() if isinstance(value, dict)
              for case, result in value.items() if str(result).startswith('FAIL')]
    if failed:
        print('FAIL: ' + '; '.join(failed))
        sys.exit(1)
    print('PASS: the Background Eraser over MCP at 8, 16 and 32 bits, in Gray, Lab, CMYK and '
          'Indexed, with its refusals, undo, replay, Fade and its default size')


if __name__ == '__main__':
    main()
