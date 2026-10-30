#!/usr/bin/env python3
"""Drive: the Stroke's Fill Type Gradient — SHAPE BURST and DITHER — and
Fill Type PATTERN, over MCP.

One owned Debug instance on a scratch socket builds, at RGB8 and RGB16, a
96 x 64 document: opaque white with a "Shape" layer (canvas-sized, offset
(0, 0)) holding RECT (x 13..63, y 9..49) of #30a050. Expectations are fixed
here before any measurement.

THE STROKES: 8 px, Normal 100 %, Fill Type Gradient, stops black (0) to
white (1), Align with Layer on; Style Shape Burst or Linear at angle 0.

THE ORACLES (the shipped rule, `scripts/photoshop-probe/stroke_fill_take.py`
WINNERS and `core/src/style_stroke_fill.rs`), on straight runs: a probe k px
from the contour (k = 1 next to it, on either side) sits d = k − 0.5 px into
the band, and
  Shape Burst  Outside t = 1 − d / 8; Inside t = d / 8; Center (half 4) the
               outer side 0.5 − 0.5 d / 4, the inner side 0.5 + 0.5 d / 4;
  Linear       t = 0.5 + (x + 0.5 − 38) / (50 + 2 r) over RECT grown by the
               outward reach r (8 Outside, 4 Center, 0 Inside; BOX = BAND).
The reading is 255 t (black to white): RGB8 bytes, RGB16 v16 / 257, RGB32
center_rgba32 x 255 (black and white are 0 and 1 in linear light, so the
linear value IS t). Bound: 1 code.
PROBES: the left side at y 29 (x = 13 − k outside, 12 + k inside) and the
top side at x 38 (y = 9 − k outside, 8 + k inside), k = 1..7 (1..3 per side
at Center); a Linear stroke adds its k = 1 row along the top at x 20..55
step 5.

ITEMS, at RGB8 and RGB16:
  S1 Shape Burst at Outside, Inside and Center: each `set_layer_style` is
     one undo step and every probe is within 1 code of the oracle.
  S2 Linear (angle 0) at Outside, Inside and Center: the same.
  S3 a STACKED pair — Shape Burst Inside under Linear Outside (array order):
     each band's probes follow its own instance's oracle.
  S4 Dither on the Center Shape Burst: no probe moves by more than one code
     in the units read (a byte at RGB8, a raw center_rgba16 code at RGB16).
  S5 `save_copy rz` writes word 94 first inside 45 (8-bit) / right after 46
     (16-bit), and the reopened copy holds the same gradient and probes.
  S6 `export_psd` layers + `open_document` psd_mode layers reopen the same
     gradient (`shape_burst`, `dither` true) and probes within one code in
     the units read (Dither's threshold is anchored in LAYER coordinates, and
     a PSD layer is its content's rect).
  S7 `save_action` + `run_action` on a second build replay the style: the
     same gradient and probes.
At RGB32 (the RGB16 build styled, then `convert_precision rgba32` structure
preserve, which carries the style): S1 against the oracle as center_rgba32,
S4 (no probe moves by more than 1/65535 linear) and S5 (46, then 94).
At CMYK16 (`convert_mode cmyk` with the Generic CMYK profile of an RGB16
build styled with the Center Shape Burst):
  C1 every probe's `ink16` is the mix by t of the inks a Color Overlay of
     each stop (#000000, #ffffff) prints on the same layer, within 1 per
     plate;
  C2 Dither moves no plate of any probe by more than one 16-bit code;
  C3 `save_copy rz` writes 94 right after the CMYK rung and reopens the same
     ink16; `export_psd` layers + reopen give the same ink16 within 7 per
     plate (the export writes each stop's ink as a four-decimal `CMYC`
     percent, which the reopen keeps: 1e-4 ink is 6.6 codes; and S6's
     reason).

FILL TYPE PATTERN. THE TILE T: 12 x 8, opaque, texel (x, y) =
(20x + 10, 30y + 15, 200 if x + y is even else 60) — take1's tile — as a
custom pattern {"name": "T", ...}; an 8 px Pattern stroke of T at 100 %,
phase (0, 0), linked unless named. THE ORACLE (take1's PLACE = OVERLAY: the
Pattern Overlay's origin): probe (X, Y) reads texel ((X - Ox) mod 12,
(Y - Oy) mod 8), O = phase + (the layer's offset now - anchor) when linked,
phase when not; the Shape layer is made at (0, 0), so the setter echoes
"anchor": [0, 0]. Probes: the Shape Burst probes above (k = 1..7, 1..3 per
side at Center), moved with the layer. Bound: 1 code.
ITEMS, at RGB8 and RGB16:
  T1 a Pattern stroke at Outside, Inside and Center: each `set_layer_style`
     is one undo step, the echo's `pattern_fill` holds "anchor": [0, 0], and
     every probe is within 1 code of the oracle at O = (0, 0).
  T2 unlinked with phase (5, 2), then `move_layers` +7,+3: the probes (moved
     by 7, 3) read O = (5, 2); then linked on the moved layer: O = (7, 3)
     (anchored where the layer was made).
  T3 `save_copy rz` of T1's Center stroke writes word 95 first inside 45
     (8-bit) / right after 46 (16-bit), and reopens the same `pattern_fill`
     and probes.
  T4 `export_psd` layers + `open_document` psd_mode layers of the moved,
     linked stroke reopen the same `pattern_fill` and probes.
  T5 `save_action` (the style with no anchor, what a recorded step holds) +
     `run_action` on a second build whose layer was MOVED to offset (10, 5)
     re-anchors where it was made: the echo's anchor is [0, 0] and the
     probes (moved by 10, 5) read O = (10, 5).
  T6 `copy_layers_to_document` (in place) of the moved, linked stroke into a
     third document carries the same `pattern_fill` and probes.
At RGB32 (the RGB16 build styled at Outside, then `convert_precision rgba32`
structure preserve): T1 (center_rgba32 x 255: the codes / 255 are linear,
VERBATIM) and T3 (46, then 95).
At CMYK16 (`convert_mode cmyk` with the Generic CMYK profile of an RGB16
build styled with a Center Pattern stroke):
  PC1 every probe's `ink16` equals that of a Color Overlay of its texel's hex
      on the same layer (read inside RECT), within 1 per plate;
  PC2 `save_copy rz` writes 95 right after the CMYK rung and reopens the same
      ink16; `export_psd` layers + reopen give the same ink16 within 7 per
      plate (C3's reason).
Every item prints PASS or raises.
"""
import argparse
import base64
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 96, 64
RECT = dict(x=13, y=9, width=50, height=40)
SOURCE_HEX = '#30a050'
SIZE = 8
BOUND = 1.0
POSITIONS = ('outside', 'inside', 'center')


def stroke(position, **gradient):
    fill = dict(stops=[dict(position=0, color='#000000', opacity=1),
                       dict(position=1, color='#ffffff', opacity=1)], **gradient)
    return dict(type='stroke', blend='normal', opacity=1, size=SIZE, position=position,
                fill_type='gradient', gradient=fill)


def style(*effects):
    return {'effects': list(effects)}


def burst(position, **extra):
    return stroke(position, style='shape_burst', **extra)


def linear(position):
    return stroke(position, style='linear', angle=0)


# ------------------------------------------------------------ the oracle --

def reaches(position, size=SIZE):
    """(outward, inward) reach in px."""
    return {'outside': (size, 0), 'inside': (0, size)}.get(position, (size // 2, size // 2))


def probes(position, kind, size=SIZE):
    """[(x, y, side, k)] (module doc, PROBES)."""
    outward, inward = reaches(position, size)
    x0, y0 = RECT['x'], RECT['y']
    out = []
    for k in range(1, outward):
        out += [(x0 - k, 29, 'out', k), (38, y0 - k, 'out', k)]
    for k in range(1, inward):
        out += [(x0 - 1 + k, 29, 'in', k), (38, y0 - 1 + k, 'in', k)]
    if kind == 'linear':
        row = y0 - 1 if outward else y0
        out += [(x, row, 'out' if outward else 'in', 1) for x in range(20, 56, 5)]
    return out


def burst_t(position, side, k, size=SIZE):
    d = k - 0.5
    if position == 'outside':
        t = 1 - d / size
    elif position == 'inside':
        t = d / size
    elif side == 'out':
        t = 0.5 - 0.5 * d / (size / 2)
    else:
        t = 0.5 + 0.5 * d / (size / 2)
    return min(1.0, max(0.0, t))


def linear_t(position, x, size=SIZE):
    r = reaches(position, size)[0]
    t = 0.5 + (x + 0.5 - (RECT['x'] + RECT['width'] / 2)) / (RECT['width'] + 2 * r)
    return min(1.0, max(0.0, t))


def want_t(kind, position, x, side, k, size=SIZE):
    return burst_t(position, side, k, size) if kind == 'burst' else linear_t(position, x, size)


# ------------------------------------------------------------ the reads --

def sample(call, doc, x, y, depth):
    reply = call('sample_color', document_id=doc, x=x, y=y)
    if depth == 32:
        return [v * 255 for v in reply['center_rgba32'][:3]]
    if depth == 16:
        return [v / 257 for v in reply['center_rgba16'][:3]]
    hexed = reply['hex'].lstrip('#')
    return [int(hexed[i:i + 2], 16) for i in (0, 2, 4)]


def raw(call, doc, x, y, depth):
    """The read in its own units: a byte, a u16 code, a linear float, ink16."""
    reply = call('sample_color', document_id=doc, x=x, y=y)
    if depth == 'cmyk16':
        return list(reply['ink16'][:4])
    if depth == 32:
        return list(reply['center_rgba32'][:3])
    if depth == 16:
        return list(reply['center_rgba16'][:3])
    hexed = reply['hex'].lstrip('#')
    return [int(hexed[i:i + 2], 16) for i in (0, 2, 4)]


def check(call, doc, depth, kind, position, what, size=SIZE):
    worst = 0.0
    for x, y, side, k in probes(position, kind, size):
        got = sample(call, doc, x, y, depth)
        want = 255 * want_t(kind, position, x, side, k, size)
        e = max(abs(g - want) for g in got)
        worst = max(worst, e)
        assert e <= BOUND, (what, depth, position, (x, y, side, k), got, round(want, 2))
    return round(worst, 3)


def reads(call, doc, depth, points):
    return [raw(call, doc, x, y, depth) for x, y, _, _ in points]


def same_reads(got, want, what, bound=1e-6):
    assert len(got) == len(want), what
    for i, (a, b) in enumerate(zip(got, want)):
        assert all(abs(p - q) <= bound for p, q in zip(a, b)), (what, i, a, b)


def index_of(call, doc, name):
    return next(l['index'] for l in call('get_document', document_id=doc)['layers']
                if l['name'] == name)


def strokes_of(call, doc, name='Shape'):
    layer = next(l for l in call('get_document', document_id=doc)['layers']
                 if l['name'] == name)
    return [e for e in (layer.get('style') or {}).get('effects', []) if e['type'] == 'stroke']


def undo_count(call, doc):
    return int(call('list_history', document_id=doc)['undo_count'])


def build(call, depth):
    doc = call('new_document', width=W, height=H, unit='px', depth=depth,
               background='white')['document']['id']
    call('new_layer', document_id=doc, name='Shape')
    call('select_rect', document_id=doc, **RECT)
    call('fill_selection', document_id=doc, layer=index_of(call, doc, 'Shape'), color=SOURCE_HEX)
    call('deselect', document_id=doc)
    return doc


def word_after(path, slot):
    data = Path(path).read_bytes()
    return struct.unpack('<I', data[4 + 4 * slot:8 + 4 * slot])[0]


def set_style(call, doc, sent, what):
    before = undo_count(call, doc)
    call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Shape'), style=sent)
    assert undo_count(call, doc) == before + 1, (what, 'one undo step')


# --------------------------------------------------------- pattern fill --

def texel(x, y):
    return [20 * x + 10, 30 * y + 15, 200 if (x + y) % 2 == 0 else 60]


def tile_t():
    rgba = bytearray()
    for y in range(8):
        for x in range(12):
            rgba += bytes(texel(x, y) + [255])
    return dict(name='T', width=12, height=8, rgba=base64.b64encode(bytes(rgba)).decode())


def pattern_stroke(position, **fill):
    return dict(type='stroke', blend='normal', opacity=1, size=SIZE, position=position,
                fill_type='pattern', pattern_fill=dict(pattern=tile_t(), **fill))


def pattern_probes(position, shift=(0, 0)):
    return [(x + shift[0], y + shift[1], side, k)
            for x, y, side, k in probes(position, 'burst')]


def want_texel(origin, x, y):
    return texel((x - origin[0]) % 12, (y - origin[1]) % 8)


def check_pattern(call, doc, depth, position, origin, what, shift=(0, 0)):
    worst = 0.0
    for x, y, _, _ in pattern_probes(position, shift):
        got, want = sample(call, doc, x, y, depth), want_texel(origin, x, y)
        e = max(abs(g - w) for g, w in zip(got, want))
        worst = max(worst, e)
        assert e <= BOUND, (what, depth, position, (x, y), got, want)
    return round(worst, 3)


def pattern_fill_of(call, doc, name='Shape'):
    stroke_fx = strokes_of(call, doc, name)[0]
    assert stroke_fx['fill_type'] == 'pattern', stroke_fx
    return stroke_fx['pattern_fill']


# ---------------------------------------------------------------- items --

def drive_depth(call, folder, depth, receipts):
    tag = f'rgb{depth}'
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, depth)
    for kind, item in (('burst', 'S1'), ('linear', 'S2')):
        worst = 0.0
        for position in POSITIONS:
            sent = style(burst(position) if kind == 'burst' else linear(position))
            set_style(call, doc, sent, (tag, kind, position))
            worst = max(worst, check(call, doc, depth, kind, position, (tag, item)))
        out[item] = worst
        print(f'PASS {tag} {item}: {kind} at Outside, Inside and Center, one undo step each, '
              f'within {BOUND} code (max {worst})', flush=True)

    set_style(call, doc, style(burst('inside'), linear('outside')), (tag, 'stacked'))
    assert [e['gradient']['style'] for e in strokes_of(call, doc)] == ['shape_burst', 'linear']
    out['S3'] = max(check(call, doc, depth, 'burst', 'inside', (tag, 'S3 inside')),
                    check(call, doc, depth, 'linear', 'outside', (tag, 'S3 outside')))
    print(f'PASS {tag} S3: the stacked pair draws each instance\'s own fill (max '
          f'{out["S3"]})', flush=True)

    points = probes('center', 'burst')
    set_style(call, doc, style(burst('center')), (tag, 'plain'))
    plain = reads(call, doc, depth, points)
    set_style(call, doc, style(burst('center', dither=True)), (tag, 'dither'))
    dithered = reads(call, doc, depth, points)
    moved = max(max(abs(p - q) for p, q in zip(a, b)) for a, b in zip(plain, dithered))
    assert moved <= 1, (tag, 'S4', moved)
    out['S4'] = moved
    print(f'PASS {tag} S4: Dither moves no probe more than one code (max {moved})', flush=True)

    effect = strokes_of(call, doc)[0]
    assert effect['gradient'].get('dither') is True and \
        effect['gradient']['style'] == 'shape_burst', effect
    rz = work / 'burst.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(2)]
    assert words == [45 if depth == 8 else 46, 94], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert strokes_of(call, again)[0]['gradient'] == effect['gradient'], tag
    same_reads(reads(call, again, depth, points), dithered, f'{tag} S5')
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} S5: save_copy rz writes {words} and reopens the same', flush=True)

    psd = work / f'{tag}.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    assert strokes_of(call, back)[0]['gradient'] == effect['gradient'], \
        (tag, strokes_of(call, back), effect)
    # Dither's threshold is anchored in LAYER coordinates and a PSD layer is
    # its content's rect, so the reopen may dither on a shifted grid: within
    # Dither's own bound, one code in the units read.
    same_reads(reads(call, back, depth, points), dithered, f'{tag} S6', bound=1)
    call('close_document', document_id=back, discard_changes=True)
    print(f'PASS {tag} S6: export_psd layers reopens shape_burst, dither and the probes',
          flush=True)

    name = f'rz-557-stroke-fill-{tag}'
    call('save_action', name=name, steps=[dict(
        tool='set_layer_style',
        arguments={'layer': {'$layer': 'active'}, 'style': style(burst('center', dither=True))},
        note='Layer ▸ Layer Style')])
    second = build(call, depth)
    call('set_active_layer', document_id=second, index=index_of(call, second, 'Shape'))
    call('run_action', document_id=second, name=name)
    assert strokes_of(call, second)[0]['gradient'] == effect['gradient'], tag
    same_reads(reads(call, second, depth, points), dithered, f'{tag} S7')
    call('delete_action', name=name)
    call('close_document', document_id=second, discard_changes=True)
    print(f'PASS {tag} S7: save_action + run_action replay the style', flush=True)
    call('close_document', document_id=doc, discard_changes=True)


def drive_float(call, folder, receipts):
    """RGB32 (module doc): S1, S4, S5 on the converted RGB16 build."""
    tag = 'rgb32'
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    worst = 0.0
    for position in POSITIONS:
        doc = build(call, 16)
        call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Shape'),
             style=style(burst(position)))
        call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
        assert strokes_of(call, doc)[0]['gradient']['style'] == 'shape_burst', tag
        worst = max(worst, check(call, doc, 32, 'burst', position, (tag, 'S1')))
        call('close_document', document_id=doc, discard_changes=True)
    out['S1'] = worst
    print(f'PASS {tag} S1: convert_precision carries Shape Burst; every probe is t in linear '
          f'light within {BOUND} code (max {worst})', flush=True)

    doc = build(call, 16)
    call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    points = probes('center', 'burst')
    set_style(call, doc, style(burst('center')), (tag, 'plain'))
    plain = reads(call, doc, 32, points)
    set_style(call, doc, style(burst('center', dither=True)), (tag, 'dither'))
    dithered = reads(call, doc, 32, points)
    moved = max(max(abs(p - q) for p, q in zip(a, b)) for a, b in zip(plain, dithered))
    assert moved <= 1 / 65535 + 1e-6, (tag, 'S4', moved)
    out['S4'] = moved * 65535
    print(f'PASS {tag} S4: Dither moves no probe more than 1/65535 linear (max '
          f'{round(moved * 65535, 3)}/65535)', flush=True)
    rz = work / 'burst.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(2)]
    assert words == [46, 94], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    same_reads(reads(call, again, 32, points), dithered, f'{tag} S5')
    call('close_document', document_id=again, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} S5: save_copy rz writes {words} and reopens the same', flush=True)


def drive_cmyk(call, folder, receipts):
    """CMYK16 (module doc): C1, C2, C3."""
    tag = 'cmyk16'
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, 16)
    layer = index_of(call, doc, 'Shape')
    call('set_layer_style', document_id=doc, layer=layer, style=style(burst('center')))
    call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    points = probes('center', 'burst')
    banded = reads(call, doc, 'cmyk16', points)

    inks = {}
    for hexed in ('#000000', '#ffffff'):
        call('set_layer_style', document_id=doc, layer=layer,
             style={'effects': [dict(type='color_overlay', color=hexed)]})
        inks[hexed] = raw(call, doc, 38, 29, 'cmyk16')
    worst = 0.0
    for (x, y, side, k), got in zip(points, banded):
        t = burst_t('center', side, k)
        want = [a + (b - a) * t for a, b in zip(inks['#000000'], inks['#ffffff'])]
        e = max(abs(p - q) for p, q in zip(got, want))
        worst = max(worst, e)
        assert e <= 1, (tag, 'C1', (x, y), got, [round(v, 2) for v in want])
    out['C1'] = round(worst, 3)
    print(f'PASS {tag} C1: every probe mixes the stops\' Color Overlay inks by t (max '
          f'{out["C1"]})', flush=True)

    call('set_layer_style', document_id=doc, layer=layer, style=style(burst('center')))
    plain = reads(call, doc, 'cmyk16', points)
    call('set_layer_style', document_id=doc, layer=layer,
         style=style(burst('center', dither=True)))
    dithered = reads(call, doc, 'cmyk16', points)
    moved = max(max(abs(p - q) for p, q in zip(a, b)) for a, b in zip(plain, dithered))
    assert moved <= 1, (tag, 'C2', moved)
    out['C2'] = moved
    print(f'PASS {tag} C2: Dither moves no plate more than one 16-bit code (max {moved})',
          flush=True)

    effect = strokes_of(call, doc)[0]
    rz = work / 'cmyk.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(3)]
    assert words[0] == 46 and 82 <= words[1] <= 88 and words[2] == 94, (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert strokes_of(call, again)[0]['gradient'] == effect['gradient'], tag
    same_reads(reads(call, again, 'cmyk16', points), dithered, f'{tag} C3 rz')
    call('close_document', document_id=again, discard_changes=True)
    psd = work / 'cmyk.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    gradient = strokes_of(call, back)[0]['gradient']
    assert gradient['style'] == 'shape_burst' and gradient.get('dither') is True, gradient
    # The export writes each stop's ink as a four-decimal `CMYC` percent,
    # which the import keeps as the stop's ink: 1e-4 ink is 6.6 codes.
    same_reads(reads(call, back, 'cmyk16', points), dithered, f'{tag} C3 psd', bound=7)
    call('close_document', document_id=back, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} C3: save_copy rz ({words}) and export_psd layers reopen the same ink16',
          flush=True)


def drive_pattern(call, folder, depth, receipts):
    """Fill Type Pattern at RGB8 / RGB16 (module doc): T1..T6."""
    tag = f'rgb{depth}'
    work = Path(folder) / f'{tag}-pattern'
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, depth)
    worst = 0.0
    for position in POSITIONS:
        set_style(call, doc, style(pattern_stroke(position)), (tag, 'T1', position))
        fill = pattern_fill_of(call, doc)
        assert fill.get('anchor') == [0, 0], (tag, 'T1', fill.get('anchor'))
        worst = max(worst, check_pattern(call, doc, depth, position, (0, 0), (tag, 'T1')))
    out['T1'] = worst
    print(f'PASS {tag} T1: a Pattern stroke at Outside, Inside and Center, one undo step each, '
          f'anchor [0, 0], the texel within {BOUND} code (max {worst})', flush=True)

    styled = pattern_fill_of(call, doc)
    rz = work / 'pattern.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(2)]
    assert words == [45 if depth == 8 else 46, 95], (tag, 'T3', words)
    again = call('open_document', path=str(rz))['document']['id']
    assert pattern_fill_of(call, again) == styled, (tag, 'T3')
    out['T3'] = check_pattern(call, again, depth, 'center', (0, 0), (tag, 'T3'))
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} T3: save_copy rz writes {words} and reopens the same', flush=True)

    layer = index_of(call, doc, 'Shape')
    set_style(call, doc, style(pattern_stroke('outside', link_with_layer=False, phase=[5, 2])),
              (tag, 'T2 unlinked'))
    call('move_layers', document_id=doc, layers=[layer], dx=7, dy=3)
    unlinked = check_pattern(call, doc, depth, 'outside', (5, 2), (tag, 'T2 unlinked'),
                             shift=(7, 3))
    set_style(call, doc, style(pattern_stroke('outside')), (tag, 'T2 linked'))
    assert pattern_fill_of(call, doc).get('anchor') == [0, 0], (tag, 'T2')
    linked = check_pattern(call, doc, depth, 'outside', (7, 3), (tag, 'T2 linked'),
                           shift=(7, 3))
    out['T2'] = max(unlinked, linked)
    print(f'PASS {tag} T2: unlinked stays at (5, 2) and linked follows to (7, 3) after '
          f'move_layers (max {out["T2"]})', flush=True)

    moved = pattern_fill_of(call, doc)
    psd = work / f'{tag}.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    assert pattern_fill_of(call, back) == moved, (tag, 'T4', pattern_fill_of(call, back), moved)
    out['T4'] = check_pattern(call, back, depth, 'outside', (7, 3), (tag, 'T4'), shift=(7, 3))
    call('close_document', document_id=back, discard_changes=True)
    print(f'PASS {tag} T4: export_psd layers reopens the same pattern_fill and probes (max '
          f'{out["T4"]})', flush=True)

    name = f'rz-557-stroke-pattern-{tag}'
    call('save_action', name=name, steps=[dict(
        tool='set_layer_style',
        arguments={'layer': {'$layer': 'active'}, 'style': style(pattern_stroke('center'))},
        note='Layer ▸ Layer Style')])
    second = build(call, depth)
    target = index_of(call, second, 'Shape')
    call('set_layer_properties', document_id=second, index=target, offset_x=10, offset_y=5)
    call('set_active_layer', document_id=second, index=target)
    call('run_action', document_id=second, name=name)
    assert pattern_fill_of(call, second).get('anchor') == [0, 0], (tag, 'T5')
    out['T5'] = check_pattern(call, second, depth, 'center', (10, 5), (tag, 'T5'),
                              shift=(10, 5))
    call('delete_action', name=name)
    call('close_document', document_id=second, discard_changes=True)
    print(f'PASS {tag} T5: run_action re-anchors where the second layer was made (max '
          f'{out["T5"]})', flush=True)

    third = call('new_document', width=W, height=H, unit='px', depth=depth,
                 background='white')['document']['id']
    call('copy_layers_to_document', document_id=doc, to_document_id=third,
         layers=[index_of(call, doc, 'Shape')], placement='in_place')
    assert pattern_fill_of(call, third) == moved, (tag, 'T6')
    out['T6'] = check_pattern(call, third, depth, 'outside', (7, 3), (tag, 'T6'), shift=(7, 3))
    call('close_document', document_id=third, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} T6: copy_layers_to_document carries the pattern stroke (max '
          f'{out["T6"]})', flush=True)


def drive_pattern_float(call, folder, receipts):
    """RGB32 (module doc): T1 and T3 of a converted Outside Pattern stroke."""
    tag = 'rgb32'
    work = Path(folder) / f'{tag}-pattern'
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, 16)
    call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Shape'),
         style=style(pattern_stroke('outside')))
    call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    styled = pattern_fill_of(call, doc)
    out['T1'] = check_pattern(call, doc, 32, 'outside', (0, 0), (tag, 'T1'))
    print(f'PASS {tag} T1: convert_precision carries the Pattern stroke; every probe is its '
          f'code / 255 in linear light within {BOUND} code (max {out["T1"]})', flush=True)
    rz = work / 'pattern.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(2)]
    assert words == [46, 95], (tag, 'T3', words)
    again = call('open_document', path=str(rz))['document']['id']
    assert pattern_fill_of(call, again) == styled, (tag, 'T3')
    out['T3'] = check_pattern(call, again, 32, 'outside', (0, 0), (tag, 'T3'))
    call('close_document', document_id=again, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} T3: save_copy rz writes {words} and reopens the same', flush=True)


def drive_pattern_cmyk(call, folder, receipts):
    """CMYK16 (module doc): PC1, PC2."""
    tag = 'cmyk16'
    work = Path(folder) / f'{tag}-pattern'
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, 16)
    layer = index_of(call, doc, 'Shape')
    sent = style(pattern_stroke('center'))
    call('set_layer_style', document_id=doc, layer=layer, style=sent)
    call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    points = pattern_probes('center')
    banded = reads(call, doc, 'cmyk16', points)
    inks = {}
    for x, y, _, _ in points:
        key = tuple(want_texel((0, 0), x, y))
        if key not in inks:
            hexed = '#%02x%02x%02x' % key
            call('set_layer_style', document_id=doc, layer=layer,
                 style={'effects': [dict(type='color_overlay', color=hexed)]})
            inks[key] = raw(call, doc, 38, 29, 'cmyk16')
    worst = 0
    for (x, y, _, _), got in zip(points, banded):
        want = inks[tuple(want_texel((0, 0), x, y))]
        e = max(abs(p - q) for p, q in zip(got, want))
        worst = max(worst, e)
        assert e <= 1, (tag, 'PC1', (x, y), got, want)
    out['PC1'] = worst
    print(f'PASS {tag} PC1: every probe prints its texel\'s Color Overlay ink (max {worst}, '
          f'{len(inks)} texels)', flush=True)

    call('set_layer_style', document_id=doc, layer=layer, style=sent)
    same_reads(reads(call, doc, 'cmyk16', points), banded, f'{tag} PC2 restyled')
    styled = pattern_fill_of(call, doc)
    rz = work / 'cmyk.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(3)]
    assert words[0] == 46 and 82 <= words[1] <= 88 and words[2] == 95, (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert pattern_fill_of(call, again) == styled, tag
    same_reads(reads(call, again, 'cmyk16', points), banded, f'{tag} PC2 rz')
    call('close_document', document_id=again, discard_changes=True)
    psd = work / 'cmyk.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    fill = pattern_fill_of(call, back)
    assert fill['pattern'] == styled['pattern'], (tag, 'PC2 psd')
    same_reads(reads(call, back, 'cmyk16', points), banded, f'{tag} PC2 psd', bound=7)
    call('close_document', document_id=back, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} PC2: save_copy rz ({words}) and export_psd layers reopen the same ink16',
          flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None, help='a folder for receipts.json')
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    from phase5_support import App
    output = Path(args.receipts or tempfile.mkdtemp(prefix='stroke-fill-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_stroke_fill.py before any measurement'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        for depth in (8, 16):
            drive_depth(app.call, folder, depth, receipts)
        drive_float(app.call, folder, receipts)
        drive_cmyk(app.call, folder, receipts)
        for depth in (8, 16):
            drive_pattern(app.call, folder, depth, receipts)
        drive_pattern_float(app.call, folder, receipts)
        drive_pattern_cmyk(app.call, folder, receipts)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print('PASS: every item', flush=True)


if __name__ == '__main__':
    main()
