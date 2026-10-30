#!/usr/bin/env python3
"""MCP drive for v1 paths and vector masks, against the Debug app.

Each take opens its own documents and checks the tools against facts written here: where a
square's fill must land, which pixels a vector mask must hide, what a refusal must say and that it
left the document and its undo stack alone.

- **The list.** `set_path`, `rename_path`, `duplicate_path`, `delete_path`, `list_paths`: one undo
  step each, an echoed shape is no change, the list rules refuse by sentence.
- **Selections.** `path_to_selection` (hard, soft, combined) and `selection_to_path` (an ellipse
  traced into the work path and back within the trace tolerance).
- **Fill and stroke at 8 and 16 bits**, with `color_space: "document"`, inside a selection, both
  subpaths of a stroke as one undo step.
- **`stroke_path`'s refusals**: a subpath over the stroke's 10,000 points (by subpath index, with
  nothing painted), a camera RAW layer. Each leaves the picture and the undo count as they were.
- **`stroke_path` at 32-bit float**: `rgba32` paints above-white ink as its own numbers in
  one undo step, the Eraser erases, a blend mode outside the float set refuses by name.
- **Vector masks at 8, 16 and 32 bits**: set, read, the two flags, a move, a raster mask beside
  the shape, a stored path by name, Reveal All, a 90 degree rotation and Image Size.
- **Save and reopen.** Paths and a vector mask (with a raster mask beside it) come back from a
  `.rz` at 8 and 16 bits as the same list, the same shape and the same picture.
- **Actions.** A recorded Action holding path and vector-mask steps replays on a fresh document as
  one undo step. A second Action is saved with the steps the Pen, the Paths panel and Layer ▸
  Vector Mask record (`{"$layer": "active"}`, `path: "Work Path"`, `reveal_all`) and replays too.

The app is this script's own child and is ended in `finally`. Prints one PASS line per check
group and a FAIL line for a take that raised; exits non-zero unless every line is PASS.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import app_under_test  # noqa: E402
from phase3_fixtures import synthetic_dng  # noqa: E402
from verification_isolation import preset_environment  # noqa: E402
from mcp_client import remove_scratch, scratch_socket, wait_ready  # noqa: E402
from verify_phase1 import APP, Client, png  # noqa: E402

TOOLS = ['list_paths', 'set_path', 'rename_path', 'duplicate_path', 'delete_path',
         'path_to_selection', 'fill_path', 'stroke_path', 'selection_to_path',
         'get_vector_mask', 'set_vector_mask', 'set_vector_mask_properties',
         'remove_vector_mask']
WIDTH, HEIGHT = 128, 96
GRAY = '#C8C8C8'
RED = '#FF0000'
RAW_REFUSAL = 'Rasterize the RAW layer first'

RESULTS = []


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


def square(x0, y0, x1, y1):
    return {'closed': True, 'anchors': [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]}


def recorded_square(x0, y0, x1, y1):
    """The square as the app records a Pen shape: every anchor spelled out."""
    return {'closed': True, 'anchors': [
        {'x': x, 'y': y, 'in': [x, y], 'out': [x, y], 'smooth': False}
        for x, y in [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]}


def corners(reply):
    """The anchor points of a reply's first subpath."""
    return [[a['x'], a['y']] for a in reply['subpaths'][0]['anchors']]


class Session:
    """One open document and the calls a take makes on it."""

    def __init__(self, client, document):
        self.client = client
        self.document = document

    def call(self, tool_name, fail=False, **arguments):
        return self.client.call(tool_name, fail=fail, document_id=self.document, **arguments)

    def pixel(self, x, y):
        """The composite's colour at (x, y) as #RRGGBB, at any depth."""
        reply = self.call('sample_pixel', x=x, y=y)
        if 'hex' in reply:
            return reply['hex'][:7]
        red, green, blue = (round(v * 255) for v in reply['normalized_rgba'][:3])
        return f'#{red:02X}{green:02X}{blue:02X}'

    def undo_count(self):
        return self.call('list_history')['undo_count']

    def paths(self):
        return self.call('list_paths')['paths']

    def names(self):
        return [path['name'] for path in self.paths()]


class Drive:
    def __init__(self, client, output):
        self.client = client
        self.output = output
        self.copies = 0

    def plain(self):
        """A fresh 128 x 96 gray document. Every open gets a file of its own, because the app
        answers an open of a path it already has open with that same document."""
        self.copies += 1
        path = self.output / f'plain-{self.copies}.png'
        png(path, WIDTH, HEIGHT, bytes([200, 200, 200, 255]) * (WIDTH * HEIGHT))
        return Session(self.client, self.client.open(path))

    def masked(self):
        """A gray document with a red layer above it: where the layer's mask hides it, gray
        shows."""
        session = self.plain()
        session.call('new_layer', name='Red')
        session.call('fill_path', layer=1, color='#ff0000', subpaths=[square(0, 0, WIDTH, HEIGHT)])
        assert session.pixel(5, 5) == RED and session.pixel(60, 50) == RED
        return session


def take_catalog(drive):
    names = {tool['name'] for tool in drive.client.rpc('tools/list', {})['tools']}
    missing = [name for name in TOOLS if name not in names]
    assert not missing, missing
    ok('the thirteen path and vector-mask tools are in the catalog')


def take_list(drive):
    s = drive.plain()
    assert s.paths() == []
    made = s.call('set_path', subpaths=[square(10.6, 10.25, 60, 50)])
    assert made['changed'] and made['created'] and made['path'] == 'Work Path', made
    listed = s.call('list_paths')
    assert [p['name'] for p in listed['paths']] == ['Work Path'], listed
    assert listed['paths'][0]['kind'] == 'work' and listed['paths'][0]['anchors'] == 4
    assert listed['paths'][0]['subpaths'][0]['anchors'][0]['x'] == 10.6, listed
    assert listed['max_paths'] == 256
    assert s.call('get_document')['paths'] == 1
    steps = s.undo_count()
    same = s.call('set_path', subpaths=listed['paths'][0]['subpaths'])
    assert same['changed'] is False and s.undo_count() == steps, same
    s.call('undo')
    assert s.paths() == []
    s.call('redo')
    assert len(s.paths()) == 1
    ok('set_path creates the work path as one undo step; an echoed shape is no change')

    saved = s.call('set_path', name='Logo', fill_rule='evenodd',
                   subpaths=[square(20, 20, 100, 80), square(40, 35, 80, 65)])
    assert saved['created'] and saved['path'] == 'Logo' and saved['paths'] == 2, saved
    renamed = s.call('rename_path', path='Work Path', name='Box')
    assert renamed['saved'] and renamed['path'] == 'Box', renamed
    assert [p['kind'] for p in s.paths()] == ['saved', 'saved']
    assert s.call('duplicate_path', path='Logo')['path'] == 'Logo copy'
    assert s.call('duplicate_path', path='Logo')['path'] == 'Logo copy 2'
    deleted = s.call('delete_path', path='Logo copy 2')
    assert deleted['deleted'] == 'Logo copy 2' and deleted['paths'] == 3, deleted
    replaced = s.call('set_path', path='Logo copy', name='Mark', subpaths=[square(1, 1, 5, 5)])
    assert replaced['created'] is False and replaced['path'] == 'Mark', replaced
    assert s.names() == ['Box', 'Logo', 'Mark']
    steps = s.undo_count()
    s.call('undo')
    assert s.names() == ['Box', 'Logo', 'Logo copy']
    s.call('redo')
    assert s.names() == ['Box', 'Logo', 'Mark'] and s.undo_count() == steps
    ok('rename saves the work path; duplicate, delete and replace-and-rename edit the list')

    steps = s.undo_count()
    before = s.paths()
    message = s.call('rename_path', fail=True, path='Box', name='Logo')
    assert 'two paths are named' in message, message
    message = s.call('delete_path', fail=True, path='Nope')
    assert 'No path is named' in message and '"Logo"' in message, message
    message = s.call('set_path', fail=True, name='Bad', subpaths=[{'anchors': []}])
    assert 'at least one anchor' in message, message
    message = s.call('set_path', fail=True, name='Far', subpaths=[{'anchors': [[2e6, 0]]}])
    assert 'outside' in message, message
    assert s.undo_count() == steps and s.paths() == before
    ok('a broken list rule refuses in its own sentence and adds no undo step')


def take_selection(drive):
    s = drive.plain()
    s.call('set_path', name='Box', subpaths=[square(10.6, 10.25, 60, 50)])
    hard = s.call('path_to_selection', path='Box', anti_alias=False)
    assert hard['selection_empty'] is False, hard
    assert hard['bounds'] == {'x': 11, 'y': 10, 'width': 49, 'height': 40}, hard
    soft = s.call('path_to_selection', path='Box')
    assert soft['bounds'] == {'x': 10, 'y': 10, 'width': 50, 'height': 40}, soft
    grown = s.call('path_to_selection', subpaths=[square(100, 60, 120, 90)], mode='add')
    assert grown['bounds'] == {'x': 10, 'y': 10, 'width': 110, 'height': 80}, grown
    message = s.call('path_to_selection', fail=True, path='Box', subpaths=[square(1, 1, 2, 2)])
    assert 'not both' in message, message
    message = s.call('path_to_selection', fail=True)
    assert 'requires path' in message, message
    message = s.call('path_to_selection', fail=True, path='Box', feather=900)
    assert 'feather' in message, message
    ok('path_to_selection makes a hard, a soft and a combined selection')

    s.call('deselect')
    message = s.call('selection_to_path', fail=True)
    assert 'nothing is selected' in message, message
    s.call('select_ellipse', x=30, y=20, width=60, height=50)
    traced = s.call('selection_to_path', tolerance=1)
    assert traced['path'] == 'Work Path' and traced['subpaths'] == 1, traced
    assert 2 <= traced['anchors'] <= 16, traced
    assert s.paths()[0]['kind'] == 'work'
    bounds = s.call('path_to_selection', path='Work Path')['bounds']
    assert abs(bounds['x'] - 30) <= 2 and abs(bounds['width'] - 60) <= 3, bounds
    assert abs(bounds['y'] - 20) <= 2 and abs(bounds['height'] - 50) <= 3, bounds
    s.call('select_rect', x=10, y=10, width=40, height=30)
    assert s.call('selection_to_path', tolerance=1)['anchors'] == 4
    back = s.call('path_to_selection', path='Work Path')
    assert back['bounds'] == {'x': 10, 'y': 10, 'width': 40, 'height': 30}, back
    message = s.call('selection_to_path', fail=True, tolerance=40)
    assert '0.5 to 10' in message, message
    ok('selection_to_path traces an ellipse and a rectangle into the work path and back')


def fill_and_stroke(s, depth):
    """The same facts at either depth; `s` holds the saved paths Logo and Box."""
    steps = s.undo_count()
    filled = s.call('fill_path', path='Logo', color='#ff0000')
    assert filled['changed'] is True and filled['layer'] == 0, filled
    if depth == 16:
        assert filled['storage'] == 'rgba16_unorm', filled
    assert s.undo_count() == steps + 1
    assert s.pixel(25, 25) == RED, 'the ring is filled'
    assert s.pixel(60, 50) == GRAY, 'the even-odd hole is not'
    assert s.pixel(5, 5) == GRAY, 'nor the outside'
    message = s.call('fill_path', fail=True, path='Logo')
    assert 'requires color' in message, message
    nothing = s.call('fill_path', subpaths=[square(500, 500, 600, 600)], color='#00ff00')
    assert nothing['changed'] is False and 'covers no pixel' in nothing['note'], nothing
    in_document = s.call('fill_path', path='Mark', color='#102030', color_space='document')
    assert in_document['changed'] is True and s.pixel(3, 3) == '#102030', in_document
    s.call('select_rect', x=0, y=0, width=64, height=96)
    clipped = s.call('fill_path', path='Logo', color='#000000')
    assert clipped['changed'] is True, clipped
    assert s.pixel(25, 25) == '#000000' and s.pixel(90, 25) == RED, 'the selection confines'
    s.call('undo')
    assert s.pixel(25, 25) == RED
    s.call('deselect')
    ok(f'fill_path at {depth} bits fills by the rule, inside the selection, in document colour')

    steps = s.undo_count()
    stroked = s.call('stroke_path', path='Logo', color='#0000ff', size=3)
    assert stroked['changed'] is True and stroked['subpaths'] == 2, stroked
    assert stroked['points'] == 10 and stroked['tool'] == 'brush', stroked
    assert s.undo_count() == steps + 1, 'both subpaths are one step'
    assert s.pixel(20, 50) == '#0000FF' and s.pixel(40, 50) == '#0000FF'
    assert s.pixel(100, 50) == '#0000FF', "the closed subpath's closing side is stroked"
    s.call('undo')
    assert s.pixel(20, 50) == RED and s.pixel(40, 50) == GRAY, 'one undo takes both'
    erased = s.call('stroke_path', path='Logo', tool='eraser', size=5)
    assert erased['changed'] is True and erased['tool'] == 'eraser', erased
    s.call('undo')
    in_document = s.call('stroke_path', path='Box', color='#203040', color_space='document',
                         size=3)
    assert in_document['changed'] is True and s.pixel(30, 50) == '#203040', in_document
    s.call('undo')
    ok(f'stroke_path at {depth} bits strokes every subpath as one undo step')

    steps = s.undo_count()
    message = s.call('stroke_path', fail=True, path='Logo', tool='eraser', color='#fff')
    assert 'takes no color' in message, message
    message = s.call('stroke_path', fail=True, path='Logo', pressures=[1, 1])
    assert 'Versioned strokes' in message, message
    message = s.call('stroke_path', fail=True, path='Logo', points=[[1, 1]])
    assert 'takes no points' in message, message
    message = s.call('stroke_path', fail=True, path='Logo', tool='pencil')
    assert 'brush' in message, message
    zigzag = [[(i % 120) + 4, 10 + (i % 2) * 60] for i in range(10001)]
    message = s.call('stroke_path', fail=True, color='#00ff00',
                     subpaths=[square(5, 5, 20, 20), {'anchors': zigzag}])
    assert message.startswith('stroke_path, subpath 1:'), message
    assert 'Too many points' in message, message
    assert s.undo_count() == steps and s.pixel(5, 12) == GRAY, 'nothing was painted'
    ok(f'stroke_path at {depth} bits refuses a subpath over 10,000 points before it paints')


def paint_document(drive):
    s = drive.plain()
    s.call('set_path', name='Logo', fill_rule='evenodd',
           subpaths=[square(20, 20, 100, 80), square(40, 35, 80, 65)])
    s.call('set_path', name='Box', subpaths=[square(30, 10, 60, 50)])
    s.call('set_path', name='Mark', subpaths=[square(1, 1, 5, 5)])
    return s


def take_paint8(drive):
    fill_and_stroke(paint_document(drive), 8)


def take_paint16(drive):
    s = paint_document(drive)
    before = s.paths()
    s.call('convert_precision', mode='rgba16', structure='preserve')
    assert s.call('get_precision')['bits_per_channel'] == 16
    assert s.paths() == before, 'the paths survive the conversion'
    fill_and_stroke(s, 16)


def take_float(drive):
    scene = drive.output / 'scene.tiff'
    shutil.copyfile(ROOT / 'core/tests/fixtures/float_11f/scene.tiff', scene)
    s = Session(drive.client, drive.client.open(scene))
    info = s.call('get_document')
    width, height = info['width'], info['height']
    assert s.call('set_path', subpaths=[square(1, 1, width - 1, height - 1)])['changed']
    steps = s.undo_count()
    ink = [4.0, 0.5, 0.25, 1.0]
    stroked = s.call('stroke_path', path='Work Path', rgba32=ink, size=7)
    assert stroked['changed'] is True and stroked['tool'] == 'brush', stroked
    assert stroked['storage'] == 'rgba32_float', stroked
    assert s.undo_count() == steps + 1, 'the stroke is one undo step'
    # The square's left side runs down x = 1: a 7 px hard stroke covers column 1 whole.
    sample = s.call('sample_color', x=1, y=height // 2)
    assert all(abs(got - want) <= 1e-5 * max(1.0, want)
               for got, want in zip(sample['center_rgba32'], ink)), sample
    erased = s.call('stroke_path', path='Work Path', tool='eraser', size=7)
    assert erased['changed'] is True and erased['tool'] == 'eraser', erased
    s.call('undo')
    s.call('undo')
    assert s.undo_count() == steps
    message = s.call('stroke_path', fail=True, path='Work Path', color='#ffffff',
                     blend_mode='Screen')
    assert 'Screen' in message and '32-bit float' in message, message
    message = s.call('stroke_path', fail=True, path='Work Path', tool='eraser', rgba32=ink)
    assert 'takes no rgba32' in message, message
    assert s.undo_count() == steps
    ok('32-bit float: stroke_path paints above-white ink and erases; a mode outside the '
       'float set refuses by name')
    filled = s.call('fill_path', path='Work Path', color='#808080')
    assert filled['changed'] is True and filled['storage'] == 'rgba32_float', filled
    selected = s.call('path_to_selection', path='Work Path')
    assert selected['selection_empty'] is False, selected
    made = s.call('set_vector_mask', layer=0, subpaths=[square(1, 1, width - 1, height - 1)])
    assert made['changed'] and made['created'], made
    read = s.call('get_vector_mask', layer=0)
    assert read['vector_mask'] and read['anchors'] == 4, read
    assert s.call('remove_vector_mask', layer=0)['vector_mask'] is False
    ok('32-bit float: paths, fill_path, path_to_selection and a vector mask work')


def take_raw(drive):
    source = drive.output / 'capture.dng'
    synthetic_dng(source)
    s = Session(drive.client, drive.client.open(source))
    assert s.call('set_path', subpaths=[square(40, 40, 200, 160)])['changed']
    steps = s.undo_count()
    before = s.pixel(40, 100)
    message = s.call('stroke_path', fail=True, path='Work Path', color='#ff0000', size=9)
    assert RAW_REFUSAL in message, message
    message = s.call('fill_path', fail=True, path='Work Path', color='#ff0000')
    assert RAW_REFUSAL in message, message
    assert s.undo_count() == steps and s.pixel(40, 100) == before
    ok('stroke_path and fill_path refuse a camera RAW layer; the document is unchanged')


def take_mask8(drive):
    s = drive.masked()
    none = s.call('get_vector_mask', layer=1)
    assert none['vector_mask'] is False and 'set_vector_mask' in none['note'], none
    steps = s.undo_count()
    made = s.call('set_vector_mask', layer=1, subpaths=[square(20, 20, 100, 80)])
    assert made['changed'] and made['created'] and made['vector_mask'], made
    assert made['enabled'] and made['linked'] and not made['has_raster_mask'], made
    assert s.undo_count() == steps + 1
    assert s.pixel(60, 50) == RED and s.pixel(5, 5) == GRAY and s.pixel(110, 50) == GRAY
    read = s.call('get_vector_mask', layer=1)
    assert corners(read) == [[20, 20], [100, 20], [100, 80], [20, 80]], read
    info = s.call('get_document')
    assert info['layers'][1]['vector_mask'] is True and info['layers'][1]['has_mask'], info
    assert info['layers'][0]['vector_mask'] is False, info
    same = s.call('set_vector_mask', layer=1, subpaths=read['subpaths'])
    assert same['changed'] is False and s.undo_count() == steps + 1, same
    s.call('undo')
    assert s.call('get_vector_mask', layer=1)['vector_mask'] is False and s.pixel(5, 5) == RED
    s.call('redo')
    assert s.call('get_vector_mask', layer=1)['vector_mask'] is True and s.pixel(5, 5) == GRAY
    ok('set_vector_mask hides the layer outside the shape as one undo step')

    steps = s.undo_count()
    message = s.call('set_vector_mask', fail=True, layer=1)
    assert 'requires path' in message and 'reveal_all' in message, message
    message = s.call('set_vector_mask', fail=True, layer=1, path='x',
                     subpaths=[square(1, 1, 2, 2)])
    assert 'not both' in message, message
    message = s.call('set_vector_mask', fail=True, layer=9, reveal_all=True)
    assert 'out of range' in message, message
    message = s.call('set_vector_mask_properties', fail=True, layer=0, enabled=False)
    assert 'has no vector mask' in message, message
    message = s.call('remove_vector_mask', fail=True, layer=0)
    assert 'has no vector mask' in message, message
    message = s.call('remove_layer_mask', fail=True, layer=1)
    assert 'vector mask with no raster mask' in message, message
    assert s.undo_count() == steps
    ok('the vector-mask refusals name the fix and add no undo step')

    off = s.call('set_vector_mask_properties', layer=1, enabled=False)
    assert off['changed'] and off['enabled'] is False and s.pixel(5, 5) == RED, off
    assert s.call('set_vector_mask_properties', layer=1, enabled=False)['changed'] is False
    on = s.call('set_vector_mask_properties', layer=1, enabled=True)
    assert on['enabled'] is True and s.pixel(5, 5) == GRAY, on
    s.call('set_layer_properties', index=1, offset_x=10, offset_y=5)
    moved = s.call('get_vector_mask', layer=1)
    assert corners(moved) == [[30, 25], [110, 25], [110, 85], [30, 85]], moved
    assert s.pixel(25, 50) == GRAY and s.pixel(105, 50) == RED
    s.call('undo')
    s.call('set_vector_mask_properties', layer=1, linked=False)
    s.call('set_layer_properties', index=1, offset_x=10, offset_y=5)
    assert corners(s.call('get_vector_mask', layer=1))[0] == [20, 20], 'unlinked: it stays'
    s.call('undo')
    s.call('undo')
    assert s.call('get_vector_mask', layer=1)['linked'] is True
    ok('a vector mask disables, enables, and moves with its layer only while linked')

    s.call('add_layer_mask', layer=1, kind='reveal_all')
    both = s.call('get_vector_mask', layer=1)
    assert both['has_raster_mask'] and both['raster_mask_enabled'], both
    # On a mask the eraser paints black, whatever the colour.
    s.call('eraser_stroke', layer=1, target='mask', size=20, hardness=100,
           points=[[50, 50], [70, 50]])
    assert corners(s.call('get_vector_mask', layer=1)) == [[20, 20], [100, 20], [100, 80],
                                                           [20, 80]]
    assert s.pixel(60, 50) == GRAY, 'the raster mask hides what was erased'
    assert s.pixel(30, 30) == RED and s.pixel(5, 5) == GRAY
    s.call('brush_stroke', layer=1, target='mask', size=20, hardness=100,
           points=[[5, 50], [70, 50]])
    assert s.pixel(60, 50) == RED and s.pixel(10, 50) == GRAY, 'the shape still clips'
    s.call('undo')
    s.call('remove_layer_mask', layer=1)
    alone = s.call('get_vector_mask', layer=1)
    assert alone['vector_mask'] and alone['has_raster_mask'] is False, alone
    assert s.pixel(60, 50) == RED and s.pixel(5, 5) == GRAY
    ok('a raster mask beside the shape is painted and removed; the shape stays')

    s.call('set_path', name='Clip', subpaths=[square(40, 30, 90, 70)])
    named = s.call('set_vector_mask', layer=1, path='Clip')
    assert named['changed'] and named['created'] is False, named
    assert corners(named) == [[40, 30], [90, 30], [90, 70], [40, 70]], named
    assert s.pixel(30, 25) == GRAY and s.pixel(60, 50) == RED
    opened = s.call('set_vector_mask', layer=0, reveal_all=True)
    assert opened['created'] and opened['reveal_all'] and opened['anchors'] == 0, opened
    gone = s.call('remove_vector_mask', layer=0)
    assert gone['vector_mask'] is False and gone['raster_mask_kept'] is False, gone
    ok('set_vector_mask takes a stored path by name, and reveal_all')

    s.call('rotate', degrees=90)
    turned = s.call('get_vector_mask', layer=1)
    # (x, y) -> (96 - y, x) on the 96 x 128 canvas.
    want = sorted([[96 - y, x] for x, y in [[40, 30], [90, 30], [90, 70], [40, 70]]])
    assert sorted(corners(turned)) == want, turned
    assert s.pixel(45, 60) == RED and s.pixel(10, 10) == GRAY
    s.call('undo')
    s.call('image_size', width=256, height=192)
    grown = s.call('get_vector_mask', layer=1)
    assert corners(grown) == [[80, 60], [180, 60], [180, 140], [80, 140]], grown
    assert s.pixel(120, 100) == RED and s.pixel(79, 100) == GRAY
    assert s.pixel(81, 100) == RED, "the edge is the shape's own, not a resampled mask's"
    s.call('undo')
    ok('a vector mask follows a 90 degree rotation and Image Size as a shape')


def take_mask16(drive):
    s = drive.masked()
    s.call('set_vector_mask', layer=1, subpaths=[square(40, 30, 90, 70)])
    s.call('convert_precision', mode='rgba16', structure='preserve')
    assert s.call('get_precision')['bits_per_channel'] == 16
    kept = s.call('get_vector_mask', layer=1)
    assert kept['vector_mask'], kept
    assert corners(kept) == [[40, 30], [90, 30], [90, 70], [40, 70]], kept
    assert s.pixel(60, 50) == RED and s.pixel(30, 25) == GRAY
    ok('a vector mask crosses the conversion to 16 bits as a shape')

    steps = s.undo_count()
    reshaped = s.call('set_vector_mask', layer=1, subpaths=[square(20, 20, 100, 80)])
    assert reshaped['changed'] and reshaped['created'] is False, reshaped
    assert s.undo_count() == steps + 1 and s.pixel(30, 25) == RED and s.pixel(5, 5) == GRAY
    off = s.call('set_vector_mask_properties', layer=1, enabled=False)
    assert off['enabled'] is False and s.pixel(5, 5) == RED, off
    s.call('set_vector_mask_properties', layer=1, enabled=True)
    s.call('add_layer_mask', layer=1, kind='hide_all')
    assert s.call('get_vector_mask', layer=1)['has_raster_mask']
    assert s.pixel(60, 50) == GRAY, 'a hide-all raster mask hides everything'
    steps = s.undo_count()
    s.call('brush_stroke', layer=1, target='mask', size=20, hardness=100,
           points=[[5, 50], [70, 50]])
    assert s.undo_count() == steps + 1
    assert s.pixel(60, 50) == RED and s.pixel(10, 50) == GRAY, 'the shape still clips'
    assert s.pixel(60, 30) == GRAY, 'the rest of the hide-all mask still hides'
    assert corners(s.call('get_vector_mask', layer=1)) == [[20, 20], [100, 20], [100, 80],
                                                           [20, 80]]
    s.call('undo')
    s.call('remove_layer_mask', layer=1)
    assert s.pixel(60, 50) == RED
    s.call('rotate', degrees=90)
    want = sorted([[96 - y, x] for x, y in [[20, 20], [100, 20], [100, 80], [20, 80]]])
    assert sorted(corners(s.call('get_vector_mask', layer=1))) == want
    s.call('undo')
    removed = s.call('remove_vector_mask', layer=1)
    assert removed['vector_mask'] is False and removed['has_mask'] is False, removed
    assert s.pixel(5, 5) == RED
    ok('the vector-mask tools set, flag, paint beside, rotate and remove at 16 bits')


def take_reopen(drive, depth):
    s = drive.masked()
    # Converted first: a vector mask with a raster mask beside it crosses a depth conversion as
    # the plain mask it baked.
    if depth == 16:
        s.call('convert_precision', mode='rgba16', structure='preserve')
    s.call('set_path', subpaths=[square(3.5, 4.25, 30, 20)])
    s.call('set_path', name='Logo', fill_rule='evenodd',
           subpaths=[square(20, 20, 100, 80), square(40, 35, 80, 65)])
    s.call('set_vector_mask', layer=1, subpaths=[square(20, 20, 100, 80)])
    s.call('add_layer_mask', layer=1, kind='reveal_all')
    s.call('eraser_stroke', layer=1, target='mask', size=20, hardness=100,
           points=[[50, 50], [70, 50]])
    s.call('set_vector_mask_properties', layer=1, linked=False)
    native = drive.output / f'paths{depth}.rz'
    s.call('save_copy', path=str(native), format='rz')
    back = Session(drive.client, drive.client.open(native))
    assert back.call('get_precision')['bits_per_channel'] == depth
    assert back.paths() == s.paths() and back.names() == ['Work Path', 'Logo'], back.paths()
    mine, theirs = s.call('get_vector_mask', layer=1), back.call('get_vector_mask', layer=1)
    for key in ['vector_mask', 'enabled', 'linked', 'has_raster_mask', 'raster_mask_enabled',
                'fill_rule', 'subpaths']:
        assert theirs[key] == mine[key], (key, theirs, mine)
    assert theirs['linked'] is False and theirs['has_raster_mask'] is True, theirs
    for x, y in [(5, 5), (30, 30), (60, 50), (110, 50), (99, 79)]:
        assert back.pixel(x, y) == s.pixel(x, y), (x, y)
    assert back.pixel(60, 50) == GRAY and back.pixel(30, 30) == RED and back.pixel(5, 5) == GRAY
    # It came back as a vector mask, not as the mask it baked: the shape still edits.
    edited = back.call('set_vector_mask', layer=1, subpaths=[square(10, 10, 60, 60)])
    assert edited['changed'] and edited['created'] is False, edited
    assert back.call('get_vector_mask', layer=1)['has_raster_mask'] is True
    ok(f'paths and a vector mask with its raster mask survive a .rz save and reopen at {depth} '
       'bits')


def take_reopen8(drive):
    take_reopen(drive, 8)


def take_reopen16(drive):
    take_reopen(drive, 16)


def take_recorded(drive):
    s = drive.masked()
    drive.client.call('start_recording')
    try:
        s.call('set_active_layer', index=0)
        s.call('set_path', name='Logo', fill_rule='evenodd',
               subpaths=[square(20, 20, 100, 80), square(40, 35, 80, 65)])
        s.call('fill_path', path='Logo', color='#00ff00', layer=0)
        s.call('stroke_path', path='Logo', color='#0000ff', size=3, layer=0)
        s.call('path_to_selection', path='Logo')
        s.call('selection_to_path', tolerance=1)
        s.call('deselect')
        s.call('list_paths')
        s.call('set_vector_mask', layer=1, path='Logo')
        s.call('get_vector_mask', layer=1)
        s.call('set_vector_mask_properties', layer=1, linked=False)
    finally:
        recorded = drive.client.call('stop_recording', name='v1-paths-recorded')
    tools = [step['tool'] for step in recorded['steps']]
    assert tools == ['set_active_layer', 'set_path', 'fill_path', 'stroke_path',
                     'path_to_selection', 'selection_to_path', 'deselect', 'set_vector_mask',
                     'set_vector_mask_properties'], tools
    ok('the path and vector-mask tools record as their own steps; the two reads record nothing')

    t = drive.masked()
    steps = t.undo_count()
    ran = t.call('run_action', name='v1-paths-recorded')
    assert ran.get('ok') is True, ran
    assert t.names() == ['Work Path', 'Logo'], t.names()
    mask = t.call('get_vector_mask', layer=1)
    assert mask['vector_mask'] and mask['linked'] is False and mask['fill_rule'] == 'evenodd', mask
    # The red layer shows in the ring only; the hole and the outside show the background, which
    # the fill and the stroke painted.
    assert t.pixel(30, 30) == RED and t.pixel(60, 50) == GRAY and t.pixel(5, 5) == GRAY
    assert t.undo_count() == steps + 1
    t.call('set_layer_properties', index=1, visible=False)
    assert t.pixel(30, 30) == '#00FF00' and t.pixel(20, 50) == '#0000FF'
    assert t.pixel(60, 50) == GRAY
    t.call('undo')
    t.call('undo')
    assert t.paths() == [] and t.call('get_vector_mask', layer=1)['vector_mask'] is False
    assert t.pixel(30, 30) == RED and t.undo_count() == steps
    ok('the recorded Action replays on a fresh document as one undo step')


def take_ui_steps(drive):
    """The steps the Pen, the Paths panel and Layer > Vector Mask record, replayed. No MCP call
    can press those controls, so the Action is written with the arguments their code records
    (EditorViewController+Paths.swift, +VectorMasks.swift, ImageDocument+Paths.swift)."""
    active = {'$layer': 'active'}
    brush = {'tool': 'brush', 'size': 3, 'hardness': 100, 'flow': 100, 'spacing': 25,
             'angle': 0, 'roundness': 100, 'opacity': 1, 'blend_mode': 'Normal'}

    def step(tool_name, note, **arguments):
        return {'tool': tool_name, 'arguments': arguments, 'note': note}

    panel = [
        step('set_path', 'Pen tool', subpaths=[recorded_square(20, 20, 100, 80)],
             fill_rule='nonzero'),
        step('rename_path', 'Paths panel: rename a path', path='Work Path', name='Outline'),
        step('duplicate_path', 'Paths panel ▸ Duplicate Path', path='Outline',
             name='Outline copy'),
        step('set_path', 'Pen tool', path='Outline copy', fill_rule='nonzero',
             subpaths=[recorded_square(40, 30, 90, 70)]),
        step('fill_path', 'Paths panel ▸ Fill Path', path='Outline', layer=active,
             target='layer', color='#00ff00'),
        step('stroke_path', 'Paths panel ▸ Stroke Path', path='Outline', layer=active,
             target='layer', color='#0000ff', **brush),
        step('path_to_selection', 'Paths panel ▸ Make Selection…', path='Outline copy',
             mode='replace', feather=0, anti_alias=True),
        step('selection_to_path', 'Select ▸ Make Work Path…', tolerance=2),
        step('deselect', 'Select ▸ Deselect'),
        step('set_vector_mask', 'Layer ▸ Vector Mask ▸ Current Path', path='Outline copy',
             layer=active),
        step('set_vector_mask_properties', 'Layer ▸ Vector Mask ▸ Enable', layer=active,
             enabled=False),
        step('set_vector_mask_properties', 'Layer ▸ Vector Mask ▸ Link', layer=active,
             linked=False),
        step('set_vector_mask', 'Pen tool', layer=active, fill_rule='nonzero',
             subpaths=[recorded_square(30, 25, 95, 75)]),
        step('delete_path', 'Paths panel ▸ Delete Path', path='Outline copy'),
    ]
    drive.client.call('save_action', name='v1-paths-panel', steps=panel)
    for depth in (8, 16):
        t = drive.plain()
        if depth == 16:
            t.call('convert_precision', mode='rgba16', structure='preserve')
        steps = t.undo_count()
        ran = t.call('run_action', name='v1-paths-panel')
        assert ran.get('ok') is True, ran
        assert t.names() == ['Work Path', 'Outline'], t.names()
        work = t.paths()[0]
        xs = [a['x'] for a in work['subpaths'][0]['anchors']]
        ys = [a['y'] for a in work['subpaths'][0]['anchors']]
        assert abs(min(xs) - 40) <= 3 and abs(max(xs) - 90) <= 3, work
        assert abs(min(ys) - 30) <= 3 and abs(max(ys) - 70) <= 3, work
        mask = t.call('get_vector_mask', layer=0)
        assert mask['vector_mask'] and mask['enabled'] is False and mask['linked'] is False, mask
        assert corners(mask) == [[30, 25], [95, 25], [95, 75], [30, 75]], mask
        # The mask is disabled, so the fill and the stroke show whole.
        assert t.pixel(60, 50) == '#00FF00' and t.pixel(20, 50) == '#0000FF'
        assert t.pixel(5, 5) == GRAY
        assert t.undo_count() == steps + 1
        t.call('undo')
        assert t.paths() == [] and t.call('get_vector_mask', layer=0)['vector_mask'] is False
        assert t.pixel(60, 50) == GRAY
    ok('the steps the Pen, the Paths panel and Layer > Vector Mask record replay at 8 and 16 '
       'bits as one undo step')

    reveal = [
        step('set_vector_mask', 'Layer ▸ Vector Mask ▸ Reveal All', layer=active,
             reveal_all=True),
        step('set_vector_mask', 'Pen tool', layer=active, fill_rule='nonzero',
             subpaths=[recorded_square(20, 20, 100, 80)]),
    ]
    drive.client.call('save_action', name='v1-paths-reveal', steps=reveal)
    drive.client.call('save_action', name='v1-paths-delete', steps=[
        step('remove_vector_mask', 'Layer ▸ Vector Mask ▸ Delete', layer=active)])
    t = drive.masked()
    t.call('set_active_layer', index=1)
    assert t.call('run_action', name='v1-paths-reveal').get('ok') is True
    mask = t.call('get_vector_mask', layer=1)
    assert corners(mask) == [[20, 20], [100, 20], [100, 80], [20, 80]], mask
    assert t.pixel(60, 50) == RED and t.pixel(5, 5) == GRAY
    assert t.call('get_vector_mask', layer=0)['vector_mask'] is False
    assert t.call('run_action', name='v1-paths-delete').get('ok') is True
    assert t.call('get_vector_mask', layer=1)['vector_mask'] is False and t.pixel(5, 5) == RED
    again = t.call('run_action', name='v1-paths-delete')
    assert again.get('ok') is False and 'has no vector mask' in str(again), again
    ok('a recorded Reveal All, a Pen edit of the mask and Delete replay on the active layer; '
       'Delete with no vector mask fails in the tool\'s sentence')


def launch(output):
    app_under_test.require_fresh_app(APP)
    agent_socket = scratch_socket()
    folder = output / 'app'
    folder.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, DR_BACKGROUND='1', DR_AGENT_SOCKET=str(agent_socket),
                       ANTHROPIC_API_KEY='dummy', DR_AUTOMATION_ALERTS='record',
                       DR_WORKING_LIMIT_BYTES='2147483648',
                       DR_ACTIONS_DIR=str(folder / 'actions'),
                       DR_RECOVERY_DIR=str(folder / 'recovery'), **preset_environment(folder))
    log = (output / 'app.log').open('w')
    process = subprocess.Popen(
        [str(APP), '-ApplePersistenceIgnoreState', 'YES',
         '-RecoveryDiscoverySuppressed', 'YES', '-ColorWorkingSpace', 'srgb'],
        env=environment, stdout=log, stderr=subprocess.STDOUT)
    client = Client(agent_socket)
    try:
        wait_ready(client, process, 30, output / 'app.log')
        return process, client, log
    except BaseException:
        stop(process, log)
        remove_scratch(agent_socket)
        raise


def stop(process, log):
    """Ends the one process this script started."""
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    arguments = parser.parse_args()
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-v1-paths-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    takes = [take_catalog, take_list, take_selection, take_paint8, take_paint16, take_float,
             take_raw, take_mask8, take_mask16, take_reopen8, take_reopen16,
             take_recorded, take_ui_steps]
    process, client, log = launch(output)
    try:
        drive = Drive(client, output)
        for take in takes:
            try:
                take(drive)
            except Exception as error:  # one failed take must not hide the others
                RESULTS.append(False)
                name = getattr(take, '__name__', 'take')
                print(f'FAIL: {name} raised {type(error).__name__} — {str(error)[:900]} '
                      f'(files: {output})', flush=True)
    finally:
        stop(process, log)
        remove_scratch(client.socket_path)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
