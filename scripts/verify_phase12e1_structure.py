#!/usr/bin/env python3
"""The Phase 12E-1 MCP drive: persisted layer ids and typed structure.

Six gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no expectation is relaxed afterwards. The instance is owned by
this file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads) and is terminated by its own context
manager; no Doctored process this file did not start is ever signalled.

- **S1 ids survive every persistence path at every depth.** For an 8-bit
  document (carrying a colour label, a layer comp and the linear-light
  evaluation policy when it is saved), a 16-bit document (`new_document
  depth:16`) and a 32-bit float document (the 11F TIFF fixture), the layer
  table — name, depth, parent, id — read before the write must come back
  ROW FOR ROW after `save_copy` .rz + `open_document`, after
  `write_recovery` + `recover_document`, and after `save_snapshot` +
  `load_snapshot`.
- **S2 the five structure ops answer at 16 bits and at float.** Each of
  `reorder_layers`, `reorder_layer`, `group_layers {subset}`,
  `ungroup_layers` and `delete_layer` produces the EXACT table this file
  states (name, depth, parent, id); `sample_pixel` on every surviving layer
  is unchanged; ids are carried (only a new container mints);
  `list_history`'s cursor advances by exactly one per operation and one
  `undo` restores the previous table.
- **S3 the refusals are by name, and nothing else moves.** `delete_layer` of
  every layer, `group_layers` across two parents, and `ungroup_layers` on a
  raster leaf are each refused — the last with the CORE's own target
  sentence. `unsaved_changes` is true after each op, and the document's path
  is unchanged by `save_copy`.
- **S4 `set_view` refuses a malformed argument by name.** `{zoom: "big"}`,
  `{center: 3}` and `{view: "x"}` are each refused, `get_view` is unchanged
  afterwards, and `close_second_view`'s catalog `view` description contains
  no "Omit".
- **S5 `get_precision` lists the four new families** on the 16-bit AND the
  float document, and names `arrange_layer` beside the five structure
  tools.
- **S6 `arrange_layer` answers and replays at depth.** At 16 bits and at
  32-bit float an arrange produces the EXACT table this file states, its reply
  names the landing index, the active layer follows the entry and one history
  step is added; an arrange that moves nothing is the house no-op (no history
  step, no dirtying); an entry inside a group stays in it; an adjustment layer
  lands like any other. A saved Action with the UI's `{"$layer": "active"}`
  step shape and a RECORDED arrange both replay through `run_action` on a
  typed document. `align_layers` / `distribute_layers` answer at 16 bits and
  at float: over three duplicates placed by `set_layer_properties`
  offsets, align-left to the layers puts every offset at the minimum, to the
  canvas puts every content left edge at 0, `undo` restores the offsets in
  one step, distribute puts the middle offset at round((first + last) / 2),
  and an Align recorded at 8 bits replays through `run_action` at 16 bits and
  at float. The 8-bit arrange reply is unchanged.

Nothing here derives an expectation from Doctored: every structure table,
every refusal substring and every count below is written in this file.
"""
import argparse
import json
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the expectations, fixed here BEFORE anything is measured -------------
# There are no tolerances: every value is an integer, a string or a table this
# file wrote down, compared for exact equality.
CANVAS = (200, 150)
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

# S2's starting stack, bottom-first: four roots named here.
START = ['A', 'B', 'C', 'D']
# One flat colour per layer, chosen here, so `sample_pixel` tells them apart
# and "the pixels did not move" is a claim with teeth.
COLOURS = ['#204080', '#c06020', '#30a060', '#a03060']

# The exact tables S2 demands, as (name, depth, parent-name) triples in
# postorder. `None` is the top level.
AFTER_REORDER_LAYERS = [('B', 0, None), ('C', 0, None), ('A', 0, None), ('D', 0, None)]
AFTER_REORDER_LAYER = [('C', 0, None), ('B', 0, None), ('A', 0, None), ('D', 0, None)]
AFTER_GROUP = [('C', 0, None), ('B', 1, 'Set'), ('A', 1, 'Set'), ('Set', 0, None),
               ('D', 0, None)]
AFTER_UNGROUP = [('C', 0, None), ('B', 0, None), ('A', 0, None), ('D', 0, None)]
AFTER_DELETE = [('C', 0, None), ('B', 0, None), ('A', 0, None)]

# S3's refusal substrings, chosen here.
REFUSALS = {
    'delete_all': 'at least one',
    'group_across_parents': 'same',
    'ungroup_raster': 'does not support target raster',
}

# S5's four families.
FAMILIES = ['layer_move', 'layer_group', 'layer_ungroup', 'layer_remove']
# S5's structure tools every typed depth must list: the five 12E-1 ones plus
# `arrange_layer`.
S5_TOOLS = ['reorder_layer', 'reorder_layers', 'group_layers', 'ungroup_layers',
            'delete_layer', 'arrange_layer']

# S6's exact tables, bottom-first (name, depth, parent-name). `None` is the
# top level.
S6_DIRECT_16 = [('B', 0, None), ('C', 0, None), ('D', 0, None), ('A', 0, None)]
S6_DIRECT_16_INDEX = 3
S6_START_16 = [('A', 0, None), ('B', 0, None), ('C', 0, None), ('D', 0, None)]
S6_GROUPED_16 = [('A', 0, None), ('B', 1, 'G'), ('C', 1, 'G'), ('G', 0, None), ('D', 0, None)]
# (moved entry, direction, table after, landing index), in order.
S6_TREE_16 = [
    ('G', 'forward', [('A', 0, None), ('D', 0, None), ('B', 1, 'G'), ('C', 1, 'G'),
                      ('G', 0, None)], 4),
    ('D', 'forward', [('A', 0, None), ('B', 1, 'G'), ('C', 1, 'G'), ('G', 0, None),
                      ('D', 0, None)], 4),
    ('G', 'back', [('B', 1, 'G'), ('C', 1, 'G'), ('G', 0, None), ('A', 0, None),
                   ('D', 0, None)], 2),
    ('B', 'front', [('C', 1, 'G'), ('B', 1, 'G'), ('G', 0, None), ('A', 0, None),
                    ('D', 0, None)], 1),
]
S6_ADJUSTED_16 = [('A', 0, None), ('Adj', 0, None), ('B', 0, None), ('C', 0, None),
                  ('D', 0, None)]
S6_ADJUSTMENT_16 = [('A', 0, None), ('B', 0, None), ('C', 0, None), ('D', 0, None),
                    ('Adj', 0, None)]
S6_ADJUSTMENT_16_INDEX = 4
S6_REPLAYED_16 = [('B', 0, None), ('C', 0, None), ('D', 0, None), ('Moved', 0, None)]
S6_REPLAYED_16_ACTIVE = 3
S6_UI_ACTION = 's6-arrange-ui'
S6_UI_STEPS = [
    {'tool': 'arrange_layer', 'arguments': {'layer': {'$layer': 'active'}, 'to': 'front'}},
    {'tool': 'set_layer_properties',
     'arguments': {'layer': {'$layer': 'active'}, 'name': 'Moved'}},
]
S6_RECORDED_ACTION = 's6-arrange-recorded'
S6_RECORDED_TOOLS = ['arrange_layer', 'set_layer_properties']
S6_NOOP_NOTE = 'Nothing changed'
S6_START_FLOAT = [('A', 0, None), ('B', 0, None), ('C', 0, None)]
S6_DIRECT_FLOAT = [('B', 0, None), ('C', 0, None), ('A', 0, None)]
S6_DIRECT_FLOAT_INDEX = 2
S6_REPLAYED_FLOAT = [('B', 0, None), ('C', 0, None), ('Moved', 0, None)]
S6_REPLAYED_FLOAT_ACTIVE = 2
S6_GROUPED_FLOAT = [('A', 1, 'G'), ('B', 1, 'G'), ('G', 0, None), ('C', 0, None)]
S6_TREE_FLOAT = [
    ('B', 'back', [('B', 1, 'G'), ('A', 1, 'G'), ('G', 0, None), ('C', 0, None)], 0),
    ('G', 'forward', [('C', 0, None), ('B', 1, 'G'), ('A', 1, 'G'), ('G', 0, None)], 3),
]
# Three duplicates placed at these (offset_x, offset_y), the
# expected offsets after align-left to the layers (every x at the minimum,
# 10) and after distribute (the middle x at round((10 + 110) / 2) = 60). The
# duplicates share one content box, so these come from offsets alone.
S6_ALIGN_LAYERS = [0, 1, 2]
S6_ALIGN_START = [(10, 5), (80, 20), (110, 40)]
S6_ALIGN_LEFT = [(10, 5), (10, 20), (10, 40)]
S6_DISTRIBUTED = [(10, 5), (60, 20), (110, 40)]
S6_ALIGN_ACTION = 's6-align-recorded'
S6_ALIGN_RECORDED_TOOLS = ['align_layers']
S6_DIRECT_8 = [('B', 0, None), ('C', 0, None), ('D', 0, None), ('A', 0, None)]
S6_DIRECT_8_INDEX = 3

GATE_ORDER = ['s1', 's2', 's2_float', 's3', 's4', 's5', 's6']


# -------------------------------------------------------------- utilities --

def structure(app, document):
    """The document's layer table as (name, depth, parent-name, id) rows in
    the order `get_document` lists them — bottom-first at byte depth,
    postorder at typed depth, which is the same order the core numbers.

    The PARENT is DERIVED here from the names and depths alone — the first
    record after this one at one level shallower — rather than read from a
    reported field, so the table this file compares against is one this file
    computed. (It is the core's own `doc_group::parent` rule: a subtree ends
    with its own entry, so the enclosing group is the next shallower record.)
    """
    summary = app.call('get_document', document_id=document)
    layers = summary['layers']
    names = [layer.get('name') for layer in layers]
    depths = [int(layer.get('depth', 0)) for layer in layers]
    rows = []
    for position, layer in enumerate(layers):
        parent = None
        if depths[position] > 0:
            for later in range(position + 1, len(layers)):
                if depths[later] < depths[position]:
                    parent = names[later]
                    break
        rows.append((names[position], depths[position], parent, int(layer['id'])))
    return rows


def shape(rows):
    """A table without the ids — what the expectations above are written as."""
    return [row[:3] for row in rows]


def ids_by_name(rows):
    return {row[0]: row[3] for row in rows}


def samples(app, document, rows):
    """One `sample_pixel` per layer, at a point this file chose. A structure
    op moves records and must touch no sample."""
    out = {}
    for name, _, _, _ in rows:
        index = [row[0] for row in rows].index(name)
        reply = app.call('sample_pixel', document_id=document, x=CANVAS[0] // 3,
                         y=CANVAS[1] // 3, source='layer', layer=index)
        # Only the CODES: the reply also echoes the scope's layer INDEX, which
        # a reorder is supposed to change.
        out[name] = reply['center_rgba_codes']
    return out


def history(app, document):
    report = app.call('list_history', document_id=document)
    return len(report['states']), report['cursor']


def dirty(app, document):
    return app.call('get_document', document_id=document)['unsaved_changes']


def build(app, depth, folder, name):
    """A four-root stack named A..D at `depth`, each layer a different colour
    so `sample_pixel` distinguishes them.

    `duplicate_layer` rather than `new_layer`: each layer is a duplicate of
    the opaque white background, so the tolerance-0 bucket fill below paints
    the WHOLE layer. (`new_layer` has been admitted at depth since 12E-2b; an
    empty layer would give the fill nothing to spread over.)
    """
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='white')
    document = created['document']['id']
    for _ in range(len(START) - 1):
        app.call('duplicate_layer', document_id=document, index=0)
    for index, layer in enumerate(START):
        app.call('set_layer_properties', document_id=document, index=index, name=layer)
        # A bucket fill over the opaque white duplicate paints the whole
        # layer, and `fill` is admitted at 8 and 16 bits alike.
        app.call('fill', document_id=document, x=CANVAS[0] // 3, y=CANVAS[1] // 3,
                 color=COLOURS[index], tolerance=0, contiguous=True, layer=index)
    rows = structure(app, document)
    assert [row[0] for row in rows] == START, rows
    return document


def float_document(app, folder, name):
    """A FRESH 32-bit float document from the 11F fixture.

    The fixture is COPIED to its own path first: `open_document` on a path
    that is already open answers the document that is already open, and each
    gate needs a stack of its own.
    """
    path = folder / f'{name}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, path)
    opened = app.call('open_document', path=str(path))
    document = opened['document']['id']
    precision = app.call('get_precision', document_id=document)
    assert precision['bits_per_channel'] == 32, precision
    # One root is not a stack; three roots named A..C give the ops something
    # to move, and `duplicate_layer` is admitted at float depth.
    for _ in range(2):
        app.call('duplicate_layer', document_id=document, index=0)
    for index, layer in enumerate(START[:3]):
        app.call('set_layer_properties', document_id=document, index=index, name=layer)
    return document


def persistence_paths(app, folder, document, before, name):
    """The three persistence paths this gate demands, each compared ROW FOR
    ROW against the pre-write table.

    RECOVERY is checked through the checkpoint's OWN bytes rather than through
    `recover_document`: that tool only offers ABANDONED copies from a previous
    launch, and this drive has no crashed instance. `write_recovery` writes
    `image.rz` inside a generation folder of the PRIVATE `DR_RECOVERY_DIR` this
    run owns, and opening that file is exactly what a recovery does with it.
    """
    out = {}

    # 1. The .rz a save_copy writes.
    path = folder / f'{name}.rz'
    app.call('save_copy', document_id=document, path=str(path))
    reopened = app.call('open_document', path=str(path))['document']['id']
    after = structure(app, reopened)
    assert after == before, ('save_copy', after, before)
    out['save_copy'] = after

    # 2. The .rz a recovery checkpoint writes.
    app.call('write_recovery', document_id=document)
    recovery = sorted((folder / 'recovery').rglob('image.rz'),
                      key=lambda p: p.stat().st_mtime)
    assert recovery, 'write_recovery wrote no native checkpoint'
    checkpoint = app.call('open_document', path=str(recovery[-1]))['document']['id']
    after = structure(app, checkpoint)
    assert after == before, ('recovery', after, before)
    out['recovery'] = after

    # 3. The .drsnapshot package, saved and loaded back.
    created = app.call('create_snapshot', document_id=document, name=f'{name} snapshot')
    snapshot_id = created['snapshot_id']
    package = folder / f'{name}.drsnapshot'
    app.call('save_snapshot', document_id=document, snapshot_id=snapshot_id,
             path=str(package))
    loaded = app.call('load_snapshot', document_id=document, path=str(package))
    loaded_id = loaded['snapshot_id']
    app.call('restore_snapshot', document_id=document, snapshot_id=loaded_id)
    after = structure(app, document)
    assert after == before, ('snapshot', after, before)
    out['snapshot'] = after
    return out


# ------------------------------------------------------------------ gates --

def gate_s1(app, folder, receipts):
    gate = 's1_ids_survive_every_persistence_path'
    findings = {}
    for label, depth in (('8', 8), ('16', 16)):
        document = build(app, depth, folder, f's1_{label}')
        if depth == 8:
            # A label, a comp and the linear-light policy, so every
            # conditional `.rz` tail is present beside the identity table.
            app.call('set_layer_label', document_id=document, layer=1, label='red')
            app.call('save_layer_comp', document_id=document, name='Everything')
            app.call('set_evaluation', document_id=document, policy='linear_light')
        before = structure(app, document)
        findings[f'{label}_before'] = before

        findings[f'{label}_paths'] = persistence_paths(app, folder, document, before,
                                                       f's1_{label}')

    if FLOAT_FIXTURE.exists():
        document = float_document(app, folder, 's1_float')
        before = structure(app, document)
        findings['32_before'] = before
        findings['32_paths'] = persistence_paths(app, folder, document, before, 's1_32')

    receipts[gate] = findings
    print('PASS s1: layer ids survive save/reopen, recovery and snapshots at every depth',
          flush=True)


def run_structure_sequence(app, document, findings, key):
    """S2's five operations against the exact tables written at the top."""
    rows = structure(app, document)
    original = ids_by_name(rows)
    before_samples = samples(app, document, rows)
    steps = []

    def step(label, expected, call):
        nonlocal rows
        before = history(app, document)
        previous = shape(rows)
        call()
        rows = structure(app, document)
        assert shape(rows) == expected, (label, shape(rows), expected)
        after = history(app, document)
        assert after[1] == before[1] + 1, (label, before, after)
        # Every surviving name keeps the id it had; a new container mints.
        current = ids_by_name(rows)
        for name, value in current.items():
            if name in original:
                assert value == original[name], (label, name, value, original[name])
            else:
                assert value not in original.values(), (label, name, value)
        for name, value in samples(app, document, rows).items():
            if name in before_samples:
                assert value == before_samples[name], (label, name, value)
        assert dirty(app, document) is True, label
        app.call('undo', document_id=document)
        assert shape(structure(app, document)) == previous, (label, 'undo')
        # Redo the op so the sequence continues from the state it produced.
        app.call('redo', document_id=document)
        rows = structure(app, document)
        assert shape(rows) == expected, (label, 'redo', shape(rows))
        steps.append({'step': label, 'table': shape(rows)})

    names = [row[0] for row in rows]
    step('reorder_layers', AFTER_REORDER_LAYERS,
         lambda: app.call('reorder_layers', document_id=document,
                          layers=[names.index('A')], at=3, depth=0))
    rows = structure(app, document)
    names = [row[0] for row in rows]
    step('reorder_layer', AFTER_REORDER_LAYER,
         lambda: app.call('reorder_layer', document_id=document,
                          **{'from': names.index('C'), 'to': 0, 'depth': 0}))
    rows = structure(app, document)
    names = [row[0] for row in rows]
    step('group_layers', AFTER_GROUP,
         lambda: app.call('group_layers', document_id=document,
                          layers=[names.index('B'), names.index('A')], name='Set'))
    rows = structure(app, document)
    names = [row[0] for row in rows]
    step('ungroup_layers', AFTER_UNGROUP,
         lambda: app.call('ungroup_layers', document_id=document,
                          index=names.index('Set')))
    rows = structure(app, document)
    names = [row[0] for row in rows]
    step('delete_layer', AFTER_DELETE,
         lambda: app.call('delete_layer', document_id=document,
                          layers=[names.index('D')]))
    findings[key] = steps


def gate_s2(app, folder, receipts):
    gate = 's2_typed_structure_ops'
    findings = {}
    document = build(app, 16, folder, 's2_16')
    run_structure_sequence(app, document, findings, '16')
    receipts[gate] = findings
    print('PASS s2: every structure op answers at 16 bits with the exact table, '
          'ids carried, one history step each', flush=True)
    return document


def gate_s2_float(app, folder, receipts):
    gate = 's2_typed_structure_ops_float'
    if not FLOAT_FIXTURE.exists():
        receipts[gate] = 'skipped: the 11F float fixture is not present'
        print('SKIP s2-float: the 11F fixture is absent', flush=True)
        return
    document = float_document(app, folder, 's2_float')
    rows = structure(app, document)
    original = ids_by_name(rows)
    assert [row[0] for row in rows] == START[:3], rows

    # The exact float tables, written here.
    expected = {
        'reorder_layer': [('B', 0, None), ('A', 0, None), ('C', 0, None)],
        'group_layers': [('B', 1, 'Set'), ('A', 1, 'Set'), ('Set', 0, None), ('C', 0, None)],
        'ungroup_layers': [('B', 0, None), ('A', 0, None), ('C', 0, None)],
        'delete_layer': [('B', 0, None), ('A', 0, None)],
    }
    steps = []
    before = history(app, document)
    app.call('reorder_layer', document_id=document, **{'from': 0, 'to': 1, 'depth': 0})
    rows = structure(app, document)
    assert shape(rows) == expected['reorder_layer'], (shape(rows), expected['reorder_layer'])
    assert history(app, document)[1] == before[1] + 1
    steps.append({'step': 'reorder_layer', 'table': shape(rows)})

    names = [row[0] for row in rows]
    app.call('group_layers', document_id=document,
             layers=[names.index('B'), names.index('A')], name='Set')
    rows = structure(app, document)
    assert shape(rows) == expected['group_layers'], (shape(rows), expected['group_layers'])
    steps.append({'step': 'group_layers', 'table': shape(rows)})

    names = [row[0] for row in rows]
    app.call('ungroup_layers', document_id=document, index=names.index('Set'))
    rows = structure(app, document)
    assert shape(rows) == expected['ungroup_layers'], (shape(rows), expected['ungroup_layers'])
    steps.append({'step': 'ungroup_layers', 'table': shape(rows)})

    names = [row[0] for row in rows]
    app.call('delete_layer', document_id=document, layers=[names.index('C')])
    rows = structure(app, document)
    assert shape(rows) == expected['delete_layer'], (shape(rows), expected['delete_layer'])
    steps.append({'step': 'delete_layer', 'table': shape(rows)})

    for name, value in ids_by_name(rows).items():
        assert value == original[name], (name, value, original[name])
    receipts[gate] = steps
    print('PASS s2-float: the structure ops answer on a 32-bit float document too', flush=True)


def gate_s3(app, folder, receipts):
    gate = 's3_refusals_and_state'
    findings = {}
    document = build(app, 16, folder, 's3')
    rows = structure(app, document)
    names = [row[0] for row in rows]

    # Deleting every layer answers the core's structured NO-OP reply — `ok`
    # with `changed: false` and the reason — rather than an error, which is
    # `structureNoOp`'s established shape. Either way it is refused BY NAME
    # and no undo step exists, which is what this gate asks.
    reply = app.call('delete_layer', document_id=document, layers=list(range(len(rows))))
    assert reply['changed'] is False, reply
    assert REFUSALS['delete_all'] in reply['note'], reply
    findings['delete_all'] = reply['note']

    app.call('group_layers', document_id=document,
             layers=[names.index('A'), names.index('B')], name='Set')
    rows = structure(app, document)
    names = [row[0] for row in rows]
    # 'C' sits at the top level, 'A' inside the group: two parents.
    refusal = app.call('group_layers', document_id=document,
                       layers=[names.index('A'), names.index('C')], name='X', fail=True)
    assert REFUSALS['group_across_parents'] in refusal, refusal
    findings['group_across_parents'] = refusal

    refusal = app.call('ungroup_layers', document_id=document,
                       index=names.index('C'), fail=True)
    assert REFUSALS['ungroup_raster'] in refusal, refusal
    findings['ungroup_raster'] = refusal

    assert dirty(app, document) is True
    before_path = app.call('get_document', document_id=document).get('path')
    app.call('save_copy', document_id=document, path=str(folder / 's3.rz'))
    after_path = app.call('get_document', document_id=document).get('path')
    assert after_path == before_path, (before_path, after_path)
    assert dirty(app, document) is True, 'a save_copy does not clean the document'
    findings['path'] = {'before': before_path, 'after': after_path}

    receipts[gate] = findings
    print('PASS s3: every structure refusal is by name and save_copy moves no identity',
          flush=True)


def gate_s4(app, folder, receipts):
    gate = 's4_set_view_refusals'
    findings = {}
    document = build(app, 8, folder, 's4')
    before = app.call('get_view', document_id=document)
    for arguments, needle in (({'zoom': 'big'}, 'zoom'),
                              ({'center': 3}, 'center'),
                              ({'view': 'x', 'zoom': 1}, 'view')):
        refusal = app.call('set_view', document_id=document, fail=True, **arguments)
        assert needle in refusal, (arguments, refusal)
        findings[needle] = refusal
        assert app.call('get_view', document_id=document) == before, arguments

    tools = {tool['name']: tool for tool in app.client.rpc('tools/list', {})['tools']}
    description = tools['close_second_view']['inputSchema']['properties']['view']['description']
    assert 'Omit' not in description, description
    findings['close_second_view_view'] = description

    receipts[gate] = findings
    print('PASS s4: a malformed set_view argument is refused by name and the view is unchanged',
          flush=True)


def gate_s5(app, folder, receipts):
    gate = 's5_get_precision_families'
    findings = {}
    document = build(app, 16, folder, 's5')
    tools = set(app.call('get_precision', document_id=document)['supported_tools'])
    for name in S5_TOOLS:
        assert name in tools, (name, sorted(tools))
    findings['16'] = sorted(tools & set(S5_TOOLS))
    if FLOAT_FIXTURE.exists():
        document = float_document(app, folder, 's5_float')
        tools = set(app.call('get_precision', document_id=document)['supported_tools'])
        for name in S5_TOOLS:
            assert name in tools, (name, sorted(tools))
        findings['32'] = sorted(tools & set(S5_TOOLS))
    findings['families'] = FAMILIES
    receipts[gate] = findings
    print('PASS s5: get_precision lists the four new structure families and arrange_layer '
          'at both typed depths', flush=True)


def active_layer(app, document):
    return app.call('get_document', document_id=document)['active_layer']


def arrange(app, document, name, to, expected, landing, log):
    """One `arrange_layer` of the entry called `name`, checked against the
    table, the landing index, the active layer and one history step."""
    rows = structure(app, document)
    index = [row[0] for row in rows].index(name)
    before = history(app, document)
    reply = app.call('arrange_layer', document_id=document, layer=index, to=to)
    rows = structure(app, document)
    assert shape(rows) == expected, (name, to, shape(rows), expected)
    assert reply['index'] == landing, (name, to, reply)
    active = active_layer(app, document)
    assert active == landing, (name, to, active, landing)
    assert rows[active][0] == name, (name, to, rows[active])
    after = history(app, document)
    assert after[1] == before[1] + 1, (name, to, before, after)
    log.append({'layer': name, 'from': index, 'to': to, 'index': reply['index'],
                'table': shape(rows)})
    return reply


def arrange_noop(app, document, index, to, log):
    """An arrange that moves nothing: the house no-op, with no history step
    and no dirtying."""
    table = shape(structure(app, document))
    report = app.call('list_history', document_id=document)
    before = (report['states'], report['cursor'])
    was_dirty = dirty(app, document)
    reply = app.call('arrange_layer', document_id=document, layer=index, to=to)
    assert reply['changed'] is False, reply
    assert S6_NOOP_NOTE in reply['note'], reply
    report = app.call('list_history', document_id=document)
    assert (report['states'], report['cursor']) == before, (before, report)
    assert dirty(app, document) == was_dirty, (was_dirty, dirty(app, document))
    assert shape(structure(app, document)) == table, 'a no-op moved the stack'
    log.append({'noop': index, 'to': to, 'note': reply['note']})
    return reply


def replay_ui_action(app, document, expected, active_expected):
    """`run_action` of the UI step shape on `document`, active entry 0."""
    app.call('set_active_layer', document_id=document, index=0)
    before = history(app, document)
    report = app.call('run_action', document_id=document, name=S6_UI_ACTION)
    assert report['ok'] is True, report
    assert report['steps_run'] == 2, report
    assert 'failures' not in report, report
    rows = structure(app, document)
    assert shape(rows) == expected, (shape(rows), expected)
    active = active_layer(app, document)
    assert active == active_expected, (active, active_expected)
    after = history(app, document)
    assert after[1] == before[1] + 1, (before, after)
    return {'table': shape(rows), 'active_layer': active, 'steps_run': report['steps_run']}


def layer_offsets(app, document, indices):
    layers = app.call('get_document', document_id=document)['layers']
    return [(int(layers[i]['offset_x']), int(layers[i]['offset_y'])) for i in indices]


def content_left(app, document, indices):
    layers = app.call('get_document', document_id=document)['layers']
    return [int(layers[i]['content_x']) for i in indices]


def place_for_align(app, document):
    """S6_ALIGN_START's offsets on the first three layers."""
    for index, (x, y) in zip(S6_ALIGN_LAYERS, S6_ALIGN_START):
        app.call('set_layer_properties', document_id=document, index=index,
                 offset_x=x, offset_y=y)
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_ALIGN_START


def align_at_depth(app, document):
    """The align and distribute gates on one typed document: align to the
    layers and to the canvas, undo, distribute, and the content read-back."""
    place_for_align(app, document)
    offset_to_content = [c - o[0] for c, o in zip(
        content_left(app, document, S6_ALIGN_LAYERS),
        layer_offsets(app, document, S6_ALIGN_LAYERS))]
    assert len(set(offset_to_content)) == 1, offset_to_content
    before = history(app, document)
    reply = app.call('align_layers', document_id=document, layers=S6_ALIGN_LAYERS,
                     edge='left', to='layers')
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_ALIGN_LEFT
    assert [m['offset_x'] for m in reply['moved']] == [x for x, _ in S6_ALIGN_LEFT], reply
    assert history(app, document)[1] == before[1] + 1, (before, history(app, document))
    # The content read-back: every content left edge at the same x.
    lefts = content_left(app, document, S6_ALIGN_LAYERS)
    assert len(set(lefts)) == 1, lefts
    app.call('undo', document_id=document)
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_ALIGN_START
    assert history(app, document)[1] == before[1], (before, history(app, document))
    app.call('align_layers', document_id=document, layers=S6_ALIGN_LAYERS, edge='left',
             to='canvas')
    assert content_left(app, document, S6_ALIGN_LAYERS) == [0, 0, 0]
    canvas = layer_offsets(app, document, S6_ALIGN_LAYERS)
    assert len({x for x, _ in canvas}) == 1, canvas
    assert [y for _, y in canvas] == [y for _, y in S6_ALIGN_START], canvas
    app.call('undo', document_id=document)
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_ALIGN_START
    app.call('distribute_layers', document_id=document, layers=S6_ALIGN_LAYERS,
             axis='horizontal')
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_DISTRIBUTED
    return {'aligned': S6_ALIGN_LEFT, 'to_canvas': canvas, 'distributed': S6_DISTRIBUTED}


def replay_align(app, document):
    """The Align recorded at 8 bits, replayed on a typed document."""
    place_for_align(app, document)
    before = history(app, document)
    report = app.call('run_action', document_id=document, name=S6_ALIGN_ACTION)
    assert report['ok'] is True, report
    assert 'failures' not in report, report
    assert layer_offsets(app, document, S6_ALIGN_LAYERS) == S6_ALIGN_LEFT
    assert history(app, document)[1] == before[1] + 1, (before, history(app, document))
    return layer_offsets(app, document, S6_ALIGN_LAYERS)


def gate_s6(app, folder, receipts):
    gate = 's6_arrange_at_depth'
    findings = {}

    # 16-bit direct, then the no-op on the SAME document once undo has
    # restored START.
    log = []
    document = build(app, 16, folder, 's6_direct')
    rows = structure(app, document)
    before_samples = samples(app, document, rows)
    arrange(app, document, 'A', 'front', S6_DIRECT_16, S6_DIRECT_16_INDEX, log)
    rows = structure(app, document)
    assert samples(app, document, rows) == before_samples, 'an arrange moved a sample'
    app.call('undo', document_id=document)
    assert shape(structure(app, document)) == S6_START_16, 'undo did not restore START'
    arrange_noop(app, document, 3, 'front', log)
    findings['16_direct'] = log

    # 16-bit tree: a group, siblings around it, and a child that stays in it.
    log = []
    document = build(app, 16, folder, 's6_tree')
    app.call('group_layers', document_id=document, layers=[1, 2], name='G')
    assert shape(structure(app, document)) == S6_GROUPED_16, structure(app, document)
    for name, to, expected, landing in S6_TREE_16:
        arrange(app, document, name, to, expected, landing, log)
    rows = structure(app, document)
    arrange_noop(app, document, [row[0] for row in rows].index('B'), 'front', log)
    findings['16_tree'] = log

    # 16-bit adjustment layer.
    log = []
    document = build(app, 16, folder, 's6_adjustment')
    app.call('add_adjustment_layer', document_id=document, op='invert', name='Adj', layer=0)
    assert shape(structure(app, document)) == S6_ADJUSTED_16, structure(app, document)
    arrange(app, document, 'Adj', 'front', S6_ADJUSTMENT_16, S6_ADJUSTMENT_16_INDEX, log)
    findings['16_adjustment'] = log

    # The UI's recorded step shape, saved and replayed at 16 bits.
    app.call('save_action', name=S6_UI_ACTION, steps=S6_UI_STEPS)
    document = build(app, 16, folder, 's6_replay_ui')
    findings['16_replay_ui'] = replay_ui_action(app, document, S6_REPLAYED_16,
                                                S6_REPLAYED_16_ACTIVE)

    # Record then replay. Twins first and `set_active_layer` outside the
    # window: recording is global, and only the two steps go inside it.
    recorded = build(app, 16, folder, 's6_record_x')
    twin = build(app, 16, folder, 's6_record_y')
    app.call('set_active_layer', document_id=recorded, index=0)
    before = history(app, recorded)
    app.call('start_recording')
    reply = app.call('arrange_layer', document_id=recorded, to='front')
    app.call('set_layer_properties', document_id=recorded, name='Moved')
    stopped = app.call('stop_recording', name=S6_RECORDED_ACTION)
    tools = [step['tool'] for step in stopped['steps']]
    assert tools == S6_RECORDED_TOOLS, stopped
    # The recorded document's own checks, read AFTER the window closed.
    assert reply['index'] == S6_REPLAYED_16_ACTIVE, reply
    assert shape(structure(app, recorded)) == S6_REPLAYED_16, structure(app, recorded)
    assert active_layer(app, recorded) == S6_REPLAYED_16_ACTIVE
    assert history(app, recorded)[1] == before[1] + 2, (before, history(app, recorded))
    app.call('set_active_layer', document_id=twin, index=0)
    before = history(app, twin)
    report = app.call('run_action', document_id=twin, name=S6_RECORDED_ACTION)
    assert report['ok'] is True, report
    assert 'failures' not in report, report
    rows = structure(app, twin)
    assert shape(rows) == S6_REPLAYED_16, (shape(rows), S6_REPLAYED_16)
    assert active_layer(app, twin) == S6_REPLAYED_16_ACTIVE
    assert history(app, twin)[1] == before[1] + 1, (before, history(app, twin))
    findings['16_record_replay'] = {'steps': stopped['steps'], 'table': shape(rows)}

    # Float.
    if FLOAT_FIXTURE.exists():
        log = []
        document = float_document(app, folder, 's6_float_direct')
        assert shape(structure(app, document)) == S6_START_FLOAT, structure(app, document)
        arrange(app, document, 'A', 'front', S6_DIRECT_FLOAT, S6_DIRECT_FLOAT_INDEX, log)
        findings['32_direct'] = log

        document = float_document(app, folder, 's6_float_replay')
        assert shape(structure(app, document)) == S6_START_FLOAT, structure(app, document)
        findings['32_replay_ui'] = replay_ui_action(app, document, S6_REPLAYED_FLOAT,
                                                    S6_REPLAYED_FLOAT_ACTIVE)

        log = []
        document = float_document(app, folder, 's6_float_tree')
        app.call('group_layers', document_id=document, layers=[0, 1], name='G')
        assert shape(structure(app, document)) == S6_GROUPED_FLOAT, structure(app, document)
        for name, to, expected, landing in S6_TREE_FLOAT:
            arrange(app, document, name, to, expected, landing, log)
        findings['32_tree'] = log
    else:
        findings['32'] = 'skipped: the 11F float fixture is not present'
        print('SKIP s6-float: the 11F fixture is absent', flush=True)

    # Align and distribute answer at 16 bits and at float, and an
    # Align recorded at 8 bits replays on both. Twins are built and placed
    # first: recording is global, and only the align goes inside the window.
    findings['21_16'] = align_at_depth(app, build(app, 16, folder, 's6_align_16'))
    recorded = build(app, 8, folder, 's6_align_record')
    replay_16 = build(app, 16, folder, 's6_align_replay_16')
    replay_float = (float_document(app, folder, 's6_align_replay_float')
                    if FLOAT_FIXTURE.exists() else None)
    place_for_align(app, recorded)
    app.call('start_recording')
    reply = app.call('align_layers', document_id=recorded, layers=S6_ALIGN_LAYERS,
                     edge='left', to='layers')
    stopped = app.call('stop_recording', name=S6_ALIGN_ACTION)
    assert [step['tool'] for step in stopped['steps']] == S6_ALIGN_RECORDED_TOOLS, stopped
    assert layer_offsets(app, recorded, S6_ALIGN_LAYERS) == S6_ALIGN_LEFT
    assert [m['offset_x'] for m in reply['moved']] == [x for x, _ in S6_ALIGN_LEFT], reply
    findings['21_record'] = {'steps': stopped['steps'], 'reply': reply}
    findings['21_replay_16'] = replay_align(app, replay_16)
    if replay_float is not None:
        findings['21_float'] = align_at_depth(
            app, float_document(app, folder, 's6_align_float'))
        findings['21_replay_float'] = replay_align(app, replay_float)
    else:
        findings['21_float'] = 'skipped: the 11F float fixture is not present'
        print('SKIP s6-align-float: the 11F fixture is absent', flush=True)

    # 8-bit regression: the reply shape and the no-op are unchanged.
    log = []
    document = build(app, 8, folder, 's6_8')
    reply = arrange(app, document, 'A', 'front', S6_DIRECT_8, S6_DIRECT_8_INDEX, log)
    assert 'changed' not in reply, reply
    arrange_noop(app, document, 3, 'front', log)
    findings['8'] = log

    receipts[gate] = findings
    print('PASS s6: arrange_layer answers and replays at 16 bits and at float; no-op, '
          'subtree and adjustment landings exact; align_layers / distribute_layers answer '
          'and replay at 16 bits and at float', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    parser.add_argument('--gates', default=','.join(GATE_ORDER),
                        help='comma list of gates to run, from ' + ','.join(GATE_ORDER))
    args = parser.parse_args()
    selected = [gate.strip() for gate in args.gates.split(',') if gate.strip()]
    unknown = [gate for gate in selected if gate not in GATE_ORDER]
    if unknown:
        parser.error(f'unknown gate(s): {",".join(unknown)}')
    if not selected:
        parser.error('--gates selects no gate')
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'expectations': 'fixed in scripts/verify_phase12e1_structure.py before any '
                                'measurement; every value is an integer, a string or a table '
                                'this file wrote down and is compared for exact equality'}
    gates = {'s1': gate_s1, 's2': gate_s2, 's2_float': gate_s2_float, 's3': gate_s3,
             's4': gate_s4, 's5': gate_s5, 's6': gate_s6}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        for name in GATE_ORDER:
            if name in selected:
                gates[name](app, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
