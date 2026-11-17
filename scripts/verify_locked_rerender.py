#!/usr/bin/env python3
"""A described-layer re-render that a Transparency lock refuses names the lock (MCP
`edit_text_layer` and `edit_shape_layer`; `AgentServer.rejectLockedRerender`, the agent twin of
`EditorViewController.refuseLockedRerender`) on an owned Debug instance, in an 8-bit and a 16-bit
RGB document.

The sentence is the app's own: "… has its transparency locked …" ending in the `set_layer_lock`
hint. The gates are FIXED here before anything is measured. The instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library) and ends with it; no Doctored process this file did not start is signalled.

Per depth, on a text layer (Menlo 24, point text) and a rect shape layer (fill only), each with
its Transparency locked:
- **resize**: `edit_text_layer {size: 60}` and `edit_shape_layer` with a wider `w` answer the
  lock's sentence; the layer's description, its bounds and the undo count read back unchanged.
- **no-op**: an edit that repeats the stored values answers success and adds no step.
- **not the lock**: an emoji under a warp on the locked text layer, and on an unlocked twin,
  answers the capture's own sentence, never the lock's.
- **dropped**: an edit that changes the description and not the raster (a short line's
  `wrap_width` raised; a fill-only rect's `radius`), first confirmed on an unlocked twin to keep
  the layer's bounds. At 16 bits the typed core keeps the old raster AND the old description
  under the lock, so the call answers the lock's sentence and the description is unchanged. At 8
  bits each answer is printed as it is (`INFO` lines).

Usage: python3 -B scripts/verify_locked_rerender.py
"""
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 400, 300
DEPTHS = (8, 16)
LOCKED = 'has its transparency locked'
HINT = 'set_layer_lock'
EMOJI = 'warped text needs outline glyphs'
TEXT = dict(text='Locked', font='Menlo', size=24, x=40, y=40, wrap_width=0,
            rgba=[0.2, 0.4, 0.8, 1])
SHORT = dict(text='Hi', font='Menlo', size=24, x=40, y=200, wrap_width=200, rgba=[0, 0, 0, 1])
WIDER_WRAP = 260
RECT = dict(kind='rect', x=200, y=40, w=80, h=50, fill_rgba=[0.2, 0.4, 0.8, 1])
WIDER_W = 140
RADIUS = 12


def main():
    output = Path(tempfile.mkdtemp(prefix='locked-rerender-'))
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    # A background instance: never activates itself or takes the screen (`BackgroundLaunch`).
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    failures = []

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def raw(tool, **arguments):
            """(isError, text) of one call, whatever it answers."""
            reply = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in reply.get('content', []) if c['type'] == 'text'), '')
            return bool(reply.get('isError')), text

        def state(doc, layer):
            row = {r['index']: r for r in call('get_document', document_id=doc)['layers']}[layer]
            return {'text': row.get('text'), 'shape': row.get('shape'),
                    'bounds': (row['offset_x'], row['offset_y'], row['width'], row['height'])}

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        def check(label, ok, detail):
            if ok:
                print(f'PASS {label}', flush=True)
            else:
                failures.append(label)
                print(f'FAIL {label}: {detail}', flush=True)

        def lock(doc, layer):
            call('set_layer_lock', document_id=doc, layer=layer, locks=['transparency'])

        def refused_by_lock(label, doc, layer, tool, **arguments):
            before, steps = state(doc, layer), undo_count(doc)
            error, text = raw(tool, document_id=doc, layer=layer, **arguments)
            after = state(doc, layer)
            check(label, error and LOCKED in text and HINT in text and after == before
                  and undo_count(doc) == steps,
                  f'error {error}: {text!r}; unchanged {after == before}; '
                  f'steps {steps} -> {undo_count(doc)}')

        def answered_ok(label, doc, layer, tool, **arguments):
            before, steps = state(doc, layer), undo_count(doc)
            error, text = raw(tool, document_id=doc, layer=layer, **arguments)
            check(label, not error and state(doc, layer) == before and undo_count(doc) == steps,
                  f'error {error}: {text!r}; steps {steps} -> {undo_count(doc)}')

        def own_words(label, doc, layer):
            error, text = raw('edit_text_layer', document_id=doc, layer=layer, text='Hi \U0001F600',
                              warp={'style': 'arch', 'bend': 0.5})
            check(label, error and EMOJI in text and LOCKED not in text, f'error {error}: {text!r}')

        def keeps_bounds(label, doc, tool, made, argument, field, key, value):
            """An unlocked twin of `made`: `argument` set to `value` reads back as `field`'s
            `key`, the bounds kept."""
            twin = call(tool.replace('edit', 'add'), document_id=doc, **made)['layer']
            before = state(doc, twin)
            call(tool, document_id=doc, layer=twin, **{argument: value})
            after = state(doc, twin)
            check(label, after['bounds'] == before['bounds']
                  and (after[field] or {}).get(key) == value,
                  f'bounds {before["bounds"]} -> {after["bounds"]}; {field} {after[field]}')
            call('delete_layer', document_id=doc, layer=twin)

        def dropped(label, depth, doc, layer, tool, **arguments):
            before = state(doc, layer)
            error, text = raw(tool, document_id=doc, layer=layer, **arguments)
            after = state(doc, layer)
            if depth == 8:
                print(f'INFO {label}: error {error}: {text!r}; description changed '
                      f'{after != before}', flush=True)
                return
            check(label, error and LOCKED in text and HINT in text and after == before,
                  f'error {error}: {text!r}; unchanged {after == before}')

        for depth in DEPTHS:
            tag = f'rgb{depth}'
            doc = call('new_document', width=W, height=H, background='white', depth=depth,
                       profile='srgb')['document']['id']
            text = call('add_text_layer', document_id=doc, **TEXT)['layer']
            shape = call('add_shape_layer', document_id=doc, **RECT)['layer']
            plain = call('add_text_layer', document_id=doc, **{**TEXT, 'y': 120})['layer']
            short = call('add_text_layer', document_id=doc, **SHORT)['layer']
            for layer in (text, shape, short):
                lock(doc, layer)

            # ---- resize ------------------------------------------------------------------------
            refused_by_lock(f'{tag} resize text: the lock sentence, nothing changed', doc, text,
                            'edit_text_layer', size=60)
            refused_by_lock(f'{tag} resize shape: the lock sentence, nothing changed', doc, shape,
                            'edit_shape_layer', w=WIDER_W)

            # ---- no-op -------------------------------------------------------------------------
            answered_ok(f'{tag} no-op text: success, no step', doc, text, 'edit_text_layer',
                        size=TEXT['size'])
            answered_ok(f'{tag} no-op shape: success, no step', doc, shape, 'edit_shape_layer',
                        w=RECT['w'])

            # ---- not the lock ------------------------------------------------------------------
            own_words(f'{tag} not the lock (locked): the capture\'s own sentence', doc, text)
            own_words(f'{tag} not the lock (unlocked): the capture\'s own sentence', doc, plain)

            # ---- dropped -----------------------------------------------------------------------
            keeps_bounds(f'{tag} dropped text twin: wrap_width keeps the bounds', doc,
                         'edit_text_layer', SHORT, 'wrap_width', 'text', 'box_width', WIDER_WRAP)
            keeps_bounds(f'{tag} dropped shape twin: radius keeps the bounds', doc,
                         'edit_shape_layer', RECT, 'radius', 'shape', 'radius', RADIUS)
            dropped(f'{tag} dropped text: the lock sentence, the description kept', depth, doc,
                    short, 'edit_text_layer', wrap_width=WIDER_WRAP)
            dropped(f'{tag} dropped shape: the lock sentence, the description kept', depth, doc,
                    shape, 'edit_shape_layer', radius=RADIUS)

            call('close_document', document_id=doc, discard_changes=True)

    if failures:
        print(f'FAIL {len(failures)}: {"; ".join(failures)}', flush=True)
        sys.exit(1)
    print('PASS all (rgb8, rgb16)', flush=True)


if __name__ == '__main__':
    main()
