#!/usr/bin/env python3
"""MCP drive for the v1 Liquify batch: the `liquify` tool end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB
working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that
process. Every expected number is computed here, by this file's own oracle,
never read back from the app.

The fixture H is a 128x128 opaque picture whose every pixel is a hash of its
position, so a pixel that moved is a pixel that changed, and the source pixel
at any position is known without asking the app. At 16 bits the file is
promoted, every code widened by 257.

At 8 and 16 bits:
- TRANSLATE: a full-strength, full-density `forward_warp` of size 40 from
  (40, 48) to (45, 48) is a whole-pixel shift: every pixel whose centre is
  within 18 px of (45, 48) is the source pixel 5 px to its left, exactly, and
  every pixel 21 px or farther away is its own source pixel (the oracle of
  core/tests/filter_liquify.rs). Then the same with the layer at offset
  (20, 10) and the stroke 20 px right and 10 px down: the points are canvas
  pixels, so the layer's own pixels move as before.
- IDENTITY, RECONSTRUCT: a strength-0 stroke, and a push followed by a
  full-strength `reconstruct` over the whole plane, answer `changed: false`
  and add no history step.
- FREEZE: after a `freeze` disc (size 60 at (30, 48)) a size-200 push across
  the picture leaves every pixel within 26 px of the disc's centre exact and
  changes pixels elsewhere; a `thaw` of the same shape between the two lets
  them change.
- SELECTION: with a rectangle selected, the same push changes pixels inside
  it and leaves every sampled pixel outside it exact.
- UNDO: one history step; `undo` restores the source.
- REPLAY: one recorded `liquify` step replays on a second copy to identical
  samples.
- REFUSALS: `size: true`, an unknown tool, an unknown stroke key, an unknown
  top-level key, empty `strokes`, a group, an adjustment layer and
  `apply_filter filter: "liquify"`, each with no history step.
- DESCRIBED: `liquify` on a text layer changes it, the layer stops being a
  text layer in that one history step, and `undo` restores the text layer.
- LEFTOVER: `refine_selection` refuses a brush whose `radius` is `true` and
  one whose point is `[true, 4]` with "Each brush stroke needs", and accepts
  the same brush with numbers.

Once, on the 32-bit float fixture (a red ramp `x / 47` along row 16):
- FLOAT: a full-strength `forward_warp` of size 16 from (20, 16) to (26, 16)
  replies `changed: true` and `storage: rgba32_float` in one history step;
  the layer's linear sample at (26, 16) is then the ramp's value at (20, 16)
  and the sample at (44, 4), far from the brush, is unchanged; `undo` puts
  (26, 16) back; and `apply_filter filter: "liquify"` still refuses.

`--reference write|check` then regenerates or checks
public-docs/src/content/docs/reference/mcp-tools.md against the same instance.

Usage: python3 scripts/verify_v1_liquify.py [--output DIR] [--reference write|check]
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
SIZE = 128
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'


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
            DR_PASTEBOARD_NAME='rz-verify-v1-liquify',
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


def source(x, y):
    """H's pixel at (x, y) as 8-bit RGBA: a hash of the position, opaque."""
    h = ((x * 73856093) ^ (y * 19349663)) & 0xffffffff
    h = ((h ^ (h >> 13)) * 0x5bd1e995) & 0xffffffff
    h ^= h >> 15
    return [h & 255, (h >> 8) & 255, (h >> 16) & 255, 255]

def hash_png(folder, tag):
    """H as an 8-bit PNG, one file per case (a path already open would answer
    with the document another case edited)."""
    pixels = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            pixels += bytes(source(x, y))
    path = Path(folder) / f'h-{tag}.png'
    png(path, SIZE, SIZE, bytes(pixels))
    return path


def opened(app, path, depth):
    """`path` opened as a document at `depth`: an 8-bit file promoted with
    every code widened by 257 when 16 bits are asked for."""
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def codes(app, document, x, y, layer=0):
    """Four storage codes of one layer at canvas pixel (x, y)."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    return reply['center_rgba_codes']


def stored(pixel, depth):
    """An 8-bit pixel as the document stores it at `depth`."""
    return [c * 257 for c in pixel] if depth == 16 else list(pixel)


def distance(x, y, cx, cy):
    """From pixel (x, y)'s centre to the point (cx, cy)."""
    return math.hypot(x + 0.5 - cx, y + 0.5 - cy)


def stroke(tool, points, size, strength=100, density=100):
    return dict(tool=tool, points=points, size=size, strength=strength, density=density)


def push(dx=0, dy=0):
    """The translation oracle's stroke, moved by (dx, dy)."""
    return stroke('forward_warp', [[40 + dx, 48 + dy], [45 + dx, 48 + dy]], 40)


SWEEP = stroke('forward_warp', [[20, 64], [100, 64]], 200)


def liquify(app, document, depth, strokes, **more):
    """One `liquify` call that is expected to succeed, with its reply's shape checked."""
    reply = app.call('liquify', document_id=document, strokes=strokes, **more)
    assert reply['ok'] and reply['target'] == 'layer' and reply['strokes'] == len(strokes), reply
    if depth == 16:
        assert reply['storage'] == 'rgba16_unorm', reply
    return reply


def sparse(stride):
    return [(x, y) for y in range(0, SIZE, stride) for x in range(0, SIZE, stride)]


# ---------------------------------------------------------------- cases --

def translate(app, depth, folder):
    document = opened(app, hash_png(folder, f'translate-{depth}'), depth)
    seen = {}
    for name, (ox, oy) in (('at origin', (0, 0)), ('at offset', (20, 10))):
        if (ox, oy) != (0, 0):
            app.call('undo', document_id=document)
            app.call('set_layer_properties', document_id=document, layer=0, offset_x=ox,
                     offset_y=oy)
        start = cursor(app, document)
        reply = liquify(app, document, depth, [push(ox, oy)])
        assert reply['changed'] and reply['layer'] == 0, reply
        assert cursor(app, document) == start + 1, 'one history step'
        cx, cy = 45 + ox, 48 + oy
        moved, kept = 0, 0
        # The whole neighbourhood of the brush, and every fourth pixel beyond it.
        for y in range(SIZE):
            for x in range(SIZE):
                d = distance(x, y, cx, cy)
                if d > 26 and (x % 4 or y % 4):
                    continue
                lx, ly = x - ox, y - oy
                if d <= 18:
                    want = stored(source(lx - 5, ly), depth)
                    moved += 1
                elif d >= 21:
                    # Canvas pixels the offset layer does not reach read transparent.
                    inside = 0 <= lx < SIZE and 0 <= ly < SIZE
                    want = stored(source(lx, ly), depth) if inside else [0, 0, 0, 0]
                    kept += 1
                else:
                    continue
                got = codes(app, document, x, y)
                assert got == want, (name, x, y, got, want)
        assert moved > 900 and kept > 1000, (moved, kept)
        seen[name] = dict(moved=moved, kept=kept)
    return seen


def identity(app, depth, folder):
    document = opened(app, hash_png(folder, f'identity-{depth}'), depth)
    start = cursor(app, document)
    still = liquify(app, document, depth,
                    [stroke('forward_warp', [[40, 48], [90, 48]], 40, strength=0)])
    assert still['changed'] is False, still
    assert cursor(app, document) == start, 'a strength-0 stroke adds no step'
    undone = liquify(app, document, depth,
                     [push(), stroke('reconstruct', [[64, 64]], 1000)])
    assert undone['changed'] is False, undone
    assert cursor(app, document) == start, 'a reconstructed warp adds no step'
    for x, y in sparse(16):
        assert codes(app, document, x, y) == stored(source(x, y), depth), (x, y)
    return dict(identity=still['changed'], reconstruct=undone['changed'])


def freeze(app, depth, folder):
    document = opened(app, hash_png(folder, f'freeze-{depth}'), depth)
    disc = [(x, y) for y in range(SIZE) for x in range(SIZE) if distance(x, y, 30, 48) <= 26]
    frozen = stroke('freeze', [[30, 48]], 60)
    start = cursor(app, document)
    reply = liquify(app, document, depth, [frozen, SWEEP])
    assert reply['changed'], reply
    for x, y in disc:
        got = codes(app, document, x, y)
        assert got == stored(source(x, y), depth), ('frozen', x, y, got)
    elsewhere = sum(1 for x, y in sparse(8)
                    if distance(x, y, 30, 48) > 34
                    and codes(app, document, x, y) != stored(source(x, y), depth))
    assert elsewhere > 0, 'the sweep changes pixels outside the frozen disc'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    thawed = stroke('thaw', [[30, 48]], 60)
    reply = liquify(app, document, depth, [frozen, thawed, SWEEP])
    assert reply['changed'], reply
    changed = sum(1 for x, y in disc if codes(app, document, x, y) != stored(source(x, y), depth))
    assert changed > len(disc) // 2, (changed, len(disc))
    return dict(frozen=len(disc), elsewhere=elsewhere, thawed=changed)


def selection(app, depth, folder):
    document = opened(app, hash_png(folder, f'selection-{depth}'), depth)
    left, top, width, height = 30, 30, 30, 40
    app.call('select_rect', document_id=document, x=left, y=top, width=width, height=height)
    start = cursor(app, document)
    reply = liquify(app, document, depth, [SWEEP])
    assert reply['changed'], reply
    if depth == 16:
        assert reply['write_scope'] == 'selection', reply
    assert cursor(app, document) == start + 1

    def inside(x, y):
        return left <= x < left + width and top <= y < top + height

    outside, changed = 0, 0
    for y in range(SIZE):
        for x in range(SIZE):
            near = left - 2 <= x < left + width + 2 and top - 2 <= y < top + height + 2
            if inside(x, y):
                if x % 3 == 0 and y % 3 == 0:
                    changed += codes(app, document, x, y) != stored(source(x, y), depth)
            elif near or (x % 4 == 0 and y % 4 == 0):
                got = codes(app, document, x, y)
                assert got == stored(source(x, y), depth), ('outside', x, y, got)
                outside += 1
    assert changed > 50 and outside > 1000, (changed, outside)
    return dict(outside=outside, changed=changed)


def undo(app, depth, folder):
    document = opened(app, hash_png(folder, f'undo-{depth}'), depth)
    start = cursor(app, document)
    reply = liquify(app, document, depth, [push(), stroke('bloat', [[90, 90], [90, 90]], 50)])
    assert reply['changed'], reply
    assert cursor(app, document) == start + 1, 'two strokes, one history step'
    assert codes(app, document, 45, 48) == stored(source(40, 48), depth)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    for x, y in sparse(4):
        assert codes(app, document, x, y) == stored(source(x, y), depth), (x, y)
    return dict(steps=1)


def replay(app, depth, folder):
    document = opened(app, hash_png(folder, f'replay-{depth}'), depth)
    second = opened(app, hash_png(folder, f'replay-copy-{depth}'), depth)
    points = sparse(4)
    name = f'v1 liquify {depth}'
    strokes = [push(), stroke('pucker', [[90, 90], [90, 90], [92, 91]], 60, strength=80, density=40),
               stroke('twirl_clockwise', [[40, 100], [40, 100]], 50)]
    app.call('start_recording', discard=True)
    reply = liquify(app, document, depth, strokes)
    assert reply['changed'], reply
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['liquify'], action
    assert len(action['steps'][0]['arguments']['strokes']) == len(strokes), action
    recorded = [codes(app, document, x, y) for x, y in points]
    assert any(got != stored(source(x, y), depth) for (x, y), got in zip(points, recorded))
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert [codes(app, second, x, y) for x, y in points] == recorded, 'the replay differs'
    return dict(steps=1, samples=len(points))


def refusals(app, depth, folder):
    document = opened(app, hash_png(folder, f'refusals-{depth}'), depth)
    start = cursor(app, document)
    good = push()
    each = 'Each liquify stroke needs'
    seen = {}
    seen['size'] = app.refused('liquify', each, document_id=document,
                               strokes=[dict(good, size=True)])
    seen['tool'] = app.refused('liquify', each, document_id=document,
                               strokes=[dict(good, tool='smear')])
    seen['stroke key'] = app.refused('liquify', each, 'Unknown stroke key: wobble.',
                                     document_id=document, strokes=[dict(good, wobble=1)])
    seen['top-level key'] = app.refused('liquify', 'Unknown liquify parameter: radius.',
                                        document_id=document, strokes=[good], radius=4)
    seen['empty'] = app.refused('liquify',
                                'liquify needs strokes: an array of 1 to 2048 strokes.',
                                document_id=document, strokes=[])
    seen['apply_filter'] = app.refused('apply_filter', document_id=document, filter='liquify')
    assert cursor(app, document) == start, 'a refusal leaves no step'
    # An adjustment layer above the picture, then both in a group.
    # At 16 bits the depth gate answers for both before the handler runs.
    typed = 'liquify requires a raster layer in 16-bit mode.'
    app.call('add_adjustment_layer', document_id=document, op='invert', layer=0)
    seen['adjustment'] = app.refused('liquify', typed if depth == 16 else 'djustment layer',
                                     document_id=document, strokes=[good], layer=1)
    app.call('group_layers', document_id=document, layers=[0, 1])
    rows = app.call('get_document', document_id=document)['layers']
    groups = [index for index, row in enumerate(rows) if row['kind'] == 'group']
    assert len(groups) == 1 and len(rows) == 3, rows
    seen['group'] = app.refused('liquify', typed if depth == 16 else 'is a group',
                                document_id=document, strokes=[good], layer=groups[0])
    before = cursor(app, document)
    assert before == start + 2, (start, before)
    # The picture inside the group (its bottom child) still takes the edit.
    child = min(rows[groups[0]]['children'])
    reply = liquify(app, document, depth, [good], layer=child)
    assert reply['changed'] and reply['layer'] == child, reply
    assert codes(app, document, 45, 48, layer=child) == stored(source(40, 48), depth)
    return seen


def float_layer(app, folder):
    """A 32-bit float layer takes the strokes in linear light."""
    scene = Path(folder) / 'float.tiff'
    shutil.copyfile(FLOAT_FIXTURE, scene)
    floating = app.call('open_document', path=str(scene))['document']['id']
    assert app.call('get_precision', document_id=floating)['bits_per_channel'] == 32

    def linear(x, y):
        reply = app.call('sample_pixel', document_id=floating, source='layer', layer=0, x=x, y=y)
        return reply['center_rgba32']

    def close(got, want):
        return all(abs(g - w) <= 1e-5 * max(1.0, abs(w)) for g, w in zip(got, want))

    start = cursor(app, floating)
    source_pixel, target, far = linear(20, 16), linear(26, 16), linear(44, 4)
    assert close(source_pixel[:1], [20 / 47]) and close(target[:1], [26 / 47]), (
        source_pixel, target)
    reply = app.call('liquify', document_id=floating,
                     strokes=[stroke('forward_warp', [[20, 16], [26, 16]], 16)])
    assert reply['ok'] and reply['changed'] is True, reply
    assert reply['storage'] == 'rgba32_float' and reply['target'] == 'layer', reply
    assert cursor(app, floating) == start + 1, 'one history step'
    moved = linear(26, 16)
    assert close(moved, source_pixel), (moved, source_pixel)
    assert linear(44, 4) == far, 'a pixel far from the brush is unchanged'
    app.call('undo', document_id=floating)
    assert cursor(app, floating) == start
    assert linear(26, 16) == target, 'undo restores the sample'
    refusal = app.refused('apply_filter', 'is unavailable in a 32-bit float document',
                          document_id=floating, filter='liquify')
    assert cursor(app, floating) == start
    return dict(reply=reply, moved=moved, refusal=refusal)


def described(app, depth, folder):
    """A re-editable text layer takes the edit, drops its description in the
    same step, and gets it back on undo. The text is added at 8 bits and the
    document promoted afterwards, so both depths start from the same layer."""
    document = opened(app, hash_png(folder, f'described-{depth}'), 8)
    app.call('add_text_layer', document_id=document, text='Liquify', x=12, y=40, size=30)
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')

    def text_rows():
        rows = app.call('get_document', document_id=document)['layers']
        return [index for index, row in enumerate(rows) if 'text' in row]

    texts = text_rows()
    assert len(texts) == 1, texts
    layer = texts[0]
    start = cursor(app, document)
    reply = liquify(app, document, depth, [stroke('forward_warp', [[30, 54], [70, 58]], 60)],
                    layer=layer)
    assert reply['changed'] and reply['layer'] == layer, reply
    if depth == 8:
        assert reply.get('rasterized_text') is True, reply
    assert cursor(app, document) == start + 1, 'the edit and the dropped description are one step'
    assert text_rows() == [], 'the warped layer still claims to be text'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert text_rows() == [layer], 'undo restores the text layer'
    return dict(layer=layer, rasterized=True)


def leftover(app, depth, folder):
    document = opened(app, hash_png(folder, f'leftover-{depth}'), depth)
    app.call('select_rect', document_id=document, x=20, y=20, width=60, height=60)
    start = cursor(app, document)
    each = 'Each brush stroke needs'
    brush = dict(mode='add', radius=6, points=[[80, 50], [80, 54]])
    app.refused('refine_selection', each, document_id=document, brush=[dict(brush, radius=True)])
    app.refused('refine_selection', each, document_id=document,
                brush=[dict(brush, points=[[True, 4]])])
    assert cursor(app, document) == start, 'a refusal leaves no step'
    reply = app.call('refine_selection', document_id=document, brush=[brush])
    assert reply['ok'], reply
    return dict(accepted=reply['changed'])


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
    parser.add_argument('--reference', choices=['write', 'check'],
                        help='regenerate or check the MCP reference page against this instance')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-liquify-'))
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

    with App(output) as app:
        for depth in (8, 16):
            run(f'translate {depth}-bit', lambda: translate(app, depth, output))
            run(f'identity and reconstruct {depth}-bit', lambda: identity(app, depth, output))
            run(f'freeze {depth}-bit', lambda: freeze(app, depth, output))
            run(f'selection {depth}-bit', lambda: selection(app, depth, output))
            run(f'undo {depth}-bit', lambda: undo(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))
            run(f'described {depth}-bit', lambda: described(app, depth, output))
            run(f'leftover {depth}-bit', lambda: leftover(app, depth, output))
        run('float layer', lambda: float_layer(app, output))
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
