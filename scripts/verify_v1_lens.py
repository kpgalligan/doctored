#!/usr/bin/env python3
"""MCP drive for the v1 lens batch: Filters > Lens Correction's
`apply_filter` mirror end to end, the RAW `lens_profile` report, and the
Liquify point bound after the layer offset.

The script owns its app instances: the Debug app (or `DR_VERIFY_APP`), each on
its own scratch MCP socket, with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, the 2 GiB
working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`), and for the RAW survey a second
instance at an 8 GiB working limit. It terminates only those
processes. Every expected number is computed here, by this file's own oracle,
never read back from the app.

The fixture is a 128x96 opaque linear ramp, R = 40 + x, G = 30 + 2y,
B = 100 + x - y. Catmull-Rom reproduces a linear function exactly wherever all
16 taps are inside, so an interior pixel of a corrected ramp is the ramp at the
pixel's mapped source point (the header's inverse map, written here
independently), within one 8-bit level. At 16 bits the file is promoted, every
code widened by 257.

At 8 and 16 bits:
- IDENTITY: every parameter at its default answers `changed: false` and adds
  no history step.
- SCALE, ROTATE, DISTORT, TCA, PERSPECTIVE: sampled interior pixels are the
  ramp at the mapped point (TCA per channel, with a distortion so the
  sub-level lateral shift of a 128-px plate is not the only change); Scale 70
  leaves the corners transparent.
- VIGNETTE: every sampled pixel is the IEC sRGB light-linear gain of
  2^(0.02 * amount * fraction^(4^((midpoint - 50)/50))) applied to the ramp.
- UNDO: one history step; `undo` restores the ramp.
- REPLAY: the recorded `apply_filter` step replays on a second copy to
  identical samples.
- REFUSALS: `distortion: 101`, a `params` object (the other argument style)
  and a 32-bit float document are refused with no history step.
- LIQUIFY BOUND: with the layer at offset (100, 0), a point at x 1,000,050
  (999,950 in the layer's pixels) is accepted and one at x 1,000,150 is refused
  with "Each liquify stroke needs".

RAW SURVEY: each camera file under ~/devel/doctored-fixtures/raw-pixls/*/ and
camera/camera.NEF that is present is opened alone (8-bit develop) and its
`lens_profile` printed; every file with a profile has it enabled by default,
and for the first such file `raw: {lens_correction: false}` develops
different pixels. Absent fixtures are skipped.

Usage: python3 scripts/verify_v1_lens.py [--output DIR] [--skip-raw]
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

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client, png  # noqa: E402

_INSTANCES = itertools.count()  # one pasteboard name per owned instance

ROOT = Path(__file__).resolve().parents[1]
W, H = 128, 96
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
FIXTURES = Path.home() / 'devel/doctored-fixtures'


class SlowClient(Client):
    """`Client` with a long timeout: a camera file develops in one call."""

    timeout = 600


class App:
    """One owned instance, on its own scratch socket."""

    def __init__(self, folder, limit, client=Client):
        self.folder = Path(folder)
        self.socket = None  # this instance's scratch DR_AGENT_SOCKET, made at __enter__
        self.limit = limit
        self.client_type = client
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
            DR_PASTEBOARD_NAME=f'rz-verify-v1-lens-{os.getpid()}-{next(_INSTANCES)}',
            **preset_environment(self.folder))
        self.log = (self.folder / 'app.log').open('w')
        self.process = subprocess.Popen(
            [str(executable), '-ApplePersistenceIgnoreState', 'YES',
             '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
            env=environment, stdout=self.log, stderr=subprocess.STDOUT)
        self.client = self.client_type(self.socket)
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


# ------------------------------------------------------------- the oracle --


def ramp_at(x, y):
    """The ramp's three channels at continuous pixel index (x, y)."""
    return [40 + x, 30 + 2 * y, 100 + x - y]


def ramp_png(folder, tag):
    pixels = bytearray()
    for y in range(H):
        for x in range(W):
            pixels += bytes([int(v) for v in ramp_at(x, y)] + [255])
    path = Path(folder) / f'ramp-{tag}.png'
    png(path, W, H, bytes(pixels))
    return path

DEFAULTS = dict(distortion=0, chromatic_red_cyan=0, chromatic_blue_yellow=0, vignette_amount=0,
                vignette_midpoint=50, vertical_perspective=0, horizontal_perspective=0, angle=0,
                scale=100)


def source_point(params, x, y, channel):
    """The header's inverse map: output pixel (x, y)'s source point for
    `channel`, pixel centres at half-integers, or None outside the picture."""
    p = dict(DEFAULTS, **params)
    cx, cy, r = W / 2, H / 2, max(W, H) / 2
    s = p['scale'] / 100
    nx, ny = (x + 0.5 - cx) / r / s, (y + 0.5 - cy) / r / s
    a = math.radians(p['angle'])
    nx, ny = nx * math.cos(a) - ny * math.sin(a), nx * math.sin(a) + ny * math.cos(a)
    d = 1 + 0.004 * (p['horizontal_perspective'] * nx + p['vertical_perspective'] * ny)
    if d <= 0:
        return None
    nx, ny = nx / d, ny / d
    k1 = -0.0008 * p['distortion']
    r2 = nx * nx + ny * ny
    if k1 < 0 and r2 >= 1 / (3 * -k1):
        return None
    lateral = {0: 1 + 0.00002 * p['chromatic_red_cyan'],
               2: 1 + 0.00002 * p['chromatic_blue_yellow']}.get(channel, 1)
    f = (1 + k1 * r2) * lateral
    return cx + r * nx * f, cy + r * ny * f


def interior(point):
    return point is not None and 2 <= point[0] <= W - 3 and 2 <= point[1] <= H - 3


def decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def encode(u):
    u = min(1.0, max(0.0, u))
    return 12.92 * u if u <= 0.0031308 else 1.055 * u ** (1 / 2.4) - 0.055


def vignetted(params, x, y):
    p = dict(DEFAULTS, **params)
    cx, cy = W / 2, H / 2
    far = math.hypot(cx - 0.5, cy - 0.5)
    fraction = math.hypot(x + 0.5 - cx, y + 0.5 - cy) / far
    stops = p['vignette_amount'] * 0.02 * fraction ** (4 ** ((p['vignette_midpoint'] - 50) / 50))
    return [encode(decode(v / 255) * 2 ** stops) * 255 for v in ramp_at(x, y)]


# --------------------------------------------------------------- plumbing --


def opened(app, path, depth):
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def levels(app, document, x, y, depth):
    """One layer pixel as 8-bit levels (16-bit codes divided by 257)."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)
    codes = reply['center_rgba_codes']
    return [c / 257 for c in codes] if depth == 16 else list(codes)

def sparse(stride):
    return [(x, y) for y in range(1, H, stride) for x in range(2, W, stride)]


def lens(app, document, **params):
    return app.call('apply_filter', document_id=document, filter='lens_correction', **params)


def geometry_case(app, depth, folder, tag, params, minimum):
    """Apply `params` and hold every sampled interior pixel to the ramp at its
    mapped point, each channel at its own."""
    document = opened(app, ramp_png(folder, f'{tag}-{depth}'), depth)
    start = cursor(app, document)
    reply = lens(app, document, **params)
    assert reply['changed'] is True, reply
    assert cursor(app, document) == start + 1, 'one history step'
    checked, worst = 0, 0.0
    for x, y in sparse(5):
        points = [source_point(params, x, y, c) for c in range(3)]
        if not all(interior(point) for point in points):
            continue
        got = levels(app, document, x, y, depth)
        for c in range(3):
            want = ramp_at(points[c][0] - 0.5, points[c][1] - 0.5)[c]
            worst = max(worst, abs(got[c] - want))
            assert abs(got[c] - want) <= 1.0, (tag, depth, x, y, c, got, want)
        assert round(got[3]) == 255, (tag, x, y, got)
        checked += 1
    assert checked >= minimum, (tag, checked)
    return document, dict(checked=checked, worst_levels=round(worst, 3))


# ----------------------------------------------------------------- cases --


def identity(app, depth, folder):
    document = opened(app, ramp_png(folder, f'identity-{depth}'), depth)
    start = cursor(app, document)
    reply = lens(app, document, **DEFAULTS)
    assert reply['changed'] is False, reply
    assert cursor(app, document) == start, 'the defaults add no step'
    reply = lens(app, document)
    assert reply['changed'] is False and cursor(app, document) == start, reply
    return dict(changed=False)

def scale(app, depth, folder):
    document, detail = geometry_case(app, depth, folder, 'scale', dict(scale=70), 150)
    for x, y in ((0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1)):
        got = levels(app, document, x, y, depth)
        assert got == [0, 0, 0, 0], ('corner', x, y, got)
    return detail


def rotate(app, depth, folder):
    return geometry_case(app, depth, folder, 'rotate', dict(angle=30), 150)[1]


def distort(app, depth, folder):
    return geometry_case(app, depth, folder, 'distort', dict(distortion=60), 200)[1]


def tca(app, depth, folder):
    # +-100 is a 0.2% lateral scale: under a level on a 128-px ramp, so 8 bits
    # alone would report `changed: false`. The distortion makes the edit
    # visible at both depths; each channel is still held to its own point.
    return geometry_case(app, depth, folder, 'tca',
                         dict(chromatic_red_cyan=100, chromatic_blue_yellow=-100, distortion=30),
                         200)[1]


def perspective(app, depth, folder):
    return geometry_case(app, depth, folder, 'perspective',
                         dict(vertical_perspective=-50, horizontal_perspective=40, scale=110),
                         150)[1]


def vignette(app, depth, folder):
    document = opened(app, ramp_png(folder, f'vignette-{depth}'), depth)
    params = dict(vignette_amount=80, vignette_midpoint=30)
    reply = lens(app, document, **params)
    assert reply['changed'] is True, reply
    worst = 0.0
    for x, y in sparse(5):
        got = levels(app, document, x, y, depth)
        want = vignetted(params, x, y)
        for c in range(3):
            worst = max(worst, abs(got[c] - want[c]))
            assert abs(got[c] - want[c]) <= 1.0, (x, y, c, got, want)
        assert round(got[3]) == 255
    corner = levels(app, document, 0, 0, depth)
    assert corner[0] > ramp_at(0, 0)[0] + 10, ('a positive amount brightens the corner', corner)
    return dict(worst_levels=round(worst, 3))


def undo(app, depth, folder):
    document = opened(app, ramp_png(folder, f'undo-{depth}'), depth)
    start = cursor(app, document)
    reply = lens(app, document, distortion=-40, angle=-12, vignette_amount=-50)
    assert reply['changed'] is True, reply
    assert cursor(app, document) == start + 1
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    for x, y in sparse(9):
        got = levels(app, document, x, y, depth)
        assert [round(v) for v in got] == [int(v) for v in ramp_at(x, y)] + [255], (x, y, got)
    return dict(steps=1)


def replay(app, depth, folder):
    document = opened(app, ramp_png(folder, f'replay-{depth}'), depth)
    second = opened(app, ramp_png(folder, f'replay-copy-{depth}'), depth)
    params = dict(distortion=35, chromatic_red_cyan=60, vignette_amount=-40, vignette_midpoint=70,
                  vertical_perspective=20, horizontal_perspective=-15, angle=8, scale=105)
    name = f'v1 lens {depth}'
    app.call('start_recording', discard=True)
    assert lens(app, document, **params)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [step['tool'] for step in action['steps']] == ['apply_filter'], action
    assert action['steps'][0]['arguments']['filter'] == 'lens_correction', action
    points = sparse(7)
    recorded = [levels(app, document, x, y, depth) for x, y in points]
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert [levels(app, second, x, y, depth) for x, y in points] == recorded, 'the replay differs'
    return dict(samples=len(points))


def refusals(app, depth, folder):
    document = opened(app, ramp_png(folder, f'refusals-{depth}'), depth)
    start = cursor(app, document)
    seen = {}
    seen['distortion'] = app.refused('apply_filter', 'distortion must be finite in [-100,100]',
                                     document_id=document, filter='lens_correction',
                                     distortion=101)
    # apply_filter reads an older filter's arguments flat and passes only its
    # own keys on, so a stray key there never reaches the core; a params
    # object, the other argument style, is refused by name.
    seen['unknown key'] = app.refused('apply_filter', 'takes its arguments flat',
                                      document_id=document, filter='lens_correction',
                                      params=dict(radius=3))
    assert cursor(app, document) == start, 'a refusal leaves no step'
    scene = Path(folder) / f'float-{depth}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, scene)
    floating = app.call('open_document', path=str(scene))['document']['id']
    assert app.call('get_precision', document_id=floating)['bits_per_channel'] == 32
    # Lens Correction has run on a float layer since 9a5e044f: it
    # records a step and keeps float storage.
    held = cursor(app, floating)
    reply = app.call('apply_filter', document_id=floating, filter='lens_correction',
                     distortion=20)
    assert reply['changed'] is True and reply['storage'] == 'rgba32_float', reply
    assert cursor(app, floating) != held, 'a float correction records a step'
    seen['float'] = reply['storage']
    return seen


def liquify_bound(app, depth, folder):
    document = opened(app, ramp_png(folder, f'liquify-{depth}'), depth)
    app.call('set_layer_properties', document_id=document, layer=0, offset_x=100, offset_y=0)
    start = cursor(app, document)

    def warp(x):
        return [dict(tool='forward_warp', points=[[x, 48], [x + 5, 48]], size=40,
                     strength=100, density=100)]

    inside = app.call('liquify', document_id=document, strokes=warp(1_000_045))
    assert inside['ok'], inside
    refused = app.refused('liquify', 'Each liquify stroke needs', document_id=document,
                          strokes=warp(1_000_150))
    held = cursor(app, document)
    assert held in (start, start + 1), (start, held)
    return dict(accepted=inside['changed'], refused=refused[:80])


def raw_files():
    found = sorted((FIXTURES / 'raw-pixls').glob('*/*')) if (FIXTURES / 'raw-pixls').is_dir() else []
    files = [path for path in found if path.suffix.lower() in
             {'.cr3', '.cr2', '.nef', '.arw', '.dng', '.raf', '.orf', '.rw2', '.pef', '.srw'}]
    camera = FIXTURES / 'camera/camera.NEF'
    if camera.is_file():
        files.append(camera)
    return files


def raw_survey(folder):
    files = raw_files()
    if not files:
        print('SKIP raw survey: no camera fixtures present', flush=True)
        return dict(skipped=True)
    table = []
    compared = None
    with App(Path(folder) / 'raw-instance', 8 * 1024 ** 3, SlowClient) as app:
        for path in files:
            reply = app.call('open_document', path=str(path), precision='rgba8')
            document = reply['document']['id']
            profile = reply.get('lens_profile')
            assert isinstance(profile, dict), ('no lens_profile', path, reply)
            row = dict(file=path.name, lens=profile['lens'], supported=profile['supported'],
                       enabled=profile['enabled'])
            print(f'RAW {path.name}: lens={profile["lens"]!r} supported={profile["supported"]} '
                  f'enabled={profile["enabled"]}', flush=True)
            if profile['supported']:
                assert profile['enabled'] is True, ('profile corrections default on', row)
            table.append(row)
            if profile['supported'] and compared is None:
                width, height = reply['document']['width'], reply['document']['height']
                points = [(int(width * fx), int(height * fy))
                          for fx in (0.01, 0.1, 0.5, 0.9, 0.99) for fy in (0.01, 0.1, 0.5, 0.9, 0.99)]

                def samples(doc):
                    return [app.call('sample_pixel', document_id=doc, source='layer', layer=0,
                                     x=x, y=y)['center_rgba_codes'] for x, y in points]

                on = samples(document)
                copy = Path(folder) / f'off-{path.name}'
                shutil.copyfile(path, copy)
                off_reply = app.call('open_document', path=str(copy), precision='rgba8',
                                     raw=dict(lens_correction=False))
                off_profile = off_reply.get('lens_profile')
                assert off_profile and off_profile['enabled'] is False, off_reply
                off = off_reply['document']['id']
                differing = sum(1 for a, b in zip(on, samples(off)) if a != b)
                assert differing > 0, ('lens_correction false changed no sampled pixel', path)
                compared = dict(file=path.name, differing=differing, of=len(points))
                app.call('close_document', document_id=off, discard_changes=True)
            app.call('close_document', document_id=document, discard_changes=True)
    return dict(table=table, compared=compared)


# ----------------------------------------------------------------- main --


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instances and the receipt')
    parser.add_argument('--skip-raw', action='store_true', help='skip the RAW survey')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-lens-'))
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
            run(f'scale {depth}-bit', lambda: scale(app, depth, output))
            run(f'rotate {depth}-bit', lambda: rotate(app, depth, output))
            run(f'distort {depth}-bit', lambda: distort(app, depth, output))
            run(f'tca {depth}-bit', lambda: tca(app, depth, output))
            run(f'perspective {depth}-bit', lambda: perspective(app, depth, output))
            run(f'vignette {depth}-bit', lambda: vignette(app, depth, output))
            run(f'undo {depth}-bit', lambda: undo(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))
            run(f'liquify bound {depth}-bit', lambda: liquify_bound(app, depth, output))
    if not arguments.skip_raw:
        run('raw survey', lambda: raw_survey(output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
