#!/usr/bin/env python3
"""Owned-app Phase 13A-3b-i drive: MCP `render` with `region` / `preview_divisor`
over LAYER STYLES WITH ENABLED EFFECTS on a raster at 8 and 16 bits, a typed
effect-less styled group, and the 13A-3b-i refusals (admitted since
13A-3b-ii / 13A-3c).

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
E1 (8 and 16 bits): a lone `place_image` raster + `set_layer_style
{drop_shadow, stroke, color_overlay}` equals the crop; then
`set_layer_properties` opacity 0.6 (the package model); then `blend_mode:
"Multiply"`.
E2 (8 and 16 bits): a styled clip base `{effects, fill_opacity: 0.3,
blend_if}` with a visible clipped member equals the crop.
E3 (16 bits): a fill-only styled Normal group equals the crop.
E4: `{knockout: "shallow"}` and `{blend_interior_as_group: true, effects:
[color_overlay]}` — refused "carries a knockout" / "blends interior effects as
a group" until 13A-3c — each equal the crop; an 8-bit group with a drop shadow
(refused "is a group with layer-style effects" until 13A-3b-ii) equals the
crop.
The plan's `global_analysis` / `required_input_region` are asserted by the
Rust tests `regional_byte_effects` / `regional_precision_styles`; `render`
reports them over MCP since 13B-7 (scripts/verify_regional_plan.py).
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
from verify_phase13a2_masks_clips import BLEND_IF, blank, layers_of, place

EFFECTS = [
    {'type': 'drop_shadow', 'distance': 5, 'size': 3},
    {'type': 'stroke', 'size': 3, 'position': 'outside', 'color': '#ff2000'},
    {'type': 'color_overlay', 'blend': 'multiply', 'color': '#3080c0', 'opacity': 0.7},
]


def styled(app, document, layer, style):
    app.call('set_layer_style', document_id=document, layer=layer, style=style)


def group(app, document, layers, name):
    return app.call('group_layers', document_id=document, layers=layers, name=name)['group']


def row_of(app, document, index):
    row = next(r for r in layers_of(app, document) if r['index'] == index)
    return {k: row.get(k) for k in ('index', 'name', 'kind', 'blend_mode', 'opacity',
                                    'clipped', 'visible', 'style') if k in row}


# ------------------------------------------------------------------ gates --

def e1(app, folder, depth):
    document = blank(app, depth)
    top = place(app, folder, document, depth, 'Top', (40, 28), (30, 18), 1)
    styled(app, document, top, {'effects': EFFECTS})
    out = {'layer': top, 'row': row_of(app, document, top)}
    out['styled'] = regions_equal_crops(app, document, f'E1 effects {depth}')
    app.call('set_layer_properties', document_id=document, index=top, opacity=0.6)
    out['opacity'] = regions_equal_crops(app, document, f'E1 opacity 0.6 {depth}')
    app.call('set_layer_properties', document_id=document, index=top, blend_mode='Multiply')
    out['multiply_row'] = row_of(app, document, top)
    out['multiply'] = regions_equal_crops(app, document, f'E1 Multiply 0.6 {depth}')
    return out


def e2(app, folder, depth):
    document = blank(app, depth)
    front = place(app, folder, document, depth, 'Front', (40, 30), (20, 10), 2)
    member = place(app, folder, document, depth, 'Member', (40, 30), (40, 24), 3)
    app.call('set_layer_clipped', document_id=document, layers=[member], clipped=True)
    styled(app, document, front, {'fill_opacity': 0.3, 'blend_if': BLEND_IF,
                                  'effects': EFFECTS})
    out = {'rows': [row_of(app, document, front), row_of(app, document, member)]}
    assert out['rows'][1].get('clipped') is True, out['rows']
    assert out['rows'][1].get('visible') is True, out['rows']
    out['regions'] = regions_equal_crops(app, document, f'E2 styled clip base {depth}')
    return out


def e3(app, folder):
    document = blank(app, 16)
    a = place(app, folder, document, 16, 'A', (40, 30), (20, 10), 4)
    b = place(app, folder, document, 16, 'B', (30, 20), (45, 30), 5)
    g = group(app, document, [a, b], 'Group')
    # A Normal group here; a styled Pass Through one isolates.
    app.call('set_layer_properties', document_id=document, index=g, blend_mode='Normal')
    styled(app, document, g, {'fill_opacity': 0.5, 'effects': []})
    out = {'group': g, 'row': row_of(app, document, g)}
    out['regions'] = regions_equal_crops(app, document, 'E3 fill-only group 16')
    return out


def e4(app, folder):
    out = {}
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (40, 28), (30, 18), 6)
    for key, style in (
            ('knockout', {'fill_opacity': 0.5, 'knockout': 'shallow', 'effects': []}),
            ('grouped_interior', {'blend_interior_as_group': True,
                                  'effects': [{'type': 'color_overlay'}]})):
        styled(app, document, top, style)
        # Admitted since 13A-3c: equals the crop.
        out[key] = regions_equal_crops(app, document, f'E4 {key} 8')
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (40, 28), (30, 18), 7)
    g = group(app, document, [top], 'Group')
    styled(app, document, g, {'effects': [{'type': 'drop_shadow', 'distance': 3,
                                           'size': 2}]})
    # Admitted since 13A-3b-ii: the materialized styled group equals the crop.
    out['group_effects'] = {'group': g, 'row': row_of(app, document, g),
                            'regions': regions_equal_crops(app, document,
                                                           'E4 group with effects 8')}
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
            receipts['E1'] = {'8': e1(app, folder, 8), '16': e1(app, folder, 16)}
            print('PASS E1 a lone raster with drop shadow, stroke and colour overlay, then '
                  'opacity 0.6, then Multiply — each equals the crop at 8 and 16 bits',
                  flush=True)
            receipts['E2'] = {'8': e2(app, folder, 8), '16': e2(app, folder, 16)}
            print('PASS E2 a styled clip base (effects, fill 0.3, Blend If) with a visible '
                  'member equals the crop at 8 and 16 bits', flush=True)
            receipts['E3'] = e3(app, folder)
            print('PASS E3 a 16-bit fill-only Normal group equals the crop', flush=True)
            receipts['E4'] = e4(app, folder)
            print('PASS E4 a knockout, a grouped interior and a group with effects each '
                  'equal the crop', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
