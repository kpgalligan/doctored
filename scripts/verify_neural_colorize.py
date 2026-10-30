#!/usr/bin/env python3
"""The M4 phase 2 MCP drive: Filter ▸ Neural Filters ▸ Colorize (`neural_colorize`) on the
Debug app with the real DDColor model from the shared fixture store.

The store is ~/devel/doctored-fixtures/models, filled once by
`scripts/fill_model_fixtures.py --release-dir ~/devel/doctored-fixtures/models-release --only
doctored/ddcolor` and only read here; without the model ready there the drive prints
`SKIP neural colorize: …` on stderr and exits 2 (0 only with `--allow-skip`). The photo is the
phase-6 fixture sky.jpg (CC0) resized to 320 × 240 by `sips`, through
`scripts/verify_neural_filters.py`'s helpers (loaded by importlib): each instance is owned by this
file (`phase5_support.App` on its own scratch MCP socket, a dummy API key, a private folder for
every preference library, DR_BACKGROUND=1); DR_MODELS_DIR is the fixture store, or for case RN an
empty scratch store. Pictures are read back through `save_copy` and the independent PNG reader.
Lab is computed here from the sRGB codes with the Bradford-adapted (D50) sRGB matrix.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt.

Colorize at 8 and 16 bits, output layer, new_layer and new_document (C8-layer, …,
C16-new_document), every slider at 0:
- ok, changed, planned_bytes > 0, the model doctored/ddcolor @ ddcolor-modelscope-coreml-1,
  output echoed;
- layer: output_layer 0, one layer, some pixel changed, every alpha kept, ONE history step, and
  one undo restores the picture bit-exact and the cursor;
- new_layer: output_layer 1, layers ['Background', 'Background Colorize'], layer 0 unchanged,
  ONE history step, one undo restores the picture and the cursor;
- new_document: a NEW document_id of 320 × 240, the source's precision, one layer
  'Background', a title holding 'Colorize'; the source's pixels and cursor unchanged.
- **LUM** the 8-bit layer output's L* against the source's: |ΔL*| ≤ LUM_BOUND for every pixel.
- **SAT** saturation −50 at 8 bits: every output pixel |a*|, |b*| ≤ SAT_BOUND.

Refusals, replay and recording:
- **R32** a 32-bit document: "neural_colorize is unavailable in a 32-bit float document".
- **RC** a CMYK document: "neural_colorize is unavailable in a CMYK document".
- **RN** a second instance on an empty scratch DR_MODELS_DIR: "the Colorize model is not
  installed; download DDColor in Settings ▸ Models".
- **AC** `run_action` of a saved `neural_colorize` step (saturation 20) changes the pixels in at
  least one step.
- **RCD** with one document open, a recording of `neural_colorize` output new_document that names
  no document_id holds exactly that one step (output new_document), not the duplicated-document
  placeholder.

Usage: python3 scripts/verify_neural_colorize.py [--receipts DIR] [--allow-skip]. Prints
PASS/FAIL per gate and exits 0 only when every gate passes (2 on a SKIP without --allow-skip).
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402

# ---- the expectations, fixed here BEFORE anything is measured -------------
MODEL = ('doctored/ddcolor', 'ddcolor-modelscope-coreml-1')
LUM_BOUND = 1.0
SAT_BOUND = 1.5
NOT_INSTALLED = 'the Colorize model is not installed; download DDColor in Settings ▸ Models'
# The ICC sRGB profile's D50-adapted matrix rows and the PCS white.
SRGB_D50 = ((0.4360747, 0.3850649, 0.1430804),
            (0.2225045, 0.7168786, 0.0606169),
            (0.0139322, 0.0971045, 0.7141733))
D50 = (0.96422, 1.0, 0.82521)


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


nf = _load('verify_neural_filters', 'scripts/verify_neural_filters.py')


def lab(r, g, b, scale):
    """Straight sRGB codes (of 255·scale) as D50 Lab."""
    def linear(v):
        v = v / (255.0 * scale)
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    rgb = (linear(r), linear(g), linear(b))
    xyz = [sum(row[c] * rgb[c] for c in range(3)) / D50[i] for i, row in enumerate(SRGB_D50)]

    def f(t):
        return t ** (1 / 3) if t > 216 / 24389 else (24389 / 27 * t + 16) / 116
    fx, fy, fz = (f(t) for t in xyz)
    return 116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)


def pixels(samples):
    return [samples[i:i + 4] for i in range(0, len(samples), 4)]


def colorize_cases(app, folder, receipts, gates):
    model = dict(id=MODEL[0], version=MODEL[1])
    for depth in (8, 16):
        scale = 1 if depth == 8 else 257
        for output in ('layer', 'new_layer', 'new_document'):
            name = f'C{depth}-{output}'
            doc = nf.opened(app, folder, depth)
            before = nf.shot(app, doc, folder, f'{name}-source')
            start = nf.cursor(app, doc)
            reply = app.call('neural_colorize', document_id=doc, layer=0, output=output)
            common = (reply.get('ok') is True and reply.get('changed') is True
                      and reply.get('planned_bytes', 0) > 0 and reply.get('model') == model
                      and reply.get('output') == output)
            if output == 'new_document':
                made = reply.get('document_id')
                details = nf.info(app, made) if made is not None else {}
                title = nf.document_title(app, made) if made is not None else ''
                after = nf.shot(app, doc, folder, f'{name}-source-after')
                shape = (made not in (None, doc) and reply.get('width') == nf.WIDTH
                         and reply.get('height') == nf.HEIGHT and nf.names(app, made) == ['Background']
                         and details.get('precision') == nf.PRECISION[depth] and 'Colorize' in title
                         and after[1] == before[1] and nf.cursor(app, doc) == start)
                receipts[name] = dict(reply=reply, title=title, precision=details.get('precision'))
                gates[name] = common and shape
                if made is not None:
                    app.call('close_document', document_id=made, discard_changes=True)
                continue
            steps = nf.cursor(app, doc) - start
            layers = nf.names(app, doc)
            if output == 'layer':
                after = nf.shot(app, doc, folder, name)
                alpha = all(a[3] == b[3] for a, b in zip(pixels(after[1]), pixels(before[1])))
                shape = (reply.get('output_layer') == 0 and layers == ['Background']
                         and after[1] != before[1] and alpha)
                if depth == 8:
                    worst = max(abs(lab(*a[:3], scale)[0] - lab(*b[:3], scale)[0])
                                for a, b in zip(pixels(after[1]), pixels(before[1])))
                    receipts['LUM'] = dict(worst=worst)
                    gates['LUM'] = worst <= LUM_BOUND
            else:
                base = nf.layer_shot(app, doc, folder, f'{name}-base', 0)
                shape = (reply.get('output_layer') == 1
                         and layers == ['Background', 'Background Colorize'] and base[1] == before[1])
            app.call('undo', document_id=doc)
            undone = nf.shot(app, doc, folder, f'{name}-undone')
            receipts[name] = dict(reply=reply, layers=layers, steps=steps)
            gates[name] = (common and shape and steps == 1 and undone[1] == before[1]
                           and nf.cursor(app, doc) == start)
            app.call('close_document', document_id=doc, discard_changes=True)

    doc = nf.opened(app, folder, 8)
    reply = app.call('neural_colorize', document_id=doc, layer=0, saturation=-50)
    gray = nf.shot(app, doc, folder, 'SAT')
    worst = max(max(abs(v) for v in lab(*p[:3], 1)[1:]) for p in pixels(gray[1]))
    receipts['SAT'] = dict(reply=reply, worst=worst)
    gates['SAT'] = reply.get('ok') is True and worst <= SAT_BOUND
    app.call('close_document', document_id=doc, discard_changes=True)


def refusal_cases(app, folder, receipts, gates):
    float_doc = nf.opened(app, folder, 32)
    cmyk = nf.opened(app, folder, 8)
    app.call('convert_mode', document_id=cmyk, mode='cmyk', profile='file', path=str(nf.GENERIC))
    for name, doc, words in [('R32', float_doc, 'is unavailable in a 32-bit float document'),
                             ('RC', cmyk, 'is unavailable in a CMYK document')]:
        message = app.call('neural_colorize', fail=True, document_id=doc, layer=0)
        receipts[name] = message
        gates[name] = f'neural_colorize {words}' in message
    for doc in (float_doc, cmyk):
        app.call('close_document', document_id=doc, discard_changes=True)

    doc = nf.opened(app, folder, 8)
    before = nf.shot(app, doc, folder, 'action-source')
    start = nf.cursor(app, doc)
    app.call('save_action', name='colorize-replay', overwrite=True,
             steps=[{'tool': 'neural_colorize', 'arguments': {'layer': 0, 'saturation': 20}}])
    app.call('run_action', document_id=doc, name='colorize-replay')
    replayed = nf.shot(app, doc, folder, 'action-colorize')
    receipts['AC'] = dict(steps=nf.cursor(app, doc) - start)
    gates['AC'] = replayed[1] != before[1] and nf.cursor(app, doc) - start >= 1

    # RCD: with one document open, a recorded new-document colorize that names no document_id is
    # the step itself, not the duplicated-document placeholder (ActionRecorder.opensDuplicate).
    for listed in app.call('list_documents').get('documents', []):
        app.call('close_document', document_id=listed['id'], discard_changes=True)
    only = nf.opened(app, folder, 8)
    app.call('start_recording', discard=True)
    reply = app.call('neural_colorize', layer=0, output='new_document')
    stopped = app.call('stop_recording', name='colorize-recorded')
    steps = stopped.get('steps') or []
    receipts['RCD'] = dict(reply=reply, steps=steps)
    gates['RCD'] = (
        reply.get('ok') is True and reply.get('document_id') != only
        and [step.get('tool') for step in steps] == ['neural_colorize']
        and steps[0].get('arguments', {}).get('output') == 'new_document')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--allow-skip', action='store_true',
                        help='exit 0 instead of 2 when the fixture store has no ready model')
    args = parser.parse_args()
    if not nf.ready(nf.STORE, MODEL[0]):
        print(f'SKIP neural colorize: no ready {MODEL[0]} in the fixture store {nf.STORE}', file=sys.stderr)
        return 0 if args.allow_skip else 2
    assert nf.PHOTO.is_file(), nf.PHOTO
    output = Path(args.receipts or tempfile.mkdtemp(prefix='neural-colorize-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_neural_colorize.py before any measurement'}
    gates = {}
    saved = {key: os.environ.get(key) for key in ('DR_MODELS_DIR', 'DR_BACKGROUND')}
    try:
        os.environ['DR_BACKGROUND'] = '1'
        os.environ['DR_MODELS_DIR'] = str(nf.STORE)
        with App(folder / 'app', folder / 'recovery', interval=600) as app:
            colorize_cases(app, folder, receipts, gates)
            refusal_cases(app, folder, receipts, gates)
        empty = folder / 'empty-store'
        empty.mkdir()
        os.environ['DR_MODELS_DIR'] = str(empty)
        with App(folder / 'bare', folder / 'recovery-bare', interval=600) as app:
            doc = nf.opened(app, folder, 8)
            message = app.call('neural_colorize', fail=True, document_id=doc, layer=0)
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
