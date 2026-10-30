#!/usr/bin/env python3
"""The Phase 12F-1b3 MCP drive: ARBITRARY CANVAS ROTATION at 8 bits.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. ONE instance is owned by this file for every gate (`HostApp`: a
free port, a dummy API key and a PRIVATE directory for every preference
library the app reads) and is terminated by its own context manager; no
Doctored process this file did not start is ever signalled.

The reference is this file's OWN f64 inverse-mapping resampler — pixel-centre
convention, transparent out-of-bounds taps, PREMULTIPLIED accumulation,
normalized weights — together with this file's OWN restatement of the
expanded-canvas box. Nothing below derives an expectation from Doctored.

TOLERANCES, fixed here BEFORE any measurement and never widened (8-bit codes,
maximum 255):

- ALPHA within 1 code everywhere;
- straight RGB within 1 code wherever the reference's own alpha is at least
  1 code;
- stored RGBA exactly 0 wherever the result's own alpha rounds to 0 — a
  transparent sample carries no colour to compare, and that clause is what
  proves the LOUD magenta stored at alpha 0 never bleeds;
- masks and channels within 1 code;
- the EXACT cases (quarter turns, the no-ops, the replays) sample for sample;
- every filtered case carries a NON-VACUITY control: it must differ from the
  UNROTATED picture in the same canvas by at least 32 codes somewhere, so a
  silently unrotated answer cannot pass.

The gates:

- **r1** an 8-bit `rotate {degrees: 30}` with `bilinear` and with `bicubic` on
  the 48 x 36 picture: the canvas grows to 60 x 56, the reply's `canvas` and
  `origin` are (60, 56) and (6, 10), the render matches this file's own
  reference under the tolerances above, and the control holds.
- **r2** the extents over MCP: (48x36, 7.5) -> 53x42, (100x10, 80) -> 28x101
  (an axis that SHRINKS), (48x36, 89.999) -> 37x49.
- **r3** the guides and the ruler origin equal this file's own shift/drop/
  clamp at the reported origin, the PPI is UNCHANGED (only an exact quarter
  turn transposes it), the selection is cleared, and the saved channel
  matches this file's own scalar reference with a control.
- **r4** the EXACT route and the SIGN: 90 / 180 / 270 / -90 / 450 / -270
  render as this file's own permutation of the pre-op render with the guides
  permuted; `89.999` with `nearest` carries a window equal to the 90-degree
  render while `-89.999` differs by at least 32 codes.
- **r5** the no-ops: `degrees` 0 / 360 / -720 and a full-canvas `crop` answer
  `ok: true`, `changed: false` with a note, an unchanged history cursor and
  an unchanged render, at 8 bits, 16 bits AND 32-bit float. At each depth a
  recording of horizontal flip / vertical flip / 180 / identity retains zero
  steps, while a no-op followed by a real edit retains and replays only the
  edit against the original canvas.
- **r6** `degrees: "abc"` refuses by name at all three depths with 0 history
  steps; the control `degrees: "30"` equals the numeric call.
- **r7** 16 bits executes filtered rotation with native saved-output and
  chrome/channel evidence; float executes it as ONE history step
  on the expanded canvas, and the float operator table admits
  `geometry_rotate_canvas`; a placed image is an ordinary raster layer
  (`source: null`) that rotates as one step with an exact undo; a text layer is
  reported in `rasterized_layers` by a filtered rotation and stays editable
  through a quarter turn.
- **r8** one `undo` and one `redo`; `.rz` save -> reopen -> resave SHA-256
  identical; a recorded step carries `degrees` as a JSON NUMBER and a
  `sampler`, and replays sample-for-sample; and an action of
  `[rotate 30, select_rect {geometry: relative}]` selects exactly that
  rectangle in the EXPANDED space on a same-size twin.

EXIT CODES: 0 = every gate PASS; 1 = a failed assertion; 2 = INCOMPLETE (any
gate NOT IMPLEMENTED). Exit 2 is never reported as a pass.
"""
import argparse
import base64
import hashlib
import json
import math
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import png_rgba, write_png8, write_png16
from support import HostApp
from verify_phase12f1_geometry import (
    CANVAS, FLOAT_FIXTURE, GUIDES, RULER, canvas_of, chrome,
    control_distance, history, offsets, pad_crop, padded_guides, padded_origin,
    place_fixture, refuses, render, seeded, straighten_picture,
)

# ---- the expectations, fixed BEFORE any measurement ----------------------
# The FILTERED angle every pixel comparison is measured at, and its samplers.
ANGLE = 30.0
SAMPLERS = ('bilinear', 'bicubic')
# The expanded canvas and origin this file's own box says 48 x 36 at 30
# degrees produces. Written out so the gate compares a NUMBER, not a formula.
R1_CANVAS = (60, 56)
R1_ORIGIN = (6, 10)

# r2's extent table: (canvas, degrees) -> the new canvas.
R2_CASES = [
    ((48, 36), 7.5, (53, 42)),
    ((100, 10), 80.0, (28, 101)),
    ((48, 36), 89.999, (37, 49)),
]

# The tolerances. A miss is recorded with its numbers and the gate FAILS.
CODE_TOLERANCE = 1.0
CONTROL_CODES = 32
MAX_CODE = 255

# The sentences, written here rather than read from the app.
UNREADABLE = 'degrees must be a finite number'
# The filtered rotation is admitted at 32-bit float.
FLOAT_FAMILY = 'geometry_rotate_canvas'


# ------------------------------------------- this file's OWN rotation plan --

def normalize(degrees):
    """Into (-180, 180], with -0 folded onto 0."""
    value = math.fmod(float(degrees), 360.0)
    if value <= -180.0:
        value += 360.0
    if value > 180.0:
        value -= 360.0
    return 0.0 if value == 0 else value


def snap(value):
    rounded = round(value)
    return float(rounded) if abs(value - rounded) <= 1e-9 else value


def rotation_plan(width, height, degrees):
    """The expanded canvas box and the canvas->canvas matrix, written out
    here from the definition: rotate the four canvas corners
    about (W/2, H/2), snap each onto an integer it sits within 1e-9 of, take
    the extent (snapped once more) and round it UP -- ceil(bw) x
    ceil(bh) -- and place the old canvas's top-left at
    (floor((W'-W)/2), floor((H'-H)/2))."""
    theta = math.radians(normalize(degrees))
    cos, sin = math.cos(theta), math.sin(theta)
    cx, cy = width / 2.0, height / 2.0
    tx = cx - cos * cx + sin * cy
    ty = cy - sin * cx - cos * cy
    def at(x, y):
        return (cos * x - sin * y + tx, sin * x + cos * y + ty)
    corners = [at(0, 0), at(width, 0), at(0, height), at(width, height)]
    xs = [snap(c[0]) for c in corners]
    ys = [snap(c[1]) for c in corners]
    new_width = math.ceil(snap(max(xs) - min(xs)))
    new_height = math.ceil(snap(max(ys) - min(ys)))
    left, top = -((new_width - width) // 2), -((new_height - height) // 2)
    matrix = [cos, sin, -sin, cos, tx - left, ty - top]
    return {'matrix': matrix, 'left': int(left), 'top': int(top),
            'width': int(new_width), 'height': int(new_height),
            'origin': (int(-left), int(-top))}


def invert(m):
    det = m[0] * m[3] - m[1] * m[2]
    return [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det,
            (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det]


# -------------------------- this file's OWN f64 inverse-mapping reference --

def kernel_weight(kind, t):
    x = abs(t)
    if kind == 'bilinear':
        return max(1.0 - x, 0.0)
    if x < 1.0:
        return ((1.5 * x - 2.5) * x) * x + 1.0
    if x < 2.0:
        return ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    return 0.0


def taps(kind, s):
    """Normalized tap weights for the continuous source coordinate `s`, where
    an integer is a pixel CENTRE at s = k + 0.5; returns the first tap."""
    radius = 1 if kind == 'bilinear' else 2
    count = 2 * radius
    p = s - 0.5
    base = math.floor(p)
    lead = radius - 1
    weights = [kernel_weight(kind, p - (base + t - lead)) for t in range(count)]
    total = sum(weights)
    if total not in (0.0, 1.0):
        weights = [w / total for w in weights]
    return int(base) - lead, weights


def quantize(value):
    return int(round(min(max(value, 0.0), float(MAX_CODE))))


def source_at(inverse, dest, i, j):
    cx, cy = dest[0] + i + 0.5, dest[1] + j + 0.5
    return (inverse[0] * cx + inverse[2] * cy + inverse[4],
            inverse[1] * cx + inverse[3] * cy + inverse[5])


def reference_rgba(source, dims, dest, inverse, kind):
    """Premultiplied f64 accumulation over normalized taps, transparent
    outside the source, unpremultiplied by the RAW alpha sum on the way out."""
    sw, sh = dims
    out = [0] * (dest[2] * dest[3] * 4)
    for j in range(dest[3]):
        for i in range(dest[2]):
            u, v = source_at(inverse, dest, i, j)
            di = (j * dest[2] + i) * 4
            if kind == 'nearest':
                x, y = math.floor(u), math.floor(v)
                if 0 <= x < sw and 0 <= y < sh:
                    si = (y * sw + x) * 4
                    out[di:di + 4] = source[si:si + 4]
                continue
            x0, wx = taps(kind, u)
            y0, wy = taps(kind, v)
            acc = [0.0, 0.0, 0.0, 0.0]
            for ty, wyt in enumerate(wy):
                sy = y0 + ty
                if wyt == 0.0 or sy < 0 or sy >= sh:
                    continue
                for tx, wxt in enumerate(wx):
                    sx = x0 + tx
                    if wxt == 0.0 or sx < 0 or sx >= sw:
                        continue
                    si = (sy * sw + sx) * 4
                    weight = wyt * wxt
                    alpha = float(source[si + 3])
                    pm = weight * alpha / float(MAX_CODE)
                    for c in range(3):
                        acc[c] += pm * float(source[si + c])
                    acc[3] += weight * alpha
            alpha = quantize(acc[3])
            if alpha == 0:
                continue
            scale = float(MAX_CODE) / acc[3]
            out[di:di + 4] = [quantize(acc[0] * scale), quantize(acc[1] * scale),
                              quantize(acc[2] * scale), alpha]
    return out


def reference_plane(source, dims, dest, inverse, kind, outside=0):
    sw, sh = dims
    out = [outside] * (dest[2] * dest[3])
    for j in range(dest[3]):
        for i in range(dest[2]):
            u, v = source_at(inverse, dest, i, j)
            di = j * dest[2] + i
            if kind == 'nearest':
                x, y = math.floor(u), math.floor(v)
                if 0 <= x < sw and 0 <= y < sh:
                    out[di] = source[y * sw + x]
                continue
            x0, wx = taps(kind, u)
            y0, wy = taps(kind, v)
            acc = 0.0
            for ty, wyt in enumerate(wy):
                sy = y0 + ty
                if wyt == 0.0 or sy < 0 or sy >= sh:
                    continue
                for tx, wxt in enumerate(wx):
                    sx = x0 + tx
                    if wxt == 0.0 or sx < 0 or sx >= sw:
                        continue
                    acc += wyt * wxt * (float(source[sy * sw + sx]) - float(outside))
            out[di] = quantize(acc + float(outside))
    return out


def compare_rgba(got, want, what):
    """The three tolerance clauses, returning the worst numbers measured so
    the receipt carries them rather than a boolean."""
    assert len(got) == len(want), (what, len(got), len(want))
    worst_alpha, worst_rgb = 0, 0
    for index in range(0, len(got), 4):
        delta = abs(got[index + 3] - want[index + 3])
        worst_alpha = max(worst_alpha, delta)
        assert delta <= CODE_TOLERANCE, \
            f'{what}: alpha at {index // 4} is {got[index + 3]} against {want[index + 3]}'
        if got[index + 3] == 0:
            assert got[index:index + 4] == [0, 0, 0, 0], \
                f'{what}: sample {index // 4} rounds to transparent and must store zeros'
            continue
        if want[index + 3] >= 1:
            for c in range(3):
                delta = abs(got[index + c] - want[index + c])
                worst_rgb = max(worst_rgb, delta)
                assert delta <= CODE_TOLERANCE, \
                    (f'{what}: channel {c} at {index // 4} is {got[index + c]} '
                     f'against {want[index + c]}')
    return worst_alpha, worst_rgb


def compare_plane(got, want, what):
    assert len(got) == len(want), (what, len(got), len(want))
    worst = 0
    for index, (g, w) in enumerate(zip(got, want)):
        worst = max(worst, abs(g - w))
        assert abs(g - w) <= CODE_TOLERANCE, \
            f'{what}: sample {index} is {g} against {w}'
    return worst


# -------------------------------------------------------------- utilities --

def image_of(width, height, samples, depth=8):
    """A render-shaped dict, so `control_distance` can measure against a
    picture this file built rather than one the app rendered."""
    return {'width': width, 'height': height, 'depth': depth, 'samples': samples}


def rotate_fixture(app, folder, tag, *, depth=8, canvas=CANVAS,
                   guides=False, channel=False):
    """The 48 x 36 picture (or a canvas of another size filled the same way)
    at `depth`, opened through `open_document`, whose default `precision` is
    "preserve"."""
    if canvas == CANVAS:
        picture = straighten_picture()
    else:
        picture = bytearray()
        for y in range(canvas[1]):
            for x in range(canvas[0]):
                picture += bytes(((x * 5) % 256, (y * 7) % 256,
                                  (x * 3 + y * 11) % 256, 255))
        picture = bytes(picture)
    path = folder / f'{tag}.png'
    if depth == 16:
        write_png16(path, canvas[0], canvas[1], [v * 257 for v in picture])
    else:
        write_png8(path, canvas[0], canvas[1], picture)
    document = app.call('open_document', path=str(path))['document']['id']
    if guides:
        for orientation, position in GUIDES:
            app.call('add_guide', document_id=document, orientation=orientation,
                     position=position)
        app.call('set_ruler_origin', document_id=document, x=RULER[0], y=RULER[1])
    if channel:
        app.call('select_rect', document_id=document, x=3, y=2, width=12, height=9)
        app.call('save_selection', document_id=document, name='Cover')
        app.call('deselect', document_id=document)
    return document


def resolution_of(app, document):
    summary = app.call('get_document', document_id=document)
    ppi = summary.get('resolution') or {}
    return (round(float(ppi.get('ppi_x', 0)), 4), round(float(ppi.get('ppi_y', 0)), 4))


def channel_of(app, document, name, width, height):
    app.call('load_selection', document_id=document, source='channel', channel=name)
    points = [{'x': i, 'y': j} for j in range(height) for i in range(width)]
    reply = app.call('get_selection', document_id=document, points=points)
    samples = reply.get('samples') or []
    assert len(samples) == len(points), (document, len(samples), len(points))
    return [int(round(float(s['coverage']))) for s in samples]


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


# ------------------------------------------------------------------ gates --

def gate_r1(app, folder, receipts):
    findings = {}
    plan = rotation_plan(CANVAS[0], CANVAS[1], ANGLE)
    assert (plan['width'], plan['height']) == R1_CANVAS, plan
    assert plan['origin'] == R1_ORIGIN, plan
    inverse = invert(plan['matrix'])
    source = list(straighten_picture())
    for sampler in SAMPLERS:
        document = rotate_fixture(app, folder, f'r1_{sampler}')
        before = render(app, document)
        reply = app.call('rotate', document_id=document, degrees=ANGLE, sampler=sampler)
        assert reply['ok'] and reply.get('changed') is not False, reply
        assert reply['degrees'] == ANGLE, reply
        assert reply['sampler'] == sampler, reply
        assert (reply['canvas']['width'], reply['canvas']['height']) == R1_CANVAS, reply
        assert (reply['origin']['x'], reply['origin']['y']) == R1_ORIGIN, reply
        assert canvas_of(app, document) == R1_CANVAS, canvas_of(app, document)

        got = render(app, document)
        assert got['depth'] == 8, got['depth']
        want = reference_rgba(source, CANVAS, (0, 0) + R1_CANVAS, inverse, sampler)
        worst_alpha, worst_rgb = compare_rgba(
            list(got['samples']), want, f'r1 {sampler}: the rotated picture')

        # THE CONTROL: the UNROTATED picture padded into the same canvas. A
        # rotation that silently did nothing would land here.
        padded = image_of(R1_CANVAS[0], R1_CANVAS[1],
                          pad_crop(list(before['samples']), CANVAS, R1_CANVAS,
                                   R1_ORIGIN))
        distance = control_distance(got, padded, f'r1 {sampler} control')

        # The LOUD magenta stored at alpha 0 never bleeds: every sample whose
        # own alpha rounds to 0 stores four zeros (asserted above), and the
        # case is not vacuous.
        transparent = sum(1 for i in range(0, len(got['samples']), 4)
                          if got['samples'][i + 3] == 0)
        translucent = sum(1 for i in range(0, len(got['samples']), 4)
                          if 0 < got['samples'][i + 3] < 255)
        assert transparent > 0 and translucent > 0, (sampler, transparent, translucent)
        findings[sampler] = {'canvas': list(R1_CANVAS), 'origin': list(R1_ORIGIN),
                             'worst_alpha_codes': worst_alpha,
                             'worst_rgb_codes': worst_rgb,
                             'tolerance_codes': CODE_TOLERANCE,
                             'control_codes': distance,
                             'control_floor_codes': CONTROL_CODES,
                             'transparent_samples': transparent,
                             'translucent_samples': translucent}
    receipts['r1'] = findings
    print(f'PASS r1: a {ANGLE} degree rotation grows the canvas to {R1_CANVAS[0]} x '
          f'{R1_CANVAS[1]} and matches this file\'s own reference within '
          f'{CODE_TOLERANCE} code for bilinear and bicubic, with a control', flush=True)


def gate_r2(app, folder, receipts):
    findings = []
    for index, (canvas, degrees, want) in enumerate(R2_CASES):
        plan = rotation_plan(canvas[0], canvas[1], degrees)
        assert (plan['width'], plan['height']) == want, (canvas, degrees, plan)
        document = rotate_fixture(app, folder, f'r2_{index}', canvas=canvas)
        reply = app.call('rotate', document_id=document, degrees=degrees,
                         sampler='bilinear')
        assert (reply['canvas']['width'], reply['canvas']['height']) == want, reply
        assert (reply['origin']['x'], reply['origin']['y']) == plan['origin'], reply
        assert canvas_of(app, document) == want, canvas_of(app, document)
        findings.append({'canvas': list(canvas), 'degrees': degrees,
                         'new_canvas': list(want), 'origin': list(plan['origin']),
                         'shrinks_on_x': want[0] < canvas[0]})
    assert any(row['shrinks_on_x'] for row in findings), findings
    receipts['r2'] = findings
    print('PASS r2: the expanded-canvas table holds over MCP, including an axis that '
          'SHRINKS (100x10 at 80 degrees is 28x101)', flush=True)


def gate_r3(app, folder, receipts):
    plan = rotation_plan(CANVAS[0], CANVAS[1], ANGLE)
    inverse = invert(plan['matrix'])
    new_canvas = (plan['width'], plan['height'])
    document = rotate_fixture(app, folder, 'r3', guides=True, channel=True)
    before_ppi = resolution_of(app, document)
    channel_before = channel_of(app, document, 'Cover', CANVAS[0], CANVAS[1])
    app.call('deselect', document_id=document)
    app.call('rotate', document_id=document, degrees=ANGLE, sampler='bilinear')

    guides, ruler = chrome(app, document)
    want_guides = padded_guides(GUIDES, plan['origin'], new_canvas)
    want_ruler = padded_origin(RULER, plan['origin'], new_canvas)
    assert guides == want_guides, (guides, want_guides)
    assert ruler == want_ruler, (ruler, want_ruler)
    # Only an EXACT quarter turn transposes the ppi; this angle is not one.
    assert resolution_of(app, document) == before_ppi, resolution_of(app, document)

    got = channel_of(app, document, 'Cover', new_canvas[0], new_canvas[1])
    want = reference_plane(channel_before, CANVAS, (0, 0) + new_canvas,
                           inverse, 'bilinear')
    worst = compare_plane(got, want, 'r3: the saved channel')
    # THE CONTROL: the unrotated channel padded into the same canvas.
    padded = pad_crop(channel_before, CANVAS, new_canvas, plan['origin'], channels=1)
    distance = max(abs(g - p) for g, p in zip(got, padded))
    assert distance >= CONTROL_CODES, f'r3 channel control: only {distance} codes'
    app.call('deselect', document_id=document)

    receipts['r3'] = {'guides': guides, 'ruler': list(ruler),
                      'ppi': list(before_ppi), 'ppi_transposed': False,
                      'worst_channel_codes': worst, 'tolerance_codes': CODE_TOLERANCE,
                      'channel_control_codes': distance,
                      'control_floor_codes': CONTROL_CODES}
    print('PASS r3: the chrome follows the padded rule, the ppi is NOT transposed, and '
          f'the saved channel is within {CODE_TOLERANCE} code of this file\'s own '
          'reference with a control', flush=True)


def permute(samples, width, height, quarter):
    """This file's own quarter-turn permutation of an RGBA render. `quarter`
    is 1, 2 or 3 CLOCKWISE turns."""
    for _ in range(quarter % 4):
        out = [0] * len(samples)
        for y in range(height):
            for x in range(width):
                # Clockwise: (x, y) -> (height - 1 - y, x) in the h x w result.
                dx, dy = height - 1 - y, x
                src = (y * width + x) * 4
                dst = (dy * height + dx) * 4
                out[dst:dst + 4] = samples[src:src + 4]
        samples, width, height = out, height, width
    return samples, width, height


def permute_guides(guides, width, height, quarter):
    """The quarter turn's own guide rule: a turn exchanges each guide's
    orientation, because the line has turned with the picture."""
    rank = {'horizontal': 0, 'vertical': 1}
    for _ in range(quarter % 4):
        moved = []
        for orientation, position in guides:
            if orientation == 'vertical':
                moved.append(('horizontal', round(position, 4)))
            else:
                moved.append(('vertical', round(float(height) - position, 4)))
        guides, width, height = moved, height, width
    return sorted(guides, key=lambda g: (rank[g[0]], g[1]))


def gate_r4(app, folder, receipts):
    findings = {}
    for degrees, quarter in [(90, 1), (180, 2), (270, 3), (-90, 3), (450, 1), (-270, 1)]:
        document = rotate_fixture(app, folder, f'r4_{degrees}', guides=True)
        before = render(app, document)
        guides_before, _ = chrome(app, document)
        reply = app.call('rotate', document_id=document, degrees=degrees)
        assert reply['ok'], reply
        # An exact quarter turn takes TODAY's reply shape, with no `canvas`,
        # `origin`, `degrees` or `sampler` key of its own.
        assert 'canvas' not in reply and 'origin' not in reply, reply
        want, w, h = permute(list(before['samples']), before['width'],
                             before['height'], quarter)
        got = render(app, document)
        assert (got['width'], got['height']) == (w, h), ((got['width'], got['height']), (w, h))
        assert list(got['samples']) == want, f'{degrees}: the permutation'
        guides, _ = chrome(app, document)
        assert guides == permute_guides(guides_before, before['width'],
                                        before['height'], quarter), (degrees, guides)
        findings[str(degrees)] = {'canvas': [w, h], 'guides': guides,
                                  'equals_permutation': True}

    # THE SIGN. 89.999 degrees with Nearest is the 90-degree permutation
    # placed where the plan's origin puts it: the old canvas's top-left plus
    # the half-difference a quarter turn about the centre moves it by, which
    # is the new canvas's corner under the floor placement; -89.999 is the
    # OTHER turn and must differ loudly.
    plan = rotation_plan(CANVAS[0], CANVAS[1], 89.999)
    quarter_doc = rotate_fixture(app, folder, 'r4_sign_quarter')
    app.call('rotate', document_id=quarter_doc, degrees=90)
    quarter_render = render(app, quarter_doc)
    near_doc = rotate_fixture(app, folder, 'r4_sign_near')
    app.call('rotate', document_id=near_doc, degrees=89.999, sampler='nearest')
    near_render = render(app, near_doc)
    assert (near_render['width'], near_render['height']) == (plan['width'], plan['height'])
    window = (plan['origin'][0] + (CANVAS[0] - CANVAS[1]) // 2,
              plan['origin'][1] + (CANVAS[1] - CANVAS[0]) // 2,
              quarter_render['width'], quarter_render['height'])
    cut = []
    for j in range(window[3]):
        row = (j + window[1]) * near_render['width'] + window[0]
        cut.extend(near_render['samples'][row * 4:(row + window[2]) * 4])
    assert cut == list(quarter_render['samples']), \
        'r4: +89.999 nearest does not carry the 90-degree render'
    opposite_doc = rotate_fixture(app, folder, 'r4_sign_opposite')
    app.call('rotate', document_id=opposite_doc, degrees=-89.999, sampler='nearest')
    opposite = render(app, opposite_doc)
    distance = control_distance(near_render, opposite, 'r4: +89.999 against -89.999')
    findings['sign'] = {'window': list(window), 'matches_quarter_render': True,
                        'opposite_distance_codes': distance,
                        'control_floor_codes': CONTROL_CODES}
    receipts['r4'] = findings
    print('PASS r4: every exact quarter turn is this file\'s own permutation with its '
          'guides, and +89.999 carries the 90-degree picture while -89.999 does not',
          flush=True)


def no_op_document(app, folder, depth, tag):
    if depth == 32:
        source = folder / f'{tag}.tiff'
        shutil.copyfile(FLOAT_FIXTURE, source)
        return app.call('open_document', path=str(source))['document']['id']
    return rotate_fixture(app, folder, tag, depth=depth)


def symmetric_no_op_document(app, folder, depth, tag):
    """A stored 1x1 picture with centred chrome, not merely a symmetric render."""
    document = no_op_document(app, folder, depth, tag)
    app.call('crop', document_id=document, x=0, y=0, width=1, height=1)
    app.call('flatten_image', document_id=document)
    app.call('set_ruler_origin', document_id=document, x=0.5, y=0.5)
    return document


def gate_r5(app, folder, receipts):
    findings = {}
    for depth in (8, 16, 32):
        rows = []
        for degrees in (0, 360, -720):
            document = no_op_document(app, folder, depth, f'r5_{depth}_{degrees}')
            before_cursor = history(app, document)
            before = render(app, document)
            reply = app.call('rotate', document_id=document, degrees=degrees)
            assert reply['ok'] is True, (depth, degrees, reply)
            assert reply['changed'] is False, (depth, degrees, reply)
            assert reply.get('note'), (depth, degrees, reply)
            assert history(app, document) == before_cursor, (depth, degrees)
            after = render(app, document)
            assert list(after['samples']) == list(before['samples']), (depth, degrees)
            rows.append({'degrees': degrees, 'changed': False,
                         'note': reply['note'], 'cursor': before_cursor})
        # The FULL-CANVAS zero-angle crop follows the same rule (O1's
        # argument-decidable half).
        document = no_op_document(app, folder, depth, f'r5_{depth}_crop')
        before_cursor = history(app, document)
        before = render(app, document)
        width, height = canvas_of(app, document)
        reply = app.call('crop', document_id=document, x=0, y=0,
                         width=width, height=height)
        assert reply['ok'] is True and reply['changed'] is False, (depth, reply)
        assert reply.get('note'), (depth, reply)
        assert history(app, document) == before_cursor, depth
        assert list(render(app, document)['samples']) == list(before['samples']), depth
        # Content-dependent exact no-ops must also be absent from the
        # recorder's independent MCP feed. Stop WITHOUT a name so this probes
        # the in-memory steps directly and writes no Action.
        symmetric = symmetric_no_op_document(app, folder, depth, f'r5_record_{depth}')
        app.call('start_recording', discard=True)
        no_ops = [
            app.call('flip', document_id=symmetric, axis='horizontal'),
            app.call('flip', document_id=symmetric, axis='vertical'),
            app.call('rotate', document_id=symmetric, degrees=180),
            app.call('rotate', document_id=symmetric, degrees=0),
        ]
        assert all(row.get('ok') is True and row.get('changed') is False
                   for row in no_ops), (depth, no_ops)
        empty = app.call('stop_recording')
        assert empty['steps'] == [], (depth, empty)

        # Suppression is per call. The unchanged rotate is omitted, the guide
        # makes the following flip a real edit, and the recorder captures the
        # original 1x1 canvas for the retained step. Saving and replaying this
        # recording proves default stop-on-error playback continues past the
        # omitted no-op because there is no failing placeholder for it.
        app.call('add_guide', document_id=symmetric,
                 orientation='vertical', position=0.25)
        action_name = f'r5-noop-then-edit-{depth}'
        app.call('start_recording', discard=True)
        unchanged = app.call('rotate', document_id=symmetric, degrees=0)
        changed = app.call('flip', document_id=symmetric, axis='horizontal')
        assert unchanged.get('changed') is False and 'changed' not in changed, \
            (depth, unchanged, changed)
        mixed = app.call('stop_recording', name=action_name)
        assert [step['tool'] for step in mixed['steps']] == ['flip'], mixed
        listed = app.call('list_actions', name=action_name)['action']
        assert listed['recorded_canvas'] == {'width': 1, 'height': 1}, listed

        twin = symmetric_no_op_document(app, folder, depth, f'r5_replay_{depth}')
        app.call('add_guide', document_id=twin,
                 orientation='vertical', position=0.25)
        before = history(app, twin)
        played = app.call('run_action', document_id=twin, name=action_name)
        assert played['ok'] is True and played['steps_run'] == 1, played
        assert history(app, twin) == before + 1, (depth, history(app, twin), before)
        assert chrome(app, twin) == chrome(app, symmetric), \
            (depth, chrome(app, twin), chrome(app, symmetric))

        findings[str(depth)] = {
            'rotate': rows, 'full_canvas_crop': reply['note'],
            'no_op_recorded_steps': len(empty['steps']),
            'mixed_tools': [step['tool'] for step in mixed['steps']],
            'recorded_canvas': listed['recorded_canvas'],
            'replayed_steps': played['steps_run'],
        }
    receipts['r5'] = findings
    print('PASS r5: identity/content no-ops add no history or Action steps at all depths, '
          'and a following real edit alone records and replays', flush=True)


def gate_r6(app, folder, receipts):
    findings = {}
    for depth in (8, 16, 32):
        document = no_op_document(app, folder, depth, f'r6_{depth}')
        before_cursor = history(app, document)
        before = render(app, document)
        message = refuses(app, 'rotate', document_id=document, degrees='abc')
        assert message is not None, f'{depth}: an unreadable degrees must refuse'
        assert UNREADABLE in message, (depth, message)
        assert history(app, document) == before_cursor, depth
        assert list(render(app, document)['samples']) == list(before['samples']), depth
        findings[str(depth)] = {'message': message, 'steps': 0}
    # THE CONTROL: a READABLE string angle is accepted and equals the number.
    string_doc = rotate_fixture(app, folder, 'r6_string')
    app.call('rotate', document_id=string_doc, degrees='30', sampler='bilinear')
    numeric_doc = rotate_fixture(app, folder, 'r6_numeric')
    app.call('rotate', document_id=numeric_doc, degrees=30, sampler='bilinear')
    assert canvas_of(app, string_doc) == R1_CANVAS
    assert list(render(app, string_doc)['samples']) == \
        list(render(app, numeric_doc)['samples']), \
        'a readable string angle did not equal the numeric call'
    findings['readable_string_equals_numeric'] = True
    receipts['r6'] = findings
    print('PASS r6: an unreadable degrees refuses by name at all three depths with no '
          'history step, and a readable string angle still rotates', flush=True)


def gate_r7(app, folder, receipts):
    findings = {}
    # 16 BITS executes the same filtered rotation as byte depth. Compare
    # saved PNGs at their ACTUAL depths rather than display/render adapters.
    deep = rotate_fixture(app, folder, 'r7_16', depth=16, guides=True, channel=True)
    byte = rotate_fixture(app, folder, 'r7_8', depth=8, guides=True, channel=True)
    cursor = history(app, deep)
    typed_reply = app.call('rotate', document_id=deep, degrees=ANGLE, sampler='bilinear')
    app.call('rotate', document_id=byte, degrees=ANGLE, sampler='bilinear')
    assert typed_reply['ok'] and canvas_of(app, deep) == R1_CANVAS, typed_reply
    assert history(app, deep) == cursor + 1
    typed_png, byte_png = folder / 'r7_16_rotated.png', folder / 'r7_8_rotated.png'
    app.call('save_copy', document_id=deep, path=str(typed_png))
    app.call('save_copy', document_id=byte, path=str(byte_png))
    typed_saved = png_rgba(typed_png.read_bytes())
    byte_saved = png_rgba(byte_png.read_bytes())
    assert typed_saved['depth'] == 16 and byte_saved['depth'] == 8, \
        (typed_saved['depth'], byte_saved['depth'])
    worst = max(abs(t / 257.0 - b)
                for t, b in zip(typed_saved['samples'], byte_saved['samples']))
    assert worst <= 1.0, worst
    assert offsets(app, deep) == offsets(app, byte)
    assert chrome(app, deep) == chrome(app, byte)
    typed_channel = channel_of(app, deep, 'Cover', *R1_CANVAS)
    byte_channel = channel_of(app, byte, 'Cover', *R1_CANVAS)
    channel_worst = max(abs(a - b) for a, b in zip(typed_channel, byte_channel))
    assert channel_worst <= 1, channel_worst
    findings['typed'] = {'canvas': list(R1_CANVAS), 'history_steps': 1,
                         'saved_depth': typed_saved['depth'],
                         'byte_equivalent_worst_codes': worst,
                         'channel_worst_codes': channel_worst,
                         'offsets': offsets(app, deep), 'chrome': chrome(app, deep)}

    # Exact quarter delegation remains lossless at 16 bits.
    quarter = rotate_fixture(app, folder, 'r7_16_quarter', depth=16)
    app.call('rotate', document_id=quarter, degrees=90)
    assert canvas_of(app, quarter) == (CANVAS[1], CANVAS[0]), canvas_of(app, quarter)
    findings['typed_quarter_turn'] = list(canvas_of(app, quarter))

    float_source = folder / 'r7_float.tiff'
    shutil.copyfile(FLOAT_FIXTURE, float_source)
    floated = app.call('open_document', path=str(float_source))['document']['id']
    float_old = canvas_of(app, floated)
    cursor = history(app, floated)
    message = refuses(app, 'rotate', document_id=floated, degrees=ANGLE)
    assert message is None, message
    assert history(app, floated) == cursor + 1
    float_plan = rotation_plan(float_old[0], float_old[1], ANGLE)
    assert canvas_of(app, floated) == (float_plan['width'], float_plan['height']), \
        (canvas_of(app, floated), float_old)
    findings['float'] = {'admitted': True, 'steps': 1,
                         'canvas': list(canvas_of(app, floated))}
    # The float OPERATOR TABLE admits the family.
    support = app.call('get_precision', document_id=floated)
    admitted = (support.get('operators') or {}).get('admitted') or []
    assert FLOAT_FAMILY in admitted, 'the float operator table does not admit the rotation'
    findings['float_operator_admits'] = FLOAT_FAMILY

    # A placed image is an ordinary raster layer (`place_fixture` asserts
    # `source: null`): it rotates with the canvas and undo is exact.
    placed, source_index = place_fixture(app, folder, 8, 'r7_placed')
    placed_before = render(app, placed)
    placed_cursor = history(app, placed)
    app.call('rotate', document_id=placed, degrees=ANGLE)
    assert history(app, placed) == placed_cursor + 1
    source_after = app.call('get_image_source', document_id=placed, layer=source_index)
    assert source_after.get('source', 'absent') is None, source_after
    app.call('undo', document_id=placed)
    assert render(app, placed)['samples'] == placed_before['samples'], 'placed rotate undo'
    findings['placed_raster_layer'] = {'source': None, 'steps': 1, 'undo_exact': True}

    # Text remains editable under filtered and exact rotations.
    described = rotate_fixture(app, folder, 'r7_text')
    app.call('add_text_layer', document_id=described, text='Rotate', x=4, y=6, size=12)
    reply = app.call('rotate', document_id=described, degrees=ANGLE, sampler='bilinear')
    assert not reply.get('rasterized_layers'), reply
    assert any('text' in row for row in app.call('get_document',
        document_id=described)['layers']), 'filtered rotation lost editable text'
    findings['filtered_keeps_text'] = True
    editable = rotate_fixture(app, folder, 'r7_text_quarter')
    app.call('add_text_layer', document_id=editable, text='Rotate', x=4, y=6, size=12)
    quarter = app.call('rotate', document_id=editable, degrees=90)
    assert 'rasterized_layers' not in quarter, quarter
    summary = app.call('get_document', document_id=editable)
    kinds = [layer.get('kind') or layer.get('describes') for layer in summary['layers']]
    findings['quarter_turn_keeps_text'] = kinds
    receipts['r7'] = findings
    print('PASS r7: 16-bit filtered rotation matches byte output and all-plane/chrome '
          'placement, float is admitted, a placed raster layer rotates and editable text survives',
          flush=True)


def gate_r8(app, folder, receipts):
    findings = {}
    document = rotate_fixture(app, folder, 'r8', guides=True, channel=True)
    before = render(app, document)
    before_canvas = canvas_of(app, document)
    cursor = history(app, document)
    app.call('rotate', document_id=document, degrees=ANGLE, sampler='bicubic')
    rotated = render(app, document)
    assert history(app, document) == cursor + 1, history(app, document)
    app.call('undo', document_id=document)
    assert canvas_of(app, document) == before_canvas, canvas_of(app, document)
    assert list(render(app, document)['samples']) == list(before['samples']), 'undo'
    app.call('redo', document_id=document)
    assert canvas_of(app, document) == R1_CANVAS
    assert list(render(app, document)['samples']) == list(rotated['samples']), 'redo'
    findings['undo_redo'] = True

    # `.rz` save -> reopen -> resave is SHA-256 identical.
    first = folder / 'r8_first.rz'
    app.call('save_copy', document_id=document, path=str(first))
    reopened = app.call('open_document', path=str(first))['document']['id']
    second = folder / 'r8_second.rz'
    app.call('save_copy', document_id=reopened, path=str(second))
    assert sha256_of(first) == sha256_of(second), 'the .rz round trip is not identical'
    assert list(render(app, reopened)['samples']) == list(rotated['samples'])
    findings['rz_sha256'] = sha256_of(first)

    # THE RECORDED STEP: `degrees` a JSON NUMBER and a `sampler`.
    recorded = rotate_fixture(app, folder, 'r8_record')
    app.call('start_recording')
    app.call('rotate', document_id=recorded, degrees=ANGLE, sampler='bicubic')
    action = app.call('stop_recording', name='r8-rotate')
    steps = action['steps']
    assert [step['tool'] for step in steps] == ['rotate'], steps
    arguments = steps[0]['arguments']
    assert isinstance(arguments['degrees'], (int, float)) and \
        not isinstance(arguments['degrees'], bool), arguments
    assert float(arguments['degrees']) == ANGLE, arguments
    assert arguments['sampler'] == 'bicubic', arguments
    listed = app.call('list_actions', name='r8-rotate')['action']
    listed_arguments = listed['steps'][0]['arguments']
    assert isinstance(listed_arguments['degrees'], (int, float)), listed_arguments
    assert float(listed_arguments['degrees']) == ANGLE, listed_arguments
    assert listed_arguments['sampler'] == 'bicubic', listed_arguments
    findings['recorded'] = listed_arguments

    twin = rotate_fixture(app, folder, 'r8_twin')
    assert app.call('run_action', document_id=twin, name='r8-rotate')['ok']
    assert canvas_of(app, twin) == R1_CANVAS
    assert list(render(app, twin)['samples']) == \
        list(render(app, recorded)['samples']), 'the replay is not sample-for-sample equal'
    findings['replay_equal'] = True

    # THE GEOMETRY REMAP, and the ONE thing it cannot do. `canvasAfter`
    # advances the recorded space to the EXPANDED canvas, so a later
    # `relative` rectangle lands exactly where it was recorded on a SAME-SIZE
    # target. It does NOT follow the rotated CONTENT on a different-size one
    # — there is no rectangle that could — and that limit is disclosed.
    rect = {'x': 10, 'y': 10, 'width': 20, 'height': 20}
    steps = [{'tool': 'rotate',
              'arguments': {'degrees': ANGLE, 'sampler': 'bicubic'}},
             {'tool': 'select_rect', 'arguments': dict(rect),
              'geometry': {'mode': 'relative'}}]
    app.call('save_action', name='r8-remap', steps=steps,
             recorded_canvas={'width': CANVAS[0], 'height': CANVAS[1]})
    target = rotate_fixture(app, folder, 'r8_remap')
    assert app.call('run_action', document_id=target, name='r8-remap')['ok']
    assert canvas_of(app, target) == R1_CANVAS
    bounds = app.call('get_selection', document_id=target)['bounds']
    assert [int(bounds['x']), int(bounds['y']),
            int(bounds['width']), int(bounds['height'])] == \
        [rect['x'], rect['y'], rect['width'], rect['height']], bounds
    findings['relative_rect_in_expanded_space'] = bounds
    receipts['r8'] = findings
    print('PASS r8: undo/redo, an identical .rz round trip, a recorded step carrying a '
          'NUMBER and a sampler that replays exactly, and a relative rectangle placed '
          'in the expanded space', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', required=True)
    parser.add_argument('--receipts', required=True)
    args = parser.parse_args()
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'expectations': 'fixed in scripts/verify_phase12f1b3_rotation.py before '
                                'any measurement: alpha and RGB within '
                                f'{CODE_TOLERANCE} 8-bit code of this file\'s own f64 '
                                'inverse-mapping reference, a transparent result storing '
                                f'four zeros, and a {CONTROL_CODES}-code non-vacuity '
                                'control on every filtered case'}
    failures, not_run = [], []
    # A gate with no implementation is `None`: it prints NOT IMPLEMENTED and
    # this drive exits 2. It is never reported as a pass.
    gates = [('r1', lambda: gate_r1(app, folder, receipts)),
             ('r2', lambda: gate_r2(app, folder, receipts)),
             ('r3', lambda: gate_r3(app, folder, receipts)),
             ('r4', lambda: gate_r4(app, folder, receipts)),
             ('r5', lambda: gate_r5(app, folder, receipts)),
             ('r6', lambda: gate_r6(app, folder, receipts)),
             ('r7', lambda: gate_r7(app, folder, receipts)),
             ('r8', lambda: gate_r8(app, folder, receipts))]
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        for name, run in gates:
            if run is None:
                not_run.append(name)
                receipts[name] = {'status': 'NOT IMPLEMENTED'}
                print(f'NOT IMPLEMENTED {name}', flush=True)
                continue
            try:
                run()
            except BaseException as error:  # a failed gate IS the receipt
                failures.append(name)
                receipts.setdefault(name, {})['failure'] = repr(error)
                print(f'FAIL {name}: {error!r}', flush=True)

    receipts['failures'] = failures
    receipts['not_run'] = not_run
    (output / 'receipt.json').write_text(
        json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    if failures:
        print(f'FAILED GATES: {failures}', flush=True)
        sys.exit(1)
    if not_run:
        print(f'INCOMPLETE: not implemented: {not_run}', flush=True)
        sys.exit(2)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
