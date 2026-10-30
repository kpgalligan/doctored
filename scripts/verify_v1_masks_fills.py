#!/usr/bin/env python3
"""MCP drive for the v1 masks and fills batch.

The script owns its app instance: the Debug app on its own scratch MCP socket with
`DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin
and private `DR_*_DIR`s (`verification_isolation.preset_environment`). It
terminates only that process. Every expected number is computed here, by this
file's own oracle, never read back from the app.

At 8 and 16 bits:
- INVERT: a layer mask whose stored rectangle is smaller than the canvas
  (reveal_all, then a growing canvas_size). `apply_filter invert target mask`
  with no selection inverts every canvas sample, inside the rectangle and
  outside it; undo restores; with a selection each sample inverts by its
  coverage and the rest is untouched. The oracle is the mask loaded as the
  selection and read sample by sample.
- QUICK MASK: in Quick Mask, `apply_filter blur` softens the coverage (an
  edge sample becomes partial, the far interior stays full) and `gradient`
  lays a black-to-white ramp over it (the samples follow (x + 0.5) / width);
  each is one history step; `target: "quick_mask"` outside Quick Mask is
  refused by name; a `gradient` naming neither `start_color` nor `stops` is
  refused and takes no step.
- CARRY: a rectangle selection's bounds after crop, canvas_size, rotate 90 and
  image_size are the rectangle through the op's map, and undo restores both
  the canvas and the selection; a polygon whose bounding box reaches the crop
  window while the polygon does not leaves no selection, and undo restores it;
  a feathered selection keeps its samples, shifted, through a crop.
- STOPS: a three-stop linear `gradient` paints the piecewise-linear ramp, and
  a diamond one its corner-standing squares; a `gradient` naming no ramp
  (neither `start_color` nor `stops`; at 16 bits nor `start_rgba`) and a stop
  whose `opacity` is not a number are refused and take no step.

At 8, 16 and 32 bits:
- FILL: `add_fill_layer` solid and gradient cover the canvas with the expected
  colour; `edit_fill_layer` re-renders in one history step; a `.rz` saved and
  reopened reports the same `fill` object and the same pixels.
- REFIT: `crop` and a growing `canvas_size` each answer ok, leave the fill
  layer covering the new canvas at (0, 0) with its ramp re-rendered, and take
  one history step that undo walks back.
- PIN: `move_layers` on a fill layer, and on a group holding one, is refused
  naming the fill layer; offsets and history are unchanged.

Usage: python3 scripts/verify_v1_masks_fills.py [--output DIR]
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
from verify_phase1 import Client  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'


class App:
    """The one owned Debug instance, on its own scratch socket."""

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
            DR_PASTEBOARD_NAME='rz-verify-v1-masks-fills',
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


def new_document(app, depth, width, height, background='white'):
    return app.call('new_document', width=width, height=height, depth=depth, profile='srgb',
                    background=background)['document']['id']

def open_float(app):
    return app.call('open_document', path=str(FLOAT_FIXTURE))['document']['id']


def document_of(app, depth, width, height):
    """A document at `depth`: new at 8 and 16 bits, the float fixture with its
    canvas set to `width` x `height` at 32 (a float document only comes from
    a file)."""
    if depth != 32:
        return new_document(app, depth, width, height)
    document = open_float(app)
    app.call('canvas_size', document_id=document, width=width, height=height, anchor='top-left')
    return document


def canvas(app, document):
    summary = app.call('get_document', document_id=document)
    return summary['width'], summary['height']


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def rect_of(row):
    if 'offset' in row:
        x, y = row['offset']
    else:
        x, y = row['offset_x'], row['offset_y']
    return [x, y, row['width'], row['height']]


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def undo_to(app, document, target):
    """Undo until the history cursor is back at `target` (reading a mask
    through load_selection / deselect takes history steps of its own)."""
    while cursor(app, document) > target:
        app.call('undo', document_id=document)
    assert cursor(app, document) == target, (cursor(app, document), target)


def coverage(app, document, width, height):
    """The selection's u16 coverage over the whole canvas, row-major."""
    points = [{'x': x, 'y': y} for y in range(height) for x in range(width)]
    samples = []
    # get_selection takes at most 4096 points a call.
    for at in range(0, len(points), 4096):
        reply = app.call('get_selection', document_id=document, points=points[at:at + 4096])
        samples += [sample['coverage16'] for sample in reply['samples']]
    return samples


def mask_coverage(app, document, layer, width, height):
    """Layer `layer`'s mask over the whole canvas, through load_selection."""
    app.call('load_selection', document_id=document, **{'from': 'layer_mask'}, layer=layer)
    samples = coverage(app, document, width, height)
    app.call('deselect', document_id=document)
    return samples


def unit(app, document, depth, layer, x, y):
    """Layer `layer`'s pixel at (x, y) as four numbers in the storage's own
    unit scaled to 0..1 (8 and 16 bits) or as stored (32-bit linear light)."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    if depth == 32:
        return [float(v) for v in reply['center_rgba32']]
    scale = 65535.0 if depth == 16 else 255.0
    return [v / scale for v in reply['center_rgba_codes']]


def srgb_to_linear(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def stored(depth, rgb):
    """An authored sRGB colour as the document stores it: encoded at 8 and 16
    bits, linear light at 32."""
    return [srgb_to_linear(v) for v in rgb] if depth == 32 else list(rgb)


def tolerance(depth):
    return {8: 1.5 / 255, 16: 1.5 / 255, 32: 2e-3}[depth]


def close(a, b, eps):
    return all(abs(x - y) <= eps for x, y in zip(a, b))


# ---------------------------------------------------------------- cases --

def invert(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    app.call('add_layer_mask', document_id=document, layer=0, kind='reveal_all')
    app.call('canvas_size', document_id=document, width=24, height=18, anchor='center')
    cw, ch = canvas(app, document)
    assert (cw, ch) == (24, 18), (cw, ch)
    before = mask_coverage(app, document, 0, cw, ch)
    inside = [v for v in before if v == 65535]
    outside = [v for v in before if v == 0]
    # The stored rectangle is the old 16 x 12 canvas: revealed inside, hidden outside.
    assert len(inside) == width * height and len(outside) == cw * ch - width * height, (
        len(inside), len(outside))

    start = cursor(app, document)
    reply = app.call('apply_filter', document_id=document, filter='invert', target='mask', layer=0)
    assert reply.get('ok') and reply.get('changed', True), reply
    assert cursor(app, document) == start + 1
    after = mask_coverage(app, document, 0, cw, ch)
    assert after == [65535 - v for v in before], 'whole-canvas invert'
    undo_to(app, document, start)
    assert mask_coverage(app, document, 0, cw, ch) == before, 'undo restores the mask'

    # A selection straddling the stored rectangle's left edge (x = 4).
    sx, sy, sw, sh = 1, 2, 8, 9
    app.call('select_rect', document_id=document, x=sx, y=sy, width=sw, height=sh)
    app.call('apply_filter', document_id=document, filter='invert', target='mask', layer=0)
    app.call('deselect', document_id=document)
    scoped = mask_coverage(app, document, 0, cw, ch)
    for y in range(ch):
        for x in range(cw):
            i = y * cw + x
            selected = sx <= x < sx + sw and sy <= y < sy + sh
            want = 65535 - before[i] if selected else before[i]
            assert scoped[i] == want, (x, y, scoped[i], want)
    return dict(canvas=[cw, ch], stored=[width, height])


def quick_mask(app, depth):
    width, height = 40, 24
    document = new_document(app, depth, width, height)
    app.refused('gradient', 'not in Quick Mask', document_id=document, target='quick_mask',
                x0=0, y0=12, x1=40, y1=12, start_color='#000000', end_color='#FFFFFF')
    app.refused('apply_filter', 'not in Quick Mask', document_id=document, filter='blur',
                sigma=2, target='quick_mask')
    app.call('select_rect', document_id=document, x=10, y=6, width=20, height=12)
    app.call('set_quick_mask', document_id=document, active=True)
    hard = coverage(app, document, width, height)
    assert hard[12 * width + 10] == 65535 and hard[12 * width + 9] == 0

    start = cursor(app, document)
    reply = app.call('apply_filter', document_id=document, filter='blur', sigma=2)
    assert reply['changed'] and reply['target'] == 'quick_mask' and reply['quick_mask_active'], reply
    assert cursor(app, document) == start + 1
    soft = coverage(app, document, width, height)
    # The edge samples are partial on both sides; the centre stays full.
    assert 0 < soft[12 * width + 9] < 32768 < soft[12 * width + 10] < 65535, (
        soft[12 * width + 9], soft[12 * width + 10])
    assert soft[12 * width + 20] >= 65000, soft[12 * width + 20]
    # A blurred edge is symmetric about the edge it softened.
    assert abs(soft[12 * width + 9] + soft[12 * width + 10] - 65535) <= 700
    app.refused('apply_filter', 'supports filter "blur" only', document_id=document,
                filter='invert', target='quick_mask')
    app.call('undo', document_id=document)
    assert coverage(app, document, width, height) == hard, 'undo restores the coverage'

    # A gradient names its ramp: with neither start_color nor stops it is
    # refused (it used to lay the black-to-transparent default), no step taken.
    start = cursor(app, document)
    app.refused('gradient', 'gradient requires start_color or stops', document_id=document,
                x0=0, y0=12, x1=width, y1=12)
    app.refused('gradient', 'gradient requires start_color or stops', document_id=document,
                x0=0, y0=12, x1=width, y1=12, target='quick_mask', end_color='#FFFFFF')
    assert cursor(app, document) == start, 'a refused gradient leaves no step'
    assert coverage(app, document, width, height) == hard, 'a refused gradient changes nothing'

    # An opaque black-to-white ramp replaces the coverage with (x + 0.5) / width.
    start = cursor(app, document)
    reply = app.call('gradient', document_id=document, x0=0, y0=12, x1=width, y1=12,
                     start_color='#000000', end_color='#FFFFFF')
    assert reply['changed'] and reply['target'] == 'quick_mask', reply
    assert cursor(app, document) == start + 1
    ramp = coverage(app, document, width, height)
    slack = 400 if depth == 16 else 600
    for x in range(width):
        want = (x + 0.5) / width * 65535
        for y in (0, 12, height - 1):
            assert abs(ramp[y * width + x] - want) <= slack, (x, y, ramp[y * width + x], want)

    # Stops: black, white in the middle, black again.
    app.call('gradient', document_id=document, x0=0, y0=12, x1=width, y1=12, target='quick_mask',
             stops=[{'position': 0, 'color': '#000000'}, {'position': 0.5, 'color': '#FFFFFF'},
                    {'position': 1, 'color': '#000000'}])
    tent = coverage(app, document, width, height)
    for x in range(width):
        t = (x + 0.5) / width
        want = (1 - abs(2 * t - 1)) * 65535
        assert abs(tent[12 * width + x] - want) <= slack, (x, tent[12 * width + x], want)
    app.call('set_quick_mask', document_id=document, active=False)
    return dict(edge=[soft[12 * width + 9], soft[12 * width + 10]])


def carry(app, depth):
    width, height = 40, 30
    rect = (10, 8, 12, 6)
    document = new_document(app, depth, width, height)

    def select():
        app.call('select_rect', document_id=document, x=rect[0], y=rect[1], width=rect[2],
                 height=rect[3])

    def bounds():
        reply = app.call('get_selection', document_id=document)
        assert not reply['selection_empty'], reply
        b = reply['bounds']
        return (b['x'], b['y'], b['width'], b['height'])

    cases = [
        ('crop', dict(x=5, y=4, width=30, height=20), (30, 20), (5, 4, 12, 6)),
        # Centre anchor: the old canvas's top-left lands at (10, 10).
        ('canvas_size', dict(width=60, height=50, anchor='center'), (60, 50), (20, 18, 12, 6)),
        # A clockwise quarter turn sends (x, y) to (height - y, x).
        ('rotate', dict(degrees=90), (30, 40), (30 - 8 - 6, 10, 6, 12)),
        ('image_size', dict(width=80, height=60), (80, 60), (20, 16, 24, 12)),
        ('flip', dict(axis='horizontal'), (40, 30), (40 - 10 - 12, 8, 12, 6)),
    ]
    seen = {}
    select()
    for tool, arguments, size, want in cases:
        assert bounds() == rect, (tool, bounds())
        start = cursor(app, document)
        app.call(tool, document_id=document, **arguments)
        assert canvas(app, document) == size, (tool, canvas(app, document))
        assert bounds() == want, (tool, bounds(), want)
        assert cursor(app, document) == start + 1, tool
        app.call('undo', document_id=document)
        assert canvas(app, document) == (width, height), tool
        assert bounds() == rect, (tool, 'undo', bounds())
        seen[tool] = list(want)

    # A shape whose bounding box reaches the new canvas while the shape does
    # not. The triangle (20,0) (30,0) (30,10) covers y < x - 20, and the crop
    # window is x < 24, y >= 4: its box overlaps the window in a corner the
    # triangle leaves empty, so nothing of it is carried, and undo restores it.
    window = dict(x=0, y=4, width=24, height=20)
    assert all((x + 1) - 20 <= y for y in range(4, 24) for x in range(0, 24)), 'the oracle'
    held = cursor(app, document)
    app.call('select_polygon', document_id=document, points=[[20, 0], [30, 0], [30, 10]])
    triangle = coverage(app, document, width, height)
    assert triangle[2 * width + 28] == 65535, 'the triangle selects its interior'
    shaped = cursor(app, document)
    app.call('crop', document_id=document, **window)
    assert canvas(app, document) == (24, 20), canvas(app, document)
    reply = app.call('get_selection', document_id=document)
    assert reply['selection_empty'] is True, reply
    assert cursor(app, document) == shaped + 1, 'the crop is one step'
    app.call('undo', document_id=document)
    assert canvas(app, document) == (width, height)
    assert coverage(app, document, width, height) == triangle, 'undo restores the triangle'
    seen['triangle'] = 'selection_empty'
    undo_to(app, document, held)
    assert bounds() == rect, ('after the triangle', bounds())

    # Coverage, not a shape: a feathered selection keeps every sample through
    # a crop, shifted by the window's origin.
    app.call('modify_selection', document_id=document, operation='feather', radius=2)
    soft = coverage(app, document, width, height)
    feathered = cursor(app, document)
    app.call('crop', document_id=document, x=5, y=4, width=30, height=20)
    moved = coverage(app, document, 30, 20)
    for y in range(20):
        for x in range(30):
            assert moved[y * 30 + x] == soft[(y + 4) * width + x + 5], (x, y)
    undo_to(app, document, feathered)
    assert coverage(app, document, width, height) == soft, 'undo restores the coverage'

    # A resampling map (Image Size x2) scales the coverage with the canvas:
    # the sample at (2x, 2y) follows the source's (x, y), within the
    # bilinear filter's reach of a soft edge.
    app.call('image_size', document_id=document, width=2 * width, height=2 * height)
    scaled = coverage(app, document, 2 * width, 2 * height)
    for y in range(1, height - 1):
        for x in range(1, width - 1):
            near = [soft[(y + dy) * width + x + dx] for dy in (-1, 0, 1) for dx in (-1, 0, 1)]
            got = scaled[(2 * y) * 2 * width + 2 * x]
            assert min(near) - 300 <= got <= max(near) + 300, (x, y, got, min(near), max(near))
    assert scaled[(2 * 11) * 2 * width + 2 * 16] >= 65000, 'the interior stays selected'
    return seen


def stops(app, depth):
    width, height = 64, 8
    document = new_document(app, depth, width, height)
    ramp = [(0.0, (1.0, 0.0, 0.0)), (0.5, (0.0, 1.0, 0.0)), (1.0, (0.0, 0.0, 1.0))]
    app.call('gradient', document_id=document, layer=0, x0=0, y0=4, x1=width, y1=4,
             stops=[{'position': p, 'color': '#%02X%02X%02X' % tuple(int(v * 255) for v in c)}
                    for p, c in ramp])

    def expected(t):
        for (p0, c0), (p1, c1) in zip(ramp, ramp[1:]):
            if t <= p1:
                f = (t - p0) / (p1 - p0)
                return [a + (b - a) * f for a, b in zip(c0, c1)]
        return list(ramp[-1][1])

    for x in (0, 7, 16, 31, 32, 40, 55, 63):
        got = unit(app, document, depth, 0, x, 3)
        want = expected((x + 0.5) / width) + [1.0]
        assert close(got, want, 2.5 / 255), (x, got, want)

    # A gradient names its ramp. With neither start_color nor stops (at 16
    # bits, nor start_rgba) it is refused, where it used to paint the
    # black-to-transparent default; so is a stop whose opacity is not a
    # number, which used to read as the default 1. Neither takes a step or
    # changes a pixel.
    start = cursor(app, document)
    unnamed = ('gradient requires start_color, start_rgba or stops' if depth == 16
               else 'gradient requires start_color or stops')
    app.refused('gradient', unnamed, document_id=document, layer=0, x0=0, y0=4, x1=width, y1=4)
    app.refused('gradient', unnamed, document_id=document, layer=0, x0=0, y0=4, x1=width, y1=4,
                end_color='#FFFFFF')
    app.refused('gradient', 'stops[1].opacity must be 0…1', document_id=document, layer=0,
                x0=0, y0=4, x1=width, y1=4,
                stops=[{'position': 0, 'color': '#000000'},
                       {'position': 1, 'color': '#FFFFFF', 'opacity': 'abc'}])
    assert cursor(app, document) == start, 'a refused gradient leaves no step'
    got = unit(app, document, depth, 0, 16, 3)
    assert close(got, expected(16.5 / width) + [1.0], 2.5 / 255), ('after the refusals', got)

    # Diamond from the centre: |dx| + |dy| over the drag's length.
    square = new_document(app, depth, 32, 32)
    app.call('gradient', document_id=square, layer=0, x0=16, y0=16, x1=32, y1=16,
             shape='diamond', start_color='#000000', end_color='#FFFFFF')
    for x, y in ((16, 16), (24, 16), (16, 24), (20, 20), (28, 18), (4, 10)):
        t = min((abs(x + 0.5 - 16) + abs(y + 0.5 - 16)) / 16, 1.0)
        got = unit(app, square, depth, 0, x, y)
        assert close(got, [t, t, t, 1.0], 2.5 / 255), (x, y, got, t)
    return dict(samples=8)


def fill_solid_matches(app, document, depth, layer, rgb, alpha=1.0):
    width, height = canvas(app, document)
    want = stored(depth, rgb) + [alpha]
    for x, y in ((0, 0), (width - 1, 0), (width // 2, height // 2), (0, height - 1),
                 (width - 1, height - 1)):
        got = unit(app, document, depth, layer, x, y)
        assert close(got, want, tolerance(depth)), (depth, x, y, got, want)


def fill_ramp_matches(app, document, depth, layer):
    """A black-to-white linear fill at angle 0 reads (x + 0.5) / width in the
    storage's own unit at every depth (black and white are 0 and 1 in both
    the encoded and the linear form, and the ramp is linear between them)."""
    width, height = canvas(app, document)
    for x in (0, width // 4, width // 2, width - 1):
        t = (x + 0.5) / width
        got = unit(app, document, depth, layer, x, height // 2)
        assert close(got, [t, t, t, 1.0], 2.5 / 255), (depth, x, got, t)


def fill(app, depth, folder):
    width, height = 48, 32
    document = document_of(app, depth, width, height)
    assert canvas(app, document) == (width, height)

    start = cursor(app, document)
    reply = app.call('add_fill_layer', document_id=document, kind='solid', color='#3366CC')
    solid = reply['layer']
    assert reply['fill']['kind'] == 'solid' and reply['fill']['color'] == '#3366CCFF', reply
    assert cursor(app, document) == start + 1
    row = layers(app, document)[solid]
    assert rect_of(row) == [0, 0, width, height], row
    assert row['fill']['canvas'] == {'width': width, 'height': height}, row['fill']
    fill_solid_matches(app, document, depth, solid, (0x33 / 255, 0x66 / 255, 0xCC / 255))

    start = cursor(app, document)
    edited = app.call('edit_fill_layer', document_id=document, layer=solid, color='#FF8000')
    assert edited['changed'] and edited['fill']['color'] == '#FF8000FF', edited
    assert cursor(app, document) == start + 1
    fill_solid_matches(app, document, depth, solid, (1.0, 0x80 / 255, 0.0))
    again = app.call('edit_fill_layer', document_id=document, layer=solid, color='#FF8000')
    assert again['changed'] is False and cursor(app, document) == start + 1, again

    gradient = app.call(
        'add_fill_layer', document_id=document, kind='gradient',
        gradient={'stops': [{'position': 0, 'color': '#000000'},
                            {'position': 1, 'color': '#FFFFFF'}],
                  'style': 'linear', 'angle': 0})['layer']
    fill_ramp_matches(app, document, depth, gradient)
    # Reversed: the ramp runs the other way, in one step.
    start = cursor(app, document)
    app.call('edit_fill_layer', document_id=document, layer=gradient, gradient={'reverse': True})
    assert cursor(app, document) == start + 1
    got = unit(app, document, depth, gradient, 0, height // 2)
    assert close(got, [1 - 0.5 / width] * 3 + [1.0], 2.5 / 255), got
    app.call('edit_fill_layer', document_id=document, layer=gradient, gradient={'reverse': False})

    # Save as .rz and reopen: the same descriptions and the same pixels.
    path = Path(folder) / f'fill-{depth}.rz'
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    reopened = app.call('open_document', path=str(path))['document']['id']
    before, after = layers(app, document), layers(app, reopened)
    assert after[solid]['fill'] == before[solid]['fill'], (after[solid], before[solid])
    assert after[gradient]['fill'] == before[gradient]['fill']
    assert after[gradient]['fill']['gradient']['style'] == 'linear'
    fill_solid_matches(app, reopened, depth, solid, (1.0, 0x80 / 255, 0.0))
    fill_ramp_matches(app, reopened, depth, gradient)
    # A reopened fill layer is still editable.
    app.call('edit_fill_layer', document_id=reopened, layer=solid, color='#00FF00')
    fill_solid_matches(app, reopened, depth, solid, (0.0, 1.0, 0.0))
    return dict(document=document, solid=solid, gradient=gradient)


def refit(app, depth, made):
    document, gradient = made['document'], made['gradient']
    width, height = canvas(app, document)

    def covers(size):
        assert canvas(app, document) == size, (canvas(app, document), size)
        for index in (made['solid'], gradient):
            row = layers(app, document)[index]
            assert rect_of(row) == [0, 0, size[0], size[1]], (index, row)
            assert row['fill']['canvas'] == {'width': size[0], 'height': size[1]}, row['fill']
        fill_ramp_matches(app, document, depth, gradient)
        fill_solid_matches(app, document, depth, made['solid'], (1.0, 0x80 / 255, 0.0))

    start = cursor(app, document)
    reply = app.call('crop', document_id=document, x=4, y=3, width=width - 10, height=height - 8)
    assert reply.get('ok'), reply
    covers((width - 10, height - 8))
    assert cursor(app, document) == start + 1, 'crop is one step'
    app.call('undo', document_id=document)
    covers((width, height))

    start = cursor(app, document)
    reply = app.call('canvas_size', document_id=document, width=width + 12, height=height + 6,
                     anchor='center')
    assert reply.get('ok'), reply
    covers((width + 12, height + 6))
    assert cursor(app, document) == start + 1, 'canvas_size is one step'
    app.call('undo', document_id=document)
    covers((width, height))

    # A quarter turn re-renders too, and turns the gradient with the canvas:
    # left-to-right becomes top-to-bottom.
    app.call('rotate', document_id=document, degrees=90)
    assert canvas(app, document) == (height, width)
    row = layers(app, document)[gradient]
    assert rect_of(row) == [0, 0, height, width], row
    for y in (0, width // 2, width - 1):
        t = (y + 0.5) / width
        got = unit(app, document, depth, gradient, height // 2, y)
        assert close(got, [t, t, t, 1.0], 2.5 / 255), (y, got, t)
    app.call('undo', document_id=document)
    covers((width, height))
    return dict(canvas=[width, height])


def pin(app, depth, made):
    document, solid, gradient = made['document'], made['solid'], made['gradient']
    before = [rect_of(row) for row in layers(app, document)]
    start = cursor(app, document)
    text = app.refused('move_layers', 'is a fill layer', 'cannot be moved or transformed',
                       document_id=document, layers=[solid], dx=3, dy=2)
    group = app.call('group_layers', document_id=document, layers=[solid, gradient])['group']
    grouped = cursor(app, document)
    assert grouped == start + 1
    app.refused('move_layers', 'is a fill layer', document_id=document, layers=[group],
                dx=3, dy=2)
    app.refused('transform_layer', 'is a fill layer', document_id=document, layer=group,
                matrix=[1, 0, 0, 1, 5, 5])
    assert cursor(app, document) == grouped, 'a refused move leaves no step'
    app.call('undo', document_id=document)
    assert [rect_of(row) for row in layers(app, document)] == before, 'nothing moved'
    return dict(refusal=text[:120])


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-masks-fills-'))
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
        for depth in (8, 16):
            run(f'invert {depth}-bit', lambda: invert(app, depth))
            run(f'quick mask {depth}-bit', lambda: quick_mask(app, depth))
            run(f'carry {depth}-bit', lambda: carry(app, depth))
            run(f'stops {depth}-bit', lambda: stops(app, depth))
        for depth in (8, 16, 32):
            made = {}

            def fills():
                made.update(fill(app, depth, output))
                return {k: made[k] for k in ('solid', 'gradient')}

            run(f'fill {depth}-bit', fills)
            if made:
                run(f'refit {depth}-bit', lambda: refit(app, depth, made))
                run(f'pin {depth}-bit', lambda: pin(app, depth, made))
            else:
                for name in ('refit', 'pin'):
                    results[f'{name} {depth}-bit'] = dict(passed=False, error='no fill layers')
                    failed.append(f'{name} {depth}-bit')
                    print(f'FAIL {name} {depth}-bit: the fill case made no layers', flush=True)

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
