#!/usr/bin/env python3
"""The Phase 12D-1 LAYER COMP MCP drive, on a built Release app.

Nine gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no tolerance is widened afterwards. The instance is owned by this
file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads) and is terminated by its own context
manager; no Doctored process this file did not start is ever signalled.

- **C1 a comp restores a state exactly, and one Undo walks it back.** A
  three-layer 8-bit document is put into state SA by `set_layer_properties`
  (visibility, opacity, offset) over `fill`ed layers; the `render` PNG's
  SHA-256 is DA. `save_layer_comp A`. The document is perturbed to SB — one
  layer hidden, one moved by (40, 30), one at opacity 0.5 — digest DB,
  `save_layer_comp B`. `apply_layer_comp A` puts the digest back to DA AND
  makes `get_document`'s rows equal the SA table this file wrote down; one
  `undo` puts the digest back to DB and `list_history`'s cursor back by one;
  `apply_layer_comp B` gives DB again.
- **C2 the .rz carries them at both depths.** `save_copy` to `.rz` and
  `open_document`: the reopened 8-bit document lists both comps ENTRY FOR
  ENTRY against the pre-save listing, and `apply_layer_comp A` on it gives DA.
  The same save/reopen/list on a 16-bit document (`new_document depth:16`).
- **C3 a deleted layer is a stale entry, by name.** `delete_layer` makes
  exactly that entry `stale: true` under the name it was recorded with;
  `apply_layer_comp` reports it in `stale` and still applies the rest; and a
  `save_copy` + `open_document` still lists it stale.
- **C5 a padded name is accepted and answered under its trimmed form.** The
  core trims a comp name before it stores it, so `save_layer_comp` with a
  trailing space must still succeed, answer with the trimmed name and list
  under it — never report a failure for a mutation that happened. The same
  for `update_layer_comp`'s padded `new_name`.
- **C4 an apply replays.** `start_recording` / `apply_layer_comp` /
  `stop_recording` / `save_action`, replayed with `run_action` on a twin that
  was put into SB, reaches DA.
- **C6 a comp moves to its FINAL index, at every depth, and replays.**
  Comps A (SA), B and C (SB) on an 8-bit document: `reorder_layer_comp` A to
  2 answers and lists `['B', 'C', 'A']` (indices 0, 1, 2) in exactly one
  history step; `undo` / `redo` toggle the two orders; C to 1 (where it
  already is) is `changed: false` with the history untouched; `to` 3, -1 and
  a missing `to` fail in band with the exact sentences; `apply_layer_comp A`
  still gives SA; a `.rz` round trip keeps the order at 8 and 16 bits; a
  recorded reorder is one `reorder_layer_comp {name, to}` step whose replay
  on an `[A, B, C]` twin gives `[B, C, A]`; and a FLOAT document reorders too.
- **C7 `list_layer_comps` reports what each entry RECORDED.** Keys in
  `get_document`'s vocabulary, present iff captured; the recorded style is
  `get_document`'s style object; a non-dyadic opacity (0.3) is spelled `0.3`
  in the reply's literal JSON; a visibility-only comp carries `visible` and
  nothing else; and at 16 bits the recorded values equal the layer rows.
- **C8 apply hides a layer the comp has no record of, groups exempt.**
  At 8 and 16 bits: `save_layer_comp A`, then `duplicate_layer` of a recorded
  layer and `group_layers` over two recorded layers. `list_layer_comps` names
  the copy, and only the copy, in A's `unrecorded`; `apply_layer_comp A` hides
  the copy (`get_document`), leaves the new group visible and names the copy
  in `hidden_unrecorded`. Showing the copy and `update_layer_comp A` (recapture
  by default) records it: `unrecorded` is empty and a second apply leaves the
  copy visible.
- **C9 comps follow host-regenerated artwork through whole-canvas geometry.**
  At 8 and 16 bits, on a fresh 200x150 document per op: a 24 px
  `add_text_layer` and a 40x30 `add_shape_layer` rect, `save_layer_comp A`,
  then ONE of `image_size` 260x195, `rotate` 17 degrees, or `crop` with a
  5 degree angle. Afterwards every text/shape entry's recorded `offset` in
  `list_layer_comps` equals that layer's offset in `get_document`, and
  `apply_layer_comp A` answers `changed: false` (nothing would move). The
  expected offsets are read from `get_document`, never from comp code.

Nothing here derives an expectation from Doctored: the sizes, the colours,
the property tables, the digests and the entry sets are computed or recorded
in this file and compared against later readings.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
# There are none. Every number below is an integer or an exact binary fraction
# this file chose, or a SHA-256 of bytes the app is asked to reproduce exactly.
CANVAS = (240, 180)
# Three OVERLAPPING coloured rectangles on three transparent layers, so every
# layer is visible in the composite and hiding, moving or fading any one of
# them changes the render. The rectangles and the colours are this file's.
LAYERS = [
    ('Base', '#2050a0', (10, 10, 150, 120)),
    ('Middle', '#c06020', (60, 40, 150, 120)),
    ('Top', '#30a060', (110, 20, 110, 140)),
]
# State SA: the three layers as this file wants them before comp A is saved.
SA = [
    {'name': 'Base', 'visible': True, 'opacity': 1.0, 'offset_x': 0, 'offset_y': 0},
    {'name': 'Middle', 'visible': True, 'opacity': 0.75, 'offset_x': 12, 'offset_y': 8},
    {'name': 'Top', 'visible': True, 'opacity': 1.0, 'offset_x': 0, 'offset_y': 0},
]
# State SB: hide one, move one by (40, 30) from SA, halve another's opacity.
SB = [
    {'name': 'Base', 'visible': True, 'opacity': 1.0, 'offset_x': 0, 'offset_y': 0},
    {'name': 'Middle', 'visible': True, 'opacity': 0.5, 'offset_x': 52, 'offset_y': 38},
    {'name': 'Top', 'visible': False, 'opacity': 1.0, 'offset_x': 0, 'offset_y': 0},
]
# The 16-bit pair moves no OFFSET. A typed offset change is a nearest-filter
# translate of the raster (`DoctoredCore.withLayerOffset`), not the bookkeeping
# write the byte path makes, and C2's question is the `.rz` carriage and the
# apply — not the typed Move, which is 12F's. Visibility and opacity alone
# still make SA and SB render differently, which is all this gate needs.
SA16 = [dict(row, offset_x=0, offset_y=0) for row in SA]
SB16 = [dict(row, offset_x=0, offset_y=0) for row in SB]
PROPERTIES = ('name', 'visible', 'opacity', 'offset_x', 'offset_y')


def render_digest(app, document):
    """The SHA-256 of `render`'s PNG bytes. The raw RPC, because the shared
    client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def rows(app, document):
    """The document's layer rows reduced to the five properties a comp can
    move, in bottom-first order.

    A 16-bit `get_document` hands back the CORE's typed info rows, which spell
    the offset as one `offset: [x, y]` pair rather than as `offset_x` /
    `offset_y`; both are normalized here so one table describes both depths.
    """
    summary = app.call('get_document', document_id=document)
    out = []
    for layer in summary['layers']:
        offset = layer.get('offset')
        out.append({
            'name': layer['name'],
            'visible': layer['visible'],
            'opacity': layer['opacity'],
            'offset_x': layer['offset_x'] if 'offset_x' in layer else offset[0],
            'offset_y': layer['offset_y'] if 'offset_y' in layer else offset[1],
        })
    return out


def history(app, document):
    """(number of states, cursor). An Undo moves the CURSOR; the state list
    keeps its redo tail, which is why both numbers are read."""
    report = app.call('list_history', document_id=document)
    return len(report['states']), report['cursor']


def apply_state(app, document, table):
    """Puts the document into `table`, one `set_layer_properties` per row."""
    for index, wanted in enumerate(table):
        app.call('set_layer_properties', document_id=document, index=index,
                 **{key: value for key, value in wanted.items() if key != 'name'})


def build(app, folder, name, depth, table=None):
    """A three-layer document of overlapping coloured rectangles at `depth`,
    in state `table` (SA by default).

    Every call here is admitted at BOTH depths: `duplicate_layer` rather than
    `new_layer` (16-bit has no new-layer op), a rectangle selection, and a
    bucket fill, which on a wholly transparent layer paints exactly the
    selected rectangle.
    """
    table = table if table is not None else SA
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='transparent')
    document = created['document']['id']
    for _ in range(len(LAYERS) - 1):
        app.call('duplicate_layer', document_id=document, index=0)
    assert len(rows(app, document)) == len(LAYERS), rows(app, document)
    for index, (layer, colour, rect) in enumerate(LAYERS):
        app.call('set_layer_properties', document_id=document, index=index, name=layer)
        x, y, width, height = rect
        app.call('select_rect', document_id=document, x=x, y=y, width=width, height=height)
        app.call('fill', document_id=document, x=x + 1, y=y + 1, color=colour,
                 tolerance=0, contiguous=True, layer=index)
    app.call('deselect', document_id=document)
    apply_state(app, document, table)
    assert rows(app, document) == table, rows(app, document)
    return document


def entry_set(listing):
    """A listing reduced to what must survive a save: comp names, flags and
    each entry's bound index, name and stale/locked flags."""
    return [
        {
            'name': comp['name'], 'comment': comp['comment'], 'captures': comp['captures'],
            'stale_count': comp['stale_count'], 'locked_count': comp['locked_count'],
            'entries': [
                {key: entry[key] for key in ('layer', 'name', 'stale', 'locked')}
                for entry in comp['entries']
            ],
        }
        for comp in listing['comps']
    ]


# ------------------------------------------------------------------ gates --

def gate_c1(app, folder, receipts):
    gate = 'c1_apply_and_undo'
    findings = {}
    document = build(app, folder, 'c1', 8)
    da = render_digest(app, document)

    saved = app.call('save_layer_comp', document_id=document, name='A',
                     comment='everything on')
    assert saved['ok'] and saved['changed'], saved
    assert saved['comp']['name'] == 'A', saved
    assert len(saved['comp']['entries']) == len(SA), saved
    findings['comp_a'] = saved['comp']

    apply_state(app, document, SB)
    assert rows(app, document) == SB, rows(app, document)
    db = render_digest(app, document)
    assert db != da, 'SA and SB must render differently for this gate to mean anything'
    app.call('save_layer_comp', document_id=document, name='B')

    listing = app.call('list_layer_comps', document_id=document)
    assert [comp['name'] for comp in listing['comps']] == ['A', 'B'], listing
    assert listing['count'] == 2, listing
    findings['listing'] = entry_set(listing)

    before_history = history(app, document)
    applied = app.call('apply_layer_comp', document_id=document, name='A')
    assert applied['ok'] and applied['changed'], applied
    assert applied['stale'] == [] and applied['locked'] == [], applied
    assert applied['applied'] == [0, 1, 2], applied
    assert rows(app, document) == SA, rows(app, document)
    assert render_digest(app, document) == da, 'comp A did not restore state A'
    after_history = history(app, document)
    assert after_history == (before_history[0] + 1, before_history[1] + 1), \
        ('apply is exactly one history step', before_history, after_history)
    findings['applied_a'] = applied
    findings['history'] = {'before': before_history, 'after_apply': after_history}

    app.call('undo', document_id=document)
    assert rows(app, document) == SB, rows(app, document)
    assert render_digest(app, document) == db, 'undo did not restore state B'
    assert history(app, document)[1] == before_history[1], 'undo walked back one state'

    back = app.call('apply_layer_comp', document_id=document, name='B')
    assert back['ok'], back
    assert render_digest(app, document) == db, 'comp B did not restore state B'
    # Applying the state the document already holds adds no undo step.
    repeat = app.call('apply_layer_comp', document_id=document, name='B')
    assert repeat['changed'] is False, repeat
    findings['no_op'] = repeat

    receipts[gate] = findings
    print('PASS c1: a comp restores a state exactly and one Undo walks it back', flush=True)
    return document, da, db


def gate_c2(app, folder, document, da, receipts):
    gate = 'c2_rz_carriage'
    findings = {}
    before = entry_set(app.call('list_layer_comps', document_id=document))

    # 8 bits.
    path = folder / 'c2-eight.rz'
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    assert path.is_file(), path
    reopened = app.open(str(path))
    after = entry_set(app.call('list_layer_comps', document_id=reopened))
    assert after == before, (after, before)
    applied = app.call('apply_layer_comp', document_id=reopened, name='A')
    assert applied['ok'], applied
    assert render_digest(app, reopened) == da, 'comp A did not restore DA after reopen'
    findings['eight_bit'] = {'listing': after, 'render_sha256': da}

    # 16 bits: its own document, its own comps, the same save/reopen/list.
    typed = build(app, folder, 'c2-typed', 16, table=SA16)
    typed_da = render_digest(app, typed)
    app.call('save_layer_comp', document_id=typed, name='A', comment='typed')
    apply_state(app, typed, SB16)
    assert render_digest(app, typed) != typed_da, 'SA16 and SB16 must render differently'
    app.call('save_layer_comp', document_id=typed, name='B')
    typed_before = entry_set(app.call('list_layer_comps', document_id=typed))
    assert [comp['name'] for comp in typed_before] == ['A', 'B'], typed_before
    applied = app.call('apply_layer_comp', document_id=typed, name='A')
    assert applied['ok'] and applied['changed'], applied
    assert rows(app, typed) == SA16, rows(app, typed)
    assert render_digest(app, typed) == typed_da, 'the typed comp A did not restore its render'
    apply_state(app, typed, SB16)

    typed_path = folder / 'c2-sixteen.rz'
    app.call('save_copy', document_id=typed, path=str(typed_path), format='rz')
    assert typed_path.is_file(), typed_path
    typed_reopened = app.open(str(typed_path))
    summary = app.call('get_document', document_id=typed_reopened)
    assert summary['precision'] == 'rgba16', summary['precision']
    typed_after = entry_set(app.call('list_layer_comps', document_id=typed_reopened))
    assert typed_after == typed_before, (typed_after, typed_before)
    applied = app.call('apply_layer_comp', document_id=typed_reopened, name='A')
    assert applied['ok'], applied
    assert rows(app, typed_reopened) == SA16, rows(app, typed_reopened)
    assert render_digest(app, typed_reopened) == typed_da, \
        'comp A did not restore the typed render after reopen'
    findings['sixteen_bit'] = {'listing': typed_after, 'render_sha256': typed_da}

    receipts[gate] = findings
    print('PASS c2: both comps survive a .rz round trip at 8 and 16 bits', flush=True)


def gate_c3(app, folder, receipts):
    gate = 'c3_stale_by_name'
    document = build(app, folder, 'c3', 8)
    app.call('save_layer_comp', document_id=document, name='A')
    before = entry_set(app.call('list_layer_comps', document_id=document))
    assert all(not entry['stale'] for entry in before[0]['entries']), before

    app.call('delete_layer', document_id=document, index=1)
    listing = app.call('list_layer_comps', document_id=document)
    comp = listing['comps'][0]
    assert comp['stale_count'] == 1, comp
    stale = [entry for entry in comp['entries'] if entry['stale']]
    assert len(stale) == 1, comp
    assert stale[0]['name'] == 'Middle', stale
    assert stale[0]['layer'] is None, stale
    survivors = [entry for entry in comp['entries'] if not entry['stale']]
    assert [entry['layer'] for entry in survivors] == [0, 1], comp

    # The apply reports it by name and still writes the rest.
    app.call('set_layer_properties', document_id=document, index=1, visible=False)
    applied = app.call('apply_layer_comp', document_id=document, name='A')
    assert applied['ok'] and applied['changed'], applied
    assert applied['stale'] == ['Middle'], applied
    assert applied['applied'] == [0, 1], applied
    assert rows(app, document)[1]['visible'] is True, rows(app, document)

    path = folder / 'c3-stale.rz'
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    reopened = app.open(str(path))
    comp = app.call('list_layer_comps', document_id=reopened)['comps'][0]
    assert comp['stale_count'] == 1, comp
    stale = [entry for entry in comp['entries'] if entry['stale']]
    assert len(stale) == 1 and stale[0]['name'] == 'Middle', comp
    assert stale[0]['layer'] is None, comp

    receipts[gate] = {'before': before, 'applied': applied, 'after_reopen': entry_set(
        app.call('list_layer_comps', document_id=reopened))}
    print('PASS c3: a deleted layer is a stale entry by name, across a save', flush=True)


def gate_c4(app, folder, da, receipts):
    gate = 'c4_recorded_replay'
    document = build(app, folder, 'c4', 8)
    app.call('save_layer_comp', document_id=document, name='A')
    apply_state(app, document, SB)

    app.call('start_recording')
    app.call('apply_layer_comp', document_id=document, name='A')
    action = app.call('stop_recording', name='Apply Hero')
    assert len(action['steps']) == 1, action
    assert action['steps'][0]['tool'] == 'apply_layer_comp', action
    assert action['steps'][0]['arguments']['name'] == 'A', action
    saved = app.call('save_action', name='Apply Hero', steps=action['steps'], overwrite=True)
    assert saved.get('ok', True), saved

    twin = build(app, folder, 'c4-twin', 8)
    twin_da = render_digest(app, twin)
    assert twin_da == da, 'the twin must start from the same picture as C1'
    app.call('save_layer_comp', document_id=twin, name='A')
    apply_state(app, twin, SB)
    assert render_digest(app, twin) != da

    played = app.call('run_action', name='Apply Hero', document_id=twin)
    assert played['ok'], played
    assert rows(app, twin) == SA, rows(app, twin)
    assert render_digest(app, twin) == da, 'the replayed apply did not reach DA'

    receipts[gate] = {'action': action, 'replayed_rows': rows(app, twin), 'render_sha256': da}
    print('PASS c4: a recorded apply_layer_comp replays to the same picture', flush=True)


def gate_c5(app, folder, receipts):
    """A padded name must not be reported as a failure.

    The core trims the name it stores. A handler that reads the new comp back
    under the CALLER's raw string throws after the edit already happened, so
    this gate asks for the trimmed name in the reply AND in the listing.
    """
    gate = 'c5_padded_name'
    document = build(app, folder, 'c5', 8)

    saved = app.call('save_layer_comp', document_id=document, name='Hero ')
    assert saved['ok'] and saved['changed'], saved
    assert saved['comp']['name'] == 'Hero', saved
    listed = [comp['name'] for comp in app.call(
        'list_layer_comps', document_id=document)['comps']]
    assert listed == ['Hero'], listed

    updated = app.call('update_layer_comp', document_id=document, name='Hero',
                       new_name='  Hero Two  ', recapture=False)
    assert updated['ok'] and updated['changed'], updated
    assert updated['comp']['name'] == 'Hero Two', updated
    relisted = [comp['name'] for comp in app.call(
        'list_layer_comps', document_id=document)['comps']]
    assert relisted == ['Hero Two'], relisted

    receipts[gate] = {'saved': saved['comp']['name'], 'listed': listed,
                      'updated': updated['comp']['name'], 'relisted': relisted}
    print('PASS c5: a padded comp name is accepted and answered trimmed', flush=True)


def comp_order(app, document):
    """(names, indices) of the document's comps in list order."""
    listing = app.call('list_layer_comps', document_id=document)
    return ([comp['name'] for comp in listing['comps']],
            [comp['index'] for comp in listing['comps']])


def three_comps(app, folder, name, depth, sa, sb):
    """A document holding comps A (state `sa`), B and C (state `sb`)."""
    document = build(app, folder, name, depth, table=sa)
    app.call('save_layer_comp', document_id=document, name='A')
    apply_state(app, document, sb)
    app.call('save_layer_comp', document_id=document, name='B')
    app.call('save_layer_comp', document_id=document, name='C')
    assert comp_order(app, document) == (['A', 'B', 'C'], [0, 1, 2])
    return document


def gate_c6(app, folder, receipts):
    gate = 'c6_reorder'
    findings = {}
    document = three_comps(app, folder, 'c6', 8, SA, SB)

    # 1. A to its FINAL index 2, in one history step.
    before = history(app, document)
    moved = app.call('reorder_layer_comp', document_id=document, name='A', to=2)
    assert moved['ok'] and moved['changed'], moved
    assert moved['name'] == 'A' and moved['index'] == 2, moved
    assert moved['order'] == ['B', 'C', 'A'], moved
    assert comp_order(app, document) == (['B', 'C', 'A'], [0, 1, 2])
    after = history(app, document)
    assert after == (before[0] + 1, before[1] + 1), ('one history step', before, after)
    findings['moved'] = moved

    # 2. Undo and redo toggle the two orders.
    app.call('undo', document_id=document)
    assert comp_order(app, document)[0] == ['A', 'B', 'C']
    app.call('redo', document_id=document)
    assert comp_order(app, document)[0] == ['B', 'C', 'A']

    # 3. A comp moved to where it already is changes nothing.
    steady = history(app, document)
    same = app.call('reorder_layer_comp', document_id=document, name='C', to=1)
    assert same['ok'] and same['changed'] is False, same
    assert same['order'] == ['B', 'C', 'A'] and same['index'] == 1, same
    assert history(app, document) == steady, 'a no-op reorder added a history step'
    findings['no_op'] = same

    # 4. The refusals, in band, with the exact sentences.
    refusals = {
        'to_3': (dict(to=3), 'layer comp destination index 3 is out of range (0..2)'),
        'to_minus_1': (dict(to=-1), 'layer comp destination index -1 is out of range (0..2)'),
        'to_missing': ({}, 'Missing required argument: to'),
    }
    for key, (arguments, sentence) in refusals.items():
        text = app.call('reorder_layer_comp', fail=True, document_id=document, name='A',
                        **arguments)
        assert text == sentence, (key, text, sentence)
        findings[key] = text
    assert history(app, document) == steady, 'a refused reorder added a history step'

    # 5. A still applies from its new index.
    applied = app.call('apply_layer_comp', document_id=document, name='A')
    assert applied['ok'] and applied['changed'], applied
    assert rows(app, document) == SA, rows(app, document)

    # 6. The order travels in the .rz, at 8 and at 16 bits.
    path = folder / 'c6-eight.rz'
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    reopened = app.open(str(path))
    assert comp_order(app, reopened) == (['B', 'C', 'A'], [0, 1, 2])
    typed = three_comps(app, folder, 'c6-typed', 16, SA16, SB16)
    app.call('reorder_layer_comp', document_id=typed, name='A', to=2)
    typed_path = folder / 'c6-sixteen.rz'
    app.call('save_copy', document_id=typed, path=str(typed_path), format='rz')
    typed_reopened = app.open(str(typed_path))
    assert app.call('get_document', document_id=typed_reopened)['precision'] == 'rgba16'
    assert comp_order(app, typed_reopened) == (['B', 'C', 'A'], [0, 1, 2])

    # 7. A recorded reorder is one step and replays on a twin.
    recorded = three_comps(app, folder, 'c6-record', 8, SA, SB)
    app.call('start_recording')
    app.call('reorder_layer_comp', document_id=recorded, name='A', to=2)
    action = app.call('stop_recording', name='Reorder Comps')
    assert len(action['steps']) == 1, action
    step = action['steps'][0]
    assert step['tool'] == 'reorder_layer_comp', action
    assert step['arguments']['name'] == 'A' and step['arguments']['to'] == 2, action
    app.call('save_action', name='Reorder Comps', steps=action['steps'], overwrite=True)
    twin = three_comps(app, folder, 'c6-twin', 8, SA, SB)
    played = app.call('run_action', name='Reorder Comps', document_id=twin)
    assert played['ok'], played
    assert comp_order(app, twin)[0] == ['B', 'C', 'A']
    findings['action'] = action

    # 8. A FLOAT document reorders too.
    scene = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
    float_document = app.open(str(scene))
    findings['float_precision'] = app.call(
        'get_document', document_id=float_document)['precision']
    app.call('save_layer_comp', document_id=float_document, name='X')
    app.call('save_layer_comp', document_id=float_document, name='Y')
    floated = app.call('reorder_layer_comp', document_id=float_document, name='X', to=1)
    assert floated['changed'] and floated['order'] == ['Y', 'X'], floated
    assert comp_order(app, float_document) == (['Y', 'X'], [0, 1])

    receipts[gate] = findings
    print('PASS c6: a comp moves to its final index at every depth and replays', flush=True)


RECORDED_KEYS = ('visible', 'offset', 'opacity', 'blend_mode', 'style')


def gate_c7(app, folder, receipts):
    gate = 'c7_recorded_state'
    findings = {}
    document = build(app, folder, 'c7', 8)
    styled = app.call('set_layer_style', document_id=document, layer=2,
                      style={'fill_opacity': 0.5})
    assert styled['ok'], styled
    # A NON-dyadic opacity: 0.3 has no exact binary form, so only a reply
    # that spells numbers the way the core wrote them prints `0.3`.
    app.call('set_layer_properties', document_id=document, index=0, opacity=0.3)

    raw = app.client.rpc('tools/call', {'name': 'save_layer_comp', 'arguments': {
        'document_id': document, 'name': 'All'}})
    assert not raw.get('isError'), raw
    text = next(c['text'] for c in raw['content'] if c['type'] == 'text')
    assert '"opacity":0.3' in text, text
    saved = json.loads(text)
    listed = app.call('list_layer_comps', document_id=document)['comps'][0]
    assert saved['comp'] == listed, (saved['comp'], listed)

    base, middle, top = listed['entries']
    assert base['opacity'] == 0.3, base
    for key, value in {'visible': True, 'offset': [12, 8], 'opacity': 0.75,
                       'blend_mode': 'Normal', 'style': None}.items():
        assert middle[key] == value, (key, middle)
    live = app.call('get_document', document_id=document)['layers'][2]
    assert top['style'] == live['style'], (top['style'], live.get('style'))
    assert top['style'] is not None, top
    findings['all'] = listed

    visibility_only = app.call('save_layer_comp', document_id=document, name='VisOnly',
                               position=False, appearance=False)['comp']
    for entry in visibility_only['entries']:
        assert entry['visible'] is True, entry
        assert not any(key in entry for key in RECORDED_KEYS[1:]), entry
    findings['visibility_only'] = visibility_only

    typed = build(app, folder, 'c7-typed', 16, table=SA16)
    typed_comp = app.call('save_layer_comp', document_id=typed, name='All')['comp']
    for entry, row in zip(typed_comp['entries'], rows(app, typed), strict=True):
        assert entry['visible'] == row['visible'], (entry, row)
        assert entry['opacity'] == row['opacity'], (entry, row)
        assert entry['offset'] == [row['offset_x'], row['offset_y']], (entry, row)
    findings['sixteen_bit'] = typed_comp

    receipts[gate] = findings
    print('PASS c7: list_layer_comps reports each entry\'s recorded state', flush=True)


def visibility(app, document):
    """Each layer's visibility by name, from `get_document`."""
    return {layer['name']: layer['visible']
            for layer in app.call('get_document', document_id=document)['layers']}


def gate_c8(app, folder, receipts):
    gate = 'c8_unrecorded_hidden'
    findings = {}
    names = {layer for layer, _, _ in LAYERS}
    for depth, table in ((8, SA), (16, SA16)):
        document = build(app, folder, f'c8-{depth}', depth, table=table)
        app.call('save_layer_comp', document_id=document, name='A')
        app.call('duplicate_layer', document_id=document, index=1)
        fresh = [name for name in visibility(app, document) if name not in names]
        assert len(fresh) == 1, visibility(app, document)
        copy = fresh[0]
        app.call('group_layers', document_id=document, layers=[0, 1], name='NG')
        shown = visibility(app, document)
        assert shown[copy] is True and shown['NG'] is True, shown

        comp = app.call('list_layer_comps', document_id=document)['comps'][0]
        assert [row['name'] for row in comp['unrecorded']] == [copy], comp['unrecorded']
        applied = app.call('apply_layer_comp', document_id=document, name='A')
        assert applied['ok'] and applied['changed'], applied
        assert applied['hidden_unrecorded'] == [copy], applied
        shown = visibility(app, document)
        assert shown[copy] is False, shown
        assert shown['NG'] is True, shown
        assert all(shown[name] for name in names), shown

        index = [layer['name'] for layer in app.call(
            'get_document', document_id=document)['layers']].index(copy)
        app.call('set_layer_properties', document_id=document, index=index, visible=True)
        app.call('update_layer_comp', document_id=document, name='A')
        comp = app.call('list_layer_comps', document_id=document)['comps'][0]
        assert comp['unrecorded'] == [], comp['unrecorded']
        again = app.call('apply_layer_comp', document_id=document, name='A')
        assert again['ok'] and again['hidden_unrecorded'] == [], again
        assert visibility(app, document)[copy] is True, visibility(app, document)
        findings[f'depth_{depth}'] = {'copy': copy, 'applied': {
            key: applied[key] for key in ('changed', 'applied', 'hidden_unrecorded')}}

    receipts[gate] = findings
    print('PASS c8: apply hides a layer the comp has no record of, groups exempt', flush=True)


C9_OPS = (
    ('image_size', {'width': 260, 'height': 195}),
    ('rotate', {'degrees': 17}),
    ('crop', {'x': 10, 'y': 12, 'width': 170, 'height': 120, 'angle': 5}),
)


def gate_c9(app, folder, receipts):
    gate = 'c9_host_artwork_follows'
    findings = {}
    for depth in (8, 16):
        for tool, arguments in C9_OPS:
            created = app.call('new_document', width=200, height=150, depth=depth,
                               background='transparent')
            document = created['document']['id']
            text = app.call('add_text_layer', document_id=document, text='Comp',
                            x=20, y=30, size=24, color='#203040')
            shape = app.call('add_shape_layer', document_id=document, kind='rect',
                             x=100, y=60, w=40, h=30, fill='#c06020')
            described = {text['layer']: 'text', shape['layer']: 'shape'}
            app.call('save_layer_comp', document_id=document, name='A')
            app.call(tool, document_id=document, **arguments)

            live = rows(app, document)
            comp = app.call('list_layer_comps', document_id=document)['comps'][0]
            checked = {}
            for entry in comp['entries']:
                if entry['layer'] not in described:
                    continue
                row = live[entry['layer']]
                want = [row['offset_x'], row['offset_y']]
                assert entry['offset'] == want, (depth, tool, described[entry['layer']],
                                                 entry['offset'], want)
                checked[described[entry['layer']]] = want
            assert sorted(checked) == ['shape', 'text'], (depth, tool, comp)
            applied = app.call('apply_layer_comp', document_id=document, name='A')
            assert applied['ok'] and applied['changed'] is False, (depth, tool, applied)
            assert applied['locked'] == [], (depth, tool, applied)
            layers = app.call('get_document', document_id=document)['layers']
            findings[f'depth_{depth}_{tool}'] = {
                'offsets': checked,
                're_editable': {kind: kind in layers[index]
                                for index, kind in described.items()},
            }
            assert all(findings[f'depth_{depth}_{tool}']['re_editable'].values()), (
                depth, tool, findings[f'depth_{depth}_{tool}'])
    receipts[gate] = findings
    print('PASS c9: comps follow host-regenerated text and shape layers through '
          'whole-canvas geometry', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
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

    receipts = {'tolerances': 'none — every number is an integer or exact binary fraction this '
                              'file chose, or a SHA-256 of bytes the app reproduces exactly'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        document, da, db = gate_c1(app, folder, receipts)
        gate_c2(app, folder, document, da, receipts)
        gate_c3(app, folder, receipts)
        gate_c4(app, folder, da, receipts)
        gate_c5(app, folder, receipts)
        gate_c6(app, folder, receipts)
        gate_c7(app, folder, receipts)
        gate_c8(app, folder, receipts)
        gate_c9(app, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
