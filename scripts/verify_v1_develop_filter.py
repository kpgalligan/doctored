#!/usr/bin/env python3
"""MCP drive for the v1 Develop Filter batch: Filters > Develop
Filter's `apply_filter` mirror (`develop_filter`, thirteen flat keys) end to end on
a real JPEG.

The script owns its app instance: the Debug app (or `DR_VERIFY_APP`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s
(`verification_isolation.preset_environment`). It terminates only that process.
Every expectation is a direction stated before the run, measured on the
picture itself, never a number read back from the app and compared with a
stored one.

Fixture: `samples/plasma.jpg` (1024x768), copied per document; at 16 bits each
copy is promoted with `convert_precision`. Statistics are taken on the
384x288 window at (0, 480), the bottom-left corner, where the picture holds
both highlights and shadows (most of it is dark), rendered at native size
(`render` with a `region`), in 0..1, luma Rec. 709.

At 8 and 16 bits:
- IDENTITY: no keys, and all thirteen at 0, answer `changed: false` and add no
  history step.
- EACH CONTROL at + and - (60 units, exposure 1 stop): one history step, `undo`
  restores the render exactly, and the control moves its statistic up for +
  and down for - (the reverse where marked):
  temperature mean R-B; tint mean G-(R+B)/2 (reversed); exposure mean luma;
  contrast luma standard deviation; highlights and whites mean luma of the
  pixels whose source luma is at least 0.6; shadows and blacks the same over
  source luma at most 0.4; vibrance and saturation mean max-min; texture mean
  |luma step| to the right neighbour; clarity the same 16 pixels across;
  dehaze mean min(R,G,B) (reversed).
- PRESENCE: texture, clarity and dehaze inside develop_filter equal `apply_filter`
  presence with the same amounts: the same render and the same sampled codes.
- SELECTION: with a rectangle selected, every sampled pixel outside it is
  exactly as it was and some pixel inside changed.
- REPLAY: the recorded `apply_filter` step replays on a second copy to
  identical samples, in one history step.
- REFUSALS, each with no history step: `temperature: 101`; an unknown flat key
  (`"develop_filter" has no argument "radius"`); and a `params` object (`takes its
  arguments flat`). A 32-bit float document RUNS it: `exposure: 1`
  answers `changed: true` in one history step.
- LEGACY ACTION: an Action file saved before the rename,
  `<instance>/actions/legacy-camera-raw.json` = {"doctored_action": 1, "name":
  "legacy camera raw", "steps": [{"tool": "apply_filter", "arguments":
  {"filter": "camera_raw", "layer": 0, "exposure": 1}}]}, loads: `run_action`
  on a copy is one history step and gives samples identical to `apply_filter
  develop_filter` exposure 1 on a twin, whose fresh recording has
  `arguments.filter == "develop_filter"`.

MASKS (the filter's local adjustments, `masks`; bounds fixed here before the
first run). Sampled points are the grid above; "unchanged" is exact codes.
At 8 and 16 bits:
- MASK RADIAL: a radial mask (centre 0.5, 0.5; radius 0.2 widths and 0.2
  heights; hardness 0.5) with exposure +1 is one history step and `undo`
  restores the render exactly; every point at normalized distance >= 1.02 is
  unchanged, and of the points at distance <= 0.45 at least 90 % are brighter
  (code sum) and none darker.
- MASK RANGE: a full mask limited to luminance 0.6..1 (hard edges) with
  exposure -1: every point whose source CIE L* (sRGB, written here) is below
  55 is unchanged, and at least one point above 65 is darker; none above 65
  is brighter.
- MASK DETAIL: the MASK RADIAL radial with sharpness 80 and
  noise_reduction 40 (no other control) is one history step and `undo`
  restores the render exactly; every point at normalized distance >= 1.02 is
  unchanged and at least half the points at distance <= 0.45 changed. A
  recording of it has one `apply_filter` step whose mask carries sharpness 80
  and noise_reduction 40, and `run_action` on a second copy gives identical
  samples in one history step.
- RANGE NORMALIZED: MASK RANGE's range with both falloffs and invert omitted
  gives exactly the samples of the fully written range on a second copy; and a
  full mask with exposure +1 and `range: null` is accepted and gives exactly
  the samples of the same mask with no range on a third copy.
- MASK REFUSALS, each with no history step: a client `range_profile` ("range_profile
  is made by the app"); a client `map` shape ("map shapes are made by the
  app"); the red plane target ("develop_filter masks edit layer pixels only"); a
  32-bit float document ("develop_filter masks are not available on a 32-bit float
  layer"); and a CMYK document (the CMYK refusal names "CMYK").
With the sky model (`DR_MODELS_DIR` = the read-only fixture store
~/devel/doctored-fixtures/models), at 8 bits on the CC0 sky.jpg:
- MASK SKY: a sky mask with exposure -4 against a full mask with exposure -4,
  both on copies of sky.jpg rendered at max_side 512: a pixel's coverage is
  its luma drop over the full mask's drop, judged where the full drop is at
  least 8/255; the pixels at coverage >= 0.5 have IoU >= 0.90 against
  `scripts/model-drives/sky_reference.json`'s polygon rasterized on the same
  grid (pixel centres) over the judged pixels.
- MASK RECORD: a recording of develop_filter with a sky mask and a range mask has
  one `apply_filter` step whose first mask's shape is exactly {"op": "sky"}
  and which carries no `range_profile`; `run_action` on a second copy gives
  identical samples in one history step.

Usage: python3 scripts/verify_v1_develop_filter.py [--output DIR]
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import base64
import itertools
import json
import os
from pathlib import Path
import shutil
import statistics
import struct
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
import app_under_test  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import Client  # noqa: E402

_INSTANCES = itertools.count()  # one pasteboard name per owned instance

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'samples/plasma.jpg'
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
SKY_FIXTURE = Path.home() / 'devel/doctored-fixtures/phase6/photos/sky.jpg'
MODEL_STORE = Path.home() / 'devel/doctored-fixtures/models'
SKY_REFERENCE = json.loads((ROOT / 'scripts/model-drives/sky_reference.json').read_text())
SKY_IOU = 0.90
WIDTH, HEIGHT = 1024, 768
WINDOW = dict(x=0, y=480, width=384, height=288)
KEYS = ('temperature', 'tint', 'exposure', 'contrast', 'highlights', 'shadows', 'whites',
        'blacks', 'texture', 'clarity', 'dehaze', 'vibrance', 'saturation')


class App:
    """One owned instance, on its own scratch socket."""

    def __init__(self, folder, limit, models=None):
        self.folder = Path(folder)
        self.models = models
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
            DR_PASTEBOARD_NAME=f'rz-verify-v1-develop-filter-{os.getpid()}-{next(_INSTANCES)}',
            **preset_environment(self.folder))
        if self.models is not None:
            environment['DR_MODELS_DIR'] = str(self.models)
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


# ------------------------------------------------------------- the picture --

def read_png(data):
    """An 8-bit RGB or RGBA non-interlaced PNG's bytes as (width, height,
    channels, rows)."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    position, packed, header = 8, b'', None
    while position < len(data):
        length, kind = struct.unpack('>I4s', data[position:position + 8])
        body = data[position + 8:position + 8 + length]
        position += 12 + length
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            packed += body
    width, height, bits, colour, _, _, interlace = header
    assert bits == 8 and colour in (2, 6) and interlace == 0, header
    channels = 3 if colour == 2 else 4
    raw, stride = zlib.decompress(packed), width * channels
    rows, previous, position = [], bytearray(stride), 0
    for _ in range(height):
        kind = raw[position]
        line = bytearray(raw[position + 1:position + 1 + stride])
        position += 1 + stride
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + ((left + up) >> 1)) & 255
            elif kind == 4:
                corner = previous[i - channels] if i >= channels else 0
                estimate = left + up - corner
                a, b, c = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                line[i] = (line[i] + (left if a <= b and a <= c else up if b <= c else corner)) & 255
        rows.append(line)
        previous = line
    return width, height, channels, rows


def render(app, document, region=None):
    """The base64 PNG `render` answers (the window when `region`)."""
    arguments = dict(document_id=document)
    if region is not None:
        arguments['region'] = region
    reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': arguments})
    assert not reply.get('isError'), reply
    return next(item['data'] for item in reply['content'] if item['type'] == 'image')


class Window:
    """The window's pixels as 0..1 RGB, row-major."""

    def __init__(self, data):
        width, height, channels, rows = read_png(base64.b64decode(data))
        assert (width, height) == (WINDOW['width'], WINDOW['height']), (width, height)
        self.width, self.height = width, height
        self.rgb = [tuple(row[x * channels + c] / 255 for c in range(3))
                    for row in rows for x in range(width)]
        self.luma = [0.2126 * r + 0.7152 * g + 0.0722 * b for r, g, b in self.rgb]


def window(app, document):
    return Window(render(app, document, WINDOW))


def mean(values):
    values = list(values)
    assert values, 'an empty statistic'
    return sum(values) / len(values)


def step(picture, shift):
    w = picture.width
    return mean(abs(picture.luma[y * w + x + shift] - picture.luma[y * w + x])
                for y in range(picture.height) for x in range(w - shift))


def statistic(key, picture, source):
    """The statistic `key` moves (module doc), over `picture`; `source` is the
    unfiltered window, whose luma picks the highlight and shadow pixels."""
    if key == 'temperature':
        return mean(r - b for r, _, b in picture.rgb)
    if key == 'tint':
        return mean(g - (r + b) / 2 for r, g, b in picture.rgb)
    if key == 'exposure':
        return mean(picture.luma)
    if key == 'contrast':
        return statistics.pstdev(picture.luma)
    if key in ('highlights', 'whites'):
        return mean(y for y, s in zip(picture.luma, source.luma) if s >= 0.6)
    if key in ('shadows', 'blacks'):
        return mean(y for y, s in zip(picture.luma, source.luma) if s <= 0.4)
    if key in ('vibrance', 'saturation'):
        return mean(max(p) - min(p) for p in picture.rgb)
    if key == 'texture':
        return step(picture, 1)
    if key == 'clarity':
        return step(picture, 16)
    if key == 'dehaze':
        return mean(min(p) for p in picture.rgb)
    raise AssertionError(key)


REVERSED = {'tint', 'dehaze'}


# --------------------------------------------------------------- plumbing --

def opened(app, folder, name, depth):
    path = Path(folder) / f'{name}-{depth}.jpg'
    shutil.copyfile(FIXTURE, path)
    document = app.call('open_document', path=str(path))['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def cursor(app, document):
    return app.call('list_history', document_id=document)['cursor']


def codes(app, document, x, y):
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)
    return list(reply['center_rgba_codes'])


def points():
    return [(x, y) for y in range(5, HEIGHT, 37) for x in range(7, WIDTH, 41)]


def samples(app, document):
    return [codes(app, document, x, y) for x, y in points()]


def develop_filter(app, document, **params):
    return app.call('apply_filter', document_id=document, filter='develop_filter', **params)


# ----------------------------------------------------------------- cases --

def identity(app, depth, folder):
    document = opened(app, folder, 'identity', depth)
    start = cursor(app, document)
    reply = develop_filter(app, document)
    assert reply['changed'] is False, reply
    assert cursor(app, document) == start, 'no keys add no step'
    reply = develop_filter(app, document, **{key: 0 for key in KEYS})
    assert reply['changed'] is False, reply
    assert cursor(app, document) == start, 'all zero adds no step'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(changed=False)


def each_control(app, depth, folder):
    document = opened(app, folder, 'controls', depth)
    base = render(app, document, WINDOW)
    source = Window(base)
    assert sum(1 for s in source.luma if s >= 0.6) > 1000, 'too few highlight pixels'
    assert sum(1 for s in source.luma if s <= 0.4) > 1000, 'too few shadow pixels'
    detail = {}
    for key in KEYS:
        amount = 1.0 if key == 'exposure' else 60
        before = statistic(key, source, source)
        seen = {}
        for sign in (1, -1):
            start = cursor(app, document)
            reply = develop_filter(app, document, **{key: sign * amount})
            assert reply['changed'] is True, (key, sign, reply)
            assert cursor(app, document) == start + 1, (key, sign, 'one history step')
            seen[sign] = statistic(key, window(app, document), source)
            app.call('undo', document_id=document)
            assert cursor(app, document) == start, (key, sign)
            assert render(app, document, WINDOW) == base, (key, sign, 'undo did not restore')
        up, down = (seen[-1], seen[1]) if key in REVERSED else (seen[1], seen[-1])
        assert up > before > down, (key, 'the statistic did not move as stated', before, seen)
        detail[key] = dict(before=round(before, 5), plus=round(seen[1], 5), minus=round(seen[-1], 5))
    app.call('close_document', document_id=document, discard_changes=True)
    return detail


def presence(app, depth, folder):
    ours = opened(app, folder, 'presence-ours', depth)
    theirs = opened(app, folder, 'presence-theirs', depth)
    params = dict(texture=40, clarity=-30, dehaze=25)
    assert develop_filter(app, ours, **params)['changed'] is True
    assert app.call('apply_filter', document_id=theirs, filter='presence', **params)['changed'] is True
    assert render(app, ours) == render(app, theirs), 'the renders differ'
    assert samples(app, ours) == samples(app, theirs), 'the sampled codes differ'
    for document in (ours, theirs):
        app.call('close_document', document_id=document, discard_changes=True)
    return dict(samples=len(points()))


ALL = dict(temperature=25, tint=-15, exposure=0.4, contrast=30, highlights=-40, shadows=35,
           whites=15, blacks=-10, texture=30, clarity=20, dehaze=15, vibrance=25, saturation=-10)


def selection(app, depth, folder):
    document = opened(app, folder, 'selection', depth)
    before = samples(app, document)
    left, top, width, height = 200, 150, 500, 400
    app.call('select_rect', document_id=document, x=left, y=top, width=width, height=height)
    assert develop_filter(app, document, **ALL)['changed'] is True
    after = samples(app, document)
    inside_moved = 0
    for (x, y), a, b in zip(points(), before, after):
        if left <= x < left + width and top <= y < top + height:
            inside_moved += a != b
        else:
            assert a == b, ('a pixel outside the selection moved', x, y, a, b)
    assert inside_moved > 0, 'no sampled pixel inside the selection changed'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(inside_moved=inside_moved)


def replay(app, depth, folder):
    document = opened(app, folder, 'replay', depth)
    second = opened(app, folder, 'replay-copy', depth)
    name = f'v1 develop filter {depth}'
    app.call('start_recording', discard=True)
    assert develop_filter(app, document, **ALL)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [s['tool'] for s in action['steps']] == ['apply_filter'], action
    assert action['steps'][0]['arguments']['filter'] == 'develop_filter', action
    recorded = samples(app, document)
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert samples(app, second) == recorded, 'the replay differs'
    for each in (document, second):
        app.call('close_document', document_id=each, discard_changes=True)
    return dict(samples=len(recorded))


def refusals(app, depth, folder):
    document = opened(app, folder, 'refusals', depth)
    start = cursor(app, document)
    seen = {}
    seen['range'] = app.refused('apply_filter', 'temperature must be finite in [-100,100]',
                                document_id=document, filter='develop_filter', temperature=101)
    seen['unknown key'] = app.refused('apply_filter', '"develop_filter" has no argument "radius"',
                                      document_id=document, filter='develop_filter', radius=3)
    seen['params object'] = app.refused('apply_filter', 'takes its arguments flat',
                                        document_id=document, filter='develop_filter',
                                        params=dict(exposure=1))
    assert cursor(app, document) == start, 'a refusal leaves no step'
    picture = Path(folder) / f'float-{depth}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, picture)
    floating = app.call('open_document', path=str(picture))['document']['id']
    assert app.call('get_precision', document_id=floating)['bits_per_channel'] == 32
    held = cursor(app, floating)
    reply = develop_filter(app, floating, exposure=1)
    assert reply['changed'] is True, reply
    assert cursor(app, floating) == held + 1, 'the float run is one step'
    seen['float'] = reply
    for each in (document, floating):
        app.call('close_document', document_id=each, discard_changes=True)
    return seen


# An Action file as a build before the rename wrote it: the filter op's earlier name
# (`doctored_action` is the file's schema-version key, `Action.fileKey`).
LEGACY_ACTION = {'doctored_action': 1, 'name': 'legacy camera raw',
                 'steps': [{'tool': 'apply_filter',
                            'arguments': {'filter': 'camera_raw', 'layer': 0, 'exposure': 1}}]}


def legacy_action(app, depth, folder):
    actions = app.folder / 'actions'
    actions.mkdir(parents=True, exist_ok=True)
    (actions / 'legacy-camera-raw.json').write_text(json.dumps(LEGACY_ACTION) + '\n')
    played = opened(app, folder, 'legacy', depth)
    twin = opened(app, folder, 'legacy-twin', depth)
    start = cursor(app, played)
    assert app.call('run_action', document_id=played, name=LEGACY_ACTION['name'])['ok']
    assert cursor(app, played) == start + 1, 'the legacy action is one history step'
    name = f'v1 develop filter legacy twin {depth}'
    app.call('start_recording', discard=True)
    assert develop_filter(app, twin, layer=0, exposure=1)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [s['tool'] for s in action['steps']] == ['apply_filter'], action
    assert action['steps'][0]['arguments']['filter'] == 'develop_filter', action
    assert samples(app, played) == samples(app, twin), 'the legacy action differs from develop_filter'
    for each in (played, twin):
        app.call('close_document', document_id=each, discard_changes=True)
    return dict(samples=len(points()), recorded_filter=action['steps'][0]['arguments']['filter'])


# ----------------------------------------------------------------- masks --

def source_lightness(rgba, top):
    """L* of a sampled colour through its Rec. 709 luma's grey (a bound with
    a margin, not the core's Lab)."""
    r, g, b = (c / top for c in rgba[:3])
    lin = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in (r, g, b)]
    y = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]
    f = y ** (1 / 3) if y > 216 / 24389 else (24389 / 27 * y + 16) / 116
    return 116 * f - 16


def mask_radial(app, depth, folder):
    document = opened(app, folder, 'mask-radial', depth)
    base = render(app, document, WINDOW)
    before = samples(app, document)
    start = cursor(app, document)
    masks = [dict(shape=dict(op='radial', center=[0.5, 0.5], radius=[0.2, 0.2], hardness=0.5), exposure=1)]
    assert develop_filter(app, document, masks=masks)['changed'] is True
    assert cursor(app, document) == start + 1, 'one history step'
    after = samples(app, document)
    inside = brighter = 0
    for (x, y), a, b in zip(points(), before, after):
        d = (((x + 0.5) / WIDTH - 0.5) / 0.2) ** 2 + (((y + 0.5) / HEIGHT - 0.5) / 0.2) ** 2
        d = d ** 0.5
        if d >= 1.02:
            assert a == b, ('a point outside the radial moved', x, y, a, b)
        elif d <= 0.45:
            inside += 1
            assert sum(b[:3]) >= sum(a[:3]), ('a point inside darkened', x, y, a, b)
            brighter += sum(b[:3]) > sum(a[:3])
    assert inside > 10 and brighter >= 0.9 * inside, (inside, brighter)
    app.call('undo', document_id=document)
    assert render(app, document, WINDOW) == base, 'undo did not restore'
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(inside=inside, brighter=brighter)


def mask_range(app, depth, folder):
    document = opened(app, folder, 'mask-range', depth)
    top = 65535 if depth == 16 else 255
    before = samples(app, document)
    masks = [dict(shape=dict(op='full'), exposure=-1,
                  range=dict(luminance=dict(low=0.6, high=1, low_falloff=0, high_falloff=0, invert=False)))]
    assert develop_filter(app, document, masks=masks)['changed'] is True
    after = samples(app, document)
    dark = bright = darker = 0
    for (x, y), a, b in zip(points(), before, after):
        l = source_lightness(a, top)
        if l < 55:
            dark += 1
            assert a == b, ('a point below the range moved', x, y, l, a, b)
        elif l > 65:
            bright += 1
            assert sum(b[:3]) <= sum(a[:3]), ('a point in the range brightened', x, y, l, a, b)
            darker += sum(b[:3]) < sum(a[:3])
    assert dark > 10 and darker > 0, (dark, bright, darker)
    app.call('close_document', document_id=document, discard_changes=True)
    return dict(dark=dark, bright=bright, darker=darker)


RADIAL = dict(op='radial', center=[0.5, 0.5], radius=[0.2, 0.2], hardness=0.5)


def radial_distance(x, y):
    return ((((x + 0.5) / WIDTH - 0.5) / 0.2) ** 2 + (((y + 0.5) / HEIGHT - 0.5) / 0.2) ** 2) ** 0.5


def mask_detail(app, depth, folder):
    document = opened(app, folder, 'mask-detail', depth)
    second = opened(app, folder, 'mask-detail-copy', depth)
    base = render(app, document, WINDOW)
    before = samples(app, document)
    start = cursor(app, document)
    masks = [dict(shape=RADIAL, sharpness=80, noise_reduction=40)]
    name = f'v1 develop filter detail {depth}'
    app.call('start_recording', discard=True)
    assert develop_filter(app, document, masks=masks)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert cursor(app, document) == start + 1, 'one history step'
    after = samples(app, document)
    inside = moved = 0
    for (x, y), a, b in zip(points(), before, after):
        d = radial_distance(x, y)
        if d >= 1.02:
            assert a == b, ('a point outside the radial moved', x, y, a, b)
        elif d <= 0.45:
            inside += 1
            moved += a != b
    assert inside > 10 and moved >= 0.5 * inside, (inside, moved)
    assert [s['tool'] for s in action['steps']] == ['apply_filter'], action
    recorded_mask = action['steps'][0]['arguments']['masks'][0]
    assert recorded_mask.get('sharpness') == 80 and recorded_mask.get('noise_reduction') == 40, recorded_mask
    held = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == held + 1
    assert samples(app, second) == after, 'the replay differs'
    app.call('undo', document_id=document)
    assert render(app, document, WINDOW) == base, 'undo did not restore'
    for each in (document, second):
        app.call('close_document', document_id=each, discard_changes=True)
    return dict(inside=inside, moved=moved)


def range_normalized(app, depth, folder):
    written = opened(app, folder, 'range-written', depth)
    omitted = opened(app, folder, 'range-omitted', depth)
    full = dict(low=0.6, high=1, low_falloff=0, high_falloff=0, invert=False)
    assert develop_filter(app, written, masks=[dict(shape=dict(op='full'), exposure=-1,
                                                 range=dict(luminance=full))])['changed'] is True
    assert develop_filter(app, omitted, masks=[dict(shape=dict(op='full'), exposure=-1,
                                                 range=dict(luminance=dict(low=0.6, high=1)))])['changed'] is True
    assert samples(app, omitted) == samples(app, written), 'omitted falloffs and invert differ'
    plain = opened(app, folder, 'range-none', depth)
    null = opened(app, folder, 'range-null', depth)
    assert develop_filter(app, plain, masks=[dict(shape=dict(op='full'), exposure=1)])['changed'] is True
    assert develop_filter(app, null, masks=[dict(shape=dict(op='full'), exposure=1, range=None)])['changed'] is True
    assert samples(app, null) == samples(app, plain), 'range null differs from no range'
    for each in (written, omitted, plain, null):
        app.call('close_document', document_id=each, discard_changes=True)
    return dict(samples=len(points()))


def mask_refusals(app, depth, folder):
    document = opened(app, folder, 'mask-refusals', depth)
    start = cursor(app, document)
    full = [dict(shape=dict(op='full'), exposure=1)]
    seen = {}
    seen['range_profile'] = app.refused('apply_filter', 'range_profile is made by the app',
                                        document_id=document, filter='develop_filter', masks=full,
                                        range_profile='AAAA')
    seen['map'] = app.refused('apply_filter', 'map shapes are made by the app',
                              document_id=document, filter='develop_filter',
                              masks=[dict(shape=dict(op='map', generated='sky', width=1, height=1,
                                                     samples='/w=='), exposure=1)])
    seen['plane'] = app.refused('apply_filter', 'develop_filter masks edit layer pixels only',
                                document_id=document, filter='develop_filter', masks=full, target='red')
    assert cursor(app, document) == start, 'a refusal leaves no step'
    picture = Path(folder) / f'mask-float-{depth}.tiff'
    shutil.copyfile(FLOAT_FIXTURE, picture)
    floating = app.call('open_document', path=str(picture))['document']['id']
    held = cursor(app, floating)
    seen['float'] = app.refused('apply_filter', 'develop_filter masks are not available on a 32-bit float layer',
                                document_id=floating, filter='develop_filter', masks=full)
    assert cursor(app, floating) == held, 'the float refusal leaves no step'
    cmyk = opened(app, folder, 'mask-cmyk', depth)
    app.call('convert_mode', document_id=cmyk, mode='cmyk')
    held = cursor(app, cmyk)
    seen['cmyk'] = app.refused('apply_filter', 'CMYK', document_id=cmyk, filter='develop_filter', masks=full)
    assert cursor(app, cmyk) == held, 'the CMYK refusal leaves no step'
    for each in (document, floating, cmyk):
        app.call('close_document', document_id=each, discard_changes=True)
    return seen


def lumas(app, document):
    """The whole canvas at max_side 512 as Rec. 709 luma in 0..1, and its size."""
    reply = app.client.rpc('tools/call', {'name': 'render',
                                          'arguments': dict(document_id=document, max_side=512)})
    assert not reply.get('isError'), reply
    data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
    width, height, channels, rows = read_png(base64.b64decode(data))
    values = [0.2126 * row[x * channels] / 255 + 0.7152 * row[x * channels + 1] / 255
              + 0.0722 * row[x * channels + 2] / 255 for row in rows for x in range(width)]
    return width, height, values


def inside_polygon(x, y, polygon):
    inside, j = False, len(polygon) - 1
    for i, (xi, yi) in enumerate(polygon):
        xj, yj = polygon[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            inside = not inside
        j = i
    return inside


def sky_copy(app, folder, name):
    path = Path(folder) / f'{name}.jpg'
    shutil.copyfile(SKY_FIXTURE, path)
    return app.call('open_document', path=str(path))['document']['id']


def mask_sky(app, folder):
    source, full, sky = (sky_copy(app, folder, f'mask-sky-{n}') for n in ('source', 'full', 'sky'))
    assert develop_filter(app, full, masks=[dict(shape=dict(op='full'), exposure=-4)])['changed'] is True
    assert develop_filter(app, sky, masks=[dict(shape=dict(op='sky'), exposure=-4)])['changed'] is True
    width, height, original = lumas(app, source)
    _, _, dropped = lumas(app, full)
    _, _, masked = lumas(app, sky)
    scale = SKY_REFERENCE['width'] / width
    polygon = SKY_REFERENCE['polygon']
    both = either = judged = 0
    for i, (o, f, m) in enumerate(zip(original, dropped, masked)):
        drop = o - f
        if drop < 8 / 255:
            continue
        judged += 1
        ours = (o - m) / drop >= 0.5
        x, y = (i % width + 0.5) * scale, (i // width + 0.5) * scale
        theirs = inside_polygon(x, y, polygon)
        both += ours and theirs
        either += ours or theirs
    value = both / either if either else 0.0
    for each in (source, full, sky):
        app.call('close_document', document_id=each, discard_changes=True)
    assert value >= SKY_IOU, ('sky IoU', value, judged)
    return dict(iou=round(value, 4), judged=judged, grid=[width, height])


def mask_record(app, folder):
    document = sky_copy(app, folder, 'mask-record')
    second = sky_copy(app, folder, 'mask-record-copy')
    name = 'v1 develop filter masks'
    masks = [dict(shape=dict(op='sky'), exposure=-0.7, dehaze=30),
             dict(shape=dict(op='full'), shadows=40,
                  range=dict(luminance=dict(low=0, high=0.4, low_falloff=0.1, high_falloff=0.1, invert=False)))]
    app.call('start_recording', discard=True)
    assert develop_filter(app, document, masks=masks)['changed'] is True
    action = app.call('stop_recording', name=name)
    assert [s['tool'] for s in action['steps']] == ['apply_filter'], action
    arguments = action['steps'][0]['arguments']
    assert arguments['masks'][0]['shape'] == {'op': 'sky'}, arguments['masks'][0]['shape']
    assert 'range_profile' not in arguments, sorted(arguments)
    recorded = samples(app, document)
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1
    assert samples(app, second) == recorded, 'the replay differs'
    for each in (document, second):
        app.call('close_document', document_id=each, discard_changes=True)
    return dict(shape=arguments['masks'][0]['shape'], samples=len(recorded))


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-develop-filter-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FIXTURE.is_file(), f'missing fixture {FIXTURE}'
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    assert SKY_FIXTURE.is_file(), f'missing sky fixture {SKY_FIXTURE}'

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
            run(f'identity {depth}-bit', lambda: identity(app, depth, output))
            run(f'each control {depth}-bit', lambda: each_control(app, depth, output))
            run(f'presence {depth}-bit', lambda: presence(app, depth, output))
            run(f'selection {depth}-bit', lambda: selection(app, depth, output))
            run(f'replay {depth}-bit', lambda: replay(app, depth, output))
            run(f'refusals {depth}-bit', lambda: refusals(app, depth, output))
            run(f'legacy action {depth}-bit', lambda: legacy_action(app, depth, output))
            run(f'mask radial {depth}-bit', lambda: mask_radial(app, depth, output))
            run(f'mask range {depth}-bit', lambda: mask_range(app, depth, output))
            run(f'mask detail {depth}-bit', lambda: mask_detail(app, depth, output))
            run(f'range normalized {depth}-bit', lambda: range_normalized(app, depth, output))
            run(f'mask refusals {depth}-bit', lambda: mask_refusals(app, depth, output))
    with App(output / 'instance-models', 2 * 1024 ** 3, models=MODEL_STORE) as app:
        run('mask sky 8-bit', lambda: mask_sky(app, output))
        run('mask record 8-bit', lambda: mask_record(app, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
