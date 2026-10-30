#!/usr/bin/env python3
"""The MCP drive: Select ▸ Sky (`select_sky`), Select ▸ Person Parts…
(`select_person_parts`) and the Object Finder (`object_finder_select`) with the real models
from the shared fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by `scripts/fill_model_fixtures.py`
(NETWORK) and only read here; without a ready sky, person-parts and SAM 2.1 model there the drive
prints `SKIP model selection: …` and exits 0. The photos are the phase-6 fixtures
(`scripts/phase6_quality_fixtures.py`): sky.jpg (Bo Jessen, CC0, 2048 × 1366), portrait.png
(NASA, public domain, 512 × 512), fur.png (CC0) and product.jpg (CC0, 2640 × 1860). The
instance is owned by this file (`phase5_support.App` on its own scratch MCP socket, a dummy API
key and a private directory for every preference library; DR_MODELS_DIR is the fixture store, or
an empty private folder for case R) and terminated by its own context manager. Masks are read
with `save_selection` and `render` of that channel; a mask pixel is "in" at 128 or more.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt and no bound
is widened afterwards.

- **S1** (8, 16 and 32 bits): sky.jpg's `select_sky` against the rasterized
  `scripts/model-drives/sky_reference.json` polygon (`select_polygon`): IoU ≥ 0.90.
- **S2**: the portrait (an indoor studio photo) refuses "No sky was found in this image."
  (Not a flat colour: the sky model reads any textureless frame as sky — a flat #402010, black,
  white or blue frame all come back ≥ 0.89 everywhere, measured 2026-10-14 — so a flat document
  is recorded beside the gate as `S2_flat`, not gated.)
- **P1**: the portrait's `face_skin`: ≥ 85 % of its pixels inside `inspect_faces` face 1's
  bounds grown by 10 % of their longer side on every side.
- **P2**: the portrait's `clothes`: its centroid lies below that grown box.
- **P3**: the union of all four parts lies inside `select_people`'s All People with at most 2 %
  of its pixels outside.
- **P4**: fur.png refuses "No people were found in this image."
- **O1**: product.jpg, one click at the camera's centre (1303, 924): IoU against
  `select_subject` ≥ 0.85, and the reply's `bounds` lie inside the mask's 50 % box grown by
  twice the edge stage's radius (one SAM cell, ⌈2640 / 256⌉ = 11 px; the guided filter mixes
  coefficients from within r, each fitted over a window of radius r, so it reaches 2r = 22 px),
  clamped to the canvas: SAM's logits are thresholded at 0, so no background haze widens them.
- **O2**: the box prompt around the camera (280, 180, 2040 × 1490): the same bound.
- **O3**: continuing O2 with an exclude point on the body cap (1544, 1030): coverage there below
  50 % (it was at or above 50 % after O2), and the reply's `bounds` inside its 50 % box grown the
  same way as O1's.
- **O4**: `continue` on a document with no live prompt refuses "continue needs the selection the
  previous object_finder_select made."
- **O5**: undoing the O3 step (after the undo of the channel saved from it) gives O2's
  selection exactly (the rendered channels are equal); and
  `run_action` of a saved `object_finder_select` step with O2's arguments on a fresh copy gives
  IoU ≥ 0.99 against O2.
- **R**: an instance whose store is empty refuses each tool with its not-installed sentence.
- **Q** (only when ~/devel/doctored-fixtures/multiple-people exists; the owner's photos, read in
  place, NUMBERS ONLY: nothing copied, written or rendered — masks are sampled on a 64 × 64 grid
  with `get_selection`): for each person of each photo, the union of the four parts on that
  person covers ≥ 50 % of their `select_subjects` model "people" mask, and any two people's parts
  overlap on ≤ 1 % of the smaller one.

Usage: python3 scripts/verify_model_selection.py [--receipts DIR]. Exit 0 only when every gate
passes.
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

import model_store_paths
from phase5_support import App
from verify_phase1 import png
from verify_phase2 import read_png

# ---- the expectations, fixed here BEFORE anything is measured -------------
STORE = Path.home() / 'devel/doctored-fixtures/models'
FIXTURES = Path(os.environ.get('DR_JOBS_PROBE_FIXTURES', str(Path.home() / 'devel/doctored-fixtures')))
PHOTOS = FIXTURES / 'phase6/photos'
PEOPLE = FIXTURES / 'multiple-people'
REFERENCE = json.loads((ROOT / 'scripts/model-drives/sky_reference.json').read_text())
SKY_IOU = 0.90
FACE_INSIDE, FACE_GROW = 0.85, 0.10
PEOPLE_OUTSIDE = 0.02
OBJECT_IOU = 0.85
REPLAY_IOU = 0.99
CAMERA_CENTRE = (1303, 924)
CAMERA_BOX = dict(x=280, y=180, width=2040, height=1490)
CAP = (1544, 1030)
EDGE_REACH = 2 * 11
PERSON_COVERED, PEOPLE_OVERLAP = 0.50, 0.01
GRID = 64
NO_SKY = 'No sky was found in this image.'
NO_PEOPLE = 'No people were found in this image.'
NO_PROMPT = 'continue needs the selection the previous object_finder_select made.'
NOT_INSTALLED = {
    'select_sky': 'the sky model is not installed; download it in Settings ▸ Models',
    'select_person_parts': 'the person parts model is not installed; download it in Settings ▸ Models',
    'object_finder_select': 'the Object Finder model is not installed; download SAM 2.1 in Settings ▸ Models',
}
MODELS = ['twistlabs/fotometis-sky', 'doctored/mediapipe-selfie-multiclass-256x256',
          'apple/coreml-sam2.1-large']
PARTS = ['hair', 'face_skin', 'body_skin', 'clothes']


def ready(store, model):
    return model_store_paths.is_ready(store, model)


def copy_of(folder, photo, name):
    path = folder / f'{name}{Path(photo).suffix}'
    path.write_bytes((PHOTOS / photo).read_bytes())
    return path


def mask(app, doc, name):
    """The current selection as a list of 0…255 values, through a saved channel."""
    app.call('save_selection', document_id=doc, name=name)
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': doc, 'max_side': 8192, 'channel': name}})
    assert not response.get('isError'), response
    width, height, pixels = read_png(base64.b64decode(
        next(i['data'] for i in response['content'] if i['type'] == 'image')))
    return width, height, [p[0] for p in pixels]


def iou(a, b):
    both = sum(1 for x, y in zip(a, b) if x >= 128 and y >= 128)
    either = sum(1 for x, y in zip(a, b) if x >= 128 or y >= 128)
    return both / either if either else 0.0


def bounds_inside(reply, values, width, height):
    """Whether the reply's bounds lie inside the 50 % box of `values` grown by EDGE_REACH."""
    inside = [i for i, v in enumerate(values) if v >= 128]
    if not inside or 'bounds' not in reply:
        return False
    xs = [i % width for i in inside]
    ys = [i // width for i in inside]
    left, top = max(0, min(xs) - EDGE_REACH), max(0, min(ys) - EDGE_REACH)
    right = min(width, max(xs) + 1 + EDGE_REACH)
    bottom = min(height, max(ys) + 1 + EDGE_REACH)
    b = reply['bounds']
    return (b['x'] >= left and b['y'] >= top and b['x'] + b['width'] <= right
            and b['y'] + b['height'] <= bottom)


def open_at(app, path, depth):
    doc = app.call('open_document', path=str(path))['document']['id']
    if depth in (16, 32):
        app.call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
    return doc


def sky_cases(app, folder, receipts, gates):
    for depth in (8, 16, 32):
        doc = open_at(app, copy_of(folder, 'sky.jpg', f'sky-{depth}'), depth)
        app.call('select_polygon', document_id=doc, points=REFERENCE['polygon'])
        _, _, reference = mask(app, doc, 'Reference')
        reply = app.call('select_sky', document_id=doc)
        _, _, sky = mask(app, doc, 'Sky')
        value = iou(sky, reference)
        receipts[f'S1_{depth}'] = dict(iou=value, reply=reply)
        gates[f'S1_{depth}'] = value >= SKY_IOU and reply.get('model', {}).get('id') == MODELS[0]
    doc = app.call('open_document', path=str(copy_of(folder, 'portrait.png', 'portrait-sky')))['document']['id']
    message = app.call('select_sky', fail=True, document_id=doc)
    receipts['S2'] = message
    gates['S2'] = NO_SKY in message
    flat = folder / 'flat.png'
    png(flat, 384, 256, bytes([0x40, 0x20, 0x10, 255]) * (384 * 256))
    doc = app.call('open_document', path=str(flat))['document']['id']
    response = app.client.rpc('tools/call', {'name': 'select_sky', 'arguments': {'document_id': doc}})
    receipts['S2_flat'] = next((c['text'] for c in response.get('content', []) if c['type'] == 'text'), '')


def parts_cases(app, folder, receipts, gates):
    doc = app.call('open_document', path=str(copy_of(folder, 'portrait.png', 'portrait')))['document']['id']
    face = app.call('inspect_faces', document_id=doc)['faces'][0]['bounds']
    grow = FACE_GROW * max(face['width'], face['height'])
    box = (face['x'] - grow, face['y'] - grow, face['x'] + face['width'] + grow, face['y'] + face['height'] + grow)
    reply = app.call('select_person_parts', document_id=doc, parts=['face_skin'])
    width, _, skin = mask(app, doc, 'FaceSkin')
    inside = [(i % width, i // width) for i, v in enumerate(skin) if v >= 128]
    share = sum(1 for x, y in inside if box[0] <= x + 0.5 <= box[2] and box[1] <= y + 0.5 <= box[3]) / max(1, len(inside))
    receipts['P1'] = dict(share=share, pixels=len(inside), box=box, reply=reply)
    gates['P1'] = bool(inside) and share >= FACE_INSIDE and reply.get('model', {}).get('id') == MODELS[1]
    app.call('select_person_parts', document_id=doc, parts=['clothes'])
    _, _, clothes = mask(app, doc, 'Clothes')
    rows = [i // width for i, v in enumerate(clothes) if v >= 128]
    centroid = sum(rows) / len(rows) + 0.5 if rows else None
    receipts['P2'] = dict(centroid_y=centroid, box_bottom=box[3])
    gates['P2'] = centroid is not None and centroid > box[3]
    app.call('select_person_parts', document_id=doc, parts=PARTS)
    _, _, union = mask(app, doc, 'Parts')
    app.call('select_people', document_id=doc)
    _, _, people = mask(app, doc, 'People')
    covered = [i for i, v in enumerate(union) if v >= 128]
    outside = sum(1 for i in covered if people[i] < 128) / max(1, len(covered))
    receipts['P3'] = dict(outside=outside, pixels=len(covered))
    gates['P3'] = bool(covered) and outside <= PEOPLE_OUTSIDE
    fur = app.call('open_document', path=str(copy_of(folder, 'fur.png', 'fur')))['document']['id']
    message = app.call('select_person_parts', fail=True, document_id=fur, parts=['hair'])
    receipts['P4'] = message
    gates['P4'] = NO_PEOPLE in message


def finder_cases(app, folder, receipts, gates):
    doc = app.call('open_document', path=str(copy_of(folder, 'product.jpg', 'product')))['document']['id']
    app.call('select_subject', document_id=doc)
    _, _, subject = mask(app, doc, 'Subject')
    reply = app.call('object_finder_select', document_id=doc,
                     points=[{'x': CAMERA_CENTRE[0], 'y': CAMERA_CENTRE[1]}])
    width, height, clicked = mask(app, doc, 'Click')
    receipts['O1'] = dict(iou=iou(clicked, subject), reply=reply,
                          bounds_inside=bounds_inside(reply, clicked, width, height))
    gates['O1'] = (receipts['O1']['iou'] >= OBJECT_IOU and receipts['O1']['bounds_inside']
                   and reply.get('model', {}).get('id') == MODELS[2])
    reply = app.call('object_finder_select', document_id=doc, box=CAMERA_BOX)
    _, _, boxed = mask(app, doc, 'Box')
    receipts['O2'] = dict(iou=iou(boxed, subject), reply=reply)
    gates['O2'] = receipts['O2']['iou'] >= OBJECT_IOU
    cap = CAP[1] * width + CAP[0]
    reply = app.call('object_finder_select', document_id=doc, box=CAMERA_BOX, **{'continue': True},
                     points=[{'x': CAP[0], 'y': CAP[1], 'include': False}])
    _, _, excluded = mask(app, doc, 'Excluded')
    # Undo the channel save, then the O3 step itself: O2's selection must be back.
    app.call('undo', document_id=doc)
    app.call('undo', document_id=doc)
    _, _, undone = mask(app, doc, 'Undone')
    receipts['O3'] = dict(before=boxed[cap], after=excluded[cap], reply=reply,
                          bounds_inside=bounds_inside(reply, excluded, width, height))
    gates['O3'] = (boxed[cap] >= 128 and excluded[cap] < 128 and reply.get('continued') is True
                   and receipts['O3']['bounds_inside'])
    receipts['O5_undo'] = dict(iou=iou(undone, boxed))
    gates['O5_undo'] = undone == boxed
    fresh = app.call('open_document', path=str(copy_of(folder, 'product.jpg', 'product-fresh')))['document']['id']
    message = app.call('object_finder_select', fail=True, document_id=fresh, **{'continue': True},
                       points=[{'x': CAP[0], 'y': CAP[1]}])
    receipts['O4'] = message
    gates['O4'] = NO_PROMPT in message
    app.call('save_action', name='object-finder-replay', overwrite=True,
             steps=[{'tool': 'object_finder_select', 'arguments': {'box': CAMERA_BOX}}])
    app.call('run_action', document_id=fresh, name='object-finder-replay')
    _, _, replayed = mask(app, fresh, 'Replay')
    receipts['O5_replay'] = dict(iou=iou(replayed, boxed))
    gates['O5_replay'] = receipts['O5_replay']['iou'] >= REPLAY_IOU


def grid(app, doc, points):
    out = []
    for start in range(0, len(points), 4096):
        reply = app.call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for x, y in points[start:start + 4096]])
        out.extend(row['coverage16'] >= 32768 for row in reply.get('samples') or [])
    return out


def people_cases(app, receipts, gates):
    """Q: the owner's photos in place; numbers only."""
    rows = []
    for photo in sorted(p for p in PEOPLE.iterdir() if p.suffix.lower() in ('.jpg', '.jpeg', '.png')):
        doc = app.call('open_document', path=str(photo))['document']['id']
        state = app.call('get_document', document_id=doc)
        width, height = state['width'], state['height']
        points = [(int((i + 0.5) * width / GRID), int((j + 0.5) * height / GRID))
                  for j in range(GRID) for i in range(GRID)]
        found = app.call('inspect_subjects', document_id=doc, model='people')
        count = len(found.get('instances') or [])
        persons, parts = [], []
        for k in range(1, count + 1):
            app.call('select_subjects', document_id=doc, model='people', instances=[k])
            persons.append(grid(app, doc, points))
            app.call('select_person_parts', document_id=doc, parts=PARTS, people=[k])
            parts.append(grid(app, doc, points))
        covered = [sum(1 for p, q in zip(person, part) if p and q) / max(1, sum(person))
                   for person, part in zip(persons, parts)]
        overlaps = []
        for a in range(count):
            for b in range(a + 1, count):
                both = sum(1 for p, q in zip(parts[a], parts[b]) if p and q)
                overlaps.append(both / max(1, min(sum(parts[a]), sum(parts[b]))))
        rows.append(dict(people=count, covered=[round(c, 4) for c in covered],
                         worst_overlap=round(max(overlaps, default=0.0), 4)))
        gates[f'Q_{len(rows)}'] = count > 0 and all(c >= PERSON_COVERED for c in covered) \
            and max(overlaps, default=0.0) <= PEOPLE_OVERLAP
    receipts['Q'] = rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    missing = [model for model in MODELS if not ready(STORE, model)]
    if missing:
        print(f'SKIP model selection: no ready {", ".join(missing)} in the fixture store {STORE}')
        return 0
    for photo in ('sky.jpg', 'portrait.png', 'fur.png', 'product.jpg'):
        assert (PHOTOS / photo).is_file(), PHOTOS / photo
    output = Path(args.receipts or tempfile.mkdtemp(prefix='model-selection-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_model_selection.py before any measurement'}
    gates = {}
    saved = os.environ.get('DR_MODELS_DIR')
    os.environ['DR_MODELS_DIR'] = str(STORE)
    try:
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            sky_cases(app, folder, receipts, gates)
            parts_cases(app, folder, receipts, gates)
            finder_cases(app, folder, receipts, gates)
            if PEOPLE.is_dir():
                people_cases(app, receipts, gates)
            else:
                receipts['Q'] = f'skipped: {PEOPLE} does not exist'
    finally:
        if saved is None:
            os.environ.pop('DR_MODELS_DIR', None)
        else:
            os.environ['DR_MODELS_DIR'] = saved

    # R: an instance whose store is an empty private folder.
    empty = folder / 'empty-store'
    empty.mkdir()
    os.environ['DR_MODELS_DIR'] = str(empty)
    try:
        with App(folder / 'empty', folder / 'recovery-empty', interval=600) as app:
            doc = app.call('open_document', path=str(copy_of(folder, 'product.jpg', 'product-empty')))['document']['id']
            messages = {
                'select_sky': app.call('select_sky', fail=True, document_id=doc),
                'select_person_parts': app.call('select_person_parts', fail=True, document_id=doc, parts=['hair']),
                'object_finder_select': app.call('object_finder_select', fail=True, document_id=doc,
                                                 points=[{'x': 10, 'y': 10}]),
            }
            receipts['R'] = messages
            for tool, message in messages.items():
                gates[f'R_{tool}'] = NOT_INSTALLED[tool] in message
    finally:
        if saved is None:
            os.environ.pop('DR_MODELS_DIR', None)
        else:
            os.environ['DR_MODELS_DIR'] = saved

    receipts['gates'] = gates
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, default=str))
    for name, ok in gates.items():
        print(f'{"PASS" if ok else "FAIL"} {name}')
    print(f'receipt: {output / "receipt.json"}')
    return 0 if all(gates.values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
