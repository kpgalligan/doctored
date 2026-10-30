#!/usr/bin/env python3
"""MCP drive for Auto-Align's extended registration reach
(`AutoAlign.register`'s feature step, `core/src/feature_register.rs`) end to
end, on every projection, at 8, 16 and 32-bit float, with both lenses.

The script owns its app instance (`verify_v1_stack_align.App`: the Debug app or
`DR_VERIFY_APP`, its own scratch socket, `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB pin, private `DR_*_DIR`s) and terminates only
that process. Every expected number is computed here, never read back.

THE PHOTO is `core/tests/fixtures/registration_photo/sky.jpg` (CC0, 2048 x
1366), converted by `sips -s format png` and decoded by
`verify_v1_stack_projection.decode_png`, grey = the mean of R, G and B. As in
`scripts/reach-probe/main.swift` it is wrapped on a CYLINDER at 1 px per unit:
cylinder (u, v) is photo point (1024 + u, 450 + v), bilinear, pixel i's centre
at i + 0.5, clamped at the edges.

THE VIEWS are three 600 x 400 grey pinhole views at f 600, yawed -30, 0 and +30
degrees (`left`, `centre`, `right`, bottom first): pixel (x, y), xc = x + 0.5 -
300, yc = y + 0.5 - 200, shows cylinder point (f (yaw + atan2(xc, f)),
f yc / hypot(xc, f)). Adjacent views overlap 44 %; `left` and `right` share no
pixel. Every three-view case, (a) to (e), passes `reference` = the middle layer
(the 0-degree view, or the O0 crop), so each floating view is one step from it.
THE LENS (the core's convention): a lens view's pixel p shows the ideal view at
u with p = c + R q (1 + k |q|^2), q = (u - c)/R, c (300, 200), R 300, k -0.05.
THE VIGNETTE: the views times V = 1 - 0.4 rho^2 in sRGB linear light before
encoding, rho a pixel centre's distance from (300, 200) over hypot(299.5,
199.5) (the core's model).

TRUTH for a floating view yawed psi against the 0-degree reference: the
rotation by psi about the vertical axis through f, p -> (f x / z + 300,
f yc / z + 200) with x = xc cos psi + f sin psi, z = -xc sin psi + f cos psi.
ERROR: the worst |H(p) - T(p)| over the 33 x 33 grid of floating points
p = (0.5 + i 599/32, 0.5 + j 399/32) whose T(p) lands in the reference frame
[0, 600] x [0, 400] (the reach probe's grid).

BOUNDS, fixed before measuring and never widened:
(a) `perspective` at 8, 16 (`convert_precision`) and float ([float scene,
    views], layers 1-3), and `auto` at 8: `unregistered` []; each aligned
    row's `homography` within 2.0 px of the truth.
(b) `cylindrical` and `spherical` at 8, and `spherical` at float: `focal`
    within 1 % of 600; the canvas height within +-2 of 399 (cylindrical) or
    385 (spherical); its width within +-10 of 600 (2 atan(1/2) + pi/3) = 1185.
(c) `collage` and `reposition` at 8 on 600 x 400 crops of the photo itself at
    O0 = (724, 250) (the reference), O0 - (300, 20) and O0 + (300, 20); the
    collage crops also turned -5 and +5 degrees about their centre (a crop's
    pixel p shows photo point Rot(p - c) + c + its origin). Each row within
    2.0 px of the truth similarity (or translation).
(d) the yaw views at k -0.05 with `perspective` + `distortion`, and
    `reposition` + `distortion` on crops at O0, O0 + (110, -14) and
    O0 + (-110, 14) at k -0.05: |distortion.coefficient + 0.05| <= 0.01, three
    `distortion.layers`, `unregistered` [].
(e) the vignetted yaw views with `perspective` + `vignette`: `surface`
    "spherical", |vignette.coefficient + 0.4| <= 0.03, `focal` within 1 %.
(f) take2's own frames (`~/devel/doctored-fixtures/photoshop/panorama-564/
    take2/fixtures/rot_{left,centre,right}.png`, SKIP when absent) with take2's
    own reference (the bottom layer): `cylindrical` 974 x 399 and `spherical`
    974 x 385, each +-2, and the layers' content x within +-10 of 0, 209 and 419.
(g) unrelated frames: two disjoint 600 x 400 crops (photo (0, 0) and
    (1400, 900)), and the 0-degree view with its left-right mirror, each with
    the default projection: every floating layer `unregistered`, `changed`
    false, no history step.
Each success case is one history step, and one `undo` restores the canvas and
every rectangle.

Usage: python3 scripts/verify_v1_stack_reach.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS` when every case passed
(a `SKIP` only for (f) without take2).
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_stack_align import (  # noqa: E402
    App, FLOAT_FIXTURE, Skipped, close, cursor, load, rect, rows)
from verify_v1_stack_projection import decode_png, decode, encode  # noqa: E402
from verify_phase1 import png  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
PHOTO = ROOT / 'core/tests/fixtures/registration_photo/sky.jpg'
TAKE2 = Path.home() / 'devel/doctored-fixtures/photoshop/panorama-564/take2/fixtures'
SIZE = (600, 400)
FOCAL = 600.0
YAWS = (-30.0, 0.0, 30.0)
NAMES = ('left', 'centre', 'right')
LENS_K = -0.05
VIGNETTE = -0.4
VIGNETTE_RADIUS = math.hypot(SIZE[0] / 2 - 0.5, SIZE[1] / 2 - 0.5)
O0 = (724.0, 250.0)
PLAIN_BOUND = 2.0
WIDTH = 600 * (2 * math.atan(0.5) + math.pi / 3)
HEIGHT = {'cylindrical': 399, 'spherical': 385}


class Grey:
    """The photo's grey levels, 0...255, bilinear with pixel i's centre at i + 0.5."""

    def __init__(self, path):
        width, height, pixels = decode_png(path.read_bytes())
        self.width, self.height = width, height
        self.rows = [[sum(p[:3]) / 3 for p in row] for row in pixels]

    def sample(self, x, y):
        fx = min(max(x - 0.5, 0.0), self.width - 1.0)
        fy = min(max(y - 0.5, 0.0), self.height - 1.0)
        x0, y0 = min(int(fx), self.width - 2), min(int(fy), self.height - 2)
        tx, ty = fx - x0, fy - y0
        top, bottom = self.rows[y0], self.rows[y0 + 1]
        upper = top[x0] + (top[x0 + 1] - top[x0]) * tx
        lower = bottom[x0] + (bottom[x0 + 1] - bottom[x0]) * tx
        return upper + (lower - upper) * ty


def undistort(fx, fy, k):
    """The ideal frame point u with p = c + R q (1 + k |q|^2): Newton on the radius."""
    qx, qy = (fx - SIZE[0] / 2) / 300.0, (fy - SIZE[1] / 2) / 300.0
    target = math.hypot(qx, qy)
    if target == 0 or k == 0:
        return fx, fy
    rho = target
    for _ in range(40):
        step = (rho * (1 + k * rho * rho) - target) / (1 + 3 * k * rho * rho)
        rho -= step
        if abs(step) < 1e-13:
            break
    s = rho / target
    return SIZE[0] / 2 + 300.0 * qx * s, SIZE[1] / 2 + 300.0 * qy * s


def falloff(x, y):
    rho2 = ((x + 0.5 - SIZE[0] / 2) ** 2 + (y + 0.5 - SIZE[1] / 2) ** 2) / VIGNETTE_RADIUS ** 2
    return 1 + VIGNETTE * rho2


def write(path, source, k=0.0, vignette=False):
    """A 600 x 400 grey PNG whose ideal point u shows `source(u)` (0...255)."""
    if path.is_file():
        return path
    out = bytearray(b'\xff' * (SIZE[0] * SIZE[1] * 4))
    for y in range(SIZE[1]):
        for x in range(SIZE[0]):
            u = undistort(x + 0.5, y + 0.5, k) if k else (x + 0.5, y + 0.5)
            g = min(max(source(*u) / 255, 0.0), 1.0)
            if vignette:
                g = encode(decode(g) * falloff(x, y))
            code = int(g * 255 + 0.5)
            p = (y * SIZE[0] + x) * 4
            out[p] = out[p + 1] = out[p + 2] = code
    staged = path.with_suffix('.partial')
    png(staged, SIZE[0], SIZE[1], bytes(out))
    staged.rename(path)
    return path


def yaw_source(grey, yaw):
    theta0 = math.radians(yaw)

    def source(fx, fy):
        xc, yc = fx - SIZE[0] / 2, fy - SIZE[1] / 2
        u = FOCAL * (theta0 + math.atan2(xc, FOCAL))
        v = FOCAL * yc / math.hypot(xc, FOCAL)
        return grey.sample(1024 + u, 450 + v)
    return source


def crop_source(grey, origin, degrees=0.0):
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    cx, cy = SIZE[0] / 2, SIZE[1] / 2

    def source(fx, fy):
        dx, dy = fx - cx, fy - cy
        return grey.sample(origin[0] + c * dx - s * dy + cx, origin[1] + s * dx + c * dy + cy)
    return source


def mirror_source(source):
    return lambda fx, fy: source(SIZE[0] - fx, fy)


def yaw_truth(psi):
    a = math.radians(psi)

    def truth(px, py):
        xc, yc = px - SIZE[0] / 2, py - SIZE[1] / 2
        x = xc * math.cos(a) + FOCAL * math.sin(a)
        z = -xc * math.sin(a) + FOCAL * math.cos(a)
        if z <= 1e-9:
            return None
        return FOCAL * x / z + SIZE[0] / 2, FOCAL * yc / z + SIZE[1] / 2
    return truth


def crop_truth(shift, degrees):
    """A floating crop at O0 + shift turned `degrees`, to the O0 reference."""
    c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
    cx, cy = SIZE[0] / 2, SIZE[1] / 2

    def truth(px, py):
        dx, dy = px - cx, py - cy
        return c * dx - s * dy + cx + shift[0], s * dx + c * dy + cy + shift[1]
    return truth


def apply(m, x, y):
    w = m[6] * x + m[7] * y + m[8]
    if not w > 0:
        return None
    return (m[0] * x + m[1] * y + m[2]) / w, (m[3] * x + m[4] * y + m[5]) / w


def worst_error(m, truth):
    worst, count = 0.0, 0
    for j in range(33):
        for i in range(33):
            p = (0.5 + i * (SIZE[0] - 1) / 32, 0.5 + j * (SIZE[1] - 1) / 32)
            t = truth(*p)
            if t is None or not (0 <= t[0] <= SIZE[0] and 0 <= t[1] <= SIZE[1]):
                continue
            count += 1
            q = apply(m, *p)
            worst = math.inf if q is None else max(worst, math.hypot(q[0] - t[0], q[1] - t[1]))
    assert count > 100, ('too few grid points in the overlap', count)
    return worst


def stack(app, paths, depth):
    reply = load(app, [FLOAT_FIXTURE] + paths if depth == 'float' else paths)
    document = reply['document']['id']
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
        assert app.call('get_precision', document_id=document)['bits_per_channel'] == 16
    return document


def aligned_call(app, paths, depth, first_reference=False, **arguments):
    """One success call on a fresh stack: one history step, then `undo`
    restores the canvas and every rectangle. Answers the reply and the layers."""
    document = stack(app, paths, depth)
    first = 1 if depth == 'float' else 0
    layers = [first + index for index in range(len(paths))]
    reference = layers[0] if first_reference else layers[1]
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    summary = app.call('get_document', document_id=document)
    canvas_before = (summary['width'], summary['height'])
    if not first_reference:
        arguments['reference'] = reference
    reply = app.call('auto_align_layers', document_id=document, layers=layers, **arguments)
    assert reply['changed'] is True, reply
    assert reply['reference'] == reference, reply
    assert reply['unregistered'] == [], reply['unregistered']
    assert sorted(row['layer'] for row in reply['aligned']) == [
        layer for layer in layers if layer != reference], reply['aligned']
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    app.call('undo', document_id=document)
    summary = app.call('get_document', document_id=document)
    assert (summary['width'], summary['height']) == canvas_before, summary
    assert [rect(row) for row in rows(app, document)] == before, 'undo restores every rect'
    close(app, document)
    return reply, layers, listed


def homography_case(app, paths, depth, truths, **arguments):
    reply, layers, _ = aligned_call(app, paths, depth, **arguments)
    errors = {}
    for row in reply['aligned']:
        truth = truths[layers.index(row['layer'])]
        error = worst_error(row['homography'], truth)
        errors[row['layer']] = dict(method=row['method'], error=round(error, 3))
        assert error <= PLAIN_BOUND, ('alignment error', row['layer'], error, row['method'])
    return errors


def projection_case(app, paths, depth, projection):
    reply, _, _ = aligned_call(app, paths, depth, projection=projection)
    focal, summary = reply['focal'], reply['document']
    detail = dict(focal=focal, canvas=(summary['width'], summary['height']))
    assert abs(focal - FOCAL) <= 0.01 * FOCAL, ('focal', focal)
    assert abs(summary['height'] - HEIGHT[projection]) <= 2, ('canvas height', summary['height'])
    assert abs(summary['width'] - WIDTH) <= 10, ('canvas width', summary['width'], WIDTH)
    return detail


def distortion_case(app, paths, depth, **arguments):
    reply, layers, _ = aligned_call(app, paths, depth, distortion=True, **arguments)
    fit = reply.get('distortion') or {}
    assert sorted(fit.get('layers') or []) == layers, ('distortion layers', fit)
    assert abs(fit['coefficient'] - LENS_K) <= 0.01, ('coefficient', fit['coefficient'])
    return dict(distortion=fit, methods=[row['method'] for row in reply['aligned']])


def vignette_case(app, paths):
    reply, layers, _ = aligned_call(app, paths, 8, projection='perspective', vignette=True)
    fit = reply.get('vignette') or {}
    assert reply.get('surface') == 'spherical', ('surface', reply.get('surface'))
    assert abs(fit['coefficient'] - VIGNETTE) <= 0.03, ('vignette coefficient', fit)
    assert abs(reply['focal'] - FOCAL) <= 0.01 * FOCAL, ('focal', reply['focal'])
    return dict(vignette=fit, focal=reply['focal'])


def take2_case(app, projection):
    paths = [TAKE2 / f'rot_{name}.png' for name in NAMES]
    if not all(path.is_file() for path in paths):
        raise Skipped(f'take2 frames not found under {TAKE2}')
    reply, layers, listed = aligned_call(app, paths, 8, first_reference=True,
                                         projection=projection)
    summary = reply['document']
    xs = [listed[index]['content_x'] for index in layers]
    detail = dict(canvas=(summary['width'], summary['height']), layer_x=xs)
    assert abs(summary['width'] - 974) <= 2, ('canvas width', summary['width'])
    assert abs(summary['height'] - HEIGHT[projection]) <= 2, ('canvas height', summary['height'])
    for got, want in zip(xs, (0, 209, 419)):
        assert abs(got - want) <= 10, ('layer x', xs)
    return detail


def unrelated_case(app, paths):
    document = load(app, paths)['document']['id']
    start = cursor(app, document)
    before = [rect(row) for row in rows(app, document)]
    reply = app.call('auto_align_layers', document_id=document, layers=[0, 1])
    assert reply['changed'] is False and reply['aligned'] == [], reply
    assert [row['layer'] for row in reply['unregistered']] == [1], reply['unregistered']
    assert cursor(app, document) == start, 'no history step'
    assert [rect(row) for row in rows(app, document)] == before, 'nothing moved'
    close(app, document)
    return dict(reasons=[row['reason'] for row in reply['unregistered']])


def author(folder):
    folder.mkdir(parents=True, exist_ok=True)
    converted = folder / 'sky.png'
    if not converted.is_file():
        subprocess.run(['/usr/bin/sips', '-s', 'format', 'png', str(PHOTO), '--out',
                        str(converted)], check=True, capture_output=True, timeout=120)
    grey = Grey(converted)
    views = {}
    for name, yaw in zip(NAMES, YAWS):
        source = yaw_source(grey, yaw)
        views[name] = write(folder / f'yaw-{name}.png', source)
        views[f'lens-{name}'] = write(folder / f'yaw-{name}-k{LENS_K:g}.png', source, k=LENS_K)
        views[f'vig-{name}'] = write(folder / f'yaw-{name}-vig.png', source, vignette=True)
    views['mirror'] = write(folder / 'yaw-centre-mirror.png',
                            mirror_source(yaw_source(grey, 0.0)))
    for label, shift, degrees in (('minus', (-300, -20), -5.0), ('plus', (300, 20), 5.0)):
        origin = (O0[0] + shift[0], O0[1] + shift[1])
        views[f'shift-{label}'] = write(folder / f'crop-{label}.png', crop_source(grey, origin))
        views[f'turn-{label}'] = write(folder / f'crop-{label}-turn.png',
                                       crop_source(grey, origin, degrees))
    views['o0'] = write(folder / 'crop-o0.png', crop_source(grey, O0))
    views['lens-o0'] = write(folder / f'crop-o0-k{LENS_K:g}.png', crop_source(grey, O0), k=LENS_K)
    for label, shift in (('a', (110, -14)), ('b', (-110, 14))):
        origin = (O0[0] + shift[0], O0[1] + shift[1])
        views[f'lens-crop-{label}'] = write(folder / f'crop-{label}-k{LENS_K:g}.png',
                                            crop_source(grey, origin), k=LENS_K)
    views['disjoint-a'] = write(folder / 'disjoint-a.png', crop_source(grey, (0.0, 0.0)))
    views['disjoint-b'] = write(folder / 'disjoint-b.png', crop_source(grey, (1400.0, 900.0)))
    return views


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-stack-reach-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    views = author(output / 'fixtures')
    yaw_paths = [views[name] for name in NAMES]
    yaw_truths = [yaw_truth(yaw) for yaw in YAWS]
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
        # (a)
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            run(f'(a) perspective {label}',
                lambda: homography_case(app, yaw_paths, depth, yaw_truths,
                                        projection='perspective'))
        run('(a) auto 8-bit', lambda: homography_case(app, yaw_paths, 8, yaw_truths,
                                                      projection='auto'))
        # (b)
        run('(b) cylindrical 8-bit', lambda: projection_case(app, yaw_paths, 8, 'cylindrical'))
        run('(b) spherical 8-bit', lambda: projection_case(app, yaw_paths, 8, 'spherical'))
        run('(b) spherical float',
            lambda: projection_case(app, yaw_paths, 'float', 'spherical'))
        # (c)
        turned = [views['turn-minus'], views['o0'], views['turn-plus']]
        shifted = [views['shift-minus'], views['o0'], views['shift-plus']]
        run('(c) collage 8-bit', lambda: homography_case(
            app, turned, 8, [crop_truth((-300, -20), -5.0), None, crop_truth((300, 20), 5.0)],
            projection='collage'))
        run('(c) reposition 8-bit', lambda: homography_case(
            app, shifted, 8, [crop_truth((-300, -20), 0.0), None, crop_truth((300, 20), 0.0)],
            projection='reposition'))
        # (d)
        run('(d) perspective + distortion 8-bit', lambda: distortion_case(
            app, [views[f'lens-{name}'] for name in NAMES], 8, projection='perspective'))
        run('(d) reposition + distortion 8-bit', lambda: distortion_case(
            app, [views['lens-crop-a'], views['lens-o0'], views['lens-crop-b']], 8,
            projection='reposition'))
        # (e)
        run('(e) perspective + vignette 8-bit',
            lambda: vignette_case(app, [views[f'vig-{name}'] for name in NAMES]))
        # (f)
        run('(f) take2 cylindrical', lambda: take2_case(app, 'cylindrical'))
        run('(f) take2 spherical', lambda: take2_case(app, 'spherical'))
        # (g)
        run('(g) disjoint crops',
            lambda: unrelated_case(app, [views['disjoint-a'], views['disjoint-b']]))
        run('(g) a view and its mirror',
            lambda: unrelated_case(app, [views['centre'], views['mirror']]))
    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
