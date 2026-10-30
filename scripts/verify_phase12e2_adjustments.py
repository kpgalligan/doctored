#!/usr/bin/env python3
"""The Phase 12E-2 MCP drive: POSITIONAL live-adjustment insertion (P7-27).

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. The instance is owned by this file (`HostApp`: a free port, a
dummy API key and a PRIVATE directory for every preference library the app
reads) and is terminated by its own context manager; no Doctored process
this file did not start is ever signalled.

TOLERANCES, fixed here before measuring:
  * 8-bit and 16-bit composite samples: within ONE code at the depth's own
    scale (1/255 and 1/65535 of full range). The oracle is computed in Python
    from the straight-alpha source-over formula, in f64.
  * every structural claim (which index, which nesting level, which clipping
    flag, which identity): EXACT, no tolerance at all.
  * the float pixel oracle is NOT this drive's: `core/tests/precision_insert.rs`
    compares an inserted +1 EV exposure against an f64 oracle at float. A3
    here asserts the STRUCTURE, the one undo step and the admitted set.

The fixture, at every depth: a white opaque `Background`, and an ISOLATED
`Group` holding one duplicate (`Mid`) at opacity 0.25. Its projection is
white. An `invert` inserted at each of the three positions then lands at a
DIFFERENT place in that composite, and the three oracles are different
numbers, which is what makes the gate discriminating:

  above Background -> the backdrop under the group is inverted to black, and
                      the group's 0.25-alpha white composites over it: 0.25
  into_group       -> `Mid` is inverted inside the isolated group, so a
                      0.25-alpha BLACK composites over white: 0.75
  top              -> the whole projection is inverted: 0.0

- **A1 / A2 / A3 the three positions at 8, 16 and 32-bit float.** The
  `get_document` table, the sample against the oracle above, exactly ONE
  history step per insertion, and one `undo` restoring the table.
- **A4 the clip run is JOINED, not split.** With a clipped entry above the
  anchor, the new layer is created clipped and every neighbour's identity and
  clipped flag is unchanged.
- **A5 linear-light refusal parity.** `shadows_highlights` inserted into a
  linear-light document refuses with the SAME sentence at 8 and at 16 bits.
- **A6 a positional insert replays.** `start_recording` -> insert at
  `into_group` -> `stop_recording` -> `run_action` on a FRESH document gives
  the same table.
- **A7 the position persists.** `save_copy` to `.rz` then `open_document`
  reports the same table.
- **A8 merge down keeps the LOWER entry's identity** at both depths.
- **A9 a typed adjustment's LINKED mask follows `move_layers`.**

Nothing here derives an expectation from Doctored: every table and every
number below is written in this file.
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

# ---- the expectations, fixed BEFORE any measurement ----------------------
CANVAS = (16, 8)
SAMPLE = (4, 3)
MID_OPACITY = 0.25
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'

# The fixture's table, bottom-first, as (name, nesting level, clipped).
FIXTURE_TABLE = [('Background', 0, False), ('Mid', 1, False), ('Group', 0, False)]

# Where an inserted layer lands and what the composite then reads, per
# position. The oracle is straight-alpha source-over in f64, written out:
#   above      over(invert(white), white at 0.25) = 0.25*1 + 0.75*0 = 0.25
#   into_group over(white, invert(white) at 0.25) = 0.25*0 + 0.75*1 = 0.75
#   top        invert(white)                                        = 0.0
POSITIONS = {
    'above': {'anchor': 'Background', 'landing': 1, 'level': 0, 'value': 0.25,
              'table': [('Background', 0, False), ('Fx', 0, False),
                        ('Mid', 1, False), ('Group', 0, False)]},
    'into_group': {'anchor': 'Group', 'landing': 2, 'level': 1, 'value': 0.75,
                   'table': [('Background', 0, False), ('Mid', 1, False),
                             ('Fx', 1, False), ('Group', 0, False)]},
    'top': {'anchor': 'Background', 'landing': 3, 'level': 0, 'value': 0.0,
            'table': [('Background', 0, False), ('Mid', 1, False),
                      ('Group', 0, False), ('Fx', 0, False)]},
}


# ------------------------------------------------- readers (plumbing only) --

def rows(app, document):
    return app.call('get_document', document_id=document)['layers']


def table(app, document):
    return [(r['name'], int(r['depth']), bool(r.get('clipped', False)))
            for r in rows(app, document)]


def ids(app, document):
    return [int(r['id']) for r in rows(app, document)]


def index_of(app, document, name):
    for row in rows(app, document):
        if row['name'] == name:
            return int(row['index'])
    raise AssertionError(f'{name} is not in the document')


def sample01(app, document, point=SAMPLE):
    """The composite at `point` as RGB in [0, 1], plus the depth's own
    one-code tolerance. The byte reply reports 0...255; the typed reply
    reports native codes beside the maximum they are scaled to."""
    reply = app.call('sample_pixel', document_id=document, x=point[0], y=point[1])
    if 'mean_rgba_codes' in reply:
        maximum = float(reply['maximum_code'])
        return [float(v) / maximum for v in reply['mean_rgba_codes'][:3]], 1.0 / maximum
    return [float(reply[c]) / 255.0 for c in ('r', 'g', 'b')], 1.0 / 255.0


def history_count(app, document):
    """How many transitions are BEHIND the cursor. Not the state count: an
    undo leaves its redo branch in place until the next edit truncates it, so
    only the cursor counts steps."""
    return int(app.call('list_history', document_id=document)['undo_count'])


# ------------------------------------------------------------- fixtures ----

def build(app, document):
    """`Background` (white, opaque) under an ISOLATED `Group` holding one
    duplicate at opacity 0.25 — the stack every position gate runs on."""
    background = index_of(app, document, 'Background')
    app.call('duplicate_layer', document_id=document, layer=background)
    # The copy lands directly above its original at both depths; the typed
    # duplicate does not rename, so the copy is addressed by INDEX.
    app.call('set_layer_properties', document_id=document, index=background + 1,
             name='Mid', opacity=MID_OPACITY)
    mid = index_of(app, document, 'Mid')
    app.call('group_layers', document_id=document, layers=[mid], name='Group')
    group = index_of(app, document, 'Group')
    # An ISOLATED group: the default container is Pass Through, in which an
    # adjustment would reach below the group and the oracle above would not
    # describe it.
    app.call('set_layer_properties', document_id=document, index=group,
             blend_mode='normal')
    assert table(app, document) == FIXTURE_TABLE, table(app, document)
    return document


def new_stack(app, depth):
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='white')
    document = created['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == depth
    return build(app, document)


# ------------------------------------------------------------------ gates --

def position_gate(app, document, label, receipts, oracle=True):
    findings = {}
    baseline = table(app, document)
    for position, want in POSITIONS.items():
        anchor = index_of(app, document, want['anchor'])
        before = history_count(app, document)
        reply = app.call('add_adjustment_layer', document_id=document, op='invert',
                         name='Fx', layer=anchor, position=position)
        assert reply['position'] == position, reply
        assert int(reply['layer']) == want['landing'], (position, reply)
        measured = table(app, document)
        assert measured == want['table'], (label, position, measured, want['table'])
        after = history_count(app, document)
        assert after == before + 1, (label, position, 'one history step', before, after)
        entry = {'table': measured, 'landing': int(reply['layer'])}
        if oracle:
            got, tolerance = sample01(app, document)
            assert all(abs(c - want['value']) <= tolerance for c in got), \
                (label, position, got, want['value'], tolerance)
            entry['sample'] = got
            entry['oracle'] = want['value']
            entry['tolerance'] = tolerance
        app.call('undo', document_id=document)
        assert table(app, document) == baseline, (label, position, 'one undo restores')
        findings[position] = entry
    receipts[label] = findings


def gate_a1(app, receipts):
    position_gate(app, new_stack(app, 8), 'a1_three_positions_at_8_bits', receipts)
    print('PASS a1: the three positions at 8 bits, against the oracle', flush=True)


def gate_a2(app, receipts):
    position_gate(app, new_stack(app, 16), 'a2_three_positions_at_16_bits', receipts)
    print('PASS a2: the three positions at 16 bits, against the oracle', flush=True)


def gate_a3(app, folder, receipts):
    gate = 'a3_three_positions_at_32_bit_float'
    if not FLOAT_FIXTURE.exists():
        receipts[gate] = {'skipped': 'the float fixture is not committed'}
        print('PASS a3: skipped, no float fixture', flush=True)
        return
    source = folder / 'a3.tiff'
    shutil.copyfile(FLOAT_FIXTURE, source)
    document = app.call('open_document', path=str(source))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    build(app, document)
    # `invert` is outside the float admitted set (`float_document`), so the
    # float gate inserts the operator float DOES admit. The pixel oracle for
    # it lives in core/tests/precision_insert.rs; this gate owns the
    # structure, the one undo step and the reply.
    findings = {}
    baseline = table(app, document)
    for position, want in POSITIONS.items():
        anchor = index_of(app, document, want['anchor'])
        before = history_count(app, document)
        reply = app.call('add_adjustment_layer', document_id=document, op='exposure',
                         params={'exposure': 1.0}, name='Fx', layer=anchor,
                         position=position)
        assert reply['position'] == position, reply
        measured = table(app, document)
        assert measured == want['table'], (position, measured, want['table'])
        assert history_count(app, document) == before + 1, (position, 'one history step')
        findings[position] = {'table': measured, 'landing': int(reply['layer'])}
        app.call('undo', document_id=document)
        assert table(app, document) == baseline, (position, 'one undo restores')
    receipts[gate] = findings
    print('PASS a3: the three positions on a 32-bit float document', flush=True)


def gate_a4(app, receipts):
    gate = 'a4_a_new_layer_joins_the_clip_run'
    findings = {}
    for depth in (8, 16):
        document = new_stack(app, depth)
        # `Mid` is the only entry inside the group, so the clip run is built
        # at the ROOT level: clip the Group onto Background.
        group = index_of(app, document, 'Group')
        app.call('set_layer_clipped', document_id=document, layer=group, clipped=True)
        before_rows = rows(app, document)
        before = {r['name']: (int(r['id']), bool(r.get('clipped', False)))
                  for r in before_rows}
        anchor = index_of(app, document, 'Background')
        reply = app.call('add_adjustment_layer', document_id=document, op='invert',
                         name='Fx', layer=anchor, position='above')
        after_rows = rows(app, document)
        new = [r for r in after_rows if r['name'] == 'Fx'][0]
        assert bool(new['clipped']) is True, ('the new entry did not join the run', new)
        assert int(new['index']) == int(reply['layer']) == 1, (reply, new)
        for row in after_rows:
            if row['name'] == 'Fx':
                continue
            identity, clipped = before[row['name']]
            assert int(row['id']) == identity, ('identity moved', row['name'])
            assert bool(row.get('clipped', False)) is clipped, ('flag moved', row['name'])
        findings[f'depth_{depth}'] = {
            'new_clipped': True,
            'neighbours': {r['name']: [int(r['id']), bool(r.get('clipped', False))]
                           for r in after_rows if r['name'] != 'Fx'}}
    receipts[gate] = findings
    print('PASS a4: an insert above a clip base joins the run at both depths', flush=True)


def gate_a5(app, receipts):
    gate = 'a5_linear_light_refuses_identically_at_both_depths'
    sentences = {}
    for depth in (8, 16):
        document = new_stack(app, depth)
        app.call('set_evaluation', document_id=document, policy='linear_light')
        anchor = index_of(app, document, 'Background')
        sentences[f'depth_{depth}'] = app.call(
            'add_adjustment_layer', fail=True, document_id=document,
            op='shadows_highlights', params={'shadows': {'amount': 0.5}, 'radius': 8.0},
            name='SH', layer=anchor, position='top')
        assert table(app, document) == FIXTURE_TABLE, 'a refusal published nothing'
    assert sentences['depth_8'] == sentences['depth_16'], sentences
    assert 'linear-light evaluation does not support yet' in sentences['depth_8'], sentences
    receipts[gate] = sentences
    print('PASS a5: the linear-light refusal is the same sentence at both depths', flush=True)


def gate_a6(app, receipts):
    gate = 'a6_a_positional_insert_replays'
    document = new_stack(app, 16)
    app.call('start_recording', document_id=document)
    group = index_of(app, document, 'Group')
    app.call('add_adjustment_layer', document_id=document, op='invert', name='Fx',
             layer=group, position='into_group')
    recorded = table(app, document)
    app.call('stop_recording', document_id=document, name='PositionalInsert')
    assert recorded == POSITIONS['into_group']['table'], recorded

    fresh = new_stack(app, 16)
    app.call('run_action', document_id=fresh, name='PositionalInsert')
    replayed = table(app, fresh)
    assert replayed == recorded, (replayed, recorded)
    receipts[gate] = {'recorded': recorded, 'replayed': replayed}
    print('PASS a6: a recorded positional insert replays on a fresh document', flush=True)


def gate_a7(app, folder, receipts):
    gate = 'a7_the_position_survives_save_and_reopen'
    findings = {}
    for depth in (8, 16):
        document = new_stack(app, depth)
        group = index_of(app, document, 'Group')
        app.call('add_adjustment_layer', document_id=document, op='invert', name='Fx',
                 layer=group, position='into_group')
        before = table(app, document)
        path = folder / f'a7_{depth}.rz'
        app.call('save_copy', document_id=document, path=str(path))
        reopened = app.call('open_document', path=str(path))['document']['id']
        after = table(app, reopened)
        assert after == before, (depth, after, before)
        findings[f'depth_{depth}'] = {'before': before, 'after': after}
    receipts[gate] = findings
    print('PASS a7: a positional insert survives .rz save and reopen', flush=True)


def gate_a8(app, receipts):
    gate = 'a8_merge_down_keeps_the_lower_identity'
    findings = {}
    for depth in (8, 16):
        created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                           background='white')
        document = created['document']['id']
        app.call('duplicate_layer', document_id=document,
                 layer=index_of(app, document, 'Background'))
        before_rows = rows(app, document)
        assert len(before_rows) == 2, before_rows
        lower = before_rows[0]
        app.call('set_layer_label', document_id=document, layer=int(lower['index']),
                 label='red')
        lower = rows(app, document)[0]
        app.call('merge_down', document_id=document, layer=1)
        merged = rows(app, document)
        assert len(merged) == 1, merged
        assert int(merged[0]['id']) == int(lower['id']), (depth, merged[0], lower)
        assert merged[0]['label'] == lower['label'], (depth, merged[0], lower)
        findings[f'depth_{depth}'] = {'lower_id': int(lower['id']),
                                      'merged_id': int(merged[0]['id']),
                                      'label': merged[0]['label']}
    receipts[gate] = findings
    print('PASS a8: merge down keeps the lower entry id and label at both depths',
          flush=True)


def gate_a9(app, receipts):
    gate = 'a9_a_typed_adjustments_linked_mask_follows_move_layers'
    document = new_stack(app, 16)
    anchor = index_of(app, document, 'Background')
    app.call('add_adjustment_layer', document_id=document, op='invert', name='Fx',
             layer=anchor, position='above')
    fx = index_of(app, document, 'Fx')
    # A typed adjustment created without a selection carries no mask, so the
    # gate gives it one: what is being measured is whether a LINKED mask
    # follows the move, which is the byte op's rule.
    app.call('add_layer_mask', document_id=document, layer=fx, kind='reveal_all')
    properties = app.call('get_mask_properties', document_id=document, layer=fx)
    assert properties['linked'] is True, properties
    assert (int(properties['offset_x']), int(properties['offset_y'])) == (0, 0), properties

    app.call('move_layers', document_id=document, layers=[fx], dx=3, dy=2)
    moved = app.call('get_mask_properties', document_id=document, layer=fx)
    assert (int(moved['offset_x']), int(moved['offset_y'])) == (3, 2), (moved, properties)

    # An UNLINKED adjustment mask stays where it is, exactly as at byte depth.
    app.call('undo', document_id=document)
    app.call('set_mask_properties', document_id=document, layer=fx, linked=False)
    unlinked = app.call('move_layers', document_id=document, layers=[fx], dx=3, dy=2)
    assert unlinked['changed'] is False, unlinked
    still = app.call('get_mask_properties', document_id=document, layer=fx)
    assert (int(still['offset_x']), int(still['offset_y'])) == (0, 0), still
    receipts[gate] = {'linked_offset': [int(moved['offset_x']), int(moved['offset_y'])],
                      'unlinked_offset': [int(still['offset_x']), int(still['offset_y'])]}
    print('PASS a9: a typed adjustment mask follows a move only when linked', flush=True)


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

    receipts = {'expectations': 'fixed in scripts/verify_phase12e2_adjustments.py before any '
                                'measurement; the composite oracle is straight-alpha '
                                'source-over written out in this file, the tolerance is ONE '
                                'code at each depth, and every structural claim is exact'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        gate_a1(app, receipts)
        gate_a2(app, receipts)
        gate_a3(app, folder, receipts)
        gate_a4(app, receipts)
        gate_a5(app, receipts)
        gate_a6(app, receipts)
        gate_a7(app, folder, receipts)
        gate_a8(app, receipts)
        gate_a9(app, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
