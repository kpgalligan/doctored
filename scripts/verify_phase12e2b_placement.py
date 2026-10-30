#!/usr/bin/env python3
"""The Phase 12E-2b MCP drive: PLACEMENT at a chosen slot, typed `new_layer`
and the retained-source / attachment LOCKS.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. The instance is owned by this file (`HostApp`: a free port, a
dummy API key and a PRIVATE directory for every preference library the app
reads) and is terminated by its own context manager; no Doctored process
this file did not start is ever signalled.

TOLERANCES, fixed here before measuring:
  * composite samples for a PLACEMENT: within ONE code at the depth's own
    scale (1/255 and 1/65535 of full range). The oracle is straight-alpha
    source-over, computed in this file in f64.
  * a `new_layer` insertion: EXACT. A fully transparent Normal leaf at
    opacity 1 contributes nothing, so the composite must not move AT ALL.
  * every structural claim (which index, which nesting level, which clipping
    flag, which identity, how many history steps): EXACT.
  * the parked `NSAnimation _runBlocking` count (P11): EXACT — equal to the
    starting value at every reading, no Dispatch Thread Soft Limit.

THE FIXTURE, at every depth this drive builds one for:

    Background   white, opaque                     level 0
    Group        ISOLATED Normal, opacity 0.5      level 0
      Mid        blue, opaque (a placed image)     level 1

Placing an OPAQUE RED image at each of the three positions then lands it at a
different place in that composite, and the three oracles are different
numbers, which is what makes the gate discriminating:

    above Background -> the group's BACKDROP becomes red, and its blue
                        content composites over it at 0.5: (0.5, 0.0, 0.5)
    into_group       -> the group's CONTENT becomes red, over the white
                        backdrop at 0.5:                   (1.0, 0.5, 0.5)
    top              -> the whole projection is red:        (1.0, 0.0, 0.0)

- **P1 / P2 embedded placement at 8 and 16 bits.** The `get_document` table,
  the reply's `layer`/`position`, the sample against the oracle above, every
  neighbour's identity unchanged, exactly ONE history step, undo restores and
  redo re-applies.
- **P3 linked placement `into_group` at both depths.**
- **P4 host media** (an ImageMagick-authored HEIC) placed `into_group`.
- **P4b `add_live_photo_layer`** on an ffmpeg-AUTHORED 4-frame movie: the
  positional call lands in the group and the LEGACY call (a bare top-index
  `layer`, no `position`) lands above that entry at level 0. If ffmpeg is
  missing or fails, P4b is written to the receipt as NOT RUN with its reason,
  the drive prints INCOMPLETE instead of ALL GATES PASS and exits 2. A host
  REFUSAL of the authored movie is a FAIL, not a NOT RUN.
- **P5 typed `new_layer`** at the three positions, at 16 bits and at 32-bit
  float: the table, one history step each, undo, and an EXACT composite. If
  the committed float fixture is absent, P5b takes P4b's treatment exactly —
  NOT RUN in the receipt with its reason, INCOMPLETE instead of ALL GATES
  PASS, exit 2 — never a silent pass.
- **P6 replay.** A recorded positional `new_layer` + `place_image` replays on
  a fresh document to the same table.
- **P7 typed `add_text_layer` / `add_shape_layer`** at the three positions.
- **P8 the LOCKS.** A position-locked placed layer refuses
  `transform_layer` with the HOST's sentence, at both depths. History is
  unchanged by the refusal.
- **P9 `.rz` round trip.** Save and reopen keeps the table.
- **P10 float takes `place_image`.** The red PNG lands above the float
  fixture's layer as an ordinary raster layer (`source` null) in a document
  that stays 32-bit, as ONE history step; the composite under it reads
  (1, 0, 0, 1) within one code of the 8-bit file placed, and one undo restores
  the table and the composite exactly. Absent float fixture: P4b's NOT RUN /
  INCOMPLETE / exit 2 treatment, as P5b's.
- **P11 the parked-thread count stays FLAT (13A-7).** Every gate above runs
  in ONE owned instance. The count of threads PARKED in
  `-[NSAnimation _runBlocking]` (in that frame for the whole of a `sample` of
  THAT instance) is read once before P1 (the starting value),
  once after P10, and then every 20 of 200 consecutive worker-backed edits
  (`place_image` + `undo`, each presenting the `BackgroundWork` panel); every
  reading must EQUAL the starting value and no sample may name the Dispatch
  Thread Soft Limit.

Nothing here derives an expectation from Doctored: every table and every
number below is written in this file.
"""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp
from verify_phase1 import Client, png


# `Client`'s ceiling raised for THIS PROCESS only. The drive holds a dozen open
# documents by its later gates and a host-media placement admits against the
# WHOLE live set, so the shared 30-second client timeout is raised here rather
# than by shortening the drive or closing documents a gate still needs.
Client.timeout = 300

# ---- the expectations, fixed BEFORE any measurement ----------------------
CANVAS = (16, 8)
SAMPLE = (4, 3)
GROUP_OPACITY = 0.5
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
FFMPEG = shutil.which('ffmpeg') or '/opt/homebrew/bin/ffmpeg'
THREAD_EDITS = 200
THREAD_MARK_EVERY = 20
# P10: the opaque red 8-bit PNG placed in the float fixture. Full-scale red is
# (1, 0, 0) in any encoding of the fixture's primaries, alpha 1; the tolerance
# is one code of the 8-bit file placed.
FLOAT_PLACE_VALUE = (1.0, 0.0, 0.0, 1.0)
FLOAT_PLACE_TOLERANCE = 1.0 / 255.0

# The fixture's table, bottom-first / postorder, as (name, level, clipped).
FIXTURE_TABLE = [('Background', 0, False), ('Mid', 1, False), ('Group', 0, False)]

# Where a placed layer lands, what the table becomes and what the composite
# then reads. The oracle is straight-alpha source-over in f64, written out:
#   above      group(blue) at 0.5 over red      -> 0.5*blue + 0.5*red
#   into_group group(red)  at 0.5 over white    -> 0.5*red  + 0.5*white
#   top        red at alpha 1 over anything     -> red
POSITIONS = {
    'above': {'anchor': 'Background', 'landing': 1,
              'value': (0.5, 0.0, 0.5),
              'table': [('Background', 0, False), ('Placed', 0, False),
                        ('Mid', 1, False), ('Group', 0, False)]},
    'into_group': {'anchor': 'Group', 'landing': 2,
                   'value': (1.0, 0.5, 0.5),
                   'table': [('Background', 0, False), ('Mid', 1, False),
                             ('Placed', 1, False), ('Group', 0, False)]},
    'top': {'anchor': 'Background', 'landing': 3,
            'value': (1.0, 0.0, 0.0),
            'table': [('Background', 0, False), ('Mid', 1, False),
                      ('Group', 0, False), ('Placed', 0, False)]},
}

# Where a NEW LAYER lands, per position, on the same fixture.
NEW_LAYER_TABLES = {
    'above': [('Background', 0, False), ('New', 0, False),
              ('Mid', 1, False), ('Group', 0, False)],
    'into_group': [('Background', 0, False), ('Mid', 1, False),
                   ('New', 1, False), ('Group', 0, False)],
    'top': [('Background', 0, False), ('Mid', 1, False),
            ('Group', 0, False), ('New', 0, False)],
}
NEW_LAYER_ANCHOR = {'above': 'Background', 'into_group': 'Group', 'top': 'Background'}
NEW_LAYER_LANDING = {'above': 1, 'into_group': 2, 'top': 3}


# ------------------------------------------------- readers (plumbing only) --


def rows(app, document):
    return app.call('get_document', document_id=document)['layers']


def table(app, document):
    return [(r['name'], int(r['depth']), bool(r.get('clipped', False)))
            for r in rows(app, document)]


def identities(app, document):
    return {r['name']: int(r['id']) for r in rows(app, document)}

def index_of(app, document, name):
    for row in rows(app, document):
        if row['name'] == name:
            return int(row['index'])
    raise AssertionError(f'{name} is not in the document')


def sample01(app, document, point=SAMPLE):
    """The composite at `point` as RGB in [0, 1], plus the depth's own
    one-code tolerance."""
    reply = app.call('sample_pixel', document_id=document, x=point[0], y=point[1])
    if 'mean_rgba_codes' in reply:
        maximum = float(reply['maximum_code'])
        return [float(v) / maximum for v in reply['mean_rgba_codes'][:3]], 1.0 / maximum
    return [float(reply[c]) / 255.0 for c in ('r', 'g', 'b')], 1.0 / 255.0


def composite_signature(app, document, point=SAMPLE):
    """The WHOLE readout at `point`, whatever shape the depth reports it in —
    compared EXACTLY, because the claim a transparent insertion makes is that
    nothing moved at all."""
    reply = app.call('sample_pixel', document_id=document, x=point[0], y=point[1])
    return {k: v for k, v in sorted(reply.items())
            if isinstance(v, (int, float, list, str, bool))}


def history_count(app, document):
    """How many transitions are BEHIND the cursor."""
    return int(app.call('list_history', document_id=document)['undo_count'])


# ------------------------------------------------------------- fixtures ----


def solid(path, rgba, size=CANVAS):
    width, height = size
    png(Path(path), width, height, bytes(bytes(rgba) * width * height))


def build(app, document, folder, blue):
    """`Background` (white, opaque) under an ISOLATED `Group` at opacity 0.5
    holding one opaque blue raster (`Mid`) — the stack every gate runs on."""
    app.call('place_image', document_id=document, path=str(blue), name='Mid',
             position='top')
    mid = index_of(app, document, 'Mid')
    app.call('group_layers', document_id=document, layers=[mid], name='Group')
    group = index_of(app, document, 'Group')
    # An ISOLATED group: the default container is Pass Through, in which the
    # group's own opacity would not apply as the oracle above describes.
    app.call('set_layer_properties', document_id=document, index=group,
             blend_mode='normal', opacity=GROUP_OPACITY)
    assert table(app, document) == FIXTURE_TABLE, table(app, document)
    return document

def new_stack(app, depth, folder, blue):
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=depth,
                       background='white')
    document = created['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == depth
    return build(app, document, folder, blue)


# ------------------------------------------------------------------ gates --

def placement_gate(app, document, label, red, receipts, storage='embedded'):
    findings = {}
    baseline = table(app, document)
    baseline_ids = identities(app, document)
    for position, want in POSITIONS.items():
        anchor = index_of(app, document, want['anchor'])
        before = history_count(app, document)
        reply = app.call('place_image', document_id=document, path=str(red),
                         name='Placed', layer=anchor, position=position,
                         storage=storage)
        assert reply['position'] == position, reply
        assert int(reply['layer']) == want['landing'], (label, position, reply)
        measured = table(app, document)
        assert measured == want['table'], (label, position, measured, want['table'])
        after_ids = identities(app, document)
        for name, identity in baseline_ids.items():
            assert after_ids[name] == identity, (label, position, 'identity moved', name)
        assert history_count(app, document) == before + 1, \
            (label, position, 'one history step')
        got, tolerance = sample01(app, document)
        assert all(abs(c - o) <= tolerance for c, o in zip(got, want['value'])), \
            (label, position, got, want['value'], tolerance)
        findings[position] = {'table': measured, 'landing': int(reply['layer']),
                              'sample': got, 'oracle': list(want['value']),
                              'tolerance': tolerance}
        app.call('undo', document_id=document)
        assert table(app, document) == baseline, (label, position, 'one undo restores')
        app.call('redo', document_id=document)
        assert table(app, document) == want['table'], (label, position, 'redo re-applies')
        app.call('undo', document_id=document)
        assert table(app, document) == baseline, (label, position, 'back to the fixture')
    receipts[label] = findings


def gate_p1(app, folder, red, blue, receipts):
    placement_gate(app, new_stack(app, 8, folder, blue),
                   'p1_embedded_placement_at_8_bits', red, receipts)
    print('PASS P1: embedded placement at three positions, 8 bits', flush=True)


def gate_p2(app, folder, red, blue, receipts):
    placement_gate(app, new_stack(app, 16, folder, blue),
                   'p2_embedded_placement_at_16_bits', red, receipts)
    print('PASS P2: embedded placement at three positions, 16 bits', flush=True)


def gate_p3(app, folder, red, blue, receipts):
    gate = 'p3_linked_placement_into_group'
    findings = {}
    for depth in (8, 16):
        document = new_stack(app, depth, folder, blue)
        group = index_of(app, document, 'Group')
        reply = app.call('place_image', document_id=document, path=str(red),
                         name='Placed', layer=group, position='into_group',
                         storage='linked')
        assert reply['position'] == 'into_group', reply
        assert int(reply['layer']) == 2, reply
        measured = table(app, document)
        assert measured == POSITIONS['into_group']['table'], (depth, measured)
        findings[f'depth_{depth}'] = {'table': measured}
    receipts[gate] = findings
    print('PASS P3: a linked placement lands inside the group at both depths', flush=True)


def gate_p4(app, folder, blue, receipts):
    gate = 'p4_host_media_into_group'
    source = folder / 'p4.png'
    solid(source, (200, 40, 60, 255))
    heic = folder / 'p4.heic'
    # AUTHORED here, never taken from the preserved 8F corpus. ImageMagick's
    # encoder is 8F's own (`scripts/host-source-probe/fixtures.py`) and its
    # output decodes to a layout `HostStillDecoder` admits; `sips`'s HEIC
    # decodes to a layout it does not, and this gate is about PLACEMENT, not
    # about widening the decoder's admitted set.
    magick = shutil.which('magick')
    assert magick, 'ImageMagick is required to author the P4 HEIC fixture'
    completed = subprocess.run([magick, str(source), '-depth', '10', '-quality', '100',
                                str(heic)], capture_output=True, text=True)
    assert completed.returncode == 0 and heic.is_file(), (completed.returncode,
                                                          completed.stderr)
    document = new_stack(app, 8, folder, blue)
    group = index_of(app, document, 'Group')
    before = history_count(app, document)
    reply = app.call('place_image', document_id=document, path=str(heic),
                     name='Placed', layer=group, position='into_group')
    assert reply['position'] == 'into_group', reply
    landing = int(reply['layer'])
    assert landing == 2, reply
    measured = table(app, document)
    assert measured == POSITIONS['into_group']['table'], measured
    assert history_count(app, document) == before + 1, 'one history step'
    app.call('undo', document_id=document)
    assert table(app, document) == FIXTURE_TABLE, 'one undo restores'
    receipts[gate] = {'table': measured, 'landing': landing}
    print('PASS P4: a host-media HEIC lands inside the group', flush=True)


def author_movie(folder):
    """The 8F recipe's own flags over a `lavfi` source — an AUTHORED fixture,
    never the preserved corpus. Answers the path, or a reason it could not be
    made."""
    if not Path(FFMPEG).is_file():
        return None, f'ffmpeg is absent ({FFMPEG})'
    destination = folder / 'fixtures'
    destination.mkdir(parents=True, exist_ok=True)
    movie = destination / 'p4b.mov'
    command = [FFMPEG, '-hide_banner', '-loglevel', 'error', '-y',
               '-f', 'lavfi', '-i', 'testsrc2=size=64x48:rate=24000/1001',
               '-frames:v', '4', '-vf', 'format=yuv420p', '-c:v', 'libx264',
               '-bf', '2', '-g', '12', '-threads', '1',
               '-color_primaries', 'bt709', '-color_trc', 'bt709',
               '-colorspace', 'bt709', '-movflags', '+write_colr',
               '-video_track_timescale', '24000', str(movie)]
    completed = subprocess.run(command, capture_output=True, text=True)
    if completed.returncode != 0 or not movie.is_file():
        return None, f'ffmpeg exited {completed.returncode}: {completed.stderr.strip()[:400]}'
    return movie, None


def gate_p4b(app, folder, blue, receipts):
    gate = 'p4b_add_live_photo_layer_positions'
    movie, reason = author_movie(folder)
    if movie is None:
        receipts[gate] = {'gate': 'P4b', 'status': 'NOT RUN', 'reason': reason}
        print(f'NOT RUN P4b: {reason}', flush=True)
        return reason

    # (a) the group as the anchor, with an explicit position.
    document = new_stack(app, 8, folder, blue)
    group = index_of(app, document, 'Group')
    before_ids = identities(app, document)
    before = history_count(app, document)
    reply = app.call('add_live_photo_layer', document_id=document, path=str(movie),
                     name='Placed', layer=group, position='into_group')
    assert reply['position'] == 'into_group', reply
    landing = int(reply['layer'])
    assert landing == 2, reply
    measured = table(app, document)
    assert measured == POSITIONS['into_group']['table'], measured
    after_ids = identities(app, document)
    for name, identity in before_ids.items():
        assert after_ids[name] == identity, ('identity moved', name)
    assert history_count(app, document) == before + 1, 'one history step'
    app.call('undo', document_id=document)
    assert table(app, document) == FIXTURE_TABLE, 'one undo restores'

    # (b) the LEGACY call: `layer` names the top index and no position is
    # passed, so the new entry lands ABOVE that entry, at level 0, unclipped,
    # and everything below it is unchanged.
    legacy_document = new_stack(app, 8, folder, blue)
    top = len(FIXTURE_TABLE) - 1
    legacy_before = identities(app, legacy_document)
    legacy = app.call('add_live_photo_layer', document_id=legacy_document,
                      path=str(movie), name='Legacy', layer=top)
    assert legacy['position'] == 'above', legacy
    assert int(legacy['layer']) == len(FIXTURE_TABLE), legacy
    legacy_table = table(app, legacy_document)
    assert legacy_table == FIXTURE_TABLE + [('Legacy', 0, False)], legacy_table
    legacy_after = identities(app, legacy_document)
    for name, identity in legacy_before.items():
        assert legacy_after[name] == identity, ('legacy identity moved', name)

    receipts[gate] = {'positional': {'table': measured, 'landing': landing},
                      'legacy': {'table': legacy_table,
                                 'landing': int(legacy['layer'])},
                      'fixture': str(movie)}
    print('PASS P4b: add_live_photo_layer places positionally and legacy-style',
          flush=True)
    return None


def new_layer_gate(app, document, label, receipts):
    findings = {}
    baseline = table(app, document)
    signature = composite_signature(app, document)
    for position, want in NEW_LAYER_TABLES.items():
        anchor = index_of(app, document, NEW_LAYER_ANCHOR[position])
        before = history_count(app, document)
        reply = app.call('new_layer', document_id=document, name='New',
                         layer=anchor, position=position)
        assert reply['position'] == position, reply
        assert int(reply['layer']) == NEW_LAYER_LANDING[position], (label, position, reply)
        measured = table(app, document)
        assert measured == want, (label, position, measured, want)
        assert history_count(app, document) == before + 1, \
            (label, position, 'one history step')
        # EXACT: a fully transparent Normal leaf contributes nothing.
        assert composite_signature(app, document) == signature, \
            (label, position, 'the composite moved')
        findings[position] = {'table': measured, 'landing': int(reply['layer'])}
        app.call('undo', document_id=document)
        assert table(app, document) == baseline, (label, position, 'one undo restores')
    receipts[label] = {'positions': findings, 'composite': signature}


def gate_p5(app, folder, blue, receipts):
    new_layer_gate(app, new_stack(app, 16, folder, blue),
                   'p5_new_layer_at_16_bits', receipts)
    print('PASS P5a: typed new_layer at three positions, 16 bits', flush=True)

    gate = 'p5_new_layer_at_32_bit_float'
    if not FLOAT_FIXTURE.exists():
        reason = f'the float fixture is absent ({FLOAT_FIXTURE})'
        receipts[gate] = {'gate': 'P5b', 'status': 'NOT RUN', 'reason': reason}
        print(f'NOT RUN P5b: {reason}', flush=True)
        return reason
    source = folder / 'p5.tiff'
    shutil.copyfile(FLOAT_FIXTURE, source)
    document = app.call('open_document', path=str(source))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    # The float fixture is one raster; the group the `into_group` position
    # needs is built from a duplicate of it.
    app.call('duplicate_layer', document_id=document, layer=0)
    app.call('set_layer_properties', document_id=document, index=1, name='Mid')
    app.call('group_layers', document_id=document, layers=[1], name='Group')
    group = index_of(app, document, 'Group')
    app.call('set_layer_properties', document_id=document, index=group,
             blend_mode='normal', opacity=GROUP_OPACITY)
    app.call('set_layer_properties', document_id=document, index=0, name='Background')
    assert table(app, document) == FIXTURE_TABLE, table(app, document)
    new_layer_gate(app, document, gate, receipts)
    print('PASS P5b: typed new_layer at three positions, 32-bit float', flush=True)
    return None


def gate_p6(app, folder, red, blue, receipts):
    gate = 'p6_a_positional_placement_replays'
    document = new_stack(app, 8, folder, blue)
    app.call('start_recording', document_id=document)
    app.call('new_layer', document_id=document, name='New',
             layer=index_of(app, document, 'Background'), position='top')
    app.call('place_image', document_id=document, path=str(red), name='Placed',
             layer=index_of(app, document, 'Group'), position='into_group')
    recorded = table(app, document)
    app.call('stop_recording', document_id=document, name='PositionalPlacement')
    assert recorded == [('Background', 0, False), ('Mid', 1, False),
                        ('Placed', 1, False), ('Group', 0, False),
                        ('New', 0, False)], recorded

    fresh = new_stack(app, 8, folder, blue)
    app.call('run_action', document_id=fresh, name='PositionalPlacement')
    replayed = table(app, fresh)
    assert replayed == recorded, (replayed, recorded)
    receipts[gate] = {'recorded': recorded, 'replayed': replayed}
    print('PASS P6: a recorded positional new_layer + place_image replays', flush=True)


def gate_p7(app, folder, blue, receipts):
    gate = 'p7_typed_text_and_shape_at_three_positions'
    findings = {}
    for tool, arguments, expected in (
        ('add_text_layer', {'text': 'Hi', 'x': 1, 'y': 1, 'size': 8}, 'Hi'),
        ('add_shape_layer', {'kind': 'rect', 'x': 1, 'y': 1, 'w': 6, 'h': 4,
                             'fill': '#00ff00'}, 'Rectangle'),
    ):
        per_tool = {}
        document = new_stack(app, 16, folder, blue)
        baseline = table(app, document)
        for position in ('above', 'into_group', 'top'):
            anchor = index_of(app, document, NEW_LAYER_ANCHOR[position])
            before = history_count(app, document)
            reply = app.call(tool, document_id=document, layer=anchor,
                             position=position, **arguments)
            assert reply['position'] == position, (tool, reply)
            landing = int(reply['layer'])
            assert landing == NEW_LAYER_LANDING[position], (tool, position, reply)
            measured = table(app, document)
            names = [row[0] for row in measured]
            want = [row[0] for row in NEW_LAYER_TABLES[position]]
            levels = [row[1] for row in measured]
            assert len(names) == len(want), (tool, position, measured)
            assert levels == [row[1] for row in NEW_LAYER_TABLES[position]], \
                (tool, position, measured)
            assert names[landing] not in ('Background', 'Mid', 'Group'), \
                (tool, position, measured)
            assert history_count(app, document) == before + 1, \
                (tool, position, 'one history step')
            per_tool[position] = {'table': measured, 'landing': landing}
            app.call('undo', document_id=document)
            assert table(app, document) == baseline, (tool, position, 'undo restores')
        findings[tool] = per_tool
    receipts[gate] = findings
    print('PASS P7: typed text and shape layers land at the three positions',
          flush=True)


def gate_p8(app, folder, red, blue, receipts):
    gate = 'p8_a_locked_placed_layer_refuses_by_name'
    findings = {}
    for depth in (8, 16):
        document = new_stack(app, depth, folder, blue)
        group = index_of(app, document, 'Group')
        app.call('place_image', document_id=document, path=str(red), name='Placed',
                 layer=group, position='into_group')
        placed = index_of(app, document, 'Placed')
        before_table = table(app, document)

        # POSITION lock -> the HOST's own sentence (`rejectLockedMove`).
        app.call('set_layer_lock', document_id=document, layer=placed,
                 locks=['position'])
        steps = history_count(app, document)
        position_refusal = app.call('transform_layer', fail=True, document_id=document,
                                    layer=placed, dx=3, dy=2)
        want_position = ('Layer \u201cPlaced\u201d has its position locked. Clear it with '
                         'set_layer_lock {"layer": %d, "locks": []} first.' % placed)
        assert position_refusal == want_position, (depth, position_refusal, want_position)
        assert history_count(app, document) == steps, (depth, 'a refusal wrote history')
        assert table(app, document) == before_table, (depth, 'a refusal changed the stack')
        findings[f'depth_{depth}'] = {'transform': position_refusal}
    receipts[gate] = findings
    print('PASS P8: a position-locked placed layer refuses the move by name at both depths',
          flush=True)


def gate_p9(app, folder, red, blue, receipts):
    gate = 'p9_a_positional_placement_survives_a_native_round_trip'
    findings = {}
    for depth in (8, 16):
        document = new_stack(app, depth, folder, blue)
        group = index_of(app, document, 'Group')
        app.call('place_image', document_id=document, path=str(red), name='Placed',
                 layer=group, position='into_group')
        before = table(app, document)
        path = folder / f'p9_{depth}.rz'
        app.call('save_copy', document_id=document, path=str(path))
        reopened = app.call('open_document', path=str(path))['document']['id']
        after = table(app, reopened)
        assert after == before, (depth, after, before)
        findings[f'depth_{depth}'] = {'before': before, 'after': after}
    receipts[gate] = findings
    print('PASS P9: a positional placement survives .rz save and reopen', flush=True)


def gate_p10(app, folder, red, receipts):
    gate = 'p10_float_place_image_lands_a_raster_layer'
    if not FLOAT_FIXTURE.exists():
        reason = f'the float fixture is absent ({FLOAT_FIXTURE})'
        receipts[gate] = {'gate': 'P10', 'status': 'NOT RUN', 'reason': reason}
        print(f'NOT RUN P10: {reason}', flush=True)
        return reason
    source = folder / 'p10.tiff'
    shutil.copyfile(FLOAT_FIXTURE, source)
    document = app.call('open_document', path=str(source))['document']['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    baseline = table(app, document)
    signature = composite_signature(app, document)
    before = history_count(app, document)
    reply = app.call('place_image', document_id=document, path=str(red), name='Placed',
                     x=0, y=0)
    landing = int(reply['layer'])
    assert reply['position'] == 'above' and landing == len(baseline), reply
    assert reply.get('source', 0) is None, reply
    bound = app.call('get_image_source', document_id=document, layer=landing)
    assert bound.get('source', 0) is None, bound
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    measured = table(app, document)
    assert measured == baseline + [('Placed', 0, False)], measured
    assert history_count(app, document) == before + 1, 'one history step'
    sample = app.call('sample_pixel', document_id=document, x=SAMPLE[0], y=SAMPLE[1])
    assert sample['storage'] == 'rgba32_float', sample
    got = [float(v) for v in sample['mean_rgba32']]
    assert all(abs(c - o) <= FLOAT_PLACE_TOLERANCE for c, o in zip(got, FLOAT_PLACE_VALUE)), \
        (got, FLOAT_PLACE_VALUE, FLOAT_PLACE_TOLERANCE)
    app.call('undo', document_id=document)
    assert table(app, document) == baseline, 'one undo restores the table'
    assert composite_signature(app, document) == signature, 'one undo restores the composite'
    receipts[gate] = {'table': measured, 'landing': landing, 'source': None, 'sample': got,
                      'oracle': list(FLOAT_PLACE_VALUE), 'tolerance': FLOAT_PLACE_TOLERANCE}
    print('PASS P10: a float document takes place_image as an ordinary raster layer',
          flush=True)
    return None


THREAD_LINE = re.compile(r'^ {4}(\d+) (Thread_\d+)')
RUN_BLOCKING = re.compile(r'^[ +!:|]*?(\d+) -\[NSAnimation _runBlocking\]')


def parked(app, folder, label):
    """A `sample` of OUR OWN instance, kept as a receipt, read as
    (parked threads, `_runBlocking` frame lines, soft limit named).

    A thread is PARKED when it sits in `-[NSAnimation _runBlocking]` for the
    WHOLE sample (its `_runBlocking` sample counts sum to its own thread's
    sample count, summed over the entries `sample` splits a thread into by
    dispatch queue). A RUNNING animation shows the frame for part of the
    sample and is not parked; the raw frame-line count (which also counts one
    thread twice when it shows the frame at two offsets) is recorded, not
    asserted.
    Checked against 12E-2b's own receipts: its before-fix sample at 20 edits
    reads 62 parked threads (63 frame lines), its after-fix end-of-200 sample
    2 (2)."""
    out = folder / f'sample-{label}.txt'
    subprocess.run(['sample', str(app.process.pid), '1', '-f', str(out)],
                   check=True, capture_output=True, text=True)
    text = out.read_text()
    own, current, inside = {}, None, {}
    for line in text.splitlines():
        thread = THREAD_LINE.match(line)
        if thread:
            current = thread.group(2)
            own[current] = own.get(current, 0) + int(thread.group(1))
            continue
        frame = RUN_BLOCKING.match(line)
        if frame and current:
            inside[current] = inside.get(current, 0) + int(frame.group(1))
    return (sum(1 for t, count in inside.items() if count >= own[t]),
            len(re.findall(r'-\[NSAnimation _runBlocking\]', text)),
            'Dispatch Thread Soft Limit' in text)


def gate_p11(app, folder, red, blue, start, after_gates, receipts):
    gate = 'p11_parked_threads_flat_over_200_worker_backed_edits'
    samples = folder / 'panel-threads'
    document = new_stack(app, 8, folder, blue)
    marks = {}
    for i in range(THREAD_EDITS):
        app.call('place_image', document_id=document, path=str(red), name=f'P{i}')
        app.call('undo', document_id=document)
        if (i + 1) % THREAD_MARK_EVERY == 0:
            marks[i + 1] = parked(app, samples, f'at-{i + 1}')
    readings = {'after_gates': after_gates, **{str(k): v for k, v in marks.items()}}
    receipts[gate] = {'start': start[0], 'edits': THREAD_EDITS,
                      'readings': {k: v[0] for k, v in readings.items()},
                      'frame_lines': {'start': start[1],
                                      **{k: v[1] for k, v in readings.items()}},
                      'soft_limit_reached': start[2] or any(v[2] for v in readings.values()),
                      'samples': str(samples)}
    assert not start[2], 'the soft limit was named before P1'
    for label, (count, _, limited) in readings.items():
        assert count == start[0], (label, count, 'start', start[0])
        assert not limited, (label, 'the Dispatch Thread Soft Limit was reached')
    print(f'PASS P11: the parked NSAnimation count stays at {start[0]} through the '
          f'drive and {THREAD_EDITS} more worker-backed edits in one instance', flush=True)


def run_gates(app, folder, red, blue, samples, receipts, incomplete):
    """Every gate, in order, in the ONE owned instance `app`. The starting
    parked-thread reading is taken once the instance holds its first document
    (as 12E-2b's measurement read it), before P1."""
    app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
             background='white')
    start = parked(app, samples, 'start')
    gate_p1(app, folder, red, blue, receipts)
    gate_p2(app, folder, red, blue, receipts)
    gate_p3(app, folder, red, blue, receipts)
    gate_p4(app, folder, blue, receipts)
    reason = gate_p4b(app, folder, blue, receipts)
    if reason:
        incomplete.append(('P4b', reason))
    reason = gate_p5(app, folder, blue, receipts)
    if reason:
        incomplete.append(('P5b', reason))
    gate_p6(app, folder, red, blue, receipts)
    gate_p7(app, folder, blue, receipts)
    gate_p8(app, folder, red, blue, receipts)
    gate_p9(app, folder, red, blue, receipts)
    reason = gate_p10(app, folder, red, receipts)
    if reason:
        incomplete.append(('P10', reason))
    after_gates = parked(app, samples, 'after-gates')
    gate_p11(app, folder, red, blue, start, after_gates, receipts)


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

    red = folder / 'red.png'
    blue = folder / 'blue.png'
    solid(red, (255, 0, 0, 255))
    solid(blue, (0, 0, 255, 255))

    receipts = {'expectations': 'fixed in scripts/verify_phase12e2b_placement.py before '
                                'any measurement; the composite oracle is straight-alpha '
                                'source-over written out in this file, the placement '
                                'tolerance is ONE code at each depth, and a transparent '
                                'new layer is compared EXACTLY'}
    # ONE OWNED INSTANCE for every gate (13A-7). The drive used to relaunch
    # per gate: every worker-backed edit shows AppKit's Cancel panel, and a
    # titled panel's default appearance animation parked one
    # `NSAnimation _runBlocking` thread per presentation until the 64-thread
    # dispatch limit deadlocked `BackgroundWork.wait`. 12E-2b's review pass
    # fixed it (`JobProgressController` sets `window.animationBehavior =
    # .none`); P11 holds the parked count flat in this one instance. The
    # instance is terminated by its own context manager.
    # Every gate that can be NOT RUN answers a REASON; none of them may reach
    # `ALL GATES PASS` by skipping quietly.
    incomplete = []
    samples = folder / 'panel-threads'
    samples.mkdir()
    try:
        with HostApp(folder / 'app', folder / 'recovery', executable) as app:
            run_gates(app, folder, red, blue, samples, receipts, incomplete)
    finally:
        (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True)
                                             + '\n')
    if incomplete:
        for name, reason in incomplete:
            print(f'INCOMPLETE: {name} NOT RUN ({reason})', flush=True)
        sys.exit(2)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
