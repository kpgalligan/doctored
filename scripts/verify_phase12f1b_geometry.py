#!/usr/bin/env python3
"""The Phase 12F-1b MCP drive: the EXACT canvas ops at 32-bit float, and the
successful no-op `canvas_size` at every depth.

Every expectation and every TOLERANCE below is fixed in this file BEFORE
anything is measured; a failed gate IS the receipt and nothing is relaxed
afterwards. ONE instance is owned by this file for every gate (`HostApp`: a
free port, a dummy API key and a PRIVATE directory for every preference
library the app reads) and is terminated by its own context manager; no
Doctored process this file did not start is ever signalled.

TOLERANCE: EXACT everywhere. A zero-angle crop keeps every allocation and
moves the window; a quarter turn and a flip are a Nearest PERMUTATION. No
sample is ever combined, so every comparison here is sample-for-sample
equality against this file's OWN crop/permutation of the pre-op render, and no
tolerance is granted.

Block E:

- **E1** `crop` (two windows), `rotate` 90/180/270/-90 and `flip`
  horizontal/vertical at 8 bits, 16 bits and 32-bit float: the render equals
  this file's own crop/permutation of the pre-op render.
- **E2** `get_document`'s guides, ruler origin and layer offsets equal the
  table computed here, and the 16-bit answer equals the 8-bit one.
- **E3** one `undo` and one `redo` for each of the three ops at each depth;
  a float `.rz` save -> reopen -> resave is SHA-256 identical.
- **E4** a recorded `crop` / `rotate` / `flip` replays through `run_action`
  on a fresh 32-bit float document.
- **E5** at float `image_size`, `crop` with `angle: 7.5` and a fractional
  `transform_layer` are ADMITTED, each as exactly ONE history step,
  and `get_precision`'s operator table carries no geometry refusal row and
  admits the filtered families.
- **E6** `get_precision` lists `crop`, `rotate`, `flip` and
  `image_size`, `transform_layer` and `distort_layer` at float.
- **E7** the no-op `canvas_size` in a REPLAY and in a BATCH: a two-step
  action `[canvas_size(W, H), flip]` runs to completion on a document ALREADY
  W x H at 8, 16 and float under the DEFAULT `on_error`, and the same action
  under the DEFAULT `stop_on_error` writes every file of a mixed-size folder.

EXIT CODES: 0 = every gate PASS; 1 = a failed assertion; 2 = INCOMPLETE (any
gate NOT RUN or NOT IMPLEMENTED). Exit 2 is never reported as a pass.

Nothing here derives an expectation from Doctored: every table, every
tolerance and every comparison below is written in this file.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

from pixels import write_png8
from support import HostApp
# The 12F-1 drive owns the shared vocabulary for this feature area; importing
# it is deliberate, so the two drives cannot describe one document two ways.
from verify_phase12f1_geometry import (CANVAS, FLOAT_FIXTURE, GUIDES, RULER,
                                       canvas_of, chrome, history, offsets,
                                       q4, refuses, render, seeded)

# ---- the expectations, fixed BEFORE any measurement ----------------------

# E1/E2/E3's crop windows, as `(x, y, trim_w, trim_h)`: the width/height are
# the canvas MINUS the trim, so one table serves canvases of two sizes.
CROP_WINDOWS = [('offset', (4, 3, 10, 7)), ('corner', (0, 0, 6, 4))]
ROTATIONS = [90, 180, 270, -90]
FLIPS = ['horizontal', 'vertical']

# E5's expectations (the filtered geometry is admitted at float).
FLOAT_FILTERED_FAMILIES = ('geometry_resize', 'geometry_straighten', 'geometry_rotate_canvas',
                           'geometry_affine', 'geometry_perspective', 'geometry_affine_set',
                           'geometry_perspective_set', 'mask_transform')

# E7's batch fixture: one input ALREADY the action's canvas (it sorts first,
# so the batch meets the no-op before anything else) and one of another size.
BATCH_OTHER = (30, 22)


# ------------------------------------------------- the independent oracle --

def window_crop(samples, old, window, channels=4):
    """This file's OWN crop: the rectangle `(x, y, w, h)` of `old`."""
    x, y, w, h = window
    out = []
    for j in range(h):
        row = (j + y) * old[0] + x
        out.extend(samples[row * channels:(row + w) * channels])
    return out


def permute(samples, old, op, channels=4):
    """This file's OWN permutation, written from the geometry rather than
    from the core: for each destination pixel, which source pixel it copies.
    `op` is one of rotate90/rotate180/rotate270/flip_h/flip_v."""
    w, h = old
    dw, dh = (h, w) if op in ('rotate90', 'rotate270') else (w, h)
    out = [0] * (dw * dh * channels)
    for y in range(dh):
        for x in range(dw):
            if op == 'rotate90':
                sx, sy = y, h - 1 - x
            elif op == 'rotate180':
                sx, sy = w - 1 - x, h - 1 - y
            elif op == 'rotate270':
                sx, sy = w - 1 - y, x
            elif op == 'flip_h':
                sx, sy = w - 1 - x, y
            elif op == 'flip_v':
                sx, sy = x, h - 1 - y
            else:
                raise AssertionError('no hand permutation for %s' % op)
            src = (sy * w + sx) * channels
            dst = (y * dw + x) * channels
            out[dst:dst + channels] = samples[src:src + channels]
    return out, (dw, dh)


def rotation_op(degrees):
    return {90: 'rotate90', 180: 'rotate180', 270: 'rotate270', -90: 'rotate270'}[degrees]


def cropped_guides(guides, window):
    """`doc_guide::cropped_guides`: shift by (-x, -y) and DROP what left."""
    x, y, w, h = window
    rank = {'horizontal': 0, 'vertical': 1}
    kept = []
    for orientation, position in guides:
        shift, extent = (x, w) if orientation == 'vertical' else (y, h)
        moved = q4(position - shift)
        if 0.0 <= moved <= extent:
            kept.append((orientation, moved))
    return sorted(kept, key=lambda g: (rank[g[0]], g[1]))


def cropped_origin(ruler, window):
    x, y, w, h = window
    return (min(max(q4(ruler[0] - x), 0.0), float(w)),
            min(max(q4(ruler[1] - y), 0.0), float(h)))


def turned_guides(guides, op, canvas):
    """`doc_guide::geometry_guides`, restated: a quarter turn exchanges a
    guide's ORIENTATION as well as its position, because the line it names
    has turned with the picture. `canvas` is the OLD canvas."""
    fw, fh = float(canvas[0]), float(canvas[1])
    rank = {'horizontal': 0, 'vertical': 1}
    out = []
    for orientation, position in guides:
        if op == 'rotate90':
            out.append(('horizontal', position) if orientation == 'vertical'
                       else ('vertical', fh - position))
        elif op == 'rotate180':
            out.append((orientation, (fw if orientation == 'vertical' else fh) - position))
        elif op == 'rotate270':
            out.append(('horizontal', fw - position) if orientation == 'vertical'
                       else ('vertical', position))
        elif op == 'flip_h':
            out.append(('vertical', fw - position) if orientation == 'vertical'
                       else ('horizontal', position))
        elif op == 'flip_v':
            out.append(('vertical', position) if orientation == 'vertical'
                       else ('horizontal', fh - position))
        else:
            raise AssertionError('no hand chrome for %s' % op)
    return sorted(((o, q4(p)) for o, p in out), key=lambda g: (rank[g[0]], g[1]))


def turned_origin(ruler, op, canvas):
    fw, fh = float(canvas[0]), float(canvas[1])
    ox, oy = ruler
    moved = {'rotate90': (fh - oy, ox),
             'rotate180': (fw - ox, fh - oy),
             'rotate270': (oy, fw - ox),
             'flip_h': (fw - ox, oy),
             'flip_v': (ox, fh - oy)}[op]
    new = (canvas[1], canvas[0]) if op in ('rotate90', 'rotate270') else canvas
    return (min(max(q4(moved[0]), 0.0), float(new[0])),
            min(max(q4(moved[1]), 0.0), float(new[1])))


# -------------------------------------------------------------- utilities --

def window_for(canvas, table):
    x, y, tw, th = table
    return (x, y, canvas[0] - tw, canvas[1] - th)


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def image_size(path):
    """The written file's dimensions, read by the PLATFORM rather than by
    Doctored — `sips`, so the check is independent of the app."""
    out = subprocess.run(['sips', '-g', 'pixelWidth', '-g', 'pixelHeight', str(path)],
                         capture_output=True, text=True, check=True).stdout
    values = {}
    for line in out.splitlines():
        parts = line.strip().split(':')
        if len(parts) == 2 and parts[0].strip() in ('pixelWidth', 'pixelHeight'):
            values[parts[0].strip()] = int(parts[1])
    assert len(values) == 2, out
    return (values['pixelWidth'], values['pixelHeight'])


def wait_for_batch(app, *, running, timeout=240):
    deadline = time.monotonic() + timeout
    while True:
        result = app.client.rpc('tools/call', {'name': 'batch_status', 'arguments': {}})
        if not result.get('isError'):
            text = ' '.join(c.get('text', '') for c in result.get('content', []))
            try:
                value = json.loads(text)
            except json.JSONDecodeError:
                value = {}
            if bool(value.get('running')) == running:
                return value
        assert time.monotonic() < deadline, 'the batch never reached running=%s' % running
        time.sleep(0.1)


def batch_fixture(path, width, height, seed):
    rgba = bytearray()
    for i in range(width * height):
        rgba += bytes(((i * 13 + seed) % 256, (i * 29 + 40) % 256, (i * 7 + 90) % 256, 255))
    write_png8(path, width, height, bytes(rgba))


# ------------------------------------------------------------------ gates --

def gate_e1(app, folder, receipts):
    findings = {}
    for depth in (8, 16, 32):
        if depth == 32 and not FLOAT_FIXTURE.exists():
            raise AssertionError('the committed float fixture is missing')
        per_depth = {}
        for name, table in CROP_WINDOWS:
            document = seeded(app, depth, folder, f'e1_{depth}_crop_{name}',
                              guides=False, channel=False, mask=False)
            old = canvas_of(app, document)
            window = window_for(old, table)
            before = render(app, document)
            assert (before['width'], before['height']) == old, (before['width'], old)
            app.call('crop', document_id=document, x=window[0], y=window[1],
                     width=window[2], height=window[3])
            after = render(app, document)
            assert (after['width'], after['height']) == (window[2], window[3]), \
                (depth, name, (after['width'], after['height']), window)
            assert after['depth'] == before['depth'], 'the render depth moved'
            assert after['samples'] == window_crop(before['samples'], old, window), \
                f'depth {depth} crop {name}: the render is not this file\'s own crop'
            per_depth[f'crop_{name}'] = {'from': list(old), 'window': list(window)}
        for degrees in ROTATIONS:
            document = seeded(app, depth, folder, f'e1_{depth}_rot_{degrees}',
                              guides=False, channel=False, mask=False)
            old = canvas_of(app, document)
            before = render(app, document)
            app.call('rotate', document_id=document, degrees=degrees)
            after = render(app, document)
            want, dims = permute(before['samples'], old, rotation_op(degrees))
            assert (after['width'], after['height']) == dims, (depth, degrees, dims)
            assert after['samples'] == want, \
                f'depth {depth} rotate {degrees}: not this file\'s own permutation'
            per_depth[f'rotate_{degrees}'] = {'from': list(old), 'to': list(dims)}
        for axis in FLIPS:
            document = seeded(app, depth, folder, f'e1_{depth}_flip_{axis}',
                              guides=False, channel=False, mask=False)
            old = canvas_of(app, document)
            before = render(app, document)
            app.call('flip', document_id=document, axis=axis)
            after = render(app, document)
            op = 'flip_h' if axis == 'horizontal' else 'flip_v'
            want, dims = permute(before['samples'], old, op)
            assert (after['width'], after['height']) == dims, (depth, axis, dims)
            assert after['samples'] == want, \
                f'depth {depth} flip {axis}: not this file\'s own permutation'
            per_depth[f'flip_{axis}'] = {'from': list(old), 'to': list(dims)}
        findings[str(depth)] = per_depth
    receipts['e1'] = findings
    print('PASS e1: crop, the three quarter turns and both flips equal the drive\'s own '
          'crop/permutation at 8, 16 and 32 bits', flush=True)


def gate_e2(app, folder, receipts):
    findings = {}
    answers = {}
    for depth in (8, 16, 32):
        per_depth = {}
        for name, op, arguments in [
                ('crop', 'crop', None),
                ('rotate90', 'rotate', {'degrees': 90}),
                ('flip_h', 'flip', {'axis': 'horizontal'})]:
            document = seeded(app, depth, folder, f'e2_{depth}_{name}',
                              channel=False, mask=False)
            old = canvas_of(app, document)
            before = offsets(app, document)
            if op == 'crop':
                window = window_for(old, CROP_WINDOWS[0][1])
                app.call('crop', document_id=document, x=window[0], y=window[1],
                         width=window[2], height=window[3])
                want_guides = cropped_guides(GUIDES, window)
                want_ruler = cropped_origin(RULER, window)
                want_offsets = [(n, x - window[0], y - window[1]) for n, x, y in before]
            else:
                app.call(op, document_id=document, **arguments)
                want_guides = turned_guides(GUIDES, name, old)
                want_ruler = turned_origin(RULER, name, old)
                # Every seeded document holds exactly one canvas-sized layer
                # at (0, 0), and each of these maps that rectangle onto the
                # new canvas's own origin.
                want_offsets = [(n, 0, 0) for n, _, _ in before]
            guides, ruler = chrome(app, document)
            assert guides == want_guides, (depth, name, guides, want_guides)
            assert ruler == want_ruler, (depth, name, ruler, want_ruler)
            assert offsets(app, document) == want_offsets, \
                (depth, name, offsets(app, document), want_offsets)
            per_depth[name] = {'guides': guides, 'ruler': list(ruler),
                               'offsets': offsets(app, document)}
        answers[depth] = per_depth
        findings[str(depth)] = per_depth
    assert answers[16] == answers[8], ('the 16-bit answer differs from the 8-bit one',
                                       answers)
    receipts['e2'] = findings
    print('PASS e2: guides, the ruler origin and every layer offset equal the drive\'s '
          'table, and the 16-bit answer equals the 8-bit one', flush=True)


def gate_e3(app, folder, receipts):
    findings = {}
    for depth in (8, 16, 32):
        per_depth = {}
        document = seeded(app, depth, folder, f'e3_{depth}',
                          guides=False, channel=False, mask=False)
        old = canvas_of(app, document)
        window = window_for(old, CROP_WINDOWS[0][1])
        for name, tool, arguments in [
                ('crop', 'crop', {'x': window[0], 'y': window[1],
                                  'width': window[2], 'height': window[3]}),
                ('rotate', 'rotate', {'degrees': 90}),
                ('flip', 'flip', {'axis': 'vertical'})]:
            before_canvas = canvas_of(app, document)
            before = render(app, document)
            cursor = history(app, document)
            app.call(tool, document_id=document, **arguments)
            moved_canvas = canvas_of(app, document)
            app.call('undo', document_id=document)
            assert canvas_of(app, document) == before_canvas, (depth, name, 'undo canvas')
            assert render(app, document)['samples'] == before['samples'], \
                (depth, name, 'undo did not restore the pixels')
            app.call('redo', document_id=document)
            assert canvas_of(app, document) == moved_canvas, (depth, name, 'redo')
            app.call('undo', document_id=document)
            assert history(app, document) == cursor, (depth, name, 'the history cursor moved')
            per_depth[name] = {'from': list(before_canvas), 'to': list(moved_canvas)}
        findings[str(depth)] = per_depth

    # The float `.rz` round trip, after a turn and a crop.
    document = seeded(app, 32, folder, 'e3_rz', guides=False, channel=False, mask=False)
    app.call('rotate', document_id=document, degrees=90)
    old = canvas_of(app, document)
    window = window_for(old, CROP_WINDOWS[0][1])
    app.call('crop', document_id=document, x=window[0], y=window[1],
             width=window[2], height=window[3])
    first = folder / 'e3_float_a.rz'
    second = folder / 'e3_float_b.rz'
    app.call('save_copy', document_id=document, path=str(first))
    reopened = app.call('open_document', path=str(first))['document']['id']
    assert canvas_of(app, reopened) == (window[2], window[3]), canvas_of(app, reopened)
    app.call('save_copy', document_id=reopened, path=str(second))
    left, right = sha256_of(first), sha256_of(second)
    assert left == right, (left, right)
    findings['float_rz'] = {'canvas': [window[2], window[3]], 'sha256': left}
    receipts['e3'] = findings
    print('PASS e3: one undo and one redo for each op at every depth; the float .rz '
          'round trip is byte-identical', flush=True)


def gate_e4(app, folder, receipts):
    document = seeded(app, 32, folder, 'e4', guides=False, channel=False, mask=False)
    app.call('start_recording', document_id=document)
    app.call('rotate', document_id=document, degrees=90)
    app.call('flip', document_id=document, axis='horizontal')
    # The quarter turn EXCHANGES the canvas axes, so the crop window is taken
    # from the canvas the crop actually meets, not from the one the document
    # started with.
    old = canvas_of(app, document)
    window = window_for(old, CROP_WINDOWS[0][1])
    app.call('crop', document_id=document, x=window[0], y=window[1],
             width=window[2], height=window[3])
    app.call('stop_recording', document_id=document, name='FloatExactGeometry12F1b')
    recorded = canvas_of(app, document)
    recorded_render = render(app, document)

    fresh = seeded(app, 32, folder, 'e4b', guides=False, channel=False, mask=False)
    report = app.call('run_action', document_id=fresh, name='FloatExactGeometry12F1b')
    assert report['ok'], report
    assert canvas_of(app, fresh) == recorded, (canvas_of(app, fresh), recorded)
    assert render(app, fresh)['samples'] == recorded_render['samples'], \
        'the replayed float document is not the recorded one'
    receipts['e4'] = {'recorded': list(recorded), 'replayed': list(canvas_of(app, fresh)),
                      'steps_run': report.get('steps_run')}
    print('PASS e4: a recorded rotate/flip/crop replays on a fresh float document',
          flush=True)


def gate_e5(app, folder, receipts):
    document = seeded(app, 32, folder, 'e5', guides=False, channel=False, mask=False)
    old = canvas_of(app, document)
    findings = {}
    for name, tool, arguments in [
            ('image_size', 'image_size', {'width': old[0] // 2, 'height': old[1] // 2}),
            ('straighten', 'crop', {'x': 2, 'y': 2, 'width': old[0] - 6,
                                    'height': old[1] - 6, 'angle': 7.5}),
            ('transform_layer', 'transform_layer', {'layer': 0, 'translate_x': 0.25,
                                                    'translate_y': 0.5,
                                                    'sampler': 'bilinear'})]:
        cursor = history(app, document)
        message = refuses(app, tool, document_id=document, **arguments)
        assert message is None, (name, 'must be admitted at 32-bit float', message)
        assert history(app, document) - cursor == 1, (name, 'an admitted call is ONE step')
        findings[name] = {'admitted': True, 'canvas': list(canvas_of(app, document))}
        app.call('undo', document_id=document)
        assert canvas_of(app, document) == old, (name, 'undo did not restore the canvas')

    reply = app.call('get_precision', document_id=document)
    operators = reply.get('operators') or {}
    rows = [row['operator'] for row in (operators.get('refused') or [])]
    assert not [row for row in rows if 'Image Size' in row or 'free transform' in row], rows
    admitted = set(operators.get('admitted') or [])
    for family in ('geometry_canvas_size', 'geometry_crop', 'geometry_rotate',
                   'geometry_flip') + FLOAT_FILTERED_FAMILIES:
        assert family in admitted, (family, sorted(admitted))
    findings['admitted_families'] = sorted(admitted & set(FLOAT_FILTERED_FAMILIES))
    receipts['e5'] = findings
    print('PASS e5: image_size, a straighten and a fractional transform_layer are admitted '
          'at float as one history step each, and no geometry row is refused', flush=True)


def gate_e6(app, folder, receipts):
    document = seeded(app, 32, folder, 'e6', guides=False, channel=False, mask=False)
    reply = app.call('get_precision', document_id=document)
    tools = set(reply.get('supported_tools') or []) | set(reply.get('shared_tools') or [])
    for tool in ('crop', 'rotate', 'flip'):
        assert tool in tools, (tool, sorted(tools))
    for tool in ('image_size', 'transform_layer', 'distort_layer'):
        assert tool in tools, (tool, 'a filtered op is listed at float')
    receipts['e6'] = {'lists': ['crop', 'rotate', 'flip', 'image_size', 'transform_layer',
                                'distort_layer']}
    print('PASS e6: get_precision lists crop, rotate, flip and the filtered ops at float',
          flush=True)


def gate_e7(app, folder, receipts):
    findings = {}
    # (a) the REPLAY, at all three depths, under the DEFAULT on_error.
    for depth in (8, 16, 32):
        document = seeded(app, depth, folder, f'e7_{depth}',
                          guides=False, channel=False, mask=False)
        size = canvas_of(app, document)
        name = f'NoOpCanvasFlip{depth}'
        app.call('save_action', name=name, overwrite=True, steps=[
            {'tool': 'canvas_size',
             'arguments': {'width': size[0], 'height': size[1], 'anchor': 'center'}},
            {'tool': 'flip', 'arguments': {'axis': 'horizontal'}},
        ])
        before = render(app, document)
        cursor = history(app, document)
        report = app.call('run_action', document_id=document, name=name)
        assert report['ok'], (depth, report)
        assert report.get('steps_run') == 2, (depth, report)
        assert not report.get('failures'), (depth, report)
        assert 'stopped_at' not in report, (depth, report)
        assert canvas_of(app, document) == size, (depth, 'the canvas moved')
        after = render(app, document)
        want, dims = permute(before['samples'], size, 'flip_h')
        assert dims == size, (depth, dims, size)
        assert after['samples'] == want, (depth, 'the replay is not this file\'s mirror')
        # ONE Undo restores: the no-op added no step of its own.
        app.call('undo', document_id=document)
        assert render(app, document)['samples'] == before['samples'], \
            (depth, 'one Undo did not restore the document')
        assert history(app, document) == cursor, (depth, 'the history cursor moved')
        findings[str(depth)] = {'steps_run': 2, 'canvas': list(size),
                                'one_undo_restores': True}

    # (b) the BATCH, under the DEFAULT stop_on_error, with the already-correct
    # file FIRST (`a_same.png` sorts ahead of `b_other.png`).
    inputs = folder / 'e7-input'
    outputs = folder / 'e7-output'
    inputs.mkdir(parents=True, exist_ok=True)
    outputs.mkdir(parents=True, exist_ok=True)
    batch_fixture(inputs / 'a_same.png', CANVAS[0], CANVAS[1], 1)
    batch_fixture(inputs / 'b_other.png', BATCH_OTHER[0], BATCH_OTHER[1], 2)
    started = app.call('run_batch', name='NoOpCanvasFlip8', input=str(inputs),
                       output=str(outputs), format='png', name_template='{name}')
    assert started['ok'] and started.get('batch_id'), started
    final = wait_for_batch(app, running=False)
    report = final['report']
    assert 'stopped' not in report, report
    rows = {row['input']: row for row in report['files']}
    assert set(rows) == {'a_same.png', 'b_other.png'}, sorted(rows)
    for key, row in rows.items():
        assert row['outcome'] == 'wrote', (key, row)
    written = {}
    for key in ('a_same.png', 'b_other.png'):
        path = outputs / key
        assert path.exists(), sorted(p.name for p in outputs.iterdir())
        written[key] = list(image_size(path))
        assert written[key] == [CANVAS[0], CANVAS[1]], (key, written[key])
    findings['batch'] = {'rows': {k: v['outcome'] for k, v in rows.items()},
                         'sizes': written, 'stopped': False}
    receipts['e7'] = findings
    print('PASS e7: a same-size canvas_size step SUCCEEDS inside a replay at 8, 16 and '
          'float, and a default-policy batch writes every file', flush=True)


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

    receipts = {'expectations': 'fixed in scripts/verify_phase12f1b_geometry.py before any '
                                'measurement; the tolerance is EXACT everywhere, because a '
                                'zero-angle crop keeps every allocation and a turn or a flip '
                                'is a Nearest permutation'}
    failures, not_run = [], []
    gates = [('e1', lambda: gate_e1(app, folder, receipts)),
             ('e2', lambda: gate_e2(app, folder, receipts)),
             ('e3', lambda: gate_e3(app, folder, receipts)),
             ('e4', lambda: gate_e4(app, folder, receipts)),
             ('e5', lambda: gate_e5(app, folder, receipts)),
             ('e6', lambda: gate_e6(app, folder, receipts)),
             ('e7', lambda: gate_e7(app, folder, receipts))]
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        for name, run in gates:
            try:
                run()
            except BaseException as error:  # a failed gate IS the receipt
                failures.append(name)
                receipts.setdefault(name, {})['failure'] = repr(error)
                print(f'FAIL {name}: {error!r}', flush=True)

    receipts['failures'] = failures
    receipts['not_run'] = not_run
    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    if failures:
        print(f'FAILED GATES: {failures}', flush=True)
        sys.exit(1)
    if not_run:
        print(f'INCOMPLETE: not run: {not_run}', flush=True)
        sys.exit(2)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
