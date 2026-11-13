#!/usr/bin/env python3
"""Owned-app Phase 13A-4b-i drive: MCP `render` with `region` / `preview_divisor`
over a guide-reading Shadows/Highlights inside ISOLATED groups (nested) and
inside groups with style effects (materialized) at 8 and 16 bits — admitted
since 13A-4b-i, where they refused before.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
N1 (8 and 16 bits): `group_layers` + `set_layer_properties blend_mode Normal`
around a radius-30 Shadows/Highlights and a holey raster placed at an odd
offset equals the crop.
N2 (8 and 16 bits): NESTED — an isolated group (radius 8 guide + a raster)
inside an isolated group (radius 30 guide above it + a raster) equals the
crop, with and without a root guide on top.
N3 (8 and 16 bits): the N1 group as a clip BASE with a visible member; then
with an ellipse `add_layer_mask` (`set_mask_properties` feather 6), opacity
0.7, Multiply — each equals the crop.
N4 (8 and 16 bits): a guide inside a Pass Through `group_layers` group equals
the crop (admitted since 13A-4b-ii-a); a guide as a clip member
(`set_layer_clipped`) of an unstyled base equals the crop (admitted since
13A-4b-ii-b-1).
N5 (8 and 16 bits): a `set_layer_style` drop-shadow group holding the guide,
and one holding a Normal group that holds the guide, each equal the crop.
F1: the float fixture refuses Shadows/Highlights by the float policy's
sentence.
The plan is asserted by the Rust tests `regional_byte_nested_guide` /
`regional_precision_nested_guide` (`render` reports it since 13B-7).
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

from support import HostApp
from verify_phase13a1_regional import opaque_picture, regions_equal_crops
from verify_phase13a2_masks_clips import index_named, layers_of
from verify_phase13a4_guide import backdrop, f1, guide, holey_picture, place_picture, row_of

SHADOW = {'effects': [{'type': 'drop_shadow', 'distance': 5, 'size': 3}]}


def isolated_group(app, document, layers, name):
    """`group_layers` over `layers`, then Normal (isolated); its index."""
    app.call('group_layers', document_id=document, layers=layers, name=name)
    g = index_named(app, document, name)
    app.call('set_layer_properties', document_id=document, index=g, blend_mode='Normal')
    return g


def guided_group(app, folder, depth, seed, name='Group'):
    """The backdrop, then an isolated group holding a holey raster at an odd
    offset and a radius-30 guide."""
    document = backdrop(app, folder, depth, seed)
    inner = place_picture(app, folder, document, depth, f'Inner{seed}', (150, 110), (37, 29),
                          holey_picture(150, 110, seed + 1))
    sh = guide(app, document, 30, f'SH{seed}')
    g = isolated_group(app, document, [inner, sh], name)
    return document, g


def rows(app, document):
    return [row_of(app, document, r['index']) for r in layers_of(app, document)]


# ------------------------------------------------------------------ gates --

def n1(app, folder, depth):
    document, g = guided_group(app, folder, depth, 11)
    return {'rows': rows(app, document),
            'regions': regions_equal_crops(app, document, f'N1 guide in a Normal group {depth}')}


def n2(app, folder, depth):
    document = backdrop(app, folder, depth, 21)
    a = place_picture(app, folder, document, depth, 'A', (170, 130), (21, 17),
                      holey_picture(170, 130, 22))
    b = place_picture(app, folder, document, depth, 'B', (90, 70), (51, 43),
                      holey_picture(90, 70, 23))
    sh8 = guide(app, document, 8, 'SH8')
    inner = isolated_group(app, document, [b, sh8], 'Inner')
    sh30 = guide(app, document, 30, 'SH30')
    a = index_named(app, document, 'A')
    inner = index_named(app, document, 'Inner')
    isolated_group(app, document, [a, inner, sh30], 'Outer')
    out = {'rows': rows(app, document),
           'nested': regions_equal_crops(app, document, f'N2 nested {depth}')}
    guide(app, document, 4, 'SHroot')
    out['rooted'] = regions_equal_crops(app, document, f'N2 nested under a root guide {depth}')
    return out


def n3(app, folder, depth):
    out = {}
    document, g = guided_group(app, folder, depth, 31)
    member = place_picture(app, folder, document, depth, 'Member', (80, 60), (90, 70),
                           opaque_picture(80, 60, 32))
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    out['clip_base'] = {'rows': rows(app, document),
                        'regions': regions_equal_crops(app, document,
                                                       f'N3 group as a clip base {depth}')}
    document, g = guided_group(app, folder, depth, 33)
    app.call('select_ellipse', document_id=document, x=40, y=30, width=150, height=110)
    app.call('add_layer_mask', document_id=document, layer=g, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=g, feather=6)
    app.call('set_layer_properties', document_id=document, index=g, opacity=0.7,
             blend_mode='Multiply')
    out['masked'] = {'row': row_of(app, document, g),
                     'mask_properties': app.call('get_mask_properties', document_id=document,
                                                 layer=g),
                     'regions': regions_equal_crops(app, document,
                                                    f'N3 masked Multiply 0.7 {depth}')}
    return out


def n4(app, folder, depth):
    out = {}
    document = backdrop(app, folder, depth, 41)
    sh = guide(app, document, 30, 'SH30')
    group = app.call('group_layers', document_id=document, layers=[sh], name='Pass')['group']
    inner = index_named(app, document, 'SH30')
    out['in_pass_through'] = {'group': row_of(app, document, group), 'layer': inner,
                              'regions': regions_equal_crops(
                                  app, document, f'N4 in a Pass Through group {depth}')}
    document = backdrop(app, folder, depth, 42)
    sh = guide(app, document, 30, 'SH30')
    app.call('set_layer_clipped', document_id=document, layer=sh, clipped=True)
    out['as_member'] = {'row': row_of(app, document, sh),
                        'regions': regions_equal_crops(app, document,
                                                       f'N4 clip member {depth}')}
    return out


def n5(app, folder, depth):
    out = {}
    document, g = guided_group(app, folder, depth, 51)
    app.call('set_layer_style', document_id=document, layer=g, style=SHADOW)
    out['styled'] = {'row': row_of(app, document, g),
                     'regions': regions_equal_crops(app, document,
                                                    f'N5 guide in a styled group {depth}')}
    document, g = guided_group(app, folder, depth, 52)
    outer = isolated_group(app, document, [g], 'Styled')
    app.call('set_layer_style', document_id=document, layer=outer, style=SHADOW)
    out['styled_over_normal'] = {
        'rows': rows(app, document),
        'regions': regions_equal_crops(app, document,
                                       f'N5 guide in a Normal group in a styled group {depth}')}
    return out


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
            receipts['N1'] = {'8': n1(app, folder, 8), '16': n1(app, folder, 16)}
            print('PASS N1 a radius-30 guide and a raster in a Normal (isolated) group equal '
                  'the crop at 8 and 16 bits', flush=True)
            receipts['N2'] = {'8': n2(app, folder, 8), '16': n2(app, folder, 16)}
            print('PASS N2 nested isolated groups each with a guide, with and without a root '
                  'guide, equal the crop at 8 and 16 bits', flush=True)
            receipts['N3'] = {'8': n3(app, folder, 8), '16': n3(app, folder, 16)}
            print('PASS N3 the guided group as a clip base, and masked (feather 6) at 0.7 '
                  'Multiply, equals the crop at 8 and 16 bits', flush=True)
            receipts['N4'] = {'8': n4(app, folder, 8), '16': n4(app, folder, 16)}
            print('PASS N4 a guide inside a Pass Through group and as a clip member equals '
                  'the crop at 8 and 16 bits', flush=True)
            receipts['N5'] = {'8': n5(app, folder, 8), '16': n5(app, folder, 16)}
            print('PASS N5 a drop-shadow group holding the guide, and holding a Normal group '
                  'that holds it, equal the crop at 8 and 16 bits', flush=True)
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
