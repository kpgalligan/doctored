#!/usr/bin/env python3
"""Owned-app Phase 13A-4a drive: MCP `render` with `region` / `preview_divisor`
over a guide-reading Shadows/Highlights at the ROOT at 8 and 16 bits —
admitted since 13A-4a (staged over declared planes), where it refused before.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
G1 (8 and 16 bits): a 256×192 transparent `new_document` + a canvas-sized
placed backdrop with transparent HOLES and translucent pixels + three
`add_adjustment_layer shadows_highlights` layers (radius 4 / 30 / 120), toggled
visible one at a time and then all together, each equals the crop.
G2 (8 and 16 bits): the radius-30 layer with an ellipse `add_layer_mask`
(`set_mask_properties` feather 6), opacity 0.7, Multiply, equals the crop.
G3 (8 and 16 bits): a guided layer inside a `group_layers` (Pass Through)
group equals the crop (admitted since 13A-4b-ii-a); as a clip member
(`set_layer_clipped`) of an unstyled base it equals the crop (admitted since
13A-4b-ii-b-1); as a clip BASE with a visible member it equals the crop.
G4 (8 and 16 bits): a zero-amount Shadows/Highlights equals the crop.
G5 (8 and 16 bits): an EMPTY `group_layers` group (its only child deleted,
set to Normal — isolated) below the radius-30 layer equals the crop.
F1: the float fixture refuses Shadows/Highlights by the float policy's
sentence (at the edit, or — if the edit lands — identically at the full and
the regional render).
The plan is not reachable over MCP (`render` returns the image, not the plan;
13B owns a plan surface); the Rust tests `regional_byte_guide` /
`regional_precision_guide` assert it.
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
from verify_phase13a1_regional import (FLOAT_FIXTURE, history, opaque_picture,
                                       regions_equal_crops, render, rpc, text_of)
from verify_phase13a2_masks_clips import index_named, layers_of

CW, CH = 256, 192
FLOAT_SENTENCE = 'is unavailable in a 32-bit float document'
REFUSAL = 'is Shadows/Highlights with a guide'


def holey_picture(width, height, seed):
    """An RGBA picture with transparent holes (a lattice and one block) and a
    band of translucent pixels."""
    out = []
    for y in range(height):
        for x in range(width):
            block = width // 5 < x < width // 5 + width // 3 and height // 2 < y < height // 2 + height // 3
            if block or (x // 7 + y // 5) % 9 == 0:
                alpha = 0
            elif (x + 2 * y) % 5 == 0 or y < height // 8:
                alpha = 50 + (x * 7 + y * 3) % 170
            else:
                alpha = 255
            out.extend(((x * 29 + y * 7 + seed * 41) % 256, (x * 5 + y * 23 + seed) % 256,
                        ((x ^ y) * 3 + seed * 13) % 256, alpha))
    return out


def write(path, depth, width, height, picture):
    if depth == 16:
        write_png16(path, width, height, [v * 257 for v in picture])
    else:
        write_png8(path, width, height, bytes(picture))


def place_picture(app, folder, document, depth, name, size, at, picture):
    path = folder / f'{name.lower()}-{depth}-{document}.png'
    write(path, depth, size[0], size[1], picture)
    reply = app.call('place_image', document_id=document, path=str(path),
                     storage='embedded', name=name, x=at[0], y=at[1])
    return reply['layer']


def backdrop(app, folder, depth, seed):
    """A 256×192 transparent document with a canvas-sized holey backdrop and a
    translucent raster over part of it."""
    document = app.call('new_document', width=CW, height=CH, depth=depth,
                        background='transparent')['document']['id']
    place_picture(app, folder, document, depth, 'Backdrop', (CW, CH), (0, 0),
                  holey_picture(CW, CH, seed))
    front = place_picture(app, folder, document, depth, 'Front', (120, 90), (60, 40),
                          opaque_picture(120, 90, seed + 1))
    app.call('set_layer_properties', document_id=document, index=front,
             blend_mode='Overlay', opacity=0.85)
    return document


def guide(app, document, radius, name, amounts=(0.7, 0.35)):
    params = {'shadows': {'amount': amounts[0], 'tone': 0.9},
              'highlights': {'amount': amounts[1], 'tone': 0.8}, 'radius': radius}
    app.call('add_adjustment_layer', document_id=document, op='shadows_highlights',
             params=params, name=name, position='top')
    return index_named(app, document, name)


def row_of(app, document, index):
    row = next(r for r in layers_of(app, document) if r['index'] == index)
    return {k: row.get(k) for k in ('index', 'name', 'kind', 'blend_mode', 'opacity',
                                    'clipped', 'visible') if k in row}


def visible(app, document, index, on):
    app.call('set_layer_properties', document_id=document, index=index, visible=on)


def refuses(app, document, index, name):
    before = history(app, document)
    reply = rpc(app, 'render', document_id=document, max_side=4096,
                region=dict(x=30, y=20, width=60, height=40))
    assert reply.get('isError'), reply
    message = text_of(reply)
    assert REFUSAL in message, message
    assert f'layer {index} ({name})' in message, message
    assert history(app, document) == before, 'a refused regional call changed history'
    return message


# ------------------------------------------------------------------ gates --

def g1(app, folder, depth):
    document = backdrop(app, folder, depth, 1)
    layers = [guide(app, document, r, f'SH{r}') for r in (4, 30, 120)]
    out = {'rows': [row_of(app, document, i) for i in layers]}
    for i in layers:
        visible(app, document, i, False)
    for i, r in zip(layers, (4, 30, 120)):
        visible(app, document, i, True)
        out[f'radius_{r}_alone'] = regions_equal_crops(app, document, f'G1 radius {r} {depth}')
        visible(app, document, i, False)
    for i in layers:
        visible(app, document, i, True)
    out['all_three'] = regions_equal_crops(app, document, f'G1 all three {depth}')
    return out


def g2(app, folder, depth):
    document = backdrop(app, folder, depth, 2)
    sh = guide(app, document, 30, 'SH30')
    app.call('select_ellipse', document_id=document, x=40, y=30, width=150, height=110)
    app.call('add_layer_mask', document_id=document, layer=sh, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=sh, feather=6)
    app.call('set_layer_properties', document_id=document, index=sh, opacity=0.7,
             blend_mode='Multiply')
    out = {'row': row_of(app, document, sh),
           'mask_properties': app.call('get_mask_properties', document_id=document, layer=sh)}
    out['regions'] = regions_equal_crops(app, document, f'G2 masked Multiply 0.7 {depth}')
    return out


def g3(app, folder, depth):
    out = {}
    # Inside a (Pass Through) group: staged by the root since 13A-4b-ii-a.
    document = backdrop(app, folder, depth, 3)
    sh = guide(app, document, 30, 'SH30')
    group = app.call('group_layers', document_id=document, layers=[sh], name='Group')['group']
    inner = index_named(app, document, 'SH30')
    out['in_group'] = {'group': group, 'layer': inner,
                       'regions': regions_equal_crops(app, document,
                                                      f'G3 in a Pass Through group {depth}')}
    # As a clip member of an unstyled base: staged since 13A-4b-ii-b-1.
    document = backdrop(app, folder, depth, 4)
    sh = guide(app, document, 30, 'SH30')
    app.call('set_layer_clipped', document_id=document, layer=sh, clipped=True)
    out['as_member'] = {'row': row_of(app, document, sh),
                        'regions': regions_equal_crops(app, document,
                                                       f'G3 clip member {depth}')}
    # As a clip base with a visible member: admitted, contributes nothing.
    document = backdrop(app, folder, depth, 5)
    sh = guide(app, document, 30, 'SH30')
    member = place_picture(app, folder, document, depth, 'Member', (80, 60), (90, 70),
                           opaque_picture(80, 60, 6))
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    out['as_base'] = {'rows': [row_of(app, document, sh), row_of(app, document, member)],
                      'regions': regions_equal_crops(app, document, f'G3 clip base {depth}')}
    return out


def g4(app, folder, depth):
    document = backdrop(app, folder, depth, 7)
    sh = guide(app, document, 30, 'SH0', amounts=(0, 0))
    return {'row': row_of(app, document, sh),
            'regions': regions_equal_crops(app, document, f'G4 zero amounts {depth}')}


def g5(app, folder, depth):
    document = backdrop(app, folder, depth, 8)
    kid = place_picture(app, folder, document, depth, 'Kid', (40, 30), (10, 10),
                        opaque_picture(40, 30, 9))
    group = app.call('group_layers', document_id=document, layers=[kid], name='Empty')['group']
    app.call('delete_layer', document_id=document, index=index_named(app, document, 'Kid'))
    group = index_named(app, document, 'Empty')
    app.call('set_layer_properties', document_id=document, index=group, blend_mode='Normal')
    sh = guide(app, document, 30, 'SH30')
    assert sh > group, (sh, group)
    out = {'rows': [row_of(app, document, group), row_of(app, document, sh)]}
    out['regions'] = regions_equal_crops(app, document, f'G5 empty group {depth}')
    return out


def f1(app):
    document = app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']
    # `rpc`'s own first parameter is `name`, so the tool's `name` goes direct.
    reply = app.client.rpc('tools/call', {'name': 'add_adjustment_layer', 'arguments': {
        'document_id': document, 'op': 'shadows_highlights', 'name': 'SH', 'position': 'top',
        'params': {'shadows': {'amount': 0.7}, 'radius': 30}}})
    if reply.get('isError'):
        message = text_of(reply)
        assert FLOAT_SENTENCE in message, message
        return {'refused_at': 'edit', 'message': message}
    full = rpc(app, 'render', document_id=document, max_side=4096)
    regional = rpc(app, 'render', document_id=document, max_side=4096,
                   region=dict(x=0, y=0, width=8, height=8))
    assert full.get('isError') and regional.get('isError'), (full, regional)
    assert FLOAT_SENTENCE in text_of(full), text_of(full)
    assert text_of(full) == text_of(regional), (text_of(full), text_of(regional))
    return {'refused_at': 'render', 'message': text_of(full)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {'canvas': [CW, CH]}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['G1'] = {'8': g1(app, folder, 8), '16': g1(app, folder, 16)}
            print('PASS G1 three guided Shadows/Highlights (radius 4 / 30 / 120) over a holey '
                  'backdrop, one at a time and all together — each equals the crop at 8 and '
                  '16 bits', flush=True)
            receipts['G2'] = {'8': g2(app, folder, 8), '16': g2(app, folder, 16)}
            print('PASS G2 the radius-30 guide with a feathered ellipse mask, opacity 0.7, '
                  'Multiply — equals the crop at 8 and 16 bits', flush=True)
            receipts['G3'] = {'8': g3(app, folder, 8), '16': g3(app, folder, 16)}
            print('PASS G3 a guide inside a Pass Through group, as a clip member and as a '
                  'clip base with a visible member equals the crop at 8 and 16 bits',
                  flush=True)
            receipts['G4'] = {'8': g4(app, folder, 8), '16': g4(app, folder, 16)}
            print('PASS G4 a zero-amount Shadows/Highlights equals the crop at 8 and 16 bits',
                  flush=True)
            receipts['G5'] = {'8': g5(app, folder, 8), '16': g5(app, folder, 16)}
            print('PASS G5 an empty isolated group below the radius-30 guide equals the crop '
                  'at 8 and 16 bits', flush=True)
            receipts['F1'] = f1(app)
            print('PASS F1 the float fixture refuses Shadows/Highlights by the float policy',
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
