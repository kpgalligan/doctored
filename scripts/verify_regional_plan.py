#!/usr/bin/env python3
"""Owned-app 13B-7 drive: MCP `render` with `region` / `preview_divisor`
reports the regional plan it ran, as a third content item
`{"regional_plan": {...}}` (the plan's top-level keys, no per-layer rows).

Gates (EXACT; the plan's numbers are the core's own, asserted here only where
the request fixes them):
P1 an 8-bit 96×64 white document with two placed rasters and a hue_saturation
adjustment: a region's plan has `global_analysis` false, `required_input_region`
equal to the region, `neighborhood` pointwise, `depth` "rgba8",
`max_output_bytes`, `requested_region`, `preview_only` false and no `layers`.
P2 a reveal-all mask feathered 6 on a placed raster: `neighborhood` spatial
with `halo_pixels` > 0 and `required_input_region` the region grown by it,
clamped to the canvas.
P3 a drop shadow on a placed raster: `global_analysis` ["layer_style_planes"]
and `required_input_region` the whole canvas.
P4 a 16-bit document: `depth` "rgba16" and no `max_output_bytes`; a
`preview_divisor` 3 render without `region` plans the whole canvas with
`preview_only` true.
P5 a render without `region` or `preview_divisor` returns no plan item.
P6 every regional image here equals the crop (or top-left-cell sampling) of
the full render, and no regional call changes `list_history`.
P7 an out-of-canvas region refuses in band with the render's sentence, once.
DR_BACKGROUND=1: the instance never takes focus. A failure exits nonzero and
leaves a JSON receipt in --receipts.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from pixels import png_rgba
from support import HostApp
from verify_phase13a1_regional import H, W, expected, history, rpc, text_of
from verify_phase13a2_masks_clips import blank, place

REGION = dict(x=20, y=12, width=40, height=30)


def reply_of(app, document, **arguments):
    reply = rpc(app, 'render', document_id=document, max_side=4096, **arguments)
    assert not reply.get('isError'), (arguments, text_of(reply))
    return reply


def plan_of(reply):
    """The `regional_plan` item's object, or None when the reply has none."""
    plans = [json.loads(row['text'])['regional_plan'] for row in reply['content']
             if row.get('type') == 'text' and row.get('text', '').startswith('{"regional_plan"')]
    assert len(plans) <= 1, plans
    return plans[0] if plans else None


def image_of(reply):
    encoded = next(row['data'] for row in reply['content'] if row['type'] == 'image')
    return png_rgba(base64.b64decode(encoded))


def regional(app, document, divisor=1, region=REGION):
    """The plan of a regional render, after checking its image against the
    full render's crop and that history did not move (P6)."""
    full = image_of(reply_of(app, document))
    before = history(app, document)
    arguments = {}
    if region is not None:
        arguments['region'] = region
    if divisor != 1:
        arguments['preview_divisor'] = divisor
    reply = reply_of(app, document, **arguments)
    got = image_of(reply)
    box = region or dict(x=0, y=0, width=full['width'], height=full['height'])
    want = expected(full, box['x'], box['y'], box['width'], box['height'], divisor)
    assert (got['width'], got['height']) == want[:2], (got['width'], got['height'], want[:2])
    assert list(got['samples']) == want[2], 'the regional image differs from the crop'
    assert history(app, document) == before, 'a regional call changed history'
    plan = plan_of(reply)
    assert plan is not None, ('no regional_plan item', [row.get('type') for row in
                                                         reply['content']])
    assert 'layers' not in plan, 'the per-layer rows are left out'
    assert plan['requested_region'] == box, plan['requested_region']
    assert plan['preview_divisor'] == divisor and plan['preview_only'] == (divisor > 1), plan
    return plan


def stack(app, folder, depth):
    document = blank(app, depth)
    front = place(app, folder, document, depth, 'Front', (40, 30), (20, 10), 1)
    edge = place(app, folder, document, depth, 'Edge', (40, 30), (70, 44), 2)
    app.call('set_layer_properties', document_id=document, index=front,
             blend_mode='Multiply', opacity=0.6)
    app.call('add_adjustment_layer', document_id=document, op='hue_saturation',
             params={'hue': 35, 'saturation': 0.25}, name='Hue')
    return document, front, edge


def grown(region, halo):
    x0, y0 = max(region['x'] - halo, 0), max(region['y'] - halo, 0)
    x1 = min(region['x'] + region['width'] + halo, W)
    y1 = min(region['y'] + region['height'] + halo, H)
    return dict(x=x0, y=y0, width=x1 - x0, height=y1 - y0)


# ------------------------------------------------------------------ gates --

def p1(app, folder):
    document, front, edge = stack(app, folder, 8)
    plan = regional(app, document)
    assert plan['global_analysis'] is False, plan['global_analysis']
    assert plan['required_input_region'] == REGION, plan['required_input_region']
    assert plan['neighborhood'] == {'kind': 'pointwise', 'halo_pixels': 0}, plan
    assert plan['depth'] == 'rgba8' and 'max_output_bytes' in plan, plan
    return {'document': document, 'front': front, 'edge': edge, 'plan': plan}


def p2(app, document, front):
    app.call('add_layer_mask', document_id=document, layer=front, kind='reveal_all')
    app.call('set_mask_properties', document_id=document, layer=front, feather=6)
    plan = regional(app, document)
    halo = plan['neighborhood']['halo_pixels']
    assert plan['neighborhood']['kind'] == 'spatial' and halo > 0, plan['neighborhood']
    assert plan['required_input_region'] == grown(REGION, halo), plan
    assert plan['global_analysis'] is False, plan
    return plan


def p3(app, document, edge):
    app.call('set_layer_style', document_id=document, layer=edge,
             style={'effects': [{'type': 'drop_shadow', 'distance': 5, 'size': 3}]})
    plan = regional(app, document)
    assert plan['global_analysis'] == ['layer_style_planes'], plan['global_analysis']
    assert plan['required_input_region'] == dict(x=0, y=0, width=W, height=H), plan
    return plan


def p4(app, folder):
    document, _, _ = stack(app, folder, 16)
    plan = regional(app, document)
    assert plan['depth'] == 'rgba16' and 'max_output_bytes' not in plan, plan
    preview = regional(app, document, divisor=3, region=None)
    assert preview['required_input_region'] == dict(x=0, y=0, width=W, height=H), preview
    return {'region': plan, 'preview': preview}


def p5(app, document):
    reply = reply_of(app, document)
    assert plan_of(reply) is None, 'a full render carries no plan'
    return [row.get('type') for row in reply['content']]


def p7(app, document):
    before = history(app, document)
    reply = rpc(app, 'render', document_id=document,
                region=dict(x=W - 4, y=0, width=8, height=8))
    assert reply.get('isError'), reply
    text = text_of(reply)
    assert text.count('render region is outside the source image') == 1, text
    assert history(app, document) == before
    return text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    os.environ['DR_BACKGROUND'] = '1'
    folder = Path(args.receipts).expanduser()
    folder.mkdir(parents=True, exist_ok=True)
    receipts = {}
    executable = app_under_test.default_app_executable()
    try:
        with HostApp(folder / 'owner', folder / 'recovery', str(executable)) as app:
            receipts['P1'] = p1(app, folder)
            print('PASS P1 a pointwise 8-bit region plans its own window', flush=True)
            document = receipts['P1']['document']
            receipts['P2'] = p2(app, document, receipts['P1']['front'])
            print('PASS P2 a feathered mask grows the input by its halo', flush=True)
            receipts['P3'] = p3(app, document, receipts['P1']['edge'])
            print('PASS P3 a drop shadow reads the whole canvas', flush=True)
            receipts['P4'] = p4(app, folder)
            print('PASS P4 16 bits: rgba16, no output budget; a preview plans the canvas',
                  flush=True)
            receipts['P5'] = p5(app, document)
            print('PASS P5 a full render returns no plan', flush=True)
            receipts['P7'] = p7(app, document)
            print('PASS P7 an out-of-canvas region refuses once, in band', flush=True)
            print('PASS P6 every regional image equals the crop; history unchanged',
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
