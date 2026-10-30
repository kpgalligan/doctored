#!/usr/bin/env python3
"""MCP drive for the v1 16-bit editing batch.

The script owns its app instance: the Debug app on its own scratch MCP socket with
`DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin
and private `DR_*_DIR`s (`verification_isolation.preset_environment`). It
terminates only that process. Every expected number is computed here, by this
file's own oracle, never read back from the app.

At 8 and 16 bits:
- VIA: with a feathered selection, `layer_via_copy` lands a layer directly
  above the source whose RGB is the source's and whose alpha is the source's
  times the selection's coverage, sample by sample, and leaves the source as
  it was; `layer_via_cut` lands the same layer and leaves the source's alpha
  times (1 - coverage). Each is one history step and undo restores. With no
  selection, a layer moved 100 px off the canvas is copied whole: after both
  layers move back, the copy's left 100 columns are the source's, not
  transparent.
- GRADIENT: `gradient` with `blend_mode: "Multiply"` over a flat colour is
  the product of the two colours; `blend_mode: "Normal"` is the call with no
  `blend_mode`, sample for sample; `target: "mask"` with Multiply is refused
  by name and takes no step.
- NULL: `gradient` with `start_color: null` is refused like a call that
  names no ramp, and takes no step.

At 16 bits:
- CLONE: `clone_stamp` with `blend_mode: "Multiply"` leaves the product of
  the source and destination colours under the dab's centre and nothing
  outside it; `output: "new_layer"` with Multiply and `heal_stroke` with a
  `blend_mode` are refused and take no step. The same Multiply clone runs at
  8 bits against the same oracle, so the two depths are held to one answer.
- FILTERS: every name in `apply_filter`'s `filter` enum answers without
  `isError` on a 16-bit layer.

Usage: python3 scripts/verify_v1_16bit.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import struct
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
            DR_PASTEBOARD_NAME='rz-verify-v1-16bit',
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


def new_document(app, depth, width, height):
    return app.call('new_document', width=width, height=height, depth=depth, profile='srgb',
                    background='white')['document']['id']

def layers(app, document):
    return app.call('get_document', document_id=document)['layers']


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def top(depth):
    """The storage's full code."""
    return 65535 if depth == 16 else 255


def codes(app, document, layer, x, y):
    """Layer `layer`'s stored RGBA codes at canvas (x, y): bytes at 8 bits,
    u16 codes at 16."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    return [int(v) for v in reply['center_rgba_codes']]


def coverage16(app, document, points):
    """The selection's u16 coverage at `points`."""
    reply = app.call('get_selection', document_id=document,
                     points=[{'x': x, 'y': y} for x, y in points])
    return [sample['coverage16'] for sample in reply['samples']]


def hex_bytes(color):
    return [int(color[i:i + 2], 16) for i in (1, 3, 5)]


def near(got, want, slack):
    return all(abs(g - w) <= slack for g, w in zip(got, want))


# ---------------------------------------------------------------- cases --

def via(app, depth):
    width, height = 40, 30
    full = top(depth)
    document = new_document(app, depth, width, height)
    # An opaque left-to-right ramp, so each column has its own RGB.
    app.call('gradient', document_id=document, layer=0, x0=0, y0=0, x1=width, y1=0,
             start_color='#C03020', end_color='#2040D0')
    app.call('select_rect', document_id=document, x=8, y=6, width=14, height=10)
    app.call('modify_selection', document_id=document, operation='feather', radius=2)
    points = [(x, y) for y in range(2, 20, 2) for x in range(4, 28, 2)]
    covered = coverage16(app, document, points)
    assert any(0 < c < 65535 for c in covered) and 0 in covered and max(covered) > 60000, (
        'the selection has full, partial and empty samples')
    source = {p: codes(app, document, 0, *p) for p in points}
    assert all(px[3] == full for px in source.values()), 'the source is opaque'

    def weight(point):
        """The selection's coverage at `point` in the depth's own unit: the
        u16 code at 16 bits, its byte at 8 (the selection crosses to the byte
        core as round(code / 257))."""
        code = covered[points.index(point)]
        return code if depth == 16 else (code + 128) // 257

    # One code: the cut's erase rounds once in f32 at 16 bits, and at 8 bits
    # the byte the selection crosses as may round a half-way code either way.
    slack = 1
    seen = {}
    for tool, cut in (('layer_via_copy', False), ('layer_via_cut', True)):
        start = cursor(app, document)
        reply = app.call(tool, document_id=document, layer=0)
        assert reply.get('ok') and reply['layer'] == 1, reply
        assert 'note' not in reply or 'whole layer' not in reply['note'], reply
        if depth == 16:
            assert reply.get('storage') == 'rgba16_unorm', reply
        assert cursor(app, document) == start + 1, f'{tool} is one history step'
        rows = layers(app, document)
        assert len(rows) == 2 and rows[1]['name'] == reply['name'], rows
        for point in points:
            was, c = source[point], weight(point)
            copy = codes(app, document, 1, *point)
            want_alpha = (was[3] * c * 2 + full) // (2 * full)
            assert abs(copy[3] - want_alpha) <= slack, (tool, point, copy, want_alpha)
            if copy[3] > 0:
                assert copy[:3] == was[:3], (tool, point, copy, was)
            now = codes(app, document, 0, *point)
            want_left = (was[3] * (full - c) * 2 + full) // (2 * full) if cut else was[3]
            assert abs(now[3] - want_left) <= (slack if cut else 0), (tool, point, now, want_left)
            # A sample the cut emptied holds no colour (the 8-bit Clear zeroes it).
            if now[3] > 0:
                assert now[:3] == was[:3], (tool, point, 'source RGB', now, was)
        app.call('undo', document_id=document)
        assert cursor(app, document) == start and len(layers(app, document)) == 1, tool
        assert all(codes(app, document, 0, *p) == source[p] for p in points[::7]), (
            f'undo restores the source after {tool}')
        seen[tool] = reply['name']

    # No selection: the whole layer is copied, the part hanging off the canvas
    # included.
    wide = new_document(app, depth, 160, 20)
    app.call('gradient', document_id=wide, layer=0, x0=0, y0=0, x1=160, y1=0,
             start_color='#C03020', end_color='#2040D0')
    columns = (0, 37, 99, 100, 159)
    before = {x: codes(app, wide, 0, x, 10) for x in columns}
    app.call('move_layers', document_id=wide, layers=[0], dx=-100, dy=0)
    reply = app.call('layer_via_copy', document_id=wide, layer=0)
    assert reply['layer'] == 1 and 'whole layer was copied' in reply.get('note', ''), reply
    app.call('move_layers', document_id=wide, layers=[0, 1], dx=100, dy=0)
    for x in columns:
        copy, kept = codes(app, wide, 1, x, 10), codes(app, wide, 0, x, 10)
        assert kept == before[x], ('the source came back whole', x, kept, before[x])
        assert copy == before[x], ('the copy holds the off-canvas columns', x, copy, before[x])
    seen['off_canvas_columns'] = 100
    return seen


def gradient(app, depth):
    width, height = 32, 16
    full = top(depth)
    base, ink = '#C08040', '#808080'
    document = new_document(app, depth, width, height)
    app.call('fill_selection', document_id=document, layer=0, color=base)
    flat = [v * full // 255 for v in hex_bytes(base)] + [full]
    assert codes(app, document, 0, 5, 5) == flat, (codes(app, document, 0, 5, 5), flat)
    start = cursor(app, document)
    app.call('gradient', document_id=document, layer=0, x0=0, y0=8, x1=width, y1=8,
             start_color=ink, end_color=ink, blend_mode='Multiply')
    assert cursor(app, document) == start + 1
    # Multiply over an opaque backdrop: Cb x Cs per channel, alpha unchanged.
    want = [b * s / 255 * full / 255 for b, s in zip(hex_bytes(base), hex_bytes(ink))] + [full]
    # 1.5 of 255: one rounding in the compositor and one in the store.
    slack = 1.5 * full / 255
    for x, y in ((0, 0), (9, 4), (16, 8), (31, 15)):
        got = codes(app, document, 0, x, y)
        assert near(got, want, slack), (x, y, got, want)

    # Normal by name is the call that names no mode.
    pair = []
    for extra in ({}, {'blend_mode': 'Normal'}):
        other = new_document(app, depth, width, height)
        app.call('gradient', document_id=other, layer=0, x0=2, y0=3, x1=29, y1=12,
                 start_color='#FF000080', end_color='#0000FF', **extra)
        pair.append([codes(app, other, 0, x, y) for y in (0, 7, 15) for x in (0, 11, 20, 31)])
    assert pair[0] == pair[1], 'Normal is the default'
    assert len(set(map(tuple, pair[0]))) > 4, 'the ramp painted'

    # Coverage has nothing to blend with.
    app.call('add_layer_mask', document_id=document, layer=0, kind='reveal_all')
    start = cursor(app, document)
    text = app.refused('gradient', "Gradient's blend mode applies to a layer's pixels",
                       document_id=document, layer=0, target='mask', x0=0, y0=8, x1=width, y1=8,
                       start_color='#000000', end_color='#FFFFFF', blend_mode='Multiply')
    assert cursor(app, document) == start, 'a refused gradient leaves no step'
    # The same call in Normal is the mask gradient it always was.
    app.call('gradient', document_id=document, layer=0, target='mask', x0=0, y0=8, x1=width, y1=8,
             start_color='#000000', end_color='#FFFFFF', blend_mode='Normal')
    assert cursor(app, document) == start + 1
    return dict(multiplied=[round(v) for v in want], refusal=text[:80])


def clone(app, depth):
    width, height = 64, 32
    full = top(depth)
    left, right = '#C08040', '#4080C0'
    document = new_document(app, depth, width, height)
    app.call('select_rect', document_id=document, x=0, y=0, width=32, height=height)
    app.call('fill_selection', document_id=document, layer=0, color=left)
    app.call('select_rect', document_id=document, x=32, y=0, width=32, height=height)
    app.call('fill_selection', document_id=document, layer=0, color=right)
    app.call('deselect', document_id=document)
    untouched = [v * full // 255 for v in hex_bytes(right)] + [full]
    assert codes(app, document, 0, 48, 16) == untouched

    stroke = dict(document_id=document, layer=0, points=[[48, 16]], source_x=16, source_y=16,
                  size=12, opacity=1, hardness=100, sample='current')
    # Refused first, so the picture below is still the two flat halves.
    start = cursor(app, document)
    app.refused('clone_stamp', 'nothing for blend_mode', output='new_layer',
                blend_mode='Multiply', **stroke)
    # Healing has no blend mode at either depth: its result replaces the
    # covered pixels, so there is no source colour to composite through one.
    heal = dict(stroke)
    del heal['opacity']
    app.refused('heal_stroke', 'lend', blend_mode='Multiply', **heal)
    assert cursor(app, document) == start, 'a refused stroke leaves no step'

    app.call('clone_stamp', blend_mode='Multiply', **stroke)
    assert cursor(app, document) == start + 1, 'the clone is one history step'
    # Under the dab's centre the source (the left colour) multiplies into the
    # destination (the right one): Cb x Cs per channel, alpha unchanged.
    want = [a * b / 255 * full / 255 for a, b in zip(hex_bytes(left), hex_bytes(right))] + [full]
    slack = 1.5 * full / 255
    for x, y in ((48, 16), (47, 15), (49, 17)):
        got = codes(app, document, 0, x, y)
        assert near(got, want, slack), (x, y, got, want)
    for x, y in ((60, 4), (36, 28), (48, 2)):
        assert codes(app, document, 0, x, y) == untouched, ('outside the dab', x, y)
    assert codes(app, document, 0, 16, 16)[:3] == [v * full // 255 for v in hex_bytes(left)], (
        'the source is not written')
    return dict(centre=[round(v) for v in want])


# Parameters for the `apply_filter` names that require a value at every depth
# (their refusal without one is about the arguments, not the depth); every
# other name runs on its defaults.
FILTER_PARAMETERS = {
    'posterize': dict(levels=4),
    # A two-entry 1D table: three little-endian f32 per entry.
    'color_lookup': dict(params=dict(kind='1d', size=2, table=base64.b64encode(
        struct.pack('<6f', 0, 0.125, 0.25, 1, 0.875, 0.75)).decode())),
    'replace_color': dict(params=dict(colors=['#C03020'], hue=60)),
}


def filters(app, depth):
    tools = app.client.rpc('tools/list', {})['tools']
    schema = next(t for t in tools if t['name'] == 'apply_filter')['inputSchema']
    names = schema['properties']['filter']['enum']
    assert len(names) >= 40, names
    refused = {}
    for name in names:
        document = new_document(app, depth, 48, 32)
        app.call('gradient', document_id=document, layer=0, x0=0, y0=0, x1=48, y1=32,
                 start_color='#C03020', end_color='#20A0D0')
        result = app.client.rpc('tools/call', {'name': 'apply_filter', 'arguments': dict(
            document_id=document, layer=0, filter=name, **FILTER_PARAMETERS.get(name, {}))})
        if result.get('isError'):
            refused[name] = next(
                (c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')[:160]
        app.call('close_document', document_id=document, discard_changes=True)
    assert not refused, refused
    return dict(filters=len(names))


def null_ramp(app, depth):
    document = new_document(app, depth, 24, 12)
    before = codes(app, document, 0, 3, 3)
    start = cursor(app, document)
    sentence = ('gradient requires start_color, start_rgba or stops' if depth == 16
                else 'gradient requires start_color or stops')
    line = dict(document_id=document, layer=0, x0=0, y0=6, x1=24, y1=6)
    app.refused('gradient', sentence, start_color=None, **line)
    app.refused('gradient', sentence, start_color=None, end_color='#FFFFFF', **line)
    app.refused('gradient', sentence, stops=None, **line)
    assert cursor(app, document) == start, 'a refused gradient leaves no step'
    assert codes(app, document, 0, 3, 3) == before, 'a refused gradient changes nothing'
    # The same line with a real ramp paints.
    app.call('gradient', start_color='#000000', end_color='#FFFFFF', **line)
    assert cursor(app, document) == start + 1
    return dict(sentence=sentence)


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-16bit-'))
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
            run(f'VIA {depth}-bit', lambda: via(app, depth))
            run(f'GRADIENT {depth}-bit', lambda: gradient(app, depth))
            run(f'CLONE {depth}-bit', lambda: clone(app, depth))
            run(f'NULL {depth}-bit', lambda: null_ramp(app, depth))
        run('FILTERS 16-bit', lambda: filters(app, 16))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
