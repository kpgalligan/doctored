#!/usr/bin/env python3
"""MCP drive for the v1 selection batch.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB
working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process. Every expected number is computed here, by this file's own oracle,
never read back from the app.

The synthetic fixture K is a composite of two known colours under a known
alpha: a disc with a 6 px linear ramp, `C = round(a*F + (1 - a)*B)`. The
selection handed to the app is the hard disc (alpha >= 0.5), so the refined
coverage is measured against the alpha the picture was made with.

At 8 and 16 bits:
- EDGE: `refine_selection edge_radius 8` on K recovers the alpha (mean error
  over fractional-alpha pixels <= 16/255, <= 24/255 with `smart_radius`, worst
  <= 64/255) and changes nothing farther than 9 px from the selection's edge.
- EDGE 512: the same on a 512x512 K (disc radius 160), a canvas far past the
  size the coverage stages' own byte budget would admit.
- OUTPUTS: each of the four outputs (layer count, the "Refined Cutout" top
  layer, mask present or applied, every other root hidden, one history step,
  undo restores).
- TWICE: `new_layer_with_mask` twice in a row succeeds both times; at a
  fractional-coverage sample m the rendered alpha is m*m / full scale.
- DECONTAMINATE: a fringe pixel moves toward the foreground colour, and
  `output: "selection"` refuses it by name.
- BRUSH: a `subtract` stroke restores the input edge under it.
- REPLAY: one recorded `refine_selection` Action replays to the same coverage.
- QUICK: `quick_select` with and without `brush_size` / `smooth` /
  `enhance_edge`.
- PHOTOS (`--photos`): `object_select` on portrait.png (non-empty, inside the
  region, IoU with `select_subjects` clipped to the region >= 0.85, a corner
  with no object refused by name), `quick_select` on fur.png (contains every
  seed, 2%-80% of the canvas, one history step), `refine_selection` after
  `select_subject` on product.jpg (changes confined to 9 px of the edge).

`--reference write|check` regenerates or checks
public-docs/src/content/docs/reference/mcp-tools.md against this instance.

Usage: python3 scripts/verify_v1_selection.py [--output DIR] [--photos DIR]
                                              [--reference write|check]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
PHOTOS = Path.home() / 'devel/doctored-fixtures/phase6/photos'
FOREGROUND = (200, 60, 40)
BACKGROUND = (40, 90, 200)


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
            DR_PASTEBOARD_NAME='rz-verify-v1-selection',
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


def full(depth):
    """The storage's full-scale code."""
    return 65535 if depth == 16 else 255

def opened(app, path, depth):
    """`path` opened as a document at `depth`: an 8-bit file promoted with
    every code widened by 257 when 16 bits are asked for."""
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def coverage_at(app, document, points):
    """The selection's u16 coverage at `points` ((x, y) pairs); 0 everywhere
    with nothing selected."""
    samples = []
    # get_selection takes at most 4096 points a call.
    for at in range(0, len(points), 4096):
        chunk = [{'x': x, 'y': y} for x, y in points[at:at + 4096]]
        reply = app.call('get_selection', document_id=document, points=chunk)
        if reply.get('selection_empty'):
            samples += [0] * len(chunk)
        else:
            samples += [sample['coverage16'] for sample in reply['samples']]
    return samples


def grid(width, height, stride=1):
    return [(x, y) for y in range(0, height, stride) for x in range(0, width, stride)]


def codes(app, document, x, y, layer=None):
    """Four storage codes at (x, y): the composite's, or one layer's."""
    if layer is None:
        reply = app.call('sample_pixel', document_id=document, source='composite', x=x, y=y)
    else:
        reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer,
                         x=x, y=y)
    return reply['center_rgba_codes']


def alpha_of(size, radius, x, y):
    """K's alpha at pixel (x, y) and the pixel centre's distance from its
    0.5 contour."""
    d = math.hypot(x + 0.5 - size / 2, y + 0.5 - size / 2)
    return min(1.0, max(0.0, (radius + 3 - d) / 6)), abs(d - radius)


def composite_png(folder, size, radius, tag):
    """K as an 8-bit PNG, one file per case (a path already open would answer
    with the document another case edited)."""
    pixels = bytearray()
    for y in range(size):
        for x in range(size):
            a, _ = alpha_of(size, radius, x, y)
            for c in range(3):
                pixels.append(math.floor(a * FOREGROUND[c] + (1 - a) * BACKGROUND[c] + 0.5))
            pixels.append(255)
    path = Path(folder) / f'k-{size}-{tag}.png'
    png(path, size, size, bytes(pixels))
    return path


def photo(photos, folder, name, tag):
    """A private copy of one pinned photo, one file per case."""
    path = Path(folder) / f'{tag}-{name}'
    shutil.copyfile(photos / name, path)
    return path


def select_disc(app, document, size, radius):
    """The hard input selection: the disc, no anti-aliasing."""
    app.call('select_ellipse', document_id=document, x=size / 2 - radius, y=size / 2 - radius,
             width=2 * radius, height=2 * radius, anti_alias=False)


def near_input_edge(before, size, x, y, reach):
    """Whether pixel (x, y) is within `reach` px of the input selection's 0.5
    contour: the signed distance the core measures, from the nearest pixel of
    the other membership, less half a pixel."""
    inside = before[y * size + x] >= 32768
    span = int(reach) + 1
    for ny in range(max(0, y - span), min(size, y + span + 1)):
        for nx in range(max(0, x - span), min(size, x + span + 1)):
            if (before[ny * size + nx] >= 32768) != inside:
                if math.hypot(nx - x, ny - y) - 0.5 <= reach:
                    return True
    return False


# ---------------------------------------------------------------- cases --

def edge(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'edge-{depth}'), depth)
    points = grid(size, size)
    seen = {}
    for smart, bound in ((False, 16), (True, 24)):
        select_disc(app, document, size, radius)
        before = coverage_at(app, document, points)
        assert set(before) <= {0, 65535}, 'the input selection is hard'
        start = cursor(app, document)
        reply = app.call('refine_selection', document_id=document, edge_radius=8,
                         smart_radius=smart)
        assert reply['changed'] and reply['output'] == 'selection', reply
        assert reply['coverage_storage'] == ('u16' if depth == 16 else 'u8'), reply
        assert cursor(app, document) == start + 1
        after = coverage_at(app, document, points)
        total, count, worst = 0.0, 0, 0.0
        for (x, y), was, now in zip(points, before, after):
            a, _ = alpha_of(size, radius, x, y)
            error = abs(now / 65535 - a)
            worst = max(worst, error)
            if 0 < a < 1:
                total += error
                count += 1
            if now != was:
                assert near_input_edge(before, size, x, y, 9), (x, y, was, now)
        mean = total / count * 255
        assert mean <= bound, (smart, mean)
        assert worst * 255 <= 64, (smart, worst * 255)
        if depth == 16:
            assert any(v % 257 for v in after), 'a 16-bit refinement keeps codes between bytes'
        app.call('undo', document_id=document)
        assert coverage_at(app, document, points) == before, 'undo restores the selection'
        seen['smart' if smart else 'plain'] = dict(mean=round(mean, 2), worst=round(worst * 255, 2))
    return seen


def edge_512(app, depth, folder):
    size, radius = 512, 160
    document = opened(app, composite_png(folder, size, radius, f'edge-{depth}'), depth)
    select_disc(app, document, size, radius)
    # Every third pixel, and the whole row and column through the centre.
    points = sorted(set(grid(size, size, 3)) | {(x, 256) for x in range(size)}
                    | {(256, y) for y in range(size)})
    before = coverage_at(app, document, points)
    reply = app.call('refine_selection', document_id=document, edge_radius=8)
    assert reply['changed'], reply
    after = coverage_at(app, document, points)
    total, count, worst, changed = 0.0, 0, 0.0, 0
    for (x, y), was, now in zip(points, before, after):
        a, from_edge = alpha_of(size, radius, x, y)
        error = abs(now / 65535 - a)
        worst = max(worst, error)
        if 0 < a < 1:
            total += error
            count += 1
        if now != was:
            changed += 1
            # The hard ellipse's own edge lies within a pixel of the circle.
            assert from_edge <= 10, (x, y, was, now, from_edge)
    assert changed > 0
    mean = total / count * 255
    assert mean <= 16 and worst * 255 <= 64, (mean, worst * 255)
    return dict(mean=round(mean, 2), worst=round(worst * 255, 2), samples=len(points))


def outputs(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'outputs-{depth}'), depth)
    select_disc(app, document, size, radius)
    base = layers(app, document)
    assert len(base) == 1 and not base[0].get('has_mask'), base
    probe = [(x, 48) for x in range(size)]
    hard = coverage_at(app, document, probe)
    seen = {}
    for output in ('selection', 'mask', 'new_layer', 'new_layer_with_mask'):
        start = cursor(app, document)
        reply = app.call('refine_selection', document_id=document, edge_radius=8, output=output)
        assert reply['changed'] and reply['output'] == output, reply
        assert cursor(app, document) == start + 1, (output, 'one history step')
        rows = layers(app, document)
        if output == 'selection':
            assert len(rows) == 1 and coverage_at(app, document, probe) != hard
            assert 'new_layer' not in reply, reply
        elif output == 'mask':
            assert len(rows) == 1 and rows[0]['has_mask'], rows
            assert coverage_at(app, document, probe) == hard, 'a mask output keeps the selection'
        else:
            assert len(rows) == 2 and reply['new_layer'] == 1, (rows, reply)
            top = rows[1]
            assert top['name'] == 'Refined Cutout' and top['visible'], top
            assert rows[0]['visible'] is False, 'the original root is hidden'
            assert bool(top.get('has_mask')) == (output == 'new_layer_with_mask'), top
            # The composite is now the cutout: opaque at the centre, clear at
            # a corner, fractional across the ramp.
            assert codes(app, document, 48, 48)[3] == full(depth)
            assert codes(app, document, 2, 2)[3] == 0
            assert 0 < codes(app, document, 78, 48)[3] < full(depth)
            # The cutout layer's own pixels: the composite's colours, with the
            # coverage in its alpha only when the mask was applied.
            inner = codes(app, document, 48, 48, layer=1)
            assert inner == [c * (full(depth) // 255) for c in FOREGROUND] + [full(depth)], inner
            ramp = codes(app, document, 78, 48, layer=1)[3]
            assert (ramp < full(depth)) == (output == 'new_layer'), (output, ramp)
        app.call('undo', document_id=document)
        assert cursor(app, document) == start
        rows = layers(app, document)
        assert len(rows) == 1 and rows[0]['visible'] and not rows[0].get('has_mask'), rows
        assert coverage_at(app, document, probe) == hard, (output, 'undo restores')
        seen[output] = True
    return seen


def twice(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'twice-{depth}'), depth)
    select_disc(app, document, size, radius)
    start = cursor(app, document)
    # Feather alone: coverage that does not depend on the picture, so both
    # cutouts carry the same m (an edge radius would read the composite the
    # first cutout changed).
    arguments = dict(document_id=document, feather=2, output='new_layer_with_mask')
    first = app.call('refine_selection', **arguments)
    assert first['changed'] and first['new_layer'] == 1, first
    # After one cutout the composite's alpha along the row IS the coverage m.
    row = [(x, 48) for x in range(60, 92)]
    scale = full(depth)
    single = [codes(app, document, x, y)[3] for x, y in row]
    fractional = [(point, m) for point, m in zip(row, single) if scale * 0.25 < m < scale * 0.75]
    assert fractional, single
    # The composite is no longer opaque, and the second cutout still lands.
    second = app.call('refine_selection', **arguments)
    assert second['changed'] and second['new_layer'] == 2, second
    rows = layers(app, document)
    assert [r['name'] for r in rows[1:]] == ['Refined Cutout', 'Refined Cutout'], rows
    assert [r['visible'] for r in rows] == [False, False, True], rows
    assert cursor(app, document) == start + 2
    slack = 2 * (scale // 255)
    for (x, y), m in fractional:
        got = codes(app, document, x, y)[3]
        want = m * m / scale
        assert abs(got - want) <= slack, (x, y, m, got, want)
    app.call('undo', document_id=document)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and len(layers(app, document)) == 1
    return dict(fractional=len(fractional))


def decontaminate(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'decontaminate-{depth}'), depth)
    select_disc(app, document, size, radius)
    start = cursor(app, document)
    app.refused('refine_selection', 'Decontaminate Colors changes pixel colours',
                document_id=document, edge_radius=8, decontaminate=True, output='selection')
    app.refused('refine_selection', 'Decontaminate Colors changes pixel colours',
                document_id=document, edge_radius=8, decontaminate=True, output='mask')
    assert cursor(app, document) == start, 'a refusal leaves no step'
    # A fringe pixel on the contour: the composite there is half background.
    x, y = 77, 48
    step = full(depth) // 255
    target = [c * step for c in FOREGROUND]

    def distance(pixel):
        return sum(abs(a - b) for a, b in zip(pixel[:3], target))

    plain = app.call('refine_selection', document_id=document, edge_radius=8,
                     output='new_layer_with_mask')
    assert plain['decontaminated'] is False, plain
    kept = codes(app, document, x, y, layer=1)
    app.call('undo', document_id=document)
    reply = app.call('refine_selection', document_id=document, edge_radius=8,
                     decontaminate=True, output='new_layer_with_mask')
    assert reply['decontaminated'] is True and reply['new_layer'] == 1, reply
    cleaned = codes(app, document, x, y, layer=1)
    assert cleaned[3] == full(depth), 'the estimate keeps the composite alpha'
    assert distance(cleaned) < distance(kept) * 0.6, (kept, cleaned, target)
    # Amount 0 is the composite unchanged.
    app.call('undo', document_id=document)
    none = app.call('refine_selection', document_id=document, edge_radius=8, decontaminate=True,
                    decontaminate_amount=0, output='new_layer_with_mask')
    assert none['decontaminated'] is False, none
    assert codes(app, document, x, y, layer=1) == kept
    return dict(before=distance(kept), after=distance(cleaned))


def brush(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'brush-{depth}'), depth)
    select_disc(app, document, size, radius)
    points = grid(size, size)
    before = coverage_at(app, document, points)
    stroke = dict(mode='subtract', radius=12, points=[[78, 48]])
    app.call('refine_selection', document_id=document, edge_radius=8, brush=[stroke])
    after = coverage_at(app, document, points)
    under, elsewhere = 0, 0
    for (x, y), was, now in zip(points, before, after):
        # Pixel centres strictly inside the stroke's disc keep the input.
        if math.hypot(x + 0.5 - 78, y + 0.5 - 48) <= 11:
            assert now == was, (x, y, was, now)
            under += 1
        elif now != was:
            elsewhere += 1
    assert under > 300 and elsewhere > 0, (under, elsewhere)
    app.call('undo', document_id=document)
    # An add stroke with no radius refines under the stroke only.
    stroke = dict(mode='add', radius=12, points=[[78, 48]])
    app.call('refine_selection', document_id=document, brush=[stroke])
    added = coverage_at(app, document, points)
    inside = 0
    for (x, y), was, now in zip(points, before, added):
        if now != was:
            assert math.hypot(x + 0.5 - 78, y + 0.5 - 48) <= 13, (x, y, was, now)
            inside += 1
    assert inside > 0
    app.refused('refine_selection', 'brush', document_id=document, brush=True)
    app.refused('refine_selection', 'radius', document_id=document,
                brush=[dict(mode='add', radius=0, points=[[1, 1]])])
    app.refused('refine_selection', 'Unknown refinement parameter: edge_strength',
                document_id=document, edge_strength=1)
    return dict(under=under, elsewhere=elsewhere, added=inside)


def replay(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'replay-{depth}'), depth)
    select_disc(app, document, size, radius)
    points = grid(size, size, 2)
    start = cursor(app, document)
    name = f'v1 selection refine {depth}'
    app.call('start_recording')
    stroke = dict(mode='add', radius=6, points=[[18, 48], [20, 52]])
    reply = app.call('refine_selection', document_id=document, edge_radius=6, smart_radius=True,
                     feather=1, brush=[stroke])
    assert reply['changed'], reply
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['refine_selection'], action
    recorded = coverage_at(app, document, points)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert app.call('run_action', document_id=document, name=name)['ok']
    assert coverage_at(app, document, points) == recorded
    assert cursor(app, document) == start + 1
    return dict(steps=1)


def quick(app, depth, folder):
    size, radius = 96, 30
    document = opened(app, composite_png(folder, size, radius, f'quick-{depth}'), depth)
    points = grid(size, size)
    start = cursor(app, document)
    plain = app.call('quick_select', document_id=document, seeds=[[48, 48]])
    assert plain['ok'] and not plain['selection_empty'], plain
    assert cursor(app, document) == start + 1
    first = coverage_at(app, document, points)
    # The seed's colour region: the disc's flat interior, and nothing of the
    # background beyond the ramp.
    for (x, y), value in zip(points, first):
        a, from_edge = alpha_of(size, radius, x, y)
        if a == 1.0:
            assert value == 65535, (x, y, value)
        if a == 0.0:
            assert value == 0, (x, y, value)
    # Naming the defaults changes nothing.
    app.call('quick_select', document_id=document, seeds=[[48, 48]], brush_size=1, smooth=0,
             enhance_edge=False)
    assert coverage_at(app, document, points) == first
    # A brush on the background side of the ramp selects its own disc's reach
    # of the background and none of the foreground.
    app.call('quick_select', document_id=document, seeds=[[6, 6]], brush_size=9, smooth=1)
    wide = coverage_at(app, document, points)
    for (x, y), value in zip(points, wide):
        a, _ = alpha_of(size, radius, x, y)
        if a == 0.0:
            assert value == 65535, (x, y, value)
        if a == 1.0:
            assert value == 0, (x, y, value)
    # Enhance Edge on the hard result softens the edge along the ramp.
    app.call('quick_select', document_id=document, seeds=[[48, 48]], brush_size=21, smooth=1,
             enhance_edge=True)
    enhanced = coverage_at(app, document, points)
    assert any(0 < v < 65535 for v in enhanced), 'Enhance Edge leaves a soft edge'
    assert enhanced[48 * size + 48] == 65535 and enhanced[2 * size + 2] == 0
    app.refused('quick_select', 'brush_size', document_id=document, seeds=[[48, 48]],
                brush_size=600)
    app.refused('quick_select', 'smooth', document_id=document, seeds=[[48, 48]], smooth=9)
    return dict(selected=sum(1 for v in first if v))


def object_select(app, depth, photos, folder):
    document = opened(app, photo(photos, folder, 'portrait.png', f'object-{depth}'), depth)
    summary = app.call('get_document', document_id=document)
    width, height = summary['width'], summary['height']
    subjects = app.call('inspect_subjects', document_id=document)['instances']
    assert subjects, 'the portrait has a foreground object'
    box = subjects[0]['bounds']
    # A region a little inside the object's box on the left, so the result is
    # clipped by the region there.
    region = dict(x=box['x'] + box['width'] // 6, y=max(0, box['y'] - 4),
                  width=box['width'] - box['width'] // 6 + 4, height=box['height'] + 8)
    region['width'] = min(region['width'], width - region['x'])
    region['height'] = min(region['height'], height - region['y'])
    points = grid(width, height, 4)

    def inside(x, y):
        return (region['x'] <= x < region['x'] + region['width']
                and region['y'] <= y < region['y'] + region['height'])

    start = cursor(app, document)
    reply = app.call('object_select', document_id=document, region=region)
    assert reply['ok'] and not reply['selection_empty'] and 1 in reply['instances'], reply
    assert cursor(app, document) == start + 1
    bounds = reply['bounds']
    assert bounds['x'] >= region['x'] and bounds['y'] >= region['y'], (bounds, region)
    assert bounds['x'] + bounds['width'] <= region['x'] + region['width'], (bounds, region)
    assert bounds['y'] + bounds['height'] <= region['y'] + region['height'], (bounds, region)
    found = coverage_at(app, document, points)
    for (x, y), value in zip(points, found):
        assert value == 0 or inside(x, y), (x, y, value)
    app.call('select_subjects', document_id=document, instances=reply['instances'])
    whole = coverage_at(app, document, points)
    both = sum(1 for (x, y), a, b in zip(points, found, whole)
               if a >= 32768 and b >= 32768 and inside(x, y))
    either = sum(1 for (x, y), a, b in zip(points, found, whole)
                 if a >= 32768 or (b >= 32768 and inside(x, y)))
    assert either and both / either >= 0.85, (both, either)
    # A corner with no object in it: one where Select Objects' own region hit
    # test (every instance-map cell the region touches) finds nothing.
    corners = [dict(x=x, y=y, width=24, height=24)
               for x, y in ((0, 0), (width - 24, 0), (0, height - 24), (width - 24, height - 24))]
    empty = None
    for corner in corners:
        result = app.client.rpc('tools/call', {'name': 'select_subjects', 'arguments': dict(
            document_id=document, region=corner)})
        if result.get('isError'):
            empty = corner
            break
    assert empty is not None, 'the portrait leaves no empty corner'
    before = cursor(app, document)
    app.refused('object_select', 'No object was found inside the region',
                document_id=document, region=empty)
    assert cursor(app, document) == before
    app.refused('object_select', 'exactly one of region or points', document_id=document)
    # A negative size is refused by name, not read as its absolute value.
    app.refused('object_select', 'width and height must be positive',
                document_id=document, region=dict(region, width=-50))
    app.refused('object_select', 'width and height must be positive',
                document_id=document, region=dict(region, height=-50))
    assert cursor(app, document) == before
    # A closed path names the same object.
    path = [[region['x'], region['y']], [region['x'] + region['width'], region['y']],
            [region['x'] + region['width'], region['y'] + region['height']],
            [region['x'], region['y'] + region['height']]]
    lasso = app.call('object_select', document_id=document, points=path, refine=False)
    assert lasso['instances'] == reply['instances'], lasso
    return dict(instances=reply['instances'], iou=round(both / either, 3))


def quick_photo(app, depth, photos, folder):
    document = opened(app, photo(photos, folder, 'fur.png', f'quick-{depth}'), depth)
    summary = app.call('get_document', document_id=document)
    width, height = summary['width'], summary['height']
    seeds = [[width // 2 - 20, height // 2], [width // 2, height // 2],
             [width // 2 + 20, height // 2]]
    start = cursor(app, document)
    # Tabby fur fills the frame, so the default tolerance (25) reaches nearly
    # all of it; 12 with an 8 edge keeps the stroke's own patch of fur.
    reply = app.call('quick_select', document_id=document, seeds=seeds, brush_size=60, smooth=1,
                     tolerance=12, edge=8)
    assert reply['ok'] and not reply['selection_empty'], reply
    assert cursor(app, document) == start + 1
    at_seeds = coverage_at(app, document, [tuple(seed) for seed in seeds])
    assert all(value > 0 for value in at_seeds), at_seeds
    sample = coverage_at(app, document, grid(width, height, 3))
    share = sum(1 for value in sample if value) / len(sample)
    assert 0.02 <= share <= 0.80, share
    return dict(share=round(share, 3))


def refine_photo(app, depth, photos, folder):
    document = opened(app, photo(photos, folder, 'product.jpg', f'refine-{depth}'), depth)
    reply = app.call('select_subject', document_id=document)
    assert not reply.get('selection_empty'), reply
    bounds = app.call('get_selection', document_id=document)['bounds']
    # A window on the subject's left edge, read in full before and after.
    side = 160
    wx = max(0, bounds['x'] - side // 2)
    wy = max(0, bounds['y'] + bounds['height'] // 2 - side // 2)
    window = [(wx + x, wy + y) for y in range(side) for x in range(side)]
    before = coverage_at(app, document, window)
    assert any(v >= 32768 for v in before) and any(v < 32768 for v in before), 'no edge in view'
    start = cursor(app, document)
    refined = app.call('refine_selection', document_id=document, edge_radius=8)
    assert refined['changed'], refined
    assert cursor(app, document) == start + 1
    after = coverage_at(app, document, window)
    changed = 0
    for index, (was, now) in enumerate(zip(before, after)):
        if was == now:
            continue
        changed += 1
        x, y = index % side, index // side
        # A change close to the window's rim may answer to an edge outside it.
        if 10 <= x < side - 10 and 10 <= y < side - 10:
            assert near_input_edge(before, side, x, y, 9), (wx + x, wy + y, was, now)
    assert changed > 0
    grown = app.call('get_selection', document_id=document)['bounds']
    assert grown['x'] >= bounds['x'] - 9 and grown['y'] >= bounds['y'] - 9, (grown, bounds)
    assert grown['x'] + grown['width'] <= bounds['x'] + bounds['width'] + 9, (grown, bounds)
    assert grown['y'] + grown['height'] <= bounds['y'] + bounds['height'] + 9, (grown, bounds)
    return dict(changed=changed)


def reference(app, mode):
    command = [sys.executable, str(ROOT / 'scripts/generate_mcp_reference.py'),
               '--socket', app.client.socket_path]
    if mode == 'check':
        command.append('--check')
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    assert done.returncode == 0, (done.returncode, done.stdout, done.stderr)
    return dict(mode=mode, stdout=done.stdout.strip(), stderr=done.stderr.strip())


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    parser.add_argument('--photos', type=Path, default=PHOTOS,
                        help='the pinned photo fixtures (portrait.png, fur.png, product.jpg)')
    parser.add_argument('--reference', choices=['write', 'check'],
                        help='regenerate or check the MCP reference page against this instance')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-selection-'))
    output.mkdir(parents=True, exist_ok=True)
    photos = arguments.photos.expanduser()
    for name in ('portrait.png', 'fur.png', 'product.jpg'):
        assert (photos / name).is_file(), f'missing photo fixture {photos / name}'

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
            run(f'edge {depth}-bit', lambda: edge(app, depth, output))
            run(f'edge 512 {depth}-bit', lambda: edge_512(app, depth, output))
            run(f'outputs {depth}-bit', lambda: outputs(app, depth, output))
            run(f'twice {depth}-bit', lambda: twice(app, depth, output))
            run(f'decontaminate {depth}-bit', lambda: decontaminate(app, depth, output))
            run(f'brush {depth}-bit', lambda: brush(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'quick {depth}-bit', lambda: quick(app, depth, output))
            run(f'object select photo {depth}-bit',
                lambda: object_select(app, depth, photos, output))
            run(f'quick select photo {depth}-bit',
                lambda: quick_photo(app, depth, photos, output))
            run(f'refine photo {depth}-bit', lambda: refine_photo(app, depth, photos, output))
        if arguments.reference:
            run('mcp reference', lambda: reference(app, arguments.reference))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
