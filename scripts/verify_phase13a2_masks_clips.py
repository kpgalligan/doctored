#!/usr/bin/env python3
"""Owned-app Phase 13A-2a drive: MCP `render` with `region` / `preview_divisor`
over LAYER MASKS, CLIP UNITS and typed BLEND IF at 8 bits, 16 bits and float.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
M1 8 bits: the 13A-1 stack with `add_adjustment_layer`'s automatic reveal-all
mask left ENABLED equals the crop; then `select_ellipse` (feather 6) +
`add_layer_mask from_selection` on Front + `set_mask_properties` (density 0.5,
feather 8) equals the crop; M2 the same at 16 bits.
C1 (8 bits) / C2 (16 bits): `set_layer_clipped` over a Multiply 0.6 base with a
visible and a HIDDEN member, and index 0 clipped (baseless), equals the crop.
C3: an Invert adjustment base whose ONLY member is hidden equals the crop at 8
and 16 bits — at 8 bits the unit contributes nothing (the full render equals
the one before the adjustment), at 16 bits the Invert applies (it differs).
F1: the float fixture + `new_layer` + `add_layer_mask` + `set_layer_clipped`.
B1: 16 bits `set_layer_style` with `blend_if` only equals the crop.
B2: 8 bits `set_layer_style` with `blend_if` only equals the crop (byte Blend
If is admitted since 13A-2b; `verify_phase13a2b_byte_styles.py` owns the rest).
N1: `{"region": 5, "layer": 0}` answers the flattened-canvas-only sentence
(the guard runs before the region parses).
A failure exits nonzero and leaves a JSON receipt in --receipts.
"""
import argparse
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import write_png8, write_png16
from support import HostApp
from verify_phase13a1_regional import (FLOAT_FIXTURE, H, W, history, opaque_picture,
                                       regions_equal_crops, render, rpc, stacked,
                                       text_of)

BLEND_IF = {'channel': 'gray', 'this_layer': [10, 60, 180, 240],
            'underlying': [0, 40, 200, 255]}


def layers_of(app, document):
    return app.call('get_document', document_id=document)['layers']


def index_named(app, document, name):
    return next(row['index'] for row in layers_of(app, document) if row['name'] == name)


def place(app, folder, document, depth, name, size, at, seed):
    w, h = size
    path = folder / f'{name.lower()}-{depth}-{document}.png'
    picture = opaque_picture(w, h, seed)
    if depth == 16:
        write_png16(path, w, h, [v * 257 for v in picture])
    else:
        write_png8(path, w, h, bytes(picture))
    reply = app.call('place_image', document_id=document, path=str(path),
                     storage='embedded', name=name, x=at[0], y=at[1])
    return reply['layer']


def blank(app, depth):
    return app.call('new_document', width=W, height=H, depth=depth,
                    background='white')['document']['id']


# ------------------------------------------------------------------ gates --

def m1_m2(app, folder, depth):
    document, _ = stacked(app, folder, depth)
    hue = index_named(app, document, 'Hue')
    row = next(r for r in layers_of(app, document) if r['index'] == hue)
    out = {'document': document, 'hue_row': row,
           'auto_mask': regions_equal_crops(app, document, f'M auto mask {depth}')}
    front = index_named(app, document, 'Front')
    app.call('select_ellipse', document_id=document, x=10, y=6, width=52, height=40,
             feather=6)
    app.call('add_layer_mask', document_id=document, layer=front, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=front, density=0.5,
             feather=8)
    out['mask_properties'] = app.call('get_mask_properties', document_id=document,
                                      layer=front)
    out['feathered'] = regions_equal_crops(app, document, f'M feathered mask {depth}')
    return out


def c1_c2(app, folder, depth):
    document = blank(app, depth)
    front = place(app, folder, document, depth, 'Front', (40, 30), (20, 10), 1)
    member = place(app, folder, document, depth, 'Member', (40, 30), (40, 24), 2)
    hidden = place(app, folder, document, depth, 'Hidden', (30, 20), (8, 6), 3)
    app.call('set_layer_properties', document_id=document, index=front,
             blend_mode='Multiply', opacity=0.6)
    app.call('set_layer_clipped', document_id=document, layers=[member, hidden], clipped=True)
    app.call('set_layer_properties', document_id=document, index=hidden, visible=False)
    app.call('set_layer_clipped', document_id=document, layer=0, clipped=True)
    rows = layers_of(app, document)
    out = {'rows': [{k: r.get(k) for k in ('index', 'name', 'clipped', 'visible',
                                           'blend_mode', 'opacity')} for r in rows]}
    out['regions'] = regions_equal_crops(app, document, f'C clip units {depth}')
    return out


def c3(app, folder, depth):
    document = blank(app, depth)
    place(app, folder, document, depth, 'Picture', (W, H), (0, 0), 4)
    before, _ = render(app, document)
    app.call('add_adjustment_layer', document_id=document, op='invert', params={},
             name='Invert')
    member = place(app, folder, document, depth, 'Member', (40, 30), (20, 10), 5)
    invert = index_named(app, document, 'Invert')
    assert member == invert + 1, (member, invert)
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    app.call('set_layer_properties', document_id=document, index=member, visible=False)
    after, _ = render(app, document)
    unchanged = list(after['samples']) == list(before['samples'])
    if depth == 8:
        assert unchanged, 'at 8 bits the hidden-only clip unit contributes nothing'
    else:
        assert not unchanged, 'at 16 bits the Invert applies (the run is empty)'
    out = {'depth': depth, 'full_render_unchanged_by_the_unit': unchanged}
    out['regions'] = regions_equal_crops(app, document, f'C3 hidden-only run {depth}')
    return out


def f1(app):
    document = app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']
    app.call('new_layer', document_id=document, name='New')
    new = index_named(app, document, 'New')
    app.call('add_layer_mask', document_id=document, layer=new, kind='hide_all')
    app.call('set_layer_clipped', document_id=document, layer=new, clipped=True)
    out = {'precision': app.call('get_document', document_id=document).get('precision'),
           'layer': new}
    out['regions'] = regions_equal_crops(app, document, 'F1 float')
    return out


def b1(app, folder):
    document = blank(app, 16)
    top = place(app, folder, document, 16, 'Top', (50, 40), (15, 9), 6)
    app.call('set_layer_style', document_id=document, layer=top,
             style={'fill_opacity': 1, 'blend_if': BLEND_IF, 'effects': []})
    return {'regions': regions_equal_crops(app, document, 'B1 Blend If 16')}


def b2(app, folder):
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (50, 40), (15, 9), 6)
    app.call('set_layer_style', document_id=document, layer=top,
             style={'fill_opacity': 1, 'blend_if': BLEND_IF, 'effects': []})
    return {'layer': top, 'regions': regions_equal_crops(app, document, 'B2 Blend If 8')}


def n1(app, document):
    before = history(app, document)
    reply = rpc(app, 'render', document_id=document, max_side=4096, region=5, layer=0)
    assert reply.get('isError'), reply
    message = text_of(reply)
    assert 'flattened canvas only' in message, message
    assert history(app, document) == before
    return {'message': message}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['M1'] = m1_m2(app, folder, 8)
            print('PASS M1 8-bit auto mask and feathered selection mask equal the crop',
                  flush=True)
            receipts['M2'] = m1_m2(app, folder, 16)
            print('PASS M2 16-bit masks equal the crop', flush=True)
            receipts['C1'] = c1_c2(app, folder, 8)
            print('PASS C1 8-bit clip unit, hidden member, baseless index 0', flush=True)
            receipts['C2'] = c1_c2(app, folder, 16)
            print('PASS C2 16-bit clip unit, hidden member, baseless index 0', flush=True)
            receipts['C3'] = [c3(app, folder, 8), c3(app, folder, 16)]
            print('PASS C3 hidden-only run: 8 bits contributes nothing, 16 bits applies; '
                  'each equals its own crop', flush=True)
            receipts['F1'] = f1(app)
            print('PASS F1 float fixture + new_layer + mask + clip', flush=True)
            receipts['B1'] = b1(app, folder)
            print('PASS B1 16-bit Blend If equals the crop', flush=True)
            receipts['B2'] = b2(app, folder)
            print('PASS B2 8-bit Blend If equals the crop (13A-2b)', flush=True)
            receipts['N1'] = n1(app, receipts['M1']['document'])
            print('PASS N1 a malformed region beside layer names the flattened canvas',
                  flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
