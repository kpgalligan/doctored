#!/usr/bin/env python3
"""MCP drive for Window ▸ Match Zoom, Match Location and Match Zoom and Location (`match_views`),
against the Debug app.

The rule this file checks, computed here: zoom — every other view takes the source view's
magnification; location — every other view centres on the same FRACTION of its own canvas as the
source's centre is of the source's canvas; rotation is never matched. The targets are every view of
every open document but the source, the source document's own other views included.

- **Zoom only** (`location: false`): every other view reads the source's zoom and keeps its centre
  (within 1 px: the receiving clip view constrains its own origin).
- **Location only** (`zoom: false`): the 1600 × 1200 document centres on (400, 300) for a source at
  (200, 150) of 800 × 600, the source's second view on (200, 150); each keeps its zoom. A mutant
  that matched the absolute pixel would leave the large document at (200, 150).
- **Both**: zoom and fraction together. A third document's view turned 30° takes the zoom and
  keeps its rotation (its centre is not measured: a turned view reads back the middle of its
  clip's bounding box in canvas pixels).
- **Nothing written**: every document's render digest, `list_history` and `unsaved_changes` are
  unchanged by every call.
- **Refusals**: both flags false, and a non-boolean flag, by name.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check; exits
non-zero unless every line is PASS.
"""
import argparse
import hashlib
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402

SMALL = (800, 600)
LARGE = (1600, 1200)
TOLERANCE_PX = 1.0
RESULTS = []


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def digest(app, doc):
    reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {'document_id': doc}})
    assert not reply.get('isError'), reply
    data = next(c['data'] for c in reply['content'] if c['type'] == 'image')
    return hashlib.sha256(data.encode()).hexdigest()


def snapshot(app, docs):
    return {doc: (digest(app, doc), len(app.call('list_history', document_id=doc)['states']),
                  app.call('get_document', document_id=doc)['unsaved_changes']) for doc in docs}


def view(app, doc, ordinal):
    return app.call('get_view', document_id=doc, view=ordinal)


def near(point, x, y):
    return abs(point['x'] - x) <= TOLERANCE_PX and abs(point['y'] - y) <= TOLERANCE_PX


def run(app):
    small = app.call('new_document', width=SMALL[0], height=SMALL[1], depth=8,
                     background='white')['document']['id']
    large = app.call('new_document', width=LARGE[0], height=LARGE[1], depth=8,
                     background='white')['document']['id']
    # A third document whose view is turned 30°: it takes the zoom and keeps its rotation. Its
    # read-back centre is the middle of the turned clip's bounding box in canvas pixels, which is
    # not the scrolled-to point to within a pixel, so the centre rule is measured on the unturned
    # views.
    turned = app.call('new_document', width=SMALL[0], height=SMALL[1], depth=8,
                      background='white')['document']['id']
    opened = app.call('open_second_view', document_id=small)
    assert opened['view']['view'] == 2, opened
    docs = [small, large, turned]
    before = snapshot(app, docs)

    # Distinct starting states, so every assertion below sees a move.
    app.call('set_view', document_id=small, view=2, zoom=1, center={'x': 600, 'y': 450})
    app.call('set_view', document_id=large, zoom=0.5, center={'x': 1200, 'y': 900})
    app.call('set_view', document_id=turned, zoom=0.5, rotation=30)
    app.call('set_view', document_id=small, view=1, zoom=4, center={'x': 200, 'y': 150})
    source = view(app, small, 1)
    assert source['zoom'] == 4.0 and near(source['center'], 200, 150), source
    second_before = view(app, small, 2)
    large_before = view(app, large, 1)

    # Zoom only.
    reply = app.call('match_views', document_id=small, view=1, location=False)
    assert reply['source']['view'] == 1 and reply['source']['zoom'] == 4.0, reply
    assert len(reply['matched']) == 3, reply
    second, big, tilted = view(app, small, 2), view(app, large, 1), view(app, turned, 1)
    assert second['zoom'] == 4.0 and big['zoom'] == 4.0 and tilted['zoom'] == 4.0, \
        (second, big, tilted)
    assert near(second['center'], second_before['center']['x'], second_before['center']['y']), \
        (second, second_before)
    assert near(big['center'], large_before['center']['x'], large_before['center']['y']), \
        (big, large_before)
    assert tilted['rotation'] == 30.0, tilted
    ok('match_views {location: false}: every other view takes zoom 4 and keeps its centre; the '
       'turned view keeps its rotation')

    # Location only, from a fresh zoom on the targets — high enough that the visible rectangle is
    # smaller than the canvas, so the clip view can reach the matched centre.
    app.call('set_view', document_id=small, view=2, zoom=6)
    app.call('set_view', document_id=large, zoom=3)
    reply = app.call('match_views', document_id=small, view=1, zoom=False)
    second, big = view(app, small, 2), view(app, large, 1)
    fraction = (source['center']['x'] / SMALL[0], source['center']['y'] / SMALL[1])
    expected_big = (fraction[0] * LARGE[0], fraction[1] * LARGE[1])
    assert near(big['center'], *expected_big), (big, expected_big)
    assert near(second['center'], source['center']['x'], source['center']['y']), second
    assert second['zoom'] == 6.0 and big['zoom'] == 3.0, (second, big)
    assert view(app, turned, 1)['rotation'] == 30.0
    rows = {(row['document_id'], row['view']): row for row in reply['matched']}
    assert near(rows[(large, 1)]['center'], *expected_big), reply
    ok(f'match_views {{zoom: false}}: the 1600 × 1200 view centres on {expected_big} (the same '
       f'fraction), the second view on the same pixel, each keeps its zoom and rotation')

    # Both.
    app.call('set_view', document_id=small, view=1, zoom=5, center={'x': 600, 'y': 300})
    source = view(app, small, 1)
    reply = app.call('match_views', document_id=small)
    second, big = view(app, small, 2), view(app, large, 1)
    expected_big = (source['center']['x'] / SMALL[0] * LARGE[0],
                    source['center']['y'] / SMALL[1] * LARGE[1])
    assert second['zoom'] == 5.0 and big['zoom'] == 5.0, (second, big)
    assert near(big['center'], *expected_big), (big, expected_big)
    assert near(second['center'], source['center']['x'], source['center']['y']), second
    tilted = view(app, turned, 1)
    assert tilted['zoom'] == 5.0 and tilted['rotation'] == 30.0, tilted
    assert source['rotation'] == 0.0, source
    ok(f'match_views (both): zoom 5 everywhere, the large view at {expected_big}, rotation kept')

    # A source in another document: every view of the other two follows it.
    reply = app.call('match_views', document_id=large, zoom=True, location=True)
    assert {(row['document_id'], row['view']) for row in reply['matched']} == \
        {(small, 1), (small, 2), (turned, 1)}, reply
    ok('match_views from the second document matches every view of the others')

    after = snapshot(app, docs)
    assert after == before, (before, after)
    ok('no render, history or unsaved-changes flag changed')

    refusal = app.client.call('match_views', fail=True, document_id=small, zoom=False,
                              location=False)
    assert 'zoom' in refusal and 'location' in refusal, refusal
    refusal = app.client.call('match_views', fail=True, document_id=small, zoom='yes')
    assert 'zoom must be true or false' in refusal, refusal
    ok('both flags false and a non-boolean flag are refused by name')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-match-views-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    with App(output / 'app', output / 'recovery', interval=600) as app:
        try:
            run(app)
        except Exception as error:
            RESULTS.append(False)
            print(f'FAIL: {type(error).__name__} — {str(error)[:900]} (files: {output})',
                  flush=True)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
