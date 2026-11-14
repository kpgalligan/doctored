#!/usr/bin/env python3
"""MCP drive for `combine_paths` (Paths panel ▸ Combine with Path…), against the Debug app.

- **Exact geometry**: rectangles sharing the edge x = 20.5 unite into one subpath of four anchors
  (area 750 by the shoelace sum); a rectangle less one with a coincident right edge leaves 387.5;
  abutting rectangles intersect to nothing (refused) and exclude to both (750).
- **Curves**: two four-cubic circles, areas by the exact Gauss-Legendre integral of each cubic:
  union + intersection − (A + B) within 0.001 of A + B; every sampled point of each result within
  0.1 px of an input outline. Bounds declared here, not widened.
- **Pixels**: `fill_path` of each result against the per-pixel algebra of `fill_path` of A and of B,
  where both inputs are fully in or fully out: equal.
- **Fill rules**: an even-odd ring united with a square inside its hole fills the ring and the
  square (5,200).
- **The list**: `with` unchanged, `path` keeps its name and its clipping designation, one undo
  step, undo restores the exact shape, `.doctored` reopen keeps the result, and a shape layer
  made from it with a stroke renders.
- **Refusals** by sentence, and one call each in 16-bit, 32-bit and CMYK documents.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check and a
FAIL line for a take that raised; exits non-zero unless every line is PASS.
"""
import argparse
import base64
import math
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402
from verify_phase2 import read_png  # noqa: E402

W, H = 200, 140
RESULTS = []
KAPPA = 0.5522847498307936


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def square(x0, y0, x1, y1, reverse=False):
    corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    return {'closed': True, 'anchors': corners[::-1] if reverse else corners}


def circle(cx, cy, r):
    """A circle as four cubics, the standard kappa construction."""
    k = KAPPA * r

    def anchor(x, y, dx, dy):
        return {'x': x, 'y': y, 'in': [x - dx, y - dy], 'out': [x + dx, y + dy], 'smooth': True}

    return {'closed': True, 'anchors': [anchor(cx + r, cy, 0, k), anchor(cx, cy + r, -k, 0),
                                        anchor(cx - r, cy, 0, -k), anchor(cx, cy - r, k, 0)]}


def normalized(anchor):
    if isinstance(anchor, list):
        return {'x': anchor[0], 'y': anchor[1], 'in': anchor, 'out': anchor}
    return anchor


def segments(subpaths):
    """Every segment of a shape as four control points, by the path model's rule."""
    out = []
    for subpath in subpaths:
        anchors = [normalized(a) for a in subpath['anchors']]
        pairs = list(zip(anchors, anchors[1:]))
        if subpath.get('closed') and len(anchors) > 1:
            pairs.append((anchors[-1], anchors[0]))
        for a, b in pairs:
            out.append(((a['x'], a['y']), tuple(a['out']), tuple(b['in']), (b['x'], b['y'])))
    return out


def point(seg, t):
    p0, p1, p2, p3 = seg
    u = 1 - t
    return tuple(u * u * u * p0[i] + 3 * u * u * t * p1[i] + 3 * u * t * t * p2[i] + t * t * t * p3[i]
                 for i in range(2))


def derivative(seg, t):
    p0, p1, p2, p3 = seg
    u = 1 - t
    return tuple(3 * u * u * (p1[i] - p0[i]) + 6 * u * t * (p2[i] - p1[i]) + 3 * t * t * (p3[i] - p2[i])
                 for i in range(2))


GAUSS = [(-0.8611363115940526, 0.3478548451374538), (-0.3399810435848563, 0.6521451548625461),
         (0.3399810435848563, 0.6521451548625461), (0.8611363115940526, 0.3478548451374538)]


def area(subpaths):
    """|½ ∮ x dy − y dx|: the integrand is a degree-5 polynomial on a cubic, which four-point
    Gauss-Legendre integrates exactly (degree 7)."""
    total = 0.0
    for seg in segments(subpaths):
        for node, weight in GAUSS:
            t = (node + 1) / 2
            x, y = point(seg, t)
            dx, dy = derivative(seg, t)
            total += weight / 2 * (x * dy - y * dx)
    return abs(total / 2)


def distance_to(p, seg):
    """The distance from p to a cubic: 64 samples, then a ternary refinement around the nearest."""
    best_t = min((i / 64 for i in range(65)),
                 key=lambda t: math.dist(p, point(seg, t)))
    lo, hi = max(best_t - 1 / 64, 0), min(best_t + 1 / 64, 1)
    for _ in range(40):
        a, b = lo + (hi - lo) / 3, hi - (hi - lo) / 3
        if math.dist(p, point(seg, a)) < math.dist(p, point(seg, b)):
            hi = b
        else:
            lo = a
    return math.dist(p, point(seg, (lo + hi) / 2))


def stray(result, inputs):
    """The farthest any sampled point of `result` lies from the inputs' outlines."""
    reference = segments(inputs)
    worst = 0.0
    for seg in segments(result):
        for i in range(17):
            p = point(seg, i / 16)
            worst = max(worst, min(distance_to(p, other) for other in reference))
    return worst


class Drive:
    def __init__(self, app, folder):
        self.app = app
        self.folder = folder

    def call(self, tool_name, **arguments):
        return self.app.call(tool_name, **arguments)

    def refuse(self, tool_name, **arguments):
        return self.app.client.call(tool_name, fail=True, **arguments)

    def new(self, **spec):
        spec = spec or dict(depth=8, profile='srgb')
        return self.call('new_document', width=W, height=H, background='white',
                         **spec)['document']['id']

    def close(self, *documents):
        for doc in documents:
            self.call('close_document', document_id=doc, discard_changes=True)

    def path(self, doc, name):
        return next(p for p in self.call('list_paths', document_id=doc)['paths'] if p['name'] == name)

    def render(self, doc):
        reply = self.app.client.rpc('tools/call', {'name': 'render', 'arguments': {
            'document_id': doc, 'max_side': 4096}})
        assert not reply.get('isError'), reply
        data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
        return read_png(base64.b64decode(data))

    def fill(self, subpaths, fill_rule='nonzero'):
        """The shape's fill_path in black on white, as one gray value per pixel."""
        doc = self.new()
        self.call('fill_path', document_id=doc, subpaths=subpaths, fill_rule=fill_rule,
                  color='#000000', layer=0)
        _, _, pixels = self.render(doc)
        self.close(doc)
        return [pixel[0] for pixel in pixels]

    def combined(self, a, b, operation, a_rule='nonzero', b_rule='nonzero'):
        """`combine_paths` of two fresh paths: the reply and the result's list row."""
        doc = self.new()
        self.call('set_path', document_id=doc, name='A', subpaths=a, fill_rule=a_rule)
        self.call('set_path', document_id=doc, name='B', subpaths=b, fill_rule=b_rule)
        reply = self.call('combine_paths', document_id=doc, path='A', **{'with': 'B'},
                          operation=operation)
        row = self.path(doc, 'A')
        self.close(doc)
        return reply, row


LEFT, RIGHT = square(5, 5, 20.5, 30), square(20.5, 5, 35, 30)
WIDE, NOTCH = square(5, 5, 35, 30), square(20.5, 5, 35, 30)


def take_rectangles(drive):
    reply, row = drive.combined([LEFT], [RIGHT], 'unite')
    assert reply['changed'] and len(row['subpaths']) == 1, (reply, row)
    corners = {(a['x'], a['y']) for a in row['subpaths'][0]['anchors']}
    assert corners == {(5, 5), (35, 5), (35, 30), (5, 30)} and len(row['subpaths'][0]['anchors']) == 4, row
    assert row['fill_rule'] == 'nonzero' and row['subpaths'][0]['closed'], row
    assert area(row['subpaths']) == 750, area(row['subpaths'])
    _, row = drive.combined([WIDE], [NOTCH], 'subtract')
    assert area(row['subpaths']) == 387.5, (area(row['subpaths']), row)
    _, row = drive.combined([LEFT], [RIGHT], 'exclude')
    assert area(row['subpaths']) == 750, (area(row['subpaths']), row)
    doc = drive.new()
    drive.call('set_path', document_id=doc, name='A', subpaths=[LEFT])
    drive.call('set_path', document_id=doc, name='B', subpaths=[RIGHT])
    steps = drive.call('list_history', document_id=doc)['undo_count']
    message = drive.refuse('combine_paths', document_id=doc, path='A', operation='intersect',
                           **{'with': 'B'})
    assert 'Nothing is left' in message, message
    assert drive.call('list_history', document_id=doc)['undo_count'] == steps
    drive.close(doc)
    ok('rectangles sharing an edge unite to one four-anchor subpath (750), subtract with a '
       'coincident edge to 387.5 and exclude to 750; their empty intersection refuses')


def take_curves(drive):
    a, b = circle(70, 70, 40), circle(110, 70, 40)
    grid_a, grid_b = drive.fill([a]), drive.fill([b])
    areas, strays = {}, {}
    for operation in ('unite', 'intersect', 'subtract', 'exclude'):
        _, row = drive.combined([a], [b], operation)
        # A segment that returns to its subpath's start merges into the first anchor: no closed
        # subpath ends on its own first point, and no two neighbours share one.
        for subpath in row['subpaths']:
            points = [(anchor['x'], anchor['y']) for anchor in subpath['anchors']]
            assert subpath['closed'] and all(p != q for p, q in zip(points, points[1:] + points[:1])), \
                (operation, points)
        if operation == 'unite':
            assert [len(s['anchors']) for s in row['subpaths']] == [8], row['subpaths']
        areas[operation] = area(row['subpaths'])
        strays[operation] = round(stray(row['subpaths'], [a, b]), 4)
        assert strays[operation] <= 0.1, (operation, strays)
        grid = drive.fill(row['subpaths'])
        for index, (pa, pb, pr) in enumerate(zip(grid_a, grid_b, grid)):
            if pa not in (0, 255) or pb not in (0, 255):
                continue
            ina, inb = pa == 0, pb == 0
            want = {'unite': ina or inb, 'intersect': ina and inb, 'subtract': ina and not inb,
                    'exclude': ina != inb}[operation]
            assert pr == (0 if want else 255), (operation, index % W, index // W, pa, pb, pr)
    each = area([a])
    identity = areas['unite'] + areas['intersect'] - 2 * each
    assert abs(identity) <= 0.001 * 2 * each, (identity, areas)
    assert abs(areas['exclude'] - (areas['unite'] - areas['intersect'])) <= 0.001 * 2 * each, areas
    ok(f'two circles: union + intersection − (A + B) = {identity:.4f} of {2 * each:.1f}; every '
       f'result stays within 0.1 px of the inputs ({strays}) and fills as the per-pixel algebra '
       'of theirs')


def take_fill_rules(drive):
    ring = [square(10, 10, 90, 90), square(30, 30, 70, 70)]
    _, row = drive.combined(ring, [square(40, 40, 60, 60)], 'unite', a_rule='evenodd')
    assert abs(area(row['subpaths']) - 5200) < 1e-6, (area(row['subpaths']), row)
    ok('an even-odd ring united with a square inside its hole fills the ring and the square (5200)')


def take_list(drive):
    doc = drive.new()
    drive.call('set_path', document_id=doc, name='A', subpaths=[WIDE])
    drive.call('set_path', document_id=doc, name='B', subpaths=[NOTCH])
    drive.call('set_clipping_path', document_id=doc, path='A')
    before = drive.call('list_paths', document_id=doc)['paths']
    steps = drive.call('list_history', document_id=doc)['undo_count']
    reply = drive.call('combine_paths', document_id=doc, path='A', operation='subtract',
                       **{'with': 'B'})
    assert reply['path'] == 'A' and reply['changed'] and reply['paths'] == 2, reply
    after = drive.call('list_paths', document_id=doc)['paths']
    assert [p['name'] for p in after] == ['A', 'B'] and after[0].get('clipping') is True, after
    assert after[1] == before[1], 'with changed'
    assert drive.call('list_history', document_id=doc)['undo_count'] == steps + 1
    again = drive.call('combine_paths', document_id=doc, path='A', operation='subtract',
                       **{'with': 'B'})
    assert again['changed'] is False, again
    saved = drive.folder / 'combined.doctored'
    drive.call('save_copy', document_id=doc, path=str(saved), format='doctored')
    reopened = drive.call('open_document', path=str(saved))['document']['id']
    assert drive.call('list_paths', document_id=reopened)['paths'] == after
    layer = drive.call('add_shape_layer', document_id=reopened, kind='path', path='A',
                       fill='#3366CC', stroke='#000000', stroke_width=2)
    assert layer['shape']['kind'] == 'path', layer
    drive.call('undo', document_id=doc)
    assert drive.call('list_paths', document_id=doc)['paths'] == before
    drive.close(reopened, doc)
    ok('the result keeps the path\'s name and clipping designation, leaves with alone, is one undo '
       'step (undone exactly), answers changed:false when repeated, reopens from .doctored and '
       'becomes a stroked shape layer')


def take_refusals(drive):
    doc = drive.new()
    drive.call('set_path', document_id=doc, name='A', subpaths=[LEFT])
    drive.call('set_path', document_id=doc, name='B', subpaths=[RIGHT])
    steps = drive.call('list_history', document_id=doc)['undo_count']
    cases = [
        (dict(path='Nope', operation='unite', **{'with': 'B'}), 'No path is named "Nope"'),
        (dict(path='A', operation='unite', **{'with': 'Gone'}), 'No path is named "Gone"'),
        (dict(path='A', operation='unite', **{'with': 'A'}), 'with names the same path'),
        (dict(path='A', operation='merge', **{'with': 'B'}), 'operation must be one of'),
        (dict(path='A', **{'with': 'B'}), 'requires operation'),
        (dict(operation='unite', **{'with': 'B'}), 'requires path'),
        (dict(path='A', operation='unite'), 'requires with'),
    ]
    for arguments, sentence in cases:
        message = drive.refuse('combine_paths', document_id=doc, **arguments)
        assert sentence in message, (arguments, message)
    assert drive.call('list_history', document_id=doc)['undo_count'] == steps
    drive.close(doc)
    ok('combine_paths refuses an unknown path, the same path twice, an unknown operation and a '
       'missing argument, each in its sentence with no undo step')


def take_modes(drive):
    """One call in every depth and colour mode that holds paths; a colour-mode document also
    lists the tool among its supported tools (its mode's closed `tools` set)."""
    specs = [('rgb16', dict(depth=16, profile='srgb'), None),
             ('rgb32', dict(depth=32, profile='srgb'), None),
             ('cmyk8', dict(color_mode='cmyk', depth=8), None),
             ('gray8', dict(color_mode='gray', depth=8), None),
             ('lab8', dict(color_mode='lab', depth=8), None),
             ('indexed', dict(depth=8, profile='srgb'),
              dict(mode='indexed', palette='exact', dither='none')),
             ('bitmap', dict(color_mode='gray', depth=8), dict(mode='bitmap', method='threshold'))]
    for label, spec, conversion in specs:
        doc = drive.new(**spec)
        if conversion:
            drive.call('convert_mode', document_id=doc, **conversion)
        if label not in ('rgb16', 'rgb32'):
            tools = drive.call('get_precision', document_id=doc)['supported_tools']
            assert 'combine_paths' in tools, (label, 'combine_paths is not a supported tool')
        drive.call('set_path', document_id=doc, name='A', subpaths=[LEFT])
        drive.call('set_path', document_id=doc, name='B', subpaths=[RIGHT])
        reply = drive.call('combine_paths', document_id=doc, path='A', operation='unite',
                           **{'with': 'B'})
        assert reply['changed'] and area(drive.path(doc, 'A')['subpaths']) == 750, (label, reply)
        drive.close(doc)
    ok('combine_paths unites in 16-bit and 32-bit float RGB, CMYK, Grayscale, Lab, Indexed and '
       'Bitmap documents, each colour mode listing it among its supported tools')


TAKES = [take_rectangles, take_curves, take_fill_rules, take_list, take_refusals, take_modes]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    parser.add_argument('--takes', default='', help='comma-separated take names (default: all)')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-path-combine-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    wanted = [name for name in arguments.takes.split(',') if name]
    takes = [take for take in TAKES if not wanted or take.__name__ in wanted]
    with App(output / 'app', output / 'recovery', interval=600) as app:
        drive = Drive(app, output)
        for take in takes:
            try:
                take(drive)
            except Exception as error:  # one failed take must not hide the others
                RESULTS.append(False)
                print(f'FAIL: {take.__name__} raised {type(error).__name__} — '
                      f'{str(error)[:900]} (files: {output})', flush=True)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
