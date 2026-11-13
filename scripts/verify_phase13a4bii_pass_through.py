#!/usr/bin/env python3
"""Owned-app Phase 13A-4b-ii-a drive: MCP `render` with `region` / `preview_divisor`
over a guide-reading Shadows/Highlights directly inside PASS THROUGH groups
(plain or gated by opacity and mask, nested, at the top level or inside an
isolated group) at 8 and 16 bits — admitted since 13A-4b-ii-a, where it
refused before.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
P1 (8 and 16 bits): a radius-30 Shadows/Highlights and a holey raster placed at
an odd offset, grouped by a default `group_layers` (Pass Through), equal the
crop.
P2 (8 and 16 bits): that group at opacity 0.6 (`set_layer_properties`) with an
ellipse `add_layer_mask` (`set_mask_properties` feather 6) equals the crop.
P3 (8 and 16 bits): the P1 group inside a Normal (isolated) group equals the
crop; a Normal group holding a guide (and a raster) inside a Pass Through group
beside a second guide equals the crop.
P4 (8 and 16 bits): a guide clipped (`set_layer_clipped`) to a raster inside a
Pass Through group equals the crop (admitted since 13A-4b-ii-b-1).
F1: the float fixture refuses Shadows/Highlights by the float policy's
sentence.
The plan is asserted by the Rust tests `regional_byte_pass_through_guide` /
`regional_precision_pass_through_guide` (`render` reports it since 13B-7).
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
from verify_phase13a3_groups import is_pass_through
from verify_phase13a4_guide import backdrop, f1, guide, holey_picture, place_picture, row_of
from verify_phase13a4b_nested_guide import isolated_group


def pass_group(app, folder, depth, seed, name='Pass'):
    """The backdrop, then a default (Pass Through) `group_layers` group holding
    a holey raster at an odd offset and a radius-30 guide; its index."""
    document = backdrop(app, folder, depth, seed)
    inner = place_picture(app, folder, document, depth, f'Inner{seed}', (150, 110), (37, 29),
                          holey_picture(150, 110, seed + 1))
    sh = guide(app, document, 30, f'SH{seed}')
    app.call('group_layers', document_id=document, layers=[inner, sh], name=name)
    g = index_named(app, document, name)
    row = next(r for r in layers_of(app, document) if r['index'] == g)
    assert is_pass_through(row), row
    return document, g


def rows(app, document):
    return [row_of(app, document, r['index']) for r in layers_of(app, document)]


# ------------------------------------------------------------------ gates --

def p1(app, folder, depth):
    document, g = pass_group(app, folder, depth, 11)
    return {'rows': rows(app, document),
            'regions': regions_equal_crops(app, document, f'P1 guide in a Pass Through {depth}')}


def p2(app, folder, depth):
    document, g = pass_group(app, folder, depth, 21)
    app.call('set_layer_properties', document_id=document, index=g, opacity=0.6)
    app.call('select_ellipse', document_id=document, x=40, y=30, width=150, height=110)
    app.call('add_layer_mask', document_id=document, layer=g, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=g, feather=6)
    row = next(r for r in layers_of(app, document) if r['index'] == g)
    assert is_pass_through(row), row
    return {'row': row_of(app, document, g),
            'mask_properties': app.call('get_mask_properties', document_id=document, layer=g),
            'regions': regions_equal_crops(app, document,
                                           f'P2 gated Pass Through 0.6 + feather 6 {depth}')}


def p3(app, folder, depth):
    out = {}
    document, g = pass_group(app, folder, depth, 31)
    isolated_group(app, document, [g], 'Normal')
    out['in_normal'] = {'rows': rows(app, document),
                        'regions': regions_equal_crops(
                            app, document, f'P3 Pass Through in a Normal group {depth}')}
    document = backdrop(app, folder, depth, 32)
    a = place_picture(app, folder, document, depth, 'A', (170, 130), (21, 17),
                      holey_picture(170, 130, 33))
    sh8 = guide(app, document, 8, 'SH8')
    inner = isolated_group(app, document, [a, sh8], 'Inner')
    sh30 = guide(app, document, 30, 'SH30')
    inner = index_named(app, document, 'Inner')
    app.call('group_layers', document_id=document, layers=[inner, sh30], name='Outer')
    outer = index_named(app, document, 'Outer')
    row = next(r for r in layers_of(app, document) if r['index'] == outer)
    assert is_pass_through(row), row
    out['normal_in_pass'] = {'rows': rows(app, document),
                             'regions': regions_equal_crops(
                                 app, document, f'P3 Normal group in a Pass Through {depth}')}
    return out


def p4(app, folder, depth):
    document = backdrop(app, folder, depth, 41)
    base = place_picture(app, folder, document, depth, 'Base', (120, 90), (40, 30),
                         opaque_picture(120, 90, 42))
    sh = guide(app, document, 30, 'SH30')
    app.call('set_layer_clipped', document_id=document, layer=sh, clipped=True)
    app.call('group_layers', document_id=document, layers=[base, sh], name='Pass')
    g = index_named(app, document, 'Pass')
    inner = index_named(app, document, 'SH30')
    row = next(r for r in layers_of(app, document) if r['index'] == g)
    assert is_pass_through(row), row
    return {'group': row_of(app, document, g), 'row': row_of(app, document, inner),
            'regions': regions_equal_crops(app, document,
                                           f'P4 clip member in a Pass Through {depth}')}


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
            receipts['P1'] = {'8': p1(app, folder, 8), '16': p1(app, folder, 16)}
            print('PASS P1 a radius-30 guide and a raster in a Pass Through group equal the '
                  'crop at 8 and 16 bits', flush=True)
            receipts['P2'] = {'8': p2(app, folder, 8), '16': p2(app, folder, 16)}
            print('PASS P2 that group at opacity 0.6 with a feathered ellipse mask equals the '
                  'crop at 8 and 16 bits', flush=True)
            receipts['P3'] = {'8': p3(app, folder, 8), '16': p3(app, folder, 16)}
            print('PASS P3 the group inside a Normal group, and a guided Normal group inside a '
                  'Pass Through group beside a guide, equal the crop at 8 and 16 bits',
                  flush=True)
            receipts['P4'] = {'8': p4(app, folder, 8), '16': p4(app, folder, 16)}
            print('PASS P4 a guide clipped inside a Pass Through group equals the crop at 8 '
                  'and 16 bits', flush=True)
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
