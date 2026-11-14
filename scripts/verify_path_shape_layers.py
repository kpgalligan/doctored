#!/usr/bin/env python3
"""MCP drive for path shape layers (`add_shape_layer` with kind "path", the agent mirror of Paths
panel ▸ Convert to Shape Layer), against the Debug app.

- **Reopen and re-render** in RGB 8, 16 and 32 bits, Grayscale, Lab and CMYK: three path shapes
  (a curved blob with a hole, even-odd, fill and stroke; an open sharp V with a 12 px miter
  stroke; a rotated, dashed, round-joined triangle) saved as `.doctored` and reopened with equal
  pixels, shape objects and bounds; each stroke width moved and back with the same; a move and its
  undo. Every raster's outermost pixels are clear (the source rect holds the stroke).
- **The fill against the path's own coverage**: the shape's fill against `fill_path` of the same
  subpaths, both fill rules, 0 differing pixels (Lab: at most 1 of 255, and a rectangle shape
  against a rectangle `fill_path` is measured beside it).
- **The miter**: the V's tip, apex + (w/2)/sin(θ/2) along the bisector, lies in the layer.
- **The fill rule**: an even-odd ring and the same ring with its hole reversed under nonzero render
  equal pixels with inside, outside and centred strokes, round joins and dots; the hole is clear.
- **By name and by shape**, **re-box** (w doubled scales the outline's x), **size** (3,000 anchors
  at 8 and 16 bits and in CMYK; get_document reports the count and no outline), **refusals** by
  sentence with the undo count unchanged, **the box kinds' versions**, and **Actions** (a step as
  the panel records it replays to the same pixels; a relative geometry mode refuses).

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check and a
FAIL line for a take that raised; exits non-zero unless every line is PASS. `--takes` runs some.
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

W, H = 300, 240
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'lab8': dict(color_mode='lab', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
RESULTS = []


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def smooth(x, y, dx, dy):
    return {'x': x, 'y': y, 'in': [x - dx, y - dy], 'out': [x + dx, y + dy], 'smooth': True}


# A: a closed curved blob with a square hole (even-odd), fill and stroke.
BLOB = [
    {'closed': True, 'anchors': [smooth(60.25, 30.5, 30, 0), smooth(130.75, 80.25, 0, 30),
                                 smooth(60.5, 130.125, -30, 0), [20.5, 80.75]]},
    {'closed': True, 'anchors': [[50, 60], [90, 60], [90, 100], [50, 100]]},
]
# B: an OPEN sharp V, stroke only, miter join: the tip reaches far past the outline's box.
VEE = [{'closed': False, 'anchors': [[170.5, 30.5], [185.5, 110.5], [200.5, 30.5]]}]
# C: a triangle with one curve, rotated, round-joined, dashed.
TRI = [{'closed': True, 'anchors': [[200, 150], smooth(270, 170, 5, 20), [215, 215]]}]
ROT = [math.cos(math.radians(15)), -math.sin(math.radians(15)),
       math.sin(math.radians(15)), math.cos(math.radians(15))]
STYLES = {
    'A': dict(subpaths=BLOB, fill_rule='evenodd', fill='#3366CC', stroke='#101010', stroke_width=3),
    'B': dict(subpaths=VEE, stroke='#AA2200', stroke_width=12),
    'C': dict(subpaths=TRI, fill='#22AA44CC', stroke='#000000', stroke_width=4,
              stroke_join='round', stroke_dash='dashed', transform=ROT),
}


def square(x0, y0, x1, y1, reverse=False):
    corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    return {'closed': True, 'anchors': corners[::-1] if reverse else corners}


class Drive:
    def __init__(self, app, folder):
        self.app = app
        self.folder = folder
        self.files = 0

    def call(self, tool_name, **arguments):
        return self.app.call(tool_name, **arguments)

    def refuse(self, tool_name, **arguments):
        return self.app.client.call(tool_name, fail=True, **arguments)

    def new(self, kind, background='white'):
        return self.call('new_document', width=W, height=H, background=background,
                         **KINDS[kind])['document']['id']

    def close(self, *documents):
        for doc in documents:
            self.call('close_document', document_id=doc, discard_changes=True)

    def render(self, doc, **extra):
        reply = self.app.client.rpc('tools/call', {'name': 'render', 'arguments': {
            'document_id': doc, 'max_side': 4096, **extra}})
        assert not reply.get('isError'), reply
        data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
        return read_png(base64.b64decode(data))

    def rows(self, doc):
        return {row['index']: row for row in self.call('get_document', document_id=doc)['layers']}

    def shaped(self, doc):
        return sorted(i for i, row in self.rows(doc).items() if row.get('shape'))

    def undo_count(self, doc):
        return self.call('list_history', document_id=doc)['undo_count']

    def snapshot(self, doc, indices):
        state = self.rows(doc)
        return {
            'layers': {n: self.render(doc, layer=i) for n, i in indices.items()},
            'shape': {n: state[i].get('shape') for n, i in indices.items()},
            'bounds': {n: bounds(state[i]) for n, i in indices.items()},
        }

    def save_reopen(self, doc, name):
        self.files += 1
        path = self.folder / f'{name}-{self.files}.doctored'
        self.call('save_copy', document_id=doc, path=str(path), format='doctored')
        return self.call('open_document', path=str(path))['document']['id']


def bounds(row):
    return (row['offset_x'], row['offset_y'], row['width'], row['height'])


def differing(first, second):
    """How many pixels two decoded renders differ at, and by how much at most."""
    assert first[:2] == second[:2], (first[:2], second[:2])
    worst, count = 0, 0
    for p, q in zip(first[2], second[2]):
        if p != q:
            count += 1
            worst = max(worst, max(abs(x - y) for x, y in zip(p, q)))
    return count, worst


def same_snapshot(now, then):
    """The names whose pixels, shape object or bounds differ."""
    return sorted(n for n in now['layers'] if now['layers'][n] != then['layers'][n]
                  or now['shape'][n] != then['shape'][n] or now['bounds'][n] != then['bounds'][n])


def layer_alpha(drive, doc, index):
    """The layer's alpha plane, canvas-sized, as (width, height, alpha per pixel). An RGB
    document's only: a colour-mode document renders no layer's alpha (the source rect the checks
    below test is the same in every mode)."""
    width, height, pixels = drive.render(doc, layer=index, channel='alpha')
    return width, height, [pixel[0] for pixel in pixels]


def edges_clear(drive, doc, index):
    """Whether the layer's outermost raster rows and columns are fully transparent."""
    x, y, w, h = bounds(drive.rows(doc)[index])
    width, height, alpha = layer_alpha(drive, doc, index)
    for py in range(y, y + h):
        for px in range(x, x + w):
            if py in (y, y + h - 1) or px in (x, x + w - 1):
                if 0 <= px < width and 0 <= py < height and alpha[py * width + px] != 0:
                    return False
    return True


# ---- the round trip ---------------------------------------------------------------------------

def take_round_trip(drive):
    for kind in KINDS:
        doc = drive.new(kind)
        for style in STYLES.values():
            reply = drive.call('add_shape_layer', document_id=doc, kind='path', **style)
            assert reply['shape']['kind'] == 'path' and reply['shape']['version'] == 7, reply
        indices = dict(zip('ABC', drive.shaped(doc)))
        made = drive.snapshot(doc, indices)
        # The V's miter tip: apex + (w / 2) / sin(θ / 2) straight down the bisector.
        tip = 110.5 + 6 / math.sin(math.atan2(15, 80))
        x, y, w, h = made['bounds']['B']
        assert y + h > tip + 1, (kind, made['bounds']['B'], tip)
        if kind.startswith('rgb'):
            for name, index in indices.items():
                assert edges_clear(drive, doc, index), (kind, name, made['bounds'][name])
            width, _, alpha = layer_alpha(drive, doc, indices['B'])
            inside = alpha[(math.floor(tip) - 2) * width + 185]
            assert inside > 64, (kind, 'the miter is not drawn near its tip', inside)

        again = drive.save_reopen(doc, kind)
        assert not same_snapshot(drive.snapshot(again, indices), made), (kind, 'reopen')
        widths = {'A': 3, 'B': 12, 'C': 4}
        for name, index in indices.items():
            drive.call('edit_shape_layer', document_id=again, layer=index,
                       stroke_width=widths[name] + 1)
        moved = drive.snapshot(again, indices)
        assert all(moved['layers'][n] != made['layers'][n] for n in indices), (kind, 'restyle')
        for name, index in indices.items():
            drive.call('edit_shape_layer', document_id=again, layer=index, stroke_width=widths[name])
        assert not same_snapshot(drive.snapshot(again, indices), made), (kind, 're-render')
        drive.call('edit_shape_layer', document_id=again, layer=indices['A'], x=40, y=20)
        assert drive.rows(again)[indices['A']]['shape']['origin'] == {'x': 40, 'y': 20}
        drive.call('undo', document_id=again)
        assert not same_snapshot(drive.snapshot(again, indices), made), (kind, 'undo')
        drive.close(again, doc)
        ok(f'{kind}: three path shapes reopen, re-render after the reopen and undo a move with '
           'equal pixels, shapes and bounds; the V\'s miter tip is inside its raster'
           + (', every raster\'s edge is clear' if kind.startswith('rgb') else ''))


# ---- the fill against fill_path ---------------------------------------------------------------

def fill_against_path(drive, kind, shape_arguments, path_arguments):
    one = drive.new(kind)
    drive.call('add_shape_layer', document_id=one, fill='#000000', **shape_arguments)
    two = drive.new(kind)
    layer = drive.call('new_layer', document_id=two)
    layer = layer.get('layer', layer.get('index'))
    drive.call('fill_path', document_id=two, color='#000000', layer=layer, **path_arguments)
    result = differing(drive.render(one), drive.render(two))
    drive.close(one, two)
    return result


def take_fill_oracle(drive):
    for kind in KINDS:
        found = {}
        for rule in ('evenodd', 'nonzero'):
            found[rule] = fill_against_path(
                drive, kind, dict(kind='path', subpaths=BLOB, fill_rule=rule),
                dict(subpaths=BLOB, fill_rule=rule))
        if kind == 'lab8':
            rect = fill_against_path(
                drive, kind, dict(kind='rect', x=20, y=30, w=101.25, h=70.5),
                dict(subpaths=[square(20, 30, 121.25, 100.5)]))
            assert all(worst <= 1 for _, worst in found.values()), (kind, found)
            ok(f'{kind}: the fill is the path\'s own coverage within 1 of 255 ({found}); a '
               f'rectangle shape against a rectangle fill_path differs {rect}')
        else:
            assert all(count == 0 for count, _ in found.values()), (kind, found)
            ok(f'{kind}: the fill is the path\'s own coverage, 0 differing pixels for both rules')


# ---- the fill rule and the stroke -------------------------------------------------------------

def take_fill_rule(drive):
    doc = drive.new('rgb8', background='transparent')
    even = [square(20, 20, 100, 100), square(40, 40, 80, 80)]
    reversed_hole = [square(20, 20, 100, 100), square(40, 40, 80, 80, reverse=True)]
    cases = [('inside', 'round', 'none'), ('outside', 'round', 'none'),
             ('center', 'round', 'none'), ('inside', 'round', 'dotted'),
             ('outside', 'round', 'dotted'), ('inside', 'miter', 'none')]
    for align, join, dash in cases:
        style = dict(kind='path', fill='#3366CC', stroke='#AA2200', stroke_width=4,
                     stroke_align=align, stroke_join=join, stroke_dash=dash)
        one = drive.call('add_shape_layer', document_id=doc, subpaths=even, fill_rule='evenodd',
                         **style)['layer']
        first = drive.render(doc, layer=one)
        two = drive.call('add_shape_layer', document_id=doc, subpaths=reversed_hole,
                         fill_rule='nonzero', **style)['layer']
        second = drive.render(doc, layer=two)
        assert differing(first, second) == (0, 0), (align, join, dash, differing(first, second))
        width, _, alpha = layer_alpha(drive, doc, one)
        assert alpha[60 * width + 60] == 0, (align, 'the hole is painted')
        drive.call('delete_layer', document_id=doc, index=two)
        drive.call('delete_layer', document_id=doc, index=one)
    drive.close(doc)
    ok('an even-odd ring and the nonzero ring with its hole reversed render equal pixels with '
       'inside, outside and centred strokes, round and miter joins and dots; the hole stays clear')


# ---- by name, re-box, size --------------------------------------------------------------------

def take_by_name(drive):
    doc = drive.new('rgb8', background='transparent')
    drive.call('set_path', document_id=doc, name='Logo', subpaths=BLOB, fill_rule='evenodd')
    style = dict(kind='path', fill='#3366CC', stroke='#101010', stroke_width=3)
    named = drive.call('add_shape_layer', document_id=doc, path='Logo', **style)
    shaped = drive.call('add_shape_layer', document_id=doc, subpaths=BLOB, fill_rule='evenodd',
                        **style)
    assert named['shape'] == shaped['shape'], (named, shaped)
    assert drive.render(doc, layer=named['layer']) == drive.render(doc, layer=shaped['layer'])
    drive.close(doc)
    ok('a path by name and the same subpaths make equal layers')


def scaled_blob(origin, sx):
    """BLOB with every x scaled by sx about origin's x."""
    def x(value):
        return origin + (value - origin) * sx

    out = []
    for subpath in BLOB:
        anchors = []
        for anchor in subpath['anchors']:
            if isinstance(anchor, list):
                anchors.append([x(anchor[0]), anchor[1]])
            else:
                anchors.append({'x': x(anchor['x']), 'y': anchor['y'],
                                'in': [x(anchor['in'][0]), anchor['in'][1]],
                                'out': [x(anchor['out'][0]), anchor['out'][1]],
                                'smooth': anchor['smooth']})
        out.append({'closed': subpath['closed'], 'anchors': anchors})
    return out


def take_rebox(drive):
    doc = drive.new('rgb8')
    made = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=BLOB,
                      fill_rule='evenodd', fill='#000000')
    shape = made['shape']
    edited = drive.call('edit_shape_layer', document_id=doc, layer=made['layer'], w=shape['w'] * 2)
    before, after = shape['subpaths'], edited['shape']['subpaths']
    for old, new in zip(before, after):
        for a, b in zip(old['anchors'], new['anchors']):
            assert abs(b['x'] - 2 * a['x']) < 1e-9 and b['y'] == a['y'], (a, b)
            for key in ('in', 'out'):
                assert abs(b[key][0] - 2 * a[key][0]) < 1e-9 and b[key][1] == a[key][1], (a, b)
    origin = shape['origin']['x']
    oracle = drive.new('rgb8')
    layer = drive.call('new_layer', document_id=oracle)
    layer = layer.get('layer', layer.get('index'))
    drive.call('fill_path', document_id=oracle, color='#000000', layer=layer,
               subpaths=scaled_blob(origin, 2), fill_rule='evenodd')
    found = differing(drive.render(doc), drive.render(oracle))
    assert found == (0, 0), found
    # A straight vertical path at x = 100.5: its box's origin floors to a whole pixel, so the box
    # is 0.5 px wide while the outline spans no width. w cannot give it one (no undo step), and
    # h scales y alone: the line stays at x = 100.5.
    line = drive.call('add_shape_layer', document_id=doc, kind='path',
                      subpaths=[{'closed': False, 'anchors': [[100.5, 20], [100.5, 80]]}],
                      stroke='#000000', stroke_width=3)
    assert line['shape']['w'] == 0.5 and line['shape']['origin'] == {'x': 100, 'y': 20}, line
    steps = drive.undo_count(doc)
    message = drive.refuse('edit_shape_layer', document_id=doc, layer=line['layer'], w=10)
    assert 'straight line with no width, so it cannot be given one' in message, message
    assert drive.undo_count(doc) == steps
    taller = drive.call('edit_shape_layer', document_id=doc, layer=line['layer'], h=120)['shape']
    anchors = [anchor for subpath in taller['subpaths'] for anchor in subpath['anchors']]
    assert taller['w'] == 0.5 and taller['h'] == 120, taller
    assert [(a['x'], a['y']) for a in anchors] == [(0.5, 0), (0.5, 120)], anchors
    drive.close(doc, oracle)
    ok('edit_shape_layer with w doubled doubles the outline\'s x and fills the scaled path exactly; '
       'a vertical line at x = 100.5 refuses a width and keeps its x under a new height')


def long_outline(count=3000):
    anchors = []
    for i in range(count):
        t = 2 * math.pi * i / count
        radius = 90 + 12 * math.sin(40 * t)
        x, y = 150 + radius * math.cos(t), 120 + radius * math.sin(t)
        tx, ty = -math.sin(t) * 0.05, math.cos(t) * 0.05
        anchors.append({'x': round(x, 4), 'y': round(y, 4),
                        'in': [round(x - tx, 4), round(y - ty, 4)],
                        'out': [round(x + tx, 4), round(y + ty, 4)], 'smooth': True})
    return [{'closed': True, 'anchors': anchors}]


def take_size(drive):
    outline = long_outline()
    for kind in ('rgb8', 'rgb16', 'cmyk8'):
        doc = drive.new(kind)
        reply = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=outline,
                           fill='#884400', stroke='#000000', stroke_width=1)
        assert len(reply['shape']['subpaths'][0]['anchors']) == 3000, reply['shape']['anchors']
        index = reply['layer']
        row = drive.rows(doc)[index]['shape']
        assert row['anchors'] == 3000 and row['subpath_count'] == 1 and 'subpaths' not in row, row
        made = drive.snapshot(doc, {'L': index})
        again = drive.save_reopen(doc, f'{kind}-long')
        assert not same_snapshot(drive.snapshot(again, {'L': index}), made), kind
        drive.call('edit_shape_layer', document_id=again, layer=index, fill='#004488')
        drive.call('edit_shape_layer', document_id=again, layer=index, fill='#884400')
        assert not same_snapshot(drive.snapshot(again, {'L': index}), made), kind
        drive.close(again, doc)
    ok('a 3,000-anchor outline reopens and re-renders at 8 and 16 bits and in CMYK; get_document '
       'reports its count and not its outline')


# ---- refusals, versions, Actions --------------------------------------------------------------

def take_refusals(drive):
    doc = drive.new('rgb8')
    drive.call('set_path', document_id=doc, name='Empty', subpaths=[])
    drive.call('set_path', document_id=doc, name='Line', subpaths=[{'anchors': [[10, 10], [90, 10]]}])
    line = drive.call('add_shape_layer', document_id=doc, kind='path', path='Line',
                      stroke='#000000', stroke_width=3)
    steps = drive.undo_count(doc)
    cases = [
        (dict(kind='path', subpaths=BLOB, x=4, fill='#000000'),
         'places itself from its outline, so x does not apply'),
        (dict(kind='path', fill='#000000'), 'requires path'),
        (dict(kind='path', subpaths=BLOB, path='Empty', fill='#000000'),
         'Pass path or subpaths, not both.'),
        (dict(kind='path', path='Empty', fill='#000000'), 'The path is empty'),
        (dict(kind='path', path='Nope', fill='#000000'), 'No path is named "Nope"'),
        (dict(kind='path', subpaths=BLOB, stroke_width=0), 'Pass at least one visible paint'),
        (dict(kind='path', path='Line', fill='#000000'), 'under 1 px across on one axis'),
        (dict(kind='rect', x=1, y=1, w=9, h=9, subpaths=BLOB, fill='#000000'),
         'subpaths goes with kind "path"'),
        (dict(kind='polygon', x=1, y=1, w=9, h=9, fill='#000000'),
         'kind must be rect, ellipse, line, or path'),
    ]
    for arguments, sentence in cases:
        message = drive.refuse('add_shape_layer', document_id=doc, **arguments)
        assert sentence in message, (arguments, message)
    message = drive.refuse('edit_shape_layer', document_id=doc, layer=line['layer'], h=10)
    assert 'cannot be given one' in message, message
    assert drive.undo_count(doc) == steps
    drive.close(doc)
    ok('every path shape refusal answers its sentence and adds no undo step')


def take_versions(drive):
    for kind, version in (('rgb8', 3), ('rgb32', 4), ('cmyk8', 6)):
        doc = drive.new(kind)
        plain = drive.call('add_shape_layer', document_id=doc, kind='rect', x=10, y=10, w=40, h=30,
                           fill='#336699')
        styled = drive.call('add_shape_layer', document_id=doc, kind='ellipse', x=10, y=10, w=40,
                            h=30, fill='#336699', stroke='#000000', stroke_dash='dashed')
        assert plain['shape']['version'] == version, (kind, plain['shape'])
        assert styled['shape']['version'] == (6 if kind == 'cmyk8' else 5), (kind, styled['shape'])
        assert 'subpaths' not in plain['shape'] and 'fill_rule' not in plain['shape']
        drive.close(doc)
    ok('the box kinds keep their versions (3, 4, 5, 6) and carry no outline')


def recorded_blob():
    """BLOB as the app records it: every anchor spelled out (PathShape.subpathsJSON)."""
    out = []
    for subpath in BLOB:
        anchors = []
        for anchor in subpath['anchors']:
            if isinstance(anchor, list):
                anchor = {'x': anchor[0], 'y': anchor[1], 'in': anchor, 'out': anchor,
                          'smooth': False}
            anchors.append(anchor)
        out.append({'closed': subpath['closed'], 'anchors': anchors})
    return out


def take_actions(drive):
    step = {'tool': 'add_shape_layer', 'note': 'Paths panel ▸ Convert to Shape Layer',
            'arguments': {'kind': 'path', 'subpaths': recorded_blob(), 'fill_rule': 'evenodd',
                          'fill_rgba': [0.2, 0.4, 0.8, 1], 'stroke_rgba': [0, 0, 0, 1],
                          'stroke_width': 3, 'transform': [1, 0, 0, 1]}}
    drive.call('save_action', name='path-shape', steps=[step], overwrite=True)
    played = drive.new('rgb8')
    ran = drive.call('run_action', document_id=played, name='path-shape')
    assert ran.get('ok') is True, ran
    direct = drive.new('rgb8')
    drive.call('add_shape_layer', document_id=direct, kind='path', subpaths=BLOB,
               fill_rule='evenodd', fill_rgba=[0.2, 0.4, 0.8, 1], stroke_rgba=[0, 0, 0, 1],
               stroke_width=3)
    assert differing(drive.render(played), drive.render(direct)) == (0, 0)
    drive.close(played, direct)
    reply = drive.app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
        'name': 'path-shape-relative', 'overwrite': True,
        'steps': [dict(step, geometry={'mode': 'relative'})],
        'recorded_canvas': {'width': W, 'height': H}}})
    message = ' '.join(row.get('text', '') for row in reply.get('content', []))
    assert reply.get('isError') and 'supports absolute geometry only' in message, reply
    ok('a recorded path shape step replays to the same pixels; a relative geometry mode refuses')


TAKES = [take_round_trip, take_fill_oracle, take_fill_rule, take_by_name, take_rebox, take_size,
         take_refusals, take_versions, take_actions]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    parser.add_argument('--takes', default='', help='comma-separated take names (default: all)')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-path-shapes-'))).expanduser()
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
