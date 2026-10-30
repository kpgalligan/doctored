#!/usr/bin/env python3
"""Drive: PATTERN OVERLAY over MCP (part 2: slice 1, then 32-bit, CMYK
and the AT-CREATION anchor; then Angle, phase 2).

One owned Debug instance on a scratch socket builds, at RGB8 and RGB16, a
96 x 64 document: opaque white with a "Pattern" layer (canvas-sized, offset
(0, 0)) holding RECT (x 13..63, y 9..49) of #30a050. Expectations are fixed
here before any measurement.

THE TILE T: 12 x 8, opaque, texel (x, y) = (20x + 10, 30y + 15, 200 if x + y
is even else 60) — take1's tile — as a custom pattern {"name": "T", ...}.
THE STYLE: one pattern_overlay, Normal 100 %, T, scale 0.37, phase (5, 2),
linked.

THE ORACLE (the drive's own PYRAMID, `pattern_tile`'s module doc): L =
max(0, floor(log2(1 / s))) periodic 2 x 2 halvings of T (an odd side keeps
its texels over two periods; here one even halving: 6 x 4); s' = s * 12 / 6 =
0.74; canvas pixel (X, Y) reads level index
((X - Ox) / s', (Y - Oy) / s'), texel j at index j, bilinear, wrapping. The
tile's canvas origin O is phase + (the layer's offset now - anchor) when
linked; the setter anchors where the layer was MADE (its offset less its
moves since, Photoshop's fxrp). T is opaque and Normal 100 %, so inside RECT
the reading IS the sample. Probes: every 6th pixel of RECT on both axes (63).
Bound: 1 code (16-bit as v16 / 257, 32-bit as center_rgba32 x 255: the tile's
codes / 255 are the float document's linear values, VERBATIM).

ITEMS, at RGB8 and RGB16:
  P1 `set_layer_style` with stripes, then with T, is one undo step each, and
     `get_document` echoes "anchor": [0, 0] (where the layer was made).
  P2 every probe equals the oracle at O = (5, 2).
  P3 `move_layers` dx 7 dy 3 moves the tile: every probe (moved by 7, 3)
     equals the oracle at O = (12, 5).
  P4 `save_copy rz` writes word 91 (the moved layer's reference) right after
     45 (8 bits) / 46 (16 bits), then word 90, and the reopened copy holds
     the same style and probes.
  P5 `save_action` (the style with its anchor stripped, what a recorded step
     holds) + `run_action` on a second document whose layer was MOVED to
     offset (10, 5) re-anchors where it was made: the echo's anchor is
     [0, 0] and the probes (moved by 10, 5) equal the oracle at O = (15, 7).
  P6 `copy_layers_to_document` (in place) into a third document carries the
     style: the same style echoed and the same probes.
  P7 `export_psd` layers + `open_document` psd_mode layers reopen the same
     style and probes.
  P8 AT-CREATION: a fresh build's layer `move_layers` +7,+3 BEFORE
     `set_layer_style`: the echo's anchor is [0, 0] and the probes (moved by
     7, 3) equal the oracle at O = (12, 5); `save_copy rz` holds word 91
     first inside 45 / right after 46 and reopens the same probes;
     `export_psd` layers + reopen give the same probes.
At RGB32 (the RGB16 build, styled, then `convert_precision rgba32` structure
preserve, which carries the style): P2, P3, P4 (46, 91, then 90) and P8 (the
RGB16 build moved, converted, then styled), reading center_rgba32 x 255
against the same oracle within 1 code.
At CMYK16 (`convert_mode cmyk` with the Generic CMYK profile of an RGB16
build styled with T at scale 1, phase (0, 0)):
  C1 at 12 probes on texel centres ((24 + i, 16 + j) reads texel (i, j),
     i < 4, j < 3) the `ink16` equals that of a Color Overlay of the texel's
     hex on the same layer, within 1 per plate;
  C2 `save_copy rz` (word 90 right after the CMYK rung) reopens with the
     same ink16 at the 12 probes;
  C3 `export_psd` layers + reopen give the same ink16 at the 12 probes.
ANGLE (phase 2), at RGB8, RGB16, RGB32 (the RGB16 build converted with
`convert_precision rgba32` structure preserve) and CMYK16 (the RGB16 build
converted as above), with T at angle 30, scale 0.5, phase (0, 0), linked. The
rotation's oracle is the core tests'; these items check parity and round
trips against the app's own render (the probes above, or the 12 CMYK probes
as ink16), exact:
  A1 `set_layer_style` with angle 30 echoes "angle": 30.0, and its probes
     differ from the same style at angle 0 (some probe by more than 2 codes).
  A2 `save_copy rz` writes word 93 first inside 45 (right after 46, or after
     the CMYK rung), then 90, and the reopened copy gives the same probes.
  A3 `save_action` + `run_action` on a second build carry the angle: the
     echo's angle is 30.0 and the probes are the first build's (at CMYK16 the
     action runs on the RGB16 build before its conversion: Actions do not run
     in a CMYK document).
  A4 `export_psd` layers + `open_document` psd_mode layers give the same
     angle and probes (not at RGB32: no Photoshop document is written for a
     32-bit float document).
Every item prints PASS or raises.

`build_exports(call, folder, run_id)` is take2's Doctored side
(`photoshop-probe/pattern_overlay_take.py --build`): a 160 x 100 document at
16 bits (psd16, psb16) and at 8 bits (psd8), each with five 24 x 20 layers —
Linked, Moved (+7, +3 after the style), Unlinked, S37 (scale 0.37) and Phase
(phase (5, 2)) — styled with a custom 12 x 8 tile named `<run_id>-psd`, plus
each document's flattened TIFF as the reference.
"""
import argparse
import json
import math
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
TW, TH = 12, 8
SCALE, PHASE = 0.37, (5, 2)
MOVE = (7, 3)
BOUND = 1.0
B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'


def texel(x, y):
    return (20 * x + 10, 30 * y + 15, 200 if (x + y) % 2 == 0 else 60)


def base64(data):
    out = []
    for i in range(0, len(data), 3):
        chunk = data[i:i + 3]
        n = int.from_bytes(chunk + bytes(3 - len(chunk)), 'big')
        for k in range(4):
            out.append(B64[(n >> (18 - 6 * k)) & 63] if k <= len(chunk) else '=')
    return ''.join(out)


def tile_t(name='T'):
    rgba = bytes(v for y in range(TH) for x in range(TW) for v in (*texel(x, y), 255))
    return dict(name=name, width=TW, height=TH, rgba=base64(rgba))


def overlay(pattern, **extra):
    return {'effects': [dict(type='pattern_overlay', blend='normal', opacity=1,
                             pattern=pattern, **extra)]}


def the_style():
    return overlay(tile_t(), scale=SCALE, phase=list(PHASE), link_with_layer=True)


# ---------------------------------------------------------- the oracle --

def level(scale):
    """(w, h, {(x, y): rgb}, (s'x, s'y)) of T at `scale` (module doc)."""
    w, h = TW, TH
    pw, ph = TW, TH
    px = {(x, y): [float(v) for v in texel(x, y)] for x in range(w) for y in range(h)}
    for _ in range(max(0, math.floor(math.log2(1 / scale)))):
        # Texel j is the mean of 2j and 2j + 1 (mod the side); an odd side
        # keeps its texels and spans two periods (ODD-DOUBLE).
        nw, nh = (w // 2 if w % 2 == 0 else w), (h // 2 if h % 2 == 0 else h)
        pw, ph = pw * (2 if w % 2 else 1), ph * (2 if h % 2 else 1)
        px = {(i, j): [sum(px[((2 * i + a) % w, (2 * j + b) % h)][c]
                           for a in (0, 1) for b in (0, 1)) / 4 for c in range(3)]
              for i in range(nw) for j in range(nh)}
        w, h = nw, nh
    return w, h, px, (scale * pw / w, scale * ph / h)


def oracle(lv, origin, x, y):
    w, h, px, (sx, sy) = lv
    u, v = (x - origin[0]) / sx, (y - origin[1]) / sy
    i0, j0 = math.floor(u), math.floor(v)
    fu, fv = u - i0, v - j0
    out = [0.0, 0.0, 0.0]
    for di, dj, k in ((0, 0, (1 - fu) * (1 - fv)), (1, 0, fu * (1 - fv)),
                      (0, 1, (1 - fu) * fv), (1, 1, fu * fv)):
        c = px[((i0 + di) % w, (j0 + dj) % h)]
        out = [o + k * q for o, q in zip(out, c)]
    return out


def probes(shift=(0, 0)):
    return [(x + shift[0], y + shift[1])
            for y in range(RECT['y'], RECT['y'] + RECT['height'], 6)
            for x in range(RECT['x'], RECT['x'] + RECT['width'], 6)]


# ------------------------------------------------------------ the reads --

def sample(call, doc, x, y, depth):
    reply = call('sample_color', document_id=doc, x=x, y=y)
    if depth == 32:
        return [v * 255 for v in reply['center_rgba32'][:3]]
    if depth == 16:
        return [v / 257 for v in reply['center_rgba16'][:3]]
    hexed = reply['hex'].lstrip('#')
    return [int(hexed[i:i + 2], 16) for i in (0, 2, 4)]


def check(call, doc, depth, origin, what, shift=(0, 0)):
    lv = level(SCALE)
    worst = 0.0
    for x, y in probes(shift):
        got, want = sample(call, doc, x, y, depth), oracle(lv, origin, x, y)
        e = max(abs(a - b) for a, b in zip(got, want))
        worst = max(worst, e)
        assert e <= BOUND, (what, depth, (x, y), got, [round(v, 2) for v in want])
    return round(worst, 3)


def index_of(call, doc, name):
    return next(l['index'] for l in call('get_document', document_id=doc)['layers']
                if l['name'] == name)


def style_of(call, doc, name='Pattern'):
    return next(l for l in call('get_document', document_id=doc)['layers']
                if l['name'] == name).get('style')


def pattern_of(style):
    effects = [e for e in (style or {}).get('effects', []) if e['type'] == 'pattern_overlay']
    assert len(effects) == 1, style
    return effects[0]


def undo_count(call, doc):
    return int(call('list_history', document_id=doc)['undo_count'])


def build(call, depth, rect=RECT, name='Pattern', size=(W, H)):
    doc = call('new_document', width=size[0], height=size[1], unit='px', depth=depth,
               background='white')['document']['id']
    call('new_layer', document_id=doc, name=name)
    call('select_rect', document_id=doc, **rect)
    call('fill_selection', document_id=doc, layer=index_of(call, doc, name), color=SOURCE_HEX)
    call('deselect', document_id=doc)
    return doc


def word_after(path, slot):
    data = Path(path).read_bytes()
    return struct.unpack('<I', data[4 + 4 * slot:8 + 4 * slot])[0]


def same_pattern(a, b):
    keys = ('pattern', 'scale', 'phase', 'link_with_layer', 'anchor', 'blend', 'opacity')
    return {k: a.get(k) for k in keys} == {k: b.get(k) for k in keys}


# ---------------------------------------------------------------- items --

def drive_depth(call, folder, depth, receipts):
    tag = f'rgb{depth}'
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, depth)
    layer = index_of(call, doc, 'Pattern')
    for label, sent in (('stripes', overlay({'preset': 'stripes'})), ('T', the_style())):
        before = undo_count(call, doc)
        call('set_layer_style', document_id=doc, layer=layer, style=sent)
        assert undo_count(call, doc) == before + 1, (tag, label)
        echoed = pattern_of(style_of(call, doc))
        assert echoed.get('anchor') == [0, 0], (tag, label, echoed.get('anchor'))
    print(f'PASS {tag} P1: set_layer_style (stripes, then T) is one undo step each, '
          'echoed with anchor [0, 0]', flush=True)

    out['P2'] = check(call, doc, depth, PHASE, 'render')
    print(f'PASS {tag} P2: {len(probes())} probes equal the PYRAMID oracle (max '
          f'{out["P2"]})', flush=True)

    call('move_layers', document_id=doc, layers=[layer], dx=MOVE[0], dy=MOVE[1])
    moved_origin = (PHASE[0] + MOVE[0], PHASE[1] + MOVE[1])
    out['P3'] = check(call, doc, depth, moved_origin, 'moved', shift=MOVE)
    print(f'PASS {tag} P3: move_layers +7,+3 moves the tile (max {out["P3"]})', flush=True)

    styled = pattern_of(style_of(call, doc))
    rz = work / 'pattern.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(3)]
    assert words == [45 if depth == 8 else 46, 91, 90], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert same_pattern(pattern_of(style_of(call, again)), styled), tag
    check(call, again, depth, moved_origin, 'reopened rz', shift=MOVE)
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} P4: save_copy rz writes words 91 then 90 and reopens the same',
          flush=True)

    recorded = the_style()
    name = f'rz-557-pattern-{tag}'
    call('save_action', name=name, steps=[dict(
        tool='set_layer_style', arguments={'layer': {'$layer': 'active'}, 'style': recorded},
        note='Layer ▸ Layer Style')])
    second = build(call, depth)
    target = index_of(call, second, 'Pattern')
    call('set_layer_properties', document_id=second, index=target, offset_x=10, offset_y=5)
    call('set_active_layer', document_id=second, index=target)
    call('run_action', document_id=second, name=name)
    echoed = pattern_of(style_of(call, second))
    assert echoed.get('anchor') == [0, 0], (tag, echoed.get('anchor'))
    out['P5'] = check(call, second, depth, (PHASE[0] + 10, PHASE[1] + 5), 'run_action',
                      shift=(10, 5))
    call('delete_action', name=name)
    call('close_document', document_id=second, discard_changes=True)
    print(f'PASS {tag} P5: run_action re-anchors where the second layer was made (max '
          f'{out["P5"]})', flush=True)

    third = call('new_document', width=W, height=H, unit='px', depth=depth,
                 background='white')['document']['id']
    call('copy_layers_to_document', document_id=doc, to_document_id=third,
         layers=[index_of(call, doc, 'Pattern')], placement='in_place')
    assert same_pattern(pattern_of(style_of(call, third)), styled), tag
    out['P6'] = check(call, third, depth, moved_origin, 'copied', shift=MOVE)
    call('close_document', document_id=third, discard_changes=True)
    print(f'PASS {tag} P6: copy_layers_to_document carries the style (max {out["P6"]})',
          flush=True)

    psd = work / f'{tag}.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    assert same_pattern(pattern_of(style_of(call, back)), styled), \
        (tag, pattern_of(style_of(call, back)), styled)
    out['P7'] = check(call, back, depth, moved_origin, 'reopened psd', shift=MOVE)
    call('close_document', document_id=back, discard_changes=True)
    print(f'PASS {tag} P7: export_psd layers reopens the same style and probes', flush=True)
    call('close_document', document_id=doc, discard_changes=True)
    at_creation(call, work, depth, tag, out)


def at_creation(call, work, depth, tag, out):
    """P8 (module doc): moved BEFORE the style; at 32 bits the RGB16 build is
    moved, converted, then styled."""
    doc = build(call, 16 if depth == 32 else depth)
    layer = index_of(call, doc, 'Pattern')
    call('move_layers', document_id=doc, layers=[layer], dx=MOVE[0], dy=MOVE[1])
    if depth == 32:
        call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Pattern'),
         style=the_style())
    echoed = pattern_of(style_of(call, doc))
    assert echoed.get('anchor') == [0, 0], (tag, echoed.get('anchor'))
    origin = (PHASE[0] + MOVE[0], PHASE[1] + MOVE[1])
    out['P8'] = check(call, doc, depth, origin, 'at creation', shift=MOVE)
    rz = work / 'created.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(3)]
    assert words == [45 if depth == 8 else 46, 91, 90], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert same_pattern(pattern_of(style_of(call, again)), echoed), tag
    check(call, again, depth, origin, 'at creation, reopened rz', shift=MOVE)
    call('close_document', document_id=again, discard_changes=True)
    if depth != 32:
        psd = work / 'created.psd'
        call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
        report = call('inspect_psd', path=str(psd))
        back = call('open_document', path=str(psd), psd_mode='layers',
                    psd_fingerprint=report['source_fingerprint'])['document']['id']
        check(call, back, depth, origin, 'at creation, reopened psd', shift=MOVE)
        call('close_document', document_id=back, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} P8: moved before the style, anchor [0, 0], probes at O = {origin} '
          f'(max {out["P8"]}), word 91 and the reopens hold', flush=True)


def drive_float(call, folder, receipts):
    """RGB32 (module doc): P2, P3, P4 and P8 on the converted RGB16 build."""
    tag, depth = 'rgb32', 32
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, 16)
    call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Pattern'),
         style=the_style())
    call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    styled = pattern_of(style_of(call, doc))
    assert styled.get('anchor') == [0, 0], (tag, styled.get('anchor'))
    out['P2'] = check(call, doc, depth, PHASE, 'render')
    print(f'PASS {tag} P2: convert_precision carries the style; {len(probes())} probes equal '
          f'the oracle as code / 255 (max {out["P2"]})', flush=True)
    call('move_layers', document_id=doc, layers=[index_of(call, doc, 'Pattern')],
         dx=MOVE[0], dy=MOVE[1])
    moved_origin = (PHASE[0] + MOVE[0], PHASE[1] + MOVE[1])
    out['P3'] = check(call, doc, depth, moved_origin, 'moved', shift=MOVE)
    print(f'PASS {tag} P3: move_layers +7,+3 moves the tile (max {out["P3"]})', flush=True)
    rz = work / 'pattern.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(3)]
    assert words == [46, 91, 90], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert same_pattern(pattern_of(style_of(call, again)), styled), tag
    check(call, again, depth, moved_origin, 'reopened rz', shift=MOVE)
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} P4: save_copy rz writes 46, 91, then 90 and reopens the same',
          flush=True)
    call('close_document', document_id=doc, discard_changes=True)
    at_creation(call, work, depth, tag, out)


# ------------------------------------------------------------------ CMYK --

def cmyk_probes():
    return [(24 + i, 16 + j) for j in range(3) for i in range(4)]


def ink16(call, doc):
    return [call('sample_color', document_id=doc, x=x, y=y)['ink16'][:4]
            for x, y in cmyk_probes()]


def same_ink(got, want, what):
    for (x, y), a, b in zip(cmyk_probes(), got, want):
        assert all(abs(p - q) <= 1 for p, q in zip(a, b)), (what, (x, y), a, b)


def drive_cmyk(call, folder, receipts):
    """CMYK16 (module doc): C1, C2, C3."""
    tag = 'cmyk16'
    work = Path(folder) / tag
    work.mkdir()
    out = receipts.setdefault(tag, {})
    doc = build(call, 16)
    style = overlay(tile_t(), scale=1, phase=[0, 0], link_with_layer=True)
    layer = index_of(call, doc, 'Pattern')
    call('set_layer_style', document_id=doc, layer=layer, style=style)
    call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    styled = pattern_of(style_of(call, doc))
    patterned = ink16(call, doc)

    rz = work / 'cmyk.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    rung = word_after(rz, 1)
    assert word_after(rz, 0) == 46 and 82 <= rung <= 88 and word_after(rz, 2) == 90, \
        (tag, [word_after(rz, i) for i in range(3)])
    again = call('open_document', path=str(rz))['document']['id']
    assert same_pattern(pattern_of(style_of(call, again)), styled), tag
    same_ink(ink16(call, again), patterned, 'C2 reopened rz')
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} C2: save_copy rz (word 90 after rung {rung}) reopens the same ink16',
          flush=True)

    psd = work / 'cmyk.psd'
    call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
    report = call('inspect_psd', path=str(psd))
    back = call('open_document', path=str(psd), psd_mode='layers',
                psd_fingerprint=report['source_fingerprint'])['document']['id']
    assert same_pattern(pattern_of(style_of(call, back)), styled), \
        (tag, pattern_of(style_of(call, back)), styled)
    same_ink(ink16(call, back), patterned, 'C3 reopened psd')
    call('close_document', document_id=back, discard_changes=True)
    print(f'PASS {tag} C3: export_psd layers reopens the same ink16', flush=True)

    worst = 0
    for (x, y), got in zip(cmyk_probes(), patterned):
        hexed = '#%02x%02x%02x' % texel(x % TW, y % TH)
        call('set_layer_style', document_id=doc, layer=layer,
             style={'effects': [dict(type='color_overlay', color=hexed)]})
        want = call('sample_color', document_id=doc, x=x, y=y)['ink16'][:4]
        e = max(abs(p - q) for p, q in zip(got, want))
        worst = max(worst, e)
        assert e <= 1, (tag, 'C1', (x, y), hexed, got, want)
    out['C1'] = worst
    call('close_document', document_id=doc, discard_changes=True)
    print(f'PASS {tag} C1: {len(cmyk_probes())} probes equal a Color Overlay of the texel '
          f'(max {worst})', flush=True)


# ----------------------------------------------------------------- Angle --

ANGLE, ANGLE_SCALE = 30, 0.5


def angled(angle=ANGLE):
    return overlay(tile_t(), scale=ANGLE_SCALE, angle=angle, phase=[0, 0],
                   link_with_layer=True)


def angle_build(call, kind):
    """The RGB8 / RGB16 build, or the RGB16 build converted to RGB32 or CMYK16."""
    doc = build(call, 8 if kind == 'rgb8' else 16)
    if kind == 'rgb32':
        call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    if kind == 'cmyk16':
        call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    return doc


def angle_reads(call, doc, kind):
    if kind == 'cmyk16':
        return ink16(call, doc)
    depth = {'rgb8': 8, 'rgb16': 16, 'rgb32': 32}[kind]
    return [sample(call, doc, x, y, depth) for x, y in probes()]


def same_reads(got, want, what):
    assert len(got) == len(want), what
    for i, (a, b) in enumerate(zip(got, want)):
        assert all(abs(p - q) <= 1e-6 for p, q in zip(a, b)), (what, i, a, b)


def drive_angle(call, folder, kind, receipts):
    """A1 to A4 (module doc)."""
    tag = f'{kind} angle'
    work = Path(folder) / f'angle-{kind}'
    work.mkdir()
    out = receipts.setdefault(kind, {})
    doc = angle_build(call, kind)
    layer = index_of(call, doc, 'Pattern')
    call('set_layer_style', document_id=doc, layer=layer, style=angled(0))
    flat = angle_reads(call, doc, kind)
    call('set_layer_style', document_id=doc, layer=layer, style=angled())
    styled = pattern_of(style_of(call, doc))
    assert styled.get('angle') == 30.0, (tag, styled.get('angle'))
    turned = angle_reads(call, doc, kind)
    differ = max(max(abs(p - q) for p, q in zip(a, b)) for a, b in zip(turned, flat))
    assert differ > 2, (tag, 'A1: the angle changes nothing', differ)
    out['A1'] = round(differ, 3)
    print(f'PASS {tag} A1: angle 30 echoes 30.0 and moves the probes (up to {out["A1"]})',
          flush=True)

    rz = work / 'angle.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    words = [word_after(rz, i) for i in range(4)]
    if kind == 'cmyk16':
        assert words[0] == 46 and 82 <= words[1] <= 88 and words[2:] == [93, 90], (tag, words)
    else:
        assert words[:3] == [45 if kind == 'rgb8' else 46, 93, 90], (tag, words)
    again = call('open_document', path=str(rz))['document']['id']
    assert pattern_of(style_of(call, again)) == styled, (tag, pattern_of(style_of(call, again)))
    same_reads(angle_reads(call, again, kind), turned, f'{tag} A2 reopened rz')
    call('close_document', document_id=again, discard_changes=True)
    print(f'PASS {tag} A2: save_copy rz writes 93 then 90 ({words}) and reopens the same',
          flush=True)

    name = f'rz-557-angle-{kind}'
    call('save_action', name=name, steps=[dict(
        tool='set_layer_style', arguments={'layer': {'$layer': 'active'}, 'style': angled()},
        note='Layer ▸ Layer Style')])
    # Actions do not run in a CMYK document: the CMYK item runs the action on
    # the RGB16 build and converts it after, as `angle_build` does.
    second = angle_build(call, 'rgb16' if kind == 'cmyk16' else kind)
    call('set_active_layer', document_id=second, index=index_of(call, second, 'Pattern'))
    call('run_action', document_id=second, name=name)
    if kind == 'cmyk16':
        call('convert_mode', document_id=second, mode='cmyk', profile='file', path=str(GENERIC))
    echoed = pattern_of(style_of(call, second))
    assert echoed.get('angle') == 30.0, (tag, echoed.get('angle'))
    same_reads(angle_reads(call, second, kind), turned, f'{tag} A3 run_action')
    call('delete_action', name=name)
    call('close_document', document_id=second, discard_changes=True)
    print(f'PASS {tag} A3: save_action + run_action carry the angle', flush=True)

    if kind == 'rgb32':
        print(f'PASS {tag} A4: not applicable (no Photoshop document is written for a 32-bit '
              'float document)', flush=True)
    else:
        psd = work / 'angle.psd'
        call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
        report = call('inspect_psd', path=str(psd))
        back = call('open_document', path=str(psd), psd_mode='layers',
                    psd_fingerprint=report['source_fingerprint'])['document']['id']
        assert pattern_of(style_of(call, back)).get('angle') == 30.0, tag
        same_reads(angle_reads(call, back, kind), turned, f'{tag} A4 reopened psd')
        call('close_document', document_id=back, discard_changes=True)
        print(f'PASS {tag} A4: export_psd layers reopens the same angle and probes', flush=True)
    call('close_document', document_id=doc, discard_changes=True)


# --------------------------------------------------------- take2's set --

INSPECT_LAYERS = [
    # (name, rect x, y, extra style keys, moved after the style)
    ('Linked', 8, 8, {}, False),
    ('Moved', 48, 8, {}, True),
    ('Unlinked', 88, 8, {'link_with_layer': False}, False),
    ('S37', 8, 56, {'scale': 0.37}, False),
    ('Phase', 48, 56, {'phase': [5, 2]}, False),
]


def build_exports(call, folder, run_id):
    """take2's inspect set (module doc): {name: {files, flat, layers}}."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    tile = tile_t(f'{run_id}-psd')
    sets = {}
    for name, depth in (('rgb16', 16), ('rgb8', 8)):
        doc = call('new_document', width=160, height=100, unit='px', depth=depth,
                   background='white')['document']['id']
        layers = {}
        for layer_name, x, y, extra, moved in INSPECT_LAYERS:
            call('new_layer', document_id=doc, name=layer_name)
            index = index_of(call, doc, layer_name)
            call('select_rect', document_id=doc, x=x, y=y, width=24, height=20)
            call('fill_selection', document_id=doc, layer=index, color=SOURCE_HEX)
            call('deselect', document_id=doc)
            call('set_layer_style', document_id=doc, layer=index,
                 style=overlay(tile, **extra))
            if moved:
                call('move_layers', document_id=doc, layers=[index], dx=MOVE[0], dy=MOVE[1])
            layers[layer_name] = dict(style=pattern_of(style_of(call, doc, layer_name)),
                                      moved=moved)
        files = {}
        if depth == 16:
            files['psd16'] = str(folder / 'pattern16.psd')
            call('export_psd', document_id=doc, path=files['psd16'], format='psd', mode='layers')
            files['psb16'] = str(folder / 'pattern16.psb')
            call('export_psd', document_id=doc, path=files['psb16'], format='psb', mode='layers')
        else:
            files['psd8'] = str(folder / 'pattern8.psd')
            call('export_psd', document_id=doc, path=files['psd8'], format='psd', mode='layers')
        flat = str(folder / f'{name}.rz.tif')
        call('export_psd', document_id=doc, path=flat, format='tiff', mode='flattened')
        call('close_document', document_id=doc, discard_changes=True)
        sets[name] = dict(files=files, flat=flat, layers=layers, pattern=tile['name'])
    return sets


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    from phase5_support import App
    output = Path(args.receipts or tempfile.mkdtemp(prefix='pattern-overlay-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_pattern_overlay.py before any measurement'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        for depth in (8, 16):
            drive_depth(app.call, folder, depth, receipts)
        drive_float(app.call, folder, receipts)
        drive_cmyk(app.call, folder, receipts)
        for kind in ('rgb8', 'rgb16', 'rgb32', 'cmyk16'):
            drive_angle(app.call, folder, kind, receipts)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print('PASS: every item', flush=True)


if __name__ == '__main__':
    main()
