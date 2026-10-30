#!/usr/bin/env python3
"""The MCP drive on an owned Debug app: a layer style and Blend
If on a PASS THROUGH group, which then composites isolated, and the style
setter refusing an unknown key by name.

The scene: a new document (its background filled #3070c0), an opaque square
on its own layer, an adjustment layer above it (Invert at 8 and 16 bits, Exposure +1 at
32, Levels on CMYK), both grouped with `group_layers` (a new group is Pass
Through). BACKDROP is `sample_color` at a background point outside the
square, read before the adjustment exists.

Gates, fixed before the run (a failed gate IS the receipt):

- **G1 accepted, 8 / 16 / 32 bits and CMYK 8 / 16.** Under the unstyled
  group the point is adjusted (it differs from BACKDROP). `set_layer_style`
  on the group with a Stroke and a Gray Blend If is accepted; `get_document`
  reports the group as Pass Through with the stroke and the `blend_if`; it is
  ONE undo step; the point reads BACKDROP again (the group isolates); Undo
  restores the adjusted reading.
- **G2 mode switch, 16 bits.** `set_layer_properties blend_mode "Pass
  Through"` on a styled Normal group is accepted and the style is kept.
- **G3 presets, 16 bits.** `apply_style_preset` (a preset saved from a
  styled layer, effects only: Blend If is a blending option a preset without
  blending does not carry) on the Pass Through group reports no `skipped`,
  and the group carries the preset's stroke.
- **G4 Actions.** `start_recording` → G1's `set_layer_style` on a 16-bit
  scene → `stop_recording` → `run_action` on a second 16-bit scene gives the
  group the same style.
- **G5 unknown key, 8 / 16 bits.** `set_layer_style` with
  `effects[0].blend_mode` is refused in-band, the message naming
  `effects[0].blend_mode`, and adds no undo step.

Launch: `phase5_support.App` (scratch socket, dummy key, background, every
preset folder private); only that instance is ever stopped.
"""
import argparse
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App  # noqa: E402

W, H = 48, 32
POINT = (4, 4)
STYLE = {'blend_if': {'channel': 'gray', 'this_layer': [0, 0, 128, 128],
                      'underlying': [0, 0, 255, 255]},
         'effects': [{'type': 'stroke', 'size': 2, 'color': '#ff0000'}]}
BAD = {'effects': [{'type': 'drop_shadow', 'blend_mode': 'normal'}]}


def rows(app, doc):
    return app.call('get_document', document_id=doc)['layers']


def index_of(app, doc, name):
    return next(row['index'] for row in rows(app, doc) if row['name'] == name)


def undo_count(app, doc):
    return app.call('list_history', document_id=doc)['undo_count']


def reading(app, doc):
    sample = app.call('sample_color', document_id=doc, x=POINT[0], y=POINT[1])
    return {k: v for k, v in sample.items() if k not in ('x', 'y', 'note')}


def adjustment(depth, cmyk):
    if cmyk:
        return dict(op='levels', params={'gamma': 1.2})
    if depth == 32:
        return dict(op='exposure', params={'exposure': 1.0})
    return dict(op='invert', params={})


def scene(app, depth, cmyk=False):
    """(document, group index, BACKDROP reading)."""
    extra = dict(color_mode='cmyk') if cmyk else {}
    doc = app.call('new_document', width=W, height=H, unit='px', depth=depth,
                   background='white', **extra)['document']['id']
    # A mid-tone backdrop, which every one of the adjustments moves.
    app.call('select_rect', document_id=doc, x=0, y=0, width=W, height=H)
    app.call('fill_selection', document_id=doc, layer=0, color='#3070c0')
    app.call('deselect', document_id=doc)
    app.call('new_layer', document_id=doc, name='Square')
    square = index_of(app, doc, 'Square')
    app.call('select_rect', document_id=doc, x=26, y=8, width=12, height=12)
    app.call('fill_selection', document_id=doc, layer=square, color='#c8c8c8')
    app.call('deselect', document_id=doc)
    backdrop = reading(app, doc)
    app.call('add_adjustment_layer', document_id=doc, name='Adj', **adjustment(depth, cmyk))
    group = app.call('group_layers', document_id=doc,
                     layers=[square, index_of(app, doc, 'Adj')], name='P')['group']
    return doc, group, backdrop


def carries(row):
    style = (row or {}).get('style') or {}
    return ([e['type'] for e in style.get('effects') or []] == ['stroke']
            and (style.get('blend_if') or {}).get('this_layer') == [0, 0, 128, 128])


def gate(gates, name, ok, detail):
    gates[name] = dict(passed=bool(ok), detail=detail)
    print(('PASS ' if ok else 'FAIL ') + name, flush=True)


def drive(app, folder, gates):
    for depth, cmyk in ((8, False), (16, False), (32, False), (8, True), (16, True)):
        label = f"{'CMYK' if cmyk else 'RGB'}{depth}"
        doc, group, backdrop = scene(app, depth, cmyk)
        adjusted = reading(app, doc)
        before = undo_count(app, doc)
        reply = app.call('set_layer_style', document_id=doc, layer=group, style=STYLE)
        row = rows(app, doc)[group]
        steps = undo_count(app, doc) - before
        isolated = reading(app, doc)
        app.call('undo', document_id=doc)
        undone = reading(app, doc)
        gate(gates, f'G1 {label} accepted, Pass Through, one step, isolates, undone',
             reply.get('ok') and row.get('blend_mode') == 'Pass Through' and carries(row)
             and steps == 1 and adjusted != backdrop and isolated == backdrop
             and undone == adjusted,
             dict(row_blend=row.get('blend_mode'), style=row.get('style'), steps=steps,
                  backdrop=backdrop, adjusted=adjusted, isolated=isolated, undone=undone))
        app.call('close_document', document_id=doc, discard_changes=True)

    doc, group, _ = scene(app, 16)
    app.call('set_layer_properties', document_id=doc, index=group, blend_mode='Normal')
    app.call('set_layer_style', document_id=doc, layer=group, style=STYLE)
    switched = app.call('set_layer_properties', document_id=doc, index=group,
                        blend_mode='Pass Through')
    row = rows(app, doc)[group]
    gate(gates, 'G2 RGB16 a styled Normal group switches to Pass Through and keeps its style',
         row.get('blend_mode') == 'Pass Through' and carries(row),
         dict(reply=switched, row_blend=row.get('blend_mode'), style=row.get('style')))
    app.call('close_document', document_id=doc, discard_changes=True)

    source, source_group, _ = scene(app, 16)
    app.call('set_layer_style', document_id=source, layer=source_group, style=STYLE)
    saved = app.call('save_style_preset', document_id=source, layer=source_group,
                     name='s558-pass-through')
    doc, group, _ = scene(app, 16)
    applied = app.call('apply_style_preset', document_id=doc, layers=[group],
                       name='s558-pass-through')
    row = rows(app, doc)[group]
    effects = [e['type'] for e in (row.get('style') or {}).get('effects') or []]
    gate(gates, 'G3 RGB16 apply_style_preset on the Pass Through group skips nothing',
         applied.get('ok') and 'skipped' not in applied and effects == ['stroke']
         and row.get('blend_mode') == 'Pass Through',
         dict(saved=saved.get('preset'), applied=applied, style=row.get('style')))
    for d in (source, doc):
        app.call('close_document', document_id=d, discard_changes=True)

    first, first_group, _ = scene(app, 16)
    app.call('start_recording', discard=True)
    app.call('set_layer_style', document_id=first, layer=first_group, style=STYLE)
    stopped = app.call('stop_recording', name='s558-pass-through-style')
    second, second_group, _ = scene(app, 16)
    replay = app.call('run_action', document_id=second, name='s558-pass-through-style')
    a = rows(app, first)[first_group].get('style')
    b = rows(app, second)[second_group].get('style')
    gate(gates, 'G4 RGB16 the recorded Action replays the group style on a twin',
         carries(rows(app, second)[second_group]) and a == b,
         dict(steps=stopped.get('steps'), replay=replay, recorded=a, replayed=b))
    for d in (first, second):
        app.call('close_document', document_id=d, discard_changes=True)

    for depth in (8, 16):
        doc, group, _ = scene(app, depth)
        before = undo_count(app, doc)
        refusal = app.call('set_layer_style', fail=True, document_id=doc, layer=group,
                           style=BAD)
        after = undo_count(app, doc)
        gate(gates, f'G5 RGB{depth} an unknown key is refused by name with no undo step',
             'effects[0].blend_mode' in refusal and after == before,
             dict(refusal=refusal, before=before, after=after))
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
    print('verify_558_pass_through: ' + ('PASS' if not failed else f'FAIL {failed}'))
    raise SystemExit(0 if not failed else 1)


if __name__ == '__main__':
    main()
