#!/usr/bin/env python3
"""The #547 phase-4b MCP drive: every geometry tool on a CMYK document over
MCP — Image Size, Canvas Size, Crop, rotation and flips, Trim, Reveal All,
Free Transform on layers, sets and masks, Straighten, Perspective Crop, Warp,
Puppet Warp, Perspective Warp, Liquify, the Move tool, align and distribute,
the selected pixels, a structured document through geometry and its DRDC 84
round trip, and the refusals CMYK mode keeps.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

The fixture is a 16-bit chunky CMYK TIFF written HERE in pure Python with
macOS's Generic CMYK Profile: 96 × 64, the left half ink L, the right half ink
R, inside a 4-px border of paper (no ink). Ink codes are `percent / 100 ×
65535` rounded half up; every composite is read as `sample_color`'s `ink16`
and `alpha16`. Each gate's edits are undone before the next gate.

- **G1 canvas ops.** `image_size` 1.5× bicubic, `canvas_size`, `crop`,
  `rotate` 90/180/270 and `flip`: the new dimensions; inside each flat half the
  ink is unchanged within 1 code; the quarter turns and flips are exact
  permutations at mapped probes (border and halves).
- **G2 Trim and Reveal All.** `trim top_left` keeps exactly the 88 × 56 inner
  box. A new layer holding ink in a rect, moved so the rect crosses the right
  edge by 10 px, then `reveal_canvas`: the canvas grows to the rect's content
  box (106 wide), not to the layer's stored extent.
- **G3 transforms.** `transform_layer` 30° bilinear, `transform_layers` on two
  layers, `transform_mask`, `straighten_layers`, `perspective_crop`: one undo
  step each, the document still CMYK; after the 30° turn the interior flat ink
  is unchanged within 1 code and a canvas corner the turned layer leaves is
  alpha 0.
- **G4 warps.** `warp_layer` (then `get_warp` answers field null),
  `puppet_warp`, `perspective_warp`, `liquify`: one undo step each, `changed`
  true where reported, `storage` `cmyka16_unorm` where reported.
- **G5 Move and align.** `move_layers` (and `duplicate`), `align_layers` to the
  canvas, `distribute_layers`: exact content offsets.
- **G6 selected pixels.** `select_rect` + `move_selected_pixels` by an integer
  offset: the landed ink equals the source ink exactly, the vacated part of the
  selection is alpha 0; `transform_selected_pixels` is one undo step.
- **G7 structure.** A group holding a layer, a clipped layer, a group mask and
  Fill 0.5, through `image_size`, `crop` and `transform_layers` on the group:
  `save_copy rz` writes the words 46, 84; the reopened document has the same
  ink16 at every probe and the same tree.
- **G8 refusals.** `transform_selected_pixels target red` refuses with "is
  unavailable in a CMYK document" (`auto_align_layers` runs on CMYK,
  `verify_cmyk_mode.py` G9).
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H, BORDER = 96, 64, 4
INK_L = [20, 40, 10, 5]
INK_R = [55, 15, 35, 25]
INK_M = [0, 60, 80, 0]
FLAT_BOUND = 1
STRUCTURE_PROBES = [(10, 10), (30, 25), (60, 30), (80, 50)]
REFUSED = 'is unavailable in a CMYK document'
STORAGE = 'cmyka16_unorm'


def codes(percent):
    # Half up, as the core's `(v · 65535).round()` rounds.
    return [int(p / 100 * 65535 + 0.5) for p in percent]


PAPER = [0, 0, 0, 0]


def cmyk_tiff(width, height, samples, icc):
    """A little-endian classic chunky 16-bit CMYK TIFF (Photometric 5)."""
    data = struct.pack(f'<{len(samples)}H', *samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<4H', 16, 16, 16, 16))
    icc_at = place(icc)
    if len(body) % 2:
        body.append(0)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 4, struct.pack('<I', bits_at)), (259, 3, 1, short(1)),
        (262, 3, 1, short(5)), (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(4)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))), (284, 3, 1, short(1)),
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def ink_at(x, y):
    if x < BORDER or y < BORDER or x >= W - BORDER or y >= H - BORDER:
        return PAPER
    return codes(INK_L if x < W // 2 else INK_R)


def fixture(path, profile):
    samples = []
    for y in range(H):
        for x in range(W):
            samples += ink_at(x, y)
    path.write_bytes(cmyk_tiff(W, H, samples, profile))


def near(got, want, bound=FLAT_BOUND):
    return all(abs(g - w) <= bound for g, w in zip(got, want))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-geometry-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_geometry.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def sample(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16'], reply['alpha16']

        def ink(doc, x, y):
            return sample(doc, x, y)[0]

        def undo_count(doc):
            return int(call('list_history', document_id=doc)['undo_count'])

        def layers(doc):
            return call('get_document', document_id=doc)['layers']

        def size(doc):
            info = call('get_document', document_id=doc)
            return info['width'], info['height']

        def is_cmyk(doc):
            return call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'

        def one_step(doc, tool, **arguments):
            before = undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == before + 1, (tool, before, undo_count(doc), reply)
            assert is_cmyk(doc), tool
            return reply

        def undo(doc, steps=1):
            for _ in range(steps):
                call('undo', document_id=doc)

        def undo_to(doc, count):
            while undo_count(doc) > count:
                call('undo', document_id=doc)

        def index_of(doc, name):
            return next(l['index'] for l in layers(doc) if l['name'] == name)

        def content(doc, name):
            row = next(l for l in layers(doc) if l['name'] == name)
            return row['content_x'], row['content_y'], row['content_width'], row['content_height']

        source = folder / 'halves.tif'
        fixture(source, profile)
        doc = call('open_document', path=str(source))['document']['id']
        assert is_cmyk(doc)
        for x, y in [(1, 1), (10, 20), (70, 40), (W - 2, H - 2)]:
            assert ink(doc, x, y) == ink_at(x, y), ((x, y), ink(doc, x, y))
        base_count = undo_count(doc)

        # ---- G1: canvas ops ------------------------------------------------------
        g1 = {}
        one_step(doc, 'image_size', width=144, height=96, filter='catmull-rom')
        assert size(doc) == (144, 96), size(doc)
        for (x, y), want in [((30, 48), codes(INK_L)), ((110, 48), codes(INK_R))]:
            got = ink(doc, x, y)
            assert near(got, want), ('image_size', (x, y), got, want)
        g1['image_size'] = size(doc)
        undo(doc)
        one_step(doc, 'canvas_size', width=110, height=80, anchor='center')
        assert size(doc) == (110, 80), size(doc)
        assert ink(doc, 7 + 20, 8 + 20) == codes(INK_L), ink(doc, 27, 28)
        assert sample(doc, 2, 2)[1] == 0, 'canvas_size: the added canvas is transparent'
        undo(doc)
        one_step(doc, 'crop', x=BORDER, y=BORDER, width=W - 2 * BORDER, height=H - 2 * BORDER)
        assert size(doc) == (W - 2 * BORDER, H - 2 * BORDER), size(doc)
        assert ink(doc, 0, 0) == codes(INK_L), ink(doc, 0, 0)
        undo(doc)
        probes = [(2, 2), (3, 10), (4, 10), (47, 30), (48, 30), (93, 61), (60, 3)]
        before = {p: ink(doc, *p) for p in probes}
        cases = {
            ('rotate', 90): (lambda x, y: (H - 1 - y, x), (H, W)),
            ('rotate', 180): (lambda x, y: (W - 1 - x, H - 1 - y), (W, H)),
            ('rotate', 270): (lambda x, y: (y, W - 1 - x), (H, W)),
            ('flip', 'horizontal'): (lambda x, y: (W - 1 - x, y), (W, H)),
            ('flip', 'vertical'): (lambda x, y: (x, H - 1 - y), (W, H)),
        }
        for (tool, value), (mapped, dims) in cases.items():
            arguments = {'degrees': value} if tool == 'rotate' else {'axis': value}
            one_step(doc, tool, **arguments)
            assert size(doc) == dims, (tool, value, size(doc))
            for p in probes:
                assert ink(doc, *mapped(*p)) == before[p], (tool, value, p)
            undo(doc)
        g1['permutations'] = [f'{t} {v}' for t, v in cases]
        assert undo_count(doc) == base_count
        receipts['g1'] = g1
        print('PASS g1: image_size, canvas_size, crop, rotations and flips on ink', flush=True)

        # ---- G2: Trim and Reveal All -----------------------------------------------
        one_step(doc, 'trim', based_on='top_left', top=True, bottom=True, left=True, right=True)
        assert size(doc) == (W - 2 * BORDER, H - 2 * BORDER), size(doc)
        undo(doc)
        call('new_layer', document_id=doc, name='Rect')
        rect = index_of(doc, 'Rect')
        call('select_rect', document_id=doc, x=60, y=20, width=30, height=20)
        call('fill_selection', document_id=doc, ink=INK_M, layer=rect)
        call('deselect', document_id=doc)
        assert content(doc, 'Rect') == (60, 20, 30, 20), content(doc, 'Rect')
        one_step(doc, 'move_layers', layers=[rect], dx=16, dy=0)
        assert content(doc, 'Rect') == (76, 20, 30, 20), content(doc, 'Rect')
        one_step(doc, 'reveal_canvas')
        assert size(doc) == (106, 64), ('reveal_canvas grows to the content box', size(doc))
        receipts['g2'] = {'trim': [W - 2 * BORDER, H - 2 * BORDER], 'reveal': list(size(doc))}
        undo(doc, 2)
        with_rect = undo_count(doc)
        print('PASS g2: trim top_left keeps the inner box; reveal_canvas reads the content box',
              flush=True)

        # ---- G3: transforms ----------------------------------------------------------
        one_step(doc, 'transform_layer', layer=0, rotate=30, sampler='bilinear')
        # Probes clear of the Rect layer whose turned-back source stays inside one flat half.
        for (x, y), want in [((30, 32), codes(INK_L)), ((66, 50), codes(INK_R))]:
            got = ink(doc, x, y)
            assert near(got, want), ('transform_layer', (x, y), got, want)
        assert sample(doc, 0, 0)[1] == 0, 'the turned layer leaves the corner transparent'
        undo(doc)
        one_step(doc, 'transform_layers', layers=[0, rect], matrix=[0.9, 0.1, -0.1, 0.9, 6, 2],
                 sampler='bicubic')
        undo(doc)
        call('select_rect', document_id=doc, x=10, y=10, width=40, height=30)
        call('add_layer_mask', document_id=doc, layer=0, kind='from_selection')
        call('deselect', document_id=doc)
        one_step(doc, 'transform_mask', layer=0, matrix=[0.96, 0.26, -0.26, 0.96, 4, -3],
                 filter='bilinear')
        undo_to(doc, with_rect)
        one_step(doc, 'straighten_layers', layers=[0], x0=10, y0=10, x1=80, y1=16,
                 sampler='bicubic')
        undo(doc)
        one_step(doc, 'perspective_crop', corners=[[6, 4], [90, 8], [86, 60], [8, 58]],
                 width=80, height=50, sampler='bilinear')
        assert size(doc) == (80, 50), size(doc)
        undo(doc)
        receipts['g3'] = 'transform_layer, transform_layers, transform_mask, straighten_layers, ' \
                         'perspective_crop'
        print('PASS g3: transforms are one step each and keep the ink', flush=True)

        # ---- G4: warps -----------------------------------------------------------------
        warps = {}
        displacements = [[0, 0]] * 9
        displacements = [list(d) for d in displacements]
        displacements[4] = [3, 2]
        reply = one_step(doc, 'warp_layer', layer=0, operation='set', target='pixels', columns=3,
                         rows=3, displacements=displacements, filter='bicubic')
        warps['warp_layer'] = reply
        field = call('get_warp', document_id=doc, layer=0)
        assert field.get('field') is None, field
        undo(doc)
        for tool, arguments in [
            ('puppet_warp', dict(layer=0, pins=[{'from': [30, 30], 'to': [33, 31]},
                                                {'from': [70, 30], 'to': [70, 30]}])),
            ('perspective_warp', dict(layer=0, quads=[{'from': [10, 10, 86, 10, 86, 54, 10, 54],
                                                       'to': [12, 12, 84, 8, 86, 56, 8, 52]}])),
            ('liquify', dict(layer=0, strokes=[{'tool': 'forward_warp', 'size': 30,
                                                'points': [[40, 30], [52, 32]]}])),
        ]:
            reply = one_step(doc, tool, **arguments)
            if 'changed' in reply:
                assert reply['changed'] is True, (tool, reply)
            if 'storage' in reply:
                assert reply['storage'] == STORAGE, (tool, reply)
            warps[tool] = {k: reply.get(k) for k in ('changed', 'storage')}
            undo(doc)
        assert undo_count(doc) == with_rect, undo_count(doc)
        receipts['g4'] = warps
        print('PASS g4: warp_layer (no field), puppet_warp, perspective_warp and liquify', flush=True)

        # ---- G5: Move and align ----------------------------------------------------------
        one_step(doc, 'move_layers', layers=[rect], dx=-7, dy=5)
        assert content(doc, 'Rect') == (53, 25, 30, 20), content(doc, 'Rect')
        count = len(layers(doc))
        one_step(doc, 'move_layers', layers=[index_of(doc, 'Rect')], dx=20, dy=-12, duplicate=True)
        assert len(layers(doc)) == count + 1, layers(doc)
        moved = sorted((l['content_x'], l['content_y'], l['content_width'], l['content_height'])
                       for l in layers(doc) if l['name'].startswith('Rect'))
        assert moved == [(53, 25, 30, 20), (73, 13, 30, 20)], moved
        call('new_layer', document_id=doc, name='Third')
        third = index_of(doc, 'Third')
        call('select_rect', document_id=doc, x=5, y=40, width=10, height=10)
        call('fill_selection', document_id=doc, ink=INK_M, layer=third)
        call('deselect', document_id=doc)
        trio = [l['index'] for l in layers(doc) if l['name'].startswith('Rect') or l['name'] == 'Third']
        # Distribute equalizes the gaps: lefts 5, 53, 73 over 5…103 with widths 10, 30, 30
        # leave 28 px of gap, 14 a side, so the middle box's left lands on 29.
        one_step(doc, 'distribute_layers', layers=trio, axis='horizontal')
        lefts = sorted(l['content_x'] for l in layers(doc) if l['index'] in trio)
        assert lefts == [5, 29, 73], lefts
        one_step(doc, 'align_layers', layers=trio, edge='left', to='canvas')
        assert all(l['content_x'] == 0 for l in layers(doc) if l['index'] in trio), layers(doc)
        receipts['g5'] = {'moved': moved, 'distributed': lefts}
        undo_to(doc, with_rect)
        print('PASS g5: move_layers, its duplicate, distribute_layers and align_layers', flush=True)

        # ---- G6: selected pixels ------------------------------------------------------------
        call('set_active_layer', document_id=doc, index=0)
        call('select_rect', document_id=doc, x=10, y=10, width=12, height=8)
        source_ink = {(x, y): ink(doc, x, y) for x in (10, 15, 21) for y in (10, 17)}
        # It lands at (30, 45), clear of the Rect layer above.
        one_step(doc, 'move_selected_pixels', layer=0, dx=20, dy=35)
        for (x, y), want in source_ink.items():
            assert ink(doc, x + 20, y + 35) == want, ((x, y), ink(doc, x + 20, y + 35), want)
        assert sample(doc, 12, 12)[1] == 0, 'the vacated selection is transparent'
        undo(doc)
        call('select_rect', document_id=doc, x=10, y=10, width=12, height=8)
        one_step(doc, 'transform_selected_pixels', layer=0, rotate=15, sampler='bilinear')
        undo(doc)
        receipts['g6'] = {str(k): v for k, v in source_ink.items()}
        print('PASS g6: move_selected_pixels copies ink exactly; transform_selected_pixels', flush=True)

        # ---- G8: refusals (before G7 changes the document) ----------------------------------
        refusals = {
            'red': call('transform_selected_pixels', fail=True, document_id=doc, layer=0,
                        rotate=5, target='red'),
        }
        for name, text in refusals.items():
            assert REFUSED in text, (name, text)
        call('deselect', document_id=doc)
        receipts['g8'] = refusals

        # ---- G7: structure through geometry, .rz 84 ------------------------------------------
        call('new_layer', document_id=doc, name='Clip')
        clip = index_of(doc, 'Clip')
        call('select_rect', document_id=doc, x=50, y=15, width=20, height=30)
        call('fill_selection', document_id=doc, ink=[0, 0, 0, 40], layer=clip)
        call('deselect', document_id=doc)
        group = call('group_layers', document_id=doc, layers=[index_of(doc, 'Rect')], name='Group')
        call('set_layer_clipped', document_id=doc, layer=index_of(doc, 'Clip'), clipped=True)
        call('select_rect', document_id=doc, x=0, y=0, width=70, height=64)
        call('add_layer_mask', document_id=doc, layer=index_of(doc, 'Group'), kind='from_selection')
        call('deselect', document_id=doc)
        call('set_layer_style', document_id=doc, layer=index_of(doc, 'Rect'),
             style={'fill_opacity': 0.5})
        one_step(doc, 'image_size', width=120, height=80, filter='bilinear')
        one_step(doc, 'crop', x=4, y=4, width=110, height=70)
        one_step(doc, 'transform_layers', layers=[index_of(doc, 'Group')],
                 matrix=[0.95, 0.05, -0.05, 0.95, 3, 1], sampler='bilinear')

        def tree(d):
            return [(l['name'], l.get('kind'), l.get('blend_mode'), l.get('clipped'),
                     (l.get('style') or {}).get('fill_opacity'), l.get('has_mask'),
                     l.get('parent')) for l in layers(d)]

        rz = folder / 'structured.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        words = struct.unpack('<II', rz.read_bytes()[4:12])
        assert words == (46, 84), words
        again = call('open_document', path=str(rz))['document']['id']
        assert is_cmyk(again)
        for p in STRUCTURE_PROBES:
            assert sample(again, *p) == sample(doc, *p), (p, sample(again, *p), sample(doc, *p))
        assert tree(again) == tree(doc), (tree(again), tree(doc))
        receipts['g7'] = {'words': list(words), 'tree': tree(again), 'group': group.get('group')}
        print('PASS g7: a structured document through geometry saves word 84 and reopens', flush=True)
        print('PASS g8: transform_selected_pixels target red refuses by name', flush=True)
        call('close_document', document_id=again, discard_changes=True)
        call('close_document', document_id=doc, discard_changes=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True,
                                                    default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
