#!/usr/bin/env python3
"""Owned-app Phase 13A-3b-ii drive: MCP `render` with `region` / `preview_divisor`
over styled GROUPS WITH ENABLED EFFECTS at 8 and 16 bits (the materialized
group), and — admitted since 13A-3c — a knockout beside an effect on a
group.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
SG1 (8 and 16 bits): `group_layers` over two `place_image` rasters (the group
set to Normal at 16 bits) + `set_layer_style {drop_shadow, stroke,
color_overlay}` on the GROUP equals the crop; then `set_layer_properties`
opacity 0.6 (the package model); then `blend_mode: "Multiply"`; then an
ellipse `add_layer_mask` (`select_ellipse` feather 6, `from_selection`,
`set_mask_properties` feather 6).
SG2 (8 and 16 bits): the styled group as a clip base with a visible clipped
member equals the crop.
SG3 (8 and 16 bits): the styled group nested in an outer group equals the
crop.
SG4 (8 and 16 bits): `{knockout: "shallow", effects}` on the group — refused
"carries a knockout" until 13A-3c — equals the crop.
The plan's `global_analysis` (`"group_materialization"`) and
`required_input_region` are not reachable over MCP (`render` returns the
image, not the plan); the Rust tests `regional_byte_styled_groups` /
`regional_precision_styled_groups` assert them.
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
from verify_phase13a1_regional import H, W, regions_equal_crops
from verify_phase13a2_masks_clips import blank, layers_of, place

EFFECTS = [
    {'type': 'drop_shadow', 'distance': 5, 'size': 3},
    {'type': 'stroke', 'size': 3, 'position': 'outside', 'color': '#ff2000'},
    {'type': 'color_overlay', 'blend': 'multiply', 'color': '#3080c0', 'opacity': 0.7},
]


def group(app, document, layers, name):
    return app.call('group_layers', document_id=document, layers=layers, name=name)['group']


def row_of(app, document, index):
    row = next(r for r in layers_of(app, document) if r['index'] == index)
    return {k: row.get(k) for k in ('index', 'name', 'kind', 'blend_mode', 'opacity',
                                    'clipped', 'visible', 'style') if k in row}


def styled_group(app, folder, depth, seed):
    """Two placed rasters grouped; at 16 bits the group is set to Normal (a
    typed styled group cannot be Pass Through); the group carries EFFECTS."""
    document = blank(app, depth)
    a = place(app, folder, document, depth, 'A', (40, 30), (20, 10), seed)
    b = place(app, folder, document, depth, 'B', (30, 20), (45, 30), seed + 1)
    g = group(app, document, [a, b], 'Group')
    if depth == 16:
        app.call('set_layer_properties', document_id=document, index=g, blend_mode='Normal')
    app.call('set_layer_style', document_id=document, layer=g, style={'effects': EFFECTS})
    return document, g


# ------------------------------------------------------------------ gates --

def sg1(app, folder, depth):
    document, g = styled_group(app, folder, depth, 1)
    out = {'group': g, 'row': row_of(app, document, g)}
    out['styled'] = regions_equal_crops(app, document, f'SG1 effects {depth}')
    app.call('set_layer_properties', document_id=document, index=g, opacity=0.6)
    out['opacity'] = regions_equal_crops(app, document, f'SG1 opacity 0.6 {depth}')
    app.call('set_layer_properties', document_id=document, index=g, blend_mode='Multiply')
    out['multiply_row'] = row_of(app, document, g)
    out['multiply'] = regions_equal_crops(app, document, f'SG1 Multiply 0.6 {depth}')
    app.call('select_ellipse', document_id=document, x=10, y=6, width=52, height=40,
             feather=6)
    app.call('add_layer_mask', document_id=document, layer=g, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=g, feather=6)
    out['mask_properties'] = app.call('get_mask_properties', document_id=document, layer=g)
    out['masked'] = regions_equal_crops(app, document, f'SG1 masked {depth}')
    return out


def sg2(app, folder, depth):
    document, g = styled_group(app, folder, depth, 3)
    member = place(app, folder, document, depth, 'Member', (40, 30), (40, 24), 5)
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    out = {'rows': [row_of(app, document, g), row_of(app, document, member)]}
    assert out['rows'][1].get('clipped') is True, out['rows']
    assert out['rows'][1].get('visible') is True, out['rows']
    out['regions'] = regions_equal_crops(app, document, f'SG2 styled group base {depth}')
    return out


def sg3(app, folder, depth):
    document, g = styled_group(app, folder, depth, 6)
    outer = group(app, document, [g], 'Outer')
    out = {'outer': outer, 'rows': [row_of(app, document, r['index'])
                                    for r in layers_of(app, document)]}
    out['regions'] = regions_equal_crops(app, document, f'SG3 nested {depth}')
    return out


def sg4(app, folder):
    out = {}
    for depth in (8, 16):
        document, g = styled_group(app, folder, depth, 8)
        app.call('set_layer_style', document_id=document, layer=g,
                 style={'knockout': 'shallow', 'effects': EFFECTS})
        # Admitted since 13A-3c: the knockout group equals the crop.
        out[str(depth)] = {'group': g, 'row': row_of(app, document, g),
                           'regions': regions_equal_crops(app, document,
                                                          f'SG4 knockout group {depth}')}
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {'canvas': [W, H]}
    executable = Path(args.app).expanduser().resolve()
    if executable.suffix == '.app':
        executable = executable / 'Contents/MacOS/Doctored'
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['SG1'] = {'8': sg1(app, folder, 8), '16': sg1(app, folder, 16)}
            print('PASS SG1 a group with drop shadow, stroke and colour overlay, then '
                  'opacity 0.6, then Multiply, then a feathered ellipse mask — each equals '
                  'the crop at 8 and 16 bits', flush=True)
            receipts['SG2'] = {'8': sg2(app, folder, 8), '16': sg2(app, folder, 16)}
            print('PASS SG2 the styled group as a clip base with a visible member equals '
                  'the crop at 8 and 16 bits', flush=True)
            receipts['SG3'] = {'8': sg3(app, folder, 8), '16': sg3(app, folder, 16)}
            print('PASS SG3 the styled group nested in an outer group equals the crop at '
                  '8 and 16 bits', flush=True)
            receipts['SG4'] = sg4(app, folder)
            print('PASS SG4 a knockout beside the effects on the group equals the crop at '
                  '8 and 16 bits', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
