#!/usr/bin/env python3
"""Owned-app Phase 13A-4b-ii-b-2 drive: MCP `render` with `region` /
`preview_divisor` over a guide-reading Shadows/Highlights as a CLIP MEMBER of
a STYLED base at 8 and 16 bits — admitted since 13A-4b-ii-b-2, where it
refused "… under a styled base" before; the 8-bit PACKAGE (effects below
opacity 1) refuses by its own sentence.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout — every regional render equals the crop / top-left-cell sampling of
the full `render`, and `list_history` is unchanged by every regional call —
using the 13A-1 drive's windows and helpers; each S gate at 8 and 16 bits):
S1: the base given `set_layer_style` `{"fill_opacity":0.5}` equals the crop.
S2: the base given `{"effects":[{"type":"drop_shadow"}]}` at opacity 1 equals
the crop.
S3: the base given `{"fill_opacity":0.5,"knockout":"shallow"}` equals the
crop.
S4: the S2 base at `set_layer_properties` opacity 0.6 — at 8 bits refuses "is
Shadows/Highlights with a guide under a packaged styled base (8-bit effects
below opacity 1)" naming the guide, `list_history` unchanged; at 16 bits
equals the crop.
S5: the guide clipped to a Normal (isolated) group holding the holey raster,
the group given the S2 effects, equals the crop.
F1: the float fixture refuses Shadows/Highlights by the float policy's
sentence.
The plan is asserted by the Rust tests `regional_byte_styled_clip_guide` /
`regional_precision_styled_clip_guide` (`render` reports it since 13B-7). A
failure exits nonzero and leaves a JSON receipt in --receipts.
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
from verify_phase13a1_regional import history, regions_equal_crops, rpc, text_of
from verify_phase13a2_masks_clips import index_named, layers_of
from verify_phase13a4_guide import backdrop, f1, guide, holey_picture, place_picture, row_of
from verify_phase13a4b_nested_guide import isolated_group
from verify_phase13a4biib_clip_members import clip_unit, clipped_row, rows

PACKAGED_BASE = ('is Shadows/Highlights with a guide under a packaged styled base '
                 '(8-bit effects below opacity 1)')
FILL = {'fill_opacity': 0.5}
EFFECTS = {'effects': [{'type': 'drop_shadow'}]}
KNOCKOUT = {'fill_opacity': 0.5, 'knockout': 'shallow'}


def styled_unit(app, folder, depth, seed, style):
    """`clip_unit` (the backdrop, a holey raster at (37, 29), a radius-30
    guide clipped to it) with the base given `style`."""
    document, base, sh = clip_unit(app, folder, depth, seed)
    app.call('set_layer_style', document_id=document, layer=index_named(app, document, base),
             style=style)
    return document, base, sh


def equal(app, folder, depth, seed, style, label):
    document, base, sh = styled_unit(app, folder, depth, seed, style)
    return {'guide': clipped_row(app, document, sh),
            'base': row_of(app, document, index_named(app, document, base)),
            'rows': rows(app, document),
            'regions': regions_equal_crops(app, document, f'{label} {depth}')}


# ------------------------------------------------------------------ gates --

def s1(app, folder, depth):
    return equal(app, folder, depth, 11, FILL, 'S1 a fill-only base')


def s2(app, folder, depth):
    return equal(app, folder, depth, 21, EFFECTS, 'S2 a drop-shadow base at opacity 1')


def s3(app, folder, depth):
    return equal(app, folder, depth, 31, KNOCKOUT, 'S3 a Shallow knockout base')


def s4(app, folder, depth):
    document, base, sh = styled_unit(app, folder, depth, 41, EFFECTS)
    app.call('set_layer_properties', document_id=document,
             index=index_named(app, document, base), opacity=0.6)
    out = {'guide': clipped_row(app, document, sh),
           'base': row_of(app, document, index_named(app, document, base))}
    if depth == 16:
        out['regions'] = regions_equal_crops(app, document,
                                             'S4 a drop-shadow base at 0.6, 16 bits')
        return out
    index = index_named(app, document, sh)
    before = history(app, document)
    reply = rpc(app, 'render', document_id=document, max_side=4096,
                region=dict(x=30, y=20, width=60, height=40))
    assert reply.get('isError'), reply
    message = text_of(reply)
    assert PACKAGED_BASE in message, message
    assert f'layer {index} ({sh})' in message, message
    assert history(app, document) == before, 'a refused regional call changed history'
    out['message'] = message
    return out


def s5(app, folder, depth):
    seed = 51
    document = backdrop(app, folder, depth, seed)
    inner = place_picture(app, folder, document, depth, f'Inner{seed}', (150, 110), (37, 29),
                          holey_picture(150, 110, seed + 1))
    g = isolated_group(app, document, [inner], 'Normal')
    app.call('set_layer_style', document_id=document, layer=g, style=EFFECTS)
    sh = f'SH{seed}'
    index = guide(app, document, 30, sh)
    app.call('set_layer_clipped', document_id=document, layer=index, clipped=True)
    group_row = next(r for r in layers_of(app, document)
                     if r['index'] == index_named(app, document, 'Normal'))
    return {'guide': clipped_row(app, document, sh), 'group': group_row,
            'rows': rows(app, document),
            'regions': regions_equal_crops(app, document,
                                           f'S5 guide clipped to a styled Normal group {depth}')}


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
            receipts['S1'] = {'8': s1(app, folder, 8), '16': s1(app, folder, 16)}
            print('PASS S1 a guide clipped to a fill-only base equals the crop at 8 and 16 '
                  'bits', flush=True)
            receipts['S2'] = {'8': s2(app, folder, 8), '16': s2(app, folder, 16)}
            print('PASS S2 a guide clipped to a drop-shadow base at opacity 1 equals the crop '
                  'at 8 and 16 bits', flush=True)
            receipts['S3'] = {'8': s3(app, folder, 8), '16': s3(app, folder, 16)}
            print('PASS S3 a guide clipped to a Shallow knockout base equals the crop at 8 and '
                  '16 bits', flush=True)
            receipts['S4'] = {'8': s4(app, folder, 8), '16': s4(app, folder, 16)}
            print('PASS S4 a drop-shadow base at 0.6 refuses "under a packaged styled base" '
                  'naming the guide at 8 bits and equals the crop at 16 bits', flush=True)
            receipts['S5'] = {'8': s5(app, folder, 8), '16': s5(app, folder, 16)}
            print('PASS S5 a guide clipped to a Normal group with effects equals the crop at 8 '
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
