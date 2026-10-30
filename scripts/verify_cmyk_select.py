#!/usr/bin/env python3
"""The #1 selection-batch MCP drive: on a 16-bit Generic CMYK document the
Magic Wand (on ink) and the Paint Bucket's All Layers, Color Range, Select
Subject, Select and Mask to a mask and to a new layer, Edit ▸ Stroke… in ink,
Apply Layer Mask, and the `.rz` round trip.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python. The CMYK profile is macOS's
Generic CMYK Profile. Ink codes are `percent / 100 × 65535` rounded half up.
"Selected" is read the one way MCP can: `fill_selection` with ink
[0, 0, 0, 100] on a new layer, then `sample_color`'s composite ink16 at a point
(K 65535 = selected, the fixture's own ink = not), then `undo` twice.

Fixture `split.tif` (40 x 24): the left half ink A [60, 10, 10, 0], the right
half ink B [10, 60, 10, 0], and a 4 x 4 patch of A at (30, 10) inside B.

- **S1 wand.** `select_magic_wand` at (5, 5), tolerance 10, contiguous: the
  reply's bounds are (0, 0, 20, 24) and (5, 5) is selected, (25, 5) and
  (31, 11) are not. Contiguous false: bounds (0, 0, 34, 24) and (31, 11) is
  selected.
- **S2 bucket All Layers.** On a new layer, `fill` at (5, 5) with ink
  [0, 0, 0, 100], tolerance 10, sample_all_layers true (contiguous, the
  default): (5, 5) reads [0, 0, 0, 65535] and (31, 11) and (25, 5) keep their
  ink exactly.
- **S3 Color Range.** `select_color_range` sampled with `samples16` = the view
  code `sample_color` reports at (5, 5) (`center_rgba16`), tolerance 5,
  softness 0: (5, 5) and (31, 11) are selected and (25, 5) is not.
- **S4 Select Subject.** On `subject.tif` (64 x 64: paper with a disc of
  [10, 90, 80, 10] radius 18 at the centre) `select_subject` is answered by
  the subject path, never the CMYK gate: either a selection with (32, 32)
  selected, or an in-band refusal whose text does not contain "unavailable in
  a CMYK document".
- **S5 Select and Mask to a mask.** `select_rect` (4, 4, 14, 16), then
  `refine_selection` feather 2, output mask, layer 0: layer 0 has a mask; the
  composite's alpha16 is 65535 at (10, 12) and 0 at (35, 20).
- **S6 Select and Mask to a new layer.** After `undo`, the same rectangle and
  `refine_selection` feather 2, output new_layer: the reply names the new
  layer; it is the top layer, every other layer is hidden; (10, 12) reads the
  ink it read before exactly with alpha16 65535 and (35, 20) reads alpha16 0.
  With output new_layer_with_mask and decontaminate true the reply says
  decontaminated true and the top layer keeps a mask.
- **S7 Stroke.** On `split.tif` again: a new layer, `select_rect`
  (10, 6, 12, 10), `fill_selection` ink [0, 0, 100, 0], `deselect`, then
  `stroke_selection` on that layer, width 2, location outside, ink
  [0, 0, 0, 100]: (9, 10) reads [0, 0, 0, 65535] exactly, (15, 10) keeps
  [0, 0, 65535, 0] exactly, (4, 4) keeps A exactly; the reply's storage is
  "cmyka16_unorm".
- **S8 Apply Mask.** `select_rect` (10, 6, 6, 10), `add_layer_mask`
  from_selection on that layer, `remove_layer_mask` apply true: the reply's
  storage is "cmyka16_unorm", `get_mask_properties` refuses (no mask), (12, 10)
  keeps [0, 0, 65535, 0] and (19, 10) now reads the base layer's ink B
  exactly.
- **S9 .rz.** `save_copy rz` reopens as CMYK with the same layer count and the
  same `sample_color` ink16 and alpha16 at every probed point (bit-exact).
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
W, H = 40, 24
INK_A = [60, 10, 10, 0]
INK_B = [10, 60, 10, 0]
PATCH = (30, 10, 4, 4)
REFUSED = 'unavailable in a CMYK document'
BLACK = [0, 0, 0, 65535]


def codes(percent):
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


def split_samples():
    px, py, pw, ph = PATCH
    out = []
    for y in range(H):
        for x in range(W):
            inside = px <= x < px + pw and py <= y < py + ph
            out += codes(INK_A if x < W // 2 or inside else INK_B)
    return out


def subject_samples(size=64):
    out = []
    for y in range(size):
        for x in range(size):
            disc = (x - 32) ** 2 + (y - 32) ** 2 <= 18 ** 2
            out += codes([10, 90, 80, 10] if disc else [0, 0, 0, 0])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-select-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_select.py before any measurement'}
    split = folder / 'split.tif'
    split.write_bytes(cmyk_tiff(W, H, split_samples(), profile))
    subject = folder / 'subject.tif'
    subject.write_bytes(cmyk_tiff(64, 64, subject_samples(), profile))

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def attempt(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return bool(result.get('isError')), text

        def sample(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16'] + [reply['alpha16']]

        def ink(doc, x, y):
            return sample(doc, x, y)[:4]

        def selected(doc, points):
            """Which of `points` the current selection covers (module doc)."""
            call('new_layer', document_id=doc, name='Probe')
            top = len(call('get_document', document_id=doc)['layers']) - 1
            call('fill_selection', document_id=doc, layer=top, ink=[0, 0, 0, 100])
            out = [ink(doc, *p) == BLACK for p in points]
            call('undo', document_id=doc)
            call('undo', document_id=doc)
            return out

        doc = call('open_document', path=str(split))['document']['id']
        assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
        a, b = codes(INK_A), codes(INK_B)

        # ---- S1: the wand on ink -------------------------------------------------
        reply = call('select_magic_wand', document_id=doc, x=5, y=5, tolerance=10, contiguous=True)
        assert reply.get('bounds') == {'x': 0, 'y': 0, 'width': 20, 'height': 24}, reply
        assert selected(doc, [(5, 5), (25, 5), (31, 11)]) == [True, False, False]
        reply = call('select_magic_wand', document_id=doc, x=5, y=5, tolerance=10, contiguous=False)
        assert reply.get('bounds') == {'x': 0, 'y': 0, 'width': 34, 'height': 24}, reply
        assert selected(doc, [(5, 5), (25, 5), (31, 11)]) == [True, False, True]
        call('deselect', document_id=doc)
        receipts['s1'] = reply
        print('PASS s1: the Magic Wand selects by ink, contiguous and global', flush=True)

        # ---- S2: the Paint Bucket's All Layers ---------------------------------------
        call('new_layer', document_id=doc, name='Bucket')
        top = len(call('get_document', document_id=doc)['layers']) - 1
        reply = call('fill', document_id=doc, layer=top, x=5, y=5, ink=[0, 0, 0, 100],
                     tolerance=10, sample_all_layers=True)
        assert reply.get('sample_all_layers') is True and reply.get('changed') is True, reply
        assert ink(doc, 5, 5) == BLACK, ink(doc, 5, 5)
        assert ink(doc, 31, 11) == a and ink(doc, 25, 5) == b, (ink(doc, 31, 11), ink(doc, 25, 5))
        call('undo', document_id=doc)
        call('undo', document_id=doc)
        receipts['s2'] = reply
        print('PASS s2: the Paint Bucket fills the composite\'s ink region on a new layer', flush=True)

        # ---- S3: Color Range on the view ----------------------------------------------
        view = call('sample_color', document_id=doc, x=5, y=5)['center_rgba16'][:3]
        reply = call('select_color_range', document_id=doc, range='sampled', samples16=[view],
                     tolerance=5, softness=0)
        assert selected(doc, [(5, 5), (25, 5), (31, 11)]) == [True, False, True]
        call('deselect', document_id=doc)
        receipts['s3'] = {'sample': view, 'reply': reply}
        print('PASS s3: Color Range selects the sampled view colour', flush=True)

        # ---- S4: Select Subject -------------------------------------------------------
        subject_doc = call('open_document', path=str(subject))['document']['id']
        refused, text = attempt('select_subject', document_id=subject_doc)
        assert REFUSED not in text, text
        if not refused:
            assert selected(subject_doc, [(32, 32)]) == [True], 'the disc is the subject'
        receipts['s4'] = {'refused': refused, 'reply': text[:400]}
        call('close_document', document_id=subject_doc, discard_changes=True)
        print(f'PASS s4: Select Subject runs on the view ({"no subject" if refused else "a subject"})',
              flush=True)

        # ---- S5: Select and Mask to a mask --------------------------------------------
        call('select_rect', document_id=doc, x=4, y=4, width=14, height=16)
        reply = call('refine_selection', document_id=doc, feather=2, output='mask', layer=0)
        call('get_mask_properties', document_id=doc, layer=0)
        assert sample(doc, 10, 12)[4] == 65535 and sample(doc, 35, 20)[4] == 0, \
            (sample(doc, 10, 12), sample(doc, 35, 20))
        receipts['s5'] = reply
        call('undo', document_id=doc)
        print('PASS s5: Select and Mask writes a layer mask', flush=True)

        # ---- S6: Select and Mask to a new layer ---------------------------------------
        before = sample(doc, 10, 12)
        call('select_rect', document_id=doc, x=4, y=4, width=14, height=16)
        reply = call('refine_selection', document_id=doc, feather=2, output='new_layer')
        layers = call('get_document', document_id=doc)['layers']
        assert reply.get('new_layer') == len(layers) - 1, (reply, layers)
        assert all(not l.get('visible', True) for l in layers[:-1]), layers
        assert sample(doc, 10, 12) == before, (sample(doc, 10, 12), before)
        assert sample(doc, 35, 20)[4] == 0, sample(doc, 35, 20)
        call('undo', document_id=doc)
        call('select_rect', document_id=doc, x=4, y=4, width=14, height=16)
        masked = call('refine_selection', document_id=doc, feather=2, output='new_layer_with_mask',
                      decontaminate=True)
        assert masked.get('decontaminated') is True, masked
        top = len(call('get_document', document_id=doc)['layers']) - 1
        call('get_mask_properties', document_id=doc, layer=top)
        call('undo', document_id=doc)
        call('deselect', document_id=doc)
        receipts['s6'] = {'new_layer': reply, 'with_mask': masked}
        print('PASS s6: Select and Mask makes a new CMYK layer, decontaminated on request', flush=True)

        # ---- S7: Stroke… in ink -------------------------------------------------------
        call('new_layer', document_id=doc, name='Shape')
        shape = len(call('get_document', document_id=doc)['layers']) - 1
        call('select_rect', document_id=doc, x=10, y=6, width=12, height=10)
        call('fill_selection', document_id=doc, layer=shape, ink=[0, 0, 100, 0])
        call('deselect', document_id=doc)
        reply = call('stroke_selection', document_id=doc, layer=shape, width=2, location='outside',
                     ink=[0, 0, 0, 100])
        assert reply.get('storage') == 'cmyka16_unorm' and reply.get('outline') == 'layer_edge', reply
        yellow = codes([0, 0, 100, 0])
        assert ink(doc, 9, 10) == BLACK, ink(doc, 9, 10)
        assert ink(doc, 15, 10) == yellow, ink(doc, 15, 10)
        assert ink(doc, 4, 4) == a, ink(doc, 4, 4)
        receipts['s7'] = reply
        print('PASS s7: Stroke… outlines the layer in ink', flush=True)

        # ---- S8: Apply Layer Mask -----------------------------------------------------
        call('select_rect', document_id=doc, x=10, y=6, width=6, height=10)
        call('add_layer_mask', document_id=doc, layer=shape, kind='from_selection')
        call('deselect', document_id=doc)
        reply = call('remove_layer_mask', document_id=doc, layer=shape, apply=True)
        assert reply.get('storage') == 'cmyka16_unorm' and reply.get('changed') is True, reply
        call('get_mask_properties', fail=True, document_id=doc, layer=shape)
        assert ink(doc, 12, 10) == yellow, ink(doc, 12, 10)
        assert ink(doc, 19, 10) == a, ink(doc, 19, 10)
        receipts['s8'] = reply
        print('PASS s8: Apply Layer Mask bakes the mask into the CMYK layer\'s alpha', flush=True)

        # ---- S9: .rz round trip -------------------------------------------------------
        probes = [(4, 4), (9, 10), (12, 10), (19, 10), (25, 5), (31, 11), (39, 23)]
        rz = folder / 'selected.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        assert len(call('get_document', document_id=again)['layers']) == \
            len(call('get_document', document_id=doc)['layers'])
        for p in probes:
            assert sample(again, *p) == sample(doc, *p), p
        receipts['s9'] = {str(p): sample(again, *p) for p in probes}
        print('PASS s9: the .rz reopens bit-exact', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
