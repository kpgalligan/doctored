#!/usr/bin/env python3
"""Owned-app Phase 13A-3c drive: MCP `render` with `region` / `preview_divisor`
over KNOCKOUT (Shallow / Deep) and "Blend Interior Effects as Group" at 8 and
16 bits — admitted since 13A-3c, where they refused before.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
K1 (8 and 16 bits): a canvas-sized opaque `place_image` at (0, 0) + a placed
raster styled `{knockout: "shallow", fill_opacity: 0.5, effects: []}` equals
the crop, then `{knockout: "deep", fill_opacity: 0}` (the hole), then
`{knockout: "shallow", effects: [drop_shadow, color_overlay]}` at opacity 0.6
(the package).
K2 (8 and 16 bits): a knockout member under a clip base (`set_layer_clipped`)
equals the crop, then the base styled `{fill_opacity: 0.5, blend_if}`.
K3 (8 and 16 bits): a Shallow child inside a pass-through `group_layers` group
equals the crop, then the group set to Multiply (isolated: the hard stop).
K4 (8 and 16 bits): `{blend_interior_as_group: true, effects: [color_overlay
multiply]}` on a lone Multiply raster equals the crop, then the same raster as
a clip base with a visible member (D6 evaluates off there).
The plan is not reachable over MCP (`render` returns the image, not the plan;
13B owns a plan surface); the Rust tests `regional_byte_knockout` /
`regional_precision_knockout` assert it.
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
    {'type': 'color_overlay', 'blend': 'multiply', 'color': '#3080c0', 'opacity': 0.7},
]


def styled(app, document, layer, style):
    app.call('set_layer_style', document_id=document, layer=layer, style=style)


def row_of(app, document, index):
    row = next(r for r in layers_of(app, document) if r['index'] == index)
    return {k: row.get(k) for k in ('index', 'name', 'kind', 'blend_mode', 'opacity',
                                    'clipped', 'visible', 'style') if k in row}


def backdrop(app, folder, depth, seed):
    """A new document with a canvas-sized opaque placed raster at (0, 0)."""
    document = blank(app, depth)
    place(app, folder, document, depth, 'Backdrop', (W, H), (0, 0), seed)
    return document


# ------------------------------------------------------------------ gates --

def k1(app, folder, depth):
    document = backdrop(app, folder, depth, 1)
    top = place(app, folder, document, depth, 'Top', (40, 28), (30, 18), 2)
    out = {'layer': top}
    styled(app, document, top, {'knockout': 'shallow', 'fill_opacity': 0.5, 'effects': []})
    out['shallow_row'] = row_of(app, document, top)
    out['shallow'] = regions_equal_crops(app, document, f'K1 shallow 0.5 {depth}')
    styled(app, document, top, {'knockout': 'deep', 'fill_opacity': 0, 'effects': []})
    out['deep_hole'] = regions_equal_crops(app, document, f'K1 deep hole {depth}')
    app.call('set_layer_properties', document_id=document, index=top, opacity=0.6)
    styled(app, document, top, {'knockout': 'shallow', 'effects': EFFECTS})
    out['package_row'] = row_of(app, document, top)
    out['package'] = regions_equal_crops(app, document, f'K1 shallow package 0.6 {depth}')
    return out


def k2(app, folder, depth):
    document = backdrop(app, folder, depth, 3)
    base = place(app, folder, document, depth, 'Base', (50, 36), (10, 8), 4)
    member = place(app, folder, document, depth, 'Member', (40, 30), (30, 20), 5)
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    styled(app, document, member, {'knockout': 'deep', 'fill_opacity': 0.4, 'effects': []})
    out = {'rows': [row_of(app, document, base), row_of(app, document, member)]}
    assert out['rows'][1].get('clipped') is True, out['rows']
    out['member'] = regions_equal_crops(app, document, f'K2 knockout member {depth}')
    styled(app, document, base, {'fill_opacity': 0.5, 'blend_if': BLEND_IF, 'effects': []})
    out['styled_base'] = regions_equal_crops(app, document, f'K2 styled base {depth}')
    return out


def k3(app, folder, depth):
    document = backdrop(app, folder, depth, 6)
    a = place(app, folder, document, depth, 'A', (44, 30), (8, 6), 7)
    app.call('set_layer_properties', document_id=document, index=a, blend_mode='Multiply')
    top = place(app, folder, document, depth, 'Top', (40, 28), (30, 18), 8)
    styled(app, document, top, {'knockout': 'shallow', 'fill_opacity': 0.5, 'effects': []})
    g = app.call('group_layers', document_id=document, layers=[a, top], name='Group')['group']
    out = {'group': g, 'row': row_of(app, document, g)}
    out['pass_through'] = regions_equal_crops(app, document, f'K3 pass-through {depth}')
    app.call('set_layer_properties', document_id=document, index=g, blend_mode='Multiply')
    out['multiply_row'] = row_of(app, document, g)
    out['multiply'] = regions_equal_crops(app, document, f'K3 Multiply group {depth}')
    return out


def k4(app, folder, depth):
    document = backdrop(app, folder, depth, 9)
    top = place(app, folder, document, depth, 'Top', (40, 28), (30, 18), 10)
    app.call('set_layer_properties', document_id=document, index=top, blend_mode='Multiply')
    styled(app, document, top, {'blend_interior_as_group': True, 'effects': [
        {'type': 'color_overlay', 'blend': 'multiply', 'color': '#3080c0'}]})
    out = {'row': row_of(app, document, top)}
    out['lone'] = regions_equal_crops(app, document, f'K4 grouped interior {depth}')
    member = place(app, folder, document, depth, 'Member', (30, 20), (40, 26), 11)
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    out['member_row'] = row_of(app, document, member)
    out['base'] = regions_equal_crops(app, document, f'K4 grouped clip base {depth}')
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
            receipts['K1'] = {'8': k1(app, folder, 8), '16': k1(app, folder, 16)}
            print('PASS K1 a Shallow 0.5 knockout over a canvas-sized backdrop, then a Deep '
                  'hole, then Shallow with effects at 0.6 — each equals the crop at 8 and '
                  '16 bits', flush=True)
            receipts['K2'] = {'8': k2(app, folder, 8), '16': k2(app, folder, 16)}
            print('PASS K2 a knockout member under a clip base, then the base styled fill '
                  '0.5 + Blend If — each equals the crop at 8 and 16 bits', flush=True)
            receipts['K3'] = {'8': k3(app, folder, 8), '16': k3(app, folder, 16)}
            print('PASS K3 a Shallow child in a pass-through group, then the group Multiply '
                  '— each equals the crop at 8 and 16 bits', flush=True)
            receipts['K4'] = {'8': k4(app, folder, 8), '16': k4(app, folder, 16)}
            print('PASS K4 Blend Interior Effects as Group on a lone Multiply raster, then '
                  'on a clip base with a visible member — each equals the crop at 8 and 16 '
                  'bits', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
