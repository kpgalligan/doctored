#!/usr/bin/env python3
"""The user's gradient library over MCP (`list_gradients`, `save_gradient`, `delete_gradient`,
`import_gradients`, `export_gradients`, and `{"name": …}` in place of `gradient`'s `stops` and a
gradient fill layer's `gradient.stops`) on owned Debug instances.

The gates are FIXED here before anything is measured. Each instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library, and one gradient folder shared by the two launches) and ends with it; no
Doctored process this file did not start is signalled.

- **G1 library.** save_gradient stores SUNSET (three stops, a midpoint, smoothness 0.4);
  list_gradients answers it exactly; a relaunch lists the same; export, delete (list empty) and
  import restore it; a mixed file imports its good entry (a taken name becomes "Sunset 2") and
  skips the one-stop entry by name.
- **G2 refusals.** A taken name without replace, one stop, 33 stops, a decreasing position, a
  colour that is not hex, a file that is not a gradient file and an unknown library name refuse by
  name; the library is unchanged.
- **G3 resolution.** At 8 and 16 bits `gradient` with `stops: {"name": "Sunset"}` paints exactly
  the pixels the same stops and smoothness given inline paint, and `add_fill_layer` with
  `gradient.stops: {"name": …}` stores exactly the inline layer's stops and smoothness.
- **G4 replay.** A recorded `gradient` naming the preset replays the same pixels after the preset
  is deleted: the step holds the stops.

Usage: python3 -B scripts/verify_gradient_library.py [--receipts <folder>]
"""
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 24, 4
SUNSET = [
    {'position': 0.0, 'color': '#1a2b8c', 'opacity': 1.0},
    {'position': 0.35, 'color': '#e0457b', 'opacity': 0.8, 'midpoint': 0.3},
    {'position': 1.0, 'color': '#ffd166', 'opacity': 1.0},
]
SMOOTHNESS = 0.4
RAMP = dict(x0=0, y0=0, x1=W, y1=0)


def read(call, doc, x, y):
    reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=0)
    for key in ('center_rgba32', 'mean_rgba32', 'center_rgba_codes'):
        if reply.get(key) is not None:
            return reply[key]
    raise AssertionError(('no readout', reply))


def row(call, doc):
    return [read(call, doc, x, 1) for x in range(W)]


def listed(call):
    return call('list_gradients')['gradients']


def refused(call, tool, needle, **arguments):
    before = listed(call)
    text = call(tool, fail=True, **arguments)
    assert needle in text, (tool, needle, text)
    assert listed(call) == before, (tool, 'the library moved')
    return text


def library(call, folder):
    out = {}
    reply = call('save_gradient', name='Sunset', stops=SUNSET, smoothness=SMOOTHNESS)
    assert reply['gradient']['name'] == 'Sunset', reply
    rows = listed(call)
    assert len(rows) == 1 and rows[0]['stops'] == SUNSET and rows[0]['smoothness'] == SMOOTHNESS, rows
    out['saved'] = rows[0]['stops']
    exported = folder / 'mine.doctoredgradients'
    assert call('export_gradients', path=str(exported))['exported'] == ['Sunset']
    call('delete_gradient', name='Sunset')
    assert listed(call) == [], 'delete empties the library'
    reply = call('import_gradients', path=str(exported))
    assert reply['imported'] == ['Sunset'] and reply['skipped'] == [], reply
    assert [r['stops'] for r in listed(call)] == [SUNSET], 'import restores the stops'
    mixed = folder / 'mixed.doctoredgradients'
    entry = {'schema_version': 1, 'name': 'Sunset', 'stops': SUNSET, 'smoothness': 1}
    bad = {'schema_version': 1, 'name': 'Lonely', 'stops': SUNSET[:1], 'smoothness': 1}
    mixed.write_text(json.dumps({'doctored_gradients': 1, 'gradients': [entry, bad]}))
    reply = call('import_gradients', path=str(mixed))
    assert reply['imported'] == ['Sunset 2'], reply
    assert [s['name'] for s in reply['skipped']] == ['Lonely'], reply
    out['mixed'] = reply
    call('delete_gradient', name='Sunset 2')
    return out


def refusals(call, folder):
    out = {}
    out['taken'] = refused(call, 'save_gradient', 'already exists', name='Sunset', stops=SUNSET)
    out['one stop'] = refused(call, 'save_gradient', '2 to 32', name='One', stops=SUNSET[:1])
    many = [{'position': i / 32, 'color': '#000000'} for i in range(33)]
    out['33 stops'] = refused(call, 'save_gradient', '2 to 32', name='Many', stops=many)
    backwards = [{'position': 0.6, 'color': '#000000'}, {'position': 0.2, 'color': '#ffffff'}]
    out['decreasing'] = refused(call, 'save_gradient', 'not below the stop before it',
                                name='Back', stops=backwards)
    colourless = [{'position': 0, 'color': 'red'}, {'position': 1, 'color': '#ffffff'}]
    out['colour'] = refused(call, 'save_gradient', 'hex', name='Red', stops=colourless)
    junk = folder / 'junk.doctoredgradients'
    junk.write_text('{"something": 1}')
    out['file'] = refused(call, 'import_gradients', 'is not a Doctored gradient file',
                          path=str(junk))
    doc = call('new_document', width=W, height=H, background='white', depth=8,
               profile='srgb')['document']['id']
    out['unknown'] = call('gradient', fail=True, document_id=doc, stops={'name': 'Nowhere'},
                          **RAMP)
    assert 'No gradient named' in out['unknown'], out['unknown']
    call('close_document', document_id=doc, discard_changes=True)
    return out


def resolution(call):
    out = {}
    for depth in (8, 16):
        named = call('new_document', width=W, height=H, background='white', depth=depth,
                     profile='srgb')['document']['id']
        inline = call('new_document', width=W, height=H, background='white', depth=depth,
                      profile='srgb')['document']['id']
        call('gradient', document_id=named, stops={'name': 'Sunset'}, **RAMP)
        call('gradient', document_id=inline, stops=SUNSET, smoothness=SMOOTHNESS, **RAMP)
        a, b = row(call, named), row(call, inline)
        assert a == b, (depth, 'the named gradient paints the inline stops', a, b)
        assert len({json.dumps(p) for p in a}) > 5, (depth, 'the ramp is not flat', a)
        out[f'gradient {depth}'] = 'equal'
        call('add_fill_layer', document_id=named, kind='gradient',
             gradient={'stops': {'name': 'Sunset'}})
        call('add_fill_layer', document_id=inline, kind='gradient',
             gradient={'stops': SUNSET, 'smoothness': SMOOTHNESS})
        fills = [call('get_document', document_id=d)['layers'][-1].get('fill')
                 for d in (named, inline)]
        assert fills[0] == fills[1] and fills[0], (depth, 'the named fill layer', fills)
        out[f'fill layer {depth}'] = fills[0]
        for d in (named, inline):
            call('close_document', document_id=d, discard_changes=True)
    return out


def replay(call):
    doc = call('new_document', width=W, height=H, background='white', depth=8,
               profile='srgb')['document']['id']
    blank = row(call, doc)
    call('start_recording', discard=True)
    call('gradient', document_id=doc, stops={'name': 'Sunset'}, **RAMP)
    recorded = row(call, doc)
    call('stop_recording', name='gradient-replay')
    call('undo', document_id=doc)
    assert row(call, doc) == blank, 'undo restores the canvas'
    call('delete_gradient', name='Sunset')
    call('run_action', document_id=doc, name='gradient-replay')
    assert row(call, doc) == recorded, 'the step replays differently once the gradient is deleted'
    call('close_document', document_id=doc, discard_changes=True)
    return 'ok'


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='gradient-library-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    os.environ['DR_GRADIENTS_DIR'] = str(folder / 'shared-gradients')
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_gradient_library.py'}
    with App(folder / 'app1', folder / 'recovery1', interval=600) as app:
        receipts['G1 library'] = library(app.call, folder)
        receipts['G2 refusals'] = refusals(app.call, folder)
        first = listed(app.call)
    with App(folder / 'app2', folder / 'recovery2', interval=600) as app:
        assert listed(app.call) == first, ('a relaunch lists the same', first)
        receipts['G1 relaunch'] = 'same'
        receipts['G3 resolution'] = resolution(app.call)
        receipts['G4 replay'] = replay(app.call)
    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    print(json.dumps(receipts, indent=2))
    print('PASS: the gradient library over MCP: save, list, relaunch, export, delete, import, '
          'refusals, {"name": …} at 8 and 16 bits and in fill layers, and replay')


if __name__ == '__main__':
    main()
