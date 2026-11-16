#!/usr/bin/env python3
"""Warp Text (Layer ▸ Warp Text…; MCP `add_text_layer` and `edit_text_layer` with `warp`;
designs/warp-text.md) on an owned Debug instance, in every document kind that holds text layers:
RGB at 8, 16 and 32 bits, Grayscale, Lab and CMYK.

The oracle is WT-2's formulas written here, never a stored image: Menlo 24, black on white with
one text layer at a time. The ink a centroid weighs is an RGB layer's own alpha plane (its
coverage: a 32-bit composite blends in linear light and so weighs edges through the display
encoding), a CMYK document's black plate (ink composited in ink numbers, not seen through the
profile) and in Grayscale and Lab the composite's darkness; presence and order read the
composite's darkness everywhere. The text block's box is the layer's origin (`text.origin`) and its unwarped bounds less twice the
padding (5 px at 24 px). The gates are FIXED here before anything is measured. The instance is
owned by this file (`phase5_support.App`: a scratch MCP socket, a dummy API key, a private
directory for every preference library) and ends with it; no Doctored process this file did not
start is signalled.

Per kind:
- **W1 arch** b 0.5: each inked column's ink-weighted y-centroid moves from the unwarped
  render's by `−b·(h/2)·p(c)` within 0.75 px.
- **W2 flag** b 0.5: the same with `b·(h/2)·sin(2πu)`.
- **W3 squeeze** b 0.5 on three lines: each inked row's x-centroid about the box's centre scales by
  `1 − 0.75·b·p(d)` within 0.75 px.
- **W4 arc** b 1: every inked pixel lies in the half annulus [R − 2, R + h + 2] about the centre
  below the box, R = w/π, and none below the centre; b −1 on "AB": A's ink (B's colour run made
  transparent) has its mean x left of B's (A's made transparent), and no ink lies inside R − 2.
- **W5 underline**: an underlined arch's underline (the render with it less the render without)
  has its columns' y-centroids moved from the unwarped underline's by W1's shift within 0.75 px.
- **W6 none**: `warp: null` and an all-zero warp each make the layer equal the never-warped one
  (render and text object), the saved copy holds no "warp", and on an unwarped layer neither adds
  an undo step.
- **W7 steps**: every warp edit is one undo step.
- **W8 reopen**: `save_copy` as .doctored and `open_document` give an equal render and text
  object; the copy with its family byte-patched to one this Mac lacks, after
  `replace_missing_fonts` back to Menlo, too.
- **W9 refusals**, each with its sentence and no undo step: an unknown style, a number 1.5, a warp
  on a layer on a path, `on_path` on a warped layer, and an emoji under a warp.

RGB 8 only, **W10 swash**: Zapfino's one-glyph "Zapfino" at 48 px under an arch b 0.5 leaves the
layer's outermost raster ring free of ink, so the mapped rect covers the warped swash.

Usage: python3 -B scripts/verify_warp_text.py [--receipts <folder>] [--kinds k,…]
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 400, 320
FONT, SIZE = 'Menlo', 24
PAD = 5                                 # TextLayer.padding(forSize: 24)
ADVANCE = 1233 / 2048 * SIZE            # Menlo's fixed advance
TEXT = 'WARPED TEXT LINE'
LINES = 'SQUEEZE ME\nTHREE ROWS\nOF MENLO'
ORIGIN = (60, 60)
BEND = 0.5
SHIFT_TOL = 0.75                        # px, a column's or a row's centroid against the formula
RING_TOL = 2                            # px, W4's annulus slack
WIDTH_TOL = 1                           # px, the box width against Menlo's advance
INK_FLOOR = 8                           # darkness below this is paper, not ink
MIN_WEIGHT = 2 * 255                    # a column or row with less ink is an antialiased sliver
LINE_WEIGHT = 255                       # the underline alone is about 2 px thick: one pixel of it
ABSENT = 'Zzzzz'                        # a family no Mac has, as long as FONT
assert len(ABSENT.encode()) == len(FONT.encode())
PATCH_FROM = f'"font":"{FONT}"'.encode()
PATCH_TO = f'"font":"{ABSENT}"'.encode()
SWASH_W, SWASH_H, SWASH_SIZE = 800, 640, 48
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'lab8': dict(color_mode='lab', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
SENTENCES = {
    'style': 'warp.style must be one of arch, upper, lower, bulge, squeeze, inflate, flag, rise, '
             'arc',
    'number': 'warp.bend must be a number from -1 to 1',
    'on_path_layer': 'a text layer on a path cannot be warped: take it off its path first '
                     '(on_path: null)',
    'warped_layer': 'a warped text layer cannot go on a path: remove the warp first (warp: null)',
    'emoji': 'warped text needs outline glyphs: remove the colour or bitmap glyphs (emoji), or '
             'remove the warp',
}


def p(t):
    return 1 - t * t


def unit(t):
    return min(max(t, 0.0), 1.0)


def ring_path(centre, r, arcs=64):
    """A clockwise (y down) closed circle of `arcs` cubic arcs."""
    step = 2 * math.pi / arcs
    h = 4 / 3 * math.tan(step / 4) * r
    anchors = []
    for k in range(arcs):
        a = k * step
        q = (centre[0] + r * math.cos(a), centre[1] + r * math.sin(a))
        t = (-math.sin(a), math.cos(a))
        anchors.append({'x': q[0], 'y': q[1], 'in': [q[0] - h * t[0], q[1] - h * t[1]],
                        'out': [q[0] + h * t[0], q[1] + h * t[1]], 'smooth': True})
    return {'closed': True, 'anchors': anchors}


def png_pixels(data):
    from verify_phase2 import read_png
    return read_png(base64.b64decode(data))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--kinds', default=','.join(KINDS))
    args = parser.parse_args()
    kinds = [k for k in args.kinds.split(',') if k]
    assert all(k in KINDS for k in kinds), kinds
    output = Path(args.receipts or tempfile.mkdtemp(prefix='warp-text-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    # A background instance: never activates itself or takes the screen (`BackgroundLaunch`).
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_warp_text.py'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def render(doc, **extra):
            reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'max_side': 4096, **extra}})
            assert not reply.get('isError'), reply
            return next(item['data'] for item in reply['content'] if item['type'] == 'image')

        def drawn(doc):
            """{(x, y): darkness} of every pixel the composite inks past INK_FLOOR."""
            width, _, pixels = png_pixels(render(doc))
            out = {}
            for i, px in enumerate(pixels):
                d = 255 - min(px[:3])
                if d > INK_FLOOR:
                    out[(i % width, i // width)] = d
            return out

        def alpha_ink(doc, layer):
            width, _, pixels = png_pixels(render(doc, layer=layer, channel='alpha'))
            return {(i % width, i // width) for i, px in enumerate(pixels) if px[0] > 0}

        def coverage(kind, doc, layer):
            """{(x, y): weight} of the layer's ink: an RGB layer's own alpha plane (its coverage,
            which a 32-bit composite would weigh through the display encoding), else the
            composite's darkness (blended in the document's encoded numbers)."""
            if kind.startswith('cmyk'):
                # The black plate: ink composited in ink numbers, never through the profile.
                width, _, pixels = png_pixels(render(doc, channel='black'))
                return {(i % width, i // width): 255 - min(px[:3]) for i, px in enumerate(pixels)
                        if 255 - min(px[:3]) > INK_FLOOR}
            if not kind.startswith('rgb'):
                return drawn(doc)
            width, _, pixels = png_pixels(render(doc, layer=layer, channel='alpha'))
            return {(i % width, i // width): px[0] for i, px in enumerate(pixels)
                    if px[0] > INK_FLOOR}

        def centroids(ink, axis, least=MIN_WEIGHT):
            """Per column (axis 0) or row (axis 1) holding at least `least` ink: (weight, the
            other coordinate's centroid)."""
            sums = {}
            for (x, y), d in ink.items():
                key, other = (x, y) if axis == 0 else (y, x)
                w, m = sums.get(key, (0.0, 0.0))
                sums[key] = (w + d, m + d * (other + 0.5))
            return {k: (w, m / w) for k, (w, m) in sums.items() if w >= least}

        def rows(doc):
            return {row['index']: row for row in call('get_document', document_id=doc)['layers']}

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        def one_step(doc, before, label):
            now = undo_count(doc)
            assert now == before + 1, (label, 'undo steps', before, now)

        def saved_bytes(doc, name):
            path = folder / name
            call('save_copy', document_id=doc, path=str(path), format='doctored')
            return path.read_bytes()

        def box_of(doc, layer, size=SIZE):
            """The layer's text block (x0, y0, w, h): its origin and its unwarped bounds less
            twice the padding."""
            row = rows(doc)[layer]
            origin = row['text']['origin']
            return (origin['x'], origin['y'], row['width'] - 2 * PAD, row['height'] - 2 * PAD)

        def compare_columns(flat, bent, shift, label, least=MIN_WEIGHT):
            """Every column inked in both: the bent centroid is the flat one moved by shift(x)."""
            a, b = centroids(flat, 0, least), centroids(bent, 0, least)
            shared = sorted(set(a) & set(b))
            assert len(shared) >= 20, (label, 'too few shared columns', len(shared))
            worst = max(abs(b[x][1] - (a[x][1] + shift(x + 0.5))) for x in shared)
            assert worst <= SHIFT_TOL, (label, 'column centroid off by', worst)
            return worst

        base = dict(font=FONT, size=SIZE, rgba=[0, 0, 0, 1], wrap_width=0)

        for kind in kinds:
            doc = call('new_document', width=W, height=H, background='white',
                       **KINDS[kind])['document']['id']
            made = call('add_text_layer', document_id=doc, text=TEXT, x=ORIGIN[0], y=ORIGIN[1],
                        **base)
            layer = made['layer']
            x0, y0, w, h = box_of(doc, layer)
            assert abs(w - len(TEXT) * ADVANCE) <= WIDTH_TOL, (kind, 'box width', w)
            xc = x0 + w / 2
            flat = coverage(kind, doc, layer)
            flat_text = rows(doc)[layer]['text']
            flat_render = render(doc)
            assert 'warp' not in flat_text, flat_text

            # ---- W6 (unwarped half): null and all-zero change nothing ---------------------
            before = undo_count(doc)
            call('edit_text_layer', document_id=doc, layer=layer, warp=None)
            call('edit_text_layer', document_id=doc, layer=layer,
                 warp={'style': 'arch', 'bend': 0, 'horizontal': 0, 'vertical': 0})
            assert undo_count(doc) == before, (kind, 'w6 a no-warp edit added a step')
            assert render(doc) == flat_render, (kind, 'w6 a no-warp edit changed the render')

            # ---- W1 arch ------------------------------------------------------------------
            before = undo_count(doc)
            reply = call('edit_text_layer', document_id=doc, layer=layer,
                         warp={'style': 'arch', 'bend': BEND})
            one_step(doc, before, f'{kind} w1')
            assert reply['text'].get('warp') == {'style': 'arch', 'bend': BEND, 'horizontal': 0,
                                                 'vertical': 0}, reply['text'].get('warp')

            def arch(x):
                return -BEND * (h / 2) * p(2 * unit((x - x0) / w) - 1)

            worst = compare_columns(flat, coverage(kind, doc, layer), arch, f'{kind} w1')
            print(f'PASS {kind} w1: arch moves every column by its shift (worst {worst:.3f} px)',
                  flush=True)

            # ---- W2 flag ------------------------------------------------------------------
            before = undo_count(doc)
            call('edit_text_layer', document_id=doc, layer=layer, warp={'style': 'flag'})
            one_step(doc, before, f'{kind} w2')

            def flag(x):
                return BEND * (h / 2) * math.sin(2 * math.pi * unit((x - x0) / w))

            worst = compare_columns(flat, coverage(kind, doc, layer), flag, f'{kind} w2')
            print(f'PASS {kind} w2: flag moves every column by its wave (worst {worst:.3f} px)',
                  flush=True)

            # ---- W6 (warped half) and W7 -------------------------------------------------
            for label, warp in (('null', None),
                                ('all zero', {'style': 'flag', 'bend': 0})):
                call('edit_text_layer', document_id=doc, layer=layer,
                     warp={'style': 'arch', 'bend': BEND})
                before = undo_count(doc)
                off = call('edit_text_layer', document_id=doc, layer=layer, warp=warp)
                one_step(doc, before, f'{kind} w6 {label}')
                assert 'warp' not in off['text'], (kind, label, off['text'])
                assert rows(doc)[layer]['text'] == flat_text, (kind, 'w6', label, 'text object')
                assert render(doc) == flat_render, (kind, 'w6', label, 'render')
            assert b'"warp"' not in saved_bytes(doc, f'{kind}-unwarped.doctored'), \
                (kind, 'w6 the saved copy holds a warp')
            print(f'PASS {kind} w6/w7: null and all zero equal the never-warped layer, each warp '
                  'edit one step, none on an unwarped layer', flush=True)

            # ---- W5 underline -------------------------------------------------------------
            call('edit_text_layer', document_id=doc, layer=layer, underline=True)
            flat_lined = coverage(kind, doc, layer)
            call('edit_text_layer', document_id=doc, layer=layer,
                 warp={'style': 'arch', 'bend': BEND})
            bent_plain_layer = call('edit_text_layer', document_id=doc, layer=layer,
                                    underline=False)
            assert bent_plain_layer['text'].get('warp'), (kind, 'w5 the edit dropped the warp')
            bent_plain = coverage(kind, doc, layer)
            call('edit_text_layer', document_id=doc, layer=layer, underline=True)
            bent_lined = coverage(kind, doc, layer)

            def only(lined, plain):
                return {k: d - plain.get(k, 0) for k, d in lined.items()
                        if d - plain.get(k, 0) > INK_FLOOR}

            # The underline alone, straight and bent: no descender in TEXT crosses it.
            worst = compare_columns(only(flat_lined, flat), only(bent_lined, bent_plain), arch,
                                    f'{kind} w5', least=LINE_WEIGHT)
            call('edit_text_layer', document_id=doc, layer=layer, underline=False, warp=None)
            print(f'PASS {kind} w5: the underline follows the arch (worst {worst:.3f} px)',
                  flush=True)

            # ---- W8 reopen ----------------------------------------------------------------
            call('edit_text_layer', document_id=doc, layer=layer,
                 warp={'style': 'rise', 'bend': -0.4, 'horizontal': 0.3, 'vertical': -0.2})
            data = saved_bytes(doc, f'{kind}.doctored')
            copy = folder / f'{kind}.doctored'
            reopened = call('open_document', path=str(copy))['document']['id']
            first, second = rows(doc), rows(reopened)
            assert first[layer]['text'] == second[layer]['text'], (kind, 'w8 text object')
            assert first[layer]['text'].get('warp'), (kind, 'w8 no warp')
            assert render(doc) == render(reopened), (kind, 'w8 render')
            call('close_document', document_id=reopened, discard_changes=True)
            assert data.count(PATCH_FROM) >= 1, (kind, 'w8 no family to patch')
            patched = folder / f'{kind}-missing.doctored'
            patched.write_bytes(data.replace(PATCH_FROM, PATCH_TO))
            missing = call('open_document', path=str(patched))['document']['id']
            assert rows(missing)[layer]['text'].get('font_missing') is True, (kind, 'w8 missing')
            call('replace_missing_fonts', document_id=missing,
                 replacements=[{'font': ABSENT, 'with': FONT}])
            replaced = rows(missing)[layer]['text']
            assert replaced == first[layer]['text'], (kind, 'w8 replaced text', {
                key: (replaced.get(key), first[layer]['text'].get(key))
                for key in set(replaced) | set(first[layer]['text'])
                if replaced.get(key) != first[layer]['text'].get(key)})
            assert render(missing) == render(doc), (kind, 'w8 replaced render')
            call('close_document', document_id=missing, discard_changes=True)
            print(f'PASS {kind} w8: the saved copy reopens equal, and Replace Missing Fonts keeps '
                  'the warp', flush=True)
            call('delete_layer', document_id=doc, layer=layer)

            # ---- W3 squeeze on three lines ------------------------------------------------
            three = call('add_text_layer', document_id=doc, text=LINES, x=ORIGIN[0],
                         y=ORIGIN[1], **base)['layer']
            sx0, sy0, sw, sh = box_of(doc, three)
            sxc = sx0 + sw / 2
            square = coverage(kind, doc, three)
            call('edit_text_layer', document_id=doc, layer=three,
                 warp={'style': 'squeeze', 'bend': BEND})
            squeezed = coverage(kind, doc, three)
            a, b = centroids(square, 1), centroids(squeezed, 1)
            shared = sorted(set(a) & set(b))
            assert len(shared) >= 30, (kind, 'w3 too few rows', len(shared))

            def scale(y):
                return 1 - 0.75 * BEND * p(2 * unit((y - sy0) / sh) - 1)

            worst = max(abs((b[y][1] - sxc) - scale(y + 0.5) * (a[y][1] - sxc)) for y in shared)
            assert worst <= SHIFT_TOL, (kind, 'w3 row centroid off by', worst)
            call('delete_layer', document_id=doc, layer=three)
            print(f'PASS {kind} w3: squeeze scales every row about the centre (worst {worst:.3f} '
                  'px)', flush=True)

            # ---- W4 arc -------------------------------------------------------------------
            arc = call('add_text_layer', document_id=doc, text=TEXT, x=ORIGIN[0], y=ORIGIN[1],
                       warp={'style': 'arc', 'bend': 1}, **base)['layer']
            big_r = w / math.pi
            centre = (xc, y0 + h + big_r)
            ink = drawn(doc)
            assert ink, (kind, 'w4 no ink')
            off = [(x, y) for (x, y) in ink
                   if not (big_r - RING_TOL
                           <= math.hypot(x + 0.5 - centre[0], y + 0.5 - centre[1])
                           <= big_r + h + RING_TOL) or y + 0.5 > centre[1] + RING_TOL]
            assert not off, (kind, 'w4 ink off the half annulus', off[:8])
            call('delete_layer', document_id=doc, layer=arc)
            # The box is read off the unwarped layer, then the layer is warped.
            pair = call('add_text_layer', document_id=doc, text='AB', x=ORIGIN[0], y=ORIGIN[1],
                        **base)['layer']
            ax0, ay0, aw, ah = box_of(doc, pair)
            call('edit_text_layer', document_id=doc, layer=pair, warp={'style': 'arc', 'bend': -1})
            small_r = aw / math.pi
            under = (ax0 + aw / 2, ay0 - small_r)
            both = drawn(doc)
            inside = [k for k in both
                      if math.hypot(k[0] + 0.5 - under[0], k[1] + 0.5 - under[1])
                      < small_r - RING_TOL]
            assert not inside, (kind, 'w4 ink inside the inner radius', inside[:8])
            means = []
            for hidden in (1, 0):
                call('edit_text_layer', document_id=doc, layer=pair, color_runs=[
                    {'location': hidden, 'length': 1, 'rgba': [0, 0, 0, 0]}])
                one = drawn(doc)
                assert one, (kind, 'w4 a glyph drew no ink')
                means.append(sum((x + 0.5) * d for (x, _), d in one.items())
                             / sum(one.values()))
            assert means[0] < means[1], (kind, 'w4 A is not left of B', means)
            call('delete_layer', document_id=doc, layer=pair)
            print(f'PASS {kind} w4: arc b 1 inks the half annulus; b −1 keeps A ({means[0]:.1f}) '
                  f'left of B ({means[1]:.1f}) and off the centre', flush=True)

            # ---- W9 refusals --------------------------------------------------------------
            plain = call('add_text_layer', document_id=doc, text=TEXT, x=ORIGIN[0], y=ORIGIN[1],
                         **base)['layer']
            ring = ring_path((W / 2, H / 2), 100)
            placed = call('add_text_layer', document_id=doc, text='ON A PATH', font=FONT,
                          size=SIZE, rgba=[0, 0, 0, 1], on_path={'subpaths': [ring]})['layer']
            warped = call('add_text_layer', document_id=doc, text=TEXT, x=ORIGIN[0], y=ORIGIN[1],
                          warp={'style': 'arch', 'bend': BEND}, **base)['layer']
            before = undo_count(doc)
            refusals = {
                'style': call('edit_text_layer', fail=True, document_id=doc, layer=plain,
                              warp={'style': 'twist'}),
                'number': call('edit_text_layer', fail=True, document_id=doc, layer=plain,
                               warp={'style': 'arch', 'bend': 1.5}),
                'on_path_layer': call('edit_text_layer', fail=True, document_id=doc,
                                      layer=placed, warp={'style': 'arch', 'bend': BEND}),
                'warped_layer': call('edit_text_layer', fail=True, document_id=doc,
                                     layer=warped, on_path={'subpaths': [ring]}),
                'emoji': call('add_text_layer', fail=True, document_id=doc, text='Hi \U0001F600',
                              x=ORIGIN[0], y=ORIGIN[1], warp={'style': 'arch', 'bend': BEND},
                              **base),
            }
            for name, text in refusals.items():
                assert SENTENCES[name] in text, (kind, name, text)
            assert undo_count(doc) == before, (kind, 'w9 a refusal added a step')
            receipts[f'{kind}_refusals'] = refusals
            print(f'PASS {kind} w9: every refusal names its reason and adds no step', flush=True)

            # ---- W10 a swash (RGB 8) ------------------------------------------------------
            if kind == 'rgb8':
                wide = call('new_document', width=SWASH_W, height=SWASH_H, background='white',
                            **KINDS[kind])['document']['id']
                swash = call('add_text_layer', document_id=wide, text='Zapfino', font='Zapfino',
                             size=SWASH_SIZE, x=200, y=150, wrap_width=0, rgba=[0, 0, 0, 1],
                             warp={'style': 'arch', 'bend': BEND})
                bounds = swash['bounds']
                inked = alpha_ink(wide, swash['layer'])
                assert inked, ('w10 no ink',)
                left, top = bounds['x'], bounds['y']
                right, bottom = left + bounds['width'] - 1, top + bounds['height'] - 1
                assert left > 0 and top > 0 and right < SWASH_W - 1 and bottom < SWASH_H - 1, \
                    ('w10 the layer leaves the canvas', bounds)
                rim = [k for k in inked if k[0] in (left, right) or k[1] in (top, bottom)]
                receipts['swash'] = {'bounds': bounds, 'rim': len(rim)}
                assert not rim, ('w10 ink on the raster ring', rim[:8], bounds)
                call('close_document', document_id=wide, discard_changes=True)
                print(f'PASS {kind} w10: the warped swash leaves its raster ring clear ({bounds})',
                      flush=True)

            call('close_document', document_id=doc, discard_changes=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all ({", ".join(kinds)}); receipts in {output}', flush=True)


if __name__ == '__main__':
    try:
        main()
    except Exception as failure:
        print(f'FAIL: {type(failure).__name__}: {failure}', flush=True)
        raise
