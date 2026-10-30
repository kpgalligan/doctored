#!/usr/bin/env python3
"""Drive: the layer REFERENCE POINT under whole-canvas ops and Free Transform
over MCP. One owned Debug instance on a scratch socket
(`phase5_support.App`, every preset folder private). Expectations are fixed
here before any measurement.

THE BUILD, at RGB8, RGB16, RGB32 (the RGB16 build converted with
`convert_precision rgba32` structure preserve) and CMYK16 (the RGB16 build
converted with `convert_mode cmyk`, the Generic CMYK profile): a 120 x 80 white
document; layer "L0" holding rect (13, 9, 50, 40) #30a050, unmoved and
unstyled; layer "L1" holding rect (70, 30, 40, 24) #3050a0, `move_layers`
+7, +3, then a linked Pattern Overlay of tile T (12 x 8, opaque, texel (x, y)
= (20x + 10, 30y + 15, 200 if x + y is even else 60)) at scale 1, phase
(5, 2). Built and styled before any conversion.

THE RULE (the drive's own, `reference_point_take.py`'s RECORDED RESULT): a
SHIPPED op maps the point by its map M = [a, b, c, d, tx, ty], (x, y) ->
(a x + c y + tx, b x + d y + ty), rounded half away from zero — image_size
187 x 127 M = (x 187/120, y 127/80); rotate 90 M = (80 − y, x); flip
horizontal M = (120 − x, y); canvas_size 151 x 97 anchor center M = the
translation by (15, 8), take1 CS-C's own reading (Photoshop floors the odd
centred delta); crop (10, 6, 100, 68) M = the translation by (−10, −6);
transform_layer L1 rotate 30 M = the clockwise 30° turn about the reply's
`applied` pivot; rotate 30 (a filtered Rotate Canvas, SWAP on the FLOOR plan)
M = the clockwise 30° turn about (120/2, 80/2) carried to that centre plus
the old canvas's floored origin o = (⌊(W′ − W)/2⌋, ⌊(H′ − H)/2⌋) in the
canvas the op reports, then moved by the rotated old canvas's margins there
swapped, (+fy, +fx). A linked anchor follows so that offset − anchor maps by
the rule (L1's echoed anchor), and a style set AFTER the op anchors at
offset′ − rule(reference). A turn or a flip also turns L1's linked tile: its
echo is T turned by the drive's own texel map (rotate 90 T′(i, j) = T(j,
7 − i), 8 x 12; flip T′(i, j) = T(11 − i, j), 12 x 8), named "T".

ITEMS, per depth and per op (image_size, rotate 90, rotate 30, flip
horizontal, transform_layer rotate 30, canvas_size, crop), each from the same
build with the op undone after:
  R1 L1's echoed anchor after the op: offset′ − anchor′ = rule(offset −
     anchor); after rotate 90 and flip, L1's echoed tile is the drive's own
     turned tile, and after every other op L1 keeps its tile.
  R2 `set_layer_style` after the op of a fresh anchor-less overlay at phase
     (0, 0) — on L0 (made at (0, 0)), or for transform_layer on L1 (reference
     (7, 3)) — echoes anchor = offset′ − rule(reference).
  R3 at RGB, for the scale-1 ops (canvas_size, crop, rotate 90, flip): every
     5th pixel inside L1's and L0's filled rects (inset 1, inside the canvas)
     reads L1's tile at floor(L⁻¹(p + 0.5 − O)) mod (12, 8), O = phase +
     offset′ − anchor′ and L the turn's linear part (the identity for the
     translations and for L0's fresh overlay), within 1 code (16 bits as v16 /
     257, 32 bits as center_rgba32 x 255). Not for transform_layer.
  R4 `save_copy rz` reopens with the same anchors and L1 tile; `export_psd`
     layers + `open_document` psd_mode layers reopens with the same offset −
     anchor on both layers and the same L1 tile (not at RGB32: no Photoshop
     document is written for it).
  R5 `undo` twice (the style, then the op) restores L1's echoed anchor and
     offset and leaves L0 unstyled.
Every item prints PASS or raises.
"""
import argparse
import json
import math
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 120, 80
L0 = dict(x=13, y=9, width=50, height=40)
L1 = dict(x=70, y=30, width=40, height=24)
L0_HEX, L1_HEX = '#30a050', '#3050a0'
MOVE = (7, 3)
PHASE1 = (5, 2)
TW, TH = 12, 8
BOUND = 1.0
B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
KINDS = ('rgb8', 'rgb16', 'rgb32', 'cmyk16')


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


def tile_rgba(width, height, source):
    """The rgba of a `width x height` tile whose (i, j) is T's `source(i, j)`."""
    return bytes(v for j in range(height) for i in range(width)
                 for v in (*texel(*source(i, j)), 255))


T_RGBA = tile_rgba(TW, TH, lambda i, j: (i, j))
# The drive's own turned tiles (module doc): (width, height, rgba).
R90_TILE = (TH, TW, tile_rgba(TH, TW, lambda i, j: (j, TH - 1 - i)))
FH_TILE = (TW, TH, tile_rgba(TW, TH, lambda i, j: (TW - 1 - i, j)))


def overlay(phase):
    tile = dict(name='T', width=TW, height=TH, rgba=base64(T_RGBA))
    return {'effects': [dict(type='pattern_overlay', blend='normal', opacity=1, pattern=tile,
                             scale=1, phase=list(phase), link_with_layer=True)]}


def half_away(v):
    return int(v + 0.5) if v >= 0 else -int(-v + 0.5)


# The ops and their rule (module doc).
def shipped(m):
    a, b, c, d, tx, ty = m
    return lambda p: (half_away(a * p[0] + c * p[1] + tx), half_away(b * p[0] + d * p[1] + ty))


def identity(u, v):
    return (u, v)


def turned_rect(r):
    """rotate 90: (x, y) -> (H − y, x); a rect's image is (H − y − h, x, h, w)."""
    return dict(x=H - r['y'] - r['height'], y=r['x'], width=r['height'], height=r['width'])


def flipped_rect(r):
    """flip horizontal: (x, y) -> (W − x, y)."""
    return dict(r, x=W - r['x'] - r['width'])


def centred(degrees, before, after):
    """rotate (filtered): the clockwise turn about `before`'s centre carried to
    `after`'s, the canvas the op reports."""
    r = math.radians(degrees)
    c, s = math.cos(r), math.sin(r)
    (cx, cy), (qx, qy) = (before[0] / 2, before[1] / 2), (after[0] / 2, after[1] / 2)
    return (c, s, -s, c, qx - c * cx + s * cy, qy - s * cx - c * cy)


def swapped(degrees, before, after):
    """rotate (filtered), SWAP on the FLOOR plan: the old
    canvas's top-left lands at o = (floor((W′ − W)/2), floor((H′ − H)/2)) in
    the canvas the op reports, so the turn about `before`'s centre is carried
    to that centre plus o; the rotated old canvas's margins there are
    fx = Cx − bw/2 and fy = Cy − bh/2 (bw = W|cos| + H|sin|, bh = W|sin| +
    H|cos|), and the point moves by them SWAPPED, (+fy, +fx)."""
    (w, h), (w2, h2) = before, after
    ox, oy = (w2 - w) // 2, (h2 - h) // 2
    a, b, c, d, tx, ty = centred(degrees, before, (w + 2 * ox, h + 2 * oy))
    r = math.radians(degrees)
    cos, sin = abs(math.cos(r)), abs(math.sin(r))
    fx = (w / 2 + ox) - (w * cos + h * sin) / 2
    fy = (h / 2 + oy) - (w * sin + h * cos) / 2
    return (a, b, c, d, tx + fy, ty + fx)


def clockwise(degrees, pivot):
    """transform_layer's matrix for `rotate` alone: pivot + R(p − pivot), R the
    clockwise-on-screen turn (canvas y grows downward)."""
    r = math.radians(degrees)
    c, s = math.cos(r), math.sin(r)
    px, py = pivot
    return (c, s, -s, c, px - c * px + s * py, py - s * px - c * py)


# Each op (module doc): the tool, its arguments, its rule (`'reply'` for one
# whose M the reply's pivot builds, `'canvas'` for one whose M the reported
# canvas after the op builds), the
# filled rects' images for R3 (`None`: no R3), L⁻¹ for L1's tile, and L1's
# turned tile.
OPS = [
    dict(name='image_size', arguments=dict(width=187, height=127, filter='nearest'),
         rule=shipped((187 / W, 0, 0, 127 / H, 0, 0)), rects=None),
    dict(name='rotate', arguments=dict(degrees=90), rule=shipped((0, 1, -1, 0, H, 0)),
         rects=turned_rect, unturn=lambda u, v: (v, -u), tile=R90_TILE),
    dict(name='rotate', arguments=dict(degrees=30), rule='canvas', rects=None),
    dict(name='flip', arguments=dict(axis='horizontal'), rule=shipped((-1, 0, 0, 1, W, 0)),
         rects=flipped_rect, unturn=lambda u, v: (-u, v), tile=FH_TILE),
    dict(name='transform_layer', arguments=dict(rotate=30), rule='reply', rects=None),
    dict(name='canvas_size', arguments=dict(width=151, height=97, anchor='center'),
         rule=shipped((1, 0, 0, 1, 15, 8)), rects='translation'),
    dict(name='crop', arguments=dict(x=10, y=6, width=100, height=68),
         rule=shipped((1, 0, 0, 1, -10, -6)), rects='translation'),
]


# ------------------------------------------------------------ the reads --

def layers(call, doc):
    return {l['name']: l for l in call('get_document', document_id=doc)['layers']}


def overlays(layer):
    return [e for e in (layer.get('style') or {}).get('effects', [])
            if e['type'] == 'pattern_overlay']


def anchor(layer):
    effects = overlays(layer)
    return tuple(effects[0]['anchor']) if effects and 'anchor' in effects[0] else None


def tile(layer):
    """The echoed tile of the layer's overlay: (name, width, height, rgba)."""
    pattern = overlays(layer)[0]['pattern']
    return (pattern.get('name'), pattern.get('width'), pattern.get('height'),
            pattern.get('rgba'))


def offset(layer):
    return (layer['offset_x'], layer['offset_y'])


def minus(a, b):
    return (a[0] - b[0], a[1] - b[1])


def sample(call, doc, x, y, kind):
    reply = call('sample_color', document_id=doc, x=x, y=y)
    if kind == 'rgb32':
        return [v * 255 for v in reply['center_rgba32'][:3]]
    if kind == 'rgb16':
        return [v / 257 for v in reply['center_rgba16'][:3]]
    hexed = reply['hex'].lstrip('#')
    return [int(hexed[i:i + 2], 16) for i in (0, 2, 4)]


def build(call, kind):
    depth = 8 if kind == 'rgb8' else 16
    doc = call('new_document', width=W, height=H, unit='px', depth=depth,
               background='white')['document']['id']
    for name, rect, color in (('L0', L0, L0_HEX), ('L1', L1, L1_HEX)):
        call('new_layer', document_id=doc, name=name)
        call('select_rect', document_id=doc, **rect)
        call('fill_selection', document_id=doc, layer=layers(call, doc)[name]['index'],
             color=color)
        call('deselect', document_id=doc)
    l1 = layers(call, doc)['L1']['index']
    call('move_layers', document_id=doc, layers=[l1], dx=MOVE[0], dy=MOVE[1])
    call('set_layer_style', document_id=doc, layer=l1, style=overlay(PHASE1))
    if kind == 'rgb32':
        call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    if kind == 'cmyk16':
        call('convert_mode', document_id=doc, mode='cmyk', profile='file', path=str(GENERIC))
    return doc


def rect_after(before_rect, before_offset, after_offset):
    """A filled rect's canvas position after a translation-only op: it moved
    with its layer."""
    d = minus(after_offset, before_offset)
    return dict(x=before_rect['x'] + d[0], y=before_rect['y'] + d[1],
                width=before_rect['width'], height=before_rect['height'])


def probe_tile(call, doc, kind, rect, origin, size, what, unturn=identity):
    """Every 5th pixel of `rect` reads T at floor(L⁻¹(p + 0.5 − origin)), `unturn`
    being L⁻¹."""
    worst = 0.0
    xs = range(max(rect['x'] + 1, 0), min(rect['x'] + rect['width'] - 1, size[0]), 5)
    ys = range(max(rect['y'] + 1, 0), min(rect['y'] + rect['height'] - 1, size[1]), 5)
    n = 0
    for y in ys:
        for x in xs:
            got = sample(call, doc, x, y, kind)
            a, b = unturn(x + 0.5 - origin[0], y + 0.5 - origin[1])
            want = texel(math.floor(a) % TW, math.floor(b) % TH)
            e = max(abs(a - b) for a, b in zip(got, want))
            worst = max(worst, e)
            n += 1
            assert e <= BOUND, (what, (x, y), got, want)
    assert n > 0, (what, 'no probe inside the canvas')
    return n, round(worst, 3)


# ---------------------------------------------------------------- items --

def drive_op(call, doc, kind, op, work, receipts):
    name, arguments, rule = op['name'], dict(op['arguments']), op['rule']
    label = f'{name} {json.dumps(arguments, sort_keys=True)}'
    tag = f'{kind} {label}'
    before = layers(call, doc)
    l1_before = (offset(before['L1']), anchor(before['L1']))
    tile_before = tile(before['L1'])
    assert l1_before[1] is not None and anchor(before['L0']) is None, (tag, l1_before)
    l0_rect_offset = offset(before['L0'])
    if name == 'transform_layer':
        arguments['layer'] = before['L1']['index']
    reply = call(name, document_id=doc, **arguments)
    if rule == 'reply':
        applied = reply['applied']
        rule = shipped(clockwise(arguments['rotate'], (applied['pivot_x'], applied['pivot_y'])))
    after = layers(call, doc)
    size = (call('get_document', document_id=doc)['width'],
            call('get_document', document_id=doc)['height'])
    if rule == 'canvas':
        rule = shipped(swapped(arguments['degrees'], (W, H), size))
    o1, a1 = offset(after['L1']), anchor(after['L1'])
    want = rule(minus(l1_before[0], l1_before[1]))
    assert minus(o1, a1) == want, (tag, 'R1', o1, a1, want)
    t1 = tile(after['L1'])
    if 'tile' in op:
        width, height, rgba = op['tile']
        assert t1 == ('T', width, height, base64(rgba)), (tag, 'R1 tile', t1[:3])
    else:
        assert t1 == tile_before, (tag, 'R1 tile kept', t1[:3])
    print(f'PASS {tag} R1: L1 anchor {a1} at offset {o1}, tile {t1[1]} x {t1[2]}', flush=True)

    # R2: a style set after the op anchors at offset′ − rule(reference).
    restyle, reference = ('L1', MOVE) if name == 'transform_layer' else ('L0', (0, 0))
    moved_reference = rule(reference)
    # Phase (0, 0): on L1 a different style from the one it holds, so the set
    # is an edit (and an undo step) even though its anchor is the mapped one.
    call('set_layer_style', document_id=doc, layer=after[restyle]['index'],
         style=overlay((0, 0)))
    styled = layers(call, doc)
    o, a = offset(styled[restyle]), anchor(styled[restyle])
    assert a == minus(o, moved_reference), (tag, 'R2', restyle, o, a, moved_reference)
    print(f'PASS {tag} R2: {restyle} styled after the op anchors at {a} = offset − '
          f'{moved_reference}', flush=True)
    a1 = anchor(styled['L1'])
    o0, a0 = offset(styled['L0']), anchor(styled['L0'])
    t1 = tile(styled['L1'])

    if op['rects'] is not None and kind != 'cmyk16':
        l1_moved = dict(L1, x=L1['x'] + MOVE[0], y=L1['y'] + MOVE[1])
        if op['rects'] == 'translation':
            l1_rect = rect_after(l1_moved, l1_before[0], o1)
            l0_rect = rect_after(L0, l0_rect_offset, o0)
        else:
            l1_rect, l0_rect = op['rects'](l1_moved), op['rects'](L0)
        n1, w1 = probe_tile(call, doc, kind, l1_rect,
                            (PHASE1[0] + o1[0] - a1[0], PHASE1[1] + o1[1] - a1[1]), size,
                            f'{tag} R3 L1', op.get('unturn', identity))
        n0, w0 = probe_tile(call, doc, kind, l0_rect, minus(o0, a0), size, f'{tag} R3 L0')
        receipts[f'{tag} R3'] = dict(l1=(n1, w1), l0=(n0, w0))
        print(f'PASS {tag} R3: {n1} L1 and {n0} L0 probes read the tile at phase + offset − '
              f'anchor (max {max(w1, w0)})', flush=True)

    stem = f'{kind}-{len(list(work.iterdir()))}'
    rz = work / f'{stem}.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    again = call('open_document', path=str(rz))['document']['id']
    back = layers(call, again)
    assert (anchor(back['L1']), anchor(back['L0'])) == (a1, a0), (tag, 'R4 rz')
    assert tile(back['L1']) == t1, (tag, 'R4 rz tile')
    call('close_document', document_id=again, discard_changes=True)
    if kind != 'rgb32':
        psd = work / f'{stem}.psd'
        call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
        report = call('inspect_psd', path=str(psd))
        opened = call('open_document', path=str(psd), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])['document']['id']
        back = layers(call, opened)
        for layer_name, (o, a) in (('L1', (o1, a1)), ('L0', (o0, a0))):
            if a is None:
                assert anchor(back[layer_name]) is None, (tag, 'R4 psd', layer_name)
                continue
            assert minus(offset(back[layer_name]), anchor(back[layer_name])) == minus(o, a), \
                (tag, 'R4 psd', layer_name, offset(back[layer_name]), anchor(back[layer_name]))
        assert tile(back['L1'])[1:] == t1[1:], (tag, 'R4 psd tile', tile(back['L1'])[:3])
        call('close_document', document_id=opened, discard_changes=True)
    print(f'PASS {tag} R4: save_copy rz' + ('' if kind == 'rgb32' else ' and export_psd')
          + ' reopen the same anchors and L1 tile', flush=True)

    call('undo', document_id=doc)
    call('undo', document_id=doc)
    restored = layers(call, doc)
    assert (offset(restored['L1']), anchor(restored['L1'])) == l1_before, \
        (tag, 'R5', offset(restored['L1']), anchor(restored['L1']), l1_before)
    assert tile(restored['L1']) == tile_before, (tag, 'R5 tile')
    assert anchor(restored['L0']) is None, (tag, 'R5 L0')
    print(f'PASS {tag} R5: undo restores L1 {l1_before} and leaves L0 unstyled', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    from phase5_support import App
    output = Path(args.receipts or tempfile.mkdtemp(prefix='reference-point-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_reference_point.py before any '
                                'measurement'}
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        for kind in KINDS:
            work = folder / kind
            work.mkdir()
            doc = build(app.call, kind)
            for op in OPS:
                drive_op(app.call, doc, kind, op, work, receipts)
            app.call('close_document', document_id=doc, discard_changes=True)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str) + '\n')
    print('PASS: every item', flush=True)


if __name__ == '__main__':
    main()
