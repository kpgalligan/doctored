#!/usr/bin/env python3
"""MCP drive for Edit > Auto-Blend Layers: `auto_blend_layers`, the
Stack Images method (focus stacking), end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that process.
Every expected number is computed here, never read back from the app, except
where a case says it compares two of the app's own layers.

THE PICTURES. A scene is a grey picture over a rectangle of the plane: the
multi-octave value noise `verify_v1_stack_align.py` registers with (lattice
cells of 64, 32, 16, 8 and 4 pixels, weights 1, 1/2, ... 1/5, contrast 1.8)
and, for the FOCUS scene only, white noise of +-13 levels from an integer hash
of the pixel, which is the fine detail a defocus removes. A frame is the scene
where it is "in focus" and a clamped-edge box mean of the scene elsewhere:
frame A is in focus left of column 320, frame B right of it. The correct answer
is therefore known: A wins the left half, B the right half, and the blend is
the scene.

BOUNDS, fixed before measuring and never widened:
- INTERIOR points are the 8 x 5 grid x in 40, 120, ... 600 and y in 40, 140,
  ... 440; each is at least 40 pixels from column 320.
- share: each of the two frames' `share` is within 0.05 of 0.5 and the two sum
  to 1 (both frames are opaque over the whole canvas).
- colour: at every interior point the merged layer is within 3 levels of the
  scene at 8 bits and within 3 x 257 codes at 16 bits, per channel.
- float: at every interior point the merged layer is within 2% + 0.002 of the
  WINNING layer's own linear sample there (the float stack's conversion of an
  8-bit file is not this drive's subject, so the app's own layer is the
  reference).
- align: the reported homography is within 2.0 px of the true translation over
  the 5 x 5 corner grid (`verify_v1_stack_align.py`'s bound), and each frame's
  `share` is at least 0.3.

CASES
- BLEND at 8 bits, at 16 (`convert_precision`) and at 32-bit float (`paths` =
  [float_11f/scene.tiff, A, B], blending layers 1 and 2): the reply's `layers`
  and shares; `merged_layer` is the index above the top blended layer; the new
  layer is named "Auto-Blend", canvas-sized at (0, 0), unclipped and unmasked;
  both frames have a layer mask; the colour (or float) bound; ONE history
  step; `undo` removes the layer and both masks.
- SEAMLESS FALSE (8 bits): no layer is added and the reply has no
  `merged_layer`; the composite at every interior point is exactly the pixel of
  the frame that should win there, which is what the two masks must show.
- ALIGN (8 bits): B is the align scene's frame B moved by (14, -9), a shift
  Vision follows. `align: true` reports `aligned` with the translation back,
  moves the layer, blends, and leaves one history step.
- ALIGN PAST THE REACH (8 bits): B moved by (592, -140), 92.5% of the width,
  which leaves 5% of the frame shared, under the agreement score's minimum
  share, so no registration is vouched for. The whole call is refused naming the layer, the
  history does not move and no layer has a mask.
- REFUSALS, each with no history step: a layer that already has a mask; one
  layer; a group.
- ACTION: a recording around one call holds one `auto_blend_layers` step, and
  `run_action` replays it on a second stack as one history step.

Fixtures are written once per output folder (`<output>/fixtures`): pure-Python
noise takes about half a minute.

Usage: python3 scripts/verify_v1_auto_blend.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import itertools
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

_INSTANCES = itertools.count()  # one pasteboard name per owned instance

ROOT = Path(__file__).resolve().parents[1]
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
SIZE = (640, 480)
SPLIT = 320
INTERIOR = [(x, y) for y in range(40, 480, 100) for x in range(40, 640, 80)]
SHARE_BOUND = 0.05
COLOUR_LEVELS = 3
FLOAT_RELATIVE, FLOAT_ABSOLUTE = 0.02, 0.002
ALIGN_BOUND_PX = 2.0
ALIGN_SHARE = 0.3
NEAR = (14, -9)
FAR = (592, -140)
FINE_LEVELS = 13


class SlowClient(Client):
    """`Client` with a long timeout: one call registers, warps and blends."""

    timeout = 600


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
            DR_PASTEBOARD_NAME=f'rz-verify-v1-auto-blend-{os.getpid()}-{next(_INSTANCES)}',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = SlowClient(self.socket)
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


# ------------------------------------------------------------ the pictures --

CELLS = (64, 32, 16, 8, 4)
WEIGHTS = tuple(1 / (index + 1) for index in range(len(CELLS)))
WEIGHT_SUM = sum(WEIGHTS)
CONTRAST = 1.8


def lattice(ix, iy, octave):
    """The noise lattice: an integer hash of the cell, in 0...1."""
    h = (ix * 374761393 + iy * 668265263 + (octave + 1) * 1274126177) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    h ^= h >> 16
    return (h & 0xFFFF) / 65535.0


def fine(x, y):
    """White noise in -1...1 from an integer hash of the pixel."""
    h = (x * 2246822519 + y * 3266489917 + 374761393) & 0xFFFFFFFF
    h = ((h ^ (h >> 15)) * 2246822519) & 0xFFFFFFFF
    h ^= h >> 13
    return (h & 0xFFFF) / 32767.5 - 1.0


def scene(rect, with_fine):
    """The scene over `rect` = (x0, y0, x1, y1) as rows of levels (floats in
    0...255): value noise at pixel centres, plus the fine noise when asked."""
    x0, y0, x1, y1 = rect
    octaves = []
    for octave, cell in enumerate(CELLS):
        ox, oy = math.floor(x0 / cell), math.floor(y0 / cell)
        nx, ny = math.floor(x1 / cell) + 2 - ox, math.floor(y1 / cell) + 2 - oy
        table = [[lattice(ox + i, oy + j, octave) for i in range(nx)] for j in range(ny)]
        octaves.append((cell, WEIGHTS[octave], table, ox, oy))
    rows = []
    floor = math.floor
    for y in range(y0, y1):
        sy = y + 0.5
        row = []
        for x in range(x0, x1):
            sx = x + 0.5
            total = 0.0
            for cell, weight, table, ox, oy in octaves:
                fx, fy = sx / cell, sy / cell
                ix, iy = floor(fx), floor(fy)
                tx, ty = fx - ix, fy - iy
                ux, uy = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
                top, below = table[iy - oy], table[iy - oy + 1]
                i = ix - ox
                a = top[i] + (top[i + 1] - top[i]) * ux
                b = below[i] + (below[i + 1] - below[i]) * ux
                total += weight * (a + (b - a) * uy)
            value = ((total / WEIGHT_SUM - 0.5) * CONTRAST + 0.5) * 255
            if with_fine:
                value += FINE_LEVELS * fine(x, y)
            row.append(min(max(value, 0.0), 255.0))
        rows.append(row)
    return rows


def box_mean(rows, radius):
    """A separable box mean of `radius` with clamped edges."""
    height, width = len(rows), len(rows[0])
    span = 2 * radius + 1
    across = []
    for row in rows:
        padded = [row[0]] * radius + row + [row[-1]] * radius
        total = sum(padded[:span])
        out = [total / span]
        for x in range(1, width):
            total += padded[x + span - 1] - padded[x - 1]
            out.append(total / span)
        across.append(out)
    padded = [across[0]] * radius + across + [across[-1]] * radius
    totals = [sum(padded[j][x] for j in range(span)) for x in range(width)]
    down = [[total / span for total in totals]]
    for y in range(1, height):
        entering, leaving = padded[y + span - 1], padded[y - 1]
        totals = [total + entering[x] - leaving[x] for x, total in enumerate(totals)]
        down.append([total / span for total in totals])
    return down


def level(value):
    """The 8-bit level a frame stores for a scene value."""
    return int(value + 0.5)


def frame_levels(sharp, blurred, rect, origin, left):
    """A `SIZE` frame as rows of levels: its pixel (x, y) is the plane point
    (x + origin[0], y + origin[1]); in focus where that point is left of
    column `SPLIT` when `left`, right of it otherwise. `rect` is the planes'."""
    out = []
    for y in range(SIZE[1]):
        py = y + origin[1] - rect[1]
        sharp_row, blurred_row = sharp[py], blurred[py]
        row = []
        for x in range(SIZE[0]):
            px = x + origin[0]
            in_focus = (px < SPLIT) == left
            source = sharp_row if in_focus else blurred_row
            row.append(level(source[px - rect[0]]))
        out.append(row)
    return out


def write_frame(path, rows):
    """An opaque grey PNG of `rows`, written once."""
    if not path.is_file():
        data = bytearray()
        for row in rows:
            for grey in row:
                data += bytes((grey, grey, grey, 255))
        staged = path.with_suffix('.partial')
        png(staged, SIZE[0], SIZE[1], bytes(data))
        staged.rename(path)
    return path


def author(folder):
    """Every fixture, and the levels each frame holds."""
    folder.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    rect = (0, 0) + SIZE
    sharp = scene(rect, True)
    blurred = box_mean(sharp, 3)
    frames = dict(a=frame_levels(sharp, blurred, rect, (0, 0), True),
                  b=frame_levels(sharp, blurred, rect, (0, 0), False))
    # The align scene has no fine noise (it is `verify_v1_stack_align.py`'s
    # picture, whose reach is measured) and a one-pixel blur, so the two
    # frames still show the same edges at the size Vision compares them.
    # Frame B's pixel q shows the plane at q - shift, so its origin is -shift.
    origins = dict(near=(-NEAR[0], -NEAR[1]), far=(-FAR[0], -FAR[1]))
    corners = [(0, 0)] + list(origins.values())
    wide = (min(x for x, _ in corners) - 4, min(y for _, y in corners) - 4,
            max(x for x, _ in corners) + SIZE[0] + 4, max(y for _, y in corners) + SIZE[1] + 4)
    plain = scene(wide, False)
    soft = box_mean(plain, 1)
    frames['align-a'] = frame_levels(plain, soft, wide, (0, 0), True)
    for name, origin in origins.items():
        frames[f'align-b-{name}'] = frame_levels(plain, soft, wide, origin, False)
    paths = {name: write_frame(folder / f'{name}.png', rows) for name, rows in frames.items()}
    target = folder / 'scene.tiff'
    if not target.is_file():
        shutil.copyfile(FLOAT_FIXTURE, target)
    paths['scene'] = target
    print(f'fixtures: {time.monotonic() - started:.1f} s', flush=True)
    return paths, frames, sharp


# --------------------------------------------------------------- plumbing --


def load(app, paths):
    return app.call('load_files_into_stack', paths=[str(path) for path in paths])['document']['id']


def close(app, document):
    app.call('close_document', document_id=document, discard_changes=True)


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']

def rows(app, document):
    return app.call('get_document', document_id=document)['layers']


def rect(row):
    return row['offset_x'], row['offset_y'], row['width'], row['height']


def layer_sample(app, document, layer, x, y, key='center_rgba_codes'):
    return app.call('sample_pixel', document_id=document, source='layer', layer=layer,
                    x=x, y=y)[key]


def check_shares(reply, layers, names):
    listed = reply['layers']
    assert [entry['layer'] for entry in listed] == layers, listed
    assert [entry['name'] for entry in listed] == names, listed
    shares = [entry['share'] for entry in listed]
    assert abs(sum(shares) - 1) <= 1e-9, shares
    for share in shares:
        assert abs(share - 0.5) <= SHARE_BOUND, shares
    return shares


def apply(m, x, y):
    w = m[6] * x + m[7] * y + m[8]
    return (m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w


# ----------------------------------------------------------------- cases --

def blend_case(app, paths, sharp, depth):
    """A below B, blended with the defaults."""
    if depth == 'float':
        document = load(app, [paths['scene'], paths['a'], paths['b']])
        layers = [1, 2]
    else:
        document = load(app, [paths['a'], paths['b']])
        layers = [0, 1]
        if depth == 16:
            app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    names = [Path(paths['a']).stem, Path(paths['b']).stem]
    start = cursor(app, document)
    count = len(rows(app, document))
    reply = app.call('auto_blend_layers', document_id=document, layers=layers)
    assert reply['ok'] is True and reply['changed'] is True, reply
    assert 'aligned' not in reply, reply
    shares = check_shares(reply, layers, names)
    merged = layers[-1] + 1
    assert reply['merged_layer'] == merged, reply
    assert reply['working_memory_plan_bytes'] > SIZE[0] * SIZE[1] * 37, reply
    assert reply['document']['layer_count'] == count + 1, reply['document']
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    assert listed[merged]['name'] == 'Auto-Blend', listed[merged]
    assert rect(listed[merged]) == (0, 0) + SIZE, listed[merged]
    assert listed[merged]['clipped'] is False and listed[merged]['has_mask'] is False
    assert all(listed[layer]['has_mask'] and listed[layer]['mask_enabled'] for layer in layers)
    worst = 0.0
    for x, y in INTERIOR:
        winner = layers[0] if x < SPLIT else layers[1]
        if depth == 'float':
            got = layer_sample(app, document, merged, x, y, 'center_rgba32')
            want = layer_sample(app, document, winner, x, y, 'center_rgba32')
            assert got[3] == 1, got
            for channel in range(3):
                error = abs(got[channel] - want[channel])
                assert error <= FLOAT_RELATIVE * abs(want[channel]) + FLOAT_ABSOLUTE, (
                    x, y, got, want)
                worst = max(worst, error)
        else:
            unit = 257 if depth == 16 else 1
            got = layer_sample(app, document, merged, x, y)
            want = level(sharp[y][x]) * unit
            assert got[3] == 255 * unit, got
            for channel in range(3):
                error = abs(got[channel] - want) / unit
                assert error <= COLOUR_LEVELS, (x, y, got, want)
                worst = max(worst, error)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start, 'undo walks the one step back'
    listed = rows(app, document)
    assert len(listed) == count, 'undo removes the merged layer'
    assert not any(row['has_mask'] for row in listed), 'undo removes the masks'
    close(app, document)
    return dict(shares=shares, worst=round(worst, 4),
                plan_bytes=reply['working_memory_plan_bytes'])


def seamless_false(app, paths, frames):
    document = load(app, [paths['a'], paths['b']])
    start = cursor(app, document)
    reply = app.call('auto_blend_layers', document_id=document, layers=[0, 1], seamless=False)
    assert reply['ok'] is True and reply['changed'] is True, reply
    assert 'merged_layer' not in reply, reply
    shares = check_shares(reply, [0, 1], [Path(paths['a']).stem, Path(paths['b']).stem])
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    assert len(listed) == 2 and all(row['has_mask'] for row in listed), listed
    for x, y in INTERIOR:
        want = frames['a' if x < SPLIT else 'b'][y][x]
        got = app.call('sample_pixel', document_id=document, x=x, y=y)
        assert (got['r'], got['g'], got['b'], got['a']) == (want, want, want, 255), (x, y, got, want)
    close(app, document)
    return dict(shares=shares)


def align_case(app, paths):
    """B moved by `NEAR`: registered, moved back and blended in one call."""
    document = load(app, [paths['align-a'], paths['align-b-near']])
    start = cursor(app, document)
    reply = app.call('auto_blend_layers', document_id=document, layers=[0, 1], align=True)
    assert reply['ok'] is True and reply['changed'] is True, reply
    aligned = reply['aligned']
    assert aligned['reference'] == 0, aligned
    assert [entry['layer'] for entry in aligned['aligned']] == [1], aligned
    assert aligned['unchanged'] == [], aligned
    entry = aligned['aligned'][0]
    assert entry['name'] == Path(paths['align-b-near']).stem, entry
    wanted = [1, 0, -NEAR[0], 0, 1, -NEAR[1], 0, 0, 1]
    worst = 0.0
    for j in range(5):
        for i in range(5):
            x, y = i * SIZE[0] / 4, j * SIZE[1] / 4
            gx, gy = apply(entry['homography'], x, y)
            wx, wy = apply(wanted, x, y)
            worst = max(worst, math.hypot(gx - wx, gy - wy))
    assert worst <= ALIGN_BOUND_PX, f'the translation is {worst:.3f} px off, bound {ALIGN_BOUND_PX}'
    shares = [layer['share'] for layer in reply['layers']]
    assert all(share >= ALIGN_SHARE for share in shares), shares
    assert reply['merged_layer'] == 2, reply
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    moved = rect(listed[1])
    assert abs(moved[0] + NEAR[0]) <= 3 and abs(moved[1] + NEAR[1]) <= 3, moved
    assert rect(listed[0]) == (0, 0) + SIZE, 'the reference did not move'
    assert listed[2]['name'] == 'Auto-Blend' and rect(listed[2]) == (0, 0) + SIZE, listed[2]
    close(app, document)
    return dict(worst_px=round(worst, 3), method=entry['method'], shares=shares)


def past_reach(app, paths):
    """B moved by `FAR`: the layer cannot be registered, so nothing is blended."""
    document = load(app, [paths['align-a'], paths['align-b-far']])
    start = cursor(app, document)
    name = Path(paths['align-b-far']).stem
    text = app.refused('auto_blend_layers', name, 'could not be aligned', 'nothing was blended',
                       document_id=document, layers=[0, 1], align=True)
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    listed = rows(app, document)
    assert len(listed) == 2 and not any(row['has_mask'] for row in listed), listed
    assert rect(listed[1]) == (0, 0) + SIZE, 'the layer did not move'
    close(app, document)
    return dict(refusal=text)


def refusals(app, paths):
    seen = {}

    def refuse(name, document, *needles, **arguments):
        held = cursor(app, document)
        seen[name] = app.refused('auto_blend_layers', *needles, document_id=document, **arguments)
        assert cursor(app, document) == held, f'{name}: a refusal leaves no history step'

    document = load(app, [paths['a'], paths['b']])
    app.call('add_layer_mask', document_id=document, layer=1)
    refuse('a layer with a mask', document, Path(paths['b']).stem, 'already has a layer mask',
           layers=[0, 1])
    close(app, document)

    document = load(app, [paths['a'], paths['b']])
    refuse('one layer', document, 'at least 2', layers=[1])
    app.call('group_layers', document_id=document, layers=[1], name='Holder')
    group = next(row['index'] for row in rows(app, document) if row['name'] == 'Holder')
    refuse('a group', document, 'Holder', 'is a group', layers=[0, group])
    close(app, document)
    return seen


def action_case(app, paths):
    document = load(app, [paths['a'], paths['b']])
    second = load(app, [paths['a'], paths['b']])
    name = 'v1 auto blend'
    app.call('start_recording', discard=True)
    assert app.call('auto_blend_layers', document_id=document, layers=[0, 1])['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['auto_blend_layers'], action
    assert action['steps'][0]['arguments']['layers'] == [0, 1], action
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1, 'the replay is one history step'
    listed = rows(app, second)
    assert len(listed) == 3 and listed[2]['name'] == 'Auto-Blend', listed
    close(app, document)
    close(app, second)
    return dict(steps=[step['tool'] for step in action['steps']])


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-auto-blend-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    paths, frames, sharp = author(output / 'fixtures')

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
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            run(f'blend {label}', lambda: blend_case(app, paths, sharp, depth))
        run('seamless false', lambda: seamless_false(app, paths, frames))
        run('align a shifted frame', lambda: align_case(app, paths))
        run('align past the reach', lambda: past_reach(app, paths))
        run('refusals', lambda: refusals(app, paths))
        run('action', lambda: action_case(app, paths))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
