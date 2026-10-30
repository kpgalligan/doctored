#!/usr/bin/env python3
"""The #1 phase-4a MCP drive: a CMYK document's layer STRUCTURE over MCP —
groups, blend modes, clipping masks, group masks, Fill Opacity, the merges and
Stamp Visible, the DRDC 84 round trip, the words a document that needs none of
it keeps, the refusals, and both mode conversions carrying the tree.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

The fixture is a 16-bit chunky CMYK TIFF written HERE in pure Python with
macOS's Generic CMYK Profile (always installed): the left half ink A, the right
half ink B. Ink codes are `percent / 100 × 65535` rounded half up; every
composite is read as `sample_color`'s `ink16`.

- **S1 group.** `new_layer` + `fill_selection ink` C over the whole canvas,
  then `group_layers` on that layer: `get_document` lists a group holding it,
  and the document is still CMYK.
- **S2 Multiply.** `set_layer_properties blend_mode Multiply` on the filled
  layer: at a probe on each half, ink16 is within 2/65535 per plate of this
  file's own `1 − (1 − a)(1 − b)` on `1 − ink` (a the backdrop's sampled ink,
  b ink C).
- **S3 clip, group mask, Fill.** `set_layer_clipped` (a new layer filled with
  ink D over a rectangle, clipped onto the group), `add_layer_mask
  from_selection` on the group, `set_layer_style {fill_opacity: 0.5}` on the
  grouped layer: each is ONE undo step (`list_history`'s undo_count), and each
  moves the composite at a probe it reaches (the clip by isolating the Pass
  Through group it sits on).
- **S4 merges.** `merge_down` (the clipped layer into the group),
  `merge_visible` and `stamp_visible`, each then `undo`: each is one undo
  step, its result layer is a CMYK leaf (`kind` layer, the document CMYK), and
  the composite is within 2/65535 per plate at every probe.
- **S5 .rz.** `save_copy rz`: bytes 4–12 hold the words 46 and 84; the reopened
  document has the same ink16 at every probe and the same tree (names, kinds,
  blend modes, clipping, Fill, masks).
- **S6 words.** A plain CMYK document saves word 82; one with a layer mask 83.
- **S7 refusals.** `set_layer_style` with Blend If on Red (Gray and the plates
  are admitted since the Blend If batch) and `set_layer_properties`
  with every mode CMYK does not offer (`NOT_OFFERED`) refuse by name (an enabled
  effect is admitted since the styles batch, #1: `verify_cmyk_styles.py`).
- **S8 conversions.** `convert_mode rgb` then `convert_mode cmyk` keeps the
  group, the Multiply mode and the clip.
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
W, H = 32, 20
INK_A = [10, 20, 30, 5]
INK_B = [60, 10, 40, 20]
INK_C = [30, 50, 10, 0]
INK_D = [0, 0, 0, 30]
CLIP_RECT = dict(x=4, y=4, width=12, height=8)
MASK_RECT = dict(x=0, y=0, width=24, height=20)
PROBES = [(4, 10), (10, 6), (20, 10), (28, 15)]
BOUND = 2
REFUSED = 'is unavailable in a CMYK document'
# The modes CMYK does not offer (take1's `menu`).
NOT_OFFERED = []


def codes(percent):
    # Half up, as the core's `(v · 65535).round()` rounds.
    return [int(p / 100 * 65535 + 0.5) for p in percent]


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


def fixture(path, profile):
    a, b = codes(INK_A), codes(INK_B)
    samples = []
    for _ in range(H):
        for x in range(W):
            samples += a if x < W // 2 else b
    path.write_bytes(cmyk_tiff(W, H, samples, profile))


def multiply(a16, b16):
    """`1 − (1 − a)(1 − b)` per plate on `1 − ink`, in codes."""
    return [int((1 - (1 - a / 65535) * (1 - b / 65535)) * 65535 + 0.5) for a, b in zip(a16, b16)]


def near(got, want, bound=BOUND):
    return all(abs(g - w) <= bound for g, w in zip(got, want))


def tree(call, doc):
    """The layers as (name, kind, blend mode, clipped, Fill, has mask, parent)."""
    rows = []
    for layer in call('get_document', document_id=doc)['layers']:
        style = layer.get('style') or {}
        rows.append((layer['name'], layer.get('kind'), layer.get('blend_mode'),
                     layer.get('clipped'), style.get('fill_opacity'), layer.get('has_mask'),
                     layer.get('parent')))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-structure-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_structure.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def ink(doc, x, y):
            sample = call('sample_color', document_id=doc, x=x, y=y)
            assert sample.get('color_mode') == 'cmyk', sample
            return sample['ink16']

        def composite(doc):
            return {p: ink(doc, *p) for p in PROBES}

        def undo_count(doc):
            return int(call('list_history', document_id=doc)['undo_count'])

        def layers(doc):
            return call('get_document', document_id=doc)['layers']

        def index_of(doc, name):
            return next(l['index'] for l in layers(doc) if l['name'] == name)

        def one_step(doc, tool, **arguments):
            before = undo_count(doc)
            reply = call(tool, document_id=doc, **arguments)
            assert undo_count(doc) == before + 1, (tool, before, undo_count(doc))
            return reply

        def is_cmyk(doc):
            return call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'

        source = folder / 'halves.tif'
        fixture(source, profile)
        doc = call('open_document', path=str(source))['document']['id']
        assert is_cmyk(doc)
        backdrop = composite(doc)
        for (x, y), got in backdrop.items():
            assert got == codes(INK_A if x < W // 2 else INK_B), ((x, y), got)

        # ---- S1: a group ------------------------------------------------------
        call('new_layer', document_id=doc, name='Ink')
        ink_layer = index_of(doc, 'Ink')
        call('select_all', document_id=doc)
        call('fill_selection', document_id=doc, ink=INK_C, layer=ink_layer)
        call('deselect', document_id=doc)
        grouped = call('group_layers', document_id=doc, layers=[ink_layer], name='Group')
        rows = layers(doc)
        group = next(l for l in rows if l['name'] == 'Group')
        assert group.get('kind') == 'group', rows
        assert next(l for l in rows if l['name'] == 'Ink').get('parent') == group['index'], rows
        assert is_cmyk(doc)
        receipts['s1'] = {'group': grouped.get('group'), 'tree': tree(call, doc)}
        print('PASS s1: group_layers groups a CMYK layer', flush=True)

        # ---- S2: Multiply -------------------------------------------------------
        one_step(doc, 'set_layer_properties', index=index_of(doc, 'Ink'), blend_mode='Multiply')
        multiplied = composite(doc)
        for p, got in multiplied.items():
            want = multiply(backdrop[p], codes(INK_C))
            assert near(got, want), (p, got, want)
        receipts['s2'] = {str(p): v for p, v in multiplied.items()}
        print('PASS s2: Multiply on ink is 1 − (1 − a)(1 − b) on 1 − ink', flush=True)

        # ---- S3: clip, group mask, Fill -----------------------------------------
        call('new_layer', document_id=doc, name='Clip')
        clip = index_of(doc, 'Clip')
        call('select_rect', document_id=doc, **CLIP_RECT)
        call('fill_selection', document_id=doc, ink=INK_D, layer=clip)
        call('deselect', document_id=doc)
        unclipped = composite(doc)
        one_step(doc, 'set_layer_clipped', layer=index_of(doc, 'Clip'), clipped=True)
        clipped = composite(doc)
        # The Pass Through group isolates once a clipped layer sits on it, so
        # Multiply inside it no longer reaches the backdrop.
        assert clipped != unclipped, 'the clip moved nothing'
        call('select_rect', document_id=doc, **MASK_RECT)
        one_step(doc, 'add_layer_mask', layer=index_of(doc, 'Group'), kind='from_selection')
        call('deselect', document_id=doc)
        masked = composite(doc)
        assert masked[(28, 15)] != clipped[(28, 15)], 'the group mask moved nothing'
        one_step(doc, 'set_layer_style', layer=index_of(doc, 'Ink'), style={'fill_opacity': 0.5})
        filled = composite(doc)
        assert filled[(4, 10)] != masked[(4, 10)], 'Fill moved nothing'
        receipts['s3'] = {'clipped': {str(p): v for p, v in clipped.items()},
                          'masked': {str(p): v for p, v in masked.items()},
                          'filled': {str(p): v for p, v in filled.items()},
                          'tree': tree(call, doc)}
        print('PASS s3: clip, group mask and Fill are one undo step each and move the composite',
              flush=True)

        # ---- S4: merges, each undone ----------------------------------------------
        merges = {}
        for tool, arguments in [('merge_down', dict(layer=index_of(doc, 'Clip'))),
                                ('merge_visible', {}),
                                ('stamp_visible', dict(name='Stamp'))]:
            reply = one_step(doc, tool, **arguments)
            assert is_cmyk(doc)
            landed = layers(doc)[reply['layer']] if 'layer' in reply else None
            if tool == 'stamp_visible':
                landed = next(l for l in layers(doc) if l['name'] == 'Stamp')
            assert landed is not None and landed.get('kind') == 'layer', (tool, reply, layers(doc))
            after = composite(doc)
            for p in PROBES:
                assert near(after[p], filled[p]), (tool, p, after[p], filled[p])
            merges[tool] = {'layer': landed['name'], 'layer_count': len(layers(doc))}
            call('undo', document_id=doc)
            assert composite(doc) == filled, f'{tool}: undo restores'
        receipts['s4'] = merges
        print('PASS s4: merge_down, merge_visible and stamp_visible keep the composite', flush=True)

        # ---- S5: .rz word 84 ---------------------------------------------------------
        rz = folder / 'structured.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        raw = rz.read_bytes()
        words = struct.unpack('<II', raw[4:12])
        assert words == (46, 84), words
        again = call('open_document', path=str(rz))['document']['id']
        assert is_cmyk(again)
        assert composite(again) == filled, (composite(again), filled)
        assert tree(call, again) == tree(call, doc), (tree(call, again), tree(call, doc))
        receipts['s5'] = {'words': list(words), 'tree': tree(call, again)}
        print('PASS s5: the .rz is word 84 and reopens bit-exact with its tree', flush=True)

        # ---- S6: words 82 and 83 -------------------------------------------------------
        # A second file: opening the first again would answer the open document.
        plain_tif = folder / 'plain.tif'
        fixture(plain_tif, profile)
        plain = call('open_document', path=str(plain_tif))['document']['id']
        assert plain != doc, plain
        plain_rz = folder / 'plain.rz'
        call('save_copy', document_id=plain, path=str(plain_rz), format='rz')
        plain_word = struct.unpack('<I', plain_rz.read_bytes()[8:12])[0]
        assert plain_word == 82, plain_word
        call('add_layer_mask', document_id=plain, layer=0, kind='hide_all')
        masked_rz = folder / 'masked.rz'
        call('save_copy', document_id=plain, path=str(masked_rz), format='rz')
        masked_word = struct.unpack('<I', masked_rz.read_bytes()[8:12])[0]
        assert masked_word == 83, masked_word
        call('close_document', document_id=plain, discard_changes=True)
        receipts['s6'] = {'plain': plain_word, 'masked': masked_word}
        print('PASS s6: plain and masked documents keep words 82 and 83', flush=True)

        # ---- S7: refusals ----------------------------------------------------------------
        target = index_of(doc, 'Ink')
        refusals = {
            'blend_if': call('set_layer_style', fail=True, document_id=doc, layer=target,
                             style={'blend_if': {'channel': 'red',
                                                 'this_layer': [10, 20, 240, 255]}}),
        }
        for mode in NOT_OFFERED:
            refusals[mode] = call('set_layer_properties', fail=True, document_id=doc,
                                  index=target, blend_mode=mode)
        assert 'Blend If on the Red channel ' + REFUSED in refusals['blend_if'], refusals
        for mode in NOT_OFFERED:
            assert 'CMYK layer blending supports' in refusals[mode], refusals
        receipts['s7'] = refusals
        print('PASS s7: Blend If on Red and the modes CMYK does not offer refuse by name',
              flush=True)

        # ---- S8: both conversions carry the tree ----------------------------------------------
        shape = [(n, k, m, c) for n, k, m, c, _, _, _ in tree(call, again)]
        rgb = call('convert_mode', document_id=again, mode='rgb')
        assert rgb['color_mode'] == 'rgb', rgb
        assert [(n, k, m, c) for n, k, m, c, _, _, _ in tree(call, again)] == shape
        back = call('convert_mode', document_id=again, mode='cmyk')
        assert back['color_mode'] == 'cmyk', back
        carried = [(n, k, m, c) for n, k, m, c, _, _, _ in tree(call, again)]
        assert carried == shape, (carried, shape)
        receipts['s8'] = {'tree': carried}
        print('PASS s8: RGB Color and CMYK Color carry the group, the mode and the clip',
              flush=True)
        call('close_document', document_id=again, discard_changes=True)
        call('close_document', document_id=doc, discard_changes=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
