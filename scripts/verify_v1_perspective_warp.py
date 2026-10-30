#!/usr/bin/env python3
"""MCP drive for Edit > Perspective Warp: the `perspective_warp` tool end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB
working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process. Every expected value is this file's own: a whole-pixel translation
is an exact copy (Catmull-Rom weights 1 and 0), so the oracle is the layer's
own sample read BEFORE the warp at the shifted position, and a straightened
edge's two warped corners share their y (or x) exactly.

The fixture P is a 96x80 picture: a 4 px transparent margin around an opaque
field whose every pixel is a hash of its position. At 16 bits the file is
promoted (every code widened by 257), at 32 bits converted to float
(`convert_precision` mode rgba32).

At 8, 16 and 32 bits:
- IDENTITY: unmoved corners answer `changed: false` and add no history step.
- TRANSLATION: one quad whose four corners move by (6, 4) is one history
  step and carries the whole layer: every sampled pixel of the moved field,
  near the quad and far from it, equals the pre-warp sample 6 px left and
  4 px up, and a pixel outside the moved layer is transparent. The reply's
  quads are the request's, with no shared edge.
- STRAIGHTEN AND JOIN: two quads joined along an edge, the first's top edge
  tilted and straightened (`auto`): the reply names one shared edge, and the
  straightened edge's warped corners have the same y.
- SELECTION FLOAT: with a rectangle selected, a quad moved by (20, 0) warps
  only the float (`write_scope: selection`): a landed pixel is the source
  20 px left, a selected pixel nothing lands on is a hole (alpha 0), and a
  pixel outside the selection and the landing is unchanged.
- UNDO: `undo` restores the source.
- REPLAY: one recorded `perspective_warp` step (a keystone and a straighten)
  replays on a second copy to identical samples.
- REFUSALS, each by its sentence and with no history step: no quads, 65
  quads, a malformed quad, a degenerate quad, a folding quad, joined corners
  warped apart, a bad straighten entry, a result past the pixel limit, and
  the float form over an empty selection.
At 8 and 16 bits:
- DESCRIBED: a text layer takes the warp and drops its description in that
  one step; `undo` restores the text layer.

Usage: python3 scripts/verify_v1_perspective_warp.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

WIDTH, HEIGHT = 96, 80
MARGIN = 4
SQUARE = [(20, 20), (60, 20), (60, 50), (20, 50)]


class App:
    """The one owned instance, on its own scratch socket."""

    def __init__(self, folder):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.process = None
        self.log = None
        self.client = None

    def __enter__(self):
        executable = app_under_test.default_app_executable()
        app_under_test.require_fresh_app(executable)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.socket = scratch_socket()
        environment = dict(
            os.environ, DR_BACKGROUND='1', DR_AGENT_SOCKET=str(self.socket), ANTHROPIC_API_KEY='dummy',
            DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES='2147483648',
            DR_ACTIONS_DIR=str(self.folder / 'actions'),
            DR_RECOVERY_DIR=str(self.folder / 'recovery'),
            DR_PASTEBOARD_NAME='rz-verify-v1-perspective-warp',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = Client(self.socket)
        try:
            wait_ready(self.client, self.process, 30, self.log.name)
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_):
        # Only the Popen process this instance launched is ever signalled.
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if self.log is not None:
            self.log.close()
            self.log = None
        remove_scratch(self.socket)

    def call(self, tool, **arguments):
        return self.client.call(tool, **arguments)

    def refused(self, tool, *needles, **arguments):
        text = self.client.call(tool, fail=True, **arguments)
        for needle in needles:
            assert needle in text, (tool, needle, text)
        return text


# ------------------------------------------------------------- plumbing --


def inside(x, y):
    return MARGIN <= x < WIDTH - MARGIN and MARGIN <= y < HEIGHT - MARGIN

def source(x, y):
    """P's pixel at (x, y) as 8-bit RGBA."""
    if not inside(x, y):
        return [0, 0, 0, 0]
    h = ((x * 73856093) ^ (y * 19349663)) & 0xffffffff
    h = ((h ^ (h >> 13)) * 0x5bd1e995) & 0xffffffff
    h ^= h >> 15
    return [h & 255, (h >> 8) & 255, (h >> 16) & 255, 255]


def fixture(folder, tag):
    pixels = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            pixels += bytes(source(x, y))
    path = Path(folder) / f'p-{tag}.png'
    png(path, WIDTH, HEIGHT, bytes(pixels))
    return path


def opened(app, folder, tag, depth):
    document = app.call('open_document', path=str(fixture(folder, tag)))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    if depth == 32:
        app.call('convert_precision', document_id=document, mode='rgba32', structure='preserve')
    return document


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def sample(app, document, depth, x, y, layer=0):
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    return reply['center_rgba32'] if depth == 32 else reply['center_rgba_codes']


def same(got, want, depth):
    if depth == 32:
        return all(abs(g - w) <= 1e-6 for g, w in zip(got, want))
    return list(got) == list(want)


def transparent(got):
    return got[3] == 0


def quad(corners, dx=0, dy=0, to=None):
    """A request quad: `corners` (four (x, y)) moved by (dx, dy), or to `to`."""
    moved = to if to is not None else [(x + dx, y + dy) for x, y in corners]
    return {'from': [v for p in corners for v in p], 'to': [v for p in moved for v in p]}


# ---------------------------------------------------------------- cases --


def identity(app, depth, folder):
    document = opened(app, folder, f'identity-{depth}', depth)
    start = cursor(app, document)
    reply = app.call('perspective_warp', document_id=document, quads=[quad(SQUARE)])
    assert reply['ok'] and reply['changed'] is False, reply
    assert cursor(app, document) == start, 'unmoved corners add no step'
    return dict(changed=reply['changed'])

def translation(app, depth, folder):
    document = opened(app, folder, f'translation-{depth}', depth)
    # The moved field, on the canvas: 6 px in from its left and top edges.
    points = [(x, y) for y in range(MARGIN + 6, HEIGHT, 3) for x in range(MARGIN + 8, WIDTH, 3)]
    before = {(x, y): sample(app, document, depth, x - 6, y - 4) for x, y in points}
    start = cursor(app, document)
    request = quad(SQUARE, 6, 4)
    reply = app.call('perspective_warp', document_id=document, quads=[request])
    assert reply['ok'] and reply['changed'] is True and reply['layer'] == 0, reply
    assert reply['write_scope'] == 'whole_layer', reply
    storage = {8: 'rgba8_unorm', 16: 'rgba16_unorm', 32: 'rgba32_float'}[depth]
    assert reply['storage'] == storage, reply
    assert reply['quads'] == [request] and reply['shared_edges'] == [], reply
    assert cursor(app, document) == start + 1, 'one history step'
    far = 0
    for x, y in points:
        got = sample(app, document, depth, x, y)
        assert same(got, before[(x, y)], depth), ((x, y), got, before[(x, y)])
        if not (20 <= x - 6 <= 60 and 20 <= y - 4 <= 50):
            far += 1
    assert far > 100, far
    for x, y in [(2, 2), (6, 5), (93, 3)]:
        got = sample(app, document, depth, x, y)
        assert transparent(got), ('outside', x, y, got)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    for x, y in points[::7]:
        got = sample(app, document, depth, x - 6, y - 4)
        assert same(got, before[(x, y)], depth), ('undo', x, y, got)
    return dict(samples=len(points), far=far)


def straighten_and_join(app, depth, folder):
    document = opened(app, folder, f'join-{depth}', depth)
    left = [(20, 20), (48, 20), (48, 56), (20, 56)]
    right = [(48, 20), (76, 20), (76, 56), (48, 56)]
    reply = app.call(
        'perspective_warp', document_id=document,
        quads=[quad(left, to=[(20, 18), (48, 22), (50, 56), (20, 56)]),
               quad(right, to=[(48, 22), (74, 20), (76, 58), (50, 56)])],
        straighten=[{'quad': 0, 'edge': 0}])
    assert reply['changed'] is True, reply
    assert [sorted(edge) for edge in reply['shared_edges']] == [[[48, 20], [48, 56]]], reply
    to = reply['quads'][0]['to']
    assert to[1] == to[3] == 20, ('the straightened edge is level at the mean', to)
    assert reply['quads'][1]['to'][1] == 20, ('the joined corner moved with it', reply['quads'])
    return dict(level=to[1])


def selection_float(app, depth, folder):
    document = opened(app, folder, f'float-{depth}', depth)
    app.call('select_rect', document_id=document, x=30, y=28, width=24, height=24)
    landed = [(x + 20, y) for x in range(33, 52, 3) for y in range(31, 50, 3)]
    before = {p: sample(app, document, depth, p[0] - 20, p[1]) for p in landed}
    kept = [(20, 40), (24, 30), (30, 56), (40, 61)]
    kept_before = {p: sample(app, document, depth, *p) for p in kept}
    start = cursor(app, document)
    reply = app.call('perspective_warp', document_id=document,
                     quads=[quad([(28, 26), (56, 26), (56, 54), (28, 54)], 20, 0)])
    assert reply['ok'] and reply['changed'] is True, reply
    assert reply['write_scope'] == 'selection', reply
    assert cursor(app, document) == start + 1, 'one history step'
    for p in landed:
        got = sample(app, document, depth, *p)
        assert same(got, before[p], depth), ('landed', p, got, before[p])
    for p in kept:
        got = sample(app, document, depth, *p)
        assert same(got, kept_before[p], depth), ('kept', p, got, kept_before[p])
    hole = sample(app, document, depth, 34, 40)
    assert transparent(hole), ('hole', hole)
    return dict(landed=len(landed))


def replay(app, depth, folder):
    document = opened(app, folder, f'replay-{depth}', depth)
    second = opened(app, folder, f'replay-copy-{depth}', depth)
    name = f'v1 perspective warp {depth}'
    app.call('start_recording', discard=True)
    reply = app.call('perspective_warp', document_id=document,
                     quads=[quad(SQUARE, to=[(24, 21), (57, 19), (66, 54), (16, 52)])],
                     straighten=[{'quad': 0, 'edge': 0}])
    assert reply['changed'] is True, reply
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['perspective_warp'], action
    points = [(x, y) for y in range(0, HEIGHT, 5) for x in range(0, WIDTH, 5)]
    recorded = [sample(app, document, depth, x, y) for x, y in points]
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    replayed = [sample(app, second, depth, x, y) for x, y in points]
    assert replayed == recorded, 'the replay differs'
    return dict(samples=len(points))


def refusals(app, depth, folder):
    document = opened(app, folder, f'refusals-{depth}', depth)
    start = cursor(app, document)
    good = quad(SQUARE, 5, 0)
    seen = {}

    def refuse(name, needle, **arguments):
        seen[name] = app.refused('perspective_warp', needle, document_id=document, **arguments)

    refuse('no quads', 'Perspective Warp needs at least one quad.', quads=[])
    refuse('too many', 'Perspective Warp takes at most 64 quads.', quads=[good] * 65)
    refuse('malformed',
           'Perspective Warp quad 1 needs from and to as eight finite numbers within 1000000.',
           quads=[good, {'from': [1, 2], 'to': [1, 2]}])
    refuse('degenerate', 'Perspective Warp quad 0 is degenerate: its corners must form a convex '
           'quad with no three in a line.',
           quads=[quad([(20, 20), (40, 20), (60, 20), (40, 50)])])
    refuse('folds', 'Perspective Warp quad 0 folds: its warped corners must form a convex quad '
           'with no three in a line, turning the same way.',
           quads=[quad(SQUARE, to=[(20, 20), (60, 50), (60, 20), (20, 50)])])
    refuse('apart', 'Perspective Warp quads 0 and 1 share a corner but warp it to different '
           'points.',
           quads=[quad(SQUARE), quad([(60, 20), (80, 20), (80, 50), (60, 50)],
                                     to=[(61, 20), (80, 20), (80, 50), (60, 50)])])
    refuse('straighten', 'Perspective Warp straighten 0 must name a quad, an edge from 0 to 3 '
           'and a direction.', quads=[good], straighten=[{'quad': 0, 'edge': 4}])
    refuse('pixel limit', 'Perspective Warp would make the layer larger than the pixel limit.',
           quads=[quad(SQUARE, to=[(-500000, -500000), (500000, -500000), (500000, 500000),
                                   (-500000, 500000)])])
    assert cursor(app, document) == start, 'a refusal leaves no step'
    app.call('select_rect', document_id=document, x=0, y=0, width=3, height=3)
    seen['float empty'] = app.refused(
        'perspective_warp',
        'The selection holds no pixels to perspective warp.',
        document_id=document, quads=[good])
    app.call('deselect', document_id=document)
    return seen


def described(app, depth, folder):
    document = opened(app, folder, f'described-{depth}', 8)
    app.call('add_text_layer', document_id=document, text='Planes', x=10, y=30, size=26)
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')

    def text_rows():
        rows = app.call('get_document', document_id=document)['layers']
        return [index for index, row in enumerate(rows) if 'text' in row]

    texts = text_rows()
    assert len(texts) == 1, texts
    layer = texts[0]
    start = cursor(app, document)
    reply = app.call('perspective_warp', document_id=document, layer=layer,
                     quads=[quad([(10, 20), (60, 20), (60, 60), (10, 60)],
                                 to=[(12, 22), (58, 18), (64, 62), (8, 58)])])
    assert reply['changed'] is True and reply['layer'] == layer, reply
    if depth == 8:
        assert reply.get('rasterized_text') is True, reply
    assert cursor(app, document) == start + 1, 'the warp and the dropped description are one step'
    assert text_rows() == [], 'the warped layer still claims to be text'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert text_rows() == [layer], 'undo restores the text layer'
    return dict(layer=layer)


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-perspective-warp-'))
    output.mkdir(parents=True, exist_ok=True)
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output) as app:
        for depth in (8, 16, 32):
            run(f'identity {depth}-bit', lambda: identity(app, depth, output))
            run(f'translation and undo {depth}-bit', lambda: translation(app, depth, output))
            run(f'straighten and join {depth}-bit',
                lambda: straighten_and_join(app, depth, output))
            run(f'selection float {depth}-bit', lambda: selection_float(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))
        for depth in (8, 16):
            run(f'described {depth}-bit', lambda: described(app, depth, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
