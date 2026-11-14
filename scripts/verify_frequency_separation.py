#!/usr/bin/env python3
"""MCP drive for Apply Image's Scale and Offset and Filters > Other >
Frequency Separation (#17), at 8 and 16 bits.

The script owns one app instance (`verify_v1_lens.App`): the Debug app (or
`DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s.
It terminates only that process. Every expected number is computed here from
codes the app reports, never from the app's own arithmetic.

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
the mask and the opacity. A group, a clipped layer and a 32-bit float
document refuse, and so does a document evaluated in linear light at 8 and
16 bits, its single layer left as it was: the detail is Subtract on stored
codes, which the Linear Light blend on decoded components does not undo.

Usage: python3 scripts/verify_frequency_separation.py [--output DIR] [--only NAME]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_lens import App  # noqa: E402

W, H = 24, 20


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


def agrees(got, exact, hair):
    rounded = int(exact + 0.5)
    if got == rounded:
        return True
    return abs(exact - int(exact) - 0.5) < hair and abs(got - exact) < 1


def open_picture(app, folder, name, depth, pixels):
    document = app.call('open_document', path=str(write(folder, name, pixels)))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


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
    rows = layers(app, document)
    assert len(rows) == 2, rows
    assert rows[0]['name'] == name + ' Low' and rows[1]['name'] == name + ' High', rows
    assert rows[1]['clipped'] is True, rows[1]
    assert rows[1]['blend_mode'] == 'Linear Light', rows[1]
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
    document = open_picture(app, folder, 'refuse-float.png', 8, a)
    app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    floated = app.refused('frequency_separation', '', document_id=document, layer=0)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(clipped=clipped[:50], group=grouped[:50], float=floated[:60])


def linear_light_case(app, folder, depth):
    a = picture(0x77 + depth)
    document = open_picture(app, folder, f'linear-{depth}.png', depth, a)
    app.call('set_evaluation', document_id=document, policy='linear_light')
    before = codes(app, document)
    refusal = app.refused('frequency_separation', 'linear light', 'Encoded', document_id=document,
                          layer=0)
    assert len(layers(app, document)) == 1, layers(app, document)
    assert codes(app, document) == before, 'the refused split changed the document'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(refusal=refusal[:70])


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
