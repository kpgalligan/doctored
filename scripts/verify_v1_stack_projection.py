#!/usr/bin/env python3
"""MCP drive for `auto_align_layers` with `projection`
"cylindrical" and "spherical" (Edit > Auto-Align Layers... > Cylindrical,
Spherical), its lens corrections, and Collage and Reposition with Geometric
Distortion (PLANAR below), end to end, on take2's geometry
(`~/devel/doctored-fixtures/photoshop/panorama-564/take2`: Cylindrical
974 x 399, Spherical 974 x 385, the layers at x 0, 209 and 419) with the yaw
step halved. Take2's 20-degree steps shift a frame about 36% of its width,
past `AutoAlign.register`'s reach (about 10-15 degrees at f 600, by a
Vision probe), so its own rows are checked through the core alone
(`core/tests/geometry_align_projection.rs`,
`take2_rows_from_the_takes_focal_and_yaw`) and the end-to-end check of the
20-degree frames waits on Auto-Align's extended registration reach
(`scripts/verify_v1_stack_reach.py`).

The script owns its app instance (`verify_v1_stack_align.App`: the Debug app or
`DR_VERIFY_APP`, its own scratch socket, `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB pin, private `DR_*_DIR`s) and terminates only
that process. Every expected number is computed here, never read back.

THE FIXTURE is take2's `rot` geometry at half the step: three 600 x 400
pinhole views, focal F = 600 px, yawed -10, 0 and +10 degrees (`left`, `centre`, `right`, stacked
bottom first, so `left` is the reference). Each samples a CONTINUOUS cylinder
picture: grey value noise with lattice cells 48, 24 and 12 px, weights 1, 1/2
and 1/3, smoothstep interpolation, contrast 1.8 about mid-grey (the lattice is
`verify_v1_stack_align.lattice`). Pixel (x, y), with xc = x - 299.5 and
yc = y - 199.5, takes the picture at u = F (yaw + atan2(xc, F)),
v = F yc / hypot(xc, F) (`panorama_fill_take.py`'s formula without its
texture), written as an 8-bit PNG.

BOUNDS, fixed before measuring and never widened:
- `focal` within 1% of 600;
- for the layers yawed 10 and 20 degrees from the reference, the reply's
  `rotation` R takes (sin psi, 0, cos psi) to within 0.5 degree of (0, 0, 1);
- the canvas height within +-2 of 399 (Cylindrical) or 385 (Spherical), take2's
  heights, which the yaw does not change; its width within +-10 of the model's
  F (2 atan(300 / F) + 20 degrees) = 766, as take2's 974 is F (2 atan(300 / F)
  + 40 degrees) = 975; each layer's content x within +-10 of F times its yaw
  from the reference: 0, 105 and 209;
- at 8 and 16 bits, per layer: mean |layer - picture| <= 3 codes (of 255) over
  the pixels with alpha 255 whose source point lies 4 px or more inside the
  frame, the source point found by this file's own inverse map from the reply's
  focal and rotation, the picture evaluated there from the TRUE geometry; the
  map's origin is the host's (the reference's axis on the
  pixel centre (300.5, 200.5)) and its principal point the frame's centre
  (300, 200), both moved by the canvas origin;
- one history step; one `undo` restores the canvas and every rectangle.

CASES: Cylindrical and Spherical, each at 8 bits, 16 bits (the stack, then
`convert_precision`) and 32-bit float (`paths` = [float_11f/scene.tiff, left,
centre, right], aligning layers 1-3; geometry and history only). REFUSED with
no history step: identical layers (`AutoAlign.focalRefusal`), a
position-locked reference, and `sampler: "lanczos"`.

VIGNETTE REMOVAL. The bound form for a falloff
V = 1 + a rho^2 applied in sRGB linear light before encoding (rho the pixel
centre's distance from (300, 200) over hypot(299.5, 199.5), the farthest
pixel centre: the core's model) is: gain_true = the mean of V over the
frame's pixel centres, 1 + a x ((600^2 - 1)/12 + (400^2 - 1)/12) /
hypot(299.5, 199.5)^2, and the BOUNDS, fixed before measuring and never
widened:
- `projection: "perspective", vignette: true` at 8, 16 and float: the reply's
  `surface` "spherical", |vignette.coefficient - a| <= 0.03,
  vignette.layers the three layers (reference first), the canvas height
  within +-2 of 385 and width within +-10 of 766; at 8 and 16 bits, per
  layer, mean |layer - picture x gain_true| <= 3 codes, the picture's linear
  light scaled by gain_true and encoded, over the pixels the cases above
  compare (Spherical's inverse map from the reply's focal and rotations);
  one history step; one `undo` restores the canvas and every rectangle.
- `projection: "spherical", vignette: true` at 8 bits and
  `projection: "auto", vignette: true` at 8 bits: the perspective case's
  bounds (`surface` "spherical").
- ACTION: `start_recording`, the perspective + vignette call at 8 bits,
  `stop_recording` with a name: the recorded `auto_align_layers` step holds
  `vignette: true`; `run_action` on a fresh stack is one history step, with
  the same canvas as the recorded call.
The SUCCESS CASES run at a = 0, on the views above (gain_true 1): they
prove the whole path end to end (the surface switch, the fit, the reply,
the history step, the action) on frames registration reaches. The cases at
a = -0.4 (`vig-<name>.png`) wait on the extended registration reach: with any
shared falloff (a = -0.02 already) Vision's translational seed on this value
noise returns dy -291 against a truth of about 0, and no layer registers,
with or without `vignette`; photographs register at 2-10 % falloff as at
none, but at +-10 degrees of yaw they register erratically even at a = 0. A
pure shift of a photograph registers, but has no lens for the sphere (the
focal refusal) and Reposition ignores Vignette Removal, so the falloff's
removal is judged in the core (`core/tests/geometry_align_vignette.rs`: a
-0.4000 at 8, 16 and float, take2's rings within 0.014). The `vig-` views
stay for the refusals.
- REFUSED with no history step: `sampler: "lanczos"` with perspective and
  vignette ("sampler must be bicubic with vignette true.") on the `vig-`
  views, and `auto` with vignette on the identical-layers pair
  (`focalRefusal`'s vignette sentence, "Turn off Vignette Removal"). Collage
  and Reposition no longer refuse it: they ignore Vignette Removal
  (AVAILABILITY below).

GEOMETRIC DISTORTION and take3's lens availability
(`scripts/photoshop-probe/auto_align_lens_take.py`: vignette applies on Auto,
Perspective, Cylindrical and Spherical and is ignored on Collage and
Reposition; distortion applies on all six, Collage and Reposition correcting
each layer within its own frame; PLANAR below). BOUNDS, fixed
before measuring and never widened:
- `projection: "perspective", distortion: true` at 8, 16 and float on the
  plain views above (no lens: k = 0): the reply's `surface` "spherical",
  |distortion.coefficient| <= 0.005, distortion.layers the three layers
  (reference first), distortion.matches > 0, the canvas height within +-2 of
  385 and width within +-10 of 766; at 8 and 16 bits, per layer, mean
  |layer - picture| <= 3 codes over the pixels the Spherical case compares
  (its inverse map from the reply's focal and rotations; the TRUE geometry,
  no lens); one history step; one `undo` restores the canvas and every
  rectangle.
- `projection: "auto", distortion: true` at 8 bits: the same bounds.
- `projection: "spherical", vignette: true, distortion: true` at 8 bits: the
  same bounds, and the reply's `vignette` (coefficient within 0.03 of 0, the
  three layers) beside its `distortion`.
- AVAILABILITY: `collage` and `reposition` with `vignette: true` at 8 bits on
  the plain views: the reply has no `vignette` and no `surface`, and its
  canvas, its aligned layers and their homographies (within 1e-6) equal the
  same call without `vignette`; one history step each.
- ACTION: `start_recording`, the perspective + distortion call at 8 bits,
  `stop_recording`: the recorded step holds `distortion: true`; `run_action`
  on a fresh stack is one history step with the recorded call's canvas.
- REAL PHOTOGRAPH (SKIP when `~/devel/doctored-fixtures/phase6/photos` is
  absent): the sky (dy -100, then +100), product (dy 0) and glass (dy 0)
  photographs (`sips -s format png`, a centred 800 x 640 crop) seen as three
  600 x 400 pinhole views at F 600 as the diag-8 probe builds
  them (1 px per cylinder unit, grey, the crop's centre shifted dy), yawed
  -10/0/+10 degrees for every set, then -5/0/+5. The first set whose lens-free
  views register all three layers with `spherical` is rendered again with
  k = -0.05 in the core's convention (the frame pixel p shows the view at
  the u with p = c + R q (1 + k |q|^2), c (300, 200), R 300) and run with
  `perspective` and `distortion: true`: |distortion.coefficient + 0.05| <=
  0.01, three distortion.layers, one history step. If no set registers, or
  the distorted set does not register all three layers, the case prints
  `PENDING real-photo distortion: <reason>`, declared not a failure.
- REFUSED with no history step: `sampler: "lanczos"` with perspective and
  distortion, and with reposition and distortion ("sampler must be bicubic
  with distortion true."), `auto` with distortion on the identical-layers
  pair ("Turn off Geometric Distortion to align them with Perspective."),
  `reposition` with distortion on a position-locked reference ("cannot move
  because of a position lock. Geometric Distortion corrects every layer, the
  reference too: unlock it first."), and `reposition` with distortion on the
  identical-layers pair (`AutoAlign.planarStillMessage`, "Geometric Distortion
  needs a layer that moves against the reference...").

PLANAR: Collage and Reposition with Geometric Distortion,
on the committed CC0 photograph
`core/tests/fixtures/registration_photo/sky.jpg` (2048 x 1366; source in the
folder's `provenance.json`), converted to PNG by `/usr/bin/sips` into
`<output>/planar/sky.png` (`planar_photo`; a failed conversion fails the run,
never skipped) and read by `Grey` through `decode_png`. Three 600 x 400 grey
views of it, the bottom one the reference, at the photo offsets
O0 = (440, 60), O0 + (55, 12) and O0 + (-50, -14); the COLLAGE views also
turn the second and third by +0.5 and -0.5 degrees about the view centre.
Every sample, about x 355...1130, y 10...510, lies in the cloud field or the
sun, clear of the rooftops and of the sparse dark sky left of x = 350.
Registration runs on the DISTORTED frames, and the lens leaves a residual
the agreement score sees: an earlier probe found O0 + (110, -14) and
+-1.5 degrees unregistered under k = -0.05 while the same views without the
lens register, so the offsets stay small. Measured on this texture (the
four PLANAR cases, Debug app): coefficient -0.04978 on Reposition at 8, 16
and float and -0.05029 on Collage; mean |delta| 0.587 / 0.603 / 0.591 codes
on Reposition 8 and 16 and 0.593 / 0.633 / 0.614 on Collage; content sizes
within 1 of the oracle. The bounds below were declared on the earlier
texture and are unchanged. Each view is distorted with k = -0.05 in the
core's convention by `unmap` (the frame pixel p shows the view's u with
p = c + R q (1 + k |q|^2), c (300, 200), R 300). The analytic map A of a
layer takes its corrected point u to the reference's, r = d + c + S (u - c)
(d its offset from O0, S its turn). Cases: `reposition` + distortion at 8,
16 and float, `collage` + distortion at 8. BOUNDS, fixed before measuring
and never widened:
- |distortion.coefficient + 0.05| <= 0.01;
- distortion.layers the three layers, unregistered [];
- each aligned row's method `translation` or `similarity`;
- no `surface` and no `focal` in the reply;
- each layer's content size within +-1 of this file's analytic oracle (the
  destination pixel centres whose A^-1 lands in the frame's pixel-centre rect
  [0.5, 599.5] x [0.5, 399.5], its lens point too): 600 x 400 on Reposition;
- at 8 and 16 bits, per layer, mean |layer - photo at O0 + p| <= 3 codes over
  the pixels with alpha 255 whose corrected point A^-1 p (and its lens point)
  lies 4 px or more inside the frame, p in the old canvas;
- one history step; one `undo` restores the canvas and every rectangle.

Usage: python3 scripts/verify_v1_stack_projection.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed.
"""
import argparse
import base64
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
from verify_v1_stack_align import (  # noqa: E402
    App, FLOAT_FIXTURE, Skipped, close, cursor, lattice, load, rect, rows)
from verify_phase1 import png  # noqa: E402

SIZE = (600, 400)
FOCAL = 600.0
YAWS = (-10.0, 0.0, 10.0)
NAMES = ('left', 'centre', 'right')
CELLS = (48, 24, 12)
WEIGHTS = (1.0, 1 / 2, 1 / 3)
CONTRAST = 1.8
# The model's rows: take2's heights (the yaw leaves them), and the width and
# the layers' x from the yaws: F (2 atan(300 / F) + span) and F (yaw - yaw0).
WIDTH = round(FOCAL * (2 * math.atan(SIZE[0] / 2 / FOCAL) + math.radians(YAWS[-1] - YAWS[0])))
HEIGHT = {'cylindrical': 399, 'spherical': 385}
LAYER_X = tuple(round(FOCAL * math.radians(yaw - YAWS[0])) for yaw in YAWS)
INSIDE = 4.0


class Cylinder:
    """The value noise over the unrolled cylinder, its lattice cached over
    the rectangle the views read."""

    def __init__(self, u0=-760, v0=-320, u1=760, v1=320):
        self.octaves = []
        for octave, cell in enumerate(CELLS):
            ox, oy = math.floor(u0 / cell), math.floor(v0 / cell)
            nx, ny = math.floor(u1 / cell) + 2 - ox, math.floor(v1 / cell) + 2 - oy
            table = [[lattice(ox + i, oy + j, octave) for i in range(nx)] for j in range(ny)]
            self.octaves.append((cell, WEIGHTS[octave], table, ox, oy))
        self.total = sum(WEIGHTS)

    def value(self, u, v):
        total = 0.0
        floor = math.floor
        for cell, weight, table, ox, oy in self.octaves:
            fx, fy = u / cell, v / cell
            ix, iy = floor(fx), floor(fy)
            tx, ty = fx - ix, fy - iy
            ux, uy = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
            row, below = table[iy - oy], table[iy - oy + 1]
            i = ix - ox
            top = row[i] + (row[i + 1] - row[i]) * ux
            bottom = below[i] + (below[i + 1] - below[i]) * ux
            total += weight * (top + (bottom - top) * uy)
        g = (total / self.total - 0.5) * CONTRAST + 0.5
        return 0.0 if g < 0 else 1.0 if g > 1 else g

    def view(self, yaw, fx, fy):
        """The view yawed `yaw` degrees at the frame's continuous point (fx, fy)."""
        xc, yc = fx - SIZE[0] / 2, fy - SIZE[1] / 2
        theta = math.radians(yaw) + math.atan2(xc, FOCAL)
        return self.value(FOCAL * theta, FOCAL * yc / math.hypot(xc, FOCAL))


def author(folder, picture):
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for name, yaw in zip(NAMES, YAWS):
        path = folder / f'{name}.png'
        if not path.is_file():
            out = bytearray(b'\xff' * (SIZE[0] * SIZE[1] * 4))
            for y in range(SIZE[1]):
                for x in range(SIZE[0]):
                    grey = int(picture.view(yaw, x + 0.5, y + 0.5) * 255 + 0.5)
                    p = (y * SIZE[0] + x) * 4
                    out[p] = out[p + 1] = out[p + 2] = grey
            staged = path.with_suffix('.partial')
            png(staged, SIZE[0], SIZE[1], bytes(out))
            staged.rename(path)
        paths.append(path)
    return paths


def decode_png(data):
    """An 8-bit grey, grey-alpha, RGB or RGBA PNG as (width, height, RGBA rows)."""
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
    assert bits == 8 and colour in (0, 2, 4, 6) and interlace == 0, header
    n = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    raw, stride = zlib.decompress(packed), width * n
    out, previous, position = [], bytearray(stride), 0
    for _ in range(height):
        kind = raw[position]
        line = bytearray(raw[position + 1:position + 1 + stride])
        position += 1 + stride
        for i in range(stride):
            left = line[i - n] if i >= n else 0
            up = previous[i]
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + ((left + up) >> 1)) & 255
            elif kind == 4:
                corner = previous[i - n] if i >= n else 0
                estimate = left + up - corner
                a, b, c = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                line[i] = (line[i] + (left if a <= b and a <= c else up if b <= c else corner)) & 255
        previous = line
        row = []
        for x in range(width):
            p = line[x * n:(x + 1) * n]
            if n == 1:
                row.append((p[0], p[0], p[0], 255))
            elif n == 2:
                row.append((p[0], p[0], p[0], p[1]))
            elif n == 3:
                row.append((p[0], p[1], p[2], 255))
            else:
                row.append(tuple(p))
        out.append(row)
    return width, height, out


def layer_pixels(app, document, layer):
    """The layer alone at scale 1, and the canvas point its top-left pixel
    sits at: `render` answers a byte layer's own buffer (at its offset) and a
    16-bit or float layer's canvas-sized display (at the origin)."""
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document, 'layer': layer, 'scale': 1}})
    assert not response.get('isError'), response
    image = next(item['data'] for item in response['content'] if item['type'] == 'image')
    width, height, pixels = decode_png(base64.b64decode(image))
    summary = app.call('get_document', document_id=document)
    if (width, height) == (summary['width'], summary['height']):
        return width, height, pixels, (0, 0)
    row = summary['layers'][layer]
    assert (width, height) == (row['width'], row['height']), ('render size', width, height, row)
    return width, height, pixels, (row['offset_x'], row['offset_y'])


def apply(r, d):
    return [r[0] * d[0] + r[1] * d[1] + r[2] * d[2],
            r[3] * d[0] + r[4] * d[1] + r[5] * d[2],
            r[6] * d[0] + r[7] * d[1] + r[8] * d[2]]


def source_point(projection, focal, rotation, origin, centre, p):
    """This file's inverse map: a canvas point of the projection to the canvas
    point of the layer whose principal point is `centre`, or None."""
    theta = (p[0] - origin[0]) / focal
    t = (p[1] - origin[1]) / focal
    if abs(theta) > math.pi:
        return None
    if projection == 'spherical':
        if abs(t) > math.pi / 2:
            return None
        d = (math.sin(theta) * math.cos(t), math.sin(t), math.cos(theta) * math.cos(t))
    else:
        d = (math.sin(theta), t, math.cos(theta))
    r = apply(rotation, d)
    if r[2] <= 1e-9:
        return None
    return centre[0] + focal * r[0] / r[2], centre[1] + focal * r[1] / r[2]


def pixel_error(app, document, layer, projection, focal, rotation, shift, yaw, picture,
                expected=None):
    """Mean |layer - picture| in codes over the compared pixels, and their count;
    `expected(yaw, fx, fy)` replaces `picture.view` when given."""
    expected = expected or picture.view
    width, height, pixels, (ox, oy) = layer_pixels(app, document, layer)
    # Where the reference's axis and this layer's principal point land in the
    # new canvas: the host's axis on the pixel centre (300.5, 200.5) and the
    # frame's centre (300, 200), moved by the canvas origin.
    origin = (SIZE[0] / 2 + 0.5 + shift[0], SIZE[1] / 2 + 0.5 + shift[1])
    centre = (SIZE[0] / 2 + shift[0], SIZE[1] / 2 + shift[1])
    total, count = 0.0, 0
    for y in range(height):
        row = pixels[y]
        for x in range(width):
            got = row[x]
            if got[3] != 255:
                continue
            q = source_point(projection, focal, rotation, origin, centre,
                             (ox + x + 0.5, oy + y + 0.5))
            if q is None:
                continue
            fx, fy = q[0] - shift[0], q[1] - shift[1]
            if not (INSIDE <= fx <= SIZE[0] - INSIDE and INSIDE <= fy <= SIZE[1] - INSIDE):
                continue
            total += abs(got[0] - expected(yaw, fx, fy) * 255)
            count += 1
    assert count > SIZE[0] * SIZE[1] // 2, ('too few pixels compared', layer, count)
    return total / count, count


def stack(app, paths, depth):
    reply = load(app, [FLOAT_FIXTURE] + paths if depth == 'float' else paths)
    document = reply['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
        assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    return document


def projection_case(app, paths, picture, projection, depth):
    document = stack(app, paths, depth)
    first = 1 if depth == 'float' else 0
    layers = [first, first + 1, first + 2]
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    reply = app.call('auto_align_layers', document_id=document, layers=layers,
                     projection=projection)
    detail = dict(focal=reply.get('focal'))
    assert reply['changed'] is True and reply['projection'] == projection, reply
    assert reply['reference'] == first, reply
    assert reply['unchanged'] == [] and reply['unregistered'] == [], reply
    assert [row['layer'] for row in reply['aligned']] == layers[1:], reply['aligned']
    focal = reply['focal']
    assert abs(focal - FOCAL) <= 0.01 * FOCAL, ('focal', focal)
    for row, psi in zip(reply['aligned'], [yaw - YAWS[0] for yaw in YAWS[1:]]):
        r = row['rotation']
        assert len(r) == 9, row
        axis = apply(r, (math.sin(math.radians(psi)), 0.0, math.cos(math.radians(psi))))
        angle = math.degrees(math.acos(max(-1.0, min(1.0, axis[2] / math.hypot(*axis)))))
        detail[f'axis_degrees_{psi:g}'] = round(angle, 4)
        assert angle <= 0.5, ('rotation', psi, angle, r)
    summary = reply['document']
    canvas = reply['canvas']
    assert (canvas['width'], canvas['height']) == (summary['width'], summary['height']), reply
    want_w, want_h = WIDTH, HEIGHT[projection]
    detail['canvas'] = (summary['width'], summary['height'])
    assert abs(summary['height'] - want_h) <= 2, ('canvas height', summary['height'], want_h)
    assert abs(summary['width'] - want_w) <= 10, ('canvas width', summary['width'], want_w)
    shift = (canvas['origin']['x'], canvas['origin']['y'])
    listed = rows(app, document)
    xs = [listed[index]['content_x'] for index in layers]
    detail['layer_x'] = xs
    for got, want in zip(xs, LAYER_X):
        assert abs(got - want) <= 10, ('layer x', xs)
    assert cursor(app, document) == start + 1, 'one history step'
    if depth != 'float':
        rotations = [[1, 0, 0, 0, 1, 0, 0, 0, 1]] + [row['rotation'] for row in reply['aligned']]
        errors = []
        for index, rotation, yaw in zip(layers, rotations, YAWS):
            mean, count = pixel_error(app, document, index, projection, focal, rotation, shift,
                                      yaw, picture)
            errors.append(round(mean, 3))
            assert mean <= 3.0, ('mean |delta|', index, mean, count)
        detail['mean_codes'] = errors
    app.call('undo', document_id=document)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == SIZE, summary
    assert [rect(row) for row in rows(app, document)] == before, 'undo restores every rect'
    close(app, document)
    return detail


def refusals(app, paths):
    seen = {}
    twin = load(app, [paths[1], paths[1]])['document']['id']
    start = cursor(app, twin)
    seen['identical'] = app.refused(
        'auto_align_layers', 'The layers do not turn about one point', 'onto a cylinder',
        document_id=twin, layers=[0, 1], projection='cylindrical')
    assert cursor(app, twin) == start
    close(app, twin)
    document = load(app, paths)['document']['id']
    start = cursor(app, document)
    seen['lanczos'] = app.refused(
        'auto_align_layers', 'sampler must be bicubic with projection cylindrical or spherical.',
        document_id=document, layers=[0, 1, 2], projection='spherical', sampler='lanczos')
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    app.call('set_layer_lock', document_id=document, layer=0, locks=['position'])
    start = cursor(app, document)
    seen['locked reference'] = app.refused(
        'auto_align_layers', 'cannot move because of a position lock',
        document_id=document, layers=[0, 1, 2], projection='cylindrical')
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    close(app, document)
    return seen


# Vignette Removal: the `vig-` views' falloff (the refusals) and the
# success cases' (the views registration reaches; docstring).
VIGNETTE = -0.4
SUCCESS_VIGNETTE = 0.0
VIGNETTE_RADIUS = math.hypot(SIZE[0] / 2 - 0.5, SIZE[1] / 2 - 0.5)
SPHERICAL_HEIGHT = HEIGHT['spherical']


def gain_true(a):
    """The mean of V = 1 + a rho^2 over the frame's pixel centres."""
    return 1 + a * ((SIZE[0] ** 2 - 1) / 12 + (SIZE[1] ** 2 - 1) / 12) / VIGNETTE_RADIUS ** 2


def decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def encode(v):
    v = min(max(v, 0.0), 1.0)
    return v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def falloff(x, y):
    """V at the frame pixel (x, y)'s centre."""
    rho2 = ((x + 0.5 - SIZE[0] / 2) ** 2 + (y + 0.5 - SIZE[1] / 2) ** 2) / VIGNETTE_RADIUS ** 2
    return 1 + VIGNETTE * rho2


def author_vignetted(folder, picture):
    """The views with V applied in sRGB linear light before encoding."""
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for name, yaw in zip(NAMES, YAWS):
        path = folder / f'vig-{name}.png'
        if not path.is_file():
            out = bytearray(b'\xff' * (SIZE[0] * SIZE[1] * 4))
            for y in range(SIZE[1]):
                for x in range(SIZE[0]):
                    grey = picture.view(yaw, x + 0.5, y + 0.5)
                    code = int(encode(decode(grey) * falloff(x, y)) * 255 + 0.5)
                    p = (y * SIZE[0] + x) * 4
                    out[p] = out[p + 1] = out[p + 2] = code
            staged = path.with_suffix('.partial')
            png(staged, SIZE[0], SIZE[1], bytes(out))
            staged.rename(path)
        paths.append(path)
    return paths


def vignette_case(app, paths, picture, projection, depth, a):
    """`projection` with `vignette: true` on `paths`, views carrying the falloff `a`
    (docstring)."""
    gain = gain_true(a)
    document = stack(app, paths, depth)
    first = 1 if depth == 'float' else 0
    layers = [first, first + 1, first + 2]
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    reply = app.call('auto_align_layers', document_id=document, layers=layers,
                     projection=projection, vignette=True)
    fit = reply.get('vignette') or {}
    detail = dict(focal=reply.get('focal'), surface=reply.get('surface'), vignette=fit,
                  gain_true=round(gain, 4))
    assert reply['changed'] is True and reply['projection'] == projection, reply
    assert reply.get('surface') == 'spherical', ('surface', reply.get('surface'))
    assert reply['unregistered'] == [], reply
    assert fit.get('layers') == layers, ('vignette layers', fit)
    coefficient = fit['coefficient']
    assert abs(coefficient - a) <= 0.03, ('coefficient', coefficient)
    summary = reply['document']
    canvas = reply['canvas']
    detail['canvas'] = (summary['width'], summary['height'])
    assert abs(summary['height'] - SPHERICAL_HEIGHT) <= 2, ('canvas height', summary['height'])
    assert abs(summary['width'] - WIDTH) <= 10, ('canvas width', summary['width'], WIDTH)
    assert cursor(app, document) == start + 1, 'one history step'
    if depth != 'float':
        shift = (canvas['origin']['x'], canvas['origin']['y'])
        rotations = [[1, 0, 0, 0, 1, 0, 0, 0, 1]] + [row['rotation'] for row in reply['aligned']]

        def expected(yaw, fx, fy):
            return encode(decode(picture.view(yaw, fx, fy)) * gain)

        errors = []
        for index, rotation, yaw in zip(layers, rotations, YAWS):
            mean, count = pixel_error(app, document, index, 'spherical', reply['focal'],
                                      rotation, shift, yaw, picture, expected)
            errors.append(round(mean, 3))
            assert mean <= 3.0, ('mean |delta|', index, mean, count)
        detail['mean_codes'] = errors
    app.call('undo', document_id=document)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == SIZE, summary
    assert [rect(row) for row in rows(app, document)] == before, 'undo restores every rect'
    close(app, document)
    return detail


def vignette_refusals(app, paths, vignetted):
    seen = {}
    document = load(app, vignetted)['document']['id']
    start = cursor(app, document)
    seen['lanczos'] = app.refused(
        'auto_align_layers', 'sampler must be bicubic with vignette true.',
        document_id=document, layers=[0, 1, 2], projection='perspective', vignette=True,
        sampler='lanczos')
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    close(app, document)
    twin = load(app, [paths[1], paths[1]])['document']['id']
    start = cursor(app, twin)
    seen['identical'] = app.refused(
        'auto_align_layers', 'onto a sphere. Turn off Vignette Removal to align them with '
        'Perspective. Nothing was changed.', document_id=twin, layers=[0, 1],
        projection='auto', vignette=True)
    assert cursor(app, twin) == start, 'a refusal leaves no history step'
    close(app, twin)
    return seen


def vignette_action(app, vignetted):
    recorded = load(app, vignetted)['document']['id']
    app.call('start_recording', discard=True)
    reply = app.call('auto_align_layers', document_id=recorded, layers=[0, 1, 2],
                     projection='perspective', vignette=True)
    stopped = app.call('stop_recording', name='v564-vignette-removal')
    steps = [step for step in stopped['steps'] if step.get('tool') == 'auto_align_layers']
    assert len(steps) == 1 and steps[0]['arguments'].get('vignette') is True, stopped['steps']
    replayed = load(app, vignetted)['document']['id']
    start = cursor(app, replayed)
    app.call('run_action', document_id=replayed, name='v564-vignette-removal')
    assert cursor(app, replayed) == start + 1, 'the replay is one history step'
    summary = app.call('get_document', document_id=replayed)
    want = (reply['document']['width'], reply['document']['height'])
    assert (summary['width'], summary['height']) == want, (summary['width'], want)
    close(app, recorded)
    close(app, replayed)
    return dict(step=steps[0], canvas=want)


# Geometric Distortion (docstring).
DISTORTION_ZERO = 0.005


def distortion_case(app, paths, picture, projection, depth, vignette=False):
    """`projection` with `distortion: true` (and `vignette`) on the lens-free views."""
    document = stack(app, paths, depth)
    first = 1 if depth == 'float' else 0
    layers = [first, first + 1, first + 2]
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    arguments = dict(document_id=document, layers=layers, projection=projection,
                     distortion=True)
    if vignette:
        arguments['vignette'] = True
    reply = app.call('auto_align_layers', **arguments)
    fit = reply.get('distortion') or {}
    detail = dict(focal=reply.get('focal'), surface=reply.get('surface'), distortion=fit)
    assert reply['changed'] is True and reply['projection'] == projection, reply
    assert reply.get('surface') == 'spherical', ('surface', reply.get('surface'))
    assert reply['unregistered'] == [], reply
    assert fit.get('layers') == layers, ('distortion layers', fit)
    assert fit.get('matches', 0) > 0, ('distortion matches', fit)
    assert abs(fit['coefficient']) <= DISTORTION_ZERO, ('coefficient', fit['coefficient'])
    if vignette:
        lens = reply.get('vignette') or {}
        detail['vignette'] = lens
        assert lens.get('layers') == layers, ('vignette layers', lens)
        assert abs(lens['coefficient']) <= 0.03, ('vignette coefficient', lens)
    summary = reply['document']
    canvas = reply['canvas']
    detail['canvas'] = (summary['width'], summary['height'])
    assert abs(summary['height'] - SPHERICAL_HEIGHT) <= 2, ('canvas height', summary['height'])
    assert abs(summary['width'] - WIDTH) <= 10, ('canvas width', summary['width'], WIDTH)
    assert cursor(app, document) == start + 1, 'one history step'
    if depth != 'float':
        shift = (canvas['origin']['x'], canvas['origin']['y'])
        rotations = [[1, 0, 0, 0, 1, 0, 0, 0, 1]] + [row['rotation'] for row in reply['aligned']]
        errors = []
        for index, rotation, yaw in zip(layers, rotations, YAWS):
            mean, count = pixel_error(app, document, index, 'spherical', reply['focal'],
                                      rotation, shift, yaw, picture)
            errors.append(round(mean, 3))
            assert mean <= 3.0, ('mean |delta|', index, mean, count)
        detail['mean_codes'] = errors
    app.call('undo', document_id=document)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == SIZE, summary
    assert [rect(row) for row in rows(app, document)] == before, 'undo restores every rect'
    close(app, document)
    return detail


def availability_case(app, paths):
    """Collage and Reposition ignore Vignette Removal (take3): the call with it
    is the call without it."""
    seen = {}
    for projection in ('collage', 'reposition'):
        replies = []
        for vignette in (False, True):
            document = load(app, paths)['document']['id']
            start = cursor(app, document)
            arguments = dict(document_id=document, layers=[0, 1, 2], projection=projection)
            if vignette:
                arguments['vignette'] = True
            reply = app.call('auto_align_layers', **arguments)
            assert cursor(app, document) == start + 1, ('one history step', projection)
            replies.append(reply)
            close(app, document)
        plain, ignored = replies
        assert 'vignette' not in ignored and 'surface' not in ignored, ignored
        assert ignored.get('canvas') == plain.get('canvas'), (plain.get('canvas'),
                                                             ignored.get('canvas'))
        assert [r['layer'] for r in ignored['aligned']] == [r['layer'] for r in plain['aligned']]
        for a, b in zip(plain['aligned'], ignored['aligned']):
            assert all(abs(x - y) <= 1e-6 for x, y in zip(a['homography'], b['homography'])), (
                a, b)
        seen[projection] = dict(canvas=ignored.get('canvas'),
                                aligned=[r['layer'] for r in ignored['aligned']])
    return seen


def distortion_refusals(app, paths):
    seen = {}
    document = load(app, paths)['document']['id']
    start = cursor(app, document)
    for projection in ('perspective', 'reposition'):
        seen[f'lanczos {projection}'] = app.refused(
            'auto_align_layers', 'sampler must be bicubic with distortion true.',
            document_id=document, layers=[0, 1, 2], projection=projection, distortion=True,
            sampler='lanczos')
        assert cursor(app, document) == start, 'a refusal leaves no history step'
    app.call('set_layer_lock', document_id=document, layer=0, locks=['position'])
    start = cursor(app, document)
    seen['locked reference'] = app.refused(
        'auto_align_layers', 'cannot move because of a position lock. Geometric Distortion '
        'corrects every layer, the reference too: unlock it first.',
        document_id=document, layers=[0, 1, 2], projection='reposition', distortion=True)
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    close(app, document)
    twin = load(app, [paths[1], paths[1]])['document']['id']
    start = cursor(app, twin)
    seen['identical'] = app.refused(
        'auto_align_layers', 'onto a sphere. Turn off Geometric Distortion to align them with '
        'Perspective. Nothing was changed.', document_id=twin, layers=[0, 1],
        projection='auto', distortion=True)
    assert cursor(app, twin) == start, 'a refusal leaves no history step'
    seen['identical reposition'] = app.refused(
        'auto_align_layers', PLANAR_STILL, document_id=twin, layers=[0, 1],
        projection='reposition', distortion=True)
    assert cursor(app, twin) == start, 'a refusal leaves no history step'
    close(app, twin)
    return seen


def distortion_action(app, paths):
    recorded = load(app, paths)['document']['id']
    app.call('start_recording', discard=True)
    reply = app.call('auto_align_layers', document_id=recorded, layers=[0, 1, 2],
                     projection='perspective', distortion=True)
    stopped = app.call('stop_recording', name='v564-geometric-distortion')
    steps = [step for step in stopped['steps'] if step.get('tool') == 'auto_align_layers']
    assert len(steps) == 1 and steps[0]['arguments'].get('distortion') is True, stopped['steps']
    replayed = load(app, paths)['document']['id']
    start = cursor(app, replayed)
    app.call('run_action', document_id=replayed, name='v564-geometric-distortion')
    assert cursor(app, replayed) == start + 1, 'the replay is one history step'
    summary = app.call('get_document', document_id=replayed)
    want = (reply['document']['width'], reply['document']['height'])
    assert (summary['width'], summary['height']) == want, (summary['width'], want)
    close(app, recorded)
    close(app, replayed)
    return dict(step=steps[0], canvas=want)


PHOTOS = Path.home() / 'devel/doctored-fixtures/phase6/photos'
REAL_SETS = (('sky', -100), ('sky', 100), ('product', 0), ('glass', 0))
REAL_YAWS = ((-10.0, 0.0, 10.0), (-5.0, 0.0, 5.0))
REAL_K = -0.05
REAL_CROP = (800, 640)


class Grey:
    """A photograph's centred crop as grey levels in 0...1, bilinear."""

    def __init__(self, path):
        width, height, pixels = decode_png(path.read_bytes())
        self.width, self.height = width, height
        self.rows = [[sum(p[:3]) / 765 for p in row] for row in pixels]

    def value(self, fx, fy):
        fx = min(max(fx, 0.0), self.width - 1.0)
        fy = min(max(fy, 0.0), self.height - 1.0)
        x0, y0 = min(int(fx), self.width - 2), min(int(fy), self.height - 2)
        tx, ty = fx - x0, fy - y0
        top, bottom = self.rows[y0], self.rows[y0 + 1]
        upper = top[x0] + (top[x0 + 1] - top[x0]) * tx
        lower = bottom[x0] + (bottom[x0 + 1] - bottom[x0]) * tx
        return upper + (lower - upper) * ty


def unmap(fx, fy, k):
    """The frame point u with p = c + R q (1 + k |q|^2), p = (fx, fy): this
    file's own Newton inverse of the core's convention."""
    qx, qy = (fx - SIZE[0] / 2) / 300.0, (fy - SIZE[1] / 2) / 300.0
    target = math.hypot(qx, qy)
    if target == 0 or k == 0:
        return fx, fy
    rho = target
    for _ in range(60):
        step = (rho * (1 + k * rho * rho) - target) / (1 + 3 * k * rho * rho)
        rho -= step
        if abs(step) < 1e-13:
            break
    s = rho / target
    return SIZE[0] / 2 + 300.0 * qx * s, SIZE[1] / 2 + 300.0 * qy * s


def real_views(folder, grey, label, dy, yaws, k):
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for name, yaw in zip(NAMES, yaws):
        path = folder / f'{label}-dy{dy}-yaw{yaws[-1]:g}-k{k:g}-{name}.png'
        if not path.is_file():
            out = bytearray(b'\xff' * (SIZE[0] * SIZE[1] * 4))
            theta0 = math.radians(yaw)
            for y in range(SIZE[1]):
                for x in range(SIZE[0]):
                    fx, fy = unmap(x + 0.5, y + 0.5, k)
                    xc, yc = fx - SIZE[0] / 2, fy - SIZE[1] / 2
                    u = FOCAL * (theta0 + math.atan2(xc, FOCAL))
                    v = FOCAL * yc / math.hypot(xc, FOCAL)
                    g = grey.value(grey.width / 2 + u - 0.5, grey.height / 2 + dy + v - 0.5)
                    code = int(min(max(g, 0.0), 1.0) * 255 + 0.5)
                    p = (y * SIZE[0] + x) * 4
                    out[p] = out[p + 1] = out[p + 2] = code
            staged = path.with_suffix('.partial')
            png(staged, SIZE[0], SIZE[1], bytes(out))
            staged.rename(path)
        paths.append(path)
    return paths


def real_photo_case(app, folder):
    if not PHOTOS.is_dir():
        raise Skipped(f'{PHOTOS} not found')
    sips = '/usr/bin/sips'
    if not Path(sips).is_file():
        raise Skipped('sips is absent')
    source = folder / 'src'
    source.mkdir(parents=True, exist_ok=True)
    greys = {}
    for photo in sorted({photo for photo, _ in REAL_SETS}):
        out = source / f'{photo}.png'
        if not out.is_file():
            converted = subprocess.run(
                [sips, '-s', 'format', 'png', '-c', str(REAL_CROP[1]), str(REAL_CROP[0]),
                 str(PHOTOS / f'{photo}.jpg'), '--out', str(out)],
                capture_output=True, text=True)
            if converted.returncode != 0 or not out.is_file():
                raise Skipped(f'sips could not convert {photo}: {converted.stderr.strip()[:200]}')
        greys[photo] = Grey(out)
    tried, chosen = [], None
    for yaws in REAL_YAWS:
        for photo, dy in REAL_SETS:
            paths = real_views(folder, greys[photo], photo, dy, yaws, 0.0)
            document = load(app, paths)['document']['id']
            try:
                reply = app.call('auto_align_layers', document_id=document, layers=[0, 1, 2],
                                 projection='spherical')
                registered = reply['unregistered'] == [] and len(reply['aligned']) == 2
                outcome = 'registers' if registered else f"unregistered {[r['layer'] for r in reply['unregistered']]}"
            except Exception as error:  # noqa: BLE001 - a refusal is an outcome here
                registered, outcome = False, f'refused: {str(error)[:120]}'
            close(app, document)
            tried.append(dict(photo=photo, dy=dy, yaw=yaws[-1], outcome=outcome))
            if registered:
                chosen = (photo, dy, yaws)
                break
        if chosen:
            break
    if chosen is None:
        reason = 'no lens-free set registers all three layers with spherical'
        print(f'PENDING real-photo distortion: {reason}', flush=True)
        return dict(pending=reason, tried=tried)
    photo, dy, yaws = chosen
    paths = real_views(folder, greys[photo], photo, dy, yaws, REAL_K)
    document = load(app, paths)['document']['id']
    start = cursor(app, document)
    try:
        reply = app.call('auto_align_layers', document_id=document, layers=[0, 1, 2],
                         projection='perspective', distortion=True)
    except Exception as error:  # noqa: BLE001 - declared PENDING
        close(app, document)
        reason = f'the distorted {photo} set (dy {dy}, yaw {yaws[-1]:g}) was refused: {str(error)[:200]}'
        print(f'PENDING real-photo distortion: {reason}', flush=True)
        return dict(pending=reason, tried=tried)
    fit = reply.get('distortion') or {}
    if reply['unregistered'] or len(fit.get('layers') or []) != 3:
        close(app, document)
        reason = (f'the distorted {photo} set (dy {dy}, yaw {yaws[-1]:g}) registered '
                  f"{[r['layer'] for r in reply['aligned']]} only")
        print(f'PENDING real-photo distortion: {reason}', flush=True)
        return dict(pending=reason, tried=tried)
    detail = dict(photo=photo, dy=dy, yaw=yaws[-1], distortion=fit, tried=tried,
                  canvas=(reply['document']['width'], reply['document']['height']))
    assert abs(fit['coefficient'] - REAL_K) <= 0.01, ('coefficient', fit['coefficient'])
    assert cursor(app, document) == start + 1, 'one history step'
    close(app, document)
    return detail


# Collage and Reposition with Geometric Distortion (docstring PLANAR).
PLANAR_PHOTO = (Path(__file__).resolve().parent.parent
                / 'core/tests/fixtures/registration_photo/sky.jpg')
PLANAR_O0 = (440.0, 60.0)
PLANAR_K = -0.05
# Each layer's (dx, dy, degrees) from O0, the reference first.
PLANAR_SETS = {
    'reposition': ((0.0, 0.0, 0.0), (55.0, 12.0, 0.0), (-50.0, -14.0, 0.0)),
    'collage': ((0.0, 0.0, 0.0), (55.0, 12.0, 0.5), (-50.0, -14.0, -0.5)),
}
PLANAR_STILL = ('Geometric Distortion needs a layer that moves against the reference; none does '
                'here, so the overlaps say nothing about the lens. Turn off Geometric Distortion '
                'to align them. Nothing was changed.')


def planar_photo(folder):
    """PLANAR_PHOTO as a PNG `Grey` decodes, converted by sips into `folder`.
    The texture is committed, so a failed conversion fails the run: PLANAR is
    never skipped."""
    folder.mkdir(parents=True, exist_ok=True)
    out = folder / 'sky.png'
    if not out.is_file():
        converted = subprocess.run(
            ['/usr/bin/sips', '-s', 'format', 'png', str(PLANAR_PHOTO), '--out', str(out)],
            capture_output=True, text=True)
        if converted.returncode != 0 or not out.is_file():
            raise AssertionError(
                f'sips could not convert {PLANAR_PHOTO.name}: {converted.stderr.strip()[:200]}')
    return out


def planar_map(dx, dy, degrees):
    """A: the layer's corrected point u to the reference's, r = d + c + S (u - c)."""
    angle = math.radians(degrees)
    a, b = math.cos(angle), math.sin(angle)
    cx, cy = SIZE[0] / 2, SIZE[1] / 2
    return [a, -b, dx + cx - (a * cx - b * cy), b, a, dy + cy - (b * cx + a * cy), 0, 0, 1]


def planar_point(m, x, y):
    w = m[6] * x + m[7] * y + m[8]
    return (m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w


def planar_inverse(m):
    """The inverse by cofactors."""
    a = [m[4] * m[8] - m[5] * m[7], m[2] * m[7] - m[1] * m[8], m[1] * m[5] - m[2] * m[4],
         m[5] * m[6] - m[3] * m[8], m[0] * m[8] - m[2] * m[6], m[2] * m[3] - m[0] * m[5],
         m[3] * m[7] - m[4] * m[6], m[1] * m[6] - m[0] * m[7], m[0] * m[4] - m[1] * m[3]]
    det = m[0] * a[0] + m[1] * a[3] + m[2] * a[6]
    return [v / det for v in a]


def distort(ux, uy, k):
    """The core's convention forward: the frame point that shows the view's u."""
    qx, qy = (ux - SIZE[0] / 2) / 300.0, (uy - SIZE[1] / 2) / 300.0
    f = 1 + k * (qx * qx + qy * qy)
    return SIZE[0] / 2 + 300.0 * qx * f, SIZE[1] / 2 + 300.0 * qy * f


def inside_frame(x, y, margin):
    return margin <= x <= SIZE[0] - margin and margin <= y <= SIZE[1] - margin


def planar_views(folder, grey, projection):
    folder.mkdir(parents=True, exist_ok=True)
    paths = []
    for dx, dy, degrees in PLANAR_SETS[projection]:
        path = folder / (f'planar-o{PLANAR_O0[0]:g}-{PLANAR_O0[1]:g}-{dx:g}-{dy:g}-{degrees:g}'
                         f'-k{PLANAR_K:g}.png')
        if not path.is_file():
            m = planar_map(dx, dy, degrees)
            out = bytearray(b'\xff' * (SIZE[0] * SIZE[1] * 4))
            for y in range(SIZE[1]):
                for x in range(SIZE[0]):
                    ux, uy = unmap(x + 0.5, y + 0.5, PLANAR_K)
                    rx, ry = planar_point(m, ux, uy)
                    g = grey.value(PLANAR_O0[0] + rx - 0.5, PLANAR_O0[1] + ry - 0.5)
                    code = int(min(max(g, 0.0), 1.0) * 255 + 0.5)
                    p = (y * SIZE[0] + x) * 4
                    out[p] = out[p + 1] = out[p + 2] = code
            staged = path.with_suffix('.partial')
            png(staged, SIZE[0], SIZE[1], bytes(out))
            staged.rename(path)
        paths.append(path)
    return paths


def planar_oracle(m):
    """The content rect (x, y, w, h) in the old canvas: the destination pixel
    centres whose A^-1 lands in the frame's pixel-centre rect, its lens point
    too."""
    back = planar_inverse(m)
    images = [planar_point(m, x, y) for x, y in
              ((0.5, 0.5), (SIZE[0] - 0.5, 0.5), (SIZE[0] - 0.5, SIZE[1] - 0.5),
               (0.5, SIZE[1] - 0.5))]
    x0 = math.floor(min(p[0] for p in images)) - 2
    x1 = math.ceil(max(p[0] for p in images)) + 2
    y0 = math.floor(min(p[1] for p in images)) - 2
    y1 = math.ceil(max(p[1] for p in images)) + 2
    low, high = [None, None], [None, None]
    for y in range(y0, y1):
        for x in range(x0, x1):
            ux, uy = planar_point(back, x + 0.5, y + 0.5)
            if not inside_frame(ux, uy, 0.5) or not inside_frame(
                    *distort(ux, uy, PLANAR_K), 0.5):
                continue
            low = [x if low[0] is None else min(low[0], x), y if low[1] is None else min(low[1], y)]
            high = [x + 1 if high[0] is None else max(high[0], x + 1),
                    y + 1 if high[1] is None else max(high[1], y + 1)]
    return low[0], low[1], high[0] - low[0], high[1] - low[1]


def planar_error(app, document, layer, m, shift, grey):
    """Mean |layer - photo at O0 + p| in codes over the compared pixels, p in
    the old canvas, and their count."""
    back = planar_inverse(m)
    width, height, pixels, (ox, oy) = layer_pixels(app, document, layer)
    total, count = 0.0, 0
    for y in range(height):
        row = pixels[y]
        for x in range(width):
            got = row[x]
            if got[3] != 255:
                continue
            px, py = ox + x + 0.5 - shift[0], oy + y + 0.5 - shift[1]
            ux, uy = planar_point(back, px, py)
            if not inside_frame(ux, uy, INSIDE) or not inside_frame(
                    *distort(ux, uy, PLANAR_K), INSIDE):
                continue
            want = grey.value(PLANAR_O0[0] + px - 0.5, PLANAR_O0[1] + py - 0.5) * 255
            total += abs(got[0] - want)
            count += 1
    assert count > SIZE[0] * SIZE[1] // 2, ('too few pixels compared', layer, count)
    return total / count, count


def planar_case(app, paths, grey, projection, depth):
    document = stack(app, paths, depth)
    first = 1 if depth == 'float' else 0
    layers = [first, first + 1, first + 2]
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    summary = app.call('get_document', document_id=document)
    canvas_before = (summary['width'], summary['height'])
    reply = app.call('auto_align_layers', document_id=document, layers=layers,
                     projection=projection, distortion=True)
    fit = reply.get('distortion') or {}
    detail = dict(distortion=fit, canvas=reply.get('canvas'),
                  methods=[row['method'] for row in reply['aligned']])
    assert reply['changed'] is True and reply['projection'] == projection, reply
    assert reply['reference'] == first, reply
    assert reply['unregistered'] == [], reply
    assert fit.get('layers') == layers, ('distortion layers', fit)
    assert abs(fit['coefficient'] - PLANAR_K) <= 0.01, ('coefficient', fit['coefficient'])
    assert 'surface' not in reply and 'focal' not in reply, reply
    assert [row['layer'] for row in reply['aligned']] == layers[1:], reply['aligned']
    for row in reply['aligned']:
        assert row['method'] in ('translation', 'similarity'), row
    assert cursor(app, document) == start + 1, 'one history step'
    canvas = reply.get('canvas')
    shift = (canvas['origin']['x'], canvas['origin']['y']) if canvas else (0, 0)
    listed = rows(app, document)
    maps = [planar_map(*layer) for layer in PLANAR_SETS[projection]]
    sizes = []
    for index, m in zip(layers, maps):
        want = planar_oracle(m)
        got = (listed[index]['content_width'], listed[index]['content_height'])
        sizes.append(dict(got=got, oracle=want[2:]))
        assert abs(got[0] - want[2]) <= 1 and abs(got[1] - want[3]) <= 1, (
            'content size', index, got, want)
    detail['sizes'] = sizes
    if depth != 'float':
        errors = []
        for index, m in zip(layers, maps):
            mean, count = planar_error(app, document, index, m, shift, grey)
            errors.append(round(mean, 3))
            assert mean <= 3.0, ('mean |delta|', index, mean, count)
        detail['mean_codes'] = errors
    app.call('undo', document_id=document)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == canvas_before, summary
    assert [rect(row) for row in rows(app, document)] == before, 'undo restores every rect'
    close(app, document)
    return detail


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-stack-projection-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    picture = Cylinder()
    paths = author(output / 'fixtures', picture)
    vignetted = author_vignetted(output / 'fixtures', picture)
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}: {json.dumps(results[name]["detail"], default=str)}', flush=True)
        except Skipped as reason:
            results[name] = dict(skipped=True, reason=str(reason))
            print(f'SKIP {name}: {reason}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for projection in ('cylindrical', 'spherical'):
            for depth in (8, 16, 'float'):
                label = f'{depth}-bit' if depth != 'float' else 'float'
                run(f'{projection} {label}',
                    lambda: projection_case(app, paths, picture, projection, depth))
        run('refusals', lambda: refusals(app, paths))
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            run(f'perspective + vignette {label}',
                lambda: vignette_case(app, paths, picture, 'perspective', depth,
                                      SUCCESS_VIGNETTE))
        run('spherical + vignette 8-bit',
            lambda: vignette_case(app, paths, picture, 'spherical', 8, SUCCESS_VIGNETTE))
        run('auto + vignette 8-bit',
            lambda: vignette_case(app, paths, picture, 'auto', 8, SUCCESS_VIGNETTE))
        run('vignette refusals', lambda: vignette_refusals(app, paths, vignetted))
        run('vignette action', lambda: vignette_action(app, paths))
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            run(f'perspective + distortion {label}',
                lambda: distortion_case(app, paths, picture, 'perspective', depth))
        run('auto + distortion 8-bit',
            lambda: distortion_case(app, paths, picture, 'auto', 8))
        run('spherical + vignette + distortion 8-bit',
            lambda: distortion_case(app, paths, picture, 'spherical', 8, vignette=True))
        run('vignette ignored on collage and reposition', lambda: availability_case(app, paths))
        run('distortion refusals', lambda: distortion_refusals(app, paths))
        run('distortion action', lambda: distortion_action(app, paths))
        run('real-photo distortion', lambda: real_photo_case(app, output / 'photos'))
        grey = Grey(planar_photo(output / 'planar'))
        for projection, depths in (('reposition', (8, 16, 'float')), ('collage', (8,))):
            planar = planar_views(output / 'planar', grey, projection)
            for depth in depths:
                label = f'{depth}-bit' if depth != 'float' else 'float'
                run(f'{projection} + distortion {label}',
                    lambda: planar_case(app, planar, grey, projection, depth))
    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
