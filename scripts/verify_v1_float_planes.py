#!/usr/bin/env python3
"""MCP and Actions drive for filters on a float plane.

In a 32-bit float document, `apply_filter` runs on one colour channel, the
layer's alpha, a layer mask and a saved alpha channel through the float
route on a one-channel plane (`float_plane_filter` in the core); the Camera
Raw Filter, which has no one-plane form, is refused by name; and a recorded
Action of two such steps replays bit for bit on a fresh copy.

The script owns its app instance (`verify_v1_16bit.App`): the Debug app on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s. It
terminates only that process. Every expected number is computed here, never
read back from the app; float pixels are read by 32-bit TIFF export
(`verify_phase11f_float.export32`). The scene's alphas are 1 and 0.5, so the
composite export reads the stored straight samples exactly.

Cases, on copies of `core/tests/fixtures/float_11f/scene.tiff` (48x32 linear
RGBA f32 with a block above white, a block with a negative component and a
half-alpha band):

- RED: `pixelate` block 4 on target red is the f64 cell mean of the stored
  red values, within T_REL relative; the above-white block's cells stay above
  white; green, blue and alpha stay bit for bit; the reply's storage is
  rgba32_float; the edit is one history step and undo restores the bits.
- ALPHA: `offset` (5, 3) with wrap on target alpha is an exact move of the
  stored alpha; RGB stays bit for bit.
- MASK: `offset` (7, -4) with wrap on a mask made from a feathered selection
  is an exact move of its u16 codes, read by `load_selection from:
  "layer_mask"`; the reply's storage is u16.
- CHANNEL: `pixelate` block 4 on a saved channel is
  `round(65535 * cell mean of code/65535)` within T_CODE codes.
- REFUSED: `develop_filter` on target red names the refusal and takes no step.
- ACTIONS: `start_recording`, the RED and MASK steps, `stop_recording`,
  `run_action` on a fresh copy carrying the same mask: the two documents'
  32-bit exports and masks are bit-identical.

Usage: python3 scripts/verify_v1_float_planes.py [--output DIR]
Prints `PASS <case>` per case and `ALL CASES PASS`, and exits 0, when every
case passed.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_16bit import App, cursor  # noqa: E402
from verify_phase11f_float import FIXTURE, HEIGHT, WIDTH, bits_equal, export32, scene  # noqa: E402

# ---- the tolerances, fixed here before anything is measured ----------------
T_REL = 1e-6          # RED: the f32 cell mean against the f64 oracle, relative
T_CODE = 1            # CHANNEL: codes of 65535
BLOCK = 4
ACTION = 'v1-float-planes-653'


# ------------------------------------------------------------- plumbing --

def fresh(app, folder, label):
    """A new float document: a private copy of the fixture, opened."""
    copy = folder / f'{label}.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    return app.call('open_document', path=str(copy))['document']['id']


def all_points():
    return [(x, y) for y in range(HEIGHT) for x in range(WIDTH)]


def coverage(app, document):
    """The selection's u16 coverage at every canvas pixel, row by row."""
    reply = app.call('get_selection', document_id=document,
                     points=[{'x': x, 'y': y} for x, y in all_points()])
    codes = [sample['coverage16'] for sample in reply['samples']]
    assert len(codes) == WIDTH * HEIGHT, len(codes)
    return codes


def feathered(app, document):
    """A feathered rectangle; returns its coverage."""
    app.call('select_rect', document_id=document, x=6, y=4, width=30, height=20)
    app.call('modify_selection', document_id=document, operation='feather', radius=2)
    codes = coverage(app, document)
    assert 0 in codes and 65535 in codes and any(0 < c < 65535 for c in codes), (
        'the selection has full, partial and empty samples')
    return codes


def masked(app, document):
    """Layer 0 gains a mask made from the feathered selection; returns its codes."""
    feathered(app, document)
    app.call('add_layer_mask', document_id=document, layer=0, kind='from_selection')
    app.call('deselect', document_id=document)
    return mask_codes(app, document)


def mask_codes(app, document):
    app.call('load_selection', **{'document_id': document, 'from': 'layer_mask', 'layer': 0})
    codes = coverage(app, document)
    app.call('deselect', document_id=document)
    return codes


def component(samples, c):
    return samples[c::4]


def f32(v):
    return struct.unpack('<f', struct.pack('<f', v))[0]


def moved(values, dx, dy):
    """`values` (row by row) offset by (dx, dy) with wrap."""
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            sx, sy = (x - dx) % WIDTH, (y - dy) % HEIGHT
            out.append(values[sy * WIDTH + sx])
    return out


def cell_means(values):
    """The f64 mean of each BLOCK x BLOCK cell from the top left, per pixel."""
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            cx, cy = x // BLOCK * BLOCK, y // BLOCK * BLOCK
            cells = [values[yy * WIDTH + xx]
                     for yy in range(cy, min(cy + BLOCK, HEIGHT))
                     for xx in range(cx, min(cx + BLOCK, WIDTH))]
            out.append(sum(cells) / len(cells))
    return out


def red_pixelate(app, document):
    return app.call('apply_filter', document_id=document, filter='pixelate', block=BLOCK,
                    target='red')


def mask_offset(app, document):
    return app.call('apply_filter', document_id=document, filter='offset', horizontal=7,
                    vertical=-4, undefined_areas='wrap', target='mask')


# ---------------------------------------------------------------- cases --

def red(app, folder):
    document = fresh(app, folder, 'red')
    base = scene()
    start = cursor(app, document)
    reply = red_pixelate(app, document)
    assert reply.get('ok') and reply.get('changed'), reply
    assert reply.get('storage') == 'rgba32_float', reply
    assert cursor(app, document) == start + 1, 'the filter is one history step'
    got = export32(app, document, folder / 'red.tiff')['samples']
    want = cell_means(component(base, 0))
    worst, above = 0.0, 0
    for i, (g, w) in enumerate(zip(component(got, 0), want)):
        worst = max(worst, abs(g - w) / max(abs(w), 1.0))
        if w > 1.0:
            assert g > 1.0, ('above white stays above white', i, g)
            above += 1
    assert worst <= T_REL and above >= 64, (worst, above)
    for c in (1, 2, 3):
        assert bits_equal(component(got, c), component(base, c)), ('kept', c)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    restored = export32(app, document, folder / 'red-undone.tiff')['samples']
    assert bits_equal(restored, base), 'undo restores the layer bit for bit'
    return dict(worst_relative=worst, above_white_pixels=above)


def alpha(app, folder):
    document = fresh(app, folder, 'alpha')
    base = scene()
    reply = app.call('apply_filter', document_id=document, filter='offset', horizontal=5,
                     vertical=3, undefined_areas='wrap', target='alpha')
    assert reply.get('ok') and reply.get('changed'), reply
    assert reply.get('storage') == 'rgba32_float', reply
    got = export32(app, document, folder / 'alpha.tiff')['samples']
    assert bits_equal(component(got, 3), moved(component(base, 3), 5, 3)), 'an exact move'
    for c in range(3):
        assert bits_equal(component(got, c), component(base, c)), ('RGB kept', c)
    return dict(moved=True)


def mask(app, folder):
    document = fresh(app, folder, 'mask')
    before = masked(app, document)
    start = cursor(app, document)
    reply = mask_offset(app, document)
    assert reply.get('ok') and reply.get('changed'), reply
    assert reply.get('storage') == 'u16', reply
    assert cursor(app, document) == start + 1, 'the filter is one history step'
    after = mask_codes(app, document)
    assert after == moved(before, 7, -4), 'an exact move of the mask codes'
    return dict(partial=sum(0 < c < 65535 for c in after))


def channel(app, folder):
    document = fresh(app, folder, 'channel')
    feathered(app, document)
    app.call('save_selection', document_id=document, name='F653')
    before = coverage(app, document)
    app.call('deselect', document_id=document)
    reply = app.call('apply_filter', document_id=document, filter='pixelate', block=BLOCK,
                     target='channel:F653')
    assert reply.get('ok') and reply.get('changed'), reply
    app.call('load_selection', document_id=document, **{'from': 'channel'}, channel='F653')
    after = coverage(app, document)
    want = [round(65535.0 * min(max(f32(m), 0.0), 1.0))
            for m in cell_means([f32(c / 65535.0) for c in before])]
    worst = max(abs(a - w) for a, w in zip(after, want))
    assert worst <= T_CODE, worst
    return dict(worst_codes=worst)


def refused(app, folder):
    document = fresh(app, folder, 'refused')
    start = cursor(app, document)
    text = app.refused('apply_filter',
                       'apply_filter "develop_filter" on target red is unavailable in a 32-bit '
                       'float document',
                       document_id=document, filter='develop_filter', target='red',
                       params={'exposure': 0.5})
    assert cursor(app, document) == start, 'a refused filter leaves no step'
    return dict(refusal=text[:160])


def actions(app, folder):
    first = fresh(app, folder, 'recorded')
    masked(app, first)
    app.call('start_recording', discard=True)
    red_pixelate(app, first)
    mask_offset(app, first)
    stopped = app.call('stop_recording', name=ACTION)
    other = fresh(app, folder, 'replayed')
    masked(app, other)
    replay = app.call('run_action', document_id=other, name=ACTION)
    recorded = export32(app, first, folder / 'recorded-out.tiff')['samples']
    replayed = export32(app, other, folder / 'replayed-out.tiff')['samples']
    assert bits_equal(recorded, replayed), 'the replay matches the recording bit for bit'
    assert not bits_equal(recorded, scene()), 'the recording changed the document'
    assert mask_codes(app, first) == mask_codes(app, other), 'the masks match'
    return dict(steps=stopped.get('steps'), replay=str(replay)[:200])


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-float-planes-'))
    output.mkdir(parents=True, exist_ok=True)

    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output) as app:
        run('RED', lambda: red(app, output))
        run('ALPHA', lambda: alpha(app, output))
        run('MASK', lambda: mask(app, output))
        run('CHANNEL', lambda: channel(app, output))
        run('REFUSED', lambda: refused(app, output))
        run('ACTIONS', lambda: actions(app, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
