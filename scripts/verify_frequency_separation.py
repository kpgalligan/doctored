#!/usr/bin/env python3
"""MCP drive for Apply Image's Scale and Offset and Filters > Other >
Frequency Separation (#17) in every form the split takes: RGB at 8, 16 and 32
bits, Grayscale at 8, 16 and 32, Duotone, Lab Color at 8 and 16, and a 16-bit
document evaluated in linear light.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process. Every expected number is computed here from
codes the app reports, never from the app's own arithmetic, and every bound
is fixed here before anything is measured.

APPLY IMAGE. A 24x20 opaque picture A (noise over hard edges). Layer 1 is a
duplicate of layer 0 inverted, so it holds 255 - A; `apply_image` onto layer 1
with source layer 0, Subtract, scale 2 and offset 128 must store
round(clamp(((255 - A) - A) / 2 + 128)) per channel (at 16 bits the same in
65535 codes with offset 128/255 of full scale), exactly or one code off where
the exact value is within a hair of a half. Multiply with a scale refuses.

FREQUENCY SEPARATION. `frequency_separation` on the opaque picture at radius
2.5: the stack becomes "<name> Low" then "<name> High" (Linear Light,
clipped), the composite differs from the original by at most one code on every
channel of every pixel, and one undo restores the single layer and its pixels
exactly. A top layer at 60 % opacity with a feathered mask, over a
background, recomposes the composite within one code, the low layer keeping
the mask and the opacity. A group and a clipped layer refuse, and so does a
CMYK document.

32-BIT FLOAT. `core/tests/fixtures/float_11f/scene.tiff` (linear samples above
white and below zero) splits at radius 2: the High layer is Linear Dodge (Add),
clipped, the reply's `high_blend_mode` says so, the High layer holds negative
samples, and on every opaque pixel the composite (`center_rgba32`) equals the
original within `float_bound` (2^-22 of the larger magnitude of the original
and the low, plus 1e-9 for the reply's decimals); one undo restores the layer.
`get_precision` lists `frequency_separation` among the float tools.

LINEAR LIGHT. At 16 bits a document evaluated in linear light splits, High in
Linear Light, and recomposes within `LINEAR_BOUND16` stored codes (the detail
is computed on decoded components; `frequency_separation_modes.rs` holds the
decoded bound). At 8 bits it refuses with `EIGHT_BIT_LINEAR` and the layer is
left as it was.

MODES. `mode_check(app, folder, mode, depth)`, the call each colour-mode drive
makes (`verify_gray_mode.py`, `verify_lab_mode.py`, `verify_duotone_mode.py`):
the picture converted to `mode` at `depth` (16 and 32 by `convert_precision`,
then `convert_mode`; Duotone through Grayscale with one black ink) splits, the
High layer clipped in Linear Light (Linear Dodge (Add) at 32 bits); the
composite's stored codes (`center_rgba16`) are within one code of the
original's on every channel, or within `float_bound` at 32 bits; Grayscale
and Duotone keep R = G = B; Grayscale and Lab Color list
`showFrequencySeparation:` in `get_precision`'s `supported_menu_items`.

Usage: python3 scripts/verify_frequency_separation.py [--output DIR] [--only NAME]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
from pathlib import Path
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_lens import App  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
W, H = 24, 20
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
LINEAR_BOUND16 = 24
EIGHT_BIT_LINEAR = ('Frequency separation in an 8-bit document evaluated in linear light would '
                    'rebuild the layer only within about twenty levels in the shadows: the detail '
                    'is stored as an 8-bit code, and linear light magnifies its rounding there. '
                    'Convert to 16 Bits/Channel, or set Image ▸ Evaluation… to Encoded, first.')
MONOTONE = [{'name': 'Black', 'lab': [0, 0, 0], 'curve': [0] + [None] * 11 + [100]}]
# The modes whose `get_precision` lists its menu items.
MENU_MODES = ('gray', 'lab')


def float_bound(original, low):
    return 2 ** -22 * max(abs(original), abs(low)) + 1e-9


def splitmix(state):
    state = (state + 0x9E3779B97F4A7C15) & (2 ** 64 - 1)
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & (2 ** 64 - 1)
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & (2 ** 64 - 1)
    return state, z ^ (z >> 31)


def picture(seed):
    state = seed
    out = []
    for y in range(H):
        row = []
        for x in range(W):
            state, v = splitmix(state)
            edge = 50 if (x // 5 + y // 4) % 2 == 0 else 200
            row.append([min(255, max(0, edge + ((v >> (8 * c)) & 63) - 31)) for c in range(3)])
        out.append(row)
    return out


def write(folder, name, pixels):
    data = bytearray()
    for row in pixels:
        for p in row:
            data += bytes(p + [255])
    path = Path(folder) / name
    png(path, W, H, bytes(data))
    return path


def codes(app, document, source='composite', layer=None):
    """Every pixel's RGBA codes (8-bit 0..255, 16-bit 0..65535)."""
    extra = {} if layer is None else dict(layer=layer)
    return [[app.call('sample_pixel', document_id=document, source=source, x=x, y=y,
                      **extra)['center_rgba_codes'] for x in range(W)] for y in range(H)]


def stored(app, document, size=(W, H), source='composite', layer=None):
    """Every pixel's stored colour: linear values at 32 bits (`center_rgba32`), else the u16
    codes (`center_rgba16`, a typed document's own codes at every depth)."""
    extra = {} if layer is None else dict(layer=layer)
    out = []
    for y in range(size[1]):
        for x in range(size[0]):
            s = app.call('sample_pixel', document_id=document, source=source, x=x, y=y, **extra)
            out.append(s['center_rgba32'] if 'center_rgba32' in s else s['center_rgba16'])
    return out


def agrees(got, exact, hair):
    rounded = int(exact + 0.5)
    if got == rounded:
        return True
    return abs(exact - int(exact) - 0.5) < hair and abs(got - exact) < 1


def open_picture(app, folder, name, depth, pixels):
    document = app.call('open_document', path=str(write(folder, name, pixels)))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if depth == 32:
        app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    return document


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def split_layers(app, document, name, mode):
    """The split's stack: "<name> Low" then "<name> High", clipped, in `mode`."""
    rows = layers(app, document)
    assert len(rows) == 2, rows
    assert rows[0]['name'] == name + ' Low' and rows[1]['name'] == name + ' High', rows
    assert rows[1]['clipped'] is True, rows[1]
    assert rows[1]['blend_mode'] == mode, rows[1]


def float_worst(before, after, low, opaque=None):
    """The worst error as a fraction of `float_bound` over the colour channels of the pixels
    `opaque` names (every pixel when None); asserts each."""
    worst = 0.0
    for i, (b, a, l) in enumerate(zip(before, after, low)):
        if opaque is not None and i not in opaque:
            continue
        for c in range(3):
            bound = float_bound(b[c], l[c])
            error = abs(a[c] - b[c])
            assert error <= bound, ('float recomposed', i, c, a[c], b[c], l[c], bound)
            worst = max(worst, error / bound)
    return worst


def apply_image_case(app, folder, depth):
    a = picture(0x51 + depth)
    document = open_picture(app, folder, f'apply-{depth}.png', depth, a)
    app.call('duplicate_layer', document_id=document, layer=0)
    app.call('apply_filter', document_id=document, layer=1, filter='invert')
    reply = app.call('apply_image', document_id=document, source='0', source_plane='rgb',
                     blend_mode='Subtract', opacity=1, target='layer', layer=1, scale=2, offset=128)
    assert reply.get('ok', True), reply
    top = codes(app, document, source='layer', layer=1)
    full = 255 if depth == 8 else 65535
    worst = 0.0
    for y in range(H):
        for x in range(W):
            for c in range(3):
                base = a[y][x][c] * full / 255
                inverted = full - base
                exact = min(max((inverted - base) / 2 + 128 / 255 * full, 0), full)
                got = top[y][x][c]
                assert agrees(got, exact, 1e-3 if depth == 8 else 0.05), (x, y, c, got, exact)
                worst = max(worst, abs(got - exact))
    refusal = app.refused('apply_image', 'Add and Subtract', document_id=document, source='0',
                          blend_mode='Multiply', target='layer', layer=1, scale=2)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(worst=round(worst, 4), refusal=refusal[:60])


def split_case(app, folder, depth):
    a = picture(0x99 + depth)
    document = open_picture(app, folder, f'split-{depth}.png', depth, a)
    before = codes(app, document)
    name = layers(app, document)[0]['name']
    reply = app.call('frequency_separation', document_id=document, layer=0, radius=2.5)
    assert reply['low_layer'] == 0 and reply['high_layer'] == 1, reply
    assert reply['high_blend_mode'] == 'Linear Light', reply
    split_layers(app, document, name, 'Linear Light')
    after = codes(app, document)
    worst = max(abs(after[y][x][c] - before[y][x][c])
                for y in range(H) for x in range(W) for c in range(4))
    assert worst <= 1, ('recomposed', worst)
    high = codes(app, document, source='layer', layer=1)
    assert any(high[y][x] != before[y][x] for y in range(H) for x in range(W)), 'detail unchanged'
    app.call('undo', document_id=document)
    assert len(layers(app, document)) == 1
    assert codes(app, document) == before, 'undo did not restore the layer'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(worst=worst)


def masked_case(app, folder, depth):
    b = picture(0x1234 + depth)
    document = open_picture(app, folder, f'backdrop-{depth}.png', depth, b)
    # The top layer: the backdrop inverted, at 60 % opacity, masked to a
    # feathered rectangle, so the mask and the opacity both shape the pair.
    app.call('duplicate_layer', document_id=document, layer=0)
    app.call('apply_filter', document_id=document, layer=1, filter='invert')
    app.call('set_layer_properties', document_id=document, layer=1, opacity=0.6)
    app.call('select_rect', document_id=document, x=4, y=3, width=14, height=12, feather=2)
    app.call('add_layer_mask', document_id=document, layer=1, kind='from_selection')
    app.call('deselect', document_id=document)
    before = codes(app, document)
    app.call('frequency_separation', document_id=document, layer=1, radius=4)
    rows = layers(app, document)
    assert len(rows) == 3 and rows[2]['clipped'] is True, rows
    assert rows[1]['has_mask'] and not rows[2]['has_mask'], rows
    assert rows[1]['opacity'] == 0.6 and rows[2]['opacity'] == 1, rows
    after = codes(app, document)
    worst = max(abs(after[y][x][c] - before[y][x][c])
                for y in range(H) for x in range(W) for c in range(4))
    assert worst <= 1, ('recomposed under opacity', worst)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(worst=worst)


def refusal_case(app, folder):
    a = picture(0x3)
    document = open_picture(app, folder, 'refuse.png', 8, a)
    app.call('duplicate_layer', document_id=document, layer=0)
    app.call('set_layer_clipped', document_id=document, layer=1, clipped=True)
    clipped = app.refused('frequency_separation', 'clipping mask', document_id=document, layer=1)
    app.call('set_layer_clipped', document_id=document, layer=1, clipped=False)
    app.call('group_layers', document_id=document, layers=[1])
    group = app.call('get_document', document_id=document)['layers']
    index = next(i for i, row in enumerate(group) if row.get('is_group') or row.get('kind') == 'group')
    grouped = app.refused('frequency_separation', 'no pixels of its own', document_id=document,
                          layer=index)
    app.call('close_document', document_id=document, discard_changes=True)
    document = open_picture(app, folder, 'refuse-cmyk.png', 8, a)
    app.call('convert_mode', document_id=document, mode='cmyk')
    cmyk = app.refused('frequency_separation', '', document_id=document, layer=0)
    assert len(layers(app, document)) == 1, layers(app, document)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(clipped=clipped[:50], group=grouped[:50], cmyk=cmyk[:70])


def float_case(app, folder):
    path = Path(folder) / 'scene.tiff'
    shutil.copyfile(FLOAT_FIXTURE, path)
    document = app.call('open_document', path=str(path))['document']['id']
    info = app.call('get_document', document_id=document)
    size = (info['width'], info['height'])
    assert info.get('precision') == 'rgba32_float', info.get('precision')
    tools = app.call('get_precision', document_id=document)['supported_tools']
    assert 'frequency_separation' in tools, tools
    name = info['layers'][0]['name']
    before = stored(app, document, size)
    assert any(v > 1 for p in before for v in p[:3]), 'the fixture holds a value above white'
    assert any(v < 0 for p in before for v in p[:3]), 'the fixture holds a value below zero'
    reply = app.call('frequency_separation', document_id=document, layer=0, radius=2)
    assert reply['high_blend_mode'] == 'Linear Dodge (Add)', reply
    split_layers(app, document, name, 'Linear Dodge (Add)')
    after = stored(app, document, size)
    low = stored(app, document, size, source='layer', layer=0)
    high = stored(app, document, size, source='layer', layer=1)
    assert any(v < 0 for p in high for v in p[:3]), 'the High layer holds a negative sample'
    opaque = {i for i, (b, l) in enumerate(zip(before, low)) if b[3] == 1 and l[3] == 1}
    assert opaque, 'no opaque pixel'
    worst = float_worst(before, after, low, opaque)
    app.call('undo', document_id=document)
    assert len(layers(app, document)) == 1
    assert stored(app, document, size) == before, 'undo did not restore the layer'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(size=size, opaque=len(opaque), worst=round(worst, 3))


def linear_light_case(app, folder, depth):
    a = picture(0x77 + depth)
    document = open_picture(app, folder, f'linear-{depth}.png', depth, a)
    app.call('set_evaluation', document_id=document, policy='linear_light')
    before = codes(app, document)
    if depth == 8:
        refusal = app.call('frequency_separation', fail=True, document_id=document, layer=0)
        assert refusal == EIGHT_BIT_LINEAR, refusal
        assert len(layers(app, document)) == 1, layers(app, document)
        assert codes(app, document) == before, 'the refused split changed the document'
        app.call('close_document', document_id=document, discard_changes=True)
        return dict(refusal=refusal[:70])
    name = layers(app, document)[0]['name']
    reply = app.call('frequency_separation', document_id=document, layer=0, radius=2.5)
    assert reply['high_blend_mode'] == 'Linear Light', reply
    split_layers(app, document, name, 'Linear Light')
    after = codes(app, document)
    worst = max(abs(after[y][x][c] - before[y][x][c])
                for y in range(H) for x in range(W) for c in range(4))
    assert worst <= LINEAR_BOUND16, ('recomposed in linear light', worst)
    app.call('undo', document_id=document)
    assert codes(app, document) == before, 'undo did not restore the layer'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(worst=worst)


def prepare(app, document, mode, depth):
    """The opened RGB picture at `depth`, then in `mode`."""
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if depth == 32:
        app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    if mode == 'duotone':
        app.call('convert_mode', document_id=document, mode='gray')
        app.call('convert_mode', document_id=document, mode='duotone', inks=MONOTONE)
    else:
        app.call('convert_mode', document_id=document, mode=mode)
    rows = app.call('list_documents')['documents']
    got = next(row for row in rows if row['id'] == document)['color_mode']
    assert got == mode, (mode, depth, got)


def mode_check(app, folder, mode, depth):
    """The colour-mode drives' call (module doc, MODES). The document is closed whatever
    happens. Answers the worst error (codes, or a fraction of `float_bound` at 32 bits)."""
    document = app.call('open_document', path=str(write(folder, f'fs-{mode}-{depth}.png',
                                                         picture(0x5150 + depth))))['document']['id']
    try:
        prepare(app, document, mode, depth)
        name = layers(app, document)[0]['name']
        before = stored(app, document)
        reply = app.call('frequency_separation', document_id=document, layer=0, radius=2.5)
        blend = 'Linear Dodge (Add)' if depth == 32 else 'Linear Light'
        assert reply['high_blend_mode'] == blend, (mode, depth, reply)
        split_layers(app, document, name, blend)
        after = stored(app, document)
        if depth == 32:
            worst = round(float_worst(before, after, stored(app, document, source='layer', layer=0)), 3)
        else:
            worst = max(abs(p[c] - q[c]) for p, q in zip(after, before) for c in range(4))
            assert worst <= 1, (mode, depth, 'recomposed', worst)
        if mode in ('gray', 'duotone'):
            assert all(p[0] == p[1] == p[2] for p in after), (mode, depth, 'R = G = B')
        if mode in MENU_MODES:
            menu = app.call('get_precision', document_id=document)['supported_menu_items']
            assert 'showFrequencySeparation:' in menu, (mode, depth, menu)
        return dict(worst=worst)
    finally:
        app.call('close_document', document_id=document, discard_changes=True)


MODE_CASES = [('gray', 8), ('gray', 16), ('gray', 32), ('duotone', 8), ('lab', 8), ('lab', 16)]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='keep the run folder here')
    parser.add_argument('--only', help='run the cases whose name contains this')
    args = parser.parse_args()
    folder = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix='rz-fs-'))
    folder.mkdir(parents=True, exist_ok=True)
    failures = 0
    with App(folder / 'instance', 2 * 1024 ** 3) as app:
        cases = []
        for depth in (8, 16):
            cases.append((f'apply_image scale offset {depth}-bit',
                          lambda depth=depth: apply_image_case(app, folder, depth)))
            cases.append((f'frequency_separation {depth}-bit',
                          lambda depth=depth: split_case(app, folder, depth)))
            cases.append((f'frequency_separation under opacity {depth}-bit',
                          lambda depth=depth: masked_case(app, folder, depth)))
            cases.append((f'frequency_separation linear light {depth}-bit',
                          lambda depth=depth: linear_light_case(app, folder, depth)))
        cases.append(('frequency_separation 32-bit float', lambda: float_case(app, folder)))
        for mode, depth in MODE_CASES:
            cases.append((f'frequency_separation {mode} {depth}-bit',
                          lambda mode=mode, depth=depth: mode_check(app, folder, mode, depth)))
        cases.append(('frequency_separation refusals', lambda: refusal_case(app, folder)))
        for name, run in cases:
            if args.only and args.only not in name:
                continue
            try:
                detail = run()
                print(f'PASS {name}: {detail}', flush=True)
            except Exception as error:  # noqa: BLE001 - every case reports
                failures += 1
                print(f'FAIL {name}: {error!r}', flush=True)
    if failures:
        print(f'{failures} CASE(S) FAILED; the run folder is {folder}')
        return 1
    print('ALL CASES PASS')
    return 0


if __name__ == '__main__':
    sys.exit(main())
