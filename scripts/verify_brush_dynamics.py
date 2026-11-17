#!/usr/bin/env python3
"""Brush dynamics (`designs/brush-dynamics.md`): `dynamics`, `tilts` and `rotations` on `brush_stroke`,
`eraser_stroke` and `quick_mask_stroke` (the Brush's, Pencil's and Eraser's Dynamics… sheet),
and `dynamics` in the brush preset library, on owned Debug instances.

The gates are FIXED here before anything is measured. Each instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library, one brush preset folder shared by the two launches) and ends with it; no
Doctored process this file did not start is signalled.

- **D1 seeded.** At 8, 16 and 32 bits and in Grayscale and CMYK, one scattered, jittered call
  paints the same render on two equal documents; another seed paints a different one. One undo
  restores the render.
- **D2 scatter band.** At 8 bits, a hard horizontal stroke scattered across its travel paints
  pixels outside the plain stroke's band and none farther from the path than
  `amount · size + size / 2 + 1`; the plain stroke stays inside its band.
- **D3 curve.** A one-dab stroke at pressure p with size on pressure through a bent curve paints
  what the plain one-dab stroke at `pressures: [C(p)]` with `pressure_size` paints, C this file's
  own natural cubic, at 8, 16 and 32 bits; without the curve it differs.
- **D4 tilt.** A one-dab stroke at tilt (0, 0.5) with size on tilt paints what the plain one-dab
  stroke at pressure 0.5 with `pressure_size` paints; without `tilts` it paints the full size.
- **D5 replay.** At 8 and 16 bits a recorded dynamics stroke replays to the same render, and the
  saved step holds `native_stroke` and no `dynamics`.
- **D6 tools.** `eraser_stroke` with scatter repeats exactly and erases outside the plain band;
  an aliased (Pencil) scattered stroke paints one colour; a scattered `quick_mask_stroke` leaves
  coverage outside the plain band.
- **D7 refusals.** Each named refusal changes nothing.
- **D8 presets.** Save with dynamics (file schema_version 2, the row carries them), a twin of
  tip and dynamics refused, the same tip without them refused, relaunch and list, export,
  delete, import back equal; a version-1 file still imports.
- **D9 geometry.** At 8 and 16 bits a recorded scattered, jittered stroke (and one with a tip
  shape) saved with absolute geometry replays to the recorded render on an equal document;
  saved relative and played on a document twice as wide and 1.5 times as tall, it paints what a
  direct call paints whose `native_stroke` is the recorded dabs remapped here (centres per
  axis, diameters by `√(sx · sy)`, shapes as recorded); the same call with the diameters left
  as recorded paints differently.
- **D10 tip shape.** At 8, 16 and 32 bits a one-dab stroke with the roundness on pressure p and
  minimum m paints the plain one-dab stroke at roundness `100·R·(m + (1 − m)·p)`; `rotations:
  [30]` with the angle on rotation paints the plain stroke at the angle + 30; a seeded shaped
  stroke repeats, records one shape per dab and replays exactly. At 16 bits the same roundness
  row on the coverage targets, `quick_mask_stroke` and `brush_stroke` on a layer mask, paints the
  plain stroke at that roundness and not the one at the tip's own. The heading rows (the angle on
  the direction of travel, recorded shapes): a stroke at rest after its travel, and an L path's
  airbrush ticks, each record the angle of the leg they lie on; the first stamp the tip's own.
- **D11 texture.** One hard dab at flow 100 on a transparent layer with the inline two-level
  tile `TILE`: an interior pixel's alpha is `round(255 · (1 − depth · (1 − t)))` at 8 bits
  (`round(65535 · …)` at 16, the unit value within 2 / 65535 at 32), `t` from this file's own
  `(x mod w, y mod h)` lookup (at scale 2 an even pixel reads texel `(x / 2, y / 2)`), at depth 1
  and 0.5 and scale 1 and 2. In Grayscale, CMYK and Lab, a textured stroke repeats exactly and
  differs from the plain one; Indexed and Bitmap the same, or the mode's refusal recorded. At 8
  and 16 bits a layer mask, an alpha channel, `quick_mask_stroke` and `eraser_stroke` each
  repeat and differ from the plain stroke. A library pattern `{name}` paints what the inline
  tile paints; a stroke recorded with it replays exactly after `delete_pattern`, its step
  holding the tile; saved absolute it replays to the recorded render, saved relative on a
  larger canvas it paints the direct call with the recorded dabs remapped and the texture as
  recorded, which differs from that call without the texture.
- **D12 dual brush** (two owned launches of its own, over a preset folder of its own). On a
  transparent layer: a click with a hard 40 px first tip and a hard 10 px second leaves full alpha
  on the second dab's interior and 0 on the first's interior outside the second's box; a line
  whose second tip walks at spacing 200 % records second centres `size · 2` apart, full at the
  pixel holding each and 0 at the pixel holding each midpoint; at 8, 16 and 32 bits a soft second
  tip over the hard first's interior leaves alpha `round(top · c)` within one code (2 / 65535 at
  32 bits), `c` this file's own smoothstep at the pixel centre. A seeded second scatter repeats
  and differs from the unscattered call. In Grayscale, CMYK and Lab a dual stroke repeats and
  differs from the plain one; Indexed and Bitmap the same, or the mode's refusal recorded. At 8
  and 16 bits a layer mask, an alpha channel, `quick_mask_stroke` and `eraser_stroke` each repeat
  and differ from the plain stroke. A recorded dual stroke's step holds `native_stroke.dual` and
  no `dynamics` and replays exactly; saved absolute it replays to the recorded render, saved
  relative on a larger canvas it paints the direct call with BOTH dab lists remapped (centres per
  axis, diameters by `√(sx · sy)`), which differs from the call with the second diameters left as
  recorded. An airbrush call's recorded `dual.dabs` equal the same call's without airbrush while
  its first list is longer (a tick never feeds the second walk). A preset holding `dual` saves at
  schema 2, lists after a relaunch, exports, deletes and imports back. Every named refusal
  (`dual` with `aliased`, a `native_stroke` `dual` on another tool, a malformed or pixel-brush
  `dual`, out-of-range and missing values, an unknown key) changes nothing.
- D7 also refuses `rotations` alone, of the wrong length or range and on another tool, `angle`
  and `roundness` with `aliased`, and a `native_stroke` with `shapes` on another tool or of the
  wrong length; and `texture` with `aliased`, a `native_stroke` with `texture` on another tool,
  on the pixel brush or naming a library pattern, a `{name}` the library lacks, a scale or depth
  out of range and a malformed tile.
- D8 also saves a textured preset by a library pattern's name (the row echoes its pattern
  without pixels; the file holds the tile), which lists, exports and imports back after a
  relaunch with another (empty) pattern library.

Usage: python3 -B scripts/verify_brush_dynamics.py [--receipts <folder>] [--only d12]
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 64, 48
INK = '#c03070'
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
STROKE = [[8, 24], [56, 24]]
SCATTERED = dict(seed=7, size={'jitter': 0.5}, opacity={'jitter': 0.4},
                 scatter={'amount': 1.5, 'count': 3, 'count_jitter': 0.5})
# SCATTERED with the tip shape: the angle on the direction of travel and jittered, the
# roundness jittered.
SHAPED = dict(SCATTERED, angle={'control': 'direction', 'jitter': 0.3},
              roundness={'control': 'off', 'jitter': 0.5})
# A canonical stroke of two dabs with their shapes (written here, as a recording writes one).
SHAPED_NATIVE = {
    'version': 1, 'encoding': 'le_f64_xy_diameter_opacity', 'hardness': 1.0, 'roundness': 0.5,
    'angle_degrees': 10.0, 'pixel': False,
    'dabs': base64.b64encode(struct.pack('<8d', 20, 24, 8, 1, 30, 24, 8, 1)).decode(),
    'shapes': base64.b64encode(struct.pack('<4d', 30, 0.5, -45, 0.25)).decode(),
}
CURVE = [[0, 0], [0.5, 0.2], [1, 1]]
P = 0.65
SIZE = 8
# D11's two-level tile: opaque white and black on a 2 × 2 checker, t 1 where x + y is even.
TILE_W, TILE_H = 2, 2
TILE_RGBA = bytes([255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255])
TILE = {'name': 'Two Level', 'width': TILE_W, 'height': TILE_H,
        'rgba': base64.b64encode(TILE_RGBA).decode()}
# D11's one dab: its centre and size; an interior pixel has all four corners inside the circle.
DAB = (30.5, 22.5)
DAB_SIZE = 24
# SHAPED_NATIVE's dabs with a texture and no shapes (written here, as a recording writes one).
TEXTURED_NATIVE = {k: v for k, v in SHAPED_NATIVE.items() if k != 'shapes'}
TEXTURED_NATIVE['texture'] = {'pattern': TILE, 'scale': 1, 'depth': 1}
# D12: SHAPED_NATIVE's dabs with a second tip of two 6 px dabs and no shapes.
DUAL_NATIVE = {k: v for k, v in SHAPED_NATIVE.items() if k != 'shapes'}
DUAL_NATIVE['dual'] = {'hardness': 1.0, 'roundness': 1.0, 'angle_degrees': 0.0,
                       'dabs': base64.b64encode(struct.pack('<8d', 20, 24, 6, 1, 30, 24, 6, 1)).decode()}
# D12's dual brush: a 6 px second tip at spacing 50 % (a 3 px step).
DUAL = {'size': 6, 'spacing': 50}


def tile_t(x, y, scale):
    """The texture's `t` at canvas pixel (x, y): the Gray luminosity of the tile's texel times
    its alpha, the tile anchored at the canvas origin; at scale 2 only for an even pixel, which
    reads texel (x / 2, y / 2) exactly."""
    if scale == 2:
        assert x % 2 == 0 and y % 2 == 0, (x, y)
        x, y = x // 2, y // 2
    k = ((y % TILE_H) * TILE_W + x % TILE_W) * 4
    r, g, b, a = TILE_RGBA[k:k + 4]
    return (0.30 * r + 0.59 * g + 0.11 * b) / 255 * a / 255


def interior(centre, size):
    """The pixels whose four corners lie inside the dab's circle, with a margin."""
    cx, cy = centre
    return [(x, y) for y in range(H) for x in range(W)
            if all(math.hypot(px - cx, py - cy) <= size / 2 - 0.01
                   for px in (x, x + 1) for py in (y, y + 1))]


def natural_cubic(points, x):
    """The natural cubic spline through `points` at `x` (second derivatives 0 at the ends),
    by Gaussian elimination on the full system, clamped to 0…1."""
    n = len(points)
    h = [points[k + 1][0] - points[k][0] for k in range(n - 1)]
    a = [[0.0] * (n + 1) for _ in range(n)]
    a[0][0] = a[n - 1][n - 1] = 1.0
    for i in range(1, n - 1):
        a[i][i - 1], a[i][i], a[i][i + 1] = h[i - 1], 2 * (h[i - 1] + h[i]), h[i]
        a[i][n] = 6 * ((points[i + 1][1] - points[i][1]) / h[i]
                       - (points[i][1] - points[i - 1][1]) / h[i - 1])
    for col in range(n):
        for row in range(col + 1, n):
            f = a[row][col] / a[col][col]
            a[row] = [v - f * w for v, w in zip(a[row], a[col])]
    m = [0.0] * n
    for row in reversed(range(n)):
        m[row] = (a[row][n] - sum(a[row][k] * m[k] for k in range(row + 1, n))) / a[row][row]
    k = next(k for k in range(n - 1) if x <= points[k + 1][0])
    (x0, y0), (x1, y1) = points[k], points[k + 1]
    t = x1 - x0
    p, q = (x1 - x) / t, (x - x0) / t
    v = p * y0 + q * y1 + ((p ** 3 - p) * m[k] + (q ** 3 - q) * m[k + 1]) * t * t / 6
    return min(max(v, 0.0), 1.0)


def png_pixels(data):
    from verify_phase2 import read_png
    return read_png(base64.b64decode(data))


def distance_to_stroke(x, y):
    """A pixel centre's distance from the segment STROKE[0]–STROKE[1]."""
    (ax, ay), (bx, by) = STROKE
    px, py = x + 0.5, y + 0.5
    t = min(max(((px - ax) * (bx - ax) + (py - ay) * (by - ay)) / ((bx - ax) ** 2 + (by - ay) ** 2), 0), 1)
    return math.hypot(px - (ax + t * (bx - ax)), py - (ay + t * (by - ay)))


def quantize(value, like):
    """A remapped number as the Actions remap writes it: an integer stays an integer (rounded
    half away from zero), anything else to four decimals."""
    if isinstance(like, int):
        return int(math.copysign(math.floor(abs(value) + 0.5), value))
    return math.copysign(math.floor(abs(value * 10000) + 0.5), value) / 10000


def decode_dabs(native):
    """A canonical stroke's dabs as (x, y, diameter, opacity) tuples."""
    data = base64.b64decode(native['dabs'])
    return [struct.unpack_from('<dddd', data, 32 * i) for i in range(len(data) // 32)]


def decode_shapes(native):
    """A canonical stroke's shapes as (angle_degrees, roundness) pairs; empty without."""
    data = base64.b64decode(native.get('shapes', ''))
    return [struct.unpack_from('<dd', data, 16 * i) for i in range(len(data) // 16)]


def encode_dabs(native, dabs):
    """`native` with its dabs replaced, every other key kept."""
    data = b''.join(struct.pack('<dddd', *dab) for dab in dabs)
    return dict(native, dabs=base64.b64encode(data).decode())


def recorded_step(actions, name):
    """The one step of the recorded action `name` in the Actions folder."""
    files = [json.loads(p.read_text()) for p in actions.glob('*')
             if p.is_file() and f'"{name}"' in p.read_text()]
    assert len(files) == 1, (name, len(files))
    steps = files[0]['steps']
    assert len(steps) == 1, (name, steps)
    return steps[0]


def smoothstep(d, hardness):
    """The soft tip's coverage at normalized distance d: 1 inside `hardness`, 0 at the rim and
    beyond, the smoothstep between."""
    if d <= hardness:
        return 1.0
    if d >= 1:
        return 0.0
    k = (d - hardness) / (1 - hardness)
    return 1 - k * k * (3 - 2 * k)


def decode_dual(native):
    """A canonical stroke's second dabs as (x, y, diameter, opacity) tuples."""
    return decode_dabs(native['dual'])


def dual_brush(folder, receipts):
    """D12 (the module doc), on two owned launches over a brush preset folder of their own."""
    from phase5_support import App
    shared = os.environ.get('DR_BRUSH_PRESETS_DIR')
    os.environ['DR_BRUSH_PRESETS_DIR'] = str(folder / 'dual-brush-presets')
    rows = receipts.setdefault('d12', {})
    try:
        with App(folder / 'd12-app1', folder / 'd12-recovery1', interval=600) as app:
            call = app.call
            actions = folder / 'd12-app1' / 'actions'

            def render(doc, **extra):
                reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                    'document_id': doc, 'max_side': 4096, **extra}})
                assert not reply.get('isError'), reply
                return png_pixels(next(item['data'] for item in reply['content']
                                       if item['type'] == 'image'))

            def changed(before, after, size=(W, H)):
                assert before[:2] == after[:2] == size, (before[:2], after[:2])
                return {(i % size[0], i // size[0]): q
                        for i, (p, q) in enumerate(zip(before[2], after[2])) if p != q}

            def sized(size, spec, background='white'):
                return call('new_document', width=size[0], height=size[1], background=background,
                            **spec)['document']['id']

            def close(doc):
                call('close_document', document_id=doc, discard_changes=True)

            def painted(spec, tool, **arguments):
                doc = sized((W, H), spec)
                blank = render(doc)
                call(tool, document_id=doc, **arguments)
                after = render(doc)
                close(doc)
                return blank, after

            def alpha(doc, x, y):
                reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=0)
                for key in ('center_rgba32', 'center_rgba_codes'):
                    if reply.get(key) is not None:
                        return reply[key][3]
                raise AssertionError(('no readout', sorted(reply)))

            names = iter(range(1000))

            def recorded(spec, canvas, **arguments):
                """One brush_stroke on a fresh document, recorded: its render and its step's
                arguments without the document."""
                name = f'dual-{next(names)}'
                doc = sized(canvas, spec)
                call('start_recording', discard=True)
                call('brush_stroke', document_id=doc, **arguments)
                after = render(doc)
                call('stop_recording', name=name)
                close(doc)
                step = recorded_step(actions, name)
                return after, {k: v for k, v in step['arguments'].items() if k != 'document_id'}

            # The click: a hard 40 px first tip and a hard 10 px second at one point.
            centre = (30.5, 22.5)
            doc = sized((W, H), KINDS['rgb8'], 'transparent')
            call('brush_stroke', document_id=doc, points=[list(centre)], size=40, color=INK,
                 hardness=100, dynamics={'dual': {'size': 10}})
            second_inside = interior(centre, 10)
            box = (math.floor(centre[0] - 5), math.floor(centre[1] - 5),
                   math.ceil(centre[0] + 5), math.ceil(centre[1] + 5))
            outside = [(x, y) for (x, y) in interior(centre, 40)
                       if not (box[0] <= x < box[2] and box[1] <= y < box[3])]
            assert len(second_inside) > 40 and len(outside) > 800, (len(second_inside), len(outside))
            full = [alpha(doc, x, y) for (x, y) in second_inside]
            empty = [alpha(doc, x, y) for (x, y) in outside[::7]]
            close(doc)
            assert set(full) == {255}, ('the second interior is full', sorted(set(full)))
            assert set(empty) == {0}, ('the first interior outside the second box', sorted(set(empty)))
            rows['click'] = {'full': len(full), 'empty': len(empty)}
            print(f'PASS d12 click: {len(full)} px of the 10 px second interior are full; '
                  f'{len(empty)} px of the 40 px first interior outside its box are 0', flush=True)

            # The line: the second tip walks at spacing 200 %, a 12 px step.
            doc = sized((W, H), KINDS['rgb8'], 'transparent')
            call('start_recording', discard=True)
            call('brush_stroke', document_id=doc, points=STROKE, size=30, color=INK, hardness=100,
                 dynamics={'dual': {'size': 6, 'spacing': 200}})
            call('stop_recording', name='dual-line')
            centres = [(x, y) for x, y, _, _ in
                       decode_dual(recorded_step(actions, 'dual-line')['arguments']['native_stroke'])]
            gaps = [math.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(centres, centres[1:])]
            assert len(centres) >= 3 and all(abs(g - 12) < 1e-9 for g in gaps), (centres, gaps)
            at = [alpha(doc, math.floor(x), math.floor(y)) for x, y in centres]
            mid = [alpha(doc, math.floor((a[0] + b[0]) / 2), math.floor((a[1] + b[1]) / 2))
                   for a, b in zip(centres, centres[1:])]
            close(doc)
            assert set(at) == {255} and set(mid) == {0}, ('the 200 % line', at, mid)
            rows['line'] = {'centres': centres, 'at': at, 'mid': mid}
            print(f'PASS d12 line: {len(centres)} second dabs 12 px apart, full at each centre and '
                  '0 midway', flush=True)

            # A soft second tip over the hard first's interior, against this file's smoothstep.
            soft = {}
            for kind in ('rgb8', 'rgb16', 'rgb32'):
                doc = sized((W, H), KINDS[kind], 'transparent')
                call('brush_stroke', document_id=doc, points=[list(centre)], size=40, color=INK,
                     hardness=100, dynamics={'dual': {'size': 20, 'hardness': 0.5}})
                first = set(interior(centre, 40))
                values = set()
                for y in range(12, 34):
                    for x in range(20, 42):
                        if (x, y) not in first:
                            continue
                        c = smoothstep(math.hypot(x + 0.5 - centre[0], y + 0.5 - centre[1]) / 10, 0.5)
                        got = alpha(doc, x, y)
                        if kind == 'rgb32':
                            ok = abs(got - c) <= 2 / 65535
                        else:
                            top = 255 if kind == 'rgb8' else 65535
                            ok = abs(got - math.floor(top * c + 0.5)) <= 1
                        assert ok, (kind, (x, y), got, c)
                        values.add(got)
                close(doc)
                assert len(values) > 10, (kind, 'the soft second tip ramps', len(values))
                soft[kind] = len(values)
                print(f'PASS {kind} d12 soft: a soft 20 px second tip over the hard first leaves '
                      f'its own smoothstep ({len(values)} levels)', flush=True)
            rows['soft_levels'] = soft

            # A seeded second scatter repeats and moves the second dabs.
            line = dict(points=STROKE, size=16, color=INK, hardness=100)
            scattered = {'seed': 9, 'dual': dict(DUAL, scatter={'amount': 2, 'count': 3})}
            _, first = painted(KINDS['rgb8'], 'brush_stroke', **line, dynamics=scattered)
            _, again = painted(KINDS['rgb8'], 'brush_stroke', **line, dynamics=scattered)
            _, unscattered = painted(KINDS['rgb8'], 'brush_stroke', **line, dynamics={'dual': DUAL})
            assert first == again and first != unscattered, 'the seeded second scatter'
            print('PASS d12 scatter: a seeded second scatter repeats and differs from the '
                  'unscattered call', flush=True)

            # The colour modes.
            dual = {'dynamics': {'dual': DUAL}}
            plain = {'stroke_version': 1}

            def mode_document(kind):
                if kind == 'indexed':
                    doc = sized((W, H), KINDS['rgb8'])
                    call('select_rect', document_id=doc, x=0, y=0, width=6, height=6)
                    call('fill_selection', document_id=doc, color=INK)
                    call('deselect', document_id=doc)
                    call('convert_mode', document_id=doc, mode='indexed', palette='exact',
                         dither='none')
                    return doc
                if kind == 'bitmap':
                    doc = sized((W, H), KINDS['gray8'])
                    call('convert_mode', document_id=doc, mode='bitmap', method='threshold')
                    return doc
                return sized((W, H), dict(color_mode='lab', depth=8) if kind == 'lab8' else KINDS[kind])

            modes = {}
            stroke = dict(points=STROKE, size=SIZE * 2, hardness=100)
            for kind in ('gray8', 'cmyk8', 'lab8', 'indexed', 'bitmap'):
                colour = '#000000' if kind == 'bitmap' else INK
                renders, refusal = [], None
                for extra in (dual, dual, plain):
                    doc = mode_document(kind)
                    reply = app.client.rpc('tools/call', {'name': 'brush_stroke', 'arguments': dict(
                        stroke, document_id=doc, color=colour, **extra)})
                    if reply.get('isError'):
                        refusal = next(c['text'] for c in reply['content'] if c['type'] == 'text')
                    renders.append(render(doc))
                    close(doc)
                if kind in ('indexed', 'bitmap') and refusal is not None:
                    modes[kind] = f'refused: {refusal}'
                    print(f'NOTE {kind} d12: the mode refuses the stroke: {refusal}', flush=True)
                    continue
                assert refusal is None, (kind, refusal)
                assert renders[0] == renders[1], (kind, 'the dual stroke repeats')
                assert renders[0] != renders[2], (kind, 'the dual brush changed nothing')
                modes[kind] = len(changed(renders[2], renders[0]))
                print(f'PASS {kind} d12: a dual stroke repeats and differs from the plain one by '
                      f'{modes[kind]} px', flush=True)
            rows['modes'] = modes

            # The coverage targets and the Eraser.
            everywhere = [{'x': x, 'y': y} for y in range(H) for x in range(W)]

            def target_read(depth, target, extra):
                doc = sized((W, H), dict(depth=depth, profile='srgb'))
                if target == 'mask':
                    call('add_layer_mask', document_id=doc, layer=0, kind='hide_all')
                    call('brush_stroke', document_id=doc, layer=0, target='mask', **stroke, **extra)
                    out = render(doc, layer=0, channel='mask')
                elif target == 'channel':
                    call('add_channel', document_id=doc, name='Alpha 1')
                    call('brush_stroke', document_id=doc, layer=0, target='channel:Alpha 1',
                         **stroke, **extra)
                    out = render(doc, channel='Alpha 1')
                elif target == 'quick mask':
                    call('set_quick_mask', document_id=doc, active=True)
                    call('quick_mask_stroke', document_id=doc, width=W, height=H, opacity=1,
                         **stroke, **extra)
                    call('set_quick_mask', document_id=doc, active=False)
                    samples = call('get_selection', document_id=doc, points=everywhere)['samples']
                    out = [(s.get('coverage'), s.get('coverage16')) for s in samples]
                else:
                    call('eraser_stroke', document_id=doc, **stroke, **extra)
                    out = render(doc)
                close(doc)
                return out

            for depth in (8, 16):
                for target in ('mask', 'channel', 'quick mask', 'eraser'):
                    first, again, single = (target_read(depth, target, extra)
                                            for extra in (dual, dual, plain))
                    assert first == again, (depth, target, 'the dual stroke repeats')
                    assert first != single, (depth, target, 'the dual brush changed nothing')
                print(f'PASS {depth}-bit d12 targets: a layer mask, an alpha channel, '
                      'quick_mask_stroke and eraser_stroke each repeat dual and differ from the '
                      'plain stroke', flush=True)

            # The recording: the step holds the second dabs, replays exactly, and remaps both
            # lists.
            arguments_line = dict(points=STROKE, size=SIZE * 2, color=INK, hardness=100,
                                  dynamics={'seed': 4, 'dual': dict(DUAL, scatter={'amount': 1})})
            doc = sized((W, H), KINDS['rgb8'])
            call('start_recording', discard=True)
            call('brush_stroke', document_id=doc, **arguments_line)
            recorded_render = render(doc)
            call('stop_recording', name='dual-replay')
            call('undo', document_id=doc)
            call('run_action', document_id=doc, name='dual-replay')
            assert render(doc) == recorded_render, 'the dual replay differs'
            close(doc)
            step = recorded_step(actions, 'dual-replay')
            arguments = {k: v for k, v in step['arguments'].items() if k != 'document_id'}
            native = arguments['native_stroke']
            assert 'dynamics' not in arguments and len(decode_dual(native)) > 5, sorted(arguments)
            for mode in ('absolute', 'relative'):
                save = app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
                    'name': f'dual-{mode}', 'overwrite': True,
                    'recorded_canvas': {'width': W, 'height': H},
                    'steps': [{'tool': 'brush_stroke', 'arguments': arguments,
                               'geometry': {'mode': mode}}]}})
                assert not save.get('isError'), save
            doc = sized((W, H), KINDS['rgb8'])
            call('run_action', document_id=doc, name='dual-absolute')
            assert render(doc) == recorded_render, 'the absolute dual replay differs'
            close(doc)
            live = (2 * W, H * 3 // 2)
            factor = math.sqrt((live[0] / W) * (live[1] / H))
            doc = sized(live, KINDS['rgb8'])
            call('run_action', document_id=doc, name='dual-relative')
            played = render(doc)
            close(doc)

            def remapped(dabs, scale=True):
                return [(x * live[0] / W, y * live[1] / H,
                         min(max(d * factor, 1), 5000) if scale else d, o) for x, y, d, o in dabs]

            size = quantize(min(max(arguments['size'] * factor, 1), 5000), arguments['size'])
            points = [[quantize(x * live[0] / W, x), quantize(y * live[1] / H, y)]
                      for x, y in arguments['points']]
            first_mapped = encode_dabs(native, remapped(decode_dabs(native)))
            direct = dict(arguments, points=points, size=size, native_stroke=dict(
                first_mapped, dual=encode_dabs(native['dual'], remapped(decode_dual(native)))))
            unscaled = dict(direct, native_stroke=dict(
                first_mapped, dual=encode_dabs(native['dual'],
                                               remapped(decode_dual(native), scale=False))))
            renders = []
            for each in (direct, unscaled):
                doc = sized(live, KINDS['rgb8'])
                call('brush_stroke', document_id=doc, **each)
                renders.append(render(doc))
                close(doc)
            assert played == renders[0], ('the relative dual replay differs from the remapped '
                                          'direct call', len(changed(played, renders[0], live)))
            assert renders[0] != renders[1], 'unscaled second diameters change nothing'
            print(f'PASS d12 replay: the recorded step holds {len(decode_dual(native))} second '
                  'dabs and replays exactly absolute; relative it remaps both lists', flush=True)

            # The airbrush: a tick never feeds the second walk.
            held = dict(points=[[10, 24], [50, 24], [50, 24]], times=[0, 0.2, 0.7], size=12,
                        color=INK, hardness=100, dynamics={'dual': DUAL})
            _, walked = recorded(KINDS['rgb8'], (W, H), **held)
            _, ticked = recorded(KINDS['rgb8'], (W, H), airbrush=True, **held)
            walked, ticked = walked['native_stroke'], ticked['native_stroke']
            assert ticked['dual']['dabs'] == walked['dual']['dabs'], 'a tick fed the second walk'
            assert len(decode_dabs(ticked)) > len(decode_dabs(walked)), 'no airbrush ticks'
            rows['airbrush'] = {'first': [len(decode_dabs(walked)), len(decode_dabs(ticked))],
                                'second': len(decode_dual(walked))}
            print(f'PASS d12 airbrush: {len(decode_dabs(ticked)) - len(decode_dabs(walked))} ticks '
                  f'lay first-tip dabs; the {len(decode_dual(walked))} second dabs are the '
                  'call\'s without airbrush', flush=True)

            # The refusals.
            doc = sized((W, H), KINDS['rgb8'])
            before = render(doc)
            base = dict(document_id=doc, points=STROKE, size=SIZE)
            refusals = {
                'dual_pencil': (call('brush_stroke', fail=True, aliased=True,
                                     dynamics={'dual': DUAL}, **base),
                                "dynamics.dual multiplies a stamped tip's coverage; the Pencil's "
                                'tip (aliased) is two-valued and takes no dual brush.'),
                'dual_tool': (call('dodge_burn', fail=True, document_id=doc,
                                   native_stroke=DUAL_NATIVE), "native_stroke dual is the Brush's"),
                'dual_malformed': (call('brush_stroke', fail=True, document_id=doc, color=INK,
                                        native_stroke=dict(DUAL_NATIVE, dual=dict(
                                            DUAL_NATIVE['dual'], dabs='AAAA'))),
                                   "A native stroke's dual brush holds a second tip"),
                'dual_pixel': (call('brush_stroke', fail=True, document_id=doc, color=INK,
                                    native_stroke=dict(DUAL_NATIVE, pixel=True)),
                               "A native stroke's dual brush holds a second tip"),
                'dual_size': (call('brush_stroke', fail=True, dynamics={'dual': {'size': 6000}},
                                   **base), 'dynamics.dual.size must be a number from 1 to 5000'),
                'dual_no_size': (call('brush_stroke', fail=True, dynamics={'dual': {'spacing': 50}},
                                      **base), 'dynamics.dual needs a size'),
                'dual_spacing': (call('brush_stroke', fail=True,
                                      dynamics={'dual': {'size': 6, 'spacing': 0.5}}, **base),
                                 'dynamics.dual.spacing must be a number from 1 to 200'),
                'dual_key': (call('brush_stroke', fail=True,
                                  dynamics={'dual': {'size': 6, 'flow': 1}}, **base),
                             'dynamics.dual takes size, hardness, roundness, angle_degrees, '
                             'spacing, scatter; it has no flow.'),
                'dual_count': (call('brush_stroke', fail=True,
                                    dynamics={'dual': {'size': 6, 'scatter': {'count': 17}}},
                                    **base), 'dynamics.dual.scatter.count'),
            }
            for name, (reply, needle) in refusals.items():
                assert needle in reply, (name, reply)
            assert not changed(before, render(doc)), 'a dual refusal changed pixels'
            close(doc)
            rows['refusals'] = {k: v[0] for k, v in refusals.items()}
            print(f'PASS d12 refusals: {len(refusals)} refusals by name change nothing', flush=True)

            # The preset.
            tip = dict(hardness=60, flow=100, spacing=25, angle=0, roundness=100, airbrush=False)
            saved = call('save_brush_preset', name='Dual Round', dynamics={'dual': dict(
                DUAL, hardness=0.5, scatter={'amount': 1, 'count': 2})}, **tip)['preset']
            assert saved['dynamics']['dual']['size'] == 6 \
                and saved['dynamics']['dual']['scatter']['count'] == 2, saved
            body = json.loads(next((folder / 'dual-brush-presets').glob('*.json')).read_text())
            assert body['schema_version'] == 2 and body['dynamics']['dual']['hardness'] == 0.5, body

        with App(folder / 'd12-app2', folder / 'd12-recovery2', interval=600) as app:
            call = app.call
            listing = call('list_brush_presets')['presets']
            mine = [p for p in listing if not p['builtin']]
            assert [p['name'] for p in mine] == ['Dual Round'] \
                and mine[0]['dynamics'] == saved['dynamics'], mine
            exported = folder / 'dual.doctoredbrushes'
            assert call('export_brush_presets', path=str(exported))['exported'] == ['Dual Round']
            call('delete_brush_preset', name='Dual Round')
            reply = call('import_brush_presets', path=str(exported))
            assert reply['imported'] == ['Dual Round'] and reply['skipped'] == [], reply
            assert call('list_brush_presets')['presets'] == listing, 'import does not restore it'
            print('PASS d12 preset: a dual preset saves at schema 2, lists after a relaunch, '
                  'exports, deletes and imports back', flush=True)
    finally:
        if shared is None:
            os.environ.pop('DR_BRUSH_PRESETS_DIR', None)
        else:
            os.environ['DR_BRUSH_PRESETS_DIR'] = shared


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--only', choices=['d12'], default=None,
                        help='run D12 (the dual brush) alone')
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='brush-dynamics-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    presets_dir = folder / 'shared-brush-presets'
    os.environ['DR_BRUSH_PRESETS_DIR'] = str(presets_dir)
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_brush_dynamics.py'}

    # ---- D12 dual brush (its own launches and preset folder) ----------------------------------
    dual_brush(folder, receipts)
    if args.only == 'd12':
        (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
        print(f'PASS d12; receipts in {output}', flush=True)
        return

    with App(folder / 'app1', folder / 'recovery1', interval=600) as app:
        call = app.call

        def render(doc, **extra):
            reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'max_side': 4096, **extra}})
            assert not reply.get('isError'), reply
            return png_pixels(next(item['data'] for item in reply['content'] if item['type'] == 'image'))

        def changed(before, after, size=(W, H)):
            assert before[:2] == after[:2] == size, (before[:2], after[:2])
            width = size[0]
            return {(i % width, i // width): q
                    for i, (p, q) in enumerate(zip(before[2], after[2])) if p != q}

        def sized(size, spec):
            return call('new_document', width=size[0], height=size[1], background='white',
                        **spec)['document']['id']

        def new(spec):
            return sized((W, H), spec)

        def painted(spec, tool, **arguments):
            doc = new(spec)
            blank = render(doc)
            call(tool, document_id=doc, **arguments)
            after = render(doc)
            return doc, blank, after

        # ---- D1 seeded ------------------------------------------------------------------------
        for kind, spec in KINDS.items():
            stroke = dict(points=STROKE, size=SIZE, color=INK, hardness=100, dynamics=SCATTERED)
            doc, blank, first = painted(spec, 'brush_stroke', **stroke)
            call('undo', document_id=doc)
            assert not changed(blank, render(doc)), (kind, 'undo')
            call('close_document', document_id=doc, discard_changes=True)
            doc, _, second = painted(spec, 'brush_stroke', **stroke)
            call('close_document', document_id=doc, discard_changes=True)
            other = dict(stroke, dynamics=dict(SCATTERED, seed=8))
            doc, _, third = painted(spec, 'brush_stroke', **other)
            call('close_document', document_id=doc, discard_changes=True)
            assert first == second, (kind, 'the same seeded call painted differently')
            assert changed(first, third), (kind, 'another seed painted the same')
            receipts[f'd1_{kind}'] = {'painted': len(changed(blank, first)),
                                      'seed_differs': len(changed(first, third))}
            print(f'PASS {kind} d1: one seeded call repeats exactly over {len(changed(blank, first))} '
                  f'px, another seed moves {len(changed(first, third))}; undo restores', flush=True)

        # ---- D2 scatter band ------------------------------------------------------------------
        spec = KINDS['rgb8']
        bound = 1.5 * SIZE + SIZE / 2 + 1
        scatter = dict(seed=3, scatter={'amount': 1.5, 'count': 3})
        doc, blank, after = painted(spec, 'brush_stroke', points=STROKE, size=SIZE, color=INK,
                                    hardness=100, dynamics=scatter)
        call('close_document', document_id=doc, discard_changes=True)
        far = [distance_to_stroke(x, y) for (x, y) in changed(blank, after)]
        doc, blank, plain = painted(spec, 'brush_stroke', points=STROKE, size=SIZE, color=INK,
                                    hardness=100, stroke_version=1)
        call('close_document', document_id=doc, discard_changes=True)
        near = [distance_to_stroke(x, y) for (x, y) in changed(blank, plain)]
        band = SIZE / 2 + 1
        assert max(near) <= band, ('the plain stroke leaves its band', max(near))
        assert sum(1 for d in far if d > band) > 20, ('scatter stays in the band', sorted(far)[-5:])
        assert max(far) <= bound, ('scatter beyond its bound', max(far), bound)
        receipts['d2'] = {'outside_band': sum(1 for d in far if d > band), 'farthest': max(far)}
        print(f'PASS d2: scatter paints {receipts["d2"]["outside_band"]} px outside the plain band, '
              f'the farthest {max(far):.2f} px from the path (bound {bound})', flush=True)

        # ---- D3 curve, D4 tilt ----------------------------------------------------------------
        c = natural_cubic(CURVE, P)
        assert abs(c - P) > 0.05, 'the curve is not bent at P'
        for kind in ('rgb8', 'rgb16', 'rgb32'):
            spec = KINDS[kind]
            one = dict(points=[[30.3, 20.6]], size=30, color=INK, hardness=100)
            _, blank, curved = painted(spec, 'brush_stroke', pressures=[P], **one,
                                       dynamics={'size': {'control': 'pressure', 'curve': CURVE}})
            _, _, oracle = painted(spec, 'brush_stroke', pressures=[c], pressure_size=True,
                                   stroke_version=1, **one)
            _, _, straight = painted(spec, 'brush_stroke', pressures=[P], pressure_size=True,
                                     stroke_version=1, **one)
            assert curved == oracle, (kind, 'the curve', len(changed(curved, oracle)))
            assert changed(curved, straight), (kind, 'the curve changed nothing')
            _, _, tilted = painted(spec, 'brush_stroke', tilts=[[0, 0.5]], **one,
                                   dynamics={'size': {'control': 'tilt'}})
            _, _, half = painted(spec, 'brush_stroke', pressures=[0.5], pressure_size=True,
                                 stroke_version=1, **one)
            _, _, untilted = painted(spec, 'brush_stroke', **one, dynamics={'size': {'control': 'tilt'}})
            _, _, full = painted(spec, 'brush_stroke', stroke_version=1, **one)
            assert tilted == half, (kind, 'tilt', len(changed(tilted, half)))
            assert untilted == full and changed(full, half), (kind, 'no tilt is not off')
            for d in call('list_documents')['documents']:
                call('close_document', document_id=d['id'], discard_changes=True)
            print(f'PASS {kind} d3/d4: a pressure curve paints the plain stroke at C({P}) = {c:.4f}; '
                  'tilt (0, 0.5) paints the plain stroke at pressure 0.5; no tilt reads as off',
                  flush=True)
        receipts['d3_curve_value'] = c

        # ---- D5 replay ------------------------------------------------------------------------
        for kind in ('rgb8', 'rgb16'):
            doc = new(KINDS[kind])
            call('start_recording', discard=True)
            call('brush_stroke', document_id=doc, points=STROKE, size=SIZE, color=INK,
                 dynamics=dict(SCATTERED, seed=11))
            recorded = render(doc)
            call('stop_recording', name=f'dynamics-{kind}')
            call('undo', document_id=doc)
            call('run_action', document_id=doc, name=f'dynamics-{kind}')
            assert not changed(recorded, render(doc)), (kind, 'the replay differs')
            steps = [json.loads(p.read_text()) for p in (folder / 'app1' / 'actions').glob('*')
                     if p.is_file() and f'dynamics-{kind}' in p.read_text()]
            assert len(steps) == 1, (kind, len(steps))
            text = json.dumps(steps[0])
            assert '"native_stroke"' in text and '"dynamics"' not in text and '"seed"' not in text, text[:400]
            call('close_document', document_id=doc, discard_changes=True)
            print(f'PASS {kind} d5: the recorded dynamics stroke replays exactly; its step holds '
                  'native_stroke and no dynamics', flush=True)

        # ---- D9 geometry replay ----------------------------------------------------------------
        live = (2 * W, H * 3 // 2)
        factor = math.sqrt((live[0] / W) * (live[1] / H))
        for kind, label in (('rgb8', 'rgb8'), ('rgb16', 'rgb16'), ('rgb8', 'rgb8-shaped'),
                            ('rgb16', 'rgb16-shaped')):
            spec = KINDS[kind]
            shaped = label.endswith('shaped')
            doc = new(spec)
            call('start_recording', discard=True)
            # The shaped stroke's dabs each carry an angle and roundness (`shapes`), which a
            # relative replay keeps as recorded.
            call('brush_stroke', document_id=doc, points=STROKE, size=SIZE, color=INK,
                 dynamics=dict(SHAPED if shaped else SCATTERED, seed=13),
                 **({'roundness': 60, 'angle': 15} if shaped else {}))
            recorded = render(doc)
            call('stop_recording', name=f'geometry-{label}')
            call('close_document', document_id=doc, discard_changes=True)
            step = recorded_step(folder / 'app1' / 'actions', f'geometry-{label}')
            arguments = {k: v for k, v in step['arguments'].items() if k != 'document_id'}
            assert ('shapes' in arguments['native_stroke']) == shaped, (label, 'shapes')
            kind = label
            for mode in ('absolute', 'relative'):
                save = app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
                    'name': f'geometry-{kind}-{mode}', 'overwrite': True,
                    'recorded_canvas': {'width': W, 'height': H},
                    'steps': [{'tool': 'brush_stroke', 'arguments': arguments,
                               'geometry': {'mode': mode}}]}})
                assert not save.get('isError'), save
            doc = new(spec)
            call('run_action', document_id=doc, name=f'geometry-{kind}-absolute')
            assert render(doc) == recorded, (kind, 'the absolute replay differs')
            call('close_document', document_id=doc, discard_changes=True)
            doc = sized(live, spec)
            blank = render(doc)
            call('run_action', document_id=doc, name=f'geometry-{kind}-relative')
            played = render(doc)
            call('close_document', document_id=doc, discard_changes=True)
            dabs = decode_dabs(arguments['native_stroke'])
            mapped = [(x * live[0] / W, y * live[1] / H, min(max(d * factor, 1), 5000), o)
                      for x, y, d, o in dabs]
            # The step's `size` is a length and its points positions (ActionGeometry); the
            # dabs' centres map per axis unquantized and their diameters by the one factor.
            size = quantize(min(max(arguments['size'] * factor, 1), 5000), arguments['size'])
            points = [[quantize(x * live[0] / W, x), quantize(y * live[1] / H, y)]
                      for x, y in arguments['points']]
            direct = dict(arguments, points=points, size=size,
                          native_stroke=encode_dabs(arguments['native_stroke'], mapped))
            unscaled = dict(direct, native_stroke=encode_dabs(
                arguments['native_stroke'], [(x, y, d, o) for (x, y, _, o), (_, _, d, _)
                                             in zip(mapped, dabs)]))
            renders = []
            for each in (direct, unscaled):
                doc = sized(live, spec)
                call('brush_stroke', document_id=doc, **each)
                renders.append(render(doc))
                call('close_document', document_id=doc, discard_changes=True)
            assert changed(blank, played, live), (kind, 'the relative replay painted nothing')
            assert played == renders[0], (kind, 'the relative replay differs from the remapped '
                                          'direct call', len(changed(played, renders[0], live)))
            assert renders[0] != renders[1], (kind, 'unscaled diameters change nothing')
            print(f'PASS {kind} d9: a recorded dynamics stroke replays exactly under absolute '
                  f'geometry; relative on {live[0]}x{live[1]} paints the {len(dabs)} recorded dabs '
                  f'remapped (centres per axis, diameters x {factor:.4f}'
                  + (', shapes as recorded)' if shaped else ')'), flush=True)

        # ---- D10 tip shape ---------------------------------------------------------------------
        recordings = iter(range(1000))

        def recorded_native(spec, canvas, **arguments):
            """One brush_stroke on a fresh document, recorded: its step's native_stroke."""
            name = f'shape-{next(recordings)}'
            doc = sized(canvas, spec)
            call('start_recording', discard=True)
            call('brush_stroke', document_id=doc, **arguments)
            call('stop_recording', name=name)
            call('close_document', document_id=doc, discard_changes=True)
            return recorded_step(folder / 'app1' / 'actions', name)['arguments']['native_stroke']

        tip_r, m, p = 0.5, 0.5, 0.5
        for kind in ('rgb8', 'rgb16', 'rgb32'):
            spec = KINDS[kind]
            one = dict(points=[[30.3, 20.6]], size=30, color=INK, hardness=100, angle=25)
            _, blank, rounded = painted(spec, 'brush_stroke', pressures=[p], roundness=100 * tip_r,
                                        **one, dynamics={'roundness': {'control': 'pressure',
                                                                       'minimum': m}})
            _, _, plain = painted(spec, 'brush_stroke', stroke_version=1,
                                  roundness=100 * tip_r * (m + (1 - m) * p), **one)
            assert rounded == plain and changed(blank, plain), (kind, 'roundness on pressure',
                                                                len(changed(rounded, plain)))
            _, _, turned = painted(spec, 'brush_stroke', rotations=[30], roundness=50,
                                   **dict(one, angle=25),
                                   dynamics={'angle': {'control': 'rotation'}})
            _, _, at55 = painted(spec, 'brush_stroke', stroke_version=1, roundness=50,
                                 **dict(one, angle=55))
            _, _, at25 = painted(spec, 'brush_stroke', stroke_version=1, roundness=50, **one)
            assert turned == at55 and changed(at25, at55), (kind, 'the angle on rotation',
                                                            len(changed(turned, at55)))
            seeded = dict(points=STROKE, size=SIZE, color=INK, roundness=60,
                          dynamics=dict(SHAPED, seed=21))
            doc, _, first = painted(spec, 'brush_stroke', **seeded)
            call('close_document', document_id=doc, discard_changes=True)
            doc = new(spec)
            call('start_recording', discard=True)
            call('brush_stroke', document_id=doc, **seeded)
            again = render(doc)
            call('stop_recording', name=f'shaped-{kind}')
            call('undo', document_id=doc)
            call('run_action', document_id=doc, name=f'shaped-{kind}')
            replayed = render(doc)
            call('close_document', document_id=doc, discard_changes=True)
            native = recorded_step(folder / 'app1' / 'actions', f'shaped-{kind}')['arguments'][
                'native_stroke']
            shapes = decode_shapes(native)
            assert first == again == replayed, (kind, 'the seeded shaped stroke repeats and replays')
            assert len(shapes) == len(decode_dabs(native)) and len({a for a, _ in shapes}) > 5, \
                (kind, 'one recorded shape per dab', len(shapes))
            for d in call('list_documents')['documents']:
                call('close_document', document_id=d['id'], discard_changes=True)
            print(f'PASS {kind} d10: roundness on pressure paints the plain stroke at '
                  f'{100 * tip_r * (m + (1 - m) * p)} %; rotations [30] paint the plain stroke at '
                  f'angle 55; a seeded shaped stroke repeats, records {len(shapes)} shapes and '
                  'replays exactly', flush=True)

        # The coverage targets at 16 bits, whose shaped coverage runs as its own job under the
        # scratch `NativeBrushStroke.coverage` plans: Quick Mask (read back as u16 coverage) and a
        # hidden layer mask (its render), each with the roundness row above.
        one = dict(points=[[30.3, 20.6]], size=30, hardness=100, angle=25)
        shaped = dict(pressures=[p], roundness=100 * tip_r,
                      dynamics={'roundness': {'control': 'pressure', 'minimum': m}})
        oracle = dict(stroke_version=1, roundness=100 * tip_r * (m + (1 - m) * p))
        unshaped = dict(stroke_version=1, roundness=100 * tip_r)
        everywhere = [{'x': x, 'y': y} for y in range(H) for x in range(W)]

        def quick_mask(arguments):
            doc = new(KINDS['rgb16'])
            call('set_quick_mask', document_id=doc, active=True)
            call('quick_mask_stroke', document_id=doc, width=W, height=H, opacity=1, **one,
                 **arguments)
            call('set_quick_mask', document_id=doc, active=False)
            samples = call('get_selection', document_id=doc, points=everywhere)['samples']
            call('close_document', document_id=doc, discard_changes=True)
            return [s['coverage16'] for s in samples]

        def layer_mask(arguments):
            doc = new(KINDS['rgb16'])
            call('add_layer_mask', document_id=doc, layer=0, kind='hide_all')
            call('brush_stroke', document_id=doc, layer=0, target='mask', **one, **arguments)
            mask = render(doc, layer=0, channel='mask')
            call('close_document', document_id=doc, discard_changes=True)
            return mask

        for target, stroked in (('quick mask', quick_mask), ('layer mask', layer_mask)):
            got, want, full = stroked(shaped), stroked(oracle), stroked(unshaped)
            assert got == want, (target, 'the shaped stroke paints the plain one at its roundness')
            assert got != full, (target, 'the shaped stroke paints the tip at its own roundness')
        print('PASS rgb16 d10 coverage: quick_mask_stroke and a layer mask brush_stroke with the '
              'roundness on pressure paint the plain stroke at '
              f'{100 * tip_r * (m + (1 - m) * p)} %, not at {100 * tip_r} %', flush=True)

        # The heading rows: the angle on the direction of travel, no jitter, no scatter, one dab
        # per stamp; a tick is a dab past the count the same call records without airbrush.
        heading = dict(size=8, color=INK, hardness=100, angle=20, roundness=50,
                       dynamics={'angle': {'control': 'direction'}})
        rows = {
            'rest': ([[50, 10], [50, 90], [50, 90]], [0, 0.2, 0.7]),
            'L': ([[10, 10], [90, 10], [90, 90], [90, 90]], [0, 0.2, 0.4, 0.9]),
        }
        for row, (points, times) in rows.items():
            walked = recorded_native(KINDS['rgb8'], (100, 100), points=points, times=times,
                                     **heading)
            held = recorded_native(KINDS['rgb8'], (100, 100), points=points, times=times,
                                   airbrush=True, **heading)
            walk = len(decode_dabs(walked))
            angles = [a for a, _ in decode_shapes(held)]
            ticks = angles[walk:]
            assert decode_dabs(held)[:walk] == decode_dabs(walked), (row, 'the walk differs')
            assert len(ticks) == round(times[-1] / 0.05), (row, len(ticks))
            assert angles[0] == 20, (row, 'the first stamp lies at the tip angle', angles[0])
            if row == 'rest':
                want = [-70] * (len(angles) - 1)
                got = angles[1:]
            else:
                # A tick before 0.2 s lies on the first leg (+x, angle 20); every later one on
                # the second leg (down, angle 20 - 90) or at rest after it.
                want = [20 if 0.05 * (k + 1) < 0.2 - 1e-9 else -70 for k in range(len(ticks))]
                got = ticks
            assert all(abs(a - w) < 1e-9 for a, w in zip(got, want)), (row, got, want)
            print(f'PASS d10 heading {row}: {walk} walk dabs and {len(ticks)} airbrush ticks '
                  'record the angle of the travel they lie on', flush=True)

        # ---- D6 tools -------------------------------------------------------------------------
        spec = KINDS['rgb8']
        band = SIZE / 2 + 1
        erase = dict(points=STROKE, size=SIZE, hardness=100, dynamics=dict(seed=5, scatter={'amount': 2}))
        doc, blank, erased = painted(spec, 'eraser_stroke', **erase)
        call('close_document', document_id=doc, discard_changes=True)
        doc, _, again = painted(spec, 'eraser_stroke', **erase)
        call('close_document', document_id=doc, discard_changes=True)
        outside = [p for p in changed(blank, erased) if distance_to_stroke(*p) > band]
        assert erased == again and len(outside) > 10, ('eraser', len(outside))
        doc, blank, pencil = painted(spec, 'brush_stroke', points=STROKE, size=SIZE, color=INK,
                                     aliased=True, dynamics=dict(seed=5, scatter={'amount': 2, 'count': 2}))
        call('close_document', document_id=doc, discard_changes=True)
        colours = set(changed(blank, pencil).values())
        assert len(colours) == 1, ('the scattered Pencil', len(colours))
        grid = [{'x': x, 'y': y} for y in range(0, H, 2) for x in range(0, W, 2)]
        for depth in (8, 16):
            doc = new(dict(depth=depth, profile='srgb'))
            call('set_quick_mask', document_id=doc, active=True)
            call('quick_mask_stroke', document_id=doc, points=STROKE, size=SIZE, width=W, height=H,
                 opacity=1, hardness=100, dynamics=dict(seed=5, scatter={'amount': 2}))
            call('set_quick_mask', document_id=doc, active=False)
            samples = call('get_selection', document_id=doc, points=grid)['samples']
            covered = [s for s in samples if s['coverage'] > 0
                       and distance_to_stroke(s['x'], s['y']) > band]
            assert len(covered) > 3, (depth, 'quick mask scatter', len(covered))
            call('close_document', document_id=doc, discard_changes=True)
        print(f'PASS d6: eraser_stroke repeats and erases {len(outside)} px outside the band; the '
              'scattered Pencil paints one colour; quick_mask_stroke scatters at 8 and 16 bits',
              flush=True)

        # ---- D11 texture ----------------------------------------------------------------------
        def texture(depth=1, scale=1, pattern=None):
            return {'texture': {'pattern': pattern or TILE, 'scale': scale, 'depth': depth}}

        def alpha(doc, x, y):
            """The layer pixel's alpha in the document's own numbers."""
            reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=0)
            for key in ('center_rgba32', 'center_rgba_codes'):
                if reply.get(key) is not None:
                    return reply[key][3]
            raise AssertionError(('no readout', sorted(reply)))

        inside = interior(DAB, DAB_SIZE)
        assert len(inside) > 300, len(inside)
        # A block about the centre, which holds both texel values at both scales.
        block = [(x, y) for (x, y) in inside if abs(x - 30) <= 3 and abs(y - 22) <= 3]
        oracle_rows = {}
        for kind in ('rgb8', 'rgb16', 'rgb32'):
            for scale in (1, 2):
                for depth in (1, 0.5):
                    doc = call('new_document', width=W, height=H, background='transparent',
                               **KINDS[kind])['document']['id']
                    call('brush_stroke', document_id=doc, points=[list(DAB)], size=DAB_SIZE,
                         color=INK, hardness=100, dynamics=texture(depth, scale))
                    factors = set()
                    for (x, y) in block:
                        if scale == 2 and (x % 2 or y % 2):
                            continue
                        f = 1 - depth * (1 - tile_t(x, y, scale))
                        got = alpha(doc, x, y)
                        if kind == 'rgb32':
                            want, ok = f, abs(got - f) <= 2 / 65535
                        else:
                            top = 255 if kind == 'rgb8' else 65535
                            want = math.floor(top * f + 0.5)
                            ok = got == want
                        assert ok, (kind, scale, depth, (x, y), got, want)
                        factors.add(round(f, 6))
                    assert len(factors) == 2, (kind, scale, depth, 'both texel values', factors)
                    call('close_document', document_id=doc, discard_changes=True)
                    oracle_rows[f'{kind}_s{scale}_d{depth}'] = sorted(factors)
            print(f"PASS {kind} d11 oracle: a textured dab's interior alpha is 1 - depth * (1 - t) "
                  'of its own tile lookup at depths 1 and 0.5 and scales 1 and 2', flush=True)
        receipts['d11_oracle'] = oracle_rows

        line = dict(points=STROKE, size=SIZE, hardness=100)
        textured, plain = dict(dynamics=texture(1)), dict(stroke_version=1)

        def mode_document(kind):
            if kind == 'indexed':
                # A palette that holds the ink and white, so a held-back pixel shows.
                doc = new(KINDS['rgb8'])
                call('select_rect', document_id=doc, x=0, y=0, width=6, height=6)
                call('fill_selection', document_id=doc, color=INK)
                call('deselect', document_id=doc)
                call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither='none')
                return doc
            if kind == 'bitmap':
                doc = new(KINDS['gray8'])
                call('convert_mode', document_id=doc, mode='bitmap', method='threshold')
                return doc
            return new(dict(color_mode='lab', depth=8) if kind == 'lab8' else KINDS[kind])

        modes = {}
        for kind in ('gray8', 'cmyk8', 'lab8', 'indexed', 'bitmap'):
            colour = '#000000' if kind == 'bitmap' else INK
            renders, refusal = [], None
            for extra in (textured, textured, plain):
                doc = mode_document(kind)
                reply = app.client.rpc('tools/call', {'name': 'brush_stroke', 'arguments': dict(
                    line, document_id=doc, color=colour, **extra)})
                if reply.get('isError'):
                    refusal = next(c['text'] for c in reply['content'] if c['type'] == 'text')
                renders.append(render(doc))
                call('close_document', document_id=doc, discard_changes=True)
            if kind in ('indexed', 'bitmap') and refusal is not None:
                modes[kind] = f'refused: {refusal}'
                print(f'NOTE {kind} d11: the mode refuses the stroke: {refusal}', flush=True)
                continue
            assert refusal is None, (kind, refusal)
            assert renders[0] == renders[1], (kind, 'the textured stroke repeats')
            assert renders[0] != renders[2], (kind, 'the texture changed nothing')
            modes[kind] = len(changed(renders[2], renders[0]))
            print(f'PASS {kind} d11: a textured stroke repeats and differs from the plain one by '
                  f'{modes[kind]} px', flush=True)
        receipts['d11_modes'] = modes

        def target_read(depth, target, extra):
            doc = new(dict(depth=depth, profile='srgb'))
            if target == 'mask':
                call('add_layer_mask', document_id=doc, layer=0, kind='hide_all')
                call('brush_stroke', document_id=doc, layer=0, target='mask', **line, **extra)
                out = render(doc, layer=0, channel='mask')
            elif target == 'channel':
                call('add_channel', document_id=doc, name='Alpha 1')
                call('brush_stroke', document_id=doc, layer=0, target='channel:Alpha 1', **line,
                     **extra)
                out = render(doc, channel='Alpha 1')
            elif target == 'quick mask':
                call('set_quick_mask', document_id=doc, active=True)
                call('quick_mask_stroke', document_id=doc, width=W, height=H, opacity=1, **line,
                     **extra)
                call('set_quick_mask', document_id=doc, active=False)
                samples = call('get_selection', document_id=doc, points=everywhere)['samples']
                out = [(s.get('coverage'), s.get('coverage16')) for s in samples]
            else:
                call('eraser_stroke', document_id=doc, **line, **extra)
                out = render(doc)
            call('close_document', document_id=doc, discard_changes=True)
            return out

        for depth in (8, 16):
            for target in ('mask', 'channel', 'quick mask', 'eraser'):
                first, again, untextured = (target_read(depth, target, extra)
                                            for extra in (textured, textured, plain))
                assert first == again, (depth, target, 'the textured stroke repeats')
                assert first != untextured, (depth, target, 'the texture changed nothing')
            print(f'PASS {depth}-bit d11 targets: a layer mask, an alpha channel, '
                  'quick_mask_stroke and eraser_stroke each repeat textured and differ from the '
                  'plain stroke', flush=True)

        # A library pattern by name paints what its inline tile paints; recorded, the step holds
        # the tile, so the replay survives delete_pattern.
        library = folder / 'two-level.doctoredpatterns'
        library.write_text(json.dumps({'doctored_patterns': 1,
                                       'patterns': [dict(schema_version=1, **TILE)]}))
        assert call('import_patterns', path=str(library))['imported'] == ['Two Level']
        stroke = dict(line, color=INK)
        named = texture(0.8, 1, {'name': 'Two Level'})
        doc, _, by_name = painted(KINDS['rgb8'], 'brush_stroke', **stroke, dynamics=named)
        call('close_document', document_id=doc, discard_changes=True)
        doc, _, by_tile = painted(KINDS['rgb8'], 'brush_stroke', **stroke, dynamics=texture(0.8))
        call('close_document', document_id=doc, discard_changes=True)
        assert by_name == by_tile, ('the named pattern paints its tile', len(changed(by_name, by_tile)))
        doc = new(KINDS['rgb8'])
        call('start_recording', discard=True)
        call('brush_stroke', document_id=doc, **stroke, dynamics=named)
        recorded = render(doc)
        call('stop_recording', name='textured-named')
        call('delete_pattern', name='Two Level')
        call('undo', document_id=doc)
        call('run_action', document_id=doc, name='textured-named')
        assert render(doc) == recorded, 'the textured replay after delete_pattern differs'
        call('close_document', document_id=doc, discard_changes=True)
        step = recorded_step(folder / 'app1' / 'actions', 'textured-named')
        arguments = {k: v for k, v in step['arguments'].items() if k != 'document_id'}
        native = arguments['native_stroke']
        assert 'dynamics' not in arguments and native['texture']['pattern']['rgba'] == TILE['rgba'] \
            and native['texture']['depth'] == 0.8, native.get('texture')
        print('PASS d11 library: {name} paints its inline tile; the recorded step holds the tile '
              'and replays exactly after delete_pattern', flush=True)

        # Absolute and relative replay: the dabs move, the texture stays as recorded.
        for mode in ('absolute', 'relative'):
            save = app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
                'name': f'textured-{mode}', 'overwrite': True,
                'recorded_canvas': {'width': W, 'height': H},
                'steps': [{'tool': 'brush_stroke', 'arguments': arguments,
                           'geometry': {'mode': mode}}]}})
            assert not save.get('isError'), save
        doc = new(KINDS['rgb8'])
        call('run_action', document_id=doc, name='textured-absolute')
        assert render(doc) == recorded, 'the absolute textured replay differs'
        call('close_document', document_id=doc, discard_changes=True)
        doc = sized(live, KINDS['rgb8'])
        call('run_action', document_id=doc, name='textured-relative')
        played = render(doc)
        call('close_document', document_id=doc, discard_changes=True)
        dabs = decode_dabs(native)
        mapped = [(x * live[0] / W, y * live[1] / H, min(max(d * factor, 1), 5000), o)
                  for x, y, d, o in dabs]
        size = quantize(min(max(arguments['size'] * factor, 1), 5000), arguments['size'])
        points = [[quantize(x * live[0] / W, x), quantize(y * live[1] / H, y)]
                  for x, y in arguments['points']]
        direct = dict(arguments, points=points, size=size, native_stroke=encode_dabs(native, mapped))
        bare = dict(direct, native_stroke={k: v for k, v in direct['native_stroke'].items()
                                           if k != 'texture'})
        renders = []
        for each in (direct, bare):
            doc = sized(live, KINDS['rgb8'])
            call('brush_stroke', document_id=doc, **each)
            renders.append(render(doc))
            call('close_document', document_id=doc, discard_changes=True)
        assert played == renders[0], ('the relative textured replay differs from the remapped '
                                      'direct call', len(changed(played, renders[0], live)))
        assert renders[0] != renders[1], 'the replayed texture changed nothing'
        print(f'PASS d11 geometry: a recorded textured stroke replays exactly absolute; relative on '
              f'{live[0]}x{live[1]} it paints its {len(dabs)} dabs remapped with the texture as '
              'recorded', flush=True)

        # ---- D7 refusals ----------------------------------------------------------------------
        doc = new(KINDS['rgb8'])
        before = render(doc)
        base = dict(document_id=doc, points=STROKE, size=SIZE)
        d = {'scatter': {'amount': 1}}
        refusals = {
            'dodge': (call('dodge_burn', fail=True, dynamics=d, **base), "dodge_burn has none"),
            'clone': (call('clone_stamp', fail=True, dynamics=d, source=[10, 10], **base),
                      "clone_stamp has none"),
            'stroke_path': (call('stroke_path', fail=True, document_id=doc, dynamics=d),
                            "stroke_path has none"),
            'tilts_alone': (call('brush_stroke', fail=True, tilts=[[0, 0], [0, 0]], **base),
                            "pass dynamics with them"),
            'both': (call('brush_stroke', fail=True, pressure_size=True,
                          dynamics={'size': {'control': 'pressure'}}, **base),
                     "either dynamics.size.control or pressure_size"),
            'pixel': (call('brush_stroke', fail=True, document_id=doc, points=STROKE, size=1,
                           dynamics=d), "above 1 px"),
            'tilts_length': (call('brush_stroke', fail=True, tilts=[[0, 0]], dynamics=d, **base),
                             "one [x, y] per point"),
            'key': (call('brush_stroke', fail=True, dynamics={'spread': 1}, **base), "has no spread"),
            'seed': (call('brush_stroke', fail=True, dynamics={'seed': -1}, **base),
                     "dynamics.seed must be a whole number"),
            'curve': (call('brush_stroke', fail=True,
                           dynamics={'size': {'curve': [[0, 0], [0.5, 1]]}}, **base),
                      "dynamics.size.curve holds 2 to 16"),
            'count': (call('brush_stroke', fail=True, dynamics={'scatter': {'count': 17}}, **base),
                      "dynamics.scatter.count"),
            'rotations_alone': (call('brush_stroke', fail=True, rotations=[0, 0], **base),
                                "rotations are read only by dynamics.angle's rotation control"),
            'rotations_length': (call('brush_stroke', fail=True, rotations=[0], dynamics=d, **base),
                                 "rotations must contain one number per point"),
            'rotations_range': (call('brush_stroke', fail=True, rotations=[0, 400], dynamics=d,
                                     **base), "rotations must contain one number per point"),
            'rotations_tool': (call('dodge_burn', fail=True, rotations=[0, 0], **base),
                               "rotations is the Brush's"),
            'angle_pencil': (call('brush_stroke', fail=True, aliased=True,
                                  dynamics={'angle': {'control': 'rotation'}}, **base),
                             "the Pencil's tip (aliased) is round"),
            'roundness_pencil': (call('brush_stroke', fail=True, aliased=True,
                                      dynamics={'roundness': {'jitter': 0.5}}, **base),
                                 "the Pencil's tip (aliased) is round"),
            'angle_control': (call('brush_stroke', fail=True,
                                   dynamics={'angle': {'control': 'pressure'}}, **base),
                              'dynamics.angle.control must be'),
            'shape_key': (call('brush_stroke', fail=True, dynamics={'roundness': {'curve': []}},
                               **base), 'dynamics.roundness takes control, minimum, jitter'),
            'shapes_tool': (call('dodge_burn', fail=True, document_id=doc,
                                 native_stroke=SHAPED_NATIVE), "native_stroke shapes are the Brush's"),
            'shapes_length': (call('brush_stroke', fail=True, document_id=doc, color=INK,
                                   native_stroke=dict(SHAPED_NATIVE, shapes=SHAPED_NATIVE['shapes'][:12])),
                              'Native stroke shapes hold one angle'),
            'texture_pencil': (call('brush_stroke', fail=True, aliased=True,
                                    dynamics={'texture': {'pattern': TILE}}, **base),
                               "the Pencil's tip (aliased) is two-valued and takes no texture"),
            'texture_tool': (call('dodge_burn', fail=True, document_id=doc,
                                  native_stroke=TEXTURED_NATIVE),
                             "native_stroke texture is the Brush's"),
            'texture_pixel': (call('brush_stroke', fail=True, document_id=doc, color=INK,
                                   native_stroke=dict(TEXTURED_NATIVE, pixel=True)),
                              "A native stroke's texture holds a pattern"),
            'texture_native_name': (call('brush_stroke', fail=True, document_id=doc, color=INK,
                                         native_stroke=dict(TEXTURED_NATIVE, texture={
                                             'pattern': {'name': 'Two Level'}})),
                                    'names a library pattern'),
            'texture_name': (call('brush_stroke', fail=True,
                                  dynamics={'texture': {'pattern': {'name': 'No Such Pattern'}}},
                                  **base), 'No pattern named'),
            'texture_scale': (call('brush_stroke', fail=True,
                                   dynamics={'texture': {'pattern': TILE, 'scale': 20}}, **base),
                              'dynamics.texture.scale must be a number from 0.01 to 10'),
            'texture_depth': (call('brush_stroke', fail=True,
                                   dynamics={'texture': {'pattern': TILE, 'depth': 1.5}}, **base),
                              'dynamics.texture.depth must be a number from 0 to 1'),
            'texture_tile': (call('brush_stroke', fail=True, dynamics={'texture': {'pattern': {
                'width': 2, 'height': 2, 'rgba': 'AAAA'}}}, **base),
                             "dynamics.texture.pattern's custom tile is 1 to 512"),
        }
        for name, (reply, needle) in refusals.items():
            assert needle in reply, (name, reply)
        assert not changed(before, render(doc)), 'a refusal changed pixels'
        call('close_document', document_id=doc, discard_changes=True)
        receipts['refusals'] = {k: v[0] for k, v in refusals.items()}
        print(f'PASS d7: {len(refusals)} refusals by name change nothing', flush=True)

        # ---- D8 presets -----------------------------------------------------------------------
        tip = dict(hardness=100, flow=100, spacing=25, angle=0, roundness=100, airbrush=False)
        dyn = {'size': {'control': 'tilt', 'minimum': 0.2},
               'scatter': {'amount': 1, 'both_axes': True, 'count': 2},
               'angle': {'control': 'rotation', 'jitter': 0.25}}
        saved = call('save_brush_preset', name='Scatter Round', dynamics=dyn, **tip)['preset']
        assert saved['dynamics']['scatter']['count'] == 2 and saved['dynamics']['size']['control'] == 'tilt', saved
        assert saved['dynamics']['angle'] == {'control': 'rotation', 'jitter': 0.25} \
            and saved['dynamics']['roundness']['control'] == 'off', saved
        body = json.loads((presets_dir / next(p.name for p in presets_dir.glob('*.json'))).read_text())
        assert body['schema_version'] == 2 and 'dynamics' in body, body
        twin = call('save_brush_preset', fail=True, name='Twin', dynamics=dyn, **tip)
        assert '“Scatter Round” already holds these values' in twin, twin
        plain = call('save_brush_preset', fail=True, name='Plain', **tip)
        assert '“Hard Round” already holds these values' in plain, plain
        bad = call('save_brush_preset', fail=True, name='Bad', dynamics={'scatter': {'amount': 11}}, **tip)
        assert 'dynamics.scatter.amount' in bad, bad
        receipts['preset_refusals'] = {'twin': twin, 'plain': plain, 'bad': bad}
        # A textured preset by a library pattern's name: the row echoes the pattern without its
        # pixels and the file holds the tile, so the next launch needs no library.
        assert call('import_patterns', path=str(library))['imported'] == ['Two Level']
        textured_tip = dict(tip, hardness=80)
        textured_saved = call('save_brush_preset', name='Textured Round', **textured_tip, dynamics={
            'texture': {'pattern': {'name': 'Two Level'}, 'scale': 2, 'depth': 0.7}})['preset']
        assert textured_saved['dynamics']['texture'] == {
            'pattern': {'name': 'Two Level', 'width': TILE_W, 'height': TILE_H},
            'scale': 2, 'depth': 0.7}, textured_saved
        files = [json.loads(p.read_text()) for p in presets_dir.glob('*.json')]
        body = next(f for f in files if f['name'] == 'Textured Round')
        assert body['schema_version'] == 2 \
            and body['dynamics']['texture']['pattern']['rgba'] == TILE['rgba'], body
        print('PASS d8 texture: a textured preset saved by a library name echoes its pattern and '
              'stores the tile at schema 2', flush=True)

    with App(folder / 'app2', folder / 'recovery2', interval=600) as app:
        call = app.call
        rows = call('list_brush_presets')['presets']
        mine = {p['name']: p for p in rows if not p['builtin']}
        assert sorted(mine) == ['Scatter Round', 'Textured Round'] \
            and mine['Scatter Round']['dynamics'] == saved['dynamics'] \
            and mine['Textured Round']['dynamics'] == textured_saved['dynamics'], mine
        assert all(p['builtin'] for p in call('list_patterns')['patterns']), \
            'this launch has a pattern library'
        exported = folder / 'mine.doctoredbrushes'
        assert sorted(call('export_brush_presets', path=str(exported))['exported']) \
            == sorted(mine)
        for name in mine:
            call('delete_brush_preset', name=name)
        reply = call('import_brush_presets', path=str(exported))
        assert sorted(reply['imported']) == sorted(mine) and reply['skipped'] == [], reply
        assert call('list_brush_presets')['presets'] == rows, 'import does not restore the listing'
        old = folder / 'old.doctoredbrushes'
        old.write_text(json.dumps({'doctored_brush_presets': 1, 'presets': [dict(
            schema_version=1, name='Old', hardness=20, flow=90, spacing=40, angle=-30,
            roundness=55, airbrush=False)]}))
        reply = call('import_brush_presets', path=str(old))
        assert reply['imported'] == ['Old'] and reply['skipped'] == [], reply
        print('PASS d8: a preset with dynamics saves at schema 2, refuses its twin, lists after a '
              'relaunch, exports, deletes and imports back; a version-1 file imports', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all; receipts in {output}', flush=True)


if __name__ == '__main__':
    main()
