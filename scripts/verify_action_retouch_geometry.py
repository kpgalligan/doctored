#!/usr/bin/env python3
"""MCP drive for relative and anchored geometry on the retouch steps and on Content-Aware Fill's
`source_mask` (ActionGeometry+Retouch.swift), against the Debug app.

Each case saves ONE recorded step (recorded on a 200 × 150 picture) with a geometry mode, plays it
with `run_action` on a resized copy of the same picture, and compares the render byte for byte with
a second copy on which this file calls the tool directly with coordinates it remapped itself:

- positions by axis (`v · W'/W`, or `f · W' + (v − f · W)` for an anchor), extents and deltas by
  axis under relative and unchanged under an anchor;
- a length with no axis (a brush size, a disc radius, a source-mask brush radius, a dab diameter)
  times `√(sx · sy)` under relative, clamped to the tool's range, unchanged under an anchor;
- each number quantized as the remap quantizes it (an integer stays an integer, rounded half away
  from zero; anything else to four decimals).

Every case also asserts that the replay changed the picture (its render differs from the untouched
copy) and that the mode mattered (an absolute replay of the same step renders differently), and
each new field kind has a SENSITIVITY twin: the direct call with that one field left unscaled must
render differently from the scaled direct call, so a remap that skipped the field could not pass.

Cases: brush_stroke (size now scales), clone_stamp and heal_stroke (source point), spot_heal_stroke,
dodge_burn, red_eye_auto (discs), patch_region and content_aware_move (polygon + delta),
content_aware_fill with a source_mask after a relative select_rect, remove_stroke with two
`strokes`; at 8 bits on a uniform 2× copy and a non-uniform 300 × 150 copy, the bottom-right
anchor on a subset, four of them at 16 bits, a pressure `native_stroke` brush stroke at 8 bits and
a 16-bit native heal stroke whose scaled dabs cross the 200 px ceiling (clamped). A pixel-brush
native stroke and a `remove_stroke` with `use_selection` still refuse a mode by name.

The app is this script's own child (`phase5_support.App`). Prints one PASS line per case and a FAIL
line for a case that raised; exits non-zero unless every line is PASS. `--cases` runs some.
"""
import argparse
import base64
import hashlib
import math
import os
from pathlib import Path
import struct
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
os.environ.setdefault('DR_BACKGROUND', '1')
from phase5_support import App  # noqa: E402
from verify_phase1 import png  # noqa: E402

RECORDED = (200, 150)
UNIFORM = (400, 300)
STRETCHED = (300, 150)
RESULTS = []
ANCHORS = {'bottom-right': (1.0, 1.0)}


def ok(label):
    RESULTS.append(True)
    print(f'PASS: {label}', flush=True)


# ---- the picture -------------------------------------------------------------------------------

def picture(width, height):
    """A texture defined in canvas FRACTIONS, so every size is the same picture: two diagonal
    ramps, a checker, and two red discs for the red-eye case. Two more red discs of 10 px sit at
    fixed offsets from the bottom-right corner — where a bottom-right anchor carries the recorded
    eyes — and coincide with the first two on the 200 × 150 recording."""
    pixels = bytearray()
    for y in range(height):
        v = (y + 0.5) / height
        for x in range(width):
            u = (x + 0.5) / width
            r = int(40 + 180 * ((7 * u + 3 * v) % 1))
            g = int(60 + 150 * abs(((5 * v - 2 * u) % 2) - 1))
            b = 90 + 80 * ((int(u * 12) + int(v * 9)) % 2)
            for cu, cv in ((0.3, 0.4), (0.7, 0.4)):
                if ((u - cu) / 0.05) ** 2 + ((v - cv) / 0.0667) ** 2 <= 1:
                    r, g, b = 220, 25, 35
            for dx, dy in ((140, 90), (60, 90)):
                if (x + 0.5 - (width - dx)) ** 2 + (y + 0.5 - (height - dy)) ** 2 <= 100:
                    r, g, b = 220, 25, 35
            pixels += bytes((r, g, b, 255))
    return bytes(pixels)


# ---- the remap, computed here ------------------------------------------------------------------

def quantize(value, like):
    if isinstance(like, int):
        return int(math.copysign(math.floor(abs(value) + 0.5), value))
    return math.copysign(math.floor(abs(value * 10000) + 0.5), value) / 10000


class Remap:
    def __init__(self, mode, live, anchor=None):
        self.mode, self.live, self.anchor = mode, live, anchor

    def raw(self, value, axis, position):
        rec, live = RECORDED[axis], self.live[axis]
        if self.mode == 'relative':
            return value * live / rec
        if not position:
            return None
        f = ANCHORS[self.anchor][axis]
        return f * live + (value - f * rec)

    def pos(self, value, axis):
        return quantize(self.raw(value, axis, True), value)

    def ext(self, value, axis):
        mapped = self.raw(value, axis, False)
        return value if mapped is None else quantize(mapped, value)

    def factor(self):
        return math.sqrt((self.live[0] / RECORDED[0]) * (self.live[1] / RECORDED[1]))

    def length(self, value, low=None, high=None):
        if self.mode != 'relative':
            return value
        scaled = value * self.factor()
        if low is not None:
            scaled = min(max(scaled, low), high)
        return quantize(scaled, value)

    def points(self, points):
        return [[self.pos(x, 0), self.pos(y, 1)] for x, y in points]

    def dabs(self, dabs, ceiling):
        out = []
        for x, y, d, o in dabs:
            rx, ry = self.raw(x, 0, True), self.raw(y, 1, True)
            scaled = d * (self.factor() if self.mode == 'relative' else 1)
            out.append((rx, ry, min(max(scaled, 1), ceiling), o))
        return out


def canonical(dabs, pixel=False):
    data = b''.join(struct.pack('<dddd', *dab) for dab in dabs)
    return {'version': 1, 'encoding': 'le_f64_xy_diameter_opacity',
            'dabs': base64.b64encode(data).decode(), 'hardness': 0.8, 'roundness': 1.0,
            'angle_degrees': 0.0, 'pixel': pixel}


# ---- the cases: (tool, recorded arguments, remap(args, R) -> direct arguments, sensitivities) ----
# A sensitivity is (label, remap(args, R) -> direct arguments with ONE field left unscaled).

def stroke_remap(low, high):
    def remap(a, r):
        return dict(a, points=r.points(a['points']), size=r.length(a['size'], low, high))
    return remap


def unscaled_size(low, high):
    def remap(a, r):
        return dict(stroke_remap(low, high)(a, r), size=a['size'])
    return remap


def source_remap(low, high):
    def remap(a, r):
        return dict(stroke_remap(low, high)(a, r), source_x=r.pos(a['source_x'], 0),
                    source_y=r.pos(a['source_y'], 1))
    return remap


def unmoved_source(low, high):
    def remap(a, r):
        return dict(stroke_remap(low, high)(a, r), source_x=a['source_x'],
                    source_y=a['source_y'])
    return remap


def region_remap(a, r):
    return dict(a, points=r.points(a['points']), offset_x=r.ext(a['offset_x'], 0),
                offset_y=r.ext(a['offset_y'], 1))


def discs_remap(a, r):
    return dict(a, eyes=[{'x': r.pos(e['x'], 0), 'y': r.pos(e['y'], 1),
                          'radius': r.length(e['radius'])} for e in a['eyes']])


def discs_unscaled_radius(a, r):
    return dict(a, eyes=[{'x': r.pos(e['x'], 0), 'y': r.pos(e['y'], 1), 'radius': e['radius']}
                         for e in a['eyes']])


def mask_remap(a, r, scale_radius=True):
    operations = []
    for op in a['source_mask']['operations']:
        if 'width' in op:
            operations.append(dict(op, x=r.pos(op['x'], 0), y=r.pos(op['y'], 1),
                                   width=r.ext(op['width'], 0), height=r.ext(op['height'], 1)))
        else:
            radius = r.length(op['radius'], 0.5, 4096) if scale_radius else op['radius']
            operations.append(dict(op, points=r.points(op['points']), radius=radius))
    return dict(a, source_mask=dict(a['source_mask'], operations=operations))


def strokes_remap(a, r, scale_size=True):
    return dict(a, strokes=[{'points': r.points(s['points']),
                             'size': r.length(s['size'], 1, 1000) if scale_size else s['size']}
                            for s in a['strokes']])


SELECT = {'x': 90, 'y': 50, 'width': 30, 'height': 30}


def select_remap(r):
    return {'x': r.pos(SELECT['x'], 0), 'y': r.pos(SELECT['y'], 1),
            'width': r.ext(SELECT['width'], 0), 'height': r.ext(SELECT['height'], 1)}


CASES = {
    'brush_stroke': ('brush_stroke', {
        'points': [[30.5, 40.25], [150.5, 110.75]], 'size': 12.5, 'color': '#204080',
        'hardness': 80, 'opacity': 1, 'flow': 100},
        stroke_remap(1, 5000), [('size', unscaled_size(1, 5000))]),
    'clone_stamp': ('clone_stamp', {
        'points': [[120.5, 90.5], [160.5, 100.5]], 'size': 14.5, 'source_x': 40.5,
        'source_y': 30.5, 'opacity': 1},
        source_remap(1, 5000), [('source point', unmoved_source(1, 5000)),
                                ('size', lambda a, r: dict(source_remap(1, 5000)(a, r),
                                                           size=a['size']))]),
    'heal_stroke': ('heal_stroke', {
        'points': [[100.5, 60.5], [140.5, 70.5]], 'size': 12.5, 'source_x': 50.5,
        'source_y': 100.5, 'opacity': 100},
        source_remap(3, 200), [('source point', unmoved_source(3, 200))]),
    'spot_heal_stroke': ('spot_heal_stroke', {
        'points': [[60.5, 100.5], [90.5, 104.5]], 'size': 10.5, 'opacity': 100, 'ring': 0,
        'seed': 0},
        stroke_remap(3, 200), []),
    'dodge_burn': ('dodge_burn', {
        'points': [[20.5, 120.5], [180.5, 20.5]], 'size': 16.5, 'exposure': 80,
        'range': 'midtones', 'burn': False},
        stroke_remap(1, 5000), [('size', unscaled_size(1, 5000))]),
    'red_eye_auto': ('red_eye_auto', {
        'eyes': [{'x': 60.0, 'y': 60.0, 'radius': 4.5}, {'x': 140.0, 'y': 60.0, 'radius': 4.5}]},
        discs_remap, [('radius', discs_unscaled_radius)]),
    'patch_region': ('patch_region', {
        'points': [[40.5, 90.5], [80.5, 90.5], [80.5, 130.5], [40.5, 130.5]],
        'offset_x': 70.5, 'offset_y': -40.5, 'direction': 'source'},
        region_remap, [('offset', lambda a, r: dict(a, points=r.points(a['points'])))]),
    'content_aware_move': ('content_aware_move', {
        'points': [[30.5, 30.5], [60.5, 30.5], [60.5, 60.5], [30.5, 60.5]],
        'offset_x': 100.5, 'offset_y': 50.5},
        region_remap, []),
    'content_aware_fill': ('content_aware_fill', {
        'source_mask': {'version': 1, 'initial': 'all', 'operations': [
            {'op': 'exclude_rect', 'x': 40.5, 'y': 20.5, 'width': 60.5, 'height': 50.5},
            {'op': 'exclude_brush', 'radius': 8.5,
             'points': [[130.5, 100.5], [170.5, 110.5]]}]},
        'seed': 0, 'ring': 0, 'sample': 'current'},
        mask_remap, [('source_mask', lambda a, r: a),
                     ('source_mask radius', lambda a, r: mask_remap(a, r, False))]),
    'remove_stroke': ('remove_stroke', {
        'strokes': [{'points': [[50.5, 50.5], [70.5, 52.5]], 'size': 9.5},
                    {'points': [[120.5, 100.5], [130.5, 110.5]], 'size': 7.5}],
        'seed': 0, 'mode': 'content_aware'},
        strokes_remap, [('strokes size', lambda a, r: strokes_remap(a, r, False))]),
}

NATIVE_DABS = [(30.5 + 12 * i, 40.25 + 7 * i, 8.0 + 1.5 * i, 1.0) for i in range(9)]
NATIVE = ('brush_stroke', {
    'points': [[30.5, 40.25], [126.5, 96.25]], 'size': 20.5, 'color': '#204080',
    'hardness': 80.0, 'opacity': 1.0, 'flow': 100.0, 'spacing': 25.0, 'angle': 0.0,
    'roundness': 100.0, 'native_stroke': canonical(NATIVE_DABS)})
HEAL_DABS = [(60.5 + 20 * i, 80.5, 150.0, 1.0) for i in range(4)]
NATIVE_HEAL = ('heal_stroke', {
    'points': [[60.5, 80.5], [120.5, 80.5]], 'size': 150.0, 'source_x': 40.5,
    'source_y': 30.5, 'opacity': 100.0, 'hardness': 80.0, 'spacing': 25.0, 'angle': 0.0,
    'roundness': 100.0, 'native_stroke': canonical(HEAL_DABS)})


def native_remap(ceiling, low):
    def remap(a, r):
        dabs = [struct.unpack_from('<dddd', base64.b64decode(a['native_stroke']['dabs']), i * 32)
                for i in range(len(base64.b64decode(a['native_stroke']['dabs'])) // 32)]
        out = dict(a, points=r.points(a['points']), size=r.length(a['size'], low, ceiling),
                   native_stroke=canonical(r.dabs(dabs, ceiling)))
        if 'source_x' in a:
            out.update(source_x=r.pos(a['source_x'], 0), source_y=r.pos(a['source_y'], 1))
        return out
    return remap


# ---- the drive ---------------------------------------------------------------------------------

class Drive:
    def __init__(self, app, folder):
        self.app, self.folder = app, folder
        self.files = {}
        self.untouched = {}

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def file(self, size):
        if size not in self.files:
            path = self.folder / f'picture-{size[0]}x{size[1]}.png'
            png(path, size[0], size[1], picture(*size))
            self.files[size] = path
        return self.files[size]

    def open(self, size, depth):
        doc = self.call('open_document', path=str(self.file(size)))['document']['id']
        if depth == 16:
            self.call('convert_precision', document_id=doc, mode='rgba16', flatten=True)
        return doc

    def digest(self, doc):
        reply = self.app.client.rpc('tools/call', {'name': 'render', 'arguments': {
            'document_id': doc, 'max_side': 4096}})
        assert not reply.get('isError'), reply
        data = next(c['data'] for c in reply['content'] if c['type'] == 'image')
        return hashlib.sha256(data.encode()).hexdigest()

    def close(self, doc):
        self.call('close_document', document_id=doc, discard_changes=True)

    def baseline(self, size, depth):
        key = (size, depth)
        if key not in self.untouched:
            doc = self.open(size, depth)
            self.untouched[key] = self.digest(doc)
            self.close(doc)
        return self.untouched[key]

    def save(self, name, steps):
        reply = self.app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
            'name': name, 'overwrite': True, 'steps': steps,
            'recorded_canvas': {'width': RECORDED[0], 'height': RECORDED[1]}}})
        text = ' '.join(c.get('text', '') for c in reply.get('content', []))
        assert not reply.get('isError'), f'save_action refused: {text}'

    def played(self, name, size, depth):
        doc = self.open(size, depth)
        ran = self.call('run_action', document_id=doc, name=name)
        assert ran.get('ok') is True, ran
        digest = self.digest(doc)
        self.close(doc)
        return digest

    def direct(self, calls, size, depth):
        doc = self.open(size, depth)
        for tool, arguments in calls:
            reply = self.call(tool, document_id=doc, **arguments)
            assert reply.get('ok') is not False, (tool, reply)
        digest = self.digest(doc)
        self.close(doc)
        return digest


def steps_for(tool, arguments, geometry, with_selection):
    steps = []
    if with_selection:
        steps.append({'tool': 'select_rect', 'arguments': dict(SELECT), 'geometry': geometry})
    steps.append({'tool': tool, 'arguments': dict(arguments, layer={'$layer': 'active'}),
                  'geometry': geometry})
    return steps


def run_case(drive, label, case, size, depth, geometry, sensitivities=True):
    tool, arguments, remap, twins = case
    mode = geometry['mode']
    r = Remap(mode, size, geometry.get('anchor'))
    selection = tool == 'content_aware_fill'
    name = f'retouch-{label}'
    drive.save(name, steps_for(tool, arguments, geometry, selection))
    played = drive.played(name, size, depth)
    prefix = [('select_rect', select_remap(r))] if selection else []
    direct = drive.direct(prefix + [(tool, remap(arguments, r))], size, depth)
    assert played == direct, f'{label}: the replay differs from the remapped direct call'
    assert played != drive.baseline(size, depth), f'{label}: the replay changed nothing'
    drive.save(name + '-absolute', steps_for(tool, arguments, {'mode': 'absolute'}, selection))
    absolute = drive.played(name + '-absolute', size, depth)
    assert absolute != played, f'{label}: the absolute replay renders the same as {mode}'
    checked = []
    if sensitivities:
        for field, unscaled in twins:
            twin = drive.direct(prefix + [(tool, unscaled(arguments, r))], size, depth)
            assert twin != direct, f'{label}: leaving {field} unscaled changes nothing'
            checked.append(field)
    return checked


def take(drive, label, case, size, depth, geometry, sensitivities=True):
    try:
        checked = run_case(drive, label, case, size, depth, geometry, sensitivities)
        ok(f'{label}: {geometry} on {size[0]}x{size[1]} at {depth} bits replays as the remapped '
           f'direct call, differs from the untouched and the absolute replay'
           + (f'; unscaled {", ".join(checked)} differs' if checked else ''))
    except Exception as error:  # one failed case must not hide the others
        RESULTS.append(False)
        print(f'FAIL: {label} raised {type(error).__name__} — {str(error)[:700]}', flush=True)


def refusal(drive, label, tool, arguments, sentence):
    try:
        reply = drive.app.client.rpc('tools/call', {'name': 'save_action', 'arguments': {
            'name': f'retouch-refusal-{label}', 'overwrite': True,
            'steps': [{'tool': tool, 'arguments': arguments, 'geometry': {'mode': 'relative'}}],
            'recorded_canvas': {'width': RECORDED[0], 'height': RECORDED[1]}}})
        text = ' '.join(c.get('text', '') for c in reply.get('content', []))
        assert reply.get('isError') and sentence in text, (label, text)
        ok(f'{label}: a relative mode is refused by name ({sentence!r})')
    except Exception as error:
        RESULTS.append(False)
        print(f'FAIL: {label} raised {type(error).__name__} — {str(error)[:700]}', flush=True)


def run(drive, wanted):
    relative = {'mode': 'relative'}
    anchor = {'mode': 'anchor', 'anchor': 'bottom-right'}

    def want(name):
        return not wanted or name in wanted

    for name, case in CASES.items():
        if want(name):
            take(drive, f'{name}-2x', case, UNIFORM, 8, relative)
    for name in ('brush_stroke', 'heal_stroke', 'red_eye_auto', 'content_aware_fill',
                 'remove_stroke'):
        if want(name):
            take(drive, f'{name}-stretched', CASES[name], STRETCHED, 8, relative,
                 sensitivities=False)
    for name in ('heal_stroke', 'patch_region', 'red_eye_auto', 'content_aware_fill'):
        if want(name):
            take(drive, f'{name}-anchor', CASES[name], UNIFORM, 8, anchor, sensitivities=False)
    for name in ('brush_stroke', 'clone_stamp', 'heal_stroke', 'dodge_burn'):
        if want(name):
            take(drive, f'{name}-16bit', CASES[name], UNIFORM, 16, relative, sensitivities=False)
    if want('native'):
        tool, arguments = NATIVE

        def unscaled_dabs(a, r):
            # Every dab centre remapped, every diameter left as recorded.
            moved = [(rx, ry, d, o) for (rx, ry, _scaled, _o), (_x, _y, d, o)
                     in zip(r.dabs(NATIVE_DABS, 5000), NATIVE_DABS)]
            return dict(native_remap(5000, 1)(a, r), native_stroke=canonical(moved))
        unscaled = ('dab diameters', unscaled_dabs)
        take(drive, 'native-brush-2x', (tool, arguments, native_remap(5000, 1), [unscaled]),
             UNIFORM, 8, relative)
        take(drive, 'native-brush-stretched', (tool, arguments, native_remap(5000, 1), []),
             STRETCHED, 8, relative, sensitivities=False)
    if want('native_heal'):
        tool, arguments = NATIVE_HEAL
        take(drive, 'native-heal-16bit-ceiling', (tool, arguments, native_remap(200, 3), []),
             UNIFORM, 16, relative, sensitivities=False)
    if want('refusals'):
        tool, arguments = NATIVE
        refusal(drive, 'pixel-native-stroke', tool,
                dict(arguments, native_stroke=canonical([(30.5, 40.5, 1.0, 1.0)], pixel=True)),
                'pixel-brush')
        refusal(drive, 'remove-use-selection', 'remove_stroke',
                {'use_selection': True, 'mode': 'content_aware'},
                'has no canvas coordinates without points or strokes')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    parser.add_argument('--cases', default='',
                        help='comma-separated case names (a CASES key, native, native_heal, '
                             'refusals); default all')
    arguments = parser.parse_args()
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-retouch-geometry-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    wanted = {name for name in arguments.cases.split(',') if name}
    with App(output / 'app', output / 'recovery', interval=600) as app:
        run(Drive(app, output), wanted)
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
