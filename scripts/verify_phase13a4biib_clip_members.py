#!/usr/bin/env python3
"""Owned-app Phase 13A-4b-ii-b-1 drive: MCP `render` with `region` /
`preview_divisor` over a guide-reading Shadows/Highlights as a CLIP MEMBER of
an unstyled base at 8 and 16 bits — admitted since 13A-4b-ii-b-1, where it
refused before.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
C1 (8 and 16 bits): a radius-30 Shadows/Highlights `set_layer_clipped` to a
holey raster placed at the odd offset (37, 29) equals the crop.
C2 (8 and 16 bits): that clip unit inside a default `group_layers` group (Pass
Through) equals the crop, and inside a Normal (isolated) group equals the
crop.
C3 (8 and 16 bits): a raster member clipped above the guide, and the guide at
opacity 0.6 Multiply, equal the crop.
C4 (8 and 16 bits): the base given `set_layer_style`
(`{"effects":[{"type":"drop_shadow"}]}`) equals the crop (13A-4b-ii-b-2 admits
a styled base; it refused "… under a styled base" before).
F1: the float fixture refuses Shadows/Highlights by the float policy's
sentence.
The plan is asserted by the Rust tests `regional_byte_clip_guide` /
`regional_precision_clip_guide` (`render` reports it since 13B-7).
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

def clip_unit(app, folder, depth, seed):
    """The backdrop, then a holey raster at (37, 29) and a radius-30 guide
    clipped to it; the document and the two layers' names."""
    document = backdrop(app, folder, depth, seed)
    place_picture(app, folder, document, depth, f'Base{seed}', (150, 110), (37, 29),
                  holey_picture(150, 110, seed + 1))
    sh = guide(app, document, 30, f'SH{seed}')
    app.call('set_layer_clipped', document_id=document, layer=sh, clipped=True)
    return document, f'Base{seed}', f'SH{seed}'


def rows(app, document):
    return [row_of(app, document, r['index']) for r in layers_of(app, document)]


def clipped_row(app, document, name):
    row = row_of(app, document, index_named(app, document, name))
    assert row.get('clipped') is True, row
    return row


# ------------------------------------------------------------------ gates --

def c1(app, folder, depth):
    document, _, sh = clip_unit(app, folder, depth, 11)
    return {'guide': clipped_row(app, document, sh), 'rows': rows(app, document),
            'regions': regions_equal_crops(app, document,
                                           f'C1 guide clipped to a raster {depth}')}


def c2(app, folder, depth):
    out = {}
    document, base, sh = clip_unit(app, folder, depth, 21)
    layers = [index_named(app, document, base), index_named(app, document, sh)]
    app.call('group_layers', document_id=document, layers=layers, name='Pass')
    g = index_named(app, document, 'Pass')
    row = next(r for r in layers_of(app, document) if r['index'] == g)
    assert is_pass_through(row), row
    out['in_pass_through'] = {'guide': clipped_row(app, document, sh),
                              'rows': rows(app, document),
                              'regions': regions_equal_crops(
                                  app, document, f'C2 clip unit in a Pass Through {depth}')}
    document, base, sh = clip_unit(app, folder, depth, 22)
    layers = [index_named(app, document, base), index_named(app, document, sh)]
    isolated_group(app, document, layers, 'Normal')
    out['in_normal'] = {'guide': clipped_row(app, document, sh), 'rows': rows(app, document),
                        'regions': regions_equal_crops(
                            app, document, f'C2 clip unit in a Normal group {depth}')}
    return out


def c3(app, folder, depth):
    document, _, sh = clip_unit(app, folder, depth, 31)
    member = place_picture(app, folder, document, depth, 'Member', (80, 60), (90, 70),
                           opaque_picture(80, 60, 32))
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    app.call('set_layer_properties', document_id=document, index=index_named(app, document, sh),
             opacity=0.6, blend_mode='Multiply')
    return {'guide': clipped_row(app, document, sh),
            'member': clipped_row(app, document, 'Member'), 'rows': rows(app, document),
            'regions': regions_equal_crops(app, document,
                                           f'C3 member above, guide 0.6 Multiply {depth}')}


def c4(app, folder, depth):
    document, base, sh = clip_unit(app, folder, depth, 41)
    app.call('set_layer_style', document_id=document, layer=index_named(app, document, base),
             style={'effects': [{'type': 'drop_shadow'}]})
    return {'guide': clipped_row(app, document, sh), 'rows': rows(app, document),
            'regions': regions_equal_crops(app, document,
                                           f'C4 guide clipped to a styled base {depth}')}


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
            receipts['C1'] = {'8': c1(app, folder, 8), '16': c1(app, folder, 16)}
            print('PASS C1 a radius-30 guide clipped to a holey raster equals the crop at 8 '
                  'and 16 bits', flush=True)
            receipts['C2'] = {'8': c2(app, folder, 8), '16': c2(app, folder, 16)}
            print('PASS C2 that clip unit inside a Pass Through group and inside a Normal group '
                  'equals the crop at 8 and 16 bits', flush=True)
            receipts['C3'] = {'8': c3(app, folder, 8), '16': c3(app, folder, 16)}
            print('PASS C3 a raster member above the guide, the guide at 0.6 Multiply, equals '
                  'the crop at 8 and 16 bits', flush=True)
            receipts['C4'] = {'8': c4(app, folder, 8), '16': c4(app, folder, 16)}
            print('PASS C4 a drop-shadow base equals the crop at 8 and 16 bits (13A-4b-ii-b-2)',
                  flush=True)
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
