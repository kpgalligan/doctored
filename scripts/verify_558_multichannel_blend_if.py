#!/usr/bin/env python3
"""The MCP drive on an owned Debug app: Blend If on several
channels at once (`blend_if.extra`) through the surfaces the Layer Style
sheet shares.

Gates, fixed before the run (a failed gate IS the receipt):

- **M1 accepted and reported, RGB 8 / RGB 16 / CMYK 8.** On a new document
  with a painted layer (a red-heavy and a blue rect), `set_layer_style` with
  a Blend If of Gray plus a second channel in `extra` (Red on RGB, Cyan on
  CMYK) is accepted; `get_document` reports it canonically on the layer's
  style (Gray the primary, the second channel in `extra`); it is ONE undo
  step; the render differs from the same document with the Gray range alone;
  Undo restores the unstyled render exactly.
- **M2 `.rz` save and reopen.** `save_copy` as `.rz` and `open_document` of
  it report the same `blend_if`, at each of the three.
- **M3 PSD.** `export_psd` (layers) then `inspect_psd` + `open_document`
  (`psd_mode` layers) report the same `blend_if` on the painted layer, with
  no Blend If loss in the inspect report, at each of the three.
- **M4 CMYK refuses Red by name, no history step.** On the CMYK document a
  Blend If with an IDENTITY Red range in `extra` is refused in-band with a
  sentence naming the Red channel, and the undo count is unchanged.
- **M5 Actions.** `start_recording` → `set_layer_style` → `stop_recording`
  → `run_action` on a SECOND, identical 8-bit document reports the same
  `blend_if` and the same render.
- **M6 copy 8 -> 16.** `copy_layers_to_document` of the styled layer into a
  16-bit document keeps both ranges.

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
FULL = [0, 0, 255, 255]
GRAY = {'channel': 'gray', 'this_layer': [0, 0, 96, 192], 'underlying': FULL}


def second(cmyk):
    return {'channel': 'cyan' if cmyk else 'red', 'this_layer': [0, 0, 64, 224],
            'underlying': FULL}


def multi_style(cmyk):
    return {'blend_if': dict(GRAY, extra=[second(cmyk)])}


def single_style():
    return {'blend_if': dict(GRAY)}


def rows(app, doc):
    return app.call('get_document', document_id=doc)['layers']


def index_of(app, doc, name):
    return next(row['index'] for row in rows(app, doc) if row['name'] == name)


def blend_if_of(app, doc, name):
    row = next((r for r in rows(app, doc) if r['name'] == name), None)
    return ((row or {}).get('style') or {}).get('blend_if')


def undo_count(app, doc):
    return app.call('list_history', document_id=doc)['undo_count']


def render(app, doc, path):
    app.call('save_copy', document_id=doc, path=str(path))
    return hashlib.sha256(path.read_bytes()).hexdigest()


def scene(app, depth, cmyk):
    """A new document with a painted layer: a red-heavy and a blue rect."""
    extra = dict(color_mode='cmyk') if cmyk else {}
    doc = app.call('new_document', width=W, height=H, unit='px', depth=depth,
                   background='white', **extra)['document']['id']
    app.call('new_layer', document_id=doc, name='Paint')
    paint = index_of(app, doc, 'Paint')
    for rect, color in (((2, 2, 20, 28), '#e03020'), ((26, 2, 20, 28), '#2040e0')):
        app.call('select_rect', document_id=doc, x=rect[0], y=rect[1], width=rect[2],
                 height=rect[3])
        app.call('fill_selection', document_id=doc, layer=paint, color=color)
    app.call('deselect', document_id=doc)
    return doc, paint


def canonical(blend_if, cmyk):
    return blend_if == dict(GRAY, extra=[second(cmyk)])


def gate(gates, name, ok, detail):
    gates[name] = dict(passed=bool(ok), detail=detail)
    print(('PASS ' if ok else 'FAIL ') + name, flush=True)


def drive(app, folder, gates):
    for label, depth, cmyk in (('RGB8', 8, False), ('RGB16', 16, False), ('CMYK8', 8, True)):
        ext = 'tif' if cmyk else 'png'
        doc, paint = scene(app, depth, cmyk)
        plain = render(app, doc, folder / f'plain-{label}.{ext}')
        app.call('set_layer_style', document_id=doc, layer=paint, style=single_style())
        single = render(app, doc, folder / f'single-{label}.{ext}')
        app.call('undo', document_id=doc)
        before = undo_count(app, doc)
        app.call('set_layer_style', document_id=doc, layer=paint, style=multi_style(cmyk))
        steps = undo_count(app, doc) - before
        reported = blend_if_of(app, doc, 'Paint')
        multi = render(app, doc, folder / f'multi-{label}.{ext}')
        app.call('undo', document_id=doc)
        restored = render(app, doc, folder / f'undone-{label}.{ext}')
        gate(gates, f'M1 {label} accepted, reported, one step, differs, undone',
             canonical(reported, cmyk) and steps == 1 and multi != single and multi != plain
             and restored == plain,
             dict(reported=reported, steps=steps, differs=multi != single,
                  undone=restored == plain))
        app.call('redo', document_id=doc)

        native = folder / f'saved-{label}.rz'
        app.call('save_copy', document_id=doc, path=str(native))
        reopened = app.call('open_document', path=str(native))['document']['id']
        again = blend_if_of(app, reopened, 'Paint')
        gate(gates, f'M2 {label} .rz save and reopen keep both ranges',
             canonical(again, cmyk), dict(reopened=again))
        app.call('close_document', document_id=reopened, discard_changes=True)

        psd = folder / f'exported-{label}.psd'
        app.call('export_psd', document_id=doc, path=str(psd))
        report = app.call('inspect_psd', path=str(psd))
        losses = [loss for loss in report.get('verified_losses', []) if 'Blend If' in loss]
        opened = app.call('open_document', path=str(psd), psd_mode='layers',
                          psd_fingerprint=report['source_fingerprint'])['document']['id']
        back = blend_if_of(app, opened, 'Paint')
        gate(gates, f'M3 {label} export_psd and reopen keep both ranges',
             canonical(back, cmyk) and not losses, dict(reopened=back, losses=losses))
        app.call('close_document', document_id=opened, discard_changes=True)

        if cmyk:
            before = undo_count(app, doc)
            red = {'blend_if': dict(GRAY, extra=[{'channel': 'red', 'this_layer': FULL,
                                                  'underlying': FULL}])}
            refusal = app.call('set_layer_style', fail=True, document_id=doc, layer=paint,
                               style=red)
            gate(gates, 'M4 CMYK refuses an identity Red range by name, no history step',
                 'Red channel' in refusal and undo_count(app, doc) == before,
                 dict(refusal=refusal))
        app.call('close_document', document_id=doc, discard_changes=True)

    first, paint = scene(app, 8, False)
    app.call('start_recording', discard=True)
    app.call('set_layer_style', document_id=first, layer=paint, style=multi_style(False))
    stopped = app.call('stop_recording', name='s558-multichannel-blend-if')
    recorded = render(app, first, folder / 'recorded.png')
    other, _ = scene(app, 8, False)
    replay = app.call('run_action', document_id=other, name='s558-multichannel-blend-if')
    replayed = render(app, other, folder / 'replayed.png')
    gate(gates, 'M5 the recorded Action replays both ranges on a second document',
         canonical(blend_if_of(app, other, 'Paint'), False) and replayed == recorded,
         dict(steps=stopped.get('steps'), replay=replay, same=replayed == recorded))

    target = app.call('new_document', width=W, height=H, unit='px', depth=16,
                      background='white')['document']['id']
    app.call('copy_layers_to_document', document_id=first, to_document_id=target,
             layers=[paint], placement='in_place')
    copied = blend_if_of(app, target, 'Paint')
    gate(gates, 'M6 copy 8 -> 16 keeps both ranges', canonical(copied, False),
         dict(copied=copied))
    for doc in (first, other, target):
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
    print('verify_558_multichannel_blend_if: ' + ('PASS' if not failed else f'FAIL {failed}'))
    raise SystemExit(0 if not failed else 1)


if __name__ == '__main__':
    main()
