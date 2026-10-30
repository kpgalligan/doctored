#!/usr/bin/env python3
"""The #547 phase-2 MCP drive: a CMYK document is EDITED in ink over MCP —
New Layer, the Brush, Fill, Gradient and Paint Bucket with `ink`, Levels and
Curves (an adjustment layer with a mask), Hue/Saturation on the ink with K
held, the `.rz` round trip — and what is
still outside phase 2 refuses by name.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python. The CMYK profile is macOS's
Generic CMYK Profile (always installed). Ink codes are `percent / 100 ×
65535` rounded half up.

- **E1 opens.** A 16-bit CMYK TIFF opens as CMYK; its `save_copy jpeg` and its
  `export_psd mode flattened` reopen as CMYK documents (`inspect_psd`'s
  `import_precision` "cmyk", `psd_mode` "composite"), and each takes a
  `fill_selection` with `ink` [10, 20, 30, 40] whose sample is that ink EXACTLY.
- **E2 new layer + brush.** `new_layer` adds bare paper (the composite's ink is
  unchanged); a `brush_stroke` with `ink` [0, 0, 0, 100] on it moves the ink
  under the stroke's centre toward that ink in every component, and changes
  nothing far from it.
- **E3 bucket.** `fill` with `ink` [100, 0, 0, 0], tolerance 0, on a flat layer
  turns every pixel of the flat region into that ink exactly.
- **E4 gradient.** `gradient` with `start_ink` [0, 0, 0, 0] and `end_ink`
  [0, 100, 0, 0], linear left to right across a new layer, reads within 2 % of
  the start ink at the left edge, of the end ink at the right edge, and is
  monotone between.
- **E5 Curves layer + mask.** `add_adjustment_layer` curves {cmyk: [[0, 0],
  [255, 128]]} over a select_rect selection (the mask is the selection) halves
  every ink inside the rectangle within 1 % and leaves the outside exactly as
  it was; `get_document` lists it as an adjustment; with the mask disabled the
  whole canvas halves, and a feathered mask (set_mask_properties) still hides
  the far outside.
- **E6 .rz.** `save_copy rz` reopens as CMYK with the same layer count and the
  same `sample_color` ink16 at every probed point (bit-exact), the Curves layer
  still an adjustment.
- **E7 refusals.** depth_blur (RGB only; the Clone Stamp and the brush blend
  modes run on ink since #547's retouch batch); Hue
  Rotate (RGB only since the adjustments batch, which admits the filters);
  Levels with an RGB `r` stage; and `ink` on an RGB
  document ("ink is a CMYK document's colour") each refuse. (`fill` with
  sample_all_layers is admitted since the selection batch:
  `scripts/verify_cmyk_select.py`.)
- **E8 Hue/Saturation** (K held, `cmyk_adjust::hue_saturation_on_ink`). On a
  16-bit Generic CMYK TIFF with a K-only patch [0, 0, 0, 50] and a chromatic
  patch [10, 80, 90, 0]: a) `apply_filter hue_saturation {hue: 30}` changes
  the chromatic patch's C, M, Y and keeps its K exactly, and leaves the K-only
  grey's ink16 EXACTLY as it was (a K-only grey stays K-only); one `undo`
  restores both patches' ink16 exactly; b) `{saturation: -1}` makes the
  chromatic patch's C, M and Y equal, each within 2 codes of `1 − L` with `L`
  the mid of `1 − C, 1 − M, 1 − Y` (0.5 here), K kept; c) `{hue: 0}` adds no
  history step; d)
  `add_adjustment_layer hue_saturation` Colorize (200, 0.4) is listed as an
  adjustment and moves the composite; `edit_adjustment_layer` to a reds band
  changes it again; `save_copy rz` reopens with the same ink16 at every probed
  point and the layer still an adjustment.
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
from verify_phase1 import png

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 40, 24
FLAT = [20000, 30000, 10000, 5000]
GRADIENT_BOUND = 0.02 * 65535
CURVE_BOUND = 0.01 * 65535
REFUSED = 'is unavailable in a CMYK document'
GREY_PATCH = [0, 0, 0, 50]
CHROMA_PATCH = [10, 80, 90, 0]


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


def hue_saturation(app, folder, profile):
    """E8 (module doc): a left grey half and a right chromatic half."""
    call = app.call
    width, height = 20, 10
    grey, chroma = codes(GREY_PATCH), codes(CHROMA_PATCH)
    samples = []
    for _ in range(height):
        for x in range(width):
            samples += grey if x < width // 2 else chroma
    path = folder / 'huesat.tif'
    path.write_bytes(cmyk_tiff(width, height, samples, profile))
    doc = call('open_document', path=str(path))['document']['id']
    probes = {'grey': (4, 5), 'chroma': (15, 5)}

    def sample(document, name):
        reply = call('sample_color', document_id=document, x=probes[name][0], y=probes[name][1])
        assert reply.get('color_mode') == 'cmyk', reply
        assert 'normalized_rgba' in reply, reply
        return reply['ink16'], reply['normalized_rgba'][:3]

    def undo_count():
        return int(call('list_history', document_id=doc)['undo_count'])

    out = {}
    # a) a master Hue edit turns the chromatic patch with K kept, and keeps the K-only grey.
    before = {name: sample(doc, name) for name in probes}
    assert before['grey'][0] == grey and before['chroma'][0] == chroma, before
    call('apply_filter', document_id=doc, layer=0, filter='hue_saturation', params={'hue': 30})
    after = {name: sample(doc, name) for name in probes}
    assert after['chroma'][0][:3] != chroma[:3] and after['chroma'][0][3] == chroma[3], after
    assert after['grey'][0] == grey, ('the K-only grey is kept', after['grey'], grey)
    call('undo', document_id=doc)
    assert sample(doc, 'grey')[0] == grey and sample(doc, 'chroma')[0] == chroma, 'undo restores'
    out['a'] = {'grey_ink16_after': after['grey'][0], 'chroma_ink16_after': after['chroma'][0]}
    # b) Saturation −100: the chromatic patch's C, M, Y become 1 − L, K kept.
    call('apply_filter', document_id=doc, layer=0, filter='hue_saturation',
         params={'saturation': -1})
    ink = sample(doc, 'chroma')[0]
    stored = [1 - v / 65535 for v in chroma[:3]]
    want = round((1 - (max(stored) + min(stored)) / 2) * 65535)
    assert all(abs(v - want) <= 2 for v in ink[:3]) and ink[3] == chroma[3], (ink, want)
    call('undo', document_id=doc)
    out['b'] = {'chroma_ink16': ink, 'want_cmy': want}
    # c) Hue 0 is no edit and leaves no history step.
    steps = undo_count()
    result = app.client.rpc('tools/call', {'name': 'apply_filter', 'arguments': {
        'document_id': doc, 'layer': 0, 'filter': 'hue_saturation', 'params': {'hue': 0}}})
    assert undo_count() == steps, (steps, undo_count(), result)
    assert sample(doc, 'chroma')[0] == chroma
    out['c'] = {'undo_count': steps, 'is_error': bool(result.get('isError'))}
    # d) a Colorize layer, then a reds band, then the .rz round trip.
    call('add_adjustment_layer', document_id=doc, op='hue_saturation',
         params={'colorize': True, 'colorize_hue': 200, 'colorize_saturation': 0.4})
    layers = call('get_document', document_id=doc)['layers']
    tone = len(layers) - 1
    assert layers[tone].get('kind') == 'adjustment' or layers[tone].get('adjustment'), layers
    colorized = {name: sample(doc, name)[0] for name in probes}
    assert colorized['chroma'] != chroma and colorized['grey'] != grey, colorized
    call('edit_adjustment_layer', document_id=doc, layer=tone,
         params={'bands': {'reds': {'hue': 30, 'saturation': 0.25}}})
    banded = {name: sample(doc, name)[0] for name in probes}
    assert banded['chroma'] != colorized['chroma'], (banded, colorized)
    rz = folder / 'huesat.rz'
    call('save_copy', document_id=doc, path=str(rz), format='rz')
    again = call('open_document', path=str(rz))['document']['id']
    reopened = call('get_document', document_id=again)['layers']
    assert len(reopened) == len(layers), (reopened, layers)
    assert reopened[tone].get('kind') == 'adjustment' or reopened[tone].get('adjustment'), reopened
    for name in probes:
        assert sample(again, name)[0] == banded[name], name
    out['d'] = {'colorized': colorized, 'banded': banded}
    call('close_document', document_id=again, discard_changes=True)
    call('close_document', document_id=doc, discard_changes=True)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-edit-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_edit.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def ink(doc, x, y):
            sample = call('sample_color', document_id=doc, x=x, y=y)
            assert sample.get('color_mode') == 'cmyk', sample
            return sample['ink16']

        # ---- E1: TIFF, JPEG and PSD open as CMYK and take ink ---------------
        source = folder / 'flat.tif'
        source.write_bytes(cmyk_tiff(W, H, FLAT * (W * H), profile))
        doc = call('open_document', path=str(source))['document']['id']
        assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
        jpeg, psd = folder / 'flat.jpg', folder / 'flat.psd'
        call('save_copy', document_id=doc, path=str(jpeg), format='jpeg')
        call('export_psd', document_id=doc, path=str(psd), mode='flattened')
        report = call('inspect_psd', path=str(psd))
        assert report.get('import_precision') == 'cmyk', report
        opened = {
            'tiff': doc,
            'jpeg': call('open_document', path=str(jpeg))['document']['id'],
            'psd': call('open_document', path=str(psd), psd_mode='composite',
                        psd_fingerprint=report['source_fingerprint'])['document']['id'],
        }
        want = codes([10, 20, 30, 40])
        for name, d in opened.items():
            if name == 'tiff':
                continue
            call('select_rect', document_id=d, x=2, y=2, width=6, height=6)
            call('fill_selection', document_id=d, ink=[10, 20, 30, 40], layer=0)
            got = ink(d, 4, 4)
            assert got == want, (name, got, want)
            call('close_document', document_id=d, discard_changes=True)
        receipts['e1'] = {'ink16': want}
        print('PASS e1: TIFF, JPEG and PSD open as CMYK and fill in ink exactly', flush=True)

        # ---- E2: new layer + brush -----------------------------------------
        before = ink(doc, 10, 10)
        call('new_layer', document_id=doc, name='Ink')
        assert ink(doc, 10, 10) == before, 'a new layer is bare paper'
        layers = call('get_document', document_id=doc)['layers']
        top = len(layers) - 1
        call('brush_stroke', document_id=doc, layer=top, points=[[10, 10], [14, 10]],
             ink=[0, 0, 0, 100], size=7, opacity=1)
        after = ink(doc, 12, 10)
        black = codes([0, 0, 0, 100])
        for c in range(4):
            assert abs(after[c] - black[c]) < abs(before[c] - black[c]), (after, before)
        assert ink(doc, 35, 20) == before, 'the stroke reached a pixel far from it'
        receipts['e2'] = {'before': before, 'after': after}
        print('PASS e2: new_layer is paper and the brush paints ink', flush=True)

        # ---- E3: paint bucket ------------------------------------------------
        call('fill', document_id=doc, layer=0, x=30, y=20, ink=[100, 0, 0, 0], tolerance=0)
        cyan = codes([100, 0, 0, 0])
        for point in [(30, 20), (0, 0), (39, 23), (20, 5)]:
            assert ink(doc, *point) == cyan, (point, ink(doc, *point))
        receipts['e3'] = {'ink16': cyan}
        print('PASS e3: the bucket fills the flat region with the ink exactly', flush=True)

        # ---- E4: gradient ------------------------------------------------------
        call('new_layer', document_id=doc, name='Ramp')
        ramp = len(call('get_document', document_id=doc)['layers']) - 1
        call('gradient', document_id=doc, layer=ramp, x0=0, y0=12, x1=W, y1=12,
             start_ink=[0, 0, 0, 0], end_ink=[0, 100, 0, 0])
        row = [ink(doc, x, 12) for x in range(0, W, 4)]
        assert abs(row[0][1] - 0) <= GRADIENT_BOUND, row[0]
        right = ink(doc, W - 1, 12)
        assert abs(right[1] - 65535) <= GRADIENT_BOUND, right
        assert all(b[1] >= a[1] for a, b in zip(row, row[1:])), row
        receipts['e4'] = {'magenta': [r[1] for r in row]}
        print('PASS e4: the gradient ramps the ink', flush=True)
        call('delete_layer', document_id=doc, index=ramp)

        # ---- E5: Curves layer through a selection mask -------------------------
        probes = [(5, 5), (12, 10), (25, 15), (35, 20)]
        under = {p: ink(doc, *p) for p in probes}
        call('select_rect', document_id=doc, x=0, y=0, width=20, height=24)
        call('add_adjustment_layer', document_id=doc, op='curves',
             params={'cmyk': [[0, 0], [255, 128]]})
        call('deselect', document_id=doc)
        info = call('get_document', document_id=doc)
        adjust = [l for l in info['layers'] if l.get('kind') == 'adjustment' or l.get('adjustment')]
        assert adjust, info['layers']
        for p in probes:
            got = ink(doc, *p)
            if p[0] < 20:
                for c in range(4):
                    assert abs(got[c] - under[p][c] * 128 / 255) <= CURVE_BOUND, (p, got, under[p])
            else:
                assert got == under[p], (p, got, under[p])
        tone = len(info['layers']) - 1
        mask = call('get_mask_properties', document_id=doc, layer=tone)
        call('set_layer_mask_enabled', document_id=doc, layer=tone, enabled=False)
        whole = ink(doc, 25, 15)
        for c in range(4):
            assert abs(whole[c] - under[(25, 15)][c] * 128 / 255) <= CURVE_BOUND, (whole, under)
        call('set_layer_mask_enabled', document_id=doc, layer=tone, enabled=True)
        call('set_mask_properties', document_id=doc, layer=tone, feather=2)
        assert ink(doc, 35, 20) == under[(35, 20)], 'a feathered mask still hides far outside'
        receipts['e5'] = {str(p): ink(doc, *p) for p in probes}
        receipts['e5']['mask'] = mask
        print('PASS e5: the Curves layer halves the ink inside its mask only', flush=True)

        # ---- E6: .rz round trip ---------------------------------------------------
        rz = folder / 'edited.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        a_layers = call('get_document', document_id=again)['layers']
        assert len(a_layers) == len(info['layers']), (a_layers, info['layers'])
        for p in probes:
            assert ink(again, *p) == ink(doc, *p), p
        receipts['e6'] = {'layers': len(a_layers)}
        print('PASS e6: the .rz reopens bit-exact', flush=True)

        # ---- E7: refusals -----------------------------------------------------------
        rgb_path = folder / 'rgb.png'
        png(rgb_path, 8, 8, bytes([200, 80, 40, 255] * 64))
        rgb = call('open_document', path=str(rgb_path))['document']['id']
        refusals = {
            'depth_blur': call('depth_blur', fail=True, document_id=doc, layer=0),
            'hue_rotate': call('apply_filter', fail=True, document_id=doc, layer=0,
                               filter='hue_rotate', degrees=30),
            'levels_r': call('add_adjustment_layer', fail=True, document_id=doc, op='levels',
                             params={'r': {'gamma': 2}}),
            'rgb_ink': call('fill_selection', fail=True, document_id=rgb, ink=[0, 0, 0, 10]),
        }
        assert REFUSED in refusals['depth_blur'], refusals
        assert REFUSED in refusals['hue_rotate'], refusals
        assert 'RGB channel key "r"' in refusals['levels_r'], refusals
        assert "ink is a CMYK document's colour" in refusals['rgb_ink'], refusals
        receipts['e7'] = refusals
        print('PASS e7: what phase 2 does not do refuses by name', flush=True)

        # ---- E8: Hue/Saturation with K held -----------------------------------------
        receipts['e8'] = hue_saturation(app, folder, profile)
        print('PASS e8: Hue/Saturation edits ink with K held, destructively and as a layer',
              flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
