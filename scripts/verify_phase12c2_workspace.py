#!/usr/bin/env python3
"""The Phase 12C-2 Navigator / workspace MCP drive, on a built Release app.

Four gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no tolerance is widened afterwards. The instance is owned by this
file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads — `DR_SHORTCUTS_DIR` and
`DR_WORKSPACES_DIR` included) and is terminated by its own context manager; no
Doctored process this file did not start is ever signalled.

- **V1 view state writes nothing.** A 1600 x 1200 `new_document`;
  `set_view {zoom: 4, center: {800, 600}}` answers EXACTLY what `get_view`
  answers and exactly what was asked for; `set_view {zoom: 0.5}` moves the
  zoom; and across all of it the `render` PNG's SHA-256 is unchanged,
  `get_document.unsaved_changes` is the same as before the call (a
  `new_document` is born dirty on purpose, so "unchanged" is the honest
  question) and `list_history` has the same number of states. `zoom: 100` and a centre outside the canvas are refused BY
  NAME.
- **W1 workspace layouts.** A seeded library (a valid "B" at its canonical
  digest name, written HERE, plus one malformed file; B's 900 x 700 frame is
  anchored on the window's own frame so `WindowFrameFit` keeps it exactly on a
  machine whose display does not start at the global origin):
  `save_workspace A`
  captures the window as it is; `restore_workspace B` reports an `applied`
  capture equal to B field for field; `save_workspace C` captures the same
  numbers under a new name; `restore_workspace A` then `save_workspace D`
  captures A's numbers again. `list_workspaces` lists A-D and exactly one
  `not_json` broken row; `delete_workspace {file}` clears it and
  `delete_workspace {name}` removes B; both-or-neither is refused; and
  `save_workspace A` without `replace` is refused.
- **W2 frame recovery.** Three layouts carrying A's panel, view and
  tool, with an off-screen frame: A's size at x = 60000, A's size at
  y = -3000, and 9000 x 3000. Each restored `applied.window` lies wholly
  inside one of the screens' visible frames (read here through
  `osascript -l JavaScript`), and the first two keep A's width and height.
  `restore_workspace A` ends the gate.
- **N2 the unreadable ICC.** `new_document {profile: <work>/missing.icc}` is
  refused with a message that NAMES that path.

Nothing here derives an expectation from Doctored: the sizes, the layout's
fields, the digest file names and the index sets are computed in this file.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from support import HostApp

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
# There are none. Pixel sizes, zoom, centres, rectangles, layout fields, file
# names and digests are EXACT: every one of them is either an integer this
# file chose or a value the app is asked to echo back unchanged.
MALFORMED = 'zz-malformed.json'
CANVAS = (1600, 1200)

def layout_b(anchor):
    """The seeded layout, anchored on `anchor` — the FRAME of the window this
    drive found: its left edge, and centred on its middle, so the 900 x 700
    frame sits inside the screen that window was centred on.

    A literal (80, 80) is not machine-independent: `applyWorkspace` runs the
    frame through `WindowFrameFit`, and on a machine whose display does not
    start at the global origin a frame outside the usable area is moved onto
    a display — correct behaviour, and not something a gate should call a
    failure. The HEIGHT is derived for the same reason, one axis over: 700 pt
    on a display whose usable height is shorter would be clamped too, so B is
    at most as tall as the anchor — a window that is on screen by definition —
    and centred inside it. The panel, the chrome and the tool stay exactly
    what this file chose.
    """
    height = min(700, int(anchor['height']))
    return {
        'schema_version': 1,
        'name': 'B',
        'window': {'x': anchor['x'],
                   'y': round(anchor['y'] + anchor['height'] / 2 - height / 2),
                   'width': 900, 'height': height},
        'panel': {'visible': True, 'tab': 'navigator', 'width': 420},
        'view': {'rulers': False, 'grid': True, 'snap': False},
        'tool': 'brush',
    }


def digest_name(name):
    """A preference library's canonical file name for `name` — sha256 of the
    en_US_POSIX case-fold, which for ASCII IS `str.lower`."""
    assert name.isascii(), name
    return hashlib.sha256(name.lower().encode()).hexdigest() + '.json'


def render_digest(app, document):
    """The SHA-256 of `render`'s PNG bytes. The raw RPC, because the shared
    client keeps only the text part of an image reply."""
    result = app.client.rpc('tools/call', {'name': 'render',
                                           'arguments': {'document_id': document}})
    assert not result.get('isError'), result
    data = next(c['data'] for c in result['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def history_length(app, document):
    return len(app.call('list_history', document_id=document)['states'])


def without(layout, *keys):
    return {key: value for key, value in layout.items() if key not in keys}


# ------------------------------------------------------------------ gates --

def gate_v1(app, receipts):
    gate = 'v1_view_state'
    findings = {}
    created = app.call('new_document', width=CANVAS[0], height=CANVAS[1], depth=8,
                       background='white')
    document = created['document']['id']
    assert created['document']['width'] == CANVAS[0], created
    assert created['document']['height'] == CANVAS[1], created

    before_render = render_digest(app, document)
    before_history = history_length(app, document)
    # A `new_document` is born DIRTY on purpose (`ImageDocument.makeNew` counts
    # the change, so the window offers to save its one layer), so the honest
    # question is whether set_view CHANGES the flag, not what it starts at.
    before_dirty = app.call('get_document', document_id=document)['unsaved_changes']

    request = {'zoom': 4, 'center': {'x': 800, 'y': 600}}
    reply = app.call('set_view', document_id=document, **request)
    read_back = app.call('get_view', document_id=document)
    # The reply IS the read-back state, minus the two keys set_view adds.
    assert without(reply, 'ok', 'note') == read_back, (reply, read_back)
    assert reply['zoom'] == request['zoom'], reply
    assert reply['center'] == request['center'], reply
    assert reply['canvas'] == {'width': CANVAS[0], 'height': CANVAS[1]}, reply
    findings['set_view_exact'] = {'request': request, 'reply': reply}

    out = app.call('set_view', document_id=document, zoom=0.5)
    assert out['zoom'] == 0.5, out
    assert app.call('get_view', document_id=document)['zoom'] == 0.5, out
    findings['zoom_out'] = out

    assert render_digest(app, document) == before_render, 'set_view changed the render'
    summary = app.call('get_document', document_id=document)
    assert summary['unsaved_changes'] == before_dirty, (summary['unsaved_changes'], before_dirty)
    assert history_length(app, document) == before_history, 'set_view added a history state'
    findings['unchanged'] = {'render_sha256': before_render,
                             'history_states': before_history,
                             'unsaved_changes_before': before_dirty,
                             'unsaved_changes_after': summary['unsaved_changes']}

    zoom_refusal = app.call('set_view', document_id=document, zoom=100, fail=True)
    assert 'zoom' in zoom_refusal, zoom_refusal
    centre_refusal = app.call('set_view', document_id=document,
                              center={'x': CANVAS[0] + 10, 'y': 10}, fail=True)
    assert 'center' in centre_refusal, centre_refusal
    empty_refusal = app.call('set_view', document_id=document, fail=True)
    assert 'set_view' in empty_refusal, empty_refusal
    findings['refusals'] = {'zoom_100': zoom_refusal, 'center_outside': centre_refusal,
                            'nothing_to_set': empty_refusal}

    receipts[gate] = findings
    print('PASS v1: set_view round-trips exactly and writes no document', flush=True)
    return document


def gate_w1(app, document, workspaces, receipts):
    gate = 'w1_workspace_layouts'
    findings = {}

    saved_a = app.call('save_workspace', document_id=document, name='A')
    assert saved_a['ok'], saved_a
    a = saved_a['workspace']
    assert a['name'] == 'A', a
    findings['a'] = a

    # B is written HERE, by this file, at its canonical digest name — now that
    # the window's own origin is known. The library is read per call, so a
    # file written after launch is what the next call sees.
    seeded = layout_b(a['window'])
    (workspaces / digest_name('B')).write_text(json.dumps(seeded))
    receipts['seeded_workspace'] = seeded

    restored = app.call('restore_workspace', document_id=document, name='B')
    assert restored['ok'], restored
    assert restored['workspace'] == seeded, restored['workspace']
    # `applied` is a fresh capture taken AFTER the apply: every field of B has
    # to have landed on the real window.
    assert restored['applied'] == seeded, (restored['applied'], seeded)
    findings['restored_b'] = restored

    saved_c = app.call('save_workspace', document_id=document, name='C')
    assert saved_c['ok'], saved_c
    assert without(saved_c['workspace'], 'name') == without(seeded, 'name'), saved_c
    findings['c'] = saved_c['workspace']

    # Restoring A last is deliberate: the VIEW CHROME (rulers, grid, snap,
    # panel width) is an app-wide UserDefaults preference with no MCP twin to
    # read it back, so A — captured before anything was applied — is what puts
    # this machine's own settings back when the drive ends.
    back = app.call('restore_workspace', document_id=document, name='A')
    assert back['ok'], back
    assert back['applied'] == a, (back['applied'], a)
    saved_d = app.call('save_workspace', document_id=document, name='D')
    assert without(saved_d['workspace'], 'name') == without(a, 'name'), (saved_d, a)
    findings['d'] = saved_d['workspace']

    listing = app.call('list_workspaces')
    names = [row['name'] for row in listing['workspaces']]
    assert names == ['A', 'B', 'C', 'D'], names
    broken = listing['broken']
    assert len(broken) == 1, broken
    assert broken[0]['file'] == MALFORMED, broken
    assert broken[0]['code'] == 'not_json', broken
    assert broken[0]['repairable'] is False, broken
    assert broken[0]['reason'], broken
    findings['listing'] = {'names': names, 'broken': broken}

    removed = app.call('delete_workspace', file=MALFORMED)
    assert removed['ok'] and removed['file'] == MALFORMED, removed
    assert not (workspaces / MALFORMED).exists(), 'the malformed file survived'
    assert app.call('list_workspaces')['broken'] == [], 'the broken row survived'

    dropped = app.call('delete_workspace', name='B')
    assert dropped['ok'] and dropped['file'] == digest_name('B'), dropped
    assert not (workspaces / digest_name('B')).exists(), 'B survived its deletion'
    assert [row['name'] for row in app.call('list_workspaces')['workspaces']] == ['A', 'C', 'D']
    findings['deletes'] = {'by_file': removed, 'by_name': dropped}

    both = app.call('delete_workspace', name='A', file=digest_name('A'), fail=True)
    assert 'exactly one' in both, both
    neither = app.call('delete_workspace', fail=True)
    assert 'exactly one' in neither, neither
    occupied = app.call('save_workspace', document_id=document, name='A', fail=True)
    assert 'already exists' in occupied, occupied
    findings['refusals'] = {'both': both, 'neither': neither, 'no_replace': occupied}

    receipts[gate] = findings
    print('PASS w1: a layout saved, restored, re-captured, listed and deleted', flush=True)


def visible_frames():
    """Every screen's visible frame, read by this file (not by Doctored)
    through AppKit in `osascript -l JavaScript`."""
    script = ("ObjC.import('AppKit'); var screens = $.NSScreen.screens; var out = []; "
              "for (var i = 0; i < screens.count; i++) { "
              "var f = screens.objectAtIndex(i).visibleFrame; "
              "out.push({x: f.origin.x, y: f.origin.y, width: f.size.width, "
              "height: f.size.height}); } JSON.stringify(out)")
    return json.loads(subprocess.check_output(['osascript', '-l', 'JavaScript', '-e', script],
                                              text=True))


def inside(window, frame):
    return (frame['x'] <= window['x'] and frame['y'] <= window['y']
            and window['x'] + window['width'] <= frame['x'] + frame['width']
            and window['y'] + window['height'] <= frame['y'] + frame['height'])


def gate_w2(app, document, workspaces, receipts):
    gate = 'w2_frame_recovery'
    screens = visible_frames()
    assert screens, 'no screens reported'
    a = receipts['w1_workspace_layouts']['a']
    frame = a['window']
    windows = {
        'W2 Far X': {'x': 60000, 'y': frame['y'], 'width': frame['width'],
                     'height': frame['height']},
        'W2 Far Y': {'x': frame['x'], 'y': -3000, 'width': frame['width'],
                     'height': frame['height']},
        'W2 Huge': {'x': frame['x'], 'y': frame['y'], 'width': 9000, 'height': 3000},
    }
    seeded, applied = {}, {}
    for name, window in windows.items():
        layout = {'schema_version': 1, 'name': name, 'window': window,
                  'panel': a['panel'], 'view': a['view'], 'tool': a['tool']}
        (workspaces / digest_name(name)).write_text(json.dumps(layout))
        seeded[name] = layout
        restored = app.call('restore_workspace', document_id=document, name=name)
        assert restored['ok'], restored
        landed = restored['applied']['window']
        assert any(inside(landed, screen) for screen in screens), (name, landed, screens)
        if name != 'W2 Huge':
            assert (landed['width'], landed['height']) == (frame['width'], frame['height']), \
                (name, landed, frame)
        applied[name] = landed
    back = app.call('restore_workspace', document_id=document, name='A')
    assert back['ok'], back
    assert back['applied'] == a, (back['applied'], a)
    receipts[gate] = {'screens': screens, 'seeded': seeded, 'applied': applied,
                      'restored_a': back['applied']}
    print('PASS w2: off-screen frames land wholly on one display', flush=True)


def gate_n2(app, folder, receipts):
    missing = folder / 'missing.icc'
    assert not missing.exists(), missing
    refusal = app.call('new_document', width=10, height=10, profile=str(missing), fail=True)
    assert str(missing) in refusal, refusal
    receipts['n2_unreadable_icc'] = {'path': str(missing), 'refusal': refusal}
    print('PASS n2: an unreadable ICC path is refused naming the file', flush=True)


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

    # HostApp redirects every preference library into <folder>/app/<name>. The
    # malformed file is seeded before launch; the valid "B" is written by
    # gate_w1, once the window's own origin is known.
    workspaces = folder / 'app' / 'workspaces'
    workspaces.mkdir(parents=True, exist_ok=True)
    (workspaces / MALFORMED).write_text('{ this is not json')

    receipts = {'tolerances': 'none — every number is an integer this file chose, a value the '
                              'app echoes back unchanged, or the window origin the app itself '
                              'reported (see layout_b)'}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        document = gate_v1(app, receipts)
        gate_w1(app, document, workspaces, receipts)
        gate_w2(app, document, workspaces, receipts)
        gate_n2(app, folder, receipts)
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
