#!/usr/bin/env python3
"""MCP drive for Edit > Auto-Blend Layers > Panorama (and its
Content-Aware Fill Transparent Areas, `fill`):
`auto_blend_layers` with `method: "panorama"`, end to end.

The script owns its app instance (`verify_v1_auto_blend.App`: the Debug app or
`DR_VERIFY_APP` on its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s from
`verification_isolation.preset_environment`). It terminates only that
process. Every expected number is computed here, never read back from the app,
except where a case says it compares the app's own samples before and after.

THE PICTURES. `verify_v1_auto_blend.scene`'s grey multi-octave value noise
(no fine noise) over an 800 x 300 plane, cut into three 400 x 300 crops at
x 0 / 200 / 400 (`left`, `middle`, `right`; two 200-column overlaps). The
exposure case's middle crop holds `round(0.8 * level)`. The align case cuts
two crops of the same plane at x 0 and x 60 (a 15% shift). The fill cases
(2b-1) place two 400 x 300 crops at (0, 0) and (200, 40) on a 600 x 340
transparent canvas, leaving two 200 x 40 transparent corners (16000 pixels):
flat grey 128 crops (`grey-a`, `grey-b`), or the scene over a 600 x 340
plane cut at rows 0..300 x columns 0..400 and rows 40..340 x columns 200..600
(`tex-a`, `tex-b`), so the overlap agrees.

BOUNDS, fixed before measuring and never widened:
- shares: the frames' shares sum to the covered fraction (1: the three crops
  cover the canvas) within 1e-9.
- gain: `g_middle * 0.8 / g_left` and `g_middle * 0.8 / g_right` within 1% of
  1 per channel.
- tones: over each overlap (a 10 x 10 grid of points), the composite is
  within a mean of 3 and at most 8 levels of the overlap's left frame's gained
  value (`g * level`).
- align: the canvas grows by the shift (460 x 300 within 3 px a side), the
  reply's `aligned.canvas` says so, one history step.
- fill (2b-1, fixed before measuring): `fill.covered_pixels` exactly 16000;
  the filled layer's samples on a 20 x 12 grid (x 5, 35, ..., y 5, 35, ...)
  have the frames' own full alpha; in the transparent corners each RGB sample
  is within 2 codes of the grey (x 257 at 16 bits, 0.01 at 32 bits), the grey
  being the left frame's own sample at (100, 100) before the blend; the
  textured case's corner samples lie inside [min, max] of the plane's levels
  give or take 2% of that range (the core test's `RANGE_SLACK`: the healing
  write-back's membrane may step past the surround's range), and every grid
  sample outside the corners equals a fill-off twin's composite exactly.

CASES
- STRUCTURE at 8, 16 and 32 bits: `new_document` (transparent) and three
  `place_image` calls at x 0 / 200 / 400; Panorama with the defaults
  (`seamless` true): the reply's `method`, shares, no layer added, every frame
  masked, ONE history step; `undo` removes the masks and gives back every
  frame's raw samples.
- SEAMLESS TONES (8 bits): the exposure case; the gain and tones bounds.
- SEAMLESS OFF (8 bits): every frame's raw samples are bit-identical after.
- ALIGN FIRST (8 bits): the two shifted crops by `load_files_into_stack`,
  `align: true`.
- ACTION: a recording around one Panorama call holds one `auto_blend_layers`
  step whose `method` is `panorama`; `run_action` replays it on a second stack
  as one history step.
- REFUSALS, each with no history step: `method: "mosaic"`; a CMYK document
  (`CmykDocument.refusal`'s sentence); a frame that already has a mask;
  `fill: true` with `stack_images`; `fill: "maybe"`.
- FILL at 8, 16 and 32 bits: the grey crops, `seamless: false`, `fill: true`:
  `fill.covered_pixels`, the layer count one more with the new layer at
  `fill.layer` named "grey-b (merged)", the fill bounds, the frames' raw
  samples unchanged, ONE history step, and `undo` gives back the layer count.
- FILL TEXTURED (8 bits): the textured crops, `seamless` on, `fill: true`,
  against a twin blended with `fill: false`.
- FILL NONE (8 bits): the structure layout covers the canvas, so `fill` is
  `{layer: null, covered_pixels: 0}` and no layer is added.
- FILL REFUSED (8 bits): two 64 x 64 grey crops at (0, 0) and (32, 32) on a
  6000 x 6000 transparent canvas, `seamless: false`, `fill: true`: the
  transparent area is one ~36 MP part, past Content-Aware Fill's 32 MP window
  cap, so the call answers in-band an error starting "Content-Aware Fill
  Transparent Areas could not fill the transparent area:" and ending "Turn it
  off to blend without the fill."; the layer count and history unchanged.
- The ACTION records its call with `fill: true` (the structure layout has
  nothing to fill) and the recorded step holds it.

Usage: python3 scripts/verify_v1_auto_blend_panorama.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_auto_blend import App, close, cursor, level, rect, rows, scene  # noqa: E402

PLANE = (800, 300)
CROP = (400, 300)
XS = [0, 200, 400]
NAMES = ['left', 'middle', 'right']
SHIFT = 60
FILL_CANVAS = (600, 340)
FILL_AT = [(0, 0), (200, 40)]
FILL_COVERED = 16000
FILL_GRID = [(x, y) for y in range(5, 340, 30) for x in range(5, 600, 30)]
FILL_CODES = {8: 2, 16: 2 * 257, 32: 0.01}
RANGE_SLACK = 0.02
REFUSED_CANVAS = 6000
FILL_REFUSED = ('Content-Aware Fill Transparent Areas could not fill the transparent area:',
                'Turn it off to blend without the fill.')
SHARE_TOLERANCE = 1e-9
GAIN_RATIO = 0.01
TONE_MEAN, TONE_MAX = 3.0, 8.0
CANVAS_SLACK = 3
GRID = [(x, y) for y in range(15, 300, 30) for x in range(10, 400, 40)]


def write_crop(path, plane, x0, width, adjust=lambda v: v):
    if not path.is_file():
        data = bytearray()
        for row in plane:
            for value in row[x0:x0 + width]:
                grey = adjust(level(value))
                data += bytes((grey, grey, grey, 255))
        staged = path.with_suffix('.partial')
        png(staged, width, CROP[1], bytes(data))
        staged.rename(path)
    return path


def author(folder):
    folder.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    plane = scene((0, 0) + PLANE, False)
    darker = lambda v: int(0.8 * v + 0.5)  # noqa: E731
    paths = {name: write_crop(folder / f'{name}.png', plane, x, CROP[0])
             for name, x in zip(NAMES, XS)}
    paths['middle-dark'] = write_crop(folder / 'middle-dark.png', plane, XS[1], CROP[0], darker)
    paths['shift-a'] = write_crop(folder / 'shift-a.png', plane, 0, CROP[0])
    paths['shift-b'] = write_crop(folder / 'shift-b.png', plane, SHIFT, CROP[0])
    for name in ('grey-a', 'grey-b'):
        paths[name] = flat_png(folder / f'{name}.png', CROP, 128)
    for name in ('small-a', 'small-b'):
        paths[name] = flat_png(folder / f'{name}.png', (64, 64), 128)
    fill_plane = scene((0, 0) + FILL_CANVAS, False)
    paths['tex-a'] = write_crop(folder / 'tex-a.png', fill_plane[0:CROP[1]], 0, CROP[0])
    paths['tex-b'] = write_crop(folder / 'tex-b.png', fill_plane[40:40 + CROP[1]], 200, CROP[0])
    fill_levels = [level(v) for row in fill_plane for v in row]
    paths['tex-range'] = (min(fill_levels), max(fill_levels))
    levels = [[level(v) for v in row] for row in plane]
    print(f'fixtures: {time.monotonic() - started:.1f} s', flush=True)
    return paths, levels


def flat_png(path, size, grey):
    if not path.is_file():
        staged = path.with_suffix('.partial')
        png(staged, size[0], size[1], bytes((grey, grey, grey, 255)) * (size[0] * size[1]))
        staged.rename(path)
    return path


# --------------------------------------------------------------- plumbing --

def placed(app, depth, crops):
    """A transparent `PLANE` document with the three crops placed at `XS`;
    answers the document and the frames' indices."""
    document = app.call('new_document', width=PLANE[0], height=PLANE[1], unit='px', depth=depth,
                        background='transparent')['document']['id']
    for path, x in zip(crops, XS):
        app.call('place_image', document_id=document, path=str(path), x=x, y=0)
    listed = rows(app, document)
    frames = [row['index'] for row in listed if row['name'] in [Path(p).stem for p in crops]]
    assert len(frames) == 3, listed
    for index, x in zip(frames, XS):
        assert rect(listed[index]) == (x, 0) + CROP, listed[index]
    return document, frames


def raw(app, document, layer, x, y):
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer, x=x, y=y)
    return reply.get('center_rgba32') or reply.get('center_rgba_codes')


def raw_grid(app, document, frames):
    return {(k, x, y): raw(app, document, k, XS[i] + x, y)
            for i, k in enumerate(frames) for x, y in GRID}


def panorama(app, document, frames, **extra):
    return app.call('auto_blend_layers', document_id=document, layers=frames,
                    method='panorama', **extra)


# ----------------------------------------------------------------- cases --

def structure(app, paths, depth):
    document, frames = placed(app, depth, [paths[n] for n in NAMES])
    before = raw_grid(app, document, frames)
    start = cursor(app, document)
    count = len(rows(app, document))
    reply = panorama(app, document, frames)
    assert reply['ok'] is True and reply['changed'] is True, reply
    assert reply['method'] == 'panorama', reply
    assert 'merged_layer' not in reply, reply
    shares = [entry['share'] for entry in reply['layers']]
    assert [entry['layer'] for entry in reply['layers']] == frames, reply
    assert abs(sum(shares) - 1) <= SHARE_TOLERANCE, shares
    assert all(len(entry['gain']) == 3 for entry in reply['layers']), reply
    assert reply['document']['layer_count'] == count, 'no layer is added'
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    assert all(listed[k]['has_mask'] and listed[k]['mask_enabled'] for k in frames), listed
    app.call('undo', document_id=document)
    assert cursor(app, document) == start, 'undo walks the one step back'
    listed = rows(app, document)
    assert not any(row['has_mask'] for row in listed), 'undo removes the masks'
    after = raw_grid(app, document, frames)
    assert after == before, 'undo gives back every frame\'s pixels'
    close(app, document)
    return dict(shares=shares, gains=[entry['gain'] for entry in reply['layers']],
                plan_bytes=reply['working_memory_plan_bytes'])


def seamless_tones(app, paths, levels):
    crops = [paths['left'], paths['middle-dark'], paths['right']]
    document, frames = placed(app, 8, crops)
    reply = panorama(app, document, frames)
    gains = [entry['gain'] for entry in reply['layers']]
    for other in (0, 2):
        for c in range(3):
            ratio = gains[1][c] * 0.8 / gains[other][c]
            assert abs(ratio - 1) <= GAIN_RATIO, (other, c, gains)
    found = {}
    for overlap, (x0, frame) in enumerate([(200, 0), (400, 1)]):
        errors = []
        for i in range(10):
            for j in range(10):
                x, y = x0 + 10 + 20 * i, 15 + 30 * j
                code = levels[y][x] if frame == 0 else int(0.8 * levels[y][x] + 0.5)
                want = gains[frame][0] * code
                got = app.call('sample_pixel', document_id=document, x=x, y=y)['r']
                errors.append(abs(got - want))
        mean, worst = sum(errors) / len(errors), max(errors)
        assert mean <= TONE_MEAN and worst <= TONE_MAX, (overlap, mean, worst)
        found[f'overlap {overlap}'] = dict(mean=round(mean, 3), worst=round(worst, 3))
    close(app, document)
    return dict(gains=gains, **found)


def seamless_off(app, paths):
    document, frames = placed(app, 8, [paths[n] for n in NAMES])
    before = raw_grid(app, document, frames)
    start = cursor(app, document)
    reply = panorama(app, document, frames, seamless=False)
    assert reply['changed'] is True and all('gain' not in e for e in reply['layers']), reply
    assert cursor(app, document) == start + 1, 'one history step'
    assert all(rows(app, document)[k]['has_mask'] for k in frames)
    assert raw_grid(app, document, frames) == before, 'no frame pixel changes'
    close(app, document)
    return dict(shares=[entry['share'] for entry in reply['layers']])


def align_first(app, paths):
    document = app.call('load_files_into_stack',
                        paths=[str(paths['shift-a']), str(paths['shift-b'])])['document']['id']
    start = cursor(app, document)
    reply = panorama(app, document, [0, 1], align=True)
    assert reply['changed'] is True, reply
    canvas = reply['aligned']['canvas']
    want = (CROP[0] + SHIFT, CROP[1])
    assert abs(canvas['width'] - want[0]) <= CANVAS_SLACK, canvas
    assert abs(canvas['height'] - want[1]) <= CANVAS_SLACK, canvas
    summary = reply['document']
    assert (summary['width'], summary['height']) == (canvas['width'], canvas['height']), summary
    assert cursor(app, document) == start + 1, 'one history step'
    listed = rows(app, document)
    assert all(listed[k]['has_mask'] for k in (0, 1)), listed
    close(app, document)
    return dict(canvas=canvas, aligned=[e['method'] for e in reply['aligned']['aligned']])


def action(app, paths):
    crops = [paths[n] for n in NAMES]
    document, frames = placed(app, 8, crops)
    second, again = placed(app, 8, crops)
    name = 'v1 auto blend panorama'
    app.call('start_recording', discard=True)
    assert panorama(app, document, frames, fill=True)['changed'] is True
    recorded = app.call('stop_recording', name=name)
    assert [step['tool'] for step in recorded['steps']] == ['auto_blend_layers'], recorded
    assert recorded['steps'][0]['arguments']['method'] == 'panorama', recorded
    assert recorded['steps'][0]['arguments']['fill'] is True, recorded
    start = cursor(app, second)
    assert app.call('run_action', document_id=second, name=name)['ok']
    assert cursor(app, second) == start + 1, 'the replay is one history step'
    listed = rows(app, second)
    assert all(listed[k]['has_mask'] for k in again), listed
    close(app, document)
    close(app, second)
    return dict(arguments=recorded['steps'][0]['arguments'])


def refusals(app, paths):
    seen = {}

    def refuse(label, document, *needles, **arguments):
        held = cursor(app, document)
        seen[label] = app.refused('auto_blend_layers', *needles, document_id=document,
                                  **arguments)
        assert cursor(app, document) == held, f'{label}: a refusal leaves no history step'

    document, frames = placed(app, 8, [paths[n] for n in NAMES])
    refuse('an unknown method', document, 'method must be panorama or stack_images',
           layers=frames, method='mosaic')
    refuse('fill with stack images', document,
           'Content-Aware Fill Transparent Areas applies only to the Panorama method.',
           layers=frames, method='stack_images', fill=True)
    refuse('a fill that is not a boolean', document, 'fill must be true or false.',
           layers=frames, method='panorama', fill='maybe')
    app.call('add_layer_mask', document_id=document, layer=frames[1])
    refuse('a masked frame', document, 'middle', 'already has a layer mask', layers=frames,
           method='panorama')
    close(app, document)

    cmyk = app.call('new_document', width=200, height=100, unit='px', color_mode='cmyk', depth=8,
                    background='white')['document']['id']
    app.call('new_layer', document_id=cmyk)
    refuse('a CMYK document', cmyk, 'auto_blend_layers is unavailable in a CMYK document',
           layers=[0, 1], method='panorama')
    close(app, cmyk)
    return seen


def fill_placed(app, depth, crops, at, canvas=FILL_CANVAS):
    """A transparent `canvas` document with `crops` placed at `at`; answers
    the document and the frames' indices, bottom first."""
    document = app.call('new_document', width=canvas[0], height=canvas[1], unit='px',
                        depth=depth, background='transparent')['document']['id']
    for path, (x, y) in zip(crops, at):
        app.call('place_image', document_id=document, path=str(path), x=x, y=y)
    listed = rows(app, document)
    stems = [Path(p).stem for p in crops]
    frames = [row['index'] for row in listed if row['name'] in stems]
    assert len(frames) == len(crops), listed
    return document, frames


def in_corner(x, y):
    """The fill cases' transparent corners."""
    return (x >= 400 and y < 40) or (x < 200 and y >= 300)


def fill_frames_grid(app, document, frames):
    return {(k, x, y): raw(app, document, k, x, y)
            for k, (fx, fy) in zip(frames, FILL_AT)
            for x, y in FILL_GRID if fx <= x < fx + CROP[0] and fy <= y < fy + CROP[1]}


def fill_flat(app, paths, depth):
    document, frames = fill_placed(app, depth, [paths['grey-a'], paths['grey-b']], FILL_AT)
    grey = raw(app, document, frames[0], 100, 100)
    before = fill_frames_grid(app, document, frames)
    start = cursor(app, document)
    count = len(rows(app, document))
    reply = panorama(app, document, frames, seamless=False, fill=True)
    assert reply['changed'] is True, reply
    fill = reply['fill']
    assert fill['covered_pixels'] == FILL_COVERED, fill
    assert reply['document']['layer_count'] == count + 1, 'one layer is added'
    listed = rows(app, document)
    assert listed[fill['layer']]['name'] == 'grey-b (merged)', listed[fill['layer']]
    assert cursor(app, document) == start + 1, 'one history step'
    tolerance = FILL_CODES[depth]
    worst = 0
    for x, y in FILL_GRID:
        sample = raw(app, document, fill['layer'], x, y)
        assert sample[3] == grey[3], ((x, y), sample, grey)
        if in_corner(x, y):
            off = max(abs(sample[c] - grey[c]) for c in range(3))
            worst = max(worst, off)
            assert off <= tolerance, ((x, y), sample, grey)
    assert fill_frames_grid(app, document, frames) == before, 'the frames keep their pixels'
    app.call('undo', document_id=document)
    assert cursor(app, document) == start, 'undo walks the one step back'
    assert len(rows(app, document)) == count, 'undo removes the filled layer'
    close(app, document)
    return dict(fill=fill, grey=grey, worst_corner_off=worst,
                plan_bytes=reply['working_memory_plan_bytes'])


def fill_textured(app, paths):
    crops = [paths['tex-a'], paths['tex-b']]
    twin, twin_frames = fill_placed(app, 8, crops, FILL_AT)
    assert panorama(app, twin, twin_frames, fill=False)['changed'] is True
    document, frames = fill_placed(app, 8, crops, FILL_AT)
    reply = panorama(app, document, frames, fill=True)
    fill = reply['fill']
    assert fill['covered_pixels'] == FILL_COVERED, fill
    low, high = paths['tex-range']
    slack = RANGE_SLACK * (high - low)
    corners = outside = 0
    for x, y in FILL_GRID:
        sample = raw(app, document, fill['layer'], x, y)
        assert sample[3] == 255, ((x, y), sample)
        if in_corner(x, y):
            corners += 1
            assert all(low - slack <= sample[c] <= high + slack for c in range(3)), \
                ((x, y), sample, (low, high))
        else:
            outside += 1
            composite = app.call('sample_pixel', document_id=twin, x=x, y=y)
            want = [composite['r'], composite['g'], composite['b'], composite['a']]
            assert list(sample) == want, ((x, y), sample, want)
    close(app, document)
    close(app, twin)
    return dict(fill=fill, range=[low, high], corners=corners, outside=outside)


def fill_none(app, paths):
    document, frames = placed(app, 8, [paths[n] for n in NAMES])
    count = len(rows(app, document))
    reply = panorama(app, document, frames, fill=True)
    assert reply['fill'] == {'layer': None, 'covered_pixels': 0}, reply.get('fill')
    assert reply['document']['layer_count'] == count, 'no layer is added'
    close(app, document)
    return dict(fill=reply['fill'])


def fill_refused(app, paths):
    side = REFUSED_CANVAS
    document, frames = fill_placed(app, 8, [paths['small-a'], paths['small-b']],
                                   [(0, 0), (32, 32)], canvas=(side, side))
    held = cursor(app, document)
    count = len(rows(app, document))
    text = app.refused('auto_blend_layers', FILL_REFUSED[0], document_id=document,
                       layers=frames, method='panorama', seamless=False, fill=True)
    message = json.loads(text)['error'] if text.lstrip().startswith('{') else text
    assert FILL_REFUSED[0] in message and message.rstrip().endswith(FILL_REFUSED[1]), message
    assert cursor(app, document) == held, 'a refusal leaves no history step'
    assert len(rows(app, document)) == count, 'no layer is added'
    close(app, document)
    return dict(message=message)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-auto-blend-panorama-'))
    output.mkdir(parents=True, exist_ok=True)
    paths, levels = author(output / 'fixtures')

    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        for depth in (8, 16, 32):
            run(f'structure {depth}-bit', lambda: structure(app, paths, depth))
        run('seamless tones', lambda: seamless_tones(app, paths, levels))
        run('seamless off', lambda: seamless_off(app, paths))
        run('align first', lambda: align_first(app, paths))
        run('action', lambda: action(app, paths))
        run('refusals', lambda: refusals(app, paths))
        for depth in (8, 16, 32):
            run(f'fill {depth}-bit', lambda: fill_flat(app, paths, depth))
        run('fill textured', lambda: fill_textured(app, paths))
        run('fill none', lambda: fill_none(app, paths))
        run('fill refused', lambda: fill_refused(app, paths))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
