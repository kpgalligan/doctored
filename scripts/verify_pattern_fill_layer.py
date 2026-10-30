#!/usr/bin/env python3
"""MCP drive for pattern fill layers, phases 1, 2 and 3.

The script owns its app instance: the Debug app on its own scratch MCP socket
with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit
pin, a private pasteboard and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process. Every expected number is computed here, by this file's own oracle,
never read back from the app.

THE ORACLE. A tile's codes are document values: a texel code c reads c / 255
at 8 and 16 bits (16-bit code c x 257) and c / 255 as linear light at 32
bits. At scale 1 and angle 0 with an integer origin O, pixel (x, y) is
tile[(x - Ox) mod w, (y - Oy) mod h]. At scale 0.5 on an even tile it is the
periodic 2 x 2 box mean of the tile, read at residual scale 1. O is phase +
the layer's reference point when linked, phase when unlinked; a layer is made
at reference (0, 0), and Canvas Size, a flip and a turn map it (translation;
FlipH (W - x, y); R90 (H - y, x)). A canvas quarter turn or flip also turns a
LINKED fill's tile: FlipH T'(i, j) = T(w - 1 - i, j), R90 T'(i, j) =
T(j, h - 1 - i), computed here. In a CMYK document the oracle is structural:
the inks are periodic with the tile, distinct texels give distinct inks, and a
round trip or a mode conversion back to RGB gives back the same numbers.

Cases, each at 8, 16 and 32 bits (CMYK at 8 and 16, the depths a CMYK
document has):
- CREATE: a custom 4 x 3 tile at phase [1, 2] matches the oracle and adds one
  history step.
- EDIT: scale 0.5 on a 4 x 2 tile matches the box mean in one step; undo
  restores the CREATE pixels.
- RZ: a 300 x 300 custom tile (a description over 64 KiB, `.rz` word 102)
  survives save_copy rz and reopen with the same `fill` object and pixels.
- REFIT: a growing canvas_size moves a linked tile's origin by the
  translation and keeps an unlinked one at (0, 0).
- COPY/PASTE: the layer copied (mode layers) and pasted into another document
  is still a pattern fill layer with the same `fill` pattern.
- RASTERIZE: rasterize_artwork drops the `fill` object and keeps the pixels.
- TURN (phase 3): a linked and an unlinked fill on the asymmetric 4 x 3 tile
  on 13 x 9, then flip horizontal, then rotate 90. The linked fill's
  `fill.pattern` is the turned custom T (3 x 4, same name) and its pixels are
  the Python-turned tile (FlipH, then R90) at O = phase + the mapped reference
  ((13, 0), then (9, 13)); the unlinked fill keeps T at O = phase.
- TURN-FT: transform_layer scale_x -1 (Free Transform's mirror, matrix
  [-1, 0, 0, 1] about the centre) keeps `fill.pattern` and the pixels are the
  UNTURNED tile at O = phase + M(0, 0) = phase + (13, 0) (KEEP).
- ACTIONS: start_recording, add_fill_layer, stop_recording and run_action on
  a new document rebuild the same fill and pixels.
- CMYK (phase 3, 8 and 16 bits): add_fill_layer kind pattern takes one step
  and reports the fill; the inks (sample_color ink16) are periodic with the
  tile and its twelve texels give twelve distinct inks; undo removes it.
  edit_fill_layer scale 0.5 takes one step. save_copy rz and export_psd
  layers each reopen with an equal `fill` and equal inks. convert_mode cmyk
  on an RGB document holding one keeps the `fill` object, and back to RGB
  the pixels are the RGB oracle. A step recorded on a CMYK document carries
  the tile; playback refuses in any CMYK document (unchanged), and the
  action run on an RGB document of the same depth rebuilds the fill.
- MOVE (phase 2): move_layers (5, -3) on a linked fill with a rect mask takes
  one step; the layer still covers the canvas, the `fill` object is unchanged,
  the pixels are the oracle at O = phase + (5, -3) and the mask's bounds moved
  (5, -3). An unlinked fill moved the same stays at O = phase. Undo restores.
- OFFSET: set_layer_properties offset (4, 1) is the same as a move (4, 1).
- TRANSFORM: transform_layer scale 0.5 about (8, 6) reports rasterized false,
  keeps the `fill` object (KEEP) and matches the oracle at O = phase +
  round(M(0, 0)) = phase + (4, 3).
- TRANSLATE: transform_layer translate (10, 0) keeps the fill at O = phase +
  (10, 0).
- LOCK: Lock Pixels, then move_layers, is refused with the locked sentence
  and takes no step.
- MIXED: a pattern fill linked to a solid fill: move_layers on the pattern
  fill is refused with the solid's pin sentence and takes no step.
- ALIGN: align_layers to canvas and distribute_layers over a pattern fill and
  two plain layers are refused with the Align and Distribute sentence and
  take no step.
- PSD (8 and 16 bits): export_psd layers, then reopen through inspect_psd and
  open_document psd_mode layers: the `fill` objects and pixels are equal.

`build_exports(call, folder, run_id)` is the Photoshop take's WRITE set
(`photoshop-probe/pattern_fill_layer_take.py`): it writes no Photoshop file.

Usage: python3 scripts/verify_pattern_fill_layer.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import base64
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

class App:
    """The one owned Debug instance, on its own scratch socket."""

    def __init__(self, folder):
        self.folder = Path(folder)
        self.socket = None
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
            DR_PASTEBOARD_NAME='rz-verify-pattern-fill-layer',
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


def new_document(app, depth, width, height, **extra):
    arguments = dict(width=width, height=height, depth=depth, profile='srgb', background='white')
    arguments.update(extra)
    return app.call('new_document', **arguments)['document']['id']


def canvas(app, document):
    summary = app.call('get_document', document_id=document)
    return summary['width'], summary['height']


def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def unit(app, document, depth, layer, x, y):
    """Layer `layer`'s pixel at (x, y) as four numbers in the storage's own
    unit scaled to 0..1 (8 and 16 bits) or as stored (32-bit linear light)."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    if depth == 32:
        return [float(v) for v in reply['center_rgba32']]
    scale = 65535.0 if depth == 16 else 255.0
    return [v / scale for v in reply['center_rgba_codes']]


def close(a, b, eps):
    return all(abs(x - y) <= eps for x, y in zip(a, b))


# One code of the storage: the exact-code oracle's bound.
EXACT = {8: 0.5 / 255, 16: 0.5 / 65535, 32: 1e-6}
# The box mean is rounded once more at 8 bits (u16, then u8).
MEAN = {8: 1.01 / 255, 16: 1.01 / 65535, 32: 1e-5}


class Tile:
    def __init__(self, width, height, texel):
        self.width, self.height = width, height
        self.rgba = bytes(c for y in range(height) for x in range(width) for c in texel(x, y))

    def at(self, x, y):
        i = (y * self.width + x) * 4
        return list(self.rgba[i:i + 4])

    def custom(self, name='T'):
        return {'name': name, 'width': self.width, 'height': self.height,
                'rgba': base64.b64encode(self.rgba).decode()}

    def oracle(self, origin, x, y):
        """Scale 1: the texel under (x, y) with the tile's origin at `origin`, unit."""
        return [c / 255 for c in self.at((x - origin[0]) % self.width,
                                         (y - origin[1]) % self.height)]

    def box_mean(self, i):
        """Level one of an even opaque tile with height 2: texel i's 2 x 2 mean."""
        texels = [self.at((2 * i + dx) % self.width, dy) for dx in (0, 1) for dy in (0, 1)]
        return [sum(t[c] for t in texels) / 4 / 255 for c in range(4)]


# An asymmetric opaque 4 x 3 tile: every texel distinct, so a flip shows.
T43 = Tile(4, 3, lambda x, y: (40 * x + 20, 60 * y + 30, (3 * x + 5 * y) * 10 + 5, 255))
# An opaque 4 x 2 tile for the half-scale box.
T42 = Tile(4, 2, lambda x, y: (50 * x + 10, 100 * y + 40, 200 - 30 * x - 70 * y, 255))
# 300 x 300: the description is past 64 KiB (about 480 KB of base64).
BIG = Tile(300, 300, lambda x, y: (x % 256, y % 256, (x + 2 * y) % 256, 255))


def assert_tile(app, document, depth, layer, tile, origin, points, what):
    for x, y in points:
        got = unit(app, document, depth, layer, x, y)
        want = tile.oracle(origin, x, y)
        assert close(got, want, EXACT[depth]), (what, depth, (x, y), got, want)


def every(width, height):
    return [(x, y) for y in range(height) for x in range(width)]


def add_pattern(app, document, pattern, **extra):
    return app.call('add_fill_layer', document_id=document, kind='pattern', pattern=pattern,
                    **extra)


# ---------------------------------------------------------------- cases --

def create_edit(app, depth):
    width, height = 13, 9
    document = new_document(app, depth, width, height)
    start = cursor(app, document)
    reply = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})
    layer = reply['layer']
    fill = reply['fill']
    assert fill['kind'] == 'pattern', reply
    pattern = fill['pattern']
    assert pattern['pattern'] == {'name': 'T', 'width': 4, 'height': 3}, pattern
    assert pattern['link_with_layer'] is True and pattern['scale'] == 1, pattern
    assert pattern['phase'] == [1, 2] and pattern['angle'] == 0, pattern
    assert reply['name'] == 'Pattern Fill', reply
    assert cursor(app, document) == start + 1, 'CREATE is one step'
    row = layers(app, document)[layer]
    assert row['fill'] == fill, (row['fill'], fill)
    # Made at reference (0, 0): O = phase.
    assert_tile(app, document, depth, layer, T43, (1, 2), every(width, height), 'CREATE')

    # EDIT: the 4 x 2 tile at scale 0.5, one step; the other keys kept.
    start = cursor(app, document)
    edited = app.call('edit_fill_layer', document_id=document, layer=layer,
                      pattern={'pattern': T42.custom('Half'), 'scale': 0.5})
    assert edited['changed'], edited
    assert edited['fill']['pattern']['phase'] == [1, 2], edited
    assert edited['fill']['pattern']['scale'] == 0.5, edited
    assert cursor(app, document) == start + 1, 'EDIT is one step'
    for x, y in every(width, height):
        i = (x - 1) % 2
        got = unit(app, document, depth, layer, x, y)
        want = T42.box_mean(i)
        assert close(got, want, MEAN[depth]), ('EDIT', depth, (x, y), got, want)
    # Refusals take no step.
    app.refused('edit_fill_layer', 'pattern has no key "anchor"', document_id=document,
                layer=layer, pattern={'anchor': [0, 0]})
    app.refused('edit_fill_layer', 'is a pattern fill', document_id=document, layer=layer,
                color='#FF0000')
    app.refused('add_fill_layer', 'A solid fill takes color', document_id=document,
                kind='solid', color='#FF0000', pattern={'scale': 1})
    app.refused('edit_fill_layer', 'pattern.pattern.rgba', document_id=document, layer=layer,
                pattern={'pattern': {'width': 4, 'height': 3, 'rgba': 'AAAA'}})
    assert cursor(app, document) == start + 1, 'a refusal takes no step'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert layers(app, document)[layer]['fill'] == fill, 'undo restores the description'
    assert_tile(app, document, depth, layer, T43, (1, 2), every(width, height), 'EDIT undo')
    return dict(layer=layer)


def rz(app, depth, folder):
    width, height = 40, 30
    document = new_document(app, depth, width, height)
    reply = add_pattern(app, document, {'pattern': BIG.custom('Big'), 'phase': [5, 7],
                                        'link_with_layer': False})
    layer = reply['layer']
    points = [(0, 0), (39, 0), (0, 29), (39, 29), (17, 11), (4, 6), (5, 7), (33, 21)]
    assert_tile(app, document, depth, layer, BIG, (5, 7), points, 'RZ before')
    path = Path(folder) / f'pattern-{depth}.rz'
    app.call('save_copy', document_id=document, path=str(path), format='rz')
    data = path.read_bytes()
    # The identity wrapper (45 or 46), then word 102 first inside it.
    assert int.from_bytes(data[8:12], 'little') == 102, 'word 102 claimed'
    reopened = app.call('open_document', path=str(path))['document']['id']
    before, after = layers(app, document)[layer], layers(app, reopened)[layer]
    assert after['fill'] == before['fill'], (after['fill'], before['fill'])
    for x, y in points:
        assert unit(app, reopened, depth, layer, x, y) == unit(app, document, depth, layer, x, y)
    # Still editable after the reopen.
    app.call('edit_fill_layer', document_id=reopened, layer=layer, pattern={'phase': [6, 7]})
    assert_tile(app, reopened, depth, layer, BIG, (6, 7), points, 'RZ edited')
    return dict(bytes=len(data))


def refit(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    linked = add_pattern(app, document, {'pattern': T43.custom()})['layer']
    unlinked = add_pattern(app, document, {'pattern': T43.custom(),
                                           'link_with_layer': False})['layer']
    reply = app.call('canvas_size', document_id=document, width=width + 6, height=height + 4,
                     anchor='center')
    assert reply.get('ok'), reply
    assert canvas(app, document) == (width + 6, height + 4)
    points = [(x, y) for y in (0, 1, 2, 9) for x in range(width + 6)]
    # The old canvas lands at (3, 2): the linked layer's reference moves there.
    assert_tile(app, document, depth, linked, T43, (3, 2), points, 'REFIT linked')
    assert_tile(app, document, depth, unlinked, T43, (0, 0), points, 'REFIT unlinked')
    for index in (linked, unlinked):
        row = layers(app, document)[index]
        assert row['fill']['canvas'] == {'width': width + 6, 'height': height + 4}, row['fill']
    return dict(translation=[3, 2])


def copy_paste(app, depth):
    width, height = 12, 8
    source = new_document(app, depth, width, height)
    layer = add_pattern(app, source, {'pattern': T43.custom(), 'phase': [2, 1]})['layer']
    fill = layers(app, source)[layer]['fill']
    app.call('copy', document_id=source, mode='layers', layers=[layer])
    target = new_document(app, depth, width, height)
    reply = app.call('paste', document_id=target)
    pasted = reply.get('layer')
    rows = layers(app, target)
    found = [i for i, row in enumerate(rows) if 'fill' in row]
    assert found, ('no fill layer after the paste', reply, rows)
    row = rows[found[-1]]
    assert row['fill']['kind'] == 'pattern', row
    assert row['fill']['pattern'] == fill['pattern'], (row['fill'], fill)
    return dict(pasted=pasted, index=found[-1])


def rasterize(app, depth):
    width, height = 10, 7
    document = new_document(app, depth, width, height)
    layer = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [3, 1]})['layer']
    points = every(width, height)
    before = [unit(app, document, depth, layer, x, y) for x, y in points]
    app.call('rasterize_artwork', document_id=document, layer=layer)
    assert 'fill' not in layers(app, document)[layer], layers(app, document)[layer]
    after = [unit(app, document, depth, layer, x, y) for x, y in points]
    assert after == before, 'rasterize keeps the pixels'
    return dict(pixels=len(points))


def turned(tile, flip=False):
    """The tile after FlipH (`flip`) or R90, by the closed forms (docstring)."""
    w, h = tile.width, tile.height
    if flip:
        return Tile(w, h, lambda i, j: tile.at(w - 1 - i, j))
    return Tile(h, w, lambda i, j: tile.at(j, h - 1 - i))


def turn(app, depth):
    width, height = 13, 9
    document = new_document(app, depth, width, height)
    phase = (1, 2)
    linked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': list(phase)})['layer']
    unlinked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': list(phase),
                                           'link_with_layer': False})['layer']
    reply = app.call('flip', document_id=document, axis='horizontal')
    assert reply.get('ok', True), reply
    # Reference (0, 0) maps to (W, 0); the tile turns with the canvas.
    flipped = turned(T43, flip=True)
    pattern = layers(app, document)[linked]['fill']['pattern']['pattern']
    assert pattern == {'name': 'T', 'width': 4, 'height': 3}, pattern
    assert_tile(app, document, depth, linked, flipped, (phase[0] + width, phase[1]),
                every(width, height), 'TURN flip')
    reply = app.call('rotate', document_id=document, degrees=90)
    assert reply.get('ok', True), reply
    assert canvas(app, document) == (height, width)
    # (13, 0) maps by R90 on 13 x 9 to (9 - 0, 13).
    both = turned(flipped)
    origin = (phase[0] + 9, phase[1] + 13)
    pattern = layers(app, document)[linked]['fill']['pattern']['pattern']
    assert pattern == {'name': 'T', 'width': 3, 'height': 4}, pattern
    assert_tile(app, document, depth, linked, both, origin, every(height, width), 'TURN linked')
    pattern = layers(app, document)[unlinked]['fill']['pattern']['pattern']
    assert pattern == {'name': 'T', 'width': 4, 'height': 3}, ('unlinked keeps T', pattern)
    assert_tile(app, document, depth, unlinked, T43, phase, every(height, width),
                'TURN unlinked')
    return dict(origin=origin)


def turn_ft(app, depth):
    width, height = 13, 9
    document = new_document(app, depth, width, height)
    phase = (1, 2)
    layer = add_pattern(app, document, {'pattern': T43.custom(), 'phase': list(phase)})['layer']
    fill = layers(app, document)[layer]['fill']
    reply = app.call('transform_layer', document_id=document, layer=layer, scale_x=-1)
    assert reply.get('rasterized') in (False, None, []), reply
    row = covers(app, document, layer, width, height, 'TURN-FT')
    assert row['fill']['pattern'] == fill['pattern'], ('KEEP', row['fill'], fill)
    # M(p) = (6.5 - (x - 6.5), y): the reference (0, 0) maps to (13, 0).
    assert_tile(app, document, depth, layer, T43, (phase[0] + width, phase[1]),
                every(width, height), 'TURN-FT')
    return dict(origin=[phase[0] + width, phase[1]])


def actions(app, depth):
    width, height = 11, 8
    source = new_document(app, depth, width, height)
    name = f'pattern-fill-{depth}'
    app.call('start_recording', discard=True)
    layer = add_pattern(app, source, {'pattern': T43.custom(), 'phase': [2, 3],
                                      'scale': 1, 'link_with_layer': True})['layer']
    stopped = app.call('stop_recording', name=name)
    steps = json.dumps(stopped)
    assert 'add_fill_layer' in steps and T43.custom()['rgba'] in steps, 'the step carries rgba'
    fill = layers(app, source)[layer]['fill']
    target = new_document(app, depth, width, height)
    app.call('run_action', document_id=target, name=name)
    rows = layers(app, target)
    found = [i for i, row in enumerate(rows) if 'fill' in row]
    assert found, rows
    assert rows[found[-1]]['fill'] == fill, (rows[found[-1]]['fill'], fill)
    assert_tile(app, target, depth, found[-1], T43, (2, 3), every(width, height), 'ACTIONS')
    return dict(layer=found[-1])


def ink(app, document, x, y):
    sample = app.call('sample_color', document_id=document, x=x, y=y)
    assert sample.get('color_mode') == 'cmyk', sample
    return sample['ink16'] + [sample['alpha16']]


def inks(app, document, points):
    return [ink(app, document, x, y) for x, y in points]


def cmyk_document(app, depth, width, height):
    return new_document(app, depth, width, height, color_mode='cmyk', profile='working')


def cmyk(app, depth, folder):
    width, height = 8, 6
    document = cmyk_document(app, depth, width, height)
    points = every(width, height)
    start = cursor(app, document)
    reply = add_pattern(app, document, {'pattern': T43.custom()})
    layer = reply['layer']
    assert reply['fill']['kind'] == 'pattern', reply
    assert cursor(app, document) == start + 1, 'CMYK create is one step'
    fill = layers(app, document)[layer]['fill']
    made = inks(app, document, points)
    # Linked, made at reference (0, 0): O = (0, 0). Periodic with the tile,
    # twelve distinct texels, twelve distinct inks.
    at = {p: v for p, v in zip(points, made)}
    for x, y in points:
        assert at[(x, y)] == at[(x % 4, y % 3)], ('periodic', (x, y), at[(x, y)])
    texels = [tuple(at[(x, y)]) for y in range(3) for x in range(4)]
    assert len(set(texels)) == 12, ('distinct inks', texels)
    assert all(v[4] == 65535 for v in made), 'opaque'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert len(layers(app, document)) == layer, 'undo removes the fill layer'
    app.call('redo', document_id=document)
    assert inks(app, document, points) == made, 'redo restores it'
    # EDIT: one step.
    before = cursor(app, document)
    edited = app.call('edit_fill_layer', document_id=document, layer=layer,
                      pattern={'scale': 0.5})
    assert edited['changed'], edited
    assert cursor(app, document) == before + 1, 'CMYK edit is one step'
    fill = layers(app, document)[layer]['fill']
    edited_inks = inks(app, document, points)
    # RZ and PSD round trips.
    rz_path = Path(folder) / f'pattern-cmyk-{depth}.rz'
    app.call('save_copy', document_id=document, path=str(rz_path), format='rz')
    reopened = app.call('open_document', path=str(rz_path))['document']['id']
    assert layers(app, reopened)[layer]['fill'] == fill, layers(app, reopened)[layer]
    assert inks(app, reopened, points) == edited_inks, 'rz inks'
    psd_path = Path(folder) / f'pattern-cmyk-{depth}.psd'
    app.call('export_psd', document_id=document, path=str(psd_path), mode='layers', format='psd')
    inspected = app.call('inspect_psd', path=str(psd_path))
    assert any('Pattern Fill layer (PtFl)' in o for o in inspected['observations']), inspected
    opened = app.call('open_document', path=str(psd_path), psd_mode='layers',
                      psd_fingerprint=inspected['source_fingerprint'])['document']['id']
    assert layers(app, opened)[layer]['fill'] == fill, layers(app, opened)[layer]
    assert inks(app, opened, points) == edited_inks, 'psd inks'
    # Mode: an RGB document holding one converts both ways keeping the fill.
    rgb = new_document(app, depth, width, height)
    rgb_layer = add_pattern(app, rgb, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    rgb_fill = layers(app, rgb)[rgb_layer]['fill']
    app.call('convert_mode', document_id=rgb, mode='cmyk', profile='working')
    assert layers(app, rgb)[rgb_layer]['fill']['pattern'] == rgb_fill['pattern'], 'to CMYK'
    app.call('convert_mode', document_id=rgb, mode='rgb', rgb_space='srgb')
    assert layers(app, rgb)[rgb_layer]['fill']['pattern'] == rgb_fill['pattern'], 'to RGB'
    assert_tile(app, rgb, depth, rgb_layer, T43, (1, 2), points, 'CMYK -> RGB')
    # Actions: a step recorded on a CMYK document carries the tile. Playback
    # is unavailable in any CMYK document (CmykDocument's rule, unchanged), so
    # it replays on an RGB document of the same depth, rebuilding the fill.
    name = f'pattern-fill-cmyk-{depth}'
    source = cmyk_document(app, depth, width, height)
    app.call('start_recording', discard=True)
    recorded = add_pattern(app, source, {'pattern': T43.custom(), 'phase': [2, 1]})['layer']
    steps = json.dumps(app.call('stop_recording', name=name))
    assert 'add_fill_layer' in steps and T43.custom()['rgba'] in steps, 'the step carries rgba'
    app.refused('run_action', 'unavailable in a CMYK document',
                document_id=cmyk_document(app, depth, width, height), name=name)
    target = new_document(app, depth, width, height)
    app.call('run_action', document_id=target, name=name)
    rows = layers(app, target)
    found = [i for i, row in enumerate(rows) if 'fill' in row]
    assert found, rows
    want = layers(app, source)[recorded]['fill']
    assert rows[found[-1]]['fill']['pattern'] == want['pattern'], (rows[found[-1]], want)
    assert_tile(app, target, depth, found[-1], T43, (2, 1), points, 'CMYK ACTIONS')
    return dict(layer=layer)


# ------------------------------------------------------- phase 2 cases --

LOCKED = 'is a pattern fill layer and is locked'
PINNED = 'is a fill layer: it always covers the canvas'
ALIGNED = 'Align and Distribute do not move it'


def rect_of(row):
    if 'offset' in row:
        x, y = row['offset']
    else:
        x, y = row['offset_x'], row['offset_y']
    return [x, y, row['width'], row['height']]


def sample_points(width, height):
    """Three whole rows and the four corners: every texel column and row
    phase of the 4 x 3 tile appears."""
    points = [(x, y) for y in (0, 4, height - 1) for x in range(width)]
    return points + [(0, height - 2), (width - 1, 1), (3, 7), (9, 10)]


def mask_bounds(app, document, layer):
    app.call('load_selection', document_id=document, **{'from': 'layer_mask'}, layer=layer)
    reply = app.call('get_selection', document_id=document)
    app.call('deselect', document_id=document)
    b = reply['bounds']
    return (b['x'], b['y'], b['width'], b['height'])


def covers(app, document, layer, width, height, what):
    row = layers(app, document)[layer]
    assert rect_of(row) == [0, 0, width, height], (what, rect_of(row))
    return row


def move(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    linked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    app.call('select_rect', document_id=document, x=2, y=3, width=8, height=5)
    app.call('add_layer_mask', document_id=document, layer=linked, kind='from_selection')
    app.call('deselect', document_id=document)
    unlinked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2],
                                           'link_with_layer': False})['layer']
    fills = {i: layers(app, document)[i]['fill'] for i in (linked, unlinked)}
    bounds = mask_bounds(app, document, linked)
    assert bounds == (2, 3, 8, 5), ('the mask', bounds)
    points = sample_points(width, height)
    start = cursor(app, document)
    app.call('move_layers', document_id=document, layers=[linked], dx=5, dy=-3)
    assert cursor(app, document) == start + 1, 'MOVE is one step'
    row = covers(app, document, linked, width, height, 'MOVE linked')
    assert row['fill'] == fills[linked], (row['fill'], fills[linked])
    assert_tile(app, document, depth, linked, T43, (6, -1), points, 'MOVE linked')
    app.call('move_layers', document_id=document, layers=[unlinked], dx=5, dy=-3)
    assert cursor(app, document) == start + 2, 'the unlinked MOVE is one step'
    row = covers(app, document, unlinked, width, height, 'MOVE unlinked')
    assert row['fill'] == fills[unlinked], (row['fill'], fills[unlinked])
    assert_tile(app, document, depth, unlinked, T43, (1, 2), points, 'MOVE unlinked')
    # Reading the mask loads and drops a selection, which are history steps
    # of their own, so it is read after the moves are counted.
    bounds = mask_bounds(app, document, linked)
    assert bounds == (7, 0, 8, 5), ('the mask moved', bounds)
    while cursor(app, document) > start:
        app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert_tile(app, document, depth, linked, T43, (1, 2), points, 'MOVE undo')
    bounds = mask_bounds(app, document, linked)
    assert bounds == (2, 3, 8, 5), ('undo restores the mask', bounds)
    return dict(origin=[6, -1])


def offset(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    layer = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    fill = layers(app, document)[layer]['fill']
    start = cursor(app, document)
    app.call('set_layer_properties', document_id=document, index=layer, offset_x=4, offset_y=1)
    assert cursor(app, document) == start + 1, 'OFFSET is one step'
    row = covers(app, document, layer, width, height, 'OFFSET')
    assert row['fill'] == fill, (row['fill'], fill)
    assert_tile(app, document, depth, layer, T43, (5, 3), sample_points(width, height), 'OFFSET')
    return dict(origin=[5, 3])


def transform(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    layer = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    fill = layers(app, document)[layer]['fill']
    start = cursor(app, document)
    reply = app.call('transform_layer', document_id=document, layer=layer, scale=0.5,
                     pivot_x=8, pivot_y=6)
    assert reply.get('rasterized') in (False, None, []), reply
    assert cursor(app, document) == start + 1, 'TRANSFORM is one step'
    row = covers(app, document, layer, width, height, 'TRANSFORM')
    assert row['fill'] == fill, ('KEEP', row['fill'], fill)
    # M(p) = (8, 6) + 0.5 (p - (8, 6)): the reference (0, 0) maps to (4, 3).
    assert_tile(app, document, depth, layer, T43, (5, 5), sample_points(width, height),
                'TRANSFORM')
    return dict(origin=[5, 5])


def translate(app, depth):
    width, height = 16, 12
    document = new_document(app, depth, width, height)
    layer = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    fill = layers(app, document)[layer]['fill']
    app.call('transform_layer', document_id=document, layer=layer, translate_x=10)
    row = covers(app, document, layer, width, height, 'TRANSLATE')
    assert row['fill'] == fill, (row['fill'], fill)
    assert_tile(app, document, depth, layer, T43, (11, 2), sample_points(width, height),
                'TRANSLATE')
    return dict(origin=[11, 2])


def lock(app, depth):
    document = new_document(app, depth, 12, 8)
    layer = add_pattern(app, document, {'pattern': T43.custom()})['layer']
    app.call('set_layer_lock', document_id=document, layer=layer, locks=['pixels'])
    start = cursor(app, document)
    text = app.refused('move_layers', LOCKED, document_id=document, layers=[layer], dx=2, dy=1)
    assert cursor(app, document) == start, 'the refusal takes no step'
    return dict(refusal=text[:120])


def mixed(app, depth):
    document = new_document(app, depth, 12, 8)
    pattern = add_pattern(app, document, {'pattern': T43.custom()})['layer']
    solid = app.call('add_fill_layer', document_id=document, kind='solid',
                     color='#FF0000')['layer']
    app.call('link_layers', document_id=document, layers=[pattern, solid])
    start = cursor(app, document)
    text = app.refused('move_layers', PINNED, document_id=document, layers=[pattern], dx=2,
                       dy=1)
    assert cursor(app, document) == start, 'the refusal takes no step'
    return dict(refusal=text[:120])


def align(app, depth):
    document = new_document(app, depth, 12, 8)
    pattern = add_pattern(app, document, {'pattern': T43.custom()})['layer']
    plain = []
    for name in ('A', 'B'):
        app.call('new_layer', document_id=document, name=name)
        plain.append(len(layers(app, document)) - 1)
    start = cursor(app, document)
    app.refused('align_layers', ALIGNED, document_id=document, layers=[pattern] + plain,
                edge='left', to='canvas')
    app.refused('distribute_layers', ALIGNED, document_id=document, layers=[pattern] + plain,
                axis='horizontal')
    assert cursor(app, document) == start, 'the refusals take no step'
    return dict(layers=[pattern] + plain)


def psd(app, depth, folder):
    width, height = 24, 16
    document = new_document(app, depth, width, height)
    linked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [1, 2]})['layer']
    app.call('move_layers', document_id=document, layers=[linked], dx=3, dy=2)
    unlinked = add_pattern(app, document, {'pattern': T43.custom(), 'phase': [2, 1],
                                           'link_with_layer': False})['layer']
    path = Path(folder) / f'pattern-fill-{depth}.psd'
    app.call('export_psd', document_id=document, path=str(path), mode='layers', format='psd')
    inspected = app.call('inspect_psd', path=str(path))
    assert any('Pattern Fill layer (PtFl)' in o for o in inspected['observations']), inspected
    reopened = app.call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=inspected['source_fingerprint'])['document']['id']
    points = sample_points(width, height)
    for index in (linked, unlinked):
        before, after = layers(app, document)[index], layers(app, reopened)[index]
        assert after['fill'] == before['fill'], (index, after['fill'], before['fill'])
        for x, y in points:
            assert unit(app, reopened, depth, index, x, y) == unit(app, document, depth, index,
                                                                    x, y), (index, x, y)
    return dict(path=str(path))


# The take's WRITE set (`pattern_fill_layer_take.py`): Doctored-written PSDs
# Photoshop opens. Tile T is take1's (12 x 8, `pattern_overlay_take.texel`).
TAKE_W, TAKE_H = 96, 64
TAKE_TILE = Tile(12, 8, lambda x, y: (20 * x + 10, 30 * y + 15,
                                      200 if (x + y) % 2 == 0 else 60, 255))
TAKE_MOVE = (7, 3)
TAKE_MASK = (16, 10, 48, 36)
WRITE_SET = [
    # (name, depth, scale, angle, linked, moved, masked)
    ('w8_l', 8, 1.0, 0, True, True, False),
    ('w8_u90', 8, 1.0, 90, False, False, False),
    ('w8_m50', 8, 0.5, 0, True, False, True),
    ('w16_l', 16, 1.0, 0, True, True, False),
]


def build_exports(call, folder, run_id):
    """The take's WRITE files: {name: {psd, flat, depth, fill, scale, angle,
    linked, moved, masked, pattern}}, each its own 96 x 64 document over white
    holding one pattern fill of tile T named `rz-670-<run id>-psd`; `psd` is
    the layered export, `flat` Doctored's flattened TIFF of the same."""
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    pattern = f'rz-670-{run_id}-psd'
    out = {}
    for name, depth, scale, angle, linked, moved, masked in WRITE_SET:
        doc = call('new_document', width=TAKE_W, height=TAKE_H, depth=depth, profile='srgb',
                   background='white')['document']['id']
        reply = call('add_fill_layer', document_id=doc, kind='pattern',
                     pattern={'pattern': TAKE_TILE.custom(pattern), 'scale': scale,
                              'angle': angle, 'link_with_layer': linked})
        layer = reply['layer']
        if masked:
            x, y, w, h = TAKE_MASK
            call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
            call('add_layer_mask', document_id=doc, layer=layer, kind='from_selection')
            call('deselect', document_id=doc)
        if moved:
            call('move_layers', document_id=doc, layers=[layer], dx=TAKE_MOVE[0],
                 dy=TAKE_MOVE[1])
        fill = call('get_document', document_id=doc)['layers'][layer]['fill']
        psd_path = str(folder / f'{name}.psd')
        call('export_psd', document_id=doc, path=psd_path, format='psd', mode='layers')
        flat = str(folder / f'{name}.rz.tif')
        call('export_psd', document_id=doc, path=flat, format='tiff', mode='flattened')
        call('close_document', document_id=doc, discard_changes=True)
        out[name] = dict(psd=psd_path, flat=flat, depth=depth, fill=fill, scale=scale,
                         angle=angle, linked=linked, moved=moved, masked=masked,
                         pattern=pattern)
    return out


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-pattern-fill-'))
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
            run(f'create+edit {depth}-bit', lambda: create_edit(app, depth))
            run(f'rz {depth}-bit', lambda: rz(app, depth, output))
            run(f'refit {depth}-bit', lambda: refit(app, depth))
            run(f'copy/paste {depth}-bit', lambda: copy_paste(app, depth))
            run(f'rasterize {depth}-bit', lambda: rasterize(app, depth))
            run(f'turn {depth}-bit', lambda: turn(app, depth))
            run(f'turn-ft {depth}-bit', lambda: turn_ft(app, depth))
            run(f'actions {depth}-bit', lambda: actions(app, depth))
            run(f'move {depth}-bit', lambda: move(app, depth))
            run(f'offset {depth}-bit', lambda: offset(app, depth))
            run(f'transform {depth}-bit', lambda: transform(app, depth))
            run(f'translate {depth}-bit', lambda: translate(app, depth))
            run(f'lock {depth}-bit', lambda: lock(app, depth))
            run(f'mixed {depth}-bit', lambda: mixed(app, depth))
            run(f'align {depth}-bit', lambda: align(app, depth))
        for depth in (8, 16):
            run(f'psd {depth}-bit', lambda: psd(app, depth, output))
        for depth in (8, 16):
            run(f'cmyk {depth}-bit', lambda: cmyk(app, depth, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
