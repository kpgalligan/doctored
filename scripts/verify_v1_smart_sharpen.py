#!/usr/bin/env python3
"""MCP drive for the v1 Smart Sharpen batch: Filter > Sharpen >
Smart Sharpen's `apply_filter` mirror and the export workspace's Smart Sharpen
output sharpening, end to end.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its own
scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that process.
Every fixture is generated here and every expectation is a property of the
fixture or an equality between two of the app's own results, never a number
read back from the app.

Fixtures:
- PLATE: a 96x72 opaque plate of gradients, an edge and texture.
- STEP: a 160x16 opaque grey strip, a step from 64 to 191 (8-bit levels)
  blurred by a Gaussian of sigma 1.5 px, centred at x = 80.
At 16 bits each file is promoted, every code widened by 257. The 32-bit case
opens `core/tests/fixtures/float_11f/scene.tiff`.

At 8 and 16 bits:
- DEFAULTS: no keys sharpens (never a no-op) in one history step, and `undo`
  restores the samples.
- USM: gaussian, amount 150, radius 1.5, noise 0 on one copy equals
  `unsharp_mask` radius 1.5, amount 1.5, threshold 0 on another, sample for
  sample.
- HALO: on STEP, lens radius 2 amount 200 (noise 0) draws at most a quarter of
  the halo of `unsharp_mask` 2/2/0, H being the sum over the middle row of the
  overshoot past 191 and the undershoot below 64 (in 8-bit levels).
- SELECTION: with a rectangle selected, every sampled pixel outside it is
  exactly as it was and some pixel inside changed.
- REPLAY: the recorded `apply_filter` step replays on a second copy to
  identical samples, in one history step.
- REFUSALS, each with no history step: `amount: 0`, `remove: "box"`, an
  unknown flat key (`"smart_sharpen" has no argument "threshold"`) and a
  `params` object (`takes its arguments flat`).
- EXPORT: `export_image` at size none with the Smart Sharpen method writes the
  same bytes (sha256) as `apply_filter` with the same settings and then an
  unsharpened export; `"method": "unsharp_mask"` writes the method-less
  request's bytes; a bad method is refused.
- PRESETS (once): `save_export_preset` keeps the Smart Sharpen method and its
  settings, `list_export_presets` reads them back, and `delete_export_preset`
  removes it.
At 32 bits: the float document sharpens in one history step (`changed`), and
`undo` restores the samples.

Usage: python3 scripts/verify_v1_smart_sharpen.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import hashlib
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
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
PLATE_W, PLATE_H = 96, 72
STEP_W, STEP_H = 160, 16
SMART = dict(amount=160, radius=1.5, noise=20, remove='lens', shadow_fade=30, highlight_fade=20)


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
            DR_PASTEBOARD_NAME=f'rz-verify-v1-smart-sharpen-{os.getpid()}-{next(_INSTANCES)}',
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


def plate(x, y):
    ramp = 30 + x * 2 + y
    edge = 60 if x > PLATE_W // 2 else 0
    texture = (x * 7 + y * 13) % 17
    return [((ramp + edge + texture + offset) % 256) / 255 for offset in (0, 45, 110)]


def step(x, _y):
    value = (64 + 127 * 0.5 * (1 + math.erf((x + 0.5 - 80) / (1.5 * math.sqrt(2))))) / 255
    return (value, value, value)


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
    """One layer pixel: its codes, or a float layer's samples."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)
    for key in ('center_rgba_codes', 'center_rgba32'):
        if key in reply:
            return list(reply[key])
    raise AssertionError(('no sample in the reply', reply))


def smart(app, document, **params):
    return app.call('apply_filter', document_id=document, filter='smart_sharpen', **params)


def plate_points():
    return [(x, y) for y in range(2, PLATE_H, 7) for x in range(3, PLATE_W, 7)]


def plate_samples(app, document):
    return [codes(app, document, x, y) for x, y in plate_points()]


def close(app, *documents):
    for document in documents:
        app.call('close_document', document_id=document, discard_changes=True)


# ----------------------------------------------------------------- cases --

def defaults(app, depth, folder):
    document = fresh(app, folder, 'defaults', depth, PLATE_W, PLATE_H, plate)
    before = plate_samples(app, document)
    start = cursor(app, document)
    reply = smart(app, document)
    assert reply['changed'] is True, reply
    assert cursor(app, document) == start + 1, 'one history step'
    after = plate_samples(app, document)
    moved = sum(1 for a, b in zip(before, after) if a != b)
    assert moved > 0, 'no sampled pixel changed'
    assert all(a[3] == b[3] for a, b in zip(before, after)), 'alpha moved'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert plate_samples(app, document) == before, 'undo did not restore'
    close(app, document)
    return dict(moved=moved)


def usm(app, depth, folder):
    first = fresh(app, folder, 'usm-smart', depth, PLATE_W, PLATE_H, plate)
    second = fresh(app, folder, 'usm-unsharp', depth, PLATE_W, PLATE_H, plate)
    assert smart(app, first, amount=150, radius=1.5, noise=0, remove='gaussian')['changed']
    assert app.call('apply_filter', document_id=second, filter='unsharp_mask', radius=1.5,
                    amount=1.5, threshold=0)['changed']
    left, right = plate_samples(app, first), plate_samples(app, second)
    assert left == right, 'smart sharpen gaussian differs from unsharp mask'
    close(app, first, second)
    return dict(samples=len(left))


def halo(app, depth, folder):
    scale = 257 if depth == 16 else 1

    def measured(document):
        total = 0.0
        for x in range(STEP_W):
            v = codes(app, document, x, STEP_H // 2)[0] / scale
            total += max(0.0, v - 191) + max(0.0, 64 - v)
        return total

    lens = fresh(app, folder, 'halo-lens', depth, STEP_W, STEP_H, step)
    unsharp = fresh(app, folder, 'halo-usm', depth, STEP_W, STEP_H, step)
    assert smart(app, lens, amount=200, radius=2, noise=0, remove='lens')['changed']
    assert app.call('apply_filter', document_id=unsharp, filter='unsharp_mask', radius=2,
                    amount=2, threshold=0)['changed']
    h_lens, h_usm = measured(lens), measured(unsharp)
    assert h_usm >= 20, ('the step draws no unsharp mask halo', h_usm)
    assert h_lens <= 0.25 * h_usm, ('lens removal halo', h_lens, h_usm)
    close(app, lens, unsharp)
    return dict(h_lens=round(h_lens, 2), h_usm=round(h_usm, 2))


def selection(app, depth, folder):
    document = fresh(app, folder, 'selection', depth, PLATE_W, PLATE_H, plate)
    before = plate_samples(app, document)
    left, top, width, height = 20, 15, 50, 40
    app.call('select_rect', document_id=document, x=left, y=top, width=width, height=height)
    assert smart(app, document, **SMART)['changed'] is True
    after = plate_samples(app, document)
    inside_moved = 0
    for (x, y), a, b in zip(plate_points(), before, after):
        if left <= x < left + width and top <= y < top + height:
            inside_moved += a != b
        else:
            assert a == b, ('a pixel outside the selection moved', x, y, a, b)
    assert inside_moved > 0, 'no sampled pixel inside the selection changed'
    close(app, document)
    return dict(inside_moved=inside_moved)


def replay(app, depth, folder):
    document = fresh(app, folder, 'replay', depth, PLATE_W, PLATE_H, plate)
    second = fresh(app, folder, 'replay-copy', depth, PLATE_W, PLATE_H, plate)
    name = f'v1 smart sharpen {depth}'
    app.call('start_recording', discard=True)
    assert smart(app, document, remove='motion', angle=30, **{
        k: v for k, v in SMART.items() if k != 'remove'})['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [s['tool'] for s in action['steps']] == ['apply_filter'], action
    assert action['steps'][0]['arguments']['filter'] == 'smart_sharpen', action
    recorded = plate_samples(app, document)
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert plate_samples(app, second) == recorded, 'the replay differs'
    close(app, document, second)
    return dict(samples=len(recorded))


def refusals(app, depth, folder):
    document = fresh(app, folder, 'refusals', depth, PLATE_W, PLATE_H, plate)
    start = cursor(app, document)
    seen = {}
    seen['range'] = app.refused('apply_filter', 'amount must be finite in [1,500]',
                                document_id=document, filter='smart_sharpen', amount=0)
    seen['remove'] = app.refused('apply_filter', 'remove must be one of gaussian, lens, motion',
                                 document_id=document, filter='smart_sharpen', remove='box')
    seen['unknown key'] = app.refused('apply_filter', '"smart_sharpen" has no argument "threshold"',
                                      document_id=document, filter='smart_sharpen', threshold=3)
    seen['params object'] = app.refused('apply_filter', 'takes its arguments flat',
                                        document_id=document, filter='smart_sharpen',
                                        params=dict(amount=120))
    assert cursor(app, document) == start, 'a refusal leaves no step'
    close(app, document)
    return seen


def exported(app, document, folder, name, settings):
    path = Path(folder) / f'{name}.png'
    reply = app.call('export_image', document_id=document, path=str(path), settings=settings,
                     overwrite=True)
    assert reply['ok'], reply
    return hashlib.sha256(path.read_bytes()).hexdigest()


def export(app, depth, folder):
    base = dict(format='png', bit_depth=depth)
    sharpen = dict(SMART, method='smart_sharpen')
    document = fresh(app, folder, 'export', depth, PLATE_W, PLATE_H, plate)
    filtered = fresh(app, folder, 'export-filtered', depth, PLATE_W, PLATE_H, plate)
    method = exported(app, document, folder, f'smart-{depth}', dict(base, sharpen=sharpen))
    assert smart(app, filtered, **SMART)['changed'] is True
    applied = exported(app, filtered, folder, f'applied-{depth}', base)
    assert method == applied, 'the smart sharpen export differs from the filter then the export'
    plain = exported(app, document, folder, f'plain-{depth}', base)
    assert plain != method, 'the smart sharpen export did not sharpen'
    usm_settings = dict(radius=1.2, amount=1.4, threshold=0.01)
    unnamed = exported(app, document, folder, f'usm-{depth}', dict(base, sharpen=usm_settings))
    named = exported(app, document, folder, f'usm-named-{depth}',
                     dict(base, sharpen=dict(usm_settings, method='unsharp_mask')))
    assert unnamed == named, 'unsharp_mask by name differs'
    refusal = app.refused('export_image', 'unsharp_mask or smart_sharpen', document_id=document,
                          path=str(Path(folder) / f'bad-{depth}.png'),
                          settings=dict(base, sharpen=dict(method='bilateral')))
    close(app, document, filtered)
    return dict(sha256=method[:16], refusal=refusal[:160])


def presets(app, folder):
    name = 'v1 smart sharpen'
    sharpen = dict(SMART, method='smart_sharpen', angle=12)
    destination = Path(folder) / 'presets-out'
    destination.mkdir(parents=True, exist_ok=True)
    saved = app.call('save_export_preset', name=name, destination=str(destination),
                     pattern='{name}', settings=dict(format='png', sharpen=sharpen), replace=True)
    assert saved['ok'], saved
    listed = [p for p in app.call('list_export_presets')['presets'] if p['name'] == name]
    assert len(listed) == 1, listed
    stored = listed[0]['settings']['sharpen']
    assert stored['method'] == 'smart_sharpen', stored
    for key, value in sharpen.items():
        assert stored[key] == value, (key, value, stored)
    app.call('delete_export_preset', name=name)
    assert not [p for p in app.call('list_export_presets')['presets'] if p['name'] == name]
    return dict(stored=stored)


def float_document(app, folder):
    picture = Path(folder) / 'float.tiff'
    shutil.copyfile(FLOAT_FIXTURE, picture)
    size = app.call('open_document', path=str(picture))['document']
    document = size['id']
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    points = [(x, y) for y in range(2, size['height'], 6) for x in range(2, size['width'], 6)]

    def sampled():
        replies = [app.call('sample_pixel', document_id=document, x=x, y=y) for x, y in points]
        return [list(reply['center_rgba32']) for reply in replies]

    before = sampled()
    start = cursor(app, document)
    assert smart(app, document, **SMART)['changed'] is True
    assert cursor(app, document) == start + 1, 'one history step'
    after = sampled()
    assert all(all(math.isfinite(v) for v in p) for p in after), 'a non-finite sample'
    moved = sum(1 for a, b in zip(before, after) if a != b)
    assert moved > 0, 'no sampled pixel changed'
    app.call('undo', document_id=document)
    assert sampled() == before, 'undo did not restore'
    close(app, document)
    return dict(moved=moved)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-smart-sharpen-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'

    results, failed = {}, []

    def run(name, case):
        try:
            results[name] = dict(passed=True, detail=case())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for depth in (8, 16):
            run(f'defaults {depth}-bit', lambda: defaults(app, depth, output))
            run(f'usm {depth}-bit', lambda: usm(app, depth, output))
            run(f'halo {depth}-bit', lambda: halo(app, depth, output))
            run(f'selection {depth}-bit', lambda: selection(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))
            run(f'export {depth}-bit', lambda: export(app, depth, output))
        run('presets', lambda: presets(app, output))
        run('float 32-bit', lambda: float_document(app, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
