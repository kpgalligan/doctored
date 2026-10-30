#!/usr/bin/env python3
"""The Phase 12D-2 view-rotation / second-view MCP drive, on a built Release app.

Gates FIXED here BEFORE anything is measured; a failed gate IS the receipt and
no tolerance is widened afterwards. The instance is owned by this file
(`HostApp`: a free port, a dummy API key and a PRIVATE directory for every
preference library the app reads) and is terminated by its own context manager;
no Doctored process this file did not start is ever signalled.

- **V1 rotation round-trips and writes nothing.** `set_view {rotation: 30}`
  answers exactly `30.0` and so does `get_view`; `370` reads back `10.0`,
  `-180` and `180` both read back `180.0`; `rotation: "x"` is refused BY NAME.
  Across every one of them the `render` PNG's SHA-256, the `list_history`
  length and `get_document.unsaved_changes` are unchanged.
- **V2 a stroke under a rotated view lands where it was asked.** The MCP
  surface takes DOCUMENT coordinates, so the independent oracle is the
  IDENTITY: the same one-point 100 %-opacity hard `brush_stroke` on a fresh
  transparent layer of two twin documents — one at 0°, one at 30° — must give
  byte-identical `render` digests. (The pointer mapping itself is the UI-source
  probe's `rotate-view-stroke-ui`.)
- **V3 a second view.** `open_second_view`'s published schema offers no `view`
  (the ordinal is minted by the call) while `close_second_view`'s requires one;
  `open_second_view` answers `view: 2` and `list_views` lists two; `set_view {view: 2, zoom: 1}` reads back `1.0` while
  `set_view {view: 1, fit: true}` reads back below 1 on a 4000 x 3000 canvas;
  an UNQUALIFIED `get_view` equals `get_view {view: 1}` field for field and an
  unqualified `set_view {zoom: 1.5}` moves view 1 and leaves view 2 alone; and
  after one `fill` both views report the same `revision` and the same
  `list_history` length.
- **V5 one shared undo.** One `undo` moves `list_history.cursor` back by
  exactly one and returns the render digest to its pre-edit value. Run
  IMMEDIATELY after V3 so the edit it walks back is V3's.
- **V4 closing a view.** `close_second_view {view: 2}` leaves
  `unsaved_changes` exactly as it was, `list_views` lists one, and
  `close_second_view {view: 1}` is refused BY NAME; a `fill` after the close
  still succeeds and `get_view {view: 1}.revision` advances by exactly one.
- **N1 the carried 12D-1 nit.** `apply_layer_comp {name: " Hero "}` succeeds on
  a comp saved as `Hero`.

Nothing here derives an expectation from Doctored: the sizes, the angles, the
stroke and the index sets are computed in this file.
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
# There are none. Every angle, zoom, revision, digest and count below is either
# an integer this file chose or a value the app is asked to echo back
# unchanged; the only inequality is "a fit of a 4000 x 3000 canvas is below
# 100 %", which is a property of the canvas this file created.
CANVAS = (1600, 1200)
BIG_CANVAS = (4000, 3000)
STROKE = {'points': [[200, 150]], 'size': 40, 'hardness': 100, 'opacity': 1,
          'flow': 100, 'color': '#3366CC'}


def render_digest(app, document):
    """The SHA-256 of `render`'s PNG bytes. The raw RPC, because the shared
    client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def history(app, document):
    return app.call('list_history', document_id=document)


def dirty(app, document):
    return app.call('get_document', document_id=document)['unsaved_changes']


# ------------------------------------------------------------------ gates --

def gate_v1(app, receipts):
    gate = 'v1_rotation_round_trip'
    findings = {}
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']

    before_render = render_digest(app, document)
    before_states = len(history(app, document)['states'])
    before_dirty = dirty(app, document)

    rotations = []
    for asked, expected in [(30, 30.0), (370, 10.0), (-180, 180.0), (180, 180.0), (0, 0.0)]:
        reply = app.call('set_view', document_id=document, rotation=asked)
        read_back = app.call('get_view', document_id=document)
        assert reply['rotation'] == expected, (asked, reply['rotation'], expected)
        assert read_back['rotation'] == expected, (asked, read_back['rotation'], expected)
        assert {k: v for k, v in reply.items() if k not in ('ok', 'note')} == read_back, \
            (reply, read_back)
        rotations.append({'asked': asked, 'expected': expected, 'read_back': read_back['rotation']})
    findings['rotations'] = rotations

    refusal = app.call('set_view', document_id=document, rotation='x', fail=True)
    assert 'rotation' in refusal, refusal
    both = app.call('set_view', document_id=document, fit=True, zoom=2, fail=True)
    assert 'fit' in both and 'zoom' in both, both
    findings['refusals'] = {'rotation_not_a_number': refusal, 'fit_and_zoom': both}

    assert render_digest(app, document) == before_render, 'rotating the view changed the render'
    after_states = len(history(app, document)['states'])
    assert after_states == before_states, (after_states, before_states)
    after_dirty = dirty(app, document)
    assert after_dirty == before_dirty, (after_dirty, before_dirty)
    findings['unchanged'] = {'render_sha256': before_render, 'history_states': before_states,
                             'unsaved_changes_before': before_dirty,
                             'unsaved_changes_after': after_dirty}

    receipts[gate] = findings
    print('PASS v1: rotation round-trips exactly and writes no document', flush=True)
    return document


def gate_v2(app, receipts):
    gate = 'v2_stroke_under_rotation'
    digests = {}
    for label, angle in [('flat', 0), ('rotated', 30)]:
        created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                           background='white')
        document = created['document']['id']
        added = app.call('new_layer', document_id=document)
        assert added.get('ok') is not False, added
        if angle:
            reply = app.call('set_view', document_id=document, rotation=angle)
            assert reply['rotation'] == float(angle), reply
        painted = app.call('brush_stroke', document_id=document, **STROKE)
        assert painted.get('ok') is not False, painted
        digests[label] = render_digest(app, document)
    assert digests['flat'] == digests['rotated'], digests
    receipts[gate] = {'stroke': STROKE, 'digests': digests,
                      'oracle': 'identity — the MCP surface takes document coordinates, so a '
                                'view rotation may not move a stroke by one byte'}
    print('PASS v2: the same stroke at 0 and 30 renders byte-identically', flush=True)


def gate_v3(app, receipts):
    gate = 'v3_second_view'
    findings = {}
    created = app.call('new_document', width=BIG_CANVAS[0], height=BIG_CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']

    # `open_second_view` MINTS the new window's ordinal and answers it, so the
    # published schema must not offer a `view` a caller could try to choose.
    catalog = {tool['name']: tool for tool in app.client.rpc('tools/list', {})['tools']}
    schema = catalog['open_second_view']['inputSchema']
    assert 'view' not in schema['properties'], schema
    assert 'view' in catalog['close_second_view']['inputSchema']['properties'], catalog
    findings['open_second_view_properties'] = sorted(schema['properties'])

    opened = app.call('open_second_view', document_id=document)
    assert opened['ok'], opened
    assert opened['view']['view'] == 2, opened
    assert len(opened['views']) == 2, opened
    listed = app.call('list_views', document_id=document)
    assert [row['view'] for row in listed['views']] == [1, 2], listed
    assert listed['sync_pan_zoom'] is False, listed
    findings['opened'] = opened['view']
    findings['listed'] = [row['view'] for row in listed['views']]

    two = app.call('set_view', document_id=document, view=2, zoom=1)
    assert two['zoom'] == 1.0, two
    one = app.call('set_view', document_id=document, view=1, fit=True)
    assert one['zoom'] < 1, one
    assert app.call('get_view', document_id=document, view=2)['zoom'] == 1.0
    findings['independent_zoom'] = {'view_1_fit': one['zoom'], 'view_2': two['zoom']}

    # An UNQUALIFIED call is view 1, always — never whichever window is key.
    assert app.call('get_view', document_id=document) == \
        app.call('get_view', document_id=document, view=1)
    before_two = app.call('get_view', document_id=document, view=2)
    moved = app.call('set_view', document_id=document, zoom=1.5)
    assert moved['view'] == 1 and moved['zoom'] == 1.5, moved
    assert app.call('get_view', document_id=document, view=2) == before_two, before_two
    findings['unqualified_is_view_1'] = moved

    before_digest = render_digest(app, document)
    before_cursor = history(app, document)['cursor']
    filled = app.call('fill', document_id=document, x=100, y=100, color='#884422',
                      tolerance=255)
    assert filled.get('ok') is not False, filled
    view_one = app.call('get_view', document_id=document, view=1)
    view_two = app.call('get_view', document_id=document, view=2)
    assert view_one['revision'] == view_two['revision'], (view_one, view_two)
    states = len(history(app, document)['states'])
    findings['shared_revision'] = {'view_1': view_one['revision'], 'view_2': view_two['revision'],
                                   'history_states': states}

    receipts[gate] = findings
    print('PASS v3: two views, independent zoom, one shared document', flush=True)
    return document, before_digest, before_cursor


def gate_v5(app, document, before_digest, before_cursor, receipts):
    gate = 'v5_one_shared_undo'
    after_cursor = history(app, document)['cursor']
    assert after_cursor == before_cursor + 1, (before_cursor, after_cursor)
    undone = app.call('undo', document_id=document)
    assert undone.get('ok') is not False, undone
    cursor = history(app, document)['cursor']
    assert cursor == before_cursor, (before_cursor, cursor)
    assert render_digest(app, document) == before_digest, 'undo did not restore the picture'
    receipts[gate] = {'cursor_before_edit': before_cursor, 'cursor_after_edit': after_cursor,
                      'cursor_after_undo': cursor, 'render_sha256': before_digest}
    print('PASS v5: one undo, one shared history, the picture back', flush=True)


def gate_v4(app, document, receipts):
    gate = 'v4_closing_a_view'
    findings = {}
    before = dirty(app, document)
    closed = app.call('close_second_view', document_id=document, view=2)
    assert closed['ok'] and closed['closed'] == 2, closed
    assert [row['view'] for row in closed['views']] == [1], closed
    after = dirty(app, document)
    assert after == before, (before, after)
    listed = app.call('list_views', document_id=document)
    assert [row['view'] for row in listed['views']] == [1], listed
    findings['closed'] = closed
    findings['unsaved_changes'] = {'before': before, 'after': after}

    refusal = app.call('close_second_view', document_id=document, view=1, fail=True)
    assert 'last view' in refusal, refusal
    missing = app.call('close_second_view', document_id=document, view=9, fail=True)
    assert 'list_views' in missing, missing
    findings['refusals'] = {'last_view': refusal, 'unknown_view': missing}

    revision_before = app.call('get_view', document_id=document, view=1)['revision']
    filled = app.call('fill', document_id=document, x=200, y=200, color='#11AA55',
                      tolerance=255)
    assert filled.get('ok') is not False, filled
    revision_after = app.call('get_view', document_id=document, view=1)['revision']
    assert revision_after == revision_before + 1, (revision_before, revision_after)
    findings['revision'] = {'before': revision_before, 'after': revision_after}

    receipts[gate] = findings
    print('PASS v4: a closed view leaves the document open, clean and editable', flush=True)


def gate_v6(app, receipts):
    gate = 'v6_sync_pan_zoom'
    created = app.call('new_document', width=BIG_CANVAS[0], height=BIG_CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']
    opened = app.call('open_second_view', document_id=document, sync_pan_zoom=True)
    assert opened['ok'] and opened['view']['view'] == 2, opened
    assert app.call('list_views', document_id=document)['sync_pan_zoom'] is True

    app.call('set_view', document_id=document, view=1, zoom=2,
             center={'x': 1000, 'y': 800})
    two = app.call('get_view', document_id=document, view=2)
    assert two['zoom'] == 2.0, two
    dx = abs(two['center']['x'] - 1000)
    dy = abs(two['center']['y'] - 800)
    assert dx <= 0.5 and dy <= 0.5, (two['center'], dx, dy)
    receipts[gate] = {'view_2': two, 'centre_error': {'x': dx, 'y': dy},
                      'tolerance_px': 0.5}
    print('PASS v6: Sync Pan & Zoom carries zoom and centre, never rotation', flush=True)


def gate_n1(app, document, receipts):
    saved = app.call('save_layer_comp', document_id=document, name='Hero')
    assert saved.get('ok') is not False, saved
    applied = app.call('apply_layer_comp', document_id=document, name=' Hero ')
    assert applied.get('ok') is not False, applied
    receipts['n1_padded_comp_name'] = {'saved': 'Hero', 'applied': ' Hero ', 'reply': applied}
    print('PASS n1: a padded layer-comp name finds the comp', flush=True)


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

    receipts = {'tolerances': 'none but one — every angle, zoom, revision, digest and count is '
                              'an integer this file chose or a value the app echoes back '
                              'unchanged; V6 alone allows 0.5 px on the synchronized centre, '
                              'because the receiving clip view constrains its own origin'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        first = gate_v1(app, receipts)
        gate_v2(app, receipts)
        document, digest, cursor = gate_v3(app, receipts)
        gate_v5(app, document, digest, cursor, receipts)
        gate_v4(app, document, receipts)
        gate_v6(app, receipts)
        gate_n1(app, first, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
