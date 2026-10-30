#!/usr/bin/env python3
"""The MCP drive: Filter ▸ Neural Filters ▸ Depth Blur over MCP (`depth_blur`,
`list_models`) with the real depth model from the shared fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by `scripts/fill_model_fixtures.py`
(NETWORK) and only read here. Without a ready model there the drive prints
`SKIP depth blur: no fixture model store at <path>` and exits 0. The photo is
`phase6/photos/portrait.png` of the fixtures folder (NASA's astronaut portrait, public domain,
512 × 512). The instance is owned by this file (`phase5_support.App` on its own scratch MCP
socket, a dummy API key and a private directory for every preference library; DR_MODELS_DIR is
the fixture store, or an empty private folder for the not-installed case) and terminated by its
own context manager; no Doctored process this file did not start is signalled.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt and no bound
is widened afterwards. "Local deviation" of a patch is the mean over its pixels of |L − the mean
of L's four neighbours|, L the Rec. 709 luma of the 8-bit render.

Each case opens its own copy of the photo (`open_document` answers an open path with the
document already open).

- **D1 focus** (8, 16 and 32 bits): `depth_blur` with focal_x 225, focal_y 125 (the face),
  blur_strength 100, leaves the focal pixel of the render EXACTLY as it was.
- **D2 far blur** (each depth): of the four 32 × 32 corner patches (inset 4 px), the one whose
  depth-map-only grey is darkest (farthest) loses at least 25 % of its local deviation.
- **D3 depth order** (each depth): `depth_map_only` gives a mean grey inside `select_subject`'s
  mask at least 16 levels above the mean grey outside it.
- **D4 undo** (each depth): one `undo` after D1 restores the render exactly.
- **D5 new layer** (8 bits): output new_layer adds exactly one layer (output_layer 1, named
  "<name> Depth Blur") and leaves layer 0's pixels alone; one undo removes it.
- **D6 replay** (8 bits): `run_action` of a saved `depth_blur` step with D1's settings gives a
  render within 1 level of D1's in every channel.
- **D7 refusals**, each by its sentence: the model not installed (an instance with an empty
  store: "the depth model is not installed; download it in Settings ▸ Models", and list_models
  reports not_downloaded); an empty layer ("depth blur needs visible pixels; layer 1 is
  empty"); focus_subject on a flat-colour layer (Vision's "No subject was found in the
  image."). `list_models` on the fixture instance reports the model ready and accepted.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App
from verify_phase1 import png
from verify_phase2 import read_png

# ---- the expectations, fixed here BEFORE anything is measured -------------
STORE = Path.home() / 'devel/doctored-fixtures/models'
PORTRAIT = Path(os.environ.get('DR_JOBS_PROBE_FIXTURES', str(Path.home() / 'devel/doctored-fixtures'))) \
    / 'phase6/photos/portrait.png'
FOCAL = (225, 125)
PATCH, INSET = 32, 4
FAR_DROP = 0.25
SUBJECT_MARGIN = 16
REPLAY_BOUND = 1
NOT_INSTALLED = 'the depth model is not installed; download it in Settings ▸ Models'
EMPTY = 'depth blur needs visible pixels; layer 1 is empty'
NO_SUBJECT = 'No subject was found in the image.'
MODEL_ID = 'apple/coreml-depth-anything-v2-small'
SETTINGS = {'focal_x': FOCAL[0], 'focal_y': FOCAL[1], 'blur_strength': 100}


def render(app, doc):
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': doc, 'max_side': 4096}})
    assert not response.get('isError'), response
    data = next(item['data'] for item in response['content'] if item['type'] == 'image')
    return read_png(base64.b64decode(data))


def luma(pixel):
    return 0.2126 * pixel[0] + 0.7152 * pixel[1] + 0.0722 * pixel[2]


def corners(width, height):
    far = (width - INSET - PATCH, height - INSET - PATCH)
    return {'top_left': (INSET, INSET), 'top_right': (far[0], INSET),
            'bottom_left': (INSET, far[1]), 'bottom_right': far}


def deviation(image, origin):
    width, _, pixels = image
    total = 0.0
    for y in range(origin[1], origin[1] + PATCH):
        for x in range(origin[0], origin[0] + PATCH):
            at = lambda dx, dy: luma(pixels[(y + dy) * width + x + dx])
            total += abs(at(0, 0) - (at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1)) / 4)
    return total / (PATCH * PATCH)


def mean_grey(image, origin):
    width, _, pixels = image
    values = [pixels[y * width + x][0] for y in range(origin[1], origin[1] + PATCH)
              for x in range(origin[0], origin[0] + PATCH)]
    return sum(values) / len(values)


def ready(store):
    marker = store / MODEL_ID.replace('/', '--')
    return any((version / 'verified.json').is_file() for version in marker.glob('*')) if marker.is_dir() else False


def copy_of(folder, name):
    """The portrait under its own name: `open_document` answers an open path with that document."""
    path = folder / f'{name}.png'
    path.write_bytes(PORTRAIT.read_bytes())
    return path


def drive_depth(app, depth, receipts, folder):
    """D1–D4 on a fresh copy of the portrait at `depth` bits."""
    call = app.call
    doc = call('open_document', path=str(copy_of(folder, f'portrait-{depth}')))['document']['id']
    if depth == 16:
        call('convert_precision', document_id=doc, mode='rgba16', structure='preserve')
    elif depth == 32:
        call('convert_precision', document_id=doc, mode='rgba32', structure='preserve')
    before = render(app, doc)
    width, height, _ = before
    row = {}

    # D3: the depth map's order, and the corner it says is farthest.
    call('depth_blur', document_id=doc, layer=0, depth_map_only=True)
    depth_map = render(app, doc)
    call('undo', document_id=doc)
    call('select_subject', document_id=doc)
    call('save_selection', document_id=doc, name='Subject')
    call('deselect', document_id=doc)
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': doc, 'max_side': 4096, 'channel': 'Subject'}})
    assert not response.get('isError'), response
    mask = read_png(base64.b64decode(next(i['data'] for i in response['content'] if i['type'] == 'image')))
    inside = [depth_map[2][i][0] for i, m in enumerate(mask[2]) if m[0] >= 128]
    outside = [depth_map[2][i][0] for i, m in enumerate(mask[2]) if m[0] < 128]
    row['subject_grey'] = sum(inside) / max(1, len(inside))
    row['outside_grey'] = sum(outside) / max(1, len(outside))
    row['D3'] = bool(inside) and bool(outside) and row['subject_grey'] >= row['outside_grey'] + SUBJECT_MARGIN
    greys = {name: mean_grey(depth_map, origin) for name, origin in corners(width, height).items()}
    far = min(greys, key=greys.get)
    row['corner_greys'] = greys
    row['far_corner'] = far

    # D1, D2: the blur.
    reply = call('depth_blur', document_id=doc, layer=0, **SETTINGS)
    row['reply'] = reply
    after = render(app, doc)
    index = FOCAL[1] * width + FOCAL[0]
    row['focal_before'], row['focal_after'] = before[2][index], after[2][index]
    row['D1'] = reply.get('changed') is True and before[2][index] == after[2][index]
    origin = corners(width, height)[far]
    row['far_deviation_before'] = deviation(before, origin)
    row['far_deviation_after'] = deviation(after, origin)
    row['D2'] = row['far_deviation_after'] <= (1 - FAR_DROP) * row['far_deviation_before']

    # D4: one undo.
    call('undo', document_id=doc)
    row['D4'] = render(app, doc)[2] == before[2]
    receipts[f'depth_{depth}'] = row
    return doc, after


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    if not ready(STORE):
        print(f'SKIP depth blur: no fixture model store at {STORE}')
        return 0
    assert PORTRAIT.is_file(), PORTRAIT
    output = Path(args.receipts or tempfile.mkdtemp(prefix='depth-blur-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_depth_blur.py before any measurement'}
    gates = {}

    saved = os.environ.get('DR_MODELS_DIR')
    os.environ['DR_MODELS_DIR'] = str(STORE)
    try:
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            call = app.call
            models = call('list_models')['models']
            model = next((m for m in models if m['id'] == MODEL_ID), {})
            receipts['list_models'] = models
            gates['list_models_ready'] = model.get('state') == 'ready' and model.get('accepted') is True
            results = {}
            for depth in (8, 16, 32):
                results[depth] = drive_depth(app, depth, receipts, folder)
                for gate in ('D1', 'D2', 'D3', 'D4'):
                    gates[f'{gate}_{depth}'] = receipts[f'depth_{depth}'][gate]

            # D5: New Layer.
            doc = call('open_document', path=str(copy_of(folder, 'portrait-new-layer')))['document']['id']
            layers = len(call('get_document', document_id=doc)['layers'])
            base = render(app, doc)
            reply = call('depth_blur', document_id=doc, layer=0, output='new_layer', **SETTINGS)
            info = call('get_document', document_id=doc)
            names = [layer.get('name') for layer in info['layers']]
            call('set_layer_properties', document_id=doc, index=1, visible=False)
            hidden = render(app, doc)
            call('undo', document_id=doc)
            call('undo', document_id=doc)
            after_undo = len(call('get_document', document_id=doc)['layers'])
            receipts['D5'] = {'reply': reply, 'names': names, 'layers': [layers, len(info['layers']), after_undo]}
            gates['D5'] = (reply.get('output_layer') == 1 and len(info['layers']) == layers + 1
                           and names[-1].endswith(' Depth Blur') and hidden[2] == base[2]
                           and after_undo == layers)

            # D6: replay.
            app.call('save_action', name='depth-blur-replay', overwrite=True,
                     steps=[{'tool': 'depth_blur', 'arguments': {'layer': 0, **SETTINGS}}])
            app.call('run_action', document_id=doc, name='depth-blur-replay')
            replayed = render(app, doc)
            direct = results[8][1]
            worst = max(abs(a - b) for p, q in zip(replayed[2], direct[2]) for a, b in zip(p, q))
            receipts['D6'] = {'worst_level': worst}
            gates['D6'] = worst <= REPLAY_BOUND

            # D7: an empty layer, and focus_subject on a flat colour.
            call('new_layer', document_id=doc)
            message = call('depth_blur', fail=True, document_id=doc, layer=1, blur_strength=50)
            receipts['D7_empty'] = message
            gates['D7_empty'] = EMPTY in message
            flat = folder / 'flat.png'
            png(flat, 96, 64, bytes([90, 140, 200, 255]) * (96 * 64))
            flat_doc = call('open_document', path=str(flat))['document']['id']
            message = call('depth_blur', fail=True, document_id=flat_doc, layer=0, focus_subject=True)
            receipts['D7_no_subject'] = message
            gates['D7_no_subject'] = NO_SUBJECT in message
    finally:
        if saved is None:
            os.environ.pop('DR_MODELS_DIR', None)
        else:
            os.environ['DR_MODELS_DIR'] = saved

    # D7: not installed, on an instance whose store is an empty private folder.
    os.environ.pop('DR_MODELS_DIR', None)
    try:
        with App(folder / 'empty', folder / 'recovery-empty', interval=600) as app:
            doc = app.call('open_document', path=str(copy_of(folder, 'portrait-empty-store')))['document']['id']
            message = app.call('depth_blur', fail=True, document_id=doc, layer=0)
            models = app.call('list_models')['models']
            receipts['D7_not_installed'] = {'message': message, 'list_models': models}
            gates['D7_not_installed'] = NOT_INSTALLED in message and all(
                m['state'] == 'not_downloaded' for m in models if m['id'] == MODEL_ID)
    finally:
        if saved is not None:
            os.environ['DR_MODELS_DIR'] = saved

    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    failed = [name for name, ok in gates.items() if not ok]
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 1 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
