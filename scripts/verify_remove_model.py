#!/usr/bin/env python3
"""The M3a (phases 1 and 2) MCP drive: the Remove tool's Model mode (`remove_stroke`, mode
"model", the default) on the Debug app with the real MI-GAN model from the shared fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by
`scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release --only
doctored/migan-512-places2` and only read here; without a ready MI-GAN there the drive prints
`SKIP remove model: …` on stderr and exits 2 (0 only with `--allow-skip`), so a missing store is
never read as a pass. The photo is the phase-6 fixture sky.jpg (CC0), resized to
1200 × 800 with `sips`; every other picture is generated here. Each instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket, a dummy API key, a private folder for every
preference library, DR_BACKGROUND=1); DR_MODELS_DIR is the fixture store, or for case RN an empty
scratch store. Whole pictures are read back through `save_copy` and the independent PNG reader
(`scripts/raw-probe/native.py`), at the document's own depth.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

At 8 and 16 bits (M, U, S, N):
- **M** `remove_stroke` with points (500, 300)–(700, 340), size 60, no mode: ok, completed, mode
  "model", model "doctored/migan-512-places2 @ migan-512-places2-coreml-1", crop [x, y, w, h]
  holding the stroke's box with w, h ≥ 512, planned_bytes > 0, covered_pixels within 15 % of
  the stroke's area; more than half the pixels within size/2 − 2 of the centre line changed, and
  every pixel farther than size/2 + 1 from it is bit-exact.
- **U** one undo restores the picture bit-exact and the history cursor.
- **S** use_selection over select_rect (100, 500, 120 × 80): every pixel outside the rect is
  bit-exact and some inside changed; then undo.
- **N** output new_layer: two layers, the new one "Remove" at index 1, layer 0's own sample at the
  stroke's centre unchanged and more than half the composite's pixels within size/2 − 2 of the
  centre line changed; then undo.

At 8 bits only:
- **A** `run_action` of a hand-written `remove_stroke` step with no mode (as the v1
  retouching batch recorded it) replays through the model and changes the pixels under the
  stroke.
- **C** mode "content_aware" still runs: completed, mode "content_aware".
- **K** (phase 2) one `remove_stroke` call with `strokes` [STROKE, STROKE2] (the tool's Apply
  with "Remove after each stroke" off): ok, completed, applied.strokes 2, ONE history step; more
  than half the pixels within size/2 − 2 of EACH centre line changed, and every pixel farther
  than size/2 + 1 from both is bit-exact; then undo.
- **RP** use_selection with points: "remove_stroke takes one of points, strokes or
  use_selection".
- **RA** (phase 2) use_selection over `select_all`: "the hole covers the whole crop: there is no
  picture around it to fill from" (the hole leaves no context, not a transparent surround).
- **RS** use_selection with no selection: "use_selection needs a selection".
- **RT** a disc alone on layer 1, a stroke over the whole disc, sample current: "there is nothing
  to sample from: every pixel around the stroke is transparent."
- **R32** a 32-bit document (`convert_precision`): "Remove's model is not available in a 32-bit
  document".
- **RC** a CMYK document (`convert_mode`): "Remove's model is not available in a CMYK document".
  (R32 and RC stay refusals and M/U/S/N run at 8 and 16 bits only.)
- **RN** a second instance on an empty scratch DR_MODELS_DIR: "the Remove model is not installed;
  download MI-GAN in Settings ▸ Models".

Usage: python3 scripts/verify_remove_model.py [--receipts DIR] [--allow-skip]. Prints PASS/FAIL
per gate and exits 0 only when every gate passes (2 on a SKIP without --allow-skip).
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import model_store_paths  # noqa: E402
from phase5_support import App  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
STORE = Path.home() / 'devel/doctored-fixtures/models'
PHOTO = Path.home() / 'devel/doctored-fixtures/phase6/photos/sky.jpg'
MODEL = 'doctored/migan-512-places2'
VERSION = 'migan-512-places2-coreml-1'
WIDTH, HEIGHT = 1200, 800
STROKE = dict(points=[[500, 300], [700, 340]], size=60)
# In the clouds: run 1 put it at (300, 600)–(420, 610), inside the black roofline silhouette,
# where a fill that reproduces black changes no pixel (scratch/remove-drive-1's receipt).
STROKE2 = dict(points=[[250, 150], [370, 160]], size=40)
SELECTION = (100, 500, 120, 80)
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
NOT_32 = "Remove's model is not available in a 32-bit document"
NOT_CMYK = "Remove's model is not available in a CMYK document"
NOT_INSTALLED = 'the Remove model is not installed; download MI-GAN in Settings ▸ Models'
NOTHING = 'there is nothing to sample from: every pixel around the stroke is transparent.'
NO_CONTEXT = 'the hole covers the whole crop: there is no picture around it to fill from'
EXCLUSIVE = 'remove_stroke takes one of points, strokes or use_selection'


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


png_samples = _load('remove_model_png_reader', 'scripts/raw-probe/native.py').png_samples


def ready(store, model):
    return model_store_paths.is_ready(store, model)


def photo(folder):
    """sky.jpg resized to WIDTH × HEIGHT, as a PNG in `folder`."""
    path = folder / 'sky-1200.png'
    if not path.is_file():
        subprocess.run(['sips', '-s', 'format', 'png', '-z', str(HEIGHT), str(WIDTH), str(PHOTO),
                        '--out', str(path)], check=True, capture_output=True)
    return path


OPENED = []


def opened(app, folder, depth):
    """A fresh document of the photo at `depth`: each open reads its own copy, since opening a
    path that is already open answers that document."""
    OPENED.append(depth)
    path = folder / f'sky-{len(OPENED)}-{depth}.png'
    path.write_bytes(photo(folder).read_bytes())
    doc = app.call('open_document', path=str(path), precision='rgba8')['document']['id']
    if depth != 8:
        app.call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
    return doc


def shot(app, doc, folder, name):
    path = folder / f'{name}.png'
    app.call('save_copy', document_id=doc, path=str(path))
    decoded = png_samples(path.read_bytes())
    return decoded['width'], decoded['samples']


def pixel(picture, x, y):
    width, samples = picture
    i = (y * width + x) * 4
    return tuple(samples[i:i + 4])


def segment_distance(x, y, a, b):
    px, py = x + 0.5 - a[0], y + 0.5 - a[1]
    dx, dy = b[0] - a[0], b[1] - a[1]
    t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - t * dx, py - t * dy)


def stroke_masks(strokes=(STROKE,)):
    """Per stroke the pixels inside it (≤ size/2 − 2), and the pixels outside every stroke
    (> size/2 + 1 from each). One stroke answers (inside, outside)."""
    insides = [[] for _ in strokes]
    outside = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            far = True
            for k, stroke in enumerate(strokes):
                a, b = stroke['points']
                half = stroke['size'] / 2
                d = segment_distance(x, y, a, b)
                if d <= half - 2:
                    insides[k].append((x, y))
                if d <= half + 1:
                    far = False
            if far:
                outside.append((x, y))
    return (insides[0], outside) if len(strokes) == 1 else (insides, outside)


def cursor(app, doc):
    return app.call('list_history', document_id=doc)['cursor']


def layer_names(app, doc):
    return [l.get('name') for l in app.call('get_document', document_id=doc).get('layers') or []]


def codes(app, doc, x, y, layer=None):
    scope = dict(source='composite') if layer is None else dict(source='layer', layer=layer)
    reply = app.call('sample_pixel', document_id=doc, x=x, y=y, **scope)
    return tuple(int(v) for v in reply['center_rgba_codes'])


def depth_cases(app, folder, depth, receipts, gates, masks):
    inside, outside = masks
    doc = opened(app, folder, depth)
    before = shot(app, doc, folder, f'before-{depth}')
    start = cursor(app, doc)
    reply = app.call('remove_stroke', document_id=doc, layer=0, **STROKE)
    after = shot(app, doc, folder, f'model-{depth}')
    changed = sum(1 for x, y in inside if pixel(after, x, y) != pixel(before, x, y))
    moved_outside = sum(1 for x, y in outside if pixel(after, x, y) != pixel(before, x, y))
    a, b = STROKE['points']
    half = STROKE['size'] / 2
    box = (min(a[0], b[0]) - half, min(a[1], b[1]) - half,
           max(a[0], b[0]) + half, max(a[1], b[1]) + half)
    crop = reply.get('crop') or [0, 0, 0, 0]
    area = math.hypot(b[0] - a[0], b[1] - a[1]) * STROKE['size'] + math.pi * half * half
    receipts[f'M{depth}'] = dict(reply=reply, changed_inside=changed, inside=len(inside),
                                 moved_outside=moved_outside, area=area)
    gates[f'M{depth}'] = (
        reply.get('ok') is True and reply.get('state') == 'completed' and reply.get('mode') == 'model'
        and reply.get('model') == f'{MODEL} @ {VERSION}' and len(crop) == 4
        and crop[0] <= box[0] and crop[1] <= box[1] and crop[0] + crop[2] >= box[2]
        and crop[1] + crop[3] >= box[3] and crop[2] >= 512 and crop[3] >= 512
        and reply.get('planned_bytes', 0) > 0
        and abs(reply.get('covered_pixels', 0) - area) <= 0.15 * area
        and changed > len(inside) / 2 and moved_outside == 0
        and cursor(app, doc) == start + 1)

    app.call('undo', document_id=doc)
    undone = shot(app, doc, folder, f'undone-{depth}')
    receipts[f'U{depth}'] = dict(cursor=cursor(app, doc), start=start)
    gates[f'U{depth}'] = undone[1] == before[1] and cursor(app, doc) == start

    x0, y0, w, h = SELECTION
    app.call('select_rect', document_id=doc, x=x0, y=y0, width=w, height=h)
    reply = app.call('remove_stroke', document_id=doc, layer=0, use_selection=True)
    selected = shot(app, doc, folder, f'selection-{depth}')
    outside_rect = sum(1 for i in range(WIDTH * HEIGHT)
                       if not (x0 <= i % WIDTH < x0 + w and y0 <= i // WIDTH < y0 + h)
                       and selected[1][i * 4:i * 4 + 4] != before[1][i * 4:i * 4 + 4])
    inside_rect = sum(1 for y in range(y0, y0 + h) for x in range(x0, x0 + w)
                      if pixel(selected, x, y) != pixel(before, x, y))
    receipts[f'S{depth}'] = dict(reply=reply, moved_outside=outside_rect, changed_inside=inside_rect)
    gates[f'S{depth}'] = (reply.get('state') == 'completed' and outside_rect == 0 and inside_rect > 0
                          and reply.get('covered_pixels') == w * h)
    app.call('undo', document_id=doc)
    app.call('deselect', document_id=doc)

    centre = (600, 320)
    base = codes(app, doc, *centre, layer=0)
    reply = app.call('remove_stroke', document_id=doc, layer=0, output='new_layer', **STROKE)
    names = layer_names(app, doc)
    layered = shot(app, doc, folder, f'new-layer-{depth}')
    changed = sum(1 for x, y in inside if pixel(layered, x, y) != pixel(before, x, y))
    receipts[f'N{depth}'] = dict(reply=reply, names=names, base=base, changed_inside=changed)
    gates[f'N{depth}'] = (reply.get('state') == 'completed' and len(names) == 2 and names[1] == 'Remove'
                          and codes(app, doc, *centre, layer=0) == base and changed > len(inside) / 2)
    app.call('undo', document_id=doc)
    return doc, before


def eight_bit_cases(app, folder, doc, before, receipts, gates, masks):
    inside, _ = masks
    insides, outside = stroke_masks((STROKE, STROKE2))
    start = cursor(app, doc)
    reply = app.call('remove_stroke', document_id=doc, layer=0, strokes=[STROKE, STROKE2])
    both = shot(app, doc, folder, 'strokes-8')
    changed = [sum(1 for x, y in each if pixel(both, x, y) != pixel(before, x, y))
               for each in insides]
    moved_outside = sum(1 for x, y in outside if pixel(both, x, y) != pixel(before, x, y))
    receipts['K'] = dict(reply=reply, changed_inside=changed, inside=[len(e) for e in insides],
                         moved_outside=moved_outside, steps=cursor(app, doc) - start)
    gates['K'] = (reply.get('ok') is True and reply.get('state') == 'completed'
                  and (reply.get('applied') or {}).get('strokes') == 2
                  and cursor(app, doc) == start + 1 and moved_outside == 0
                  and all(c > len(e) / 2 for c, e in zip(changed, insides)))
    app.call('undo', document_id=doc)

    app.call('save_action', name='remove-replay', overwrite=True,
             steps=[{'tool': 'remove_stroke', 'arguments': {'layer': 0, **STROKE}}])
    app.call('run_action', document_id=doc, name='remove-replay')
    replayed = shot(app, doc, folder, 'replayed-8')
    changed = sum(1 for x, y in inside if pixel(replayed, x, y) != pixel(before, x, y))
    receipts['A'] = dict(changed_inside=changed)
    gates['A'] = changed > len(inside) / 2
    app.call('undo', document_id=doc)

    reply = app.call('remove_stroke', document_id=doc, layer=0, mode='content_aware', **STROKE)
    receipts['C'] = reply
    gates['C'] = reply.get('state') == 'completed' and reply.get('mode') == 'content_aware'
    app.call('undo', document_id=doc)

    app.call('select_rect', document_id=doc, x=10, y=10, width=50, height=50)
    message = app.call('remove_stroke', fail=True, document_id=doc, layer=0, use_selection=True,
                       **STROKE)
    receipts['RP'] = message
    gates['RP'] = EXCLUSIVE in message
    app.call('deselect', document_id=doc)
    app.call('select_all', document_id=doc)
    message = app.call('remove_stroke', fail=True, document_id=doc, layer=0, use_selection=True)
    receipts['RA'] = message
    gates['RA'] = NO_CONTEXT in message
    app.call('deselect', document_id=doc)
    message = app.call('remove_stroke', fail=True, document_id=doc, layer=0, use_selection=True)
    receipts['RS'] = message
    gates['RS'] = 'use_selection needs a selection' in message

    app.call('new_layer', document_id=doc)
    app.call('select_ellipse', document_id=doc, x=580, y=380, width=40, height=40)
    app.call('fill_selection', document_id=doc, layer=1, color='#FF00FF')
    app.call('deselect', document_id=doc)
    message = app.call('remove_stroke', fail=True, document_id=doc, layer=1, sample='current',
                       points=[[600, 400]], size=80)
    receipts['RT'] = message
    gates['RT'] = NOTHING in message

    float_doc = opened(app, folder, 32)
    message = app.call('remove_stroke', fail=True, document_id=float_doc, layer=0, **STROKE)
    receipts['R32'] = message
    gates['R32'] = NOT_32 in message

    cmyk = opened(app, folder, 8)
    app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(GENERIC))
    message = app.call('remove_stroke', fail=True, document_id=cmyk, layer=0, **STROKE)
    receipts['RC'] = message
    gates['RC'] = NOT_CMYK in message


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--allow-skip', action='store_true',
                        help='exit 0 instead of 2 when the fixture store has no ready model')
    args = parser.parse_args()
    if not ready(STORE, MODEL):
        print(f'SKIP remove model: no ready {MODEL} in the fixture store {STORE}', file=sys.stderr)
        return 0 if args.allow_skip else 2
    assert PHOTO.is_file(), PHOTO
    output = Path(args.receipts or tempfile.mkdtemp(prefix='remove-model-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_remove_model.py before any measurement'}
    gates = {}
    masks = stroke_masks()
    saved = {key: os.environ.get(key) for key in ('DR_MODELS_DIR', 'DR_BACKGROUND')}
    try:
        os.environ['DR_BACKGROUND'] = '1'
        os.environ['DR_MODELS_DIR'] = str(STORE)
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            first = None
            for depth in (8, 16):
                doc, before = depth_cases(app, folder, depth, receipts, gates, masks)
                if depth == 8:
                    first = (doc, before)
            eight_bit_cases(app, folder, *first, receipts, gates, masks)
        empty = folder / 'empty-store'
        empty.mkdir()
        os.environ['DR_MODELS_DIR'] = str(empty)
        with App(folder / 'bare', folder / 'recovery-bare', interval=600) as app:
            doc = opened(app, folder, 8)
            message = app.call('remove_stroke', fail=True, document_id=doc, layer=0, **STROKE)
            receipts['RN'] = message
            gates['RN'] = NOT_INSTALLED in message
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 0 if gates and all(gates.values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
