#!/usr/bin/env python3
"""The MCP drive on an owned Debug app: a layer style and Blend If on an
ADJUSTMENT layer, through the surfaces the Layer Style sheet shares.

Gates, fixed before the run (a failed gate IS the receipt):

- **S1 accepted and reported, 8 / 16 / 32 bits.** On a new document with an
  opaque layer under an adjustment layer (Invert at 8 and 16 bits, Exposure
  +1 at 32), `set_layer_style` with a Stroke, Fill 75 % and a Gray Blend If
  is accepted; `get_document` reports the style on the adjustment row (its
  stroke, its fill and its `blend_if`); it is ONE undo step; the rendered
  copy differs from the unstyled one; Undo restores the unstyled render
  exactly.
- **S2 accepted on a CMYK adjustment, 8 / 16 bits.** On a
  CMYK document with a painted layer under a Levels adjustment layer,
  `set_layer_style` with the same style is accepted, reported on the
  adjustment row, ONE undo step, the render differs from the unstyled one and
  Undo restores it exactly.
- **S3 Actions.** `start_recording` → `set_layer_style` → `stop_recording`
  (the Action saved by name) → `run_action` on a SECOND, identical 8-bit
  document reports the same style on its adjustment row and the same render.
  The CMYK replay: the step recorded on an 8-bit CMYK adjustment replays the
  style onto an RGB twin's adjustment row (`run_action` is unavailable in a
  CMYK document, the CMYK limitation: it refuses with that sentence and adds
  no history step).
- **S4 copy 8 → 16.** `copy_layers_to_document` of the styled adjustment
  into a 16-bit document keeps the style (its stroke and `blend_if`).

Launch: `phase5_support.App` (scratch socket, dummy key, background, every
preset folder private); only that instance is ever stopped.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402

W, H = 48, 32
STYLE = {'fill_opacity': 0.75,
         'blend_if': {'channel': 'gray', 'this_layer': [0, 0, 200, 255],
                      'underlying': [0, 0, 255, 255]},
         'effects': [{'type': 'stroke', 'size': 3, 'color': '#ff0000'}]}


def adjustment_of(depth):
    if depth == 32:
        return dict(op='exposure', params={'exposure': 1.0})
    return dict(op='invert', params={})


def rows(app, doc):
    return app.call('get_document', document_id=doc)['layers']


def index_of(app, doc, name):
    return next(row['index'] for row in rows(app, doc) if row['name'] == name)


def undo_count(app, doc):
    return app.call('list_history', document_id=doc)['undo_count']


def render(app, doc, path):
    app.call('save_copy', document_id=doc, path=str(path))
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ink_render(app, doc, path):
    """A CMYK document's composite fingerprint: `sample_color`'s ink16 every
    4 px (a CMYK document saves no PNG), written to `path` and hashed."""
    samples = []
    for y in range(0, H, 4):
        for x in range(0, W, 4):
            sample = app.call('sample_color', document_id=doc, x=x, y=y)
            assert sample.get('color_mode') == 'cmyk', sample
            samples.append(sample['ink16'])
    path.write_text(json.dumps(samples) + '\n')
    return hashlib.sha256(path.read_bytes()).hexdigest()


def styled_scene(app, depth, name='Adj'):
    """A new document: a painted opaque layer, then the adjustment."""
    doc = app.call('new_document', width=W, height=H, unit='px', depth=depth,
                   background='white')['document']['id']
    app.call('new_layer', document_id=doc, name='Paint')
    paint = index_of(app, doc, 'Paint')
    app.call('select_rect', document_id=doc, x=8, y=6, width=30, height=18)
    app.call('fill_selection', document_id=doc, layer=paint, color='#3070c0')
    app.call('deselect', document_id=doc)
    app.call('add_adjustment_layer', document_id=doc, name=name, **adjustment_of(depth))
    return doc, index_of(app, doc, name)


def cmyk_scene(app, depth, name='Adj'):
    """A new CMYK document: a painted opaque layer, then a Levels adjustment."""
    doc = app.call('new_document', width=W, height=H, unit='px', color_mode='cmyk',
                   depth=depth, background='white')['document']['id']
    app.call('new_layer', document_id=doc, name='Paint')
    paint = index_of(app, doc, 'Paint')
    app.call('select_rect', document_id=doc, x=8, y=6, width=30, height=18)
    app.call('fill_selection', document_id=doc, layer=paint, color='#3070c0')
    app.call('deselect', document_id=doc)
    app.call('add_adjustment_layer', document_id=doc, name=name, op='levels',
             params={'gamma': 1.2})
    return doc, index_of(app, doc, name)


def reported(style):
    return (style is not None and [e['type'] for e in style.get('effects') or []] == ['stroke']
            and abs(style.get('fill_opacity', 1) - 0.75) < 1e-3
            and (style.get('blend_if') or {}).get('this_layer') == [0, 0, 200, 255])


def gate(gates, name, ok, detail):
    gates[name] = dict(passed=bool(ok), detail=detail)
    print(('PASS ' if ok else 'FAIL ') + name, flush=True)


def drive(app, folder, gates):
    for depth in (8, 16, 32):
        doc, index = styled_scene(app, depth)
        plain = render(app, doc, folder / f'plain{depth}.png')
        before = undo_count(app, doc)
        app.call('set_layer_style', document_id=doc, layer=index, style=STYLE)
        style = rows(app, doc)[index].get('style')
        styled = render(app, doc, folder / f'styled{depth}.png')
        steps = undo_count(app, doc) - before
        app.call('undo', document_id=doc)
        restored = render(app, doc, folder / f'undone{depth}.png')
        gate(gates, f'S1 RGB{depth} accepted, reported, one step, undone',
             reported(style) and steps == 1 and styled != plain and restored == plain,
             dict(style=style, steps=steps, changed=styled != plain, undone=restored == plain))
        app.call('close_document', document_id=doc, discard_changes=True)

    for depth in (8, 16):
        doc, index = cmyk_scene(app, depth)
        plain = ink_render(app, doc, folder / f'cmyk_plain{depth}.json')
        before = undo_count(app, doc)
        app.call('set_layer_style', document_id=doc, layer=index, style=STYLE)
        style = rows(app, doc)[index].get('style')
        styled = ink_render(app, doc, folder / f'cmyk_styled{depth}.json')
        steps = undo_count(app, doc) - before
        app.call('undo', document_id=doc)
        restored = ink_render(app, doc, folder / f'cmyk_undone{depth}.json')
        gate(gates, f'S2 CMYK{depth} adjustment accepted, reported, one step, undone',
             reported(style) and steps == 1 and styled != plain and restored == plain,
             dict(style=style, steps=steps, changed=styled != plain, undone=restored == plain))
        app.call('close_document', document_id=doc, discard_changes=True)

    first, index = styled_scene(app, 8)
    app.call('start_recording', discard=True)
    app.call('set_layer_style', document_id=first, layer=index, style=STYLE)
    stopped = app.call('stop_recording', name='s558-adjustment-style')
    recorded = render(app, first, folder / 'recorded.png')
    second, index2 = styled_scene(app, 8)
    replay = app.call('run_action', document_id=second, name='s558-adjustment-style')
    replayed = render(app, second, folder / 'replayed.png')
    gate(gates, 'S3 the recorded Action replays the style on a second document',
         reported(rows(app, second)[index2].get('style')) and replayed == recorded,
         dict(steps=stopped.get('steps'), replay=replay, same=replayed == recorded))

    # The CMYK replay: run_action is unavailable in a CMYK document (the CMYK
    # limitation), so the step recorded on a CMYK adjustment replays on an
    # RGB twin, and the CMYK refusal is recorded with no history step.
    source, cmyk_index = cmyk_scene(app, 8)
    app.call('start_recording', discard=True)
    app.call('set_layer_style', document_id=source, layer=cmyk_index, style=STYLE)
    cmyk_stopped = app.call('stop_recording', name='s558-adjustment-style-cmyk')
    twin, twin_index = styled_scene(app, 8, name='Adj')
    twin_replay = app.call('run_action', document_id=twin, name='s558-adjustment-style-cmyk')
    cmyk_second, _ = cmyk_scene(app, 8)
    before = undo_count(app, cmyk_second)
    refusal = app.call('run_action', fail=True, document_id=cmyk_second,
                       name='s558-adjustment-style-cmyk')
    gate(gates, 'S3 CMYK8: the Action recorded on a CMYK adjustment replays its style',
         reported(rows(app, source)[cmyk_index].get('style'))
         and reported(rows(app, twin)[twin_index].get('style'))
         and 'run_action is unavailable in a CMYK document' in refusal
         and undo_count(app, cmyk_second) == before,
         dict(steps=cmyk_stopped.get('steps'), replay=twin_replay, refusal=refusal[:80]))
    for doc in (source, twin, cmyk_second):
        app.call('close_document', document_id=doc, discard_changes=True)
    index = index_of(app, first, 'Adj')

    target = app.call('new_document', width=W, height=H, unit='px', depth=16,
                      background='white')['document']['id']
    app.call('copy_layers_to_document', document_id=first, to_document_id=target,
             layers=[index], placement='in_place')
    copied = next((row for row in rows(app, target) if row['name'] == 'Adj'), None)
    gate(gates, 'S4 copy 8 -> 16 keeps the style',
         copied is not None and reported(copied.get('style')), dict(row=copied))
    for doc in (first, second, target):
        app.call('close_document', document_id=doc, discard_changes=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    gates = {}
    with App(output / 'app', output / 'recovery', interval=600) as app:
        drive(app, output, gates)
    (output / 'receipt.json').write_text(json.dumps(gates, indent=2, default=str) + '\n')
    failed = [name for name, g in gates.items() if not g['passed']]
    print('verify_558_adjustment_styles: ' + ('PASS' if not failed else f'FAIL {failed}'))
    raise SystemExit(0 if not failed else 1)


if __name__ == '__main__':
    main()
