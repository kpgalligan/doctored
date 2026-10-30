#!/usr/bin/env python3
"""Owned-app Phase 13A-3a drive: MCP `render` with `region` / `preview_divisor`
over GROUPS — isolated and pass-through, gated, nested, as clip bases and clip
members — at 8 bits, 16 bits and float.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
G1 (8 bits) / G2 (16 bits): `group_layers` (Pass Through) over the 13A-2 clip
stack (a Multiply 0.6 base with a visible and a hidden clipped member, index 0
clipped and baseless, a hue/saturation adjustment above) equals the crop; then
`set_layer_properties` opacity 0.6; then `add_layer_mask` from an ellipse
selection (feather 6) + `set_mask_properties` density 0.5; then `blend_mode:
"Multiply"` (isolated); then a nested outer group — each equals the crop.
G3 (8 and 16 bits): a Pass Through group (a raster + an Invert adjustment) as a
clip base with a visible member (`set_layer_clipped`) and a group member, each
equal to the crop; then both members hidden (`set_layer_properties visible
false`), the base back to Pass Through: still equals the crop, and the full
render differs from the isolated one (the Invert reaches the backdrop again).
G4: the float fixture + `new_layer` + `group_layers` equals the crop.
G5: 8 bits `set_layer_style` with `drop_shadow` on the GROUP — refused "is a
group with layer-style effects" in 13A-3b-i, admitted since 13A-3b-ii (the
materialized group) — equals the crop; 16 bits a fill-only style on the
(isolated) group — admitted since 13A-3b-i — equals the crop.
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
from verify_phase13a1_regional import FLOAT_FIXTURE, H, W, regions_equal_crops, render
from verify_phase13a2_masks_clips import blank, index_named, layers_of, place

ROW_KEYS = ('index', 'name', 'kind', 'depth', 'parent', 'clipped', 'visible',
            'blend_mode', 'opacity')


def rows_of(app, document):
    return [{k: r.get(k) for k in ROW_KEYS if k in r} for r in layers_of(app, document)]


def is_pass_through(row):
    """A group row in Pass Through: both depths name the mode."""
    return row.get('blend_mode') == 'Pass Through'


def group(app, document, layers, name):
    reply = app.call('group_layers', document_id=document, layers=layers, name=name)
    return reply['group']


# ------------------------------------------------------------------ gates --

def g1_g2(app, folder, depth):
    document = blank(app, depth)
    front = place(app, folder, document, depth, 'Front', (40, 30), (20, 10), 1)
    member = place(app, folder, document, depth, 'Member', (40, 30), (40, 24), 2)
    hidden = place(app, folder, document, depth, 'Hidden', (30, 20), (8, 6), 3)
    app.call('set_layer_properties', document_id=document, index=front,
             blend_mode='Multiply', opacity=0.6)
    app.call('set_layer_clipped', document_id=document, layers=[member, hidden], clipped=True)
    app.call('set_layer_properties', document_id=document, index=hidden, visible=False)
    app.call('set_layer_clipped', document_id=document, layer=0, clipped=True)
    app.call('add_adjustment_layer', document_id=document, op='hue_saturation',
             params={'hue': 35, 'saturation': 0.25}, name='Hue')
    names = ('Front', 'Member', 'Hidden', 'Hue')
    grouped = [index_named(app, document, name) for name in names]
    g = group(app, document, grouped, 'Group')
    out = {'document': document, 'rows': rows_of(app, document)}
    row = next(r for r in layers_of(app, document) if r['index'] == g)
    assert is_pass_through(row), row
    out['pass_through'] = regions_equal_crops(app, document, f'G pass-through {depth}')
    app.call('set_layer_properties', document_id=document, index=g, opacity=0.6)
    out['opacity'] = regions_equal_crops(app, document, f'G opacity 0.6 {depth}')
    app.call('select_ellipse', document_id=document, x=10, y=6, width=52, height=40,
             feather=6)
    app.call('add_layer_mask', document_id=document, layer=g, kind='from_selection')
    app.call('set_mask_properties', document_id=document, layer=g, density=0.5)
    out['mask_properties'] = app.call('get_mask_properties', document_id=document, layer=g)
    out['masked'] = regions_equal_crops(app, document, f'G masked {depth}')
    app.call('set_layer_properties', document_id=document, index=g, blend_mode='Multiply')
    out['multiply'] = regions_equal_crops(app, document, f'G multiply {depth}')
    outer = group(app, document, [g], 'Outer')
    out['nested_rows'] = rows_of(app, document)
    out['outer'] = outer
    out['nested'] = regions_equal_crops(app, document, f'G nested {depth}')
    return out


def g3(app, folder, depth):
    document = blank(app, depth)
    place(app, folder, document, depth, 'A', (40, 30), (20, 10), 4)
    app.call('add_adjustment_layer', document_id=document, op='invert', params={},
             name='Invert')
    base = group(app, document, [index_named(app, document, 'A'),
                                 index_named(app, document, 'Invert')], 'Base')
    pass_full, _ = render(app, document)
    member = place(app, folder, document, depth, 'C', (30, 20), (50, 30), 5)
    app.call('set_layer_clipped', document_id=document, layer=member, clipped=True)
    out = {'document': document, 'rows': rows_of(app, document)}
    out['visible_member'] = regions_equal_crops(app, document, f'G3 visible member {depth}')
    isolated_full, _ = render(app, document)
    place(app, folder, document, depth, 'D', (24, 16), (8, 30), 6)
    m = group(app, document, [index_named(app, document, 'D')], 'M')
    app.call('set_layer_clipped', document_id=document, layer=m, clipped=True)
    out['group_member_rows'] = rows_of(app, document)
    out['group_member'] = regions_equal_crops(app, document, f'G3 group member {depth}')
    for name in ('C', 'M'):
        app.call('set_layer_properties', document_id=document,
                 index=index_named(app, document, name), visible=False)
    row = next(r for r in layers_of(app, document) if r['name'] == 'Base')
    assert is_pass_through(row), row
    hidden_full, _ = render(app, document)
    assert list(hidden_full['samples']) == list(pass_full['samples']), \
        'with every member hidden the base is Pass Through again'
    assert list(hidden_full['samples']) != list(isolated_full['samples']), \
        'isolation stopped the Invert reaching the backdrop'
    out['hidden_members_rows'] = rows_of(app, document)
    out['hidden_members'] = regions_equal_crops(app, document, f'G3 members hidden {depth}')
    out['base'] = base
    return out


def g4(app):
    document = app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']
    app.call('new_layer', document_id=document, name='New')
    rows = layers_of(app, document)
    g = group(app, document, [row['index'] for row in rows], 'Group')
    out = {'precision': app.call('get_document', document_id=document).get('precision'),
           'group': g, 'rows': rows_of(app, document)}
    out['regions'] = regions_equal_crops(app, document, 'G4 float group')
    return out


def g5(app, folder):
    out = {}
    for depth, style in ((8, {'effects': [{'type': 'drop_shadow', 'distance': 3,
                                            'size': 2}]}),
                         (16, {'fill_opacity': 0.5, 'effects': []})):
        document = blank(app, depth)
        top = place(app, folder, document, depth, 'Top', (50, 40), (15, 9), 7)
        g = group(app, document, [top], 'Group')
        if depth == 16:
            # A Normal group here; a styled Pass Through one isolates.
            app.call('set_layer_properties', document_id=document, index=g,
                     blend_mode='Normal')
        app.call('set_layer_style', document_id=document, layer=g, style=style)
        # A fill-only group style is admitted since 13A-3b-i; a group style with
        # an enabled effect since 13A-3b-ii.
        label = 'G5 fill-only group 16' if depth == 16 else 'G5 drop-shadow group 8'
        out[str(depth)] = {'group': g, 'regions': regions_equal_crops(app, document, label)}
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
            receipts['G1'] = g1_g2(app, folder, 8)
            print('PASS G1 8-bit pass-through group over the clip stack, opacity, mask, '
                  'Multiply, nested — each equals the crop', flush=True)
            receipts['G2'] = g1_g2(app, folder, 16)
            print('PASS G2 16-bit pass-through group over the clip stack, opacity, mask, '
                  'Multiply, nested — each equals the crop', flush=True)
            receipts['G3'] = [g3(app, folder, 8), g3(app, folder, 16)]
            print('PASS G3 group clip base with a visible and a group member, then both '
                  'hidden (Pass Through again) — each equals the crop at 8 and 16 bits',
                  flush=True)
            receipts['G4'] = g4(app)
            print('PASS G4 float fixture + new_layer + group_layers equals the crop',
                  flush=True)
            receipts['G5'] = g5(app, folder)
            print('PASS G5 an 8-bit group with an effect and a 16-bit fill-only group '
                  'each equal the crop', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
