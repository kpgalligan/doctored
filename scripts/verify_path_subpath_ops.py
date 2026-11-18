#!/usr/bin/env python3
"""MCP drive for per-subpath operations held on a path (`subpaths[i].op`,
`designs/v1-paths-held-subpaths.md`, HS-7), against the Debug app, in RGB 8, 16 and 32 bits.

- **Read-back**: `set_path` → `list_paths`, `set_vector_mask` → `get_vector_mask`, and
  `add_shape_layer` / `edit_shape_layer` (kind path) → `get_shape_layer` (`subpaths` and
  `canvas_subpaths`) carry every subpath's `op` back; a shape without operations reads back with no
  `op` key.
- **The four renderers**: each operation on two overlapping rectangles, through `fill_path`,
  `path_to_selection` then `fill_selection`, a vector mask over a black layer, and a path shape
  layer, renders the same pixels as `fill_path` of the combined polygon written out here by hand
  (0 differing pixels).
- **One-anchor subpaths** on all four renderers: [rectangle unite, [[50, 50]] unite, rectangle
  subtract] is the first less the second; [rectangle unite, [[50, 50]] intersect, rectangle unite]
  is the second alone; the one-anchor subpath last, or first as a unite, changes nothing (0
  differing each), and each reads back three subpaths with their `op`.
- `stroke_path` with operations paints what it paints without them (0 differing).
- **Refusals** by sentence with the undo count unchanged: a mixed and an unknown `op`, a stroked
  holding shape (added, and edited in both directions), an unbounded shape outline.
- **History and files**: one undo step each, undo restoring the read-back and the pixels; a
  `.doctored` save and reopen keeps them; a recorded `set_path`, `set_vector_mask` and
  `edit_shape_layer` step with `op` replays to the same pixels; `combine_paths` with
  `subpath_indices` on a holding path leaves a path with no `op` that fills the fold.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per check and a
FAIL line for a take that raised; exits non-zero unless every line is PASS. `--takes` runs some.
"""
import argparse
import base64
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402
from verify_phase2 import read_png  # noqa: E402

W, H = 200, 160
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
}
OPERATIONS = ['unite', 'subtract', 'intersect', 'exclude']
RESULTS = []


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def polygon(points, op=None):
    subpath = {'closed': True, 'anchors': [list(p) for p in points]}
    if op is not None:
        subpath['op'] = op
    return subpath


def rect(x0, y0, x1, y1, op=None):
    return polygon([(x0, y0), (x1, y0), (x1, y1), (x0, y1)], op)


def dot(op):
    return {'closed': False, 'anchors': [[50, 50]], 'op': op}


# Two overlapping rectangles on whole pixels; (50, 50) lies inside A and outside B.
A = (20, 20, 120, 100)
B = (70, 50, 170, 140)
# The combined regions, written out by hand as polygons that fill by nonzero.
A_LESS_B = polygon([(20, 20), (120, 20), (120, 50), (70, 50), (70, 100), (20, 100)])
B_LESS_A = polygon([(120, 50), (170, 50), (170, 140), (70, 140), (70, 100), (120, 100)])
HAND = {
    'unite': [polygon([(20, 20), (120, 20), (120, 50), (170, 50), (170, 140), (70, 140),
                       (70, 100), (20, 100)])],
    'subtract': [A_LESS_B],
    'intersect': [polygon([(70, 50), (120, 50), (120, 100), (70, 100)])],
    'exclude': [A_LESS_B, B_LESS_A],
}
# The one-anchor rows: the subpaths and the hand polygon each must equal.
DOT_ROWS = [
    ('middle unite', [rect(*A, 'unite'), dot('unite'), rect(*B, 'subtract')], [A_LESS_B]),
    ('middle intersect', [rect(*A, 'unite'), dot('intersect'), rect(*B, 'unite')], [rect(*B)]),
    ('last', [rect(*A, 'unite'), rect(*B, 'subtract'), dot('unite')], [A_LESS_B]),
    ('first', [dot('unite'), rect(*A, 'unite'), rect(*B, 'subtract')], [A_LESS_B]),
]
WHOLE = [rect(0, 0, W, H)]


def held(op):
    return [rect(*A, 'unite'), rect(*B, op)]


def ops_of(subpaths):
    return [subpath.get('op') for subpath in subpaths]


class Drive:
    def __init__(self, app, folder):
        self.app = app
        self.folder = folder
        self.files = 0

    def call(self, tool_name, **arguments):
        return self.app.call(tool_name, **arguments)

    def refuse(self, tool_name, **arguments):
        return self.app.client.call(tool_name, fail=True, **arguments)

    def new(self, kind):
        return self.call('new_document', width=W, height=H, background='white',
                         **KINDS[kind])['document']['id']

    def new_layer(self, doc):
        reply = self.call('new_layer', document_id=doc)
        return reply.get('layer', reply.get('index'))

    def close(self, *documents):
        for doc in documents:
            self.call('close_document', document_id=doc, discard_changes=True)

    def render(self, doc):
        reply = self.app.client.rpc('tools/call', {'name': 'render', 'arguments': {
            'document_id': doc, 'max_side': 4096}})
        assert not reply.get('isError'), reply
        data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
        return read_png(base64.b64decode(data))

    def undo_count(self, doc):
        return self.call('list_history', document_id=doc)['undo_count']

    def path(self, doc, name):
        return next(p for p in self.call('list_paths', document_id=doc)['paths']
                    if p['name'] == name)

    def shape(self, doc, layer):
        return self.call('get_shape_layer', document_id=doc, layer=layer)['shape']

    def save_reopen(self, doc, name):
        self.files += 1
        path = self.folder / f'{name}-{self.files}.doctored'
        self.call('save_copy', document_id=doc, path=str(path), format='doctored')
        return self.call('open_document', path=str(path))['document']['id']

    # ---- the four renderers, each in a fresh document over white -------------------------------

    def fill_path(self, kind, subpaths):
        doc = self.new(kind)
        layer = self.new_layer(doc)
        self.call('fill_path', document_id=doc, layer=layer, color='#000000', subpaths=subpaths)
        return doc

    def selection(self, kind, subpaths):
        doc = self.new(kind)
        layer = self.new_layer(doc)
        self.call('path_to_selection', document_id=doc, subpaths=subpaths)
        self.call('fill_selection', document_id=doc, layer=layer, color='#000000')
        return doc

    def vector_mask(self, kind, subpaths):
        doc = self.new(kind)
        layer = self.new_layer(doc)
        self.call('fill_path', document_id=doc, layer=layer, color='#000000', subpaths=WHOLE)
        self.call('set_vector_mask', document_id=doc, layer=layer, subpaths=subpaths)
        return doc

    def shape_layer(self, kind, subpaths):
        doc = self.new(kind)
        self.call('add_shape_layer', document_id=doc, kind='path', subpaths=subpaths,
                  fill='#000000')
        return doc

    def pixels(self, renderer, kind, subpaths):
        doc = renderer(kind, subpaths)
        try:
            return self.render(doc)
        finally:
            self.close(doc)


def differing(first, second):
    """How many pixels two decoded renders differ at, and by how much at most."""
    assert first[:2] == second[:2], (first[:2], second[:2])
    worst, count = 0, 0
    for p, q in zip(first[2], second[2]):
        if p != q:
            count += 1
            worst = max(worst, max(abs(x - y) for x, y in zip(p, q)))
    return count, worst


def renderers(drive):
    return [('fill_path', drive.fill_path), ('path_to_selection', drive.selection),
            ('vector mask', drive.vector_mask), ('shape layer', drive.shape_layer)]


# ---- read-back ----------------------------------------------------------------------------------

def take_read_back(drive):
    for kind in KINDS:
        doc = drive.new(kind)
        layer = drive.new_layer(doc)
        drive.call('fill_path', document_id=doc, layer=layer, color='#000000', subpaths=WHOLE)
        for op in OPERATIONS:
            sent = held(op)
            drive.call('set_path', document_id=doc, name='Held', subpaths=sent)
            assert ops_of(drive.path(doc, 'Held')['subpaths']) == ['unite', op], (kind, op)
            drive.call('set_vector_mask', document_id=doc, layer=layer, subpaths=sent)
            mask = drive.call('get_vector_mask', document_id=doc, layer=layer)
            assert ops_of(mask['subpaths']) == ['unite', op], (kind, op, 'vector mask')
        drive.call('set_path', document_id=doc, name='Plain', subpaths=[rect(*A), rect(*B)])
        assert ops_of(drive.path(doc, 'Plain')['subpaths']) == [None, None], (kind, 'plain path')
        drive.call('set_vector_mask', document_id=doc, layer=layer, subpaths=[rect(*A)])
        mask = drive.call('get_vector_mask', document_id=doc, layer=layer)
        assert ops_of(mask['subpaths']) == [None], (kind, 'plain vector mask')
        made = drive.call('add_shape_layer', document_id=doc, kind='path', subpaths=held('exclude'),
                          fill='#336699')['layer']
        shape = drive.shape(doc, made)
        assert ops_of(shape['subpaths']) == ['unite', 'exclude'], (kind, shape)
        assert ops_of(shape['canvas_subpaths']) == ['unite', 'exclude'], (kind, shape)
        edited = drive.call('edit_shape_layer', document_id=doc, layer=made,
                            subpaths=held('intersect'))['shape']
        assert ops_of(edited['canvas_subpaths']) == ['unite', 'intersect'], (kind, edited)
        assert ops_of(drive.shape(doc, made)['subpaths']) == ['unite', 'intersect'], kind
        drive.call('edit_shape_layer', document_id=doc, layer=made, subpaths=[rect(*A), rect(*B)])
        plain = drive.shape(doc, made)
        assert ops_of(plain['subpaths']) == [None, None], (kind, plain)
        assert ops_of(plain['canvas_subpaths']) == [None, None], (kind, plain)
        drive.close(doc)
    ok('set_path, set_vector_mask and add/edit_shape_layer read every subpath\'s op back through '
       'list_paths, get_vector_mask and get_shape_layer in RGB 8/16/32; a shape without '
       'operations reads back with no op key')


# ---- the four renderers against the hand polygon -----------------------------------------------

def take_renderers(drive):
    for kind in KINDS:
        found = {}
        for op in OPERATIONS:
            reference = drive.pixels(drive.fill_path, kind, HAND[op])
            for label, renderer in renderers(drive):
                result = differing(drive.pixels(renderer, kind, held(op)), reference)
                found[(op, label)] = result
                assert result == (0, 0), (kind, op, label, result)
        stroked = drive.new(kind)
        layer = drive.new_layer(stroked)
        drive.call('stroke_path', document_id=stroked, layer=layer, subpaths=held('subtract'),
                   size=5)
        plain = drive.new(kind)
        layer = drive.new_layer(plain)
        drive.call('stroke_path', document_id=plain, layer=layer, subpaths=[rect(*A), rect(*B)],
                   size=5)
        strokes = differing(drive.render(stroked), drive.render(plain))
        assert strokes == (0, 0), (kind, 'stroke_path', strokes)
        drive.close(stroked, plain)
        ok(f'{kind}: unite, subtract, intersect and exclude render through fill_path, '
           'path_to_selection, a vector mask and a path shape layer as fill_path of the hand '
           'polygon (0 differing pixels in all 16), and stroke_path with operations paints what '
           'it paints without them (0 differing)')


def take_one_anchor(drive):
    for kind in KINDS:
        for row, subpaths, hand in DOT_ROWS:
            reference = drive.pixels(drive.fill_path, kind, hand)
            for label, renderer in renderers(drive):
                doc = renderer(kind, subpaths)
                result = differing(drive.render(doc), reference)
                assert result == (0, 0), (kind, row, label, result)
                if label == 'fill_path':
                    drive.call('set_path', document_id=doc, name='Dot', subpaths=subpaths)
                    read = drive.path(doc, 'Dot')['subpaths']
                elif label == 'vector mask':
                    read = drive.call('get_vector_mask', document_id=doc, layer=1)['subpaths']
                elif label == 'shape layer':
                    read = drive.shape(doc, 1)['canvas_subpaths']
                else:
                    read = subpaths
                assert ops_of(read) == ops_of(subpaths), (kind, row, label, read)
                drive.close(doc)
        ok(f'{kind}: a one-anchor subpath in the middle (unite, intersect), last and first, on '
           'all four renderers, equals the hand polygon (0 differing) and reads back three '
           'subpaths with their op')


# ---- refusals -----------------------------------------------------------------------------------

def take_refusals(drive):
    doc = drive.new('rgb8')
    holding = drive.call('add_shape_layer', document_id=doc, kind='path',
                         subpaths=held('subtract'), fill='#000000')['layer']
    stroked = drive.call('add_shape_layer', document_id=doc, kind='path',
                         subpaths=[rect(*A), rect(*B)], fill='#000000', stroke='#ff0000',
                         stroke_width=3)['layer']
    mixed = [rect(*A, 'unite'), rect(*B)]
    unknown = [rect(*A, 'union'), rect(*B, 'unite')]
    cases = [
        ('set_path', dict(name='Mixed', subpaths=mixed), 'subpath 1 has no op'),
        ('set_path', dict(name='Unknown', subpaths=unknown),
         'subpath 0: op must be "unite", "subtract", "intersect" or "exclude".'),
        ('fill_path', dict(layer=0, color='#000000', subpaths=mixed), 'subpath 1 has no op'),
        ('add_shape_layer', dict(kind='path', subpaths=held('subtract'), fill='#000000',
                                 stroke='#ff0000', stroke_width=3), 'cannot take a stroke'),
        ('edit_shape_layer', dict(layer=holding, stroke='#ff0000', stroke_width=3),
         'cannot take a stroke'),
        ('edit_shape_layer', dict(layer=stroked, subpaths=held('intersect')),
         'cannot take a stroke'),
        ('add_shape_layer', dict(kind='path', subpaths=[rect(*A, 'subtract'), rect(*B, 'unite')],
                                 fill='#000000'), 'must be bounded'),
        ('edit_shape_layer', dict(layer=holding,
                                  subpaths=[rect(*A, 'subtract'), rect(*B, 'exclude')]),
         'must be bounded'),
    ]
    for tool, arguments, sentence in cases:
        steps = drive.undo_count(doc)
        message = drive.refuse(tool, document_id=doc, **arguments)
        assert sentence in message, (tool, arguments, message)
        assert drive.undo_count(doc) == steps, (tool, 'an undo step')
    drive.close(doc)
    ok('a mixed and an unknown op, a stroked holding shape (added, edited onto a holding outline, '
       'a holding outline edited onto a stroke) and an unbounded outline refuse by sentence with '
       'no undo step')


# ---- undo, reopen, Actions and combine ----------------------------------------------------------

def take_history(drive):
    for kind in KINDS:
        doc = drive.new(kind)
        layer = drive.new_layer(doc)
        drive.call('fill_path', document_id=doc, layer=layer, color='#000000', subpaths=WHOLE)
        shape = drive.call('add_shape_layer', document_id=doc, kind='path',
                           subpaths=[rect(*A), rect(*B)], fill='#336699')['layer']
        for tool, arguments, read in [
            ('set_path', dict(name='Held', subpaths=held('subtract')),
             lambda: drive.call('list_paths', document_id=doc)['paths']),
            ('set_vector_mask', dict(layer=layer, subpaths=held('intersect')),
             lambda: drive.call('get_vector_mask', document_id=doc, layer=layer)),
            ('edit_shape_layer', dict(layer=shape, subpaths=held('exclude')),
             lambda: drive.shape(doc, shape)),
        ]:
            before, pixels, steps = read(), drive.render(doc), drive.undo_count(doc)
            drive.call(tool, document_id=doc, **arguments)
            assert drive.undo_count(doc) == steps + 1, (kind, tool, 'one undo step')
            after = read()
            assert after != before, (kind, tool, 'nothing changed')
            drive.call('undo', document_id=doc)
            assert read() == before, (kind, tool, 'undo read-back')
            assert drive.render(doc) == pixels, (kind, tool, 'undo pixels')
            drive.call('redo', document_id=doc)
            assert read() == after, (kind, tool, 'redo read-back')
        paths = drive.call('list_paths', document_id=doc)['paths']
        mask = drive.call('get_vector_mask', document_id=doc, layer=layer)
        outline, pixels = drive.shape(doc, shape), drive.render(doc)
        again = drive.save_reopen(doc, kind)
        assert drive.call('list_paths', document_id=again)['paths'] == paths, (kind, 'reopen')
        assert drive.call('get_vector_mask', document_id=again, layer=layer) == mask, kind
        assert drive.shape(again, shape) == outline, (kind, 'reopen shape')
        assert drive.render(again) == pixels, (kind, 'reopen pixels')
        drive.close(doc, again)
        ok(f'{kind}: set_path, set_vector_mask and edit_shape_layer with op are one undo step '
           'each, undo restores the read-back and the pixels, and a .doctored reopen keeps both')


def take_actions(drive):
    steps = [
        {'tool': 'set_path', 'note': 'Path Selection ▸ Operation',
         'arguments': {'path': 'Held', 'subpaths': held('subtract')}},
        {'tool': 'set_vector_mask', 'note': 'Path Selection ▸ Operation',
         'arguments': {'layer': 1, 'subpaths': held('intersect')}},
        {'tool': 'edit_shape_layer', 'note': 'Path Selection ▸ Operation',
         'arguments': {'layer': 2, 'subpaths': held('exclude'), 'fill_rule': 'nonzero'}},
    ]
    drive.call('save_action', name='subpath-ops', steps=steps, overwrite=True)
    played, direct = drive.new('rgb8'), drive.new('rgb8')
    for doc in (played, direct):
        layer = drive.new_layer(doc)
        drive.call('fill_path', document_id=doc, layer=layer, color='#000000', subpaths=WHOLE)
        drive.call('add_shape_layer', document_id=doc, kind='path',
                   subpaths=[rect(*A), rect(*B)], fill='#336699')
        drive.call('set_path', document_id=doc, name='Held', subpaths=[rect(*A)])
    ran = drive.call('run_action', document_id=played, name='subpath-ops')
    assert ran.get('ok') is True, ran
    for step in steps:
        drive.call(step['tool'], document_id=direct, **step['arguments'])
    assert differing(drive.render(played), drive.render(direct)) == (0, 0)
    assert drive.path(played, 'Held') == drive.path(direct, 'Held')
    assert drive.shape(played, 2) == drive.shape(direct, 2)
    assert ops_of(drive.shape(played, 2)['subpaths']) == ['unite', 'exclude']
    drive.close(played, direct)
    ok('a recorded set_path, set_vector_mask and edit_shape_layer step with op replays to the same '
       'pixels and read-back')


def take_combine(drive):
    # The listed subpath takes the operation and the whole fold becomes the geometry: here
    # (A less B) united with C, where combining the rest's summed winding with C would keep B.
    c = (130, 10, 180, 40)
    for kind in KINDS:
        doc = drive.new(kind)
        drive.call('set_path', document_id=doc, name='Held',
                   subpaths=held('subtract') + [rect(*c, 'intersect')])
        drive.call('combine_paths', document_id=doc, path='Held', subpath_indices=[2],
                   operation='unite')
        result = drive.path(doc, 'Held')['subpaths']
        assert all('op' not in subpath for subpath in result), (kind, result)
        drive.close(doc)
        filled = drive.pixels(drive.fill_path, kind, result)
        hand = drive.pixels(drive.fill_path, kind, [A_LESS_B, rect(*c)])
        assert differing(filled, hand) == (0, 0), (kind, differing(filled, hand))
    ok('combine_paths with subpath_indices on a holding path gives the listed subpath the '
       'operation and leaves the fold as a path with no op (0 differing from the hand polygon), '
       'in RGB 8/16/32')


TAKES = [take_read_back, take_renderers, take_one_anchor, take_refusals, take_history,
         take_actions, take_combine]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    parser.add_argument('--takes', default='', help='comma-separated take names (default: all)')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-subpath-ops-'))).expanduser()
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
