#!/usr/bin/env python3
"""MCP drive for Edit > Puppet Warp: the `puppet_warp` tool end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB
working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process. Every expected value is this file's own: a whole-pixel translation
is an exact copy (Catmull-Rom weights 1 and 0), so the oracle is the layer's
own sample read BEFORE the warp at the shifted position.

The fixture E is a 96x80 picture: transparent, with an opaque ellipse
(centre (48, 40), radii 30 and 24) whose every pixel is a hash of its
position. At 16 bits the file is promoted (every code widened by 257), at 32
bits converted to float (`convert_precision` mode rgba32).

At 8, 16 and 32 bits:
- IDENTITY: unmoved pins answer `changed: false` and add no history step.
- TRANSLATION: five pins moved by (6, 4) are one history step; every sampled
  pixel well inside the moved ellipse equals the pre-warp sample 6 px left
  and 4 px up, and a pixel far outside it is transparent.
- PIN LANDING: the reply's pins are snapped to mesh vertices and each lands
  exactly (6, 4) from its vertex; vertices and triangles are reported.
- SELECTION FLOAT: with a rectangle selected, pins moved by (20, 0) warp only
  the float (`write_scope: selection`): a selected pixel the landing misses is
  a hole (alpha 0), a landed pixel is the source 20 px left, and a pixel
  outside the selection and the landing is unchanged.
- UNDO: `undo` restores the source.
- REPLAY: one recorded `puppet_warp` step replays on a second copy to
  identical samples.
- REFUSALS, each by its sentence and with no history step: no pins, 257
  pins, a pin outside the mesh, two pins on one vertex, a layer with no
  pixels, a result past the pixel limit, a bad mode, density, expansion,
  depth, rotate and point, and the float form over an empty selection.
At 8 and 16 bits:
- DESCRIBED: a text layer takes the warp and drops its description in that
  one step; `undo` restores the text layer.

Usage: python3 scripts/verify_v1_puppet_warp.py [--output DIR]
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
CENTRE, RADII = (48, 40), (30, 24)
PINS = [(48, 40), (36, 40), (60, 40), (48, 32), (48, 48)]


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
            DR_PASTEBOARD_NAME='rz-verify-v1-puppet-warp',
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


def ellipse(x, y):
    """E's ellipse membership at pixel (x, y)'s centre, as the radius ratio."""
    dx = (x + 0.5 - CENTRE[0]) / RADII[0]
    dy = (y + 0.5 - CENTRE[1]) / RADII[1]
    return (dx * dx + dy * dy) ** 0.5

def source(x, y):
    """E's pixel at (x, y) as 8-bit RGBA."""
    if ellipse(x, y) > 1:
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
    path = Path(folder) / f'e-{tag}.png'
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
    return all(v == 0 for v in got[3:4])


def pins(dx, dy, at=PINS):
    return [dict(**{'from': [x, y], 'to': [x + dx, y + dy]}) for x, y in at]


def interior(margin=0.8, stride=3):
    """Pixels well inside E (radius ratio at most `margin`), every `stride`th."""
    return [(x, y) for y in range(0, HEIGHT, stride) for x in range(0, WIDTH, stride)
            if ellipse(x, y) <= margin]


# ---------------------------------------------------------------- cases --


def identity(app, depth, folder):
    document = opened(app, folder, f'identity-{depth}', depth)
    start = cursor(app, document)
    reply = app.call('puppet_warp', document_id=document, pins=pins(0, 0))
    assert reply['ok'] and reply['changed'] is False, reply
    assert cursor(app, document) == start, 'unmoved pins add no step'
    return dict(changed=reply['changed'])

def translation(app, depth, folder):
    document = opened(app, folder, f'translation-{depth}', depth)
    points = [(x + 6, y + 4) for x, y in interior()]
    before = {(x, y): sample(app, document, depth, x - 6, y - 4) for x, y in points}
    start = cursor(app, document)
    reply = app.call('puppet_warp', document_id=document, pins=pins(6, 4), expansion=4)
    assert reply['ok'] and reply['changed'] is True and reply['layer'] == 0, reply
    assert reply['write_scope'] == 'whole_layer', reply
    storage = {8: 'rgba8_unorm', 16: 'rgba16_unorm', 32: 'rgba32_float'}[depth]
    assert reply['storage'] == storage, reply
    assert cursor(app, document) == start + 1, 'one history step'
    for x, y in points:
        got = sample(app, document, depth, x, y)
        assert same(got, before[(x, y)], depth), ((x, y), got, before[(x, y)])
    for x, y in [(2, 2), (93, 3), (4, 76), (92, 77)]:
        got = sample(app, document, depth, x, y)
        assert transparent(got), ('far', x, y, got)
    # Pin landing: snapped to vertices, each exactly (6, 4) from its vertex.
    assert reply['vertices'] > 0 and reply['triangles'] > 0, reply
    assert len(reply['pins']) == len(PINS), reply
    for pin in reply['pins']:
        assert [pin['to'][0] - pin['from'][0], pin['to'][1] - pin['from'][1]] == [6, 4], pin
        assert pin['from'] == [round(v) for v in pin['from']], ('a vertex is integral', pin)
    # Undo.
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    for x, y in points[::7]:
        got = sample(app, document, depth, x - 6, y - 4)
        assert same(got, before[(x, y)], depth), ('undo', x, y, got)
    return dict(samples=len(points), vertices=reply['vertices'])


def selection_float(app, depth, folder):
    document = opened(app, folder, f'float-{depth}', depth)
    app.call('select_rect', document_id=document, x=30, y=28, width=24, height=24)
    landed = [(x + 20, y) for x in range(33, 52, 3) for y in range(31, 50, 3)]
    before = {p: sample(app, document, depth, p[0] - 20, p[1]) for p in landed}
    kept = [(20, 40), (24, 30), (30, 56), (40, 61)]
    kept_before = {p: sample(app, document, depth, *p) for p in kept}
    start = cursor(app, document)
    reply = app.call('puppet_warp', document_id=document,
                     pins=pins(20, 0, at=[(40, 36), (46, 44)]))
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
    name = f'v1 puppet warp {depth}'
    moved = [dict(p, to=[p['to'][0] + 3 * i, p['to'][1] - 2 * i], depth=i)
             for i, p in enumerate(pins(0, 0))]
    moved[0]['rotate'] = 15
    app.call('start_recording', discard=True)
    reply = app.call('puppet_warp', document_id=document, pins=moved, mode='rigid',
                     density='more', expansion=3)
    assert reply['changed'] is True, reply
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['puppet_warp'], action
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
    good = pins(5, 0)[:1]
    pin = good[0]
    seen = {}

    def refuse(name, needle, **arguments):
        seen[name] = app.refused('puppet_warp', needle, document_id=document, **arguments)

    refuse('no pins', 'Puppet Warp needs at least one pin.', pins=[])
    refuse('too many', 'Puppet Warp takes at most 256 pins.', pins=[pin] * 257)
    refuse('outside', 'Puppet Warp pin 1 is outside the mesh.',
           pins=[pin, {'from': [1, 1], 'to': [3, 3]}])
    refuse('shared', 'Puppet Warp pins 0 and 1 share a mesh vertex.',
           pins=[pin, dict(pin, to=[60, 50])])
    refuse('pixel limit', 'Puppet Warp would make the layer larger than the pixel limit.',
           pins=[{'from': [36, 40], 'to': [-500000, -500000]},
                 {'from': [60, 40], 'to': [500000, 500000]}])
    refuse('mode', 'Puppet Warp mode must be rigid, normal or distort.', pins=good,
           mode='bendy')
    refuse('density', 'Puppet Warp density must be fewer, normal or more.', pins=good,
           density='most')
    refuse('expansion', 'Puppet Warp expansion must be a whole number of pixels from -100 to 100.',
           pins=good, expansion=2.5)
    refuse('depth', 'Puppet Warp pin 0 depth must be a whole number from -1000 to 1000.',
           pins=[dict(pin, depth=2000)])
    refuse('rotate', 'Puppet Warp pin 0 rotate must be a finite angle in degrees or null.',
           pins=[dict(pin, rotate='left')])
    refuse('point', 'Puppet Warp pin 0 needs from and to as two finite numbers within 1000000.',
           pins=[{'from': [1, 'a'], 'to': [2, 2]}])
    assert cursor(app, document) == start, 'a refusal leaves no step'
    app.call('select_rect', document_id=document, x=1, y=1, width=8, height=8)
    seen['float empty'] = app.refused(
        'puppet_warp',
        'The selection holds no pixels to puppet warp.',
        document_id=document, pins=good)
    app.call('deselect', document_id=document)
    app.call('new_layer', document_id=document)
    rows = app.call('get_document', document_id=document)['layers']
    seen['empty'] = app.refused(
        'puppet_warp', 'Puppet Warp needs pixels to mesh, and there are none at expansion 2 px.',
        document_id=document, pins=good, layer=len(rows) - 1)
    after = cursor(app, document)
    seen['steps'] = after - start
    return seen


def described(app, depth, folder):
    document = opened(app, folder, f'described-{depth}', 8)
    app.call('add_text_layer', document_id=document, text='Puppet', x=10, y=30, size=26)
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')

    def text_rows():
        rows = app.call('get_document', document_id=document)['layers']
        return [index for index, row in enumerate(rows) if 'text' in row]

    texts = text_rows()
    assert len(texts) == 1, texts
    layer = texts[0]
    # A glyph pixel of the text layer, found by sampling: the pin goes there.
    probe = next(((x, y) for y in range(24, 64, 2) for x in range(8, 92, 2)
                  if sample(app, document, depth, x, y, layer=layer)[3] > 0), None)
    assert probe is not None, 'the text layer has no opaque pixel'
    start = cursor(app, document)
    reply = app.call('puppet_warp', document_id=document, layer=layer,
                     pins=[{'from': [probe[0] + 0.5, probe[1] + 0.5],
                            'to': [probe[0] + 4.5, probe[1] + 2.5]}])
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
        tempfile.mkdtemp(prefix='rz-v1-puppet-warp-'))
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
            run(f'translation, pin landing and undo {depth}-bit',
                lambda: translation(app, depth, output))
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
