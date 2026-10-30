#!/usr/bin/env python3
"""MCP drive for the v1 Presence batch: Filters > Presence's
`apply_filter` mirror (Texture, Clarity, Dehaze) end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that process.
Every fixture is generated here and every expectation is a property of the
fixture, never a number read back from the app.

Fixtures:
- STRIP(P): a 2048x16 opaque grey strip, value 0.5 + 0.04 sin(2 pi (x + 1/2) / P).
  Presence's radii are a share of the longer side: at 2048 they are 1, 6 and 40
  pixels, so P = 12 sits in Texture's band and P = 96 in Clarity's.
- SCENE: 256x256, 8-pixel tiles whose colours each have one channel at 0, below
  a 48-row sky band of the airlight A = (0.90, 0.92, 0.95). HAZED is the scene
  under I = J t + A (1 - t), t = 0.8 in the left half and 0.5 in the right.
- PLATE: a 96x72 opaque plate of gradients, an edge and texture.
At 16 bits each file is promoted, every code widened by 257.

At 8 and 16 bits:
- IDENTITY: no keys, and all three at 0, answer `changed: false` and add no
  history step.
- EACH SLIDER: texture, clarity and dehaze alone (positive and negative) each
  change sampled pixels in one history step, and `undo` restores the samples.
- BANDS: the standard deviation of STRIP(12)'s middle row rises at texture +100
  and falls at -100; STRIP(96)'s rises at clarity +100 and falls at -100.
- DEHAZE: dehaze +100 on HAZED cuts the mean error to SCENE at least 4x, over
  the tile centres below the sky and more than 16 pixels from a zone boundary.
- SELECTION: with a rectangle selected, every sampled pixel outside it is
  exactly as it was and some pixel inside changed.
- REPLAY: the recorded `apply_filter` step replays on a second copy to
  identical samples, in one history step.
- REFUSALS, each with no history step: `texture: 101`; an unknown flat key
  (`"presence" has no argument "radius"`); a `params` object (`takes its
  arguments flat`); and a 32-bit float document (the message names "presence"
  and says 32-bit float).

Usage: python3 scripts/verify_v1_presence.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import itertools
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

_INSTANCES = itertools.count()  # one pasteboard name per owned instance

ROOT = Path(__file__).resolve().parents[1]
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
STRIP_W, STRIP_H = 2048, 16
SCENE, TILE, SKY_ROWS = 256, 8, 48
AIRLIGHT = (0.90, 0.92, 0.95)
PLATE_W, PLATE_H = 96, 72


class App:
    """One owned instance, on its own scratch socket."""

    def __init__(self, folder, limit):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.limit = limit
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
            DR_AUTOMATION_ALERTS='record', DR_WORKING_LIMIT_BYTES=str(self.limit),
            DR_ACTIONS_DIR=str(self.folder / 'actions'),
            DR_RECOVERY_DIR=str(self.folder / 'recovery'),
            DR_PASTEBOARD_NAME=f'rz-verify-v1-presence-{os.getpid()}-{next(_INSTANCES)}',
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


# ------------------------------------------------------------ the fixtures --

def write(folder, name, width, height, colour):
    """A PNG of `colour(x, y)` (three values in 0..1), opaque."""
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            pixels += bytes([round(v * 255) for v in colour(x, y)] + [255])
    path = Path(folder) / f'{name}.png'
    png(path, width, height, bytes(pixels))
    return path


def strip(period):
    def colour(x, _y):
        value = 0.5 + 0.04 * math.sin(2 * math.pi * (x + 0.5) / period)
        return (value, value, value)
    return colour


def scene(x, y):
    """The clean scene below the sky: one channel of each tile is 0."""
    tx, ty = x // TILE, y // TILE
    colour = [0.25 + 0.75 * ((tx * 37 + ty * 59 + k * 101) % 97) / 96 for k in range(3)]
    colour[(tx + 2 * ty) % 3] = 0.0
    return colour


def hazed(x, y):
    if y < SKY_ROWS:
        return AIRLIGHT
    t = 0.8 if x < SCENE // 2 else 0.5
    return [j * t + a * (1 - t) for j, a in zip(scene(x, y), AIRLIGHT)]


def measured(x, y):
    """Below the sky, more than 16 pixels from it and from the zone line."""
    return y > SKY_ROWS + 16 and (x + 17 < SCENE // 2 or x > SCENE // 2 + 16)


def plate(x, y):
    ramp = 30 + x * 2 + y
    edge = 60 if x > PLATE_W // 2 else 0
    texture = (x * 7 + y * 13) % 17
    return [((ramp + edge + texture + offset) % 256) / 255 for offset in (0, 45, 110)]


# --------------------------------------------------------------- plumbing --


def opened(app, path, depth):
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document

def fresh(app, folder, name, depth, width, height, colour):
    """A new document of the fixture; `name` keeps each file its own."""
    return opened(app, write(folder, f'{name}-{depth}', width, height, colour), depth)


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def codes(app, document, x, y):
    """One layer pixel in the document's own codes."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)
    return list(reply['center_rgba_codes'])


def presence(app, document, **params):
    return app.call('apply_filter', document_id=document, filter='presence', **params)


def plate_points():
    return [(x, y) for y in range(2, PLATE_H, 7) for x in range(3, PLATE_W, 7)]


def plate_samples(app, document):
    return [codes(app, document, x, y) for x, y in plate_points()]


# ----------------------------------------------------------------- cases --

def identity(app, depth, folder):
    document = fresh(app, folder, 'identity', depth, PLATE_W, PLATE_H, plate)
    start = cursor(app, document)
    reply = presence(app, document)
    assert reply['changed'] is False, reply
    assert cursor(app, document) == start, 'no keys add no step'
    reply = presence(app, document, texture=0, clarity=0, dehaze=0)
    assert reply['changed'] is False, reply
    assert cursor(app, document) == start, 'all zero adds no step'
    return dict(changed=False)


def each_slider(app, depth, folder):
    detail = {}
    for key in ('texture', 'clarity', 'dehaze'):
        for amount in (70, -70):
            document = fresh(app, folder, f'slider-{key}-{amount}', depth, PLATE_W, PLATE_H, plate)
            before = plate_samples(app, document)
            start = cursor(app, document)
            reply = presence(app, document, **{key: amount})
            assert reply['changed'] is True, (key, amount, reply)
            assert cursor(app, document) == start + 1, (key, amount, 'one history step')
            after = plate_samples(app, document)
            moved = sum(1 for a, b in zip(before, after) if a != b)
            assert moved > 0, (key, amount, 'no sampled pixel changed')
            assert all(a[3] == b[3] for a, b in zip(before, after)), (key, amount, 'alpha moved')
            app.call('undo', document_id=document)
            assert cursor(app, document) == start, (key, amount)
            assert plate_samples(app, document) == before, (key, amount, 'undo did not restore')
            detail[f'{key} {amount:+d}'] = moved
            app.call('close_document', document_id=document, discard_changes=True)
    return detail


def row_deviation(app, document):
    """The standard deviation of the strip's middle row, over every second
    column of its middle quarter."""
    y = STRIP_H // 2
    return statistics.pstdev(codes(app, document, x, y)[0]
                             for x in range(STRIP_W * 3 // 8, STRIP_W * 5 // 8, 2))


def bands(app, depth, folder):
    detail = {}
    for key, period in (('texture', 12), ('clarity', 96)):
        document = fresh(app, folder, f'band-{key}', depth, STRIP_W, STRIP_H, strip(period))
        before = row_deviation(app, document)
        assert before > 0, (key, 'the strip is flat')
        seen = {}
        for amount in (100, -100):
            start = cursor(app, document)
            assert presence(app, document, **{key: amount})['changed'] is True, (key, amount)
            seen[amount] = row_deviation(app, document)
            app.call('undo', document_id=document)
            assert cursor(app, document) == start
        assert seen[100] > before, (key, 'the +100 contrast did not rise', before, seen)
        assert seen[-100] < before, (key, 'the -100 contrast did not fall', before, seen)
        detail[key] = dict(period=period, before=round(before, 3), up=round(seen[100], 3),
                           down=round(seen[-100], 3))
        app.call('close_document', document_id=document, discard_changes=True)
    return detail


def dehaze(app, depth, folder):
    document = fresh(app, folder, 'hazed', depth, SCENE, SCENE, hazed)
    scale = 65535 if depth == 16 else 255
    points = [(x, y) for y in range(TILE // 2, SCENE, TILE) for x in range(TILE // 2, SCENE, TILE)
              if measured(x, y)]
    assert len(points) > 100, len(points)

    def error():
        total = 0.0
        for x, y in points:
            got = codes(app, document, x, y)
            total += sum(abs(got[c] / scale - scene(x, y)[c]) for c in range(3))
        return total / (3 * len(points))

    before = error()
    start = cursor(app, document)
    assert presence(app, document, dehaze=100)['changed'] is True
    assert cursor(app, document) == start + 1
    after = error()
    assert after * 4 <= before, ('dehaze +100 cut the error less than 4x', before, after)
    return dict(points=len(points), before=round(before, 4), after=round(after, 4))


def selection(app, depth, folder):
    document = fresh(app, folder, 'selection', depth, PLATE_W, PLATE_H, plate)
    before = plate_samples(app, document)
    left, top, width, height = 20, 15, 50, 40
    app.call('select_rect', document_id=document, x=left, y=top, width=width, height=height)
    assert presence(app, document, texture=80, clarity=60, dehaze=40)['changed'] is True
    after = plate_samples(app, document)
    inside_moved = 0
    for (x, y), a, b in zip(plate_points(), before, after):
        if left <= x < left + width and top <= y < top + height:
            inside_moved += a != b
        else:
            assert a == b, ('a pixel outside the selection moved', x, y, a, b)
    assert inside_moved > 0, 'no sampled pixel inside the selection changed'
    return dict(inside_moved=inside_moved)


def replay(app, depth, folder):
    document = fresh(app, folder, 'replay', depth, PLATE_W, PLATE_H, plate)
    second = fresh(app, folder, 'replay-copy', depth, PLATE_W, PLATE_H, plate)
    params = dict(texture=45, clarity=-30, dehaze=25)
    name = f'v1 presence {depth}'
    app.call('start_recording', discard=True)
    assert presence(app, document, **params)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['apply_filter'], action
    assert action['steps'][0]['arguments']['filter'] == 'presence', action
    recorded = plate_samples(app, document)
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert plate_samples(app, second) == recorded, 'the replay differs'
    return dict(samples=len(recorded))


def refusals(app, depth, folder):
    document = fresh(app, folder, 'refusals', depth, PLATE_W, PLATE_H, plate)
    start = cursor(app, document)
    seen = {}
    seen['range'] = app.refused('apply_filter', 'texture must be finite in [-100,100]',
                                document_id=document, filter='presence', texture=101)
    seen['unknown key'] = app.refused('apply_filter', '"presence" has no argument "radius"',
                                      document_id=document, filter='presence', radius=3)
    seen['params object'] = app.refused('apply_filter', 'takes its arguments flat',
                                        document_id=document, filter='presence',
                                        params=dict(clarity=20))
    assert cursor(app, document) == start, 'a refusal leaves no step'
    picture = Path(folder) / f'float-{depth}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, picture)
    floating = app.call('open_document', path=str(picture))['document']['id']
    assert app.call('get_precision', document_id=floating)['bits_per_channel'] == 32
    held = cursor(app, floating)
    seen['float'] = app.refused('apply_filter', '"presence"', '32-bit float',
                                document_id=floating, filter='presence', clarity=20)
    assert cursor(app, floating) == held, 'the float refusal leaves no step'
    return seen


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-presence-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'

    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for depth in (8, 16):
            run(f'identity {depth}-bit', lambda: identity(app, depth, output))
            run(f'each slider {depth}-bit', lambda: each_slider(app, depth, output))
            run(f'bands {depth}-bit', lambda: bands(app, depth, output))
            run(f'dehaze {depth}-bit', lambda: dehaze(app, depth, output))
            run(f'selection {depth}-bit', lambda: selection(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
