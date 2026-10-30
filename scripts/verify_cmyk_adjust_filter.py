#!/usr/bin/env python3
"""The #547 adjustments-and-filters MCP drive: on a CMYK document every
adjustment enabled in CMYK applies destructively and (where it has a
layer kind) as an adjustment layer, a filter from each submenu runs on
ink, Edit ▸ Fade works, the `.rz` round trip keeps the new records, and
what stays refused refuses by name.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

The fixture is written HERE: a 30 × 12 16-bit CMYK TIFF in macOS's Generic
CMYK Profile, three 10-px bands — K-only [0, 0, 0, 60], chromatic
[10, 80, 90, 0] and rich [60, 50, 40, 90] percent — ink codes `percent / 100
× 65535` rounded half up. Probes: each band's centre (5, 6), (15, 6),
(25, 6), and the two band edges (9, 6), (10, 6), (19, 6), (20, 6).

- **F1 destructive.** Each enabled adjustment's `apply_filter` (bcs, levels,
  curves, hue_saturation, invert, posterize, threshold, color_balance,
  photo_filter with ink, channel_mixer, selective_color, gradient_map with ink
  stops, color_lookup, shadows_highlights, replace_color with inks) and each
  Auto tool (auto_tone, auto_contrast, auto_color) changes ink16 at some
  probe, and one `undo` restores every probe exactly.
- **F2 layers.** Each layer-capable op's `add_adjustment_layer` lists as an
  adjustment (`is_adjustment`, its `adjustment.op`) and moves the composite's
  ink16 at some probe; `edit_adjustment_layer` with other params moves it
  again; two `undo`s restore every probe exactly.
- **F3 .rz.** A document with Channel Mixer, Selective Color, Gradient Map,
  Photo Filter and Threshold layers saves as `rz` and reopens with equal ink16
  at every probe and the same ops.
- **F4 filters.** `blur` 3, `add_noise`, `high_pass`, `pixelate`, `clouds`
  with `foreground_ink`, `unsharp_mask`, `smart_sharpen` and `emboss` each
  change ink16 at some probe; one `undo` restores every probe exactly.
- **F5 fade.** `fade` 50 % Normal after `blur` 4 lands within 1 code of the
  before/after midpoint, per ink, at three probes on the band edges.
- **F6 refusals.** `hue_rotate`; `shadows_highlights` as a layer; `depth_blur`;
  `adjust` with saturation 0.5; and each disabled op of take1
  (exposure, vibrance, black_and_white, lens_correction, presence, add_grain,
  develop_filter) refuse by name.
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

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 30, 12
BANDS = [[0, 0, 0, 60], [10, 80, 90, 0], [60, 50, 40, 90]]
PROBES = [(5, 6), (15, 6), (25, 6), (9, 6), (10, 6), (19, 6), (20, 6)]
FADE_PROBES = [(9, 6), (10, 6), (19, 6)]
FADE_BOUND = 1
REFUSED = 'is unavailable in a CMYK document'


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


def lut_table():
    """A 2-node 3D LUT that darkens red: 8 RGB triples of f32, red fastest."""
    values = []
    for b in (0, 1):
        for g in (0, 1):
            for r in (0, 1):
                values += [0.8 * r, float(g), float(b)]
    return base64.b64encode(struct.pack(f'<{len(values)}f', *values)).decode()


# Destructive: (filter, flat arguments, params).
DESTRUCTIVE = [
    ('adjust', dict(brightness=0.2, contrast=0.2, legacy=False), None),
    ('levels', dict(gamma=1.5), None),
    ('curves', {}, {'cmyk': [[0, 0], [128, 180], [255, 255]]}),
    ('hue_saturation', {}, {'hue': 30}),
    ('invert', {}, None),
    ('posterize', dict(levels=3), None),
    ('threshold', dict(level=0.5), None),
    ('color_balance', {}, {'midtones': {'cyan_red': 0.4}}),
    ('photo_filter', {}, {'ink': [0, 0.5, 1, 0], 'density': 0.5}),
    ('channel_mixer', {}, {'cyan': {'c': 0.5, 'k': 0.5}}),
    ('selective_color', {}, {'reds': {'c': -0.3}, 'neutrals': {'k': 0.2}, 'blacks': {'k': -0.2}}),
    ('gradient_map', {}, {'gradient': {'stops': [{'position': 0, 'ink': [0, 0, 0, 1]},
                                                 {'position': 1, 'ink': [0.2, 0, 0, 0]}]}}),
    ('color_lookup', {}, {'kind': '3d', 'size': 2, 'table': lut_table()}),
    ('shadows_highlights', {}, {'shadows': {'amount': 0.8}, 'radius': 4}),
    ('replace_color', {}, {'inks': [[0.1, 0.8, 0.9, 0]], 'fuzziness': 40, 'hue': 60}),
]
AUTO = ['auto_tone', 'auto_contrast', 'auto_color']
# Layers: (op, params, edited params).
LAYERS = [
    ('bcs', {'brightness': 0.2, 'legacy': False}, {'contrast': 0.3, 'legacy': False}),
    ('levels', {'gamma': 1.5}, {'gamma': 0.6}),
    ('curves', {'cmyk': [[0, 0], [128, 180], [255, 255]]}, {'k': [[0, 0], [128, 60], [255, 255]]}),
    ('hue_saturation', {'hue': 30}, {'hue': -40}),
    ('invert', {}, None),
    ('posterize', {'levels': 3}, {'levels': 5}),
    ('threshold', {'level': 0.5}, {'level': 0.3}),
    ('color_balance', {'midtones': {'cyan_red': 0.4}}, {'shadows': {'yellow_blue': 0.5}}),
    ('photo_filter', {'ink': [0, 0.5, 1, 0], 'density': 0.5}, {'ink': [1, 0.3, 0, 0], 'density': 0.7}),
    ('channel_mixer', {'cyan': {'c': 0.5, 'k': 0.5}}, {'black': {'k': 0.5, 'c': 0.3}}),
    ('selective_color', {'neutrals': {'k': 0.2}}, {'reds': {'m': 0.4}}),
    ('gradient_map', {'gradient': {'stops': [{'position': 0, 'ink': [0, 0, 0, 1]},
                                             {'position': 1, 'ink': [0.2, 0, 0, 0]}]}},
     {'gradient': {'stops': [{'position': 0, 'ink': [1, 0, 0, 0]},
                             {'position': 1, 'ink': [0, 0, 1, 0]}]}}),
    ('color_lookup', {'kind': '3d', 'size': 2, 'table': lut_table()},
     {'kind': '3d', 'size': 2, 'table': lut_table(), 'strength': 0.5}),
]
F3_OPS = ['channel_mixer', 'selective_color', 'gradient_map', 'photo_filter', 'threshold']
FILTERS = [
    ('blur', dict(sigma=3)),
    ('add_noise', dict(amount=0.2, seed=5)),
    ('high_pass', dict(radius=3)),
    ('pixelate', dict(block=4)),
    ('clouds', dict(seed=3, foreground_ink=[0, 0, 0, 100], background_ink=[0, 0, 0, 0])),
    ('unsharp_mask', dict(radius=2, amount=1.5)),
    ('smart_sharpen', {}),
    ('emboss', {}),
]
# Disabled in CMYK (take1); each: (tool, arguments).
DISABLED = [
    ('apply_filter', dict(filter='exposure', params={'exposure': 1})),
    ('apply_filter', dict(filter='vibrance', params={'vibrance': 0.5})),
    ('apply_filter', dict(filter='black_and_white', params={})),
    ('apply_filter', dict(filter='lens_correction', distortion=0.2)),
    ('apply_filter', dict(filter='presence', clarity=20)),
    ('apply_filter', dict(filter='add_grain', amount=30)),
    ('apply_filter', dict(filter='develop_filter', exposure=0.5)),
]


def run(output):
    receipts = {}
    folder = Path(tempfile.mkdtemp(prefix='cmyk-af-', dir=output))
    profile = GENERIC.read_bytes()
    samples = []
    for _ in range(H):
        for x in range(W):
            samples += codes(BANDS[x // 10])
    path = folder / 'bands.tif'
    path.write_bytes(cmyk_tiff(W, H, samples, profile))
    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call
        doc = call('open_document', path=str(path))['document']['id']
        precision = call('get_precision', document_id=doc)
        assert precision.get('color_mode') == 'cmyk', precision

        def ink(document, x, y):
            reply = call('sample_color', document_id=document, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16']

        def snapshot(document):
            return [ink(document, *p) for p in PROBES]

        base = snapshot(doc)
        assert base[0] == codes(BANDS[0]) and base[1] == codes(BANDS[1]), base

        # ---- F1: destructive adjustments and Auto ------------------------------
        moved = {}
        for name, flat, params in DESTRUCTIVE:
            args = dict(flat)
            if params is not None:
                args['params'] = params
            reply = call('apply_filter', document_id=doc, layer=0, filter=name, **args)
            assert reply.get('changed'), (name, reply)
            after = snapshot(doc)
            assert after != base, (name, after)
            moved[name] = sum(1 for a, b in zip(after, base) if a != b)
            call('undo', document_id=doc)
            assert snapshot(doc) == base, f'{name}: undo restores'
        for tool in AUTO:
            reply = call(tool, document_id=doc, layer=0)
            assert reply.get('changed'), (tool, reply)
            assert reply.get('clip') == 0.001, (tool, reply)
            assert snapshot(doc) != base, tool
            moved[tool] = reply.get('form')
            call('undo', document_id=doc)
            assert snapshot(doc) == base, f'{tool}: undo restores'
            # Auto runs on a layer's ink only: a mask target refuses and writes nothing.
            refused = call(tool, fail=True, document_id=doc, layer=0, target='mask')
            assert 'runs Auto on a layer' in refused, (tool, refused)
            assert snapshot(doc) == base, f'{tool}: a refused mask target writes nothing'
        receipts['f1'] = moved
        print('PASS f1: every enabled adjustment and Auto edits ink; undo is exact', flush=True)

        # ---- F2: adjustment layers -------------------------------------------------
        layered = {}
        for op, params, edited in LAYERS:
            call('add_adjustment_layer', document_id=doc, op=op, params=params)
            layers = call('get_document', document_id=doc)['layers']
            top = len(layers) - 1
            assert layers[top].get('is_adjustment'), (op, layers[top])
            assert (layers[top].get('adjustment') or {}).get('op') == op, (op, layers[top])
            first = snapshot(doc)
            assert first != base, (op, first)
            if edited is not None:
                call('edit_adjustment_layer', document_id=doc, layer=top, params=edited)
                second = snapshot(doc)
                assert second != first, (op, second)
                call('undo', document_id=doc)
            call('undo', document_id=doc)
            assert snapshot(doc) == base, f'{op}: undo restores'
            layered[op] = {'moved': sum(1 for a, b in zip(first, base) if a != b)}
        receipts['f2'] = layered
        print('PASS f2: every layer-capable op is an adjustment layer and edits', flush=True)

        # ---- F3: the .rz round trip ------------------------------------------------
        for op in F3_OPS:
            params = next(p for o, p, _ in LAYERS if o == op)
            call('add_adjustment_layer', document_id=doc, op=op, params=params)
        stacked = snapshot(doc)
        ops = [(layer.get('adjustment') or {}).get('op')
               for layer in call('get_document', document_id=doc)['layers']]
        rz = folder / 'records.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        assert snapshot(again) == stacked, 'the .rz reopens bit-exact'
        reopened = [(layer.get('adjustment') or {}).get('op')
                    for layer in call('get_document', document_id=again)['layers']]
        assert reopened == ops, (reopened, ops)
        call('close_document', document_id=again, discard_changes=True)
        for _ in F3_OPS:
            call('undo', document_id=doc)
        assert snapshot(doc) == base, 'F3 undo restores'
        receipts['f3'] = {'ops': ops}
        print('PASS f3: the new records reopen bit-exact', flush=True)

        # ---- F4: one filter from each submenu ---------------------------------------
        filtered = {}
        for name, args in FILTERS:
            reply = call('apply_filter', document_id=doc, layer=0, filter=name, **args)
            assert reply.get('changed'), (name, reply)
            after = snapshot(doc)
            assert after != base, (name, after)
            filtered[name] = sum(1 for a, b in zip(after, base) if a != b)
            call('undo', document_id=doc)
            assert snapshot(doc) == base, f'{name}: undo restores'
        receipts['f4'] = filtered
        print('PASS f4: a filter from each submenu runs on ink; undo is exact', flush=True)

        # ---- F5: Fade ------------------------------------------------------------------
        before = [ink(doc, *p) for p in FADE_PROBES]
        call('apply_filter', document_id=doc, layer=0, filter='blur', sigma=4)
        blurred = [ink(doc, *p) for p in FADE_PROBES]
        assert blurred != before, blurred
        reply = call('fade', document_id=doc, opacity=0.5)
        assert reply.get('changed'), reply
        faded = [ink(doc, *p) for p in FADE_PROBES]
        for b, a, f in zip(before, blurred, faded):
            for c in range(4):
                assert abs(f[c] - (a[c] + b[c]) / 2) <= FADE_BOUND, (b, a, f)
        call('undo', document_id=doc)
        receipts['f5'] = {'before': before, 'blurred': blurred, 'faded': faded}
        print('PASS f5: Fade 50 % lands on the midpoint', flush=True)

        # ---- F6: refusals ---------------------------------------------------------------
        refusals = {
            'hue_rotate': call('apply_filter', fail=True, document_id=doc, layer=0,
                               filter='hue_rotate', degrees=30),
            'shadows_highlights_layer': call('add_adjustment_layer', fail=True, document_id=doc,
                                             op='shadows_highlights', params={}),
            'depth_blur': call('depth_blur', fail=True, document_id=doc, layer=0),
            'bcs_saturation': call('apply_filter', fail=True, document_id=doc, layer=0,
                                   filter='adjust', saturation=0.5, legacy=False),
        }
        for tool, args in DISABLED:
            refusals[args['filter']] = call(tool, fail=True, document_id=doc, layer=0, **args)
        assert 'Doctored offers it on RGB documents only' in refusals['hue_rotate'], refusals
        assert 'Shadows/Highlights applies to pixels only' in \
            refusals['shadows_highlights_layer'], refusals
        assert 'Depth Blur ' + REFUSED in refusals['depth_blur'], refusals
        assert 'Saturation in Brightness/Contrast ' + REFUSED in refusals['bcs_saturation'], refusals
        for _, args in DISABLED:
            refusal = refusals[args['filter']]
            assert REFUSED in refusal and 'RGB documents only' not in refusal, refusals
        receipts['f6'] = refusals
        print('PASS f6: what stays refused refuses by name', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, default=None)
    arguments = parser.parse_args()
    output = arguments.output or Path(tempfile.mkdtemp(prefix='verify-cmyk-af-'))
    output.mkdir(parents=True, exist_ok=True)
    run(output)


if __name__ == '__main__':
    main()
