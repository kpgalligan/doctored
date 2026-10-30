#!/usr/bin/env python3
"""Owned-app Phase 13A-1 drive: MCP `render` with `region` / `preview_divisor`
— the regional document compositor at 8 bits, 16 bits and 32-bit float.

Gates (fixed in designs/phase-13a-regional-status.md before measuring; EXACT
throughout, and every fixture is opaque so the PNG round trip is per pixel):
R1 an 8-bit 96×64 white `new_document` with two `place_image` rasters (one
partly off-canvas) in Multiply 0.6 and Normal under a hue_saturation
adjustment: `render(region)` at an interior window, the top-left edge, the
bottom-right edge and 1×1 equals the crop of the full `render` (both
`max_side: 4096`, decoded with `pixels.png_rgba`), and `preview_divisor: 3`
equals top-left-cell sampling of the full render; R2 the same at depth 16 (the
placed rasters are 16-bit PNGs). At 8 bits `add_adjustment_layer` adds a
reveal-all mask, which since 13A-2a stays ENABLED (masks are admitted); R3 the
committed float fixture `core/tests/fixtures/float_11f/scene.tiff` via
`open_document` + `new_layer`:
region == crop; R4 `add_layer_mask` at 8 and 16 bits: since 13A-2a the masked
regional render EQUALS the crop of the full render, and `list_history` is
unchanged by every regional call; R5 an out-of-canvas region,
`preview_divisor: 0` and `region` beside `layer` refuse in band. A failure
exits nonzero and leaves a JSON receipt in --receipts.
"""
import argparse
import base64
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8, write_png16
from support import HostApp

W, H = 96, 64
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'


# ------------------------------------------------------------- plumbing --

def rpc(app, name, **arguments):
    return app.client.rpc('tools/call', {'name': name, 'arguments': arguments})


def text_of(reply):
    return ' '.join(row.get('text', '') for row in reply.get('content', []))


def render(app, document, **arguments):
    """The render tool's decoded PNG and reply text."""
    reply = rpc(app, 'render', document_id=document, max_side=4096, **arguments)
    assert not reply.get('isError'), (arguments, text_of(reply))
    encoded = next(row['data'] for row in reply['content'] if row['type'] == 'image')
    decoded = png_rgba(base64.b64decode(encoded))
    return decoded, text_of(reply)


def refusal(app, document, **arguments):
    reply = rpc(app, 'render', document_id=document, max_side=4096, **arguments)
    assert reply.get('isError'), (arguments, reply)
    return text_of(reply)


def history(app, document):
    return json.dumps(app.call('list_history', document_id=document), sort_keys=True)


def expected(full, x, y, w, h, d):
    """Top-left-cell samples of the full render, row-major."""
    width = full['width']
    samples = full['samples']
    out = []
    for j in range(0, h, d):
        for i in range(0, w, d):
            at = ((y + j) * width + (x + i)) * 4
            out.extend(samples[at:at + 4])
    return (len(range(0, w, d)), len(range(0, h, d)), out)


def windows(width, height):
    return [
        ('interior', dict(x=width // 7, y=height // 7, width=width * 3 // 7,
                          height=height * 3 // 7), 1),
        ('top_left_edge', dict(x=0, y=0, width=17, height=11), 1),
        ('bottom_right_edge', dict(x=width - 19, y=height - 13, width=19, height=13), 1),
        ('one_by_one', dict(x=width // 2, y=height // 3, width=1, height=1), 1),
        ('preview_3', None, 3),
    ]


def regions_equal_crops(app, document, label):
    """Every window/divisor of `windows` equals the crop of the full render,
    and `list_history` is unchanged by each regional call."""
    full, _ = render(app, document)
    width, height = full['width'], full['height']
    before = history(app, document)
    out = {'full': [width, height, full['depth']]}
    for name, window, divisor in windows(width, height):
        arguments = {}
        if window is not None:
            arguments['region'] = window
        if divisor != 1:
            arguments['preview_divisor'] = divisor
        got, text = render(app, document, **arguments)
        box = window or dict(x=0, y=0, width=width, height=height)
        want = expected(full, box['x'], box['y'], box['width'], box['height'], divisor)
        assert got['depth'] == full['depth'], (label, name, got['depth'])
        assert (got['width'], got['height']) == want[:2], (label, name, got['width'],
                                                            got['height'], want[:2])
        assert list(got['samples']) == want[2], (label, name, 'samples differ from the crop')
        assert f"at divisor {divisor}" in text, (label, name, text)
        assert ('(preview)' in text) == (divisor > 1), (label, name, text)
        assert history(app, document) == before, (label, name, 'history changed')
        out[name] = {'arguments': arguments, 'output': [got['width'], got['height']],
                     'text': text}
    return out


def opaque_picture(width, height, seed):
    return [value for y in range(height) for x in range(width)
            for value in ((x * 29 + y * 7 + seed * 41) % 256, (x * 5 + y * 23 + seed) % 256,
                          ((x ^ y) * 3 + seed * 13) % 256, 255)]


def stacked(app, folder, depth):
    """96×64 white, two placed rasters (the second partly off-canvas) in
    Multiply 0.6 and Normal, then a hue_saturation adjustment."""
    document = app.call('new_document', width=W, height=H, depth=depth,
                        background='white')['document']['id']
    placed = []
    for name, (w, h), (x, y), seed in (('Front', (40, 30), (20, 10), 1),
                                       ('Edge', (40, 30), (70, 44), 2)):
        path = folder / f'{name.lower()}-{depth}.png'
        picture = opaque_picture(w, h, seed)
        if depth == 16:
            write_png16(path, w, h, [v * 257 for v in picture])
        else:
            write_png8(path, w, h, bytes(picture))
        reply = app.call('place_image', document_id=document, path=str(path),
                         storage='embedded', name=name, x=x, y=y)
        placed.append(reply['layer'])
    app.call('set_layer_properties', document_id=document, index=placed[0],
             blend_mode='Multiply', opacity=0.6)
    adjustment = app.call('add_adjustment_layer', document_id=document, op='hue_saturation',
                          params={'hue': 35, 'saturation': 0.25}, name='Hue')
    # At 8 bits `add_adjustment_layer` adds a mask (reveal_all). Since 13A-2a
    # an enabled mask is admitted, so it stays ENABLED; the receipt records
    # whether the layer carries one. A 16-bit adjustment layer is created
    # without one.
    row = next(r for r in app.call('get_document', document_id=document)['layers']
               if r['index'] == adjustment['layer'])
    return document, bool(row.get('has_mask') or row.get('mask'))


# ------------------------------------------------------------------ gates --

def r1_r2(app, folder, depth):
    document, auto_mask = stacked(app, folder, depth)
    out = regions_equal_crops(app, document, f'depth {depth}')
    out['adjustment_auto_mask_enabled'] = auto_mask
    rows = app.call('get_document', document_id=document)['layers']
    out['rows'] = [{k: row.get(k) for k in ('index', 'name', 'blend_mode', 'opacity')}
                   for row in rows]
    out['document'] = document
    return out


def r3(app):
    document = app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']
    precision = app.call('get_document', document_id=document).get('precision')
    app.call('new_layer', document_id=document, name='New')
    out = regions_equal_crops(app, document, 'float')
    out['precision'] = precision
    return out


def r4(app, document, depth):
    before = history(app, document)
    front = next(row['index'] for row in
                 app.call('get_document', document_id=document)['layers']
                 if row['name'] == 'Front')
    app.call('add_layer_mask', document_id=document, layer=front, kind='reveal_all')
    after_mask = history(app, document)
    assert after_mask != before, 'add_layer_mask records a step'
    # Since 13A-2a the masked render is admitted and EQUALS the crop.
    out = regions_equal_crops(app, document, f'R4 masked {depth}')
    assert history(app, document) == after_mask, 'a regional call changed history'
    return {'depth': depth, 'layer': front, 'regions': out}


def r5(app, document):
    before = history(app, document)
    cases = {
        'outside': dict(region=dict(x=W - 4, y=0, width=8, height=8)),
        'divisor_zero': dict(preview_divisor=0),
        'with_layer': dict(region=dict(x=0, y=0, width=4, height=4), layer=0),
        'with_channel': dict(preview_divisor=2, channel='red'),
    }
    out = {}
    for name, arguments in cases.items():
        out[name] = refusal(app, document, **arguments)
    assert 'outside' in out['outside'], out['outside']
    assert 'preview_divisor' in out['divisor_zero'], out['divisor_zero']
    for name in ('with_layer', 'with_channel'):
        assert 'flattened canvas only' in out[name], out[name]
    assert history(app, document) == before, 'a refused call changed history'
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
            receipts['R1'] = r1_r2(app, folder, 8)
            print('PASS R1 8-bit regions and divisor-3 preview equal the full render', flush=True)
            receipts['R2'] = r1_r2(app, folder, 16)
            print('PASS R2 16-bit regions and divisor-3 preview equal the full render', flush=True)
            receipts['R3'] = r3(app)
            print('PASS R3 float fixture + new_layer: regions equal the full render', flush=True)
            receipts['R5'] = r5(app, receipts['R1']['document'])
            print('PASS R5 out-of-canvas, divisor 0 and layer/channel refuse in band', flush=True)
            receipts['R4'] = [r4(app, receipts['R1']['document'], 8),
                              r4(app, receipts['R2']['document'], 16)]
            print('PASS R4 an enabled mask equals the crop; history unchanged', flush=True)
    except BaseException as error:
        receipts['failure'] = repr(error)
        (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
        raise
    receipts['result'] = 'ALL GATES PASS'
    (folder / 'report.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
