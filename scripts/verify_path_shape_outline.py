#!/usr/bin/env python3
"""MCP drive for a path shape layer's outline as an editing target, against the Debug app:
`get_shape_layer` reads the outline back without an edit, and `edit_shape_layer` with `subpaths`
(or `path`) replaces it — the agent mirror of the Pen, Path Selection and Direct Selection on the
layer (ShapeOutlineTarget.swift), which record exactly that step.

- **Read back without an edit** in RGB 8, 16 and 32 bits, Grayscale, Lab and CMYK:
  `canvas_subpaths` equals the subpaths the layer was made from (each coordinate to four
  decimals, the path model's own rule), and the undo count, get_document and the render are
  unchanged by the call. A raster layer answers `shape` null; a rectangle carries no outline.
- **The edit against the path's own coverage**, the same six kinds: a curved, even-odd outline
  replaced by a moved and reshaped one renders the same pixels as `fill_path` of the new subpaths
  on a twin document (0 differing pixels; Lab within 1 of 255, as `verify_path_shape_layers.py`
  measures a plain rectangle there) and as a fresh `add_shape_layer` of them (0 differing); one
  undo step; the undo restores the pixels and the shape object exactly; the stroke and its style
  are kept; a `.doctored` save and reopen keeps the read-back and the pixels.
- **Under a transform** (RGB 8 and CMYK 8): the read-back is `origin + T·local` computed here from
  `origin`, `transform` and `subpaths`; after an edit it equals the sent canvas subpaths within
  1e-3. Bounds declared before measuring: under a 2× x-scale with an outline on a half-pixel grid
  the fill equals `fill_path` of the sent subpaths exactly (0 differing) and a fresh layer of the
  same local outline and transform exactly; under a 30° rotation of a polygon every pixel is
  within 1 of `fill_path`'s.
- **Actions**: the step a canvas gesture records (`layer`, `subpaths`, `fill_rule`) replays to the
  same pixels; a relative geometry mode refuses it.
- **Refusals** by sentence with the undo count unchanged: an outline on a rectangle, an outline
  with `w`, an empty outline, `path` with `subpaths`, a pixel-locked layer.

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


# A closed curved blob with a square hole (even-odd), on fractional coordinates.
BLOB = [
    {'closed': True, 'anchors': [smooth(60.25, 30.5, 30, 0), smooth(130.75, 80.25, 0, 30),
                                 smooth(60.5, 130.125, -30, 0), [20.5, 80.75]]},
    {'closed': True, 'anchors': [[50, 60], [90, 60], [90, 100], [50, 100]]},
]
# The new outline: BLOB moved by (17.25, 9.5) with its left corner pulled further left, and the
# hole squeezed — what a Path Selection move then a Direct Selection anchor drag leave.
NEW_BLOB = [
    {'closed': True, 'anchors': [smooth(77.5, 40, 30, 0), smooth(148, 89.75, 0, 30),
                                 smooth(77.75, 139.625, -30, 0), [24.25, 90.25]]},
    {'closed': True, 'anchors': [[67.25, 69.5], [100.25, 69.5], [107.25, 109.5], [67.25, 109.5]]},
]
# Polygons for the transformed takes: on a half-pixel grid, so T⁻¹ of a 2× x-scale keeps every
# local coordinate on a quarter-pixel grid (exact at four decimals).
POLY = [{'closed': True, 'anchors': [[40, 40], [120, 40], [130.5, 100], [50, 110.5]]}]
NEW_POLY = [{'closed': True, 'anchors': [[61, 52.5], [150.5, 47], [161, 121.5], [70.5, 130]]}]
SCALE = [2, 0, 0, 1]
TURN = 30
ROT = [math.cos(math.radians(TURN)), -math.sin(math.radians(TURN)),
       math.sin(math.radians(TURN)), math.cos(math.radians(TURN))]


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

    def document(self, doc):
        """get_document's layer rows: what a read could change."""
        return self.call('get_document', document_id=doc)['layers']

    def undo_count(self, doc):
        return self.call('list_history', document_id=doc)['undo_count']

    def shape(self, doc, layer):
        reply = self.call('get_shape_layer', document_id=doc, layer=layer)
        assert reply['ok'] is True and reply['layer'] == layer, reply
        return reply['shape']

    def save_reopen(self, doc, name):
        self.files += 1
        path = self.folder / f'{name}-{self.files}.doctored'
        self.call('save_copy', document_id=doc, path=str(path), format='doctored')
        return self.call('open_document', path=str(path))['document']['id']


def differing(first, second):
    """How many pixels two decoded renders differ at, and by how much at most."""
    assert first[:2] == second[:2], (first[:2], second[:2])
    worst, count = 0, 0
    for p, q in zip(first[2], second[2]):
        if p != q:
            count += 1
            worst = max(worst, max(abs(x - y) for x, y in zip(p, q)))
    return count, worst


# ---- the geometry, written out independently ---------------------------------------------------

def q4(value):
    """The path model's four-decimal rule (`style_json::q4_f64`)."""
    result = round(value * 1e4) / 1e4
    return 0.0 if result == 0 else result


def flat(subpaths):
    """Each subpath as (closed, [(x, y, in_x, in_y, out_x, out_y, smooth)]), from either form an
    argument may take ([x, y] corners or anchor objects)."""
    out = []
    for subpath in subpaths:
        anchors = []
        for anchor in subpath['anchors']:
            if isinstance(anchor, list):
                anchors.append((anchor[0], anchor[1], anchor[0], anchor[1], anchor[0], anchor[1],
                                False))
            else:
                point_in = anchor.get('in', [anchor['x'], anchor['y']])
                point_out = anchor.get('out', [anchor['x'], anchor['y']])
                anchors.append((anchor['x'], anchor['y'], point_in[0], point_in[1], point_out[0],
                                point_out[1], bool(anchor.get('smooth', False))))
        out.append((bool(subpath.get('closed', False)), anchors))
    return out


def worst_gap(first, second):
    """The largest coordinate difference between two outlines of the same structure; raises
    when the structure (subpaths, anchors, closed, smooth) differs."""
    one, two = flat(first), flat(second)
    assert len(one) == len(two), (len(one), len(two))
    worst = 0.0
    for (closed_a, anchors_a), (closed_b, anchors_b) in zip(one, two):
        assert closed_a == closed_b and len(anchors_a) == len(anchors_b), (closed_a, closed_b)
        for a, b in zip(anchors_a, anchors_b):
            assert a[6] == b[6], ('smooth', a, b)
            worst = max(worst, max(abs(x - y) for x, y in zip(a[:6], b[:6])))
    return worst


def quantized(subpaths):
    """`subpaths` with every coordinate to four decimals, as anchor objects."""
    return [{'closed': closed, 'anchors': [
        {'x': q4(a[0]), 'y': q4(a[1]), 'in': [q4(a[2]), q4(a[3])], 'out': [q4(a[4]), q4(a[5])],
         'smooth': a[6]} for a in anchors]} for closed, anchors in flat(subpaths)]


def placed_on_canvas(shape):
    """The outline where the layer draws it: origin + T·local for every point, T the wire's
    row-major [a, b, c, d] (x' = a·x + b·y, y' = c·x + d·y)."""
    a, b, c, d = shape['transform']
    ox, oy = shape['origin']['x'], shape['origin']['y']

    def at(x, y):
        return ox + a * x + b * y, oy + c * x + d * y

    out = []
    for closed, anchors in flat(shape['subpaths']):
        mapped = []
        for anchor in anchors:
            px, py = at(anchor[0], anchor[1])
            ix, iy = at(anchor[2], anchor[3])
            jx, jy = at(anchor[4], anchor[5])
            mapped.append({'x': px, 'y': py, 'in': [ix, iy], 'out': [jx, jy], 'smooth': anchor[6]})
        out.append({'closed': closed, 'anchors': mapped})
    return out


def fill_path_twin(drive, kind, subpaths, fill_rule):
    """A twin document: a new layer over white with `subpaths` filled black by fill_path."""
    twin = drive.new(kind)
    layer = drive.call('new_layer', document_id=twin)
    layer = layer.get('layer', layer.get('index'))
    drive.call('fill_path', document_id=twin, color='#000000', layer=layer, subpaths=subpaths,
               fill_rule=fill_rule)
    return twin


# ---- read back without an edit -----------------------------------------------------------------

def take_read_back(drive):
    for kind in KINDS:
        doc = drive.new(kind)
        made = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=BLOB,
                          fill_rule='evenodd', fill='#3366CC', stroke='#101010', stroke_width=3)
        layer = made['layer']
        steps, state, pixels = drive.undo_count(doc), drive.document(doc), drive.render(doc)
        shape = drive.shape(doc, layer)
        assert shape['kind'] == 'path' and shape['fill_rule'] == 'evenodd', shape
        gap = worst_gap(shape['canvas_subpaths'], quantized(BLOB))
        assert gap <= 1e-9, (kind, 'read-back', gap)
        assert worst_gap(placed_on_canvas(shape), shape['canvas_subpaths']) <= 1e-9, kind
        assert drive.undo_count(doc) == steps, (kind, 'get_shape_layer is not an edit')
        assert drive.document(doc) == state, (kind, 'get_document changed')
        assert drive.render(doc) == pixels, (kind, 'the render changed')
        if kind == 'rgb8':
            rect = drive.call('add_shape_layer', document_id=doc, kind='rect', x=10, y=10, w=40,
                              h=30, fill='#336699')['layer']
            boxed = drive.shape(doc, rect)
            assert boxed['kind'] == 'rect' and 'subpaths' not in boxed, boxed
            assert 'canvas_subpaths' not in boxed, boxed
            plain = drive.call('get_shape_layer', document_id=doc, layer=0)
            assert plain['shape'] is None and 'not a shape layer' in plain['note'], plain
        drive.close(doc)
    ok('get_shape_layer reads a path shape\'s outline back on the canvas, to four decimals, in '
       'RGB 8/16/32, Gray, Lab and CMYK, with no undo step and get_document and the render '
       'unchanged; a rectangle carries no outline and a raster layer answers shape null')


# ---- the edit against fill_path, a fresh layer, undo and reopen --------------------------------

def take_edit(drive):
    for kind in KINDS:
        # The fill alone, against fill_path and against a fresh layer of the new outline.
        one = drive.new(kind)
        layer = drive.call('add_shape_layer', document_id=one, kind='path', subpaths=BLOB,
                           fill_rule='evenodd', fill='#000000')['layer']
        steps = drive.undo_count(one)
        reply = drive.call('edit_shape_layer', document_id=one, layer=layer, subpaths=NEW_BLOB,
                           fill_rule='evenodd')
        assert reply.get('changed') is True, reply
        assert drive.undo_count(one) == steps + 1, (kind, 'one undo step')
        assert worst_gap(reply['shape']['canvas_subpaths'], quantized(NEW_BLOB)) <= 1e-9, kind
        twin = fill_path_twin(drive, kind, NEW_BLOB, 'evenodd')
        count, worst = differing(drive.render(one), drive.render(twin))
        if kind == 'lab8':
            assert worst <= 1, (kind, count, worst)
        else:
            assert count == 0, (kind, count, worst)
        fresh = drive.new(kind)
        drive.call('add_shape_layer', document_id=fresh, kind='path', subpaths=NEW_BLOB,
                   fill_rule='evenodd', fill='#000000')
        assert differing(drive.render(one), drive.render(fresh)) == (0, 0), (kind, 'fresh')
        assert drive.shape(one, layer)['origin'] == drive.shape(fresh, layer)['origin'], kind
        drive.close(one, twin, fresh)

        # Fill and stroke: the style is kept; undo restores; a reopen keeps both.
        doc = drive.new(kind)
        layer = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=BLOB,
                           fill_rule='evenodd', fill='#3366CC', stroke='#101010', stroke_width=5,
                           stroke_join='round', stroke_dash='dashed')['layer']
        before, pixels = drive.shape(doc, layer), drive.render(doc, layer=layer)
        edited = drive.call('edit_shape_layer', document_id=doc, layer=layer, subpaths=NEW_BLOB,
                            fill_rule='evenodd')['shape']
        # A CMYK layer's inks too (`fill_ink`, `stroke_ink`), which the colours re-separate from.
        for key in ('fill', 'stroke', 'stroke_width', 'stroke_join', 'stroke_dash',
                    'stroke_align', 'transform', 'ink', 'fill_ink', 'stroke_ink'):
            assert edited.get(key) == before.get(key), (kind, key, edited.get(key), before.get(key))
        after, moved = drive.shape(doc, layer), drive.render(doc, layer=layer)
        assert moved != pixels, (kind, 'the edit changed nothing')
        again = drive.save_reopen(doc, kind)
        assert drive.shape(again, layer) == after, (kind, 'reopen read-back')
        assert drive.render(again, layer=layer) == moved, (kind, 'reopen pixels')
        drive.close(again)
        drive.call('undo', document_id=doc)
        assert drive.shape(doc, layer) == before, (kind, 'undo read-back')
        assert drive.render(doc, layer=layer) == pixels, (kind, 'undo pixels')
        drive.close(doc)
        ok(f'{kind}: the new outline fills as fill_path of it'
           + (' within 1 of 255' if kind == 'lab8' else ', 0 differing pixels')
           + ', equals a fresh layer of it, is one undo step that undoes exactly, keeps the '
           'stroke and its style, and survives a .doctored reopen')


# ---- under a transform --------------------------------------------------------------------------

def transformed_case(drive, kind, transform, label, exact):
    doc = drive.new(kind)
    layer = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=POLY,
                       fill='#000000', transform=transform)['layer']
    shape = drive.shape(doc, layer)
    # The read-back is origin + T·local, computed here.
    assert worst_gap(shape['canvas_subpaths'], placed_on_canvas(shape)) <= 1e-6, (kind, label)
    reply = drive.call('edit_shape_layer', document_id=doc, layer=layer, subpaths=NEW_POLY)
    edited = drive.shape(doc, layer)
    assert edited['transform'] == shape['transform'], (kind, label, 'the map is kept')
    gap = worst_gap(edited['canvas_subpaths'], quantized(NEW_POLY))
    assert gap <= 1e-3, (kind, label, 'read-back after the edit', gap)
    assert worst_gap(placed_on_canvas(edited), edited['canvas_subpaths']) <= 1e-6, (kind, label)
    assert reply['shape']['canvas_subpaths'] == edited['canvas_subpaths'], (kind, label)
    twin = fill_path_twin(drive, kind, NEW_POLY, 'nonzero')
    count, worst = differing(drive.render(doc), drive.render(twin))
    found = {'fill_path': (count, worst)}
    if exact:
        assert count == 0, (kind, label, count, worst)
        # A fresh layer of the same local outline and map, placed at the edited origin.
        fresh = drive.new(kind)
        made = drive.call('add_shape_layer', document_id=fresh, kind='path',
                          subpaths=edited['subpaths'], fill='#000000', transform=transform)
        drive.call('edit_shape_layer', document_id=fresh, layer=made['layer'],
                   x=edited['origin']['x'], y=edited['origin']['y'])
        assert drive.shape(fresh, made['layer'])['subpaths'] == edited['subpaths'], (kind, label)
        found['fresh'] = differing(drive.render(doc), drive.render(fresh))
        assert found['fresh'] == (0, 0), (kind, label, found)
        drive.close(fresh)
    else:
        assert worst <= 1, (kind, label, count, worst)
    drive.close(doc, twin)
    return found


def take_transformed(drive):
    for kind in ('rgb8', 'cmyk8'):
        scaled = transformed_case(drive, kind, SCALE, '2x', exact=True)
        turned = transformed_case(drive, kind, ROT, '30deg', exact=False)
        ok(f'{kind}: under a 2× x-scale and a 30° rotation the read-back is origin + T·local, an '
           f'edit reads back the sent subpaths within 1e-3; the 2× fill equals fill_path and a '
           f'fresh layer exactly {scaled}, the rotated fill is within 1 of fill_path {turned}')


# ---- Actions -------------------------------------------------------------------------------------

def take_actions(drive):
    # The step `ActionSteps.editShapeOutline` records: layer, subpaths (every anchor spelled out,
    # `PathShape.subpathsJSON`), fill_rule.
    step = {'tool': 'edit_shape_layer', 'note': 'Direct Selection tool',
            'arguments': {'layer': 1, 'subpaths': quantized(NEW_BLOB), 'fill_rule': 'evenodd'}}
    drive.call('save_action', name='path-outline', steps=[step], overwrite=True)
    played, direct = drive.new('rgb8'), drive.new('rgb8')
    for doc in (played, direct):
        drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=BLOB,
                   fill_rule='evenodd', fill='#3366CC', stroke='#101010', stroke_width=3)
    ran = drive.call('run_action', document_id=played, name='path-outline')
    assert ran.get('ok') is True, ran
    drive.call('edit_shape_layer', document_id=direct, layer=1, subpaths=NEW_BLOB,
               fill_rule='evenodd')
    assert differing(drive.render(played), drive.render(direct)) == (0, 0)
    assert drive.shape(played, 1) == drive.shape(direct, 1)
    drive.close(played, direct)
    reply = drive.app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
        'name': 'path-outline-relative', 'overwrite': True,
        'steps': [dict(step, geometry={'mode': 'relative'})],
        'recorded_canvas': {'width': W, 'height': H}}})
    message = ' '.join(row.get('text', '') for row in reply.get('content', []))
    assert reply.get('isError') and 'supports absolute geometry only' in message, reply
    ok('the step a canvas gesture records replays to the same pixels and outline; a relative '
       'geometry mode refuses it')


# ---- refusals ------------------------------------------------------------------------------------

def take_refusals(drive):
    doc = drive.new('rgb8')
    drive.call('set_path', document_id=doc, name='Empty', subpaths=[])
    path = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=BLOB,
                      fill_rule='evenodd', fill='#000000')['layer']
    rect = drive.call('add_shape_layer', document_id=doc, kind='rect', x=10, y=10, w=40, h=30,
                      fill='#336699')['layer']
    steps = drive.undo_count(doc)
    cases = [
        (dict(layer=rect, subpaths=BLOB), 'subpaths goes with a path shape: a rect is placed'),
        (dict(layer=path, subpaths=NEW_BLOB, w=40), 'so w does not apply with subpaths or path'),
        (dict(layer=path, subpaths=NEW_BLOB, transform=SCALE),
         'so transform does not apply with subpaths or path'),
        (dict(layer=path, subpaths=[]), 'The path is empty'),
        (dict(layer=path, path='Empty', subpaths=BLOB), 'Pass path or subpaths, not both.'),
        (dict(layer=path, path='Nope'), 'No path is named "Nope"'),
    ]
    for arguments, sentence in cases:
        message = drive.refuse('edit_shape_layer', document_id=doc, **arguments)
        assert sentence in message, (arguments, message)
    drive.call('set_layer_lock', document_id=doc, layer=path, locks=['pixels'])
    steps = drive.undo_count(doc)
    message = drive.refuse('edit_shape_layer', document_id=doc, layer=path, subpaths=NEW_BLOB)
    assert 'lock' in message.lower(), message
    assert drive.undo_count(doc) == steps
    # The outline the layer already has: what an unchanged edit answers.
    drive.call('set_layer_lock', document_id=doc, layer=path, locks=[])
    steps = drive.undo_count(doc)
    same = drive.call('edit_shape_layer', document_id=doc, layer=path,
                      subpaths=drive.shape(doc, path)['canvas_subpaths'], fill_rule='evenodd')
    unchanged = (same.get('changed'), drive.undo_count(doc) - steps)
    drive.close(doc)
    ok('every outline refusal answers its sentence and adds no undo step; the outline the layer '
       f'already has answers (changed, undo steps added) = {unchanged}')


TAKES = [take_read_back, take_edit, take_transformed, take_actions, take_refusals]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    parser.add_argument('--takes', default='', help='comma-separated take names (default: all)')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-path-outline-'))).expanduser()
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
