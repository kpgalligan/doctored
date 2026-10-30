#!/usr/bin/env python3
"""The MCP drive: Layer > Duplicate to New Document.

Six gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no expectation is relaxed afterwards. The instance is owned by
this file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads) and is terminated by its own context
manager; no Doctored process this file did not start is ever signalled.

- **D1 8 and 16 bits.** A stack holding a raster with a mask, a style, an
  opacity, a blend mode, a label and a lock; a text layer; an adjustment
  layer; and a group is copied whole by `duplicate_to_new_document`. The new
  document's precision, profile, size and PPI equal the source's, every row's
  carried properties equal the source row's, `unsaved_changes` is true, its
  history holds one state (no undo step), `edit_text_layer` succeeds on the
  copied text layer, and the SOURCE's history and dirty flag are unchanged.
- **D2 32-bit float.** The same checks over the 11F fixture with a masked
  raster, an exposure adjustment and a group.
- **D3 replay.** A saved step `{"layers": {"$layers": "selected"}}` run on a
  document with two of its three layers selected opens ONE new document
  holding exactly those two. An MCP-recorded step carries its literal
  `layers`.
- **D4 edits in the duplicate.** Recording a duplicate and then an opacity
  change on the NEW document records the duplicate step plus ONE placeholder;
  running that action on another document reports the placeholder as a
  failed step, and that document's opacity and history are untouched by it.
- **D6 duplicate of a duplicate.** Recording an opacity change on a source,
  a duplicate of it, and then a duplicate OF THAT duplicate records the edit,
  the first duplicate step and ONE placeholder (replay would otherwise copy
  the run's own document), and the saved action keeps the source's canvas.
  The second call names the first duplicate's `document_id`: a background
  instance has no current document, so that is the deterministic way to read
  from it.
- **D5 Batch.** `run_batch` with an action holding the step over a one-file
  folder fails that file's row with the named reason, and `list_documents`
  shows no extra document afterwards.

Nothing here derives an expectation from Doctored: every property list,
count and refusal substring below is written in this file.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

# ---- the expectations, fixed here BEFORE anything is measured -------------
CANVAS = (160, 120)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
DEFAULT_APP = app_under_test.default_app_bundle()

# The per-row properties a copy carries verbatim (`get_document`'s keys).
# Indices (index, parent, children) are left out: they renumber when only a
# subset is copied, and D1/D2 copy every root, so the table order itself is
# compared instead.
CARRIED = ['name', 'width', 'height', 'offset_x', 'offset_y', 'opacity', 'blend_mode',
           'visible', 'has_mask', 'mask_enabled', 'clipped', 'label', 'kind', 'depth',
           'locks', 'text', 'is_adjustment', 'adjustment', 'style', 'mask_properties', 'open']
RASTER_COLOUR = '#c06020'
RASTER_OPACITY = 0.5
RASTER_BLEND = 'multiply'
RASTER_LABEL = 'red'
RASTER_LOCKS = ['position']
RASTER_STYLE = {'effects': [{'type': 'drop_shadow', 'distance': 4, 'size': 3}]}
EDITED_TEXT = 'World'

REPLAY_ACTION = 'issue129-replay-selected'
REPLAY_STEP = {'tool': 'duplicate_to_new_document',
               'arguments': {'layers': {'$layers': 'selected'}}}
REPLAY_NAMES = ['A', 'B', 'C']
REPLAY_SELECTED = [0, 2]
REPLAY_EXPECTED = ['A', 'C']
RECORDED_ACTION = 'issue129-recorded'
RECORDED_LAYERS = [1]
EDITS_ACTION = 'issue129-edits'
BATCH_ACTION = 'issue129-batch'
NESTED_ACTION = 'issue129-nested'
UNRECORDED = '__unrecorded'
PLACEHOLDER = 'Edits in a document made by Duplicate to New Document'
EDIT_OPACITY = 0.25
BATCH_REASON = 'cannot run inside a Batch'

GATE_ORDER = ['d1', 'd2', 'd3', 'd4', 'd5', 'd6']


# -------------------------------------------------------------- utilities --

def rows(app, document):
    return app.call('get_document', document_id=document)['layers']


def carried(row):
    return {key: row.get(key) for key in CARRIED}


def history(app, document):
    report = app.call('list_history', document_id=document)
    return len(report['states']), report['cursor']


def dirty(app, document):
    return app.call('get_document', document_id=document)['unsaved_changes']


def document_ids(app):
    return sorted(doc['id'] for doc in app.call('list_documents')['documents'])


def facts(app, document):
    """Precision, profile, size and PPI — what the new document must share."""
    doc = app.call('get_document', document_id=document)
    precision = app.call('get_precision', document_id=document)
    return {
        'bits': precision['bits_per_channel'],
        'profile': doc['color_profile']['name'],
        'size': (doc['width'], doc['height']),
        'ppi': (doc['resolution']['ppi_x'], doc['resolution']['ppi_y']),
    }


def duplicate_whole(app, document, findings, label):
    """Copies every root and checks D1/D2's claims on the result."""
    source_rows = rows(app, document)
    source_history = history(app, document)
    source_dirty = dirty(app, document)
    roots = [row['index'] for row in source_rows if int(row.get('depth', 0)) == 0]
    reply = app.call('duplicate_to_new_document', document_id=document, layers=roots)
    made = reply['document']['id']
    assert made != document, reply
    assert reply['source_document_id'] == document, reply
    assert reply['source_layers'] == roots, reply
    before, after = facts(app, document), facts(app, made)
    assert after == before, (label, before, after)
    assert reply['depth'] == before['bits'], reply
    assert (reply['width'], reply['height']) == before['size'], reply
    copied_rows = rows(app, made)
    assert [carried(r) for r in copied_rows] == [carried(r) for r in source_rows], (
        label, [carried(r) for r in copied_rows], [carried(r) for r in source_rows])
    source_ids = {row['id'] for row in source_rows}
    assert not source_ids & {row['id'] for row in copied_rows}, 'ids must be fresh'
    assert dirty(app, made) is True, 'the new document is unsaved'
    states, _ = history(app, made)
    assert states == 1, ('the new document has no undo step', states)
    assert history(app, document) == source_history, 'the source gained a history step'
    assert dirty(app, document) == source_dirty, 'the source dirty flag changed'
    findings[label] = {'reply': reply, 'facts': after, 'rows': [carried(r) for r in copied_rows]}
    return made, copied_rows


def build_stack(app, depth):
    """Background, a styled masked labelled locked raster, a text layer, an
    adjustment layer, and the background wrapped in a group."""
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='white')
    document = created['document']['id']
    app.call('duplicate_layer', document_id=document, index=0)
    app.call('set_layer_properties', document_id=document, index=1, name='Raster',
             opacity=RASTER_OPACITY, blend_mode=RASTER_BLEND)
    app.call('fill', document_id=document, x=CANVAS[0] // 3, y=CANVAS[1] // 3,
             color=RASTER_COLOUR, tolerance=0, contiguous=True, layer=1)
    app.call('add_layer_mask', document_id=document, layer=1, kind='reveal_all')
    app.call('set_layer_style', document_id=document, layer=1, style=RASTER_STYLE)
    app.call('set_layer_label', document_id=document, layer=1, label=RASTER_LABEL)
    app.call('set_layer_lock', document_id=document, layer=1, locks=RASTER_LOCKS)
    app.call('set_active_layer', document_id=document, index=1)
    app.call('add_text_layer', document_id=document, text='Hello', x=10, y=10, size=24,
             color='#000000')
    app.call('add_adjustment_layer', document_id=document, op='invert')
    app.call('group_layers', document_id=document, layers=[0], name='Group')
    return document


# ------------------------------------------------------------------ gates --

def gate_d1(app, folder, receipts):
    findings = {}
    for depth in (8, 16):
        document = build_stack(app, depth)
        made, copied = duplicate_whole(app, document, findings, str(depth))
        text = next(row['index'] for row in copied if row.get('text'))
        edited = app.call('edit_text_layer', document_id=made, layer=text, text=EDITED_TEXT)
        findings[f'{depth}_edit_text'] = edited
        assert rows(app, made)[text]['text']['string'] == EDITED_TEXT
    receipts['d1_8_and_16'] = findings
    print('PASS d1: 8 and 16 bits carry every property into a dirty, undo-free document',
          flush=True)


def gate_d2(app, folder, receipts):
    if not FLOAT_FIXTURE.exists():
        receipts['d2_float'] = 'skipped: the 11F float fixture is not present'
        print('SKIP d2: the 11F fixture is absent', flush=True)
        return
    path = folder / 'd2.tiff'
    shutil.copyfile(FLOAT_FIXTURE, path)
    document = app.call('open_document', path=str(path))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    app.call('add_layer_mask', document_id=document, layer=0, kind='reveal_all')
    app.call('add_adjustment_layer', document_id=document, op='exposure',
             params={'exposure': 0.5})
    app.call('group_layers', document_id=document, layers=[0], name='Group')
    findings = {}
    duplicate_whole(app, document, findings, '32')
    receipts['d2_float'] = findings
    print('PASS d2: a float document copies at 32 bits with its mask, exposure and group',
          flush=True)


def three_layers(app):
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']
    for _ in range(len(REPLAY_NAMES) - 1):
        app.call('duplicate_layer', document_id=document, index=0)
    for index, name in enumerate(REPLAY_NAMES):
        app.call('set_layer_properties', document_id=document, index=index, name=name)
    return document


def gate_d3(app, folder, receipts):
    findings = {}
    app.call('save_action', name=REPLAY_ACTION, steps=[REPLAY_STEP])
    document = three_layers(app)
    app.call('set_selected_layers', document_id=document, layers=REPLAY_SELECTED)
    before = document_ids(app)
    run = app.call('run_action', name=REPLAY_ACTION, document_id=document)
    after = document_ids(app)
    opened = [doc for doc in after if doc not in before]
    assert len(opened) == 1, (before, after, run)
    names = [row['name'] for row in rows(app, opened[0])]
    assert names == REPLAY_EXPECTED, names
    findings['run'] = run
    findings['names'] = names

    app.call('start_recording', discard=True)
    app.call('duplicate_to_new_document', document_id=document, layers=RECORDED_LAYERS)
    stopped = app.call('stop_recording', name=RECORDED_ACTION)
    steps = stopped['steps']
    assert [step['tool'] for step in steps] == ['duplicate_to_new_document'], steps
    assert steps[0]['arguments'].get('layers') == RECORDED_LAYERS, steps
    findings['recorded'] = steps
    receipts['d3_replay'] = findings
    print('PASS d3: a selected-layers step follows the run target; an MCP step is literal',
          flush=True)


def gate_d4(app, folder, receipts):
    findings = {}
    source = three_layers(app)
    app.call('start_recording', discard=True)
    reply = app.call('duplicate_to_new_document', document_id=source, layers=[0])
    made = reply['document']['id']
    app.call('set_layer_properties', document_id=made, index=0, opacity=EDIT_OPACITY)
    app.call('set_layer_properties', document_id=made, index=0, opacity=EDIT_OPACITY / 2)
    stopped = app.call('stop_recording', name=EDITS_ACTION)
    steps = stopped['steps']
    tools = [step['tool'] for step in steps]
    assert tools == ['duplicate_to_new_document', UNRECORDED], steps
    assert steps[1]['arguments'].get('action') == PLACEHOLDER, steps
    findings['steps'] = steps

    target = three_layers(app)
    opacity = rows(app, target)[0]['opacity']
    states_before = history(app, target)
    run = app.call('run_action', name=EDITS_ACTION, document_id=target)
    text = json.dumps(run)
    assert PLACEHOLDER in text, run
    assert rows(app, target)[0]['opacity'] == opacity, 'the placeholder edited the run target'
    states_after = history(app, target)
    assert states_after == states_before, (states_before, states_after)
    findings['run'] = run
    receipts['d4_edits_in_duplicate'] = findings
    print('PASS d4: edits in the duplicate record one placeholder that fails by name',
          flush=True)


def gate_d5(app, folder, receipts):
    findings = {}
    source = three_layers(app)
    inbox = folder / 'batch-in'
    outbox = folder / 'batch-out'
    inbox.mkdir(parents=True, exist_ok=True)
    app.call('save_copy', document_id=source, path=str(inbox / 'one.png'))
    app.call('save_action', name=BATCH_ACTION, steps=[REPLAY_STEP])
    before = document_ids(app)
    started = app.call('run_batch', name=BATCH_ACTION, input=str(inbox), output=str(outbox))
    batch = started['batch_id']
    deadline = time.monotonic() + 60
    while True:
        status = app.call('batch_status', batch_id=batch)
        if not status['running']:
            break
        assert time.monotonic() < deadline, status
        time.sleep(0.2)
    text = json.dumps(status['report'])
    assert BATCH_REASON in text, status
    after = document_ids(app)
    assert after == before, (before, after)
    findings['status'] = status
    receipts['d5_batch'] = findings
    print('PASS d5: Batch refuses the step by name and leaves no extra document', flush=True)


def gate_d6(app, folder, receipts):
    findings = {}
    source = three_layers(app)
    app.call('start_recording', discard=True)
    app.call('set_layer_properties', document_id=source, index=0, opacity=EDIT_OPACITY)
    first = app.call('duplicate_to_new_document', document_id=source, layers=[0, 1])
    made = first['document']['id']
    second = app.call('duplicate_to_new_document', document_id=made, layers=[0])
    assert second['source_document_id'] == made, second
    stopped = app.call('stop_recording', name=NESTED_ACTION)
    steps = stopped['steps']
    tools = [step['tool'] for step in steps]
    assert tools == ['set_layer_properties', 'duplicate_to_new_document', UNRECORDED], steps
    assert steps[2]['arguments'].get('action') == PLACEHOLDER, steps
    listed = [row for row in app.call('list_actions')['actions'] if row['name'] == NESTED_ACTION]
    assert len(listed) == 1, listed
    canvas = listed[0]['recorded_canvas']
    assert canvas == {'width': CANVAS[0], 'height': CANVAS[1]}, canvas
    findings['steps'] = steps
    findings['recorded_canvas'] = canvas
    receipts['d6_duplicate_of_duplicate'] = findings
    print('PASS d6: a duplicate of a duplicate records one placeholder; the canvas stays the '
          'source\'s', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', default=str(DEFAULT_APP))
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--gates', default=','.join(GATE_ORDER),
                        help='comma list of gates to run, from ' + ','.join(GATE_ORDER))
    args = parser.parse_args()
    selected = [gate.strip() for gate in args.gates.split(',') if gate.strip()]
    unknown = [gate for gate in selected if gate not in GATE_ORDER]
    if unknown:
        parser.error(f'unknown gate(s): {",".join(unknown)}')
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts or tempfile.mkdtemp(prefix='issue129-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'expectations': 'fixed in scripts/verify_issue129_duplicate_to_new_document.py '
                                'before any measurement'}
    gates = {'d1': gate_d1, 'd2': gate_d2, 'd3': gate_d3, 'd4': gate_d4, 'd5': gate_d5,
             'd6': gate_d6}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        for name in GATE_ORDER:
            if name in selected:
                gates[name](app, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
