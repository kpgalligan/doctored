#!/usr/bin/env python3
"""MCP drive for the v1 retouching batch.

The script owns its app instance: the Debug app on its own scratch MCP socket with
`DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin
and private `DR_*_DIR`s (`verification_isolation.preset_environment`). It
terminates only that process. Every picture is a PNG this file writes, and
every expected number is computed here from that picture, never read back from
the app. Whole pictures are read back through `save_copy` and the independent
PNG reader (`scripts/raw-probe/native.py`), single samples through
`sample_pixel`'s exact codes.

The pictures are column ramps: each channel's value depends on x alone, 0.1
code per pixel on the 8-bit scale (the slope of `core/tests/content_aware_large.rs`).
A bound of N codes is N on the 8-bit scale at both depths (N x 257 at 16 bits).

At 8 and 16 bits:
- REMOVE: `remove_stroke` over a magenta disc on a ramp leaves every pixel
  well inside the stroke within 12 codes of the ramp, every pixel outside the
  stroke's box exactly as it was and alpha untouched. It is one history step
  and undo restores the picture. `output: "new_layer"` adds one layer named
  "Content-Aware Fill", leaves the target's pixels alone and shows the same
  repair. The same stroke and seed fill identically, a one-point stroke
  removes a round dab, and a selection confines the stroke. On a two-layer
  document whose upper layer holds the disc alone, `sample_all_layers: false`
  is refused (that layer has nothing around the stroke to sample) and
  `sample_all_layers: true` onto a new layer removes the disc from the
  composite, leaving both layers as they were.
- LARGE: `content_aware_fill` over a 1200 x 1000 selection on 1600 x 1400
  (1.2 MP, above the old 1 MP limit) completes; with `background: true` it
  reports progress through `job_status` and lands the same pixels; a second
  background run cancelled with `cancel_job` ends `cancelled` with the
  document unchanged.

- MOVE: `content_aware_move` on a gentler ramp (0.05 code per pixel, 60 codes
  over 1200 px) with a 100 x 100 coloured square inside a 140 x 140 outlined
  region. Moved by (60, 0), less than the region's width so the two places
  overlap, and by (400, 0): the square's pixels at the new place are its
  colour plus the ramp's difference between the two places, every other pixel
  of the moved region and every pixel of the vacated part of the old region
  is the ramp, all within 12 codes, and every pixel outside both regions
  grown by 2 is exactly as it was. `mode: "extend"` leaves the old place
  exactly as it was and lands the same copy. Each is one history step and
  undo restores the picture. The current selection stands in for `points`,
  a background run lands the waiting run's pixels, and a zero offset, an
  unknown mode, a missing region, a move off the canvas and `output:
  "new_layer"` are refused with no step.
- PATCH16: `patch_region` (direction source) takes its region from 10 px to
  the right: each pixel well inside is the pixel 10 px right of it plus the
  ramp's difference, within 12 codes, and outside the region grown by 2
  nothing changes. Offsets 10 and 10.4 give identical pictures, because the
  offset is rounded to whole pixels. Its subject is the 16-bit Patch; the
  8-bit run is its twin and shows the rule is the same at both depths.

Usage: python3 scripts/verify_v1_retouch.py [--output DIR] [--only NAME ...]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import zlib

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
MAGENTA = (255, 0, 255)
# One fill over a megapixel takes seconds; the shared client allows 30.
CALL_TIMEOUT = 600
# The worker slot's two refusals: a job's start, and a 16-bit edit's preparation.
BUSY = ('Another background operation is running',
        'Wait for the current background operation')


def _load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


png_samples = _load('v1_retouch_png_reader', 'scripts/raw-probe/native.py').png_samples


class PatientClient(Client):
    """The shared MCP client with a timeout a megapixel fill fits in."""

    timeout = CALL_TIMEOUT


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
            DR_PASTEBOARD_NAME='rz-verify-v1-retouch',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = PatientClient(self.socket)
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

    def start(self, tool, **arguments):
        """`tool` once the one worker is free. A job tool refuses at once while
        another owner (a recovery autosave after the previous edit) holds the
        worker; that refusal changes nothing and is retried for 60 s."""
        deadline = time.monotonic() + 60
        while True:
            result = self.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            if not result.get('isError'):
                return json.loads(text)
            assert any(busy in text for busy in BUSY) and time.monotonic() < deadline, (tool, text)
            time.sleep(0.1)


# ------------------------------------------------------------- pictures --


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(width, height, depth, rows):
    """An RGBA PNG at `depth` bits tagged sRGB, from one packed row per line."""
    profile = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc').read_bytes()
    assert len(rows) == height and all(len(row) == width * depth // 2 for row in rows)
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
            + chunk(b'iCCP', b'Fixture RGB\0\0' + zlib.compress(profile))
            + chunk(b'IDAT', zlib.compress(b''.join(b'\0' + row for row in rows), 1))
            + chunk(b'IEND', b''))

class Ramp:
    """A column ramp and the things drawn on it: the picture's own oracle."""

    def __init__(self, width, height, depth, base, slope=0.1):
        self.width, self.height, self.depth = width, height, depth
        self.scale = 257 if depth == 16 else 1
        self.full = 65535 if depth == 16 else 255
        # The ramp's stored RGBA codes per column.
        self.columns = [tuple(round((b + slope * x) * self.scale) for b in base) + (self.full,)
                        for x in range(width)]
        self.marks = []

    def ink(self, rgb):
        return tuple(v * self.scale for v in rgb) + (self.full,)

    def mark(self, inside, rgb):
        """Paints `rgb` wherever `inside(x, y)`."""
        self.marks.append((inside, self.ink(rgb)))

    def code(self, x, y):
        """The fixture's stored RGBA at (x, y), marks included."""
        for inside, ink in reversed(self.marks):
            if inside(x, y):
                return ink
        return self.columns[x]

    def write(self, path):
        pack = (lambda flat: struct.pack(f'>{len(flat)}H', *flat)) if self.depth == 16 else bytes
        plain = pack([v for column in self.columns for v in column])
        rows, packed = [], {}
        for y in range(self.height):
            # Which mark, if any, owns each column of this row; rows that
            # agree (every row of a rectangle) are packed once.
            owners = tuple(next((i for i in reversed(range(len(self.marks)))
                                 if self.marks[i][0](x, y)), None) for x in range(self.width))
            if owners not in packed:
                packed[owners] = plain if not any(o is not None for o in owners) else pack(
                    [v for x, o in enumerate(owners)
                     for v in (self.columns[x] if o is None else self.marks[o][1])])
            rows.append(packed[owners])
        Path(path).write_bytes(png(self.width, self.height, self.depth, rows))
        return path


def rect(x0, y0, width, height):
    return lambda x, y: x0 <= x < x0 + width and y0 <= y < y0 + height


def disc(cx, cy, radius):
    return lambda x, y: math.hypot(x + 0.5 - cx, y + 0.5 - cy) <= radius


# ------------------------------------------------------------- plumbing --


def opened(app, path, depth):
    reply = app.call('open_document', path=str(path),
                     precision='rgba8' if depth == 8 else 'preserve')
    document = reply['document']['id']
    assert app.call('get_precision', document_id=document)['mode'] == f'rgba{depth}', depth
    return document


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']

def codes(app, document, x, y, layer=None):
    """Stored RGBA codes at canvas (x, y): layer `layer`'s own, or the
    composite's. Bytes at 8 bits, u16 codes at 16."""
    scope = dict(source='composite') if layer is None else dict(source='layer', layer=layer)
    reply = app.call('sample_pixel', document_id=document, x=x, y=y, **scope)
    return tuple(int(v) for v in reply['center_rgba_codes'])


def shot(app, document, folder, name):
    """The document's flattened picture at its own depth, through `save_copy`
    and the independent PNG reader: (width, height, depth, samples)."""
    path = Path(folder) / f'{name}.png'
    app.call('save_copy', document_id=document, path=str(path))
    decoded = png_samples(path.read_bytes())
    return decoded['width'], decoded['height'], decoded['depth'], decoded['samples']


def pixel(picture, x, y):
    width, _, _, samples = picture
    i = (y * width + x) * 4
    return tuple(samples[i:i + 4])


def expect_fixture(picture, ramp, what):
    width, height, depth, samples = picture
    assert (width, height, depth) == (ramp.width, ramp.height, ramp.depth), (what, picture[:3])
    for y in range(height):
        for x in range(width):
            assert pixel(picture, x, y) == ramp.code(x, y), (what, x, y, pixel(picture, x, y))


def segment_distance(x, y, a, b):
    """Distance from pixel (x, y)'s centre to the segment a-b."""
    px, py = x + 0.5 - a[0], y + 0.5 - a[1]
    dx, dy = b[0] - a[0], b[1] - a[1]
    t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - t * dx, py - t * dy)


# ---------------------------------------------------------------- cases --

# REMOVE's picture: a 400 x 300 ramp with a magenta disc, and a stroke that
# covers the disc with 18 px to spare on every side.
REMOVE_SIZE = (400, 300)
REMOVE_BASE = (80, 100, 60)
REMOVE_DISC = (200, 150, 14)
REMOVE_STROKE = dict(points=[[176, 150], [224, 150]], size=64)
REMOVE_BOUND = 12


def check_removed(picture, ramp, want, what):
    """`picture` against the stroke: within REMOVE_BOUND codes of `want(x)`
    well inside it, the fixture exactly outside its box (grown by one pixel
    for the anti-aliased rim) and outside the stroke itself, alpha kept.
    Returns the largest error inside."""
    a, b = REMOVE_STROKE['points']
    half = REMOVE_STROKE['size'] / 2
    x0, x1 = a[0] - half - 1, b[0] + half + 1
    y0, y1 = a[1] - half - 1, a[1] + half + 1
    worst, inside = 0, 0
    for y in range(ramp.height):
        for x in range(ramp.width):
            got = pixel(picture, x, y)
            assert got[3] == ramp.full, (what, 'alpha', x, y, got)
            distance = segment_distance(x, y, a, b)
            if not (x0 <= x < x1 and y0 <= y < y1) or distance >= half + 1.5:
                assert got == ramp.code(x, y), (what, 'outside the stroke', x, y, got)
            elif distance <= half - 1.5:
                error = max(abs(g - w) for g, w in zip(got[:3], want(x)))
                worst, inside = max(worst, error), inside + 1
                assert error <= REMOVE_BOUND * ramp.scale, (what, x, y, got, want(x))
    assert inside > 5000, (what, inside)
    return worst / ramp.scale


def remove(app, depth, folder):
    width, height = REMOVE_SIZE
    ramp = Ramp(width, height, depth, REMOVE_BASE)
    ramp.mark(disc(*REMOVE_DISC), MAGENTA)
    document = opened(app, ramp.write(folder / f'remove-{depth}.png'), depth)
    clean = lambda x: ramp.columns[x][:3]  # noqa: E731 - the ramp under the disc
    centre = REMOVE_DISC[:2]
    initial = shot(app, document, folder, f'remove-{depth}-initial')
    expect_fixture(initial, ramp, 'the fixture opens exactly')
    seen = {}

    # In place.
    start = cursor(app, document)
    reply = app.start('remove_stroke', mode='content_aware', document_id=document, layer=0,
                      seed=3, **REMOVE_STROKE)
    assert reply.get('ok') and reply['state'] == 'completed' and reply['layer'] == 0, reply
    assert reply['points'] == 2, reply
    # The stroke's area: a 48 x 64 bar and two half discs of radius 32.
    area = 48 * 64 + math.pi * 32 * 32
    assert area - 100 <= reply['covered_pixels'] <= area + 400, (reply['covered_pixels'], area)
    applied = reply['applied']
    assert (applied['size'], applied['sample'], applied['output'], applied['seed']) == (
        64, 'current', 'layer', 3), applied
    assert cursor(app, document) == start + 1, 'remove_stroke is one history step'
    assert len(layers(app, document)) == 1
    filled = shot(app, document, folder, f'remove-{depth}-filled')
    seen['in_place_worst_codes'] = check_removed(filled, ramp, clean, 'in place')
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert shot(app, document, folder, f'remove-{depth}-undone')[3] == initial[3], (
        'undo restores the picture')

    # The same seed gives the same pixels; a click is a round dab.
    again = app.start('remove_stroke', mode='content_aware', document_id=document, layer=0,
                      seed=3, **REMOVE_STROKE)
    assert again['state'] == 'completed', again
    assert shot(app, document, folder, f'remove-{depth}-again')[3] == filled[3], (
        'the same stroke and seed fill identically')
    app.call('undo', document_id=document)
    dab = app.start('remove_stroke', mode='content_aware', document_id=document, layer=0, size=40,
                    points=[list(centre)])
    assert dab['state'] == 'completed' and dab['points'] == 1, dab
    assert abs(dab['covered_pixels'] - math.pi * 400) <= 120, dab
    got = codes(app, document, *centre, layer=0)
    assert max(abs(g - w) for g, w in zip(got[:3], clean(centre[0]))) <= REMOVE_BOUND * ramp.scale, (
        'a one-point stroke removes the disc under it', got)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    # A stroke that misses the canvas is refused in the tool's own words.
    app.refused('remove_stroke', 'covers no pixel', mode='content_aware', document_id=document,
                layer=0, size=40,
                points=[[-500, -500], [-400, -500]])
    assert cursor(app, document) == start, 'a refused stroke leaves no step'

    # Onto a new layer: the target keeps its pixels, the picture shows the repair.
    reply = app.start('remove_stroke', mode='content_aware', document_id=document, layer=0, seed=3,
                      output='new_layer', **REMOVE_STROKE)
    assert reply['state'] == 'completed' and reply['applied']['output'] == 'new_layer', reply
    assert cursor(app, document) == start + 1, 'the new-layer stroke is one history step'
    rows = layers(app, document)
    assert len(rows) == 2 and rows[1]['name'] == 'Content-Aware Fill', rows
    for x, y in (centre, (190, 150), (150, 150), (250, 181), (20, 20), (399, 299)):
        assert codes(app, document, x, y, layer=0) == ramp.code(x, y), (
            'the target layer is not written', x, y)
    layered = shot(app, document, folder, f'remove-{depth}-new-layer')
    seen['new_layer_worst_codes'] = check_removed(layered, ramp, clean, 'new layer')
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and len(layers(app, document)) == 1
    assert shot(app, document, folder, f'remove-{depth}-new-layer-undone')[3] == initial[3]

    # A selection confines the stroke: with the left half of it selected,
    # nothing at or right of x = 200 is written, the disc's right half included.
    app.call('select_rect', document_id=document, x=100, y=100, width=100, height=100)
    start = cursor(app, document)
    reply = app.start('remove_stroke', mode='content_aware', document_id=document, layer=0,
                      seed=3, **REMOVE_STROKE)
    assert reply['state'] == 'completed', reply
    assert abs(reply['covered_pixels'] - area / 2) <= 250, (reply['covered_pixels'], area / 2)
    assert cursor(app, document) == start + 1
    confined = shot(app, document, folder, f'remove-{depth}-confined')
    a, b = REMOVE_STROKE['points']
    for y in range(height):
        for x in range(width):
            if x >= 200 or segment_distance(x, y, a, b) >= REMOVE_STROKE['size'] / 2 + 1.5:
                assert pixel(confined, x, y) == ramp.code(x, y), ('outside the selection', x, y)
    assert confined[3] != initial[3], 'the selected half of the stroke is filled'
    app.call('undo', document_id=document)
    app.call('close_document', document_id=document, discard_changes=True)

    # Two layers: the object alone on a layer above the clean picture. The
    # layer's own pixels offer nothing to sample (everything around the stroke
    # is transparent), so `sample_all_layers: false` refuses; `true` onto a new
    # layer takes the picture below and removes the object from the composite.
    plain = Ramp(width, height, depth, REMOVE_BASE)
    document = opened(app, plain.write(folder / f'remove-{depth}-plain.png'), depth)
    radius = REMOVE_DISC[2]
    app.call('new_layer', document_id=document)
    app.call('select_ellipse', document_id=document, x=centre[0] - radius, y=centre[1] - radius,
             width=2 * radius, height=2 * radius)
    app.call('fill_selection', document_id=document, layer=1, color='#FF00FF')
    app.call('deselect', document_id=document)
    magenta = plain.ink(MAGENTA)
    assert codes(app, document, *centre) == magenta and len(layers(app, document)) == 2
    start = cursor(app, document)
    seen['layer_only_refusal'] = app.refused(
        'remove_stroke', 'nothing to sample from', mode='content_aware', document_id=document,
        layer=1, sample_all_layers=False, **REMOVE_STROKE)[:80]
    assert cursor(app, document) == start, 'a refused stroke leaves no step'
    assert codes(app, document, *centre) == magenta, 'a refused stroke changes nothing'
    reply = app.start('remove_stroke', mode='content_aware', document_id=document, layer=1, seed=3,
                      sample_all_layers=True, output='new_layer', **REMOVE_STROKE)
    assert reply['state'] == 'completed' and reply['layer'] == 1, reply
    assert reply['applied']['sample'] == 'all' and reply['applied']['sample_all_layers'] is True, reply
    assert cursor(app, document) == start + 1
    rows = layers(app, document)
    assert len(rows) == 3 and rows[2]['name'] == 'Content-Aware Fill', rows
    assert codes(app, document, *centre, layer=1) == magenta, 'the object layer is not written'
    assert codes(app, document, *centre, layer=0) == plain.code(*centre), 'nor the picture below'
    composite = shot(app, document, folder, f'remove-{depth}-all-layers')
    seen['all_layers_worst_codes'] = check_removed(
        composite, plain, clean, 'all layers onto a new layer')
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and len(layers(app, document)) == 2
    assert codes(app, document, *centre) == magenta, 'undo brings the object back'
    app.call('close_document', document_id=document, discard_changes=True)
    return seen


# LARGE's picture: a 1600 x 1400 ramp whose 1200 x 1000 middle is magenta,
# selected whole, so the fill has 1.2 MP to synthesize and 200 px of ramp on
# every side to take it from.
LARGE_SIZE = (1600, 1400)
LARGE_HOLE = (200, 200, 1200, 1000)
LARGE_BASE = (40, 60, 50)
# The declared largest error of `content_aware_large`'s ramp fixtures.
LARGE_BOUND = 40


def finished(app, job_id, cancel=False):
    """Polls `job_status` until the job leaves `running`. Returns its last
    report and every progress value seen while it ran. With `cancel`, the job
    is cancelled once it has reported progress."""
    progress, cancelled = [], False
    deadline = time.monotonic() + CALL_TIMEOUT
    while True:
        report = app.call('job_status', job_id=job_id)
        if report['state'] != 'running':
            return report, progress
        progress.append(report['progress'])
        if cancel and not cancelled and report['progress'] > 0:
            answer = app.call('cancel_job', job_id=job_id)
            assert answer['cancel_requested'] is True, answer
            cancelled = True
        assert time.monotonic() < deadline, (job_id, report)
        time.sleep(0.02)


def large(app, depth, folder):
    width, height = LARGE_SIZE
    x0, y0, w, h = LARGE_HOLE
    ramp = Ramp(width, height, depth, LARGE_BASE)
    ramp.mark(rect(*LARGE_HOLE), MAGENTA)
    document = opened(app, ramp.write(folder / f'large-{depth}.png'), depth)
    inside = [(x, y) for y in (y0 + 3, y0 + 250, y0 + 500, y0 + 750, y0 + h - 4)
              for x in (x0 + 3, x0 + 300, x0 + 600, x0 + 900, x0 + w - 4)]
    outside = [(0, 0), (800, 100), (1599, 1399), (100, 700), (1500, 700), (800, 1300),
               (x0 - 1, 700), (x0 + w, 700), (800, y0 - 1), (800, y0 + h)]

    def untouched(what):
        for x, y in inside + outside:
            assert codes(app, document, x, y, layer=0) == ramp.code(x, y), (what, x, y)

    untouched('the fixture opens exactly')
    app.call('select_rect', document_id=document, x=x0, y=y0, width=w, height=h)
    fill = dict(document_id=document, layer=0, seed=7)
    start = cursor(app, document)

    # Waiting.
    began = time.monotonic()
    reply = app.start('content_aware_fill', **fill)
    seconds = time.monotonic() - began
    assert reply.get('ok') and reply['state'] == 'completed', reply
    assert reply['selected_pixels'] == w * h == 1_200_000, reply
    assert cursor(app, document) == start + 1, 'the fill is one history step'
    worst, first = 0, {}
    for x, y in inside:
        got = codes(app, document, x, y, layer=0)
        error = max(abs(g - c) for g, c in zip(got[:3], ramp.columns[x][:3]))
        assert error <= LARGE_BOUND * ramp.scale and got[3] == ramp.full, (x, y, got, ramp.columns[x])
        worst, first[(x, y)] = max(worst, error), got
    for x, y in outside:
        assert codes(app, document, x, y, layer=0) == ramp.code(x, y), ('outside the selection', x, y)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    untouched('undo restores the picture')

    # In the background: progress through job_status, and the same pixels.
    job = app.start('content_aware_fill', background=True, **fill)
    assert job['state'] == 'running' and job['job_id'], job
    report, progress = finished(app, job['job_id'])
    assert report['state'] == 'completed', report
    assert any(0 < p < 1 for p in progress), ('job_status reported no progress', progress[:8])
    assert all(a <= b for a, b in zip(progress, progress[1:])), 'progress never goes back'
    assert cursor(app, document) == start + 1
    for point, got in first.items():
        assert codes(app, document, *point, layer=0) == got, ('the same seed, the same fill', point)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start

    # Cancelled: nothing lands.
    job = app.start('content_aware_fill', background=True, **fill)
    report, seen = finished(app, job['job_id'], cancel=True)
    assert report['state'] == 'cancelled' and report['cancel_requested'] is True, report
    assert cursor(app, document) == start, 'a cancelled fill leaves no step'
    untouched('a cancelled fill changes nothing')
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(waiting_seconds=round(seconds, 2), inside_worst_codes=worst / ramp.scale,
                progress_reports=len(progress), progress_first=progress[:1],
                cancelled_at=seen[-1:] if seen else None)


# MOVE's picture: a 1200 x 400 ramp of 0.05 code per pixel (60 codes across),
# a 100 x 100 square and a 140 x 140 region outlined 20 px around it.
MOVE_SIZE = (1200, 400)
MOVE_BASE = (60, 90, 70)
MOVE_SLOPE = 0.05
MOVE_SQUARE = (300, 150, 100, 100)
MOVE_INK = (180, 60, 40)
MOVE_REGION = (280, 130, 140, 140)
MOVE_POINTS = [[280, 130], [420, 130], [420, 270], [280, 270]]
MOVE_BOUND = 12


def check_moved(picture, ramp, dx, extend, what):
    """`picture` after the region moved `dx` px right. Within MOVE_BOUND codes:
    the square at its new place is its colour plus the ramp's difference
    between the two places, the rest of the moved region is the ramp, and
    (unless `extend`) so is every pixel of the old region the moved one does
    not cover. Exactly the fixture: every pixel outside both regions grown by
    2, and with `extend` every pixel outside the moved region grown by 2.
    Alpha is kept everywhere. Returns the largest error of each class."""
    rx, ry, rw, rh = MOVE_REGION
    sx, sy, sw, sh = MOVE_SQUARE
    old, new = rect(rx, ry, rw, rh), rect(rx + dx, ry, rw, rh)
    old_grown = rect(rx - 2, ry - 2, rw + 4, rh + 4)
    new_grown = rect(rx + dx - 2, ry - 2, rw + 4, rh + 4)
    square = rect(sx + dx, sy, sw, sh)
    ink = ramp.ink(MOVE_INK)
    bound = MOVE_BOUND * ramp.scale
    worst = dict(square=0, moved=0, vacated=0)
    count = dict(square=0, moved=0, vacated=0)
    for y in range(ramp.height):
        for x in range(ramp.width):
            got = pixel(picture, x, y)
            assert got[3] == ramp.full, (what, 'alpha', x, y, got)
            here = ramp.columns[x]
            if new(x, y) and square(x, y):
                there = ramp.columns[x - dx]
                want = [ink[c] + here[c] - there[c] for c in range(3)]
                kind = 'square'
            elif new(x, y):
                want, kind = here[:3], 'moved'
            elif old(x, y) and not extend:
                want, kind = here[:3], 'vacated'
            else:
                if not new_grown(x, y) and (extend or not old_grown(x, y)):
                    assert got == ramp.code(x, y), (what, 'outside the regions', x, y, got)
                continue
            error = max(abs(g - w) for g, w in zip(got[:3], want))
            assert error <= bound, (what, kind, x, y, got, want)
            worst[kind], count[kind] = max(worst[kind], error), count[kind] + 1
    vacated = 0 if extend else (rw - max(0, rw - dx)) * rh
    assert count == dict(square=sw * sh, moved=rw * rh - sw * sh, vacated=vacated), (what, count)
    return {kind: value / ramp.scale for kind, value in worst.items()}


def move(app, depth, folder):
    width, height = MOVE_SIZE
    rx, ry, rw, rh = MOVE_REGION
    ramp = Ramp(width, height, depth, MOVE_BASE, slope=MOVE_SLOPE)
    spread = max(ramp.columns[-1][c] - ramp.columns[0][c] for c in range(3)) / ramp.scale
    assert width >= 1200 and spread <= 64, ('the ramp spans at most 64 codes', spread)
    ramp.mark(rect(*MOVE_SQUARE), MOVE_INK)
    document = opened(app, ramp.write(folder / f'move-{depth}.png'), depth)
    initial = shot(app, document, folder, f'move-{depth}-initial')
    expect_fixture(initial, ramp, 'the fixture opens exactly')
    seen = {}

    def moved(tag, dx, extend=False, **arguments):
        """One call, its reply and history step checked, the picture checked
        against the oracle, then undone. Returns the picture it made."""
        start = cursor(app, document)
        reply = app.start('content_aware_move', document_id=document, layer=0, seed=5,
                          offset_x=dx, offset_y=0, **arguments)
        assert reply.get('ok') and reply['state'] == 'completed' and reply['layer'] == 0, reply
        assert reply['mode'] == ('extend' if extend else 'move'), reply
        assert reply['offset'] == dict(x=dx, y=0), reply
        assert reply['moved_bounds'] == dict(x=rx + dx, y=ry, width=rw, height=rh), reply
        applied = reply['applied']
        assert (applied['sample'], applied['seed']) == ('all', 5), applied
        assert cursor(app, document) == start + 1, (tag, 'one history step')
        assert len(layers(app, document)) == 1
        picture = shot(app, document, folder, f'move-{depth}-{tag}')
        seen[tag] = check_moved(picture, ramp, dx, extend, tag)
        app.call('undo', document_id=document)
        assert cursor(app, document) == start
        assert shot(app, document, folder, f'move-{depth}-{tag}-undone')[3] == initial[3], (
            tag, 'undo restores the picture')
        return reply, picture

    # Move is the default mode. 60 px is less than the region's 140, so the
    # moved region lies over part of the place it left.
    reply, _ = moved('move-60', 60, points=MOVE_POINTS)
    assert reply['region'] == 'polygon', reply
    reply, far = moved('move-400', 400, points=MOVE_POINTS, mode='move')
    assert reply['region'] == 'polygon', reply
    # Extend: the same copy lands and the old place keeps the square.
    moved('extend-400', 400, extend=True, points=MOVE_POINTS, mode='extend')

    # The current selection stands in for the outline.
    app.call('select_rect', document_id=document, x=rx, y=ry, width=rw, height=rh)
    reply, selected = moved('selection-400', 400)
    assert reply['region'] == 'selection', reply
    seen['selection_equals_polygon'] = selected[3] == far[3]
    app.call('deselect', document_id=document)

    # In the background: the waiting run's pixels.
    start = cursor(app, document)
    job = app.start('content_aware_move', document_id=document, layer=0, seed=5,
                    offset_x=400, offset_y=0, points=MOVE_POINTS, background=True)
    assert job['state'] == 'running' and job['job_id'], job
    report, _ = finished(app, job['job_id'])
    assert report['state'] == 'completed' and report['mode'] == 'move', report
    assert cursor(app, document) == start + 1
    assert shot(app, document, folder, f'move-{depth}-background')[3] == far[3], (
        'the same move and seed land the same pixels')
    app.call('undo', document_id=document)
    assert cursor(app, document) == start

    # Refused, each with no step.
    region = dict(document_id=document, layer=0, points=MOVE_POINTS)
    app.refused('content_aware_move', 'non-zero offset', offset_x=0.2, offset_y=0, **region)
    app.refused('content_aware_move', 'mode must be move or extend',
                offset_x=60, offset_y=0, mode='copy', **region)
    app.refused('content_aware_move', 'needs a region', document_id=document, layer=0,
                offset_x=60, offset_y=0)
    app.refused('content_aware_move', 'falls outside the picture',
                offset_x=5000, offset_y=0, **region)
    app.refused('content_aware_move', 'no new-layer form',
                offset_x=60, offset_y=0, output='new_layer', **region)
    app.refused('content_aware_move', 'offset_x and offset_y', offset_x=60, **region)
    assert cursor(app, document) == start, 'a refused move leaves no step'
    assert shot(app, document, folder, f'move-{depth}-refused')[3] == initial[3], (
        'a refused move changes nothing')
    app.call('close_document', document_id=document, discard_changes=True)
    return seen


# PATCH16's picture: REMOVE's ramp and disc, and a 60 x 60 region around the
# disc. Direction source reads 10 px to the right of each pixel it heals.
PATCH_REGION = (170, 120, 60, 60)
PATCH_POINTS = [[170, 120], [230, 120], [230, 180], [170, 180]]
PATCH_OFFSET = 10
PATCH_BOUND = 12


def patch16(app, depth, folder):
    width, height = REMOVE_SIZE
    rx, ry, rw, rh = PATCH_REGION
    ramp = Ramp(width, height, depth, REMOVE_BASE)
    ramp.mark(disc(*REMOVE_DISC), MAGENTA)
    document = opened(app, ramp.write(folder / f'patch-{depth}.png'), depth)
    initial = shot(app, document, folder, f'patch-{depth}-initial')
    expect_fixture(initial, ramp, 'the fixture opens exactly')

    def patched(tag, offset_x):
        start = cursor(app, document)
        reply = app.start('patch_region', document_id=document, layer=0, points=PATCH_POINTS,
                          offset_x=offset_x, offset_y=0)
        assert reply.get('ok') and reply['state'] == 'completed', reply
        assert reply['direction'] == 'source', reply
        assert reply['offset'] == dict(x=PATCH_OFFSET, y=0), reply
        assert reply['healed_bounds'] == dict(x=rx, y=ry, width=rw, height=rh), reply
        assert cursor(app, document) == start + 1, (tag, 'one history step')
        picture = shot(app, document, folder, f'patch-{depth}-{tag}')
        app.call('undo', document_id=document)
        assert cursor(app, document) == start
        assert shot(app, document, folder, f'patch-{depth}-{tag}-undone')[3] == initial[3], (
            tag, 'undo restores the picture')
        return picture

    whole = patched('whole', PATCH_OFFSET)
    fractional = patched('fractional', PATCH_OFFSET + 0.4)
    assert fractional[3] == whole[3], 'offsets 10.4 and 10 give identical pixels'

    # The whole-pixel patch against its oracle: a pixel well inside the region
    # is the fixture's pixel 10 px to its right plus the ramp's difference
    # between the two columns (the heal's join), clamped to the code range.
    inside, grown = rect(rx + 2, ry + 2, rw - 4, rh - 4), rect(rx - 2, ry - 2, rw + 4, rh + 4)
    worst, changed, checked = 0, 0, 0
    for y in range(height):
        for x in range(width):
            got = pixel(whole, x, y)
            assert got[3] == ramp.full, ('alpha', x, y, got)
            if not grown(x, y):
                assert got == ramp.code(x, y), ('outside the region', x, y, got)
            elif inside(x, y):
                source = ramp.code(x + PATCH_OFFSET, y)
                here, there = ramp.columns[x], ramp.columns[x + PATCH_OFFSET]
                want = [min(max(source[c] + here[c] - there[c], 0), ramp.full) for c in range(3)]
                error = max(abs(g - w) for g, w in zip(got[:3], want))
                assert error <= PATCH_BOUND * ramp.scale, (x, y, got, want)
                worst, checked = max(worst, error), checked + 1
                changed += got != ramp.code(x, y)
    assert checked == (rw - 4) * (rh - 4) and changed > 300, (checked, changed)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(inside_worst_codes=worst / ramp.scale, changed_pixels=changed)


# Each case runs at every depth it lists.
CASES = [
    ('REMOVE', remove, (8, 16)),
    ('LARGE', large, (8, 16)),
    ('MOVE', move, (8, 16)),
    ('PATCH16', patch16, (8, 16)),
]


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance, its pictures and its receipt')
    parser.add_argument('--only', nargs='+', metavar='NAME',
                        help='run only these cases (by name, e.g. REMOVE)')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-retouch-'))
    output.mkdir(parents=True, exist_ok=True)
    unknown = set(arguments.only or []) - {name for name, _, _ in CASES}
    assert not unknown, f'unknown case(s): {sorted(unknown)}'

    results, failed = {}, []
    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'app') as app:
        for name, case, depths in CASES:
            if arguments.only and name not in arguments.only:
                continue
            for depth in depths:
                run(f'{name} {depth}-bit', lambda: case(app, depth, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
