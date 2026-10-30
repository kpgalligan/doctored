#!/usr/bin/env python3
"""Owned-app Phase 13A-2b drive: MCP `render` with `region` / `preview_divisor`
over 8-bit layer styles with NO enabled effect (fill opacity, Blend If) on a
lone raster, a clip base and a clip member.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers):
S1: a lone `place_image` raster + `set_layer_style {fill_opacity: 0.5,
effects: []}` equals the crop, then `{fill_opacity: 0.5, blend_if, effects:
[]}` equals the crop.
S2: `set_layer_style` with Blend If only equals the crop.
S3: a Multiply 0.6 base styled `{fill_opacity: 0.3, blend_if}` with a visible,
a hidden and a visible opacity-0 clipped member equals the crop; then the base
at fill 0; then the visible member styled `{fill_opacity: 0.5}`.
S4: `{effects: [drop_shadow]}` — admitted since 13A-3b-i — equals the crop,
and so does `{fill_opacity: 0.5, knockout: "shallow"}` (refused "carries a
knockout" until 13A-3c).
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
from verify_phase13a1_regional import regions_equal_crops
from verify_phase13a2_masks_clips import BLEND_IF, blank, layers_of, place


def styled(app, document, layer, style):
    app.call('set_layer_style', document_id=document, layer=layer, style=style)


# ------------------------------------------------------------------ gates --

def s1(app, folder):
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (50, 40), (15, 9), 6)
    styled(app, document, top, {'fill_opacity': 0.5, 'effects': []})
    out = {'fill': regions_equal_crops(app, document, 'S1 fill 0.5')}
    styled(app, document, top, {'fill_opacity': 0.5, 'blend_if': BLEND_IF, 'effects': []})
    out['fill_blend_if'] = regions_equal_crops(app, document, 'S1 fill 0.5 + Blend If')
    return out


def s2(app, folder):
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (50, 40), (15, 9), 7)
    styled(app, document, top, {'fill_opacity': 1, 'blend_if': BLEND_IF, 'effects': []})
    return {'regions': regions_equal_crops(app, document, 'S2 Blend If 8')}


def s3(app, folder):
    document = blank(app, 8)
    front = place(app, folder, document, 8, 'Front', (40, 30), (20, 10), 1)
    member = place(app, folder, document, 8, 'Member', (40, 30), (40, 24), 2)
    hidden = place(app, folder, document, 8, 'Hidden', (30, 20), (8, 6), 3)
    zero = place(app, folder, document, 8, 'Zero', (30, 20), (26, 14), 4)
    app.call('set_layer_properties', document_id=document, index=front,
             blend_mode='Multiply', opacity=0.6)
    app.call('set_layer_clipped', document_id=document, layers=[member, hidden, zero],
             clipped=True)
    app.call('set_layer_properties', document_id=document, index=hidden, visible=False)
    app.call('set_layer_properties', document_id=document, index=zero, opacity=0)
    styled(app, document, front, {'fill_opacity': 0.3, 'blend_if': BLEND_IF, 'effects': []})
    rows = layers_of(app, document)
    out = {'rows': [{k: r.get(k) for k in ('index', 'name', 'clipped', 'visible',
                                           'blend_mode', 'opacity')} for r in rows]}
    # The clamp-only step is provably exercised: the `zero` member is VISIBLE
    # at opacity 0 and the `hidden` member is hidden.
    by_index = {r.get('index'): r for r in rows}
    assert by_index[zero].get('opacity') == 0, by_index[zero]
    assert by_index[zero].get('visible') is True, by_index[zero]
    assert by_index[hidden].get('visible') is False, by_index[hidden]
    out['styled_base'] = regions_equal_crops(app, document, 'S3 styled base')
    styled(app, document, front, {'fill_opacity': 0, 'blend_if': BLEND_IF, 'effects': []})
    out['fill_0_base'] = regions_equal_crops(app, document, 'S3 base fill 0')
    styled(app, document, member, {'fill_opacity': 0.5, 'effects': []})
    out['styled_member'] = regions_equal_crops(app, document, 'S3 styled member')
    return out


def s4(app, folder):
    document = blank(app, 8)
    top = place(app, folder, document, 8, 'Top', (50, 40), (15, 9), 8)
    out = {'layer': top}
    # Since 13A-3b-i an enabled effect is admitted and equals the crop.
    styled(app, document, top, {'effects': [{'type': 'drop_shadow', 'distance': 3,
                                             'size': 2}]})
    out['effect'] = regions_equal_crops(app, document, 'S4 drop shadow')
    # Since 13A-3c a knockout is admitted and equals the crop.
    styled(app, document, top, {'fill_opacity': 0.5, 'knockout': 'shallow', 'effects': []})
    out['knockout'] = regions_equal_crops(app, document, 'S4 knockout')
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
            receipts['S1'] = s1(app, folder)
            print('PASS S1 8-bit fill opacity, then fill + Blend If, equal the crop',
                  flush=True)
            receipts['S2'] = s2(app, folder)
            print('PASS S2 8-bit Blend If equals the crop', flush=True)
            receipts['S3'] = s3(app, folder)
            print('PASS S3 styled clip base with visible, hidden and opacity-0 members; '
                  'fill 0 base; styled member — each equals the crop', flush=True)
            receipts['S4'] = s4(app, folder)
            print('PASS S4 an enabled effect and a knockout each equal the crop',
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
