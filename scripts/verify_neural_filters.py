#!/usr/bin/env python3
"""The M4 phase 1 MCP drive: Filter ▸ Neural Filters ▸ Super Zoom (`super_zoom`) and JPEG
Artifacts Removal (`jpeg_artifacts_removal`) on the Debug app with the real Real-ESRGAN x4plus and
FBCNN models from the shared fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by
`scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release --only
doctored/realesrgan-x4plus` (and `--only doctored/fbcnn-color`) and only read here; without both
models ready there the drive prints `SKIP neural filters: …` on stderr and exits 2 (0 only with
`--allow-skip`). The photo is the phase-6 fixture sky.jpg (CC0), resized to 320 × 240 with
`sips`, and for the JPEG cases re-encoded by `sips` at JPEG quality 20; every other picture is
generated here. Each instance is owned by this file (`phase5_support.App` on its own scratch MCP
socket, a dummy API key, a private folder for every preference library, DR_BACKGROUND=1);
DR_MODELS_DIR is the fixture store, or for case RN an empty scratch store. Pictures are read
back through `save_copy` and the independent PNG reader (`scripts/raw-probe/native.py`).

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

Super Zoom, at 8 and 16 bits and ×2 and ×4 (SZ8x2, SZ8x4, SZ16x2, SZ16x4):
- ok; a NEW document_id; width/height = 320·N × 240·N; zoom N; tiles = ⌈320/224⌉·⌈240/224⌉ = 4;
  planned_bytes > 0; models = [doctored/realesrgan-x4plus @ realesrgan-x4plus-coreml-1]; the new
  document has one layer "Background", the source's precision, a name holding "Super Zoom ×N";
  every output alpha is opaque; the mean |model − classical| RGB (the same call with
  enhance_details false) is above 0 and at most MAX_MODEL_DELTA codes of 255; the source's
  history cursor and pixels are unchanged.
- **SZJ** (8 bits) remove_jpeg_artifacts true: models name both, tiles = 1 + 4.
- **SZC** classical ×2 of a 48 × 32 generated picture (8 bits): every sample within 1 code of an
  independent Catmull-Rom oracle (s = (o + 0.5)/2, clamped taps, alpha-weighted).
- **SZL** a 4000 × 3000 document at ×4: "the zoomed image would be 16000 × 12000 (192.0
  megapixels), past the 100-megapixel limit: choose a smaller zoom".

JPEG Artifacts Removal on the quality-20 JPEG at 8 and 16 bits, strength low and high, output
layer and new_layer (J8-low-layer, …, J16-high-new_layer):
- ok, changed, quality_factor 70 / 10, tiles 1, the model doctored/fbcnn-color @
  fbcnn-color-coreml-1; layer: one layer, some pixel changed, every alpha kept; new_layer: two
  layers, layer 1 "Background JPEG Artifacts Removal", layer 0's pixels unchanged; ONE history
  step, and one undo restores the picture bit-exact and the cursor.
- **JS** at each depth the high result moves farther from the input than the low one (mean |Δ|).

Refusals and replay:
- **R32** a 32-bit document: both tools refused, "<tool> is unavailable in a 32-bit float
  document".
- **RC** a CMYK document: both refused, "<tool> is unavailable in a CMYK document".
- **RN** a second instance on an empty scratch DR_MODELS_DIR: "the Super Zoom model is not
  installed; download Real-ESRGAN in Settings ▸ Models" and "the JPEG Artifacts Removal model is
  not installed; download FBCNN in Settings ▸ Models".
- **AZ** `run_action` of a saved `super_zoom` step (zoom 2) opens one more document of 640 × 480.
- **AJ** `run_action` of a saved `jpeg_artifacts_removal` step (strength high) changes the
  pixels in one step.
- **RZ** with one document open, a recording of `super_zoom` (zoom 2) that names no document_id
  holds exactly that one `super_zoom` step, not the duplicated-document placeholder.

Usage: python3 scripts/verify_neural_filters.py [--receipts DIR] [--allow-skip]. Prints PASS/FAIL
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
ZOOM_MODEL = ('doctored/realesrgan-x4plus', 'realesrgan-x4plus-coreml-1')
JPEG_MODEL = ('doctored/fbcnn-color', 'fbcnn-color-coreml-1')
WIDTH, HEIGHT = 320, 240
TILES = math.ceil(WIDTH / 224) * math.ceil(HEIGHT / 224)
# The model's detail against the classical resample: a sanity band, not a quality figure.
MAX_MODEL_DELTA = 24.0
CLASSICAL = (48, 32)
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
LIMIT = ('the zoomed image would be 16000 × 12000 (192.0 megapixels), past the 100-megapixel '
         'limit: choose a smaller zoom')
NOT_INSTALLED_ZOOM = 'the Super Zoom model is not installed; download Real-ESRGAN in Settings ▸ Models'
NOT_INSTALLED_JPEG = ('the JPEG Artifacts Removal model is not installed; download FBCNN in '
                      'Settings ▸ Models')
QUALITY = {'low': 70, 'high': 10}
# get_document's own `precision` words (run 1 had 'u8-non-linear', another tool's).
PRECISION = {8: 'rgba8', 16: 'rgba16'}


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


png_samples = _load('neural_png_reader', 'scripts/raw-probe/native.py').png_samples


def ready(store, model):
    return model_store_paths.is_ready(store, model)


def photo(folder, jpeg=False):
    """sky.jpg resized to WIDTH × HEIGHT as a PNG, or re-encoded as a quality-20 JPEG."""
    path = folder / ('sky-q20.jpg' if jpeg else 'sky-320.png')
    if not path.is_file():
        command = ['sips', '-s', 'format', 'jpeg', '-s', 'formatOptions', '20'] if jpeg else \
            ['sips', '-s', 'format', 'png']
        subprocess.run(command + ['-z', str(HEIGHT), str(WIDTH), str(PHOTO), '--out', str(path)],
                       check=True, capture_output=True)
    return path


OPENED = []


def opened(app, folder, depth, jpeg=False):
    """A fresh document of the photo at `depth` (each open reads its own copy)."""
    OPENED.append(depth)
    source = photo(folder, jpeg)
    path = folder / f'open-{len(OPENED)}-{depth}{source.suffix}'
    path.write_bytes(source.read_bytes())
    doc = app.call('open_document', path=str(path), precision='rgba8')['document']['id']
    if depth != 8:
        app.call('convert_precision', document_id=doc, mode=f'rgba{depth}', structure='preserve')
    return doc


def shot(app, doc, folder, name):
    path = folder / f'{name}.png'
    app.call('save_copy', document_id=doc, path=str(path))
    decoded = png_samples(path.read_bytes())
    return decoded['width'], decoded['samples']


def layer_shot(app, doc, folder, name, layer):
    """Layer `layer` alone, read back through a duplicate holding only it."""
    reply = app.call('duplicate_to_new_document', document_id=doc, layers=[layer])
    copy = reply['document']['id']
    picture = shot(app, copy, folder, name)
    app.call('close_document', document_id=copy, discard_changes=True)
    return picture


def cursor(app, doc):
    return app.call('list_history', document_id=doc)['cursor']


def info(app, doc):
    return app.call('get_document', document_id=doc)


def names(app, doc):
    return [layer.get('name') for layer in info(app, doc).get('layers') or []]


def document_title(app, doc):
    for entry in app.call('list_documents').get('documents', []):
        if entry.get('id') == doc:
            return ' '.join(str(v) for v in entry.values() if isinstance(v, str))
    return ''


def mean_delta(a, b, scale):
    """The mean |a − b| over RGB, in codes of 255."""
    total, count = 0, 0
    for i in range(0, min(len(a), len(b)), 4):
        for c in range(3):
            total += abs(a[i + c] - b[i + c])
            count += 1
    return total / max(1, count) / scale


def catmull_rom(x):
    x = abs(x)
    if x < 1:
        return (1.5 * x - 2.5) * x * x + 1
    if x < 2:
        return ((-0.5 * x + 2.5) * x - 4) * x + 2
    return 0.0


def classical(samples, w, h, scale):
    """The independent Catmull-Rom oracle (module doc), straight RGBA8 codes."""
    out = []
    def taps(o):
        p = (o + 0.5) / scale - 0.5
        base = math.floor(p)
        return [(t, catmull_rom(p - t)) for t in range(base - 1, base + 3)]
    for oy in range(h * scale):
        ty = taps(oy)
        for ox in range(w * scale):
            acc = [0.0] * 4
            for yy, wy in ty:
                y = min(max(yy, 0), h - 1)
                for xx, wx in taps(ox):
                    x = min(max(xx, 0), w - 1)
                    i = (y * w + x) * 4
                    a = samples[i + 3]
                    weight = wx * wy
                    for c in range(3):
                        acc[c] += weight * a / 255 * samples[i + c]
                    acc[3] += weight * a
            alpha = round(min(max(acc[3], 0), 255))
            if alpha == 0:
                out.extend([0, 0, 0, 0])
                continue
            out.extend([round(min(max(acc[c] * 255 / acc[3], 0), 255)) for c in range(3)] + [alpha])
    return out


def zoom_cases(app, folder, receipts, gates):
    for depth in (8, 16):
        scale = 1 if depth == 8 else 257
        source = opened(app, folder, depth)
        before = shot(app, source, folder, f'zoom-source-{depth}')
        start = cursor(app, source)
        for zoom in (2, 4):
            name = f'SZ{depth}x{zoom}'
            reply = app.call('super_zoom', document_id=source, layer=0, zoom=zoom)
            made = reply.get('document_id')
            plain = app.call('super_zoom', document_id=source, layer=0, zoom=zoom, enhance_details=False)
            enhanced = shot(app, made, folder, f'{name}-model')
            resampled = shot(app, plain['document_id'], folder, f'{name}-classical')
            alpha = all(enhanced[1][i + 3] == 255 * scale for i in range(0, len(enhanced[1]), 4))
            delta = mean_delta(enhanced[1], resampled[1], scale)
            details = info(app, made)
            title = document_title(app, made)
            receipts[name] = dict(reply=reply, delta=delta, title=title,
                                  precision=details.get('precision'), layers=names(app, made))
            gates[name] = (
                reply.get('ok') is True and made not in (None, source)
                and reply.get('width') == WIDTH * zoom and reply.get('height') == HEIGHT * zoom
                and enhanced[0] == WIDTH * zoom and reply.get('zoom') == zoom
                and reply.get('tiles') == TILES and reply.get('planned_bytes', 0) > 0
                and reply.get('models') == [dict(id=ZOOM_MODEL[0], version=ZOOM_MODEL[1])]
                and names(app, made) == ['Background'] and details.get('precision') == PRECISION[depth]
                and f'Super Zoom ×{zoom}' in title and alpha and 0 < delta <= MAX_MODEL_DELTA
                and cursor(app, source) == start)
            for doc in (made, plain['document_id']):
                app.call('close_document', document_id=doc, discard_changes=True)
        after = shot(app, source, folder, f'zoom-source-after-{depth}')
        receipts[f'SZS{depth}'] = dict(cursor=cursor(app, source), start=start)
        gates[f'SZS{depth}'] = after[1] == before[1] and cursor(app, source) == start
        if depth == 8:
            reply = app.call('super_zoom', document_id=source, layer=0, zoom=2, remove_jpeg_artifacts=True)
            receipts['SZJ'] = reply
            gates['SZJ'] = (reply.get('ok') is True and reply.get('tiles') == 1 + TILES
                            and reply.get('models') == [dict(id=ZOOM_MODEL[0], version=ZOOM_MODEL[1]),
                                                        dict(id=JPEG_MODEL[0], version=JPEG_MODEL[1])])
            app.call('close_document', document_id=reply['document_id'], discard_changes=True)

    w, h = CLASSICAL
    small = app.call('new_document', width=w, height=h)['document']['id']
    app.call('gradient', document_id=small, layer=0, x0=0, y0=0, x1=w, y1=h,
             start_color='#102030', end_color='#F0C080')
    source = shot(app, small, folder, 'classical-source')
    reply = app.call('super_zoom', document_id=small, layer=0, zoom=2, enhance_details=False)
    got = shot(app, reply['document_id'], folder, 'classical-zoom')
    want = classical(source[1], w, h, 2)
    worst = max(abs(a - b) for a, b in zip(got[1], want)) if len(got[1]) == len(want) else None
    receipts['SZC'] = dict(worst=worst, length=len(got[1]), expected=len(want))
    gates['SZC'] = worst is not None and worst <= 1

    big = app.call('new_document', width=4000, height=3000)['document']['id']
    message = app.call('super_zoom', fail=True, document_id=big, layer=0, zoom=4)
    receipts['SZL'] = message
    gates['SZL'] = LIMIT in message
    app.call('close_document', document_id=big, discard_changes=True)


def jpeg_cases(app, folder, receipts, gates):
    for depth in (8, 16):
        scale = 1 if depth == 8 else 257
        doc = opened(app, folder, depth, jpeg=True)
        before = shot(app, doc, folder, f'jpeg-source-{depth}')
        moved = {}
        for strength in ('low', 'high'):
            for output in ('layer', 'new_layer'):
                name = f'J{depth}-{strength}-{output}'
                start = cursor(app, doc)
                reply = app.call('jpeg_artifacts_removal', document_id=doc, layer=0, strength=strength,
                                 output=output)
                steps = cursor(app, doc) - start
                layers = names(app, doc)
                if output == 'layer':
                    after = shot(app, doc, folder, name)
                    changed = after[1] != before[1]
                    alpha = all(after[1][i + 3] == before[1][i + 3] for i in range(0, len(before[1]), 4))
                    moved[strength] = mean_delta(after[1], before[1], scale)
                    shape = layers == ['Background'] and changed and alpha
                else:
                    base = layer_shot(app, doc, folder, f'{name}-base', 0)
                    shape = (layers == ['Background', 'Background JPEG Artifacts Removal']
                             and base[1] == before[1])
                app.call('undo', document_id=doc)
                undone = shot(app, doc, folder, f'{name}-undone')
                receipts[name] = dict(reply=reply, layers=layers, steps=steps)
                gates[name] = (
                    reply.get('ok') is True and reply.get('changed') is True
                    and reply.get('quality_factor') == QUALITY[strength] and reply.get('tiles') == 1
                    and reply.get('model') == dict(id=JPEG_MODEL[0], version=JPEG_MODEL[1])
                    and shape and steps == 1 and undone[1] == before[1] and cursor(app, doc) == start)
        receipts[f'JS{depth}'] = moved
        gates[f'JS{depth}'] = moved.get('high', 0) > moved.get('low', 0)


def refusal_cases(app, folder, receipts, gates):
    float_doc = opened(app, folder, 32)
    cmyk = opened(app, folder, 8)
    app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(GENERIC))
    for name, doc, words in [('R32', float_doc, 'is unavailable in a 32-bit float document'),
                             ('RC', cmyk, 'is unavailable in a CMYK document')]:
        zoom = app.call('super_zoom', fail=True, document_id=doc, layer=0, zoom=2)
        jpeg = app.call('jpeg_artifacts_removal', fail=True, document_id=doc, layer=0)
        receipts[name] = dict(super_zoom=zoom, jpeg=jpeg)
        gates[name] = f'super_zoom {words}' in zoom and f'jpeg_artifacts_removal {words}' in jpeg

    doc = opened(app, folder, 8, jpeg=True)
    before = shot(app, doc, folder, 'action-source')
    count = len(app.call('list_documents').get('documents', []))
    app.call('save_action', name='super-zoom-replay', overwrite=True,
             steps=[{'tool': 'super_zoom', 'arguments': {'layer': 0, 'zoom': 2}}])
    app.call('run_action', document_id=doc, name='super-zoom-replay')
    documents = app.call('list_documents').get('documents', [])
    made = [d for d in documents if d.get('width') == WIDTH * 2 and d.get('height') == HEIGHT * 2]
    receipts['AZ'] = dict(before=count, after=len(documents), made=made)
    gates['AZ'] = len(documents) == count + 1 and len(made) >= 1
    start = cursor(app, doc)
    app.call('save_action', name='jpeg-replay', overwrite=True,
             steps=[{'tool': 'jpeg_artifacts_removal', 'arguments': {'layer': 0, 'strength': 'high'}}])
    app.call('run_action', document_id=doc, name='jpeg-replay')
    replayed = shot(app, doc, folder, 'action-jpeg')
    receipts['AJ'] = dict(steps=cursor(app, doc) - start)
    gates['AJ'] = replayed[1] != before[1] and cursor(app, doc) - start >= 1

    # RZ: with one document open, a recorded super_zoom that names no document_id is the step
    # itself, not the duplicated-document placeholder, though the call's target resolves after
    # it ran and may be the new document (ActionRecorder.opensDuplicateTools).
    for listed in app.call('list_documents').get('documents', []):
        app.call('close_document', document_id=listed['id'], discard_changes=True)
    only = opened(app, folder, 8)
    app.call('start_recording', discard=True)
    reply = app.call('super_zoom', layer=0, zoom=2)
    stopped = app.call('stop_recording', name='super-zoom-recorded')
    steps = stopped.get('steps') or []
    receipts['RZ'] = dict(reply=reply, steps=steps)
    gates['RZ'] = (
        reply.get('ok') is True and reply.get('document_id') != only
        and [step.get('tool') for step in steps] == ['super_zoom']
        and steps[0].get('arguments', {}).get('zoom') == 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--allow-skip', action='store_true',
                        help='exit 0 instead of 2 when the fixture store has no ready model')
    args = parser.parse_args()
    for model, _ in (ZOOM_MODEL, JPEG_MODEL):
        if not ready(STORE, model):
            print(f'SKIP neural filters: no ready {model} in the fixture store {STORE}', file=sys.stderr)
            return 0 if args.allow_skip else 2
    assert PHOTO.is_file(), PHOTO
    output = Path(args.receipts or tempfile.mkdtemp(prefix='neural-filters-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_neural_filters.py before any measurement'}
    gates = {}
    saved = {key: os.environ.get(key) for key in ('DR_MODELS_DIR', 'DR_BACKGROUND')}
    try:
        os.environ['DR_BACKGROUND'] = '1'
        os.environ['DR_MODELS_DIR'] = str(STORE)
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            zoom_cases(app, folder, receipts, gates)
            jpeg_cases(app, folder, receipts, gates)
            refusal_cases(app, folder, receipts, gates)
        empty = folder / 'empty-store'
        empty.mkdir()
        os.environ['DR_MODELS_DIR'] = str(empty)
        with App(folder / 'bare', folder / 'recovery-bare', interval=600) as app:
            doc = opened(app, folder, 8)
            zoom = app.call('super_zoom', fail=True, document_id=doc, layer=0, zoom=2)
            jpeg = app.call('jpeg_artifacts_removal', fail=True, document_id=doc, layer=0)
            receipts['RN'] = dict(super_zoom=zoom, jpeg=jpeg)
            gates['RN'] = NOT_INSTALLED_ZOOM in zoom and NOT_INSTALLED_JPEG in jpeg
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
