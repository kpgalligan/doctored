#!/usr/bin/env python3
"""Text on a path (Layer ▸ Place Text on Path / Remove Text from Path; MCP `add_text_layer` and
`edit_text_layer` with `on_path`; designs/type-on-a-path.md) on an owned Debug instance, in every
document kind that holds text layers: RGB at 8, 16 and 32 bits, Grayscale, Lab and CMYK.

The oracle is geometry written here, never a stored image: a clockwise circle of 64 cubic arcs of
radius R about C, Menlo's fixed advance (1233/2048 em) and its vertical metrics (ascender 1901,
descender 483 per 2048), and the ink read back from `render`: black text on white with one text
layer at a time, so the composite's darkness is that layer's ink in every document kind (a
Grayscale or Lab document renders no single layer); the wide glyph reads an RGB layer's own alpha
plane. The gates are FIXED here before anything is measured. The instance is owned
by this file (`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for
every preference library) and ends with it; no Doctored process this file did not start is
signalled.

Per kind:
- **T1 place.** `add_text_layer` with `on_path` (the circle, offset 100) is one undo step; every
  inked pixel lies in the annulus [R − descent − 2, R + ascent + 2]; the ink's angular span (alpha
  ≥ 128) is the line's advance over R within 3 %; the reply reports the offset, side left, the
  length 2πR within 0.05 px, no hidden glyph and `canvas_subpaths` equal to the circle passed.
- **T2 offset.** `on_path` {offset: 100 + Δ} keeps the outline and turns the ink's mean angle by
  Δ/R (within 0.005 rad); one undo step.
- **T3 alignment.** Center moves the mean angle back by half the advance over R, right by the
  whole advance over R (within 0.005 rad).
- **T4 side.** `side` right puts every inked pixel in [R − ascent − 2, R + descent + 2], the
  mean radius inside R.
- **T5 underline.** An underlined line's ink stays in T1's annulus (a straight underline would
  leave it).
- **T6 remove.** `on_path` null is one undo step and equals a never-placed point-text layer at the
  same origin: render and text object.
- **T7 reopen.** Placed again, `save_copy` as .doctored and `open_document`: the render and every
  text object are equal. The copy with its family byte-patched to one this Mac lacks opens with
  the font missing, and `replace_missing_fonts` back to Menlo gives the same render and text
  object, `on_path` included.
- **T8 an open arc.** A quarter of the circle, shorter than the line: `hidden_glyphs` is the count
  of glyph centres (i + ½)·advance past the reported length, and no ink lies past the arc's end
  beyond half an advance (the last drawn glyph's own half).
- **T9 rotated.** A layer under a 30° rotation reports `canvas_subpaths` equal to the subpaths
  passed (1e-6) and inks the annulus.
- **T10 refusals.** Each answers its sentence and adds no undo step: a line break, a wrap_width,
  an emoji, two subpaths with no index, an index past the end, an unknown side, subpaths and
  path_name together, and edit_text_layer naming no path on a layer that is not on one.

RGB 8 only, **T11 wide glyphs**: two one-glyph lines, each the first of its group whose
straight, never-placed render inks a box wider than 3 × its size; a group with none FAILS.
Zapfino's one-glyph "Zapfino" ligature, whose swash reaches far below its baseline and past its
advance, so the straight render is uncut only when its raster covers the glyph's ink
(designs/text-ink-rect.md). And a Helvetica Neue line supplied by a fallback face (U+FDFD, which
Geeza Pro supplies; else U+2E3B), whose box is twice what a rect grown by Helvetica Neue's line
height and padding alone (1.37 em) would hold of it, so an on-path rect without the half-advance
term cuts it; Zapfino's far taller face holds its own glyph without that term. Each is placed
centred on the top of a circle whose top is the middle of a flattened chord (so the glyph sits
level), and centred 1 px inside the end of an open horizontal line (a translation, so a cut end
shows as a narrower box); its ink box's width and height equal its straight render's within 1 px.

RGB 8 only, **T12 legacy reopen**: `scripts/fixtures/text-ink-rect/legacy-zapfino.doctored`, a
straight Zapfino layer saved by a build before `ink_outset` (its numbers in
`legacy-zapfino.json`), reopens with its bounds, origin and alpha ink box, and its `save_copy`
holds the saved payload verbatim; a second copy's first `transform_layer` composes into the
description (`rasterized` false: the raster is not read as stale). Under a transparency lock a
colour-only `edit_text_layer` succeeds with the bounds and origin unchanged and no
`ink_outset`. Unlocked, an `edit_text_layer` keeps the origin, inks T11's straight Zapfino box
within 1 px and saves `ink_outset`.

Usage: python3 -B scripts/verify_text_on_path.py [--receipts <folder>] [--kinds k,…]
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
W, H = 400, 400
CENTRE = (200.0, 200.0)
R = 120.0
ARCS = 64
FONT, SIZE = 'Menlo', 24
ADVANCE = 1233 / 2048 * SIZE            # Menlo's fixed advance
ASCENT = 1901 / 2048 * SIZE             # Menlo's ascender
DESCENT = 483 / 2048 * SIZE             # Menlo's descender, as a depth
TEXT = 'HAND ON THE PATH'
LINE = len(TEXT) * ADVANCE              # the straightened line's advance
OFFSET, DELTA = 100.0, 60.0
ORIGIN = (40, 40)
ANGLE_TOL = 0.005                       # rad: 0.6 px at R
INK_FLOOR = 8                           # darkness below this is paper, not ink
ABSENT = 'Zzzzz'                        # a family no Mac has, as long as FONT
assert len(ABSENT.encode()) == len(FONT.encode())
PATCH_FROM = f'"font":"{FONT}"'.encode()
PATCH_TO = f'"font":"{ABSENT}"'.encode()
SPAN_TOL = 0.03
LENGTH_TOL = 0.05
SUBPATH_TOL = 1e-6
BOX_TOL = 1
WIDE_FACTOR = 3.0
WIDE_SIZE = 48
# T11's two groups (docstring): the ink-rect witness and the half-advance witness.
WIDE_GROUPS = [[('Zapfino', 'Zapfino')], [('﷽', 'Helvetica Neue'), ('⸻', 'Helvetica Neue')]]
# T12's saved layer and what the build that saved it reported.
LEGACY = ROOT / 'scripts/fixtures/text-ink-rect/legacy-zapfino.doctored'
LEGACY_RECORD = ROOT / 'scripts/fixtures/text-ink-rect/legacy-zapfino.json'
# The wide glyph's canvas: room for a glyph some 310 px wide on the circle's top and past the
# line's end.
WIDE_W, WIDE_H = 800, 640
WIDE_CENTRE = (400.0, 360.0)
WIDE_LINE = ((100, 470), (340, 470))
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'lab8': dict(color_mode='lab', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
SENTENCES = {
    'line_break': 'text on a path is one line: remove the line break from text',
    'wrap': 'text on a path does not wrap: leave wrap_width out',
    'emoji': 'text on a path needs outline glyphs: remove the colour or bitmap glyphs (emoji), '
             'or remove the text from the path',
    'one_subpath': 'on_path takes one subpath of at most 1024 anchors with a length: pass '
                   'subpath to pick one',
    'side': 'on_path.side must be "left" or "right"',
    'both': 'pass subpaths or path_name in on_path, not both',
    'none': 'this layer is not on a path: on_path needs subpaths or path_name',
}


def circle(phase=0.0, arcs=ARCS, first=0, last=None, closed=True, centre=CENTRE, r=R):
    """A clockwise (y down) circle of `arcs` cubic arcs from angle `phase`: anchors `first` to
    `last` (all, closed, by default), handles along the tangent at (4/3)·tan(Δ/4)·r."""
    step = 2 * math.pi / arcs
    h = 4 / 3 * math.tan(step / 4) * r
    anchors = []
    for k in range(first, arcs if last is None else last + 1):
        a = phase + k * step
        p = (centre[0] + r * math.cos(a), centre[1] + r * math.sin(a))
        t = (-math.sin(a), math.cos(a))
        anchors.append({'x': p[0], 'y': p[1], 'in': [p[0] - h * t[0], p[1] - h * t[1]],
                        'out': [p[0] + h * t[0], p[1] + h * t[1]], 'smooth': True})
    return {'closed': closed, 'anchors': anchors}


def png_pixels(data):
    from verify_phase2 import read_png
    return read_png(base64.b64decode(data))


def wrap(angle):
    return (angle + math.pi) % (2 * math.pi) - math.pi


def label(text):
    """A receipt key: the text itself, or its code point when it is one character."""
    return f'U+{ord(text):04X}' if len(text) == 1 else text


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--kinds', default=','.join(KINDS))
    args = parser.parse_args()
    kinds = [k for k in args.kinds.split(',') if k]
    assert all(k in KINDS for k in kinds), kinds
    output = Path(args.receipts or tempfile.mkdtemp(prefix='text-on-path-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    # A background instance: never activates itself or takes the screen (`BackgroundLaunch`).
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_text_on_path.py'}
    ring = circle()

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def render(doc, **extra):
            reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'max_side': 4096, **extra}})
            assert not reply.get('isError'), reply
            return next(item['data'] for item in reply['content'] if item['type'] == 'image')

        def ink(doc, layer):
            """(x, y, alpha) of every inked pixel of `layer`, canvas coordinates (RGB only)."""
            width, _, pixels = png_pixels(render(doc, layer=layer, channel='alpha'))
            return [(i % width, i // width, p[0]) for i, p in enumerate(pixels) if p[0] > 0]

        def drawn(doc):
            """(x, y, darkness) of every pixel the composite inks past INK_FLOOR: black text on
            white, the one text layer the document holds, in every document kind."""
            width, _, pixels = png_pixels(render(doc))
            dark = [(i % width, i // width, 255 - min(p[:3])) for i, p in enumerate(pixels)]
            return [(x, y, d) for x, y, d in dark if d > INK_FLOOR]

        def polar(points):
            return [(math.hypot(x + 0.5 - CENTRE[0], y + 0.5 - CENTRE[1]),
                     math.atan2(y + 0.5 - CENTRE[1], x + 0.5 - CENTRE[0]), a)
                    for x, y, a in points]

        def mean_angle(points):
            s = sum(a * math.sin(t) for _, t, a in points)
            c = sum(a * math.cos(t) for _, t, a in points)
            return math.atan2(s, c)

        def span(points):
            """The angular extent of the pixels inked at least half the deepest one's ink."""
            centre = mean_angle(points)
            half = max(a for _, _, a in points) / 2
            angles = [wrap(t - centre) for _, t, a in points if a >= half]
            return max(angles) - min(angles)

        def ink_box(points):
            xs, ys = [x for x, _, _ in points], [y for _, y, _ in points]
            return (max(xs) - min(xs) + 1, max(ys) - min(ys) + 1)

        def ink_rect(points):
            xs, ys = [x for x, _, _ in points], [y for _, y, _ in points]
            return [min(xs), min(ys), max(xs) - min(xs) + 1, max(ys) - min(ys) + 1]

        def saved_payload(doc, name):
            """The text payload a `save_copy` of `doc` stores: DRDC keeps layer meta as is."""
            path = folder / name
            call('save_copy', document_id=doc, path=str(path), format='doctored')
            data = path.read_bytes()
            start = data.find(b'{"alignment"')
            assert start >= 0, (name, 'no text payload')
            depth = 0
            for end in range(start, len(data)):
                depth += {b'{': 1, b'}': -1}.get(data[end:end + 1], 0)
                if depth == 0:
                    return data[start:end + 1].decode()
            raise AssertionError((name, 'unterminated payload'))

        def in_ring(points, low, high):
            return [(round(r, 2), round(t, 3)) for r, t, _ in points if not low <= r <= high]

        def rows(doc):
            return {row['index']: row for row in call('get_document', document_id=doc)['layers']}

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        def same_subpaths(got, want):
            assert len(got) == len(want), (len(got), len(want))
            for g, w in zip(got, want):
                assert g['closed'] == w['closed'] and len(g['anchors']) == len(w['anchors'])
                for a, b in zip(g['anchors'], w['anchors']):
                    pairs = [(a['x'], b['x']), (a['y'], b['y'])] + list(zip(a['in'], b['in'])) \
                        + list(zip(a['out'], b['out']))
                    assert all(abs(p - q) <= SUBPATH_TOL for p, q in pairs), (a, b)

        def one_step(doc, before, label):
            now = undo_count(doc)
            assert now == before + 1, (label, 'undo steps', before, now)

        base = dict(text=TEXT, font=FONT, size=SIZE, rgba=[0, 0, 0, 1])
        annulus = (R - DESCENT - 2, R + ASCENT + 2)

        for kind in kinds:
            doc = call('new_document', width=W, height=H, background='white',
                       **KINDS[kind])['document']['id']
            # ---- T1 place -------------------------------------------------------------------
            before = undo_count(doc)
            reply = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                         on_path={'subpaths': [ring], 'offset': OFFSET}, **base)
            one_step(doc, before, f'{kind} t1')
            layer = reply['layer']
            report = reply['text'].get('on_path')
            assert report and report['offset'] == OFFSET and report['side'] == 'left', report
            assert abs(report['length'] - 2 * math.pi * R) <= LENGTH_TOL, report['length']
            assert report['hidden_glyphs'] == 0, report
            same_subpaths(report['canvas_subpaths'], [ring])
            placed = polar(drawn(doc))
            assert placed, (kind, 't1 no ink')
            outside = in_ring(placed, *annulus)
            assert not outside, (kind, 't1 ink outside the annulus', outside[:8])
            measured = span(placed)
            assert abs(measured - LINE / R) <= SPAN_TOL * LINE / R, (kind, 't1 span', measured,
                                                                       LINE / R)
            mean_at_offset = mean_angle(placed)
            print(f'PASS {kind} t1: placed in one step, every pixel in the annulus, span '
                  f'{measured:.4f} rad against {LINE / R:.4f}', flush=True)

            # ---- T2 offset ------------------------------------------------------------------
            before = undo_count(doc)
            moved = call('edit_text_layer', document_id=doc, layer=layer,
                         on_path={'offset': OFFSET + DELTA})
            one_step(doc, before, f'{kind} t2')
            same_subpaths(moved['text']['on_path']['canvas_subpaths'], [ring])
            turned = polar(drawn(doc))
            mean_moved = mean_angle(turned)
            turn = wrap(mean_moved - mean_at_offset)
            assert abs(turn - DELTA / R) <= ANGLE_TOL, (kind, 't2 turn', turn, DELTA / R)
            print(f'PASS {kind} t2: offset + {DELTA:g} turns the ink by {turn:.4f} rad', flush=True)

            # ---- T3 alignment ---------------------------------------------------------------
            for alignment, fraction in (('center', 0.5), ('right', 1.0)):
                call('edit_text_layer', document_id=doc, layer=layer, alignment=alignment)
                shifted = wrap(mean_angle(polar(drawn(doc))) - mean_moved)
                want = -fraction * LINE / R
                assert abs(shifted - want) <= ANGLE_TOL, (kind, 't3', alignment, shifted, want)
            call('edit_text_layer', document_id=doc, layer=layer, alignment='left')
            print(f'PASS {kind} t3: center and right move the line by half and the whole advance',
                  flush=True)

            # ---- T4 side --------------------------------------------------------------------
            call('edit_text_layer', document_id=doc, layer=layer, on_path={'side': 'right'})
            inside = polar(drawn(doc))
            outside = in_ring(inside, R - ASCENT - 2, R + DESCENT + 2)
            assert not outside, (kind, 't4 ink outside the inner annulus', outside[:8])
            mean_radius = sum(r * a for r, _, a in inside) / sum(a for _, _, a in inside)
            assert mean_radius < R, (kind, 't4 mean radius', mean_radius)
            call('edit_text_layer', document_id=doc, layer=layer, on_path={'side': 'left'})
            print(f'PASS {kind} t4: side right runs inside the circle (mean radius '
                  f'{mean_radius:.1f})', flush=True)

            # ---- T5 underline ---------------------------------------------------------------
            call('edit_text_layer', document_id=doc, layer=layer, underline=True)
            lined = polar(drawn(doc))
            outside = in_ring(lined, *annulus)
            assert not outside, (kind, 't5 underline ink outside the annulus', outside[:8])
            assert len(lined) > len(turned), (kind, 't5 the underline added no ink')
            call('edit_text_layer', document_id=doc, layer=layer, underline=False)
            print(f'PASS {kind} t5: the underline bends with the line', flush=True)

            # ---- T6 remove ------------------------------------------------------------------
            # One text layer at a time, so the composite is that layer's ink in every kind.
            before = undo_count(doc)
            removed = call('edit_text_layer', document_id=doc, layer=layer, on_path=None)
            one_step(doc, before, f'{kind} t6')
            assert 'on_path' not in removed['text'], removed['text']
            off_text, off_render = rows(doc)[layer]['text'], render(doc)
            call('delete_layer', document_id=doc, layer=layer)
            plain = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                         wrap_width=0, **base)['layer']
            plain_text = rows(doc)[plain]['text']
            assert off_text == plain_text, (kind, off_text, plain_text)
            assert off_render == render(doc), (kind, 't6 render')
            call('delete_layer', document_id=doc, layer=plain)
            print(f'PASS {kind} t6: off the path equals a never-placed layer', flush=True)

            # ---- T7 reopen ------------------------------------------------------------------
            layer = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                         on_path={'subpaths': [ring], 'offset': OFFSET}, **base)['layer']
            copy = folder / f'{kind}.doctored'
            call('save_copy', document_id=doc, path=str(copy), format='doctored')
            reopened = call('open_document', path=str(copy))['document']['id']
            first, second = rows(doc), rows(reopened)
            assert set(first) == set(second), (kind, sorted(first), sorted(second))
            for index in first:
                assert first[index].get('text') == second[index].get('text'), (kind, index)
            assert render(doc) == render(reopened), (kind, 't7 composite')
            call('close_document', document_id=reopened, discard_changes=True)
            # Replace Missing Fonts keeps the path: the family patched to one this Mac lacks
            # (same UTF-8 length), then replaced back, renders and reads as before.
            data = copy.read_bytes()
            assert data.count(PATCH_FROM) >= 1, (kind, 't7 no family to patch')
            patched = folder / f'{kind}-missing.doctored'
            patched.write_bytes(data.replace(PATCH_FROM, PATCH_TO))
            missing = call('open_document', path=str(patched))['document']['id']
            assert rows(missing)[layer]['text'].get('font_missing') is True, (kind, 't7 not missing')
            call('replace_missing_fonts', document_id=missing,
                 replacements=[{'font': ABSENT, 'with': FONT}])
            replaced = rows(missing)[layer]['text']
            assert replaced == first[layer]['text'], (kind, 't7 replaced text', {
                key: (replaced.get(key), first[layer]['text'].get(key))
                for key in set(replaced) | set(first[layer]['text'])
                if replaced.get(key) != first[layer]['text'].get(key)})
            assert render(missing) == render(doc), (kind, 't7 replaced render')
            call('close_document', document_id=missing, discard_changes=True)
            call('delete_layer', document_id=doc, layer=layer)
            print(f'PASS {kind} t7: the saved copy reopens with an equal render and text, and '
                  'Replace Missing Fonts keeps the path', flush=True)

            # ---- T8 an open arc -------------------------------------------------------------
            quarter = circle(last=ARCS // 4, closed=False)
            arc = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                       on_path={'subpaths': [quarter]}, **base)
            length = arc['text']['on_path']['length']
            assert abs(length - math.pi / 2 * R) <= LENGTH_TOL, (kind, 't8 length', length)
            hidden = sum(1 for i in range(len(TEXT)) if (i + 0.5) * ADVANCE > length)
            assert hidden > 0 and arc['text']['on_path']['hidden_glyphs'] == hidden, \
                (kind, arc['text']['on_path'], hidden)
            ends = polar(drawn(doc))
            last = max(t for _, t, _ in ends)
            reach = math.pi / 2 + (ADVANCE / 2 + 2) / R
            assert last <= reach and last >= math.pi / 2 - ADVANCE / R, (kind, 't8 end', last, reach)
            assert min(t for _, t, _ in ends) >= -2 / R, (kind, 't8 start')
            call('delete_layer', document_id=doc, layer=arc['layer'])
            print(f'PASS {kind} t8: {hidden} glyphs hidden past the arc and no ink beyond it',
                  flush=True)

            # ---- T9 rotated -----------------------------------------------------------------
            a = math.radians(30)
            turned_layer = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                                transform=[math.cos(a), -math.sin(a), math.sin(a), math.cos(a)],
                                on_path={'subpaths': [ring], 'offset': OFFSET}, **base)
            same_subpaths(turned_layer['text']['on_path']['canvas_subpaths'], [ring])
            outside = in_ring(polar(drawn(doc)), *annulus)
            assert not outside, (kind, 't9 ink outside the annulus', outside[:8])
            call('delete_layer', document_id=doc, layer=turned_layer['layer'])
            print(f'PASS {kind} t9: a rotated layer reports the subpaths passed and inks the circle',
                  flush=True)

            # ---- T10 refusals ---------------------------------------------------------------
            before = undo_count(doc)
            add = dict(document_id=doc, x=ORIGIN[0], y=ORIGIN[1], font=FONT, size=SIZE)
            path = {'subpaths': [ring]}
            refusals = {
                'line_break': call('add_text_layer', fail=True, text='HAND\nPATH', on_path=path,
                                   **add),
                'wrap': call('add_text_layer', fail=True, text=TEXT, wrap_width=200, on_path=path,
                             **add),
                'emoji': call('add_text_layer', fail=True, text='Hi \U0001F600', on_path=path,
                              **add),
                'one_subpath': call('add_text_layer', fail=True, text=TEXT,
                                    on_path={'subpaths': [ring, quarter]}, **add),
                'past_end': call('add_text_layer', fail=True, text=TEXT,
                                 on_path={'subpaths': [ring], 'subpath': 5}, **add),
                'side': call('add_text_layer', fail=True, text=TEXT,
                             on_path={'subpaths': [ring], 'side': 'up'}, **add),
                'both': call('add_text_layer', fail=True, text=TEXT,
                             on_path={'subpaths': [ring], 'path_name': 'Work Path'}, **add),
            }
            plain = call('add_text_layer', document_id=doc, x=ORIGIN[0], y=ORIGIN[1],
                         wrap_width=0, **base)['layer']
            after_plain = undo_count(doc)
            refusals['none'] = call('edit_text_layer', fail=True, document_id=doc, layer=plain,
                                    on_path={'offset': 5})
            for name, text in refusals.items():
                sentence = SENTENCES['one_subpath' if name == 'past_end' else name]
                assert sentence in text, (kind, name, text)
            assert after_plain == before + 1, (kind, 't10 a refusal added a step', before,
                                                after_plain)
            assert undo_count(doc) == after_plain, (kind, 't10 the edit refusal added a step')
            receipts[f'{kind}_refusals'] = refusals
            print(f'PASS {kind} t10: every refusal names its reason and adds no step', flush=True)

            # ---- T11 wide glyph (RGB 8) -----------------------------------------------------
            if kind == 'rgb8':
                # A canvas that holds the widest candidate whole wherever it is placed.
                wide = call('new_document', width=WIDE_W, height=WIDE_H, background='white',
                            **KINDS[kind])['document']['id']
                receipts['wide'] = {}
                for group in WIDE_GROUPS:
                    chosen = None
                    for text, font in group:
                        straight = call('add_text_layer', document_id=wide, text=text, font=font,
                                        size=WIDE_SIZE, x=100, y=250, wrap_width=0,
                                        rgba=[0, 0, 0, 1])
                        box = ink_box(ink(wide, straight['layer']))
                        call('delete_layer', document_id=wide, layer=straight['layer'])
                        receipts[f'wide_{label(text)}'] = box
                        if box[0] > WIDE_FACTOR * WIDE_SIZE:
                            chosen = (text, font, box)
                            break
                    assert chosen, ('t11 no wide glyph qualifies', group, receipts)
                    text, font, box = chosen
                    # The top of this circle is the middle of a flattened chord: its anchors
                    # start an eighth of an arc early, and each arc flattens into four chords at
                    # 1/64 px.
                    level = circle(phase=-math.pi / ARCS / 4, centre=WIDE_CENTRE)
                    top = call('add_text_layer', document_id=wide, text=text, font=font,
                               size=WIDE_SIZE, rgba=[0, 0, 0, 1], alignment='center',
                               on_path={'subpaths': [level]})
                    length = top['text']['on_path']['length']
                    call('edit_text_layer', document_id=wide, layer=top['layer'],
                         on_path={'offset': length * (ARCS * 3 / 4 + 1 / 8) / ARCS})
                    at_top = ink_box(ink(wide, top['layer']))
                    line = {'closed': False, 'anchors': [list(WIDE_LINE[0]), list(WIDE_LINE[1])]}
                    end = call('add_text_layer', document_id=wide, text=text, font=font,
                               size=WIDE_SIZE, rgba=[0, 0, 0, 1], alignment='center',
                               on_path={'subpaths': [line],
                                        'offset': WIDE_LINE[1][0] - WIDE_LINE[0][0] - 1})
                    at_end = ink_box(ink(wide, end['layer']))
                    receipts['wide'][label(text)] = {'straight': box, 'top': at_top, 'end': at_end}
                call('close_document', document_id=wide, discard_changes=True)
                (output / 'receipts.json').write_text(
                    json.dumps(receipts, indent=2, sort_keys=True) + '\n')
                for glyph, got in receipts['wide'].items():
                    for name in ('top', 'end'):
                        assert all(abs(g - s) <= BOX_TOL
                                   for g, s in zip(got[name], got['straight'])), \
                            ('t11', glyph, name, got[name], got['straight'])
                print(f'PASS {kind} t11: each wide glyph keeps its straight ink box on the circle '
                      f'and at a line\'s end: {receipts["wide"]}', flush=True)

                # ---- T12 legacy reopen (RGB 8) ----------------------------------------------
                record = json.loads(LEGACY_RECORD.read_text())
                straight_zapfino = receipts.get('wide_Zapfino')
                assert straight_zapfino, ('t12 T11 measured no straight Zapfino box', receipts)
                legacy_copy = folder / 'legacy-zapfino.doctored'
                legacy_copy.write_bytes(LEGACY.read_bytes())
                legacy = call('open_document', path=str(legacy_copy))['document']['id']
                index = record['layer']

                def placement():
                    row = rows(legacy)[index]
                    bounds = {'x': row['offset_x'], 'y': row['offset_y'], 'width': row['width'],
                              'height': row['height']}
                    return bounds, row['text']['origin']

                bounds, origin = placement()
                reopened_box = ink_rect(ink(legacy, index))
                receipts['legacy'] = {'bounds': bounds, 'origin': origin, 'ink': reopened_box}
                assert bounds == record['bounds'], ('t12 bounds', bounds, record['bounds'])
                assert origin == record['origin'], ('t12 origin', origin, record['origin'])
                assert reopened_box == record['ink_box'], ('t12 ink', reopened_box,
                                                           record['ink_box'])
                stored = saved_payload(legacy, 'legacy-reopened.doctored')
                assert stored == record['payload'], ('t12 payload', stored, record['payload'])
                assert 'ink_outset' not in stored, ('t12 the reopen marked the layer', stored)
                # Its raster is its description's rendering, so a second copy's first transform
                # composes into the description; a rect read as grown would call the raster
                # stale and rasterize it (its swash reaches only right and below, so the
                # recovered origin alone cannot show that).
                twin_copy = folder / 'legacy-zapfino-twin.doctored'
                twin_copy.write_bytes(LEGACY.read_bytes())
                twin = call('open_document', path=str(twin_copy))['document']['id']
                turned = call('transform_layer', document_id=twin, layer=index, rotate=30)
                assert turned.get('rasterized') is False, ('t12 the saved raster read as stale',
                                                           turned)
                call('close_document', document_id=twin, discard_changes=True)
                # A transparency lock keeps a layer saved without the marker unmarked: a
                # colour-only edit keeps its rect (TI-8).
                call('set_layer_lock', document_id=legacy, layer=index, locks=['transparency'])
                call('edit_text_layer', document_id=legacy, layer=index, rgba=[1, 0, 0, 1])
                locked_bounds, locked_origin = placement()
                assert (locked_bounds, locked_origin) == (record['bounds'], record['origin']), \
                    ('t12 locked colour edit', locked_bounds, locked_origin)
                locked = saved_payload(legacy, 'legacy-locked.doctored')
                assert 'ink_outset' not in locked, ('t12 the locked edit marked the layer', locked)
                # Unlocked, the next re-render marks it at the anchor it had.
                call('set_layer_lock', document_id=legacy, layer=index, locks=[])
                call('edit_text_layer', document_id=legacy, layer=index, text='Zapfino')
                marked_bounds, marked_origin = placement()
                marked_box = ink_box(ink(legacy, index))
                receipts['legacy_marked'] = {'bounds': marked_bounds, 'origin': marked_origin,
                                             'ink': marked_box}
                assert marked_origin == record['origin'], ('t12 marked origin', marked_origin)
                assert all(abs(g - s) <= BOX_TOL for g, s in zip(marked_box, straight_zapfino)), \
                    ('t12 marked ink', marked_box, straight_zapfino)
                marked = saved_payload(legacy, 'legacy-marked.doctored')
                assert '"ink_outset"' in marked, ('t12 the edit left the layer unmarked', marked)
                call('close_document', document_id=legacy, discard_changes=True)
                (output / 'receipts.json').write_text(
                    json.dumps(receipts, indent=2, sort_keys=True) + '\n')
                print(f'PASS {kind} t12: the saved layer reopens at {origin} with its bytes, '
                      f'keeps its rect under a transparency lock, and an edit marks it with the '
                      f'{marked_box} ink box', flush=True)

            call('close_document', document_id=doc, discard_changes=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all ({", ".join(kinds)}); receipts in {output}', flush=True)


if __name__ == '__main__':
    try:
        main()
    except Exception as failure:
        print(f'FAIL: {type(failure).__name__}: {failure}', flush=True)
        raise
