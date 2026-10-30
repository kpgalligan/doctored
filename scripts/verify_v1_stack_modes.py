#!/usr/bin/env python3
"""MCP drive for Stack Modes and Reduce Noise from Stack: `stack_mode`
(Layer > Stack Mode) and `reduce_noise_from_stack` (Layer > Reduce Noise from
Stack...) end to end.

The script owns its app instance (`verify_v1_stack_align.App`): the Debug app
(or `DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`,
a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private
`DR_*_DIR`s. It terminates only that process. Every expected number is computed
here, never read back from the app.

THE FRAMES. 320x240 opaque PNGs: a flat (96, 96, 96) plus independent integer
noise, uniform in -12...12 on each channel of each pixel (`random.Random(seed)`,
one seed a frame). The OBJECT frame is one more such frame carrying an opaque
(255, 255, 255) rectangle at (80, 60), 160x120. Pixels are read with
`sample_pixel` (`source: "layer"`) on a 24x18 grid of cell centres; a source
layer is read BEFORE the edit.

DEPTHS. Every stack case runs at 8 bits (the frames as a stack), at 16 (that
stack, then `convert_precision`) and at 32-bit float (`paths` =
[core/tests/fixtures/float_11f/scene.tiff, frames...], so the members are
layers 1...N and layer 0 is a visible non-member).

BOUNDS, fixed before measuring and never widened:
- NOISE, for N = 4 and N = 16: std(one source layer) / std(the Mean layer),
  each over the grid's three colour channels (per-channel variance about the
  channel's own mean, pooled), is within 15% of sqrt(N).
- OBJECT, N = 5 with the object frame third: at every grid point inside the
  rectangle and on every channel, the Median sample lies between the least and
  the greatest of the four clean frames' samples there, and the Mean sample is
  above the greatest.
- DENOISE: mean |result - A| over `centre_grid` is under half of
  mean |b-translation - A| before.
- SIX MODES (phase 1): every sampled channel equals this file's own
  statistic (`statistic`, written from the rule in `stack_moments`' module
  doc) exactly at 8 and 16 bits for Summation, Variance and Majority and
  within one code for Skewness, Kurtosis and Entropy (an f64 step), and within
  1e-5 relative (floor 1e-5) at float.
- CMYK: Median and Maximum over three layers equal the L-domain oracle
  (L = 65535 - ink per plate, the statistic of L, ink = 65535 - result)
  exactly.

CASES
- NOISE and OBJECT, as above.
- STRUCTURE: the reply's fields; the new layer directly above the topmost
  layer given, named after the mode (or `name`), active; the layers read
  hidden and nothing else; the history cursor moves by one; one `undo`
  restores the layer count and every layer's visibility. A second call over
  the two lowest members lands its layer above the second, under the third.
- REFUSALS, each with no history step: one layer, a group, an adjustment
  layer, an unknown mode (`trimmed_mean`; and, for `reduce_noise_from_stack`, a `stack_mode`
  statistic it does not take). At 8 bits the sentence must name what is wrong,
  and a group or an adjustment layer is refused in the asking command's own
  name ("Stack Mode takes raster layers" / "Reduce Noise from Stack takes
  raster layers").
- DENOISE at 8 bits, on `verify_v1_stack_align.author`'s 640x480 fixtures
  (written here by the same calls: `frame-a`, `frame-b-translation`,
  `frame-b-rotation`, `frame-b-past-reach`):
  [a, b-translation, b-rotation] with `mode: "mean"` aligns both, `excluded`
  is empty, and the pixel bound holds;
  [a, b-translation, b-past-reach] names layer 2 in `excluded`, leaves its
  rectangle, visibility and sampled pixels unchanged, reads two members and
  replies `layer: 3` -- row 3 is "Noise Reduced" and row 2 is still
  `frame-b-past-reach`, so the result is above the layer it left out;
  [a, b-past-reach] is refused with a sentence naming it, no history step,
  nothing moved.
- SIX MODES at 8, 16 and float: five members [n0, n1, n0, n2, n1] (two values
  repeated, so Majority meets ties) over every fourth grid point; each new
  mode's layer against `statistic`, then undone.
- CMYK STACK (8 bits, `convert_mode cmyk`, three members): each member's ink
  is read as the composite's `ink16` with the layers above it hidden, then
  Median and Maximum are each checked against the L-domain oracle and undone.

Usage: python3 scripts/verify_v1_stack_modes.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import math
from pathlib import Path
import random
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_stack_align import (  # noqa: E402
    App, FLOAT_FIXTURE, IDENTITY, MOVE_K, Picture, SMALL, SMALL_K, centre_grid, close, cursor,
    fixture, history, inverse, load, mean_difference, rect, rows)

FRAME = (320, 240)
FLAT = 96
NOISE = 12
GRID = (24, 18)
OBJECT_RECT = (80, 60, 160, 120)
RATIO_TOLERANCE = 0.15
MODES = ('mean', 'median', 'minimum', 'maximum', 'range', 'standard_deviation', 'summation',
         'variance', 'entropy', 'kurtosis', 'skewness', 'majority')
NEW_MODES = ('summation', 'variance', 'entropy', 'kurtosis', 'skewness', 'majority')
REAL_STEP = ('skewness', 'kurtosis', 'entropy')
TITLES = dict(mean='Mean', median='Median', minimum='Minimum', maximum='Maximum', range='Range',
              standard_deviation='Standard Deviation', summation='Summation',
              variance='Variance', entropy='Entropy', kurtosis='Kurtosis', skewness='Skewness',
              majority='Majority')


# --------------------------------------------------------------- fixtures --

def noisy_frame(folder, name, seed, rectangle=None):
    """Writes `<folder>/<name>.png` once per output folder: the flat grey plus
    the seed's noise, and the opaque white `rectangle` when given."""
    path = Path(folder) / f'{name}.png'
    if path.is_file():
        return path
    width, height = FRAME
    generator = random.Random(seed)
    out = bytearray(b'\xff' * (width * height * 4))
    for index in range(width * height):
        base = index * 4
        out[base] = FLAT + generator.randint(-NOISE, NOISE)
        out[base + 1] = FLAT + generator.randint(-NOISE, NOISE)
        out[base + 2] = FLAT + generator.randint(-NOISE, NOISE)
    if rectangle:
        x0, y0, w, h = rectangle
        for y in range(y0, y0 + h):
            start = (y * width + x0) * 4
            out[start:start + w * 4] = b'\xff' * (w * 4)
    png(path, width, height, bytes(out))
    return path


def author(folder):
    folder.mkdir(parents=True, exist_ok=True)
    frames = [noisy_frame(folder, f'noise-{seed:02d}', seed) for seed in range(1, 17)]
    carrying = noisy_frame(folder, 'noise-object', 101, OBJECT_RECT)
    scene = folder / 'scene.tiff'
    if not scene.is_file():
        shutil.copyfile(FLOAT_FIXTURE, scene)
    # `verify_v1_stack_align.author`'s 640x480 fixtures, by the same calls.
    small = Picture(-640, -256, SMALL[0] + 256, SMALL[1] + 256)
    aligned = {'a': fixture(folder, 'frame-a', small, SMALL, IDENTITY)}
    for name, k in (('translation', SMALL_K['translation']), ('rotation', SMALL_K['rotation']),
                    ('past-reach', MOVE_K['past-reach'])):
        aligned[f'b-{name}'] = fixture(folder, f'frame-b-{name}', small, SMALL, inverse(k))
    return frames, carrying, scene, aligned, small


# --------------------------------------------------------------- plumbing --

def grid_points():
    return [(int((i + 0.5) * FRAME[0] / GRID[0]), int((j + 0.5) * FRAME[1] / GRID[1]))
            for j in range(GRID[1]) for i in range(GRID[0])]


def inside(point, rectangle):
    x0, y0, w, h = rectangle
    return x0 <= point[0] < x0 + w and y0 <= point[1] < y0 + h


def sample(app, document, layer, point, depth):
    """The layer's own RGB at `point`: codes at 8 and 16 bits, linear values
    at float."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=layer,
                     x=point[0], y=point[1])
    values = reply['center_rgba32'] if depth == 'float' else reply['center_rgba_codes']
    assert len(values) == 4, reply
    return values[:3]


def samples(app, document, layer, points, depth):
    return [sample(app, document, layer, point, depth) for point in points]


def pooled_std(values):
    """sqrt of the mean, over the three channels, of each channel's variance
    about its own mean."""
    total = 0.0
    for channel in range(3):
        column = [value[channel] for value in values]
        mean = sum(column) / len(column)
        total += sum((v - mean) ** 2 for v in column) / len(column)
    return math.sqrt(total / 3)


def open_stack(app, paths, depth, scene):
    """The frames as a stack at `depth`: (document, the frames' layer indices)."""
    if depth == 'float':
        document = load(app, [scene] + list(paths))['document']['id']
        members = list(range(1, len(paths) + 1))
    else:
        document = load(app, paths)['document']['id']
        members = list(range(len(paths)))
        if depth == 16:
            app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    bits = app.call('get_precision', document_id=document)['bits_per_channel']
    assert bits == (32 if depth == 'float' else depth), bits
    return document, members


def statistic(mode, values, full, real):
    """One channel's statistic over fully covered members, from the rule:
    `full` is white (255, 65535 or 1.0); integers round half up and clamp."""
    n = len(values)

    def code(x):
        return x if real else max(0, min(full, math.floor(x + 0.5)))

    if mode == 'summation':
        return sum(values) if real else min(full, sum(values))
    mean = sum(values) / n
    if mode == 'variance':
        if real:
            return sum((v - mean) ** 2 for v in values) / n
        num = n * sum(v * v for v in values) - sum(values) ** 2
        q, r = divmod(num, n * n * full)
        return min(full, q + (1 if 2 * r >= n * n * full else 0))
    if mode in ('skewness', 'kurtosis'):
        skew = mode == 'skewness'
        if n <= 2 or all(v == values[0] for v in values):
            return code((0.5 if skew else 0.0) * full)
        m2 = sum((v - mean) ** 2 for v in values) / n
        m3 = sum((v - mean) ** 3 for v in values) / n
        m4 = sum((v - mean) ** 4 for v in values) / n
        if skew:
            s_max = (n - 2) / math.sqrt(n - 1)
            return code(full * 0.5 * (1 + (m3 / m2 ** 1.5) / s_max))
        k_max = (n * n - 3 * n + 3) / (n - 1)
        return code(full * (m4 / (m2 * m2) - 1) / (k_max - 1))
    if mode == 'entropy':
        if n < 2:
            return code(0.0)
        bins = [min(255, math.floor(min(max(v, 0.0), 1.0) * 256)) if real else v * 256 // (full + 1)
                for v in values]
        h = 0.0
        for b in set(bins):
            p = bins.count(b) / n
            h -= p * math.log2(p)
        return code(full * h / math.log2(min(n, 256)))
    if mode == 'majority':
        counts = {}
        for v in values:
            counts[v] = counts.get(v, 0) + 1
        most = max(counts.values())
        tied = sorted(v for v, c in counts.items() if c == most)
        t = len(tied)
        if t % 2:
            return tied[t // 2]
        lo, hi = tied[t // 2 - 1], tied[t // 2]
        return (lo + hi) / 2 if real else (lo + hi + 1) // 2
    raise ValueError(mode)


def visibility(app, document):
    return [row['visible'] for row in rows(app, document)]


# ------------------------------------------------------------------ cases --

def noise_case(app, frames, scene, count, depth):
    document, members = open_stack(app, frames[:count], depth, scene)
    points = grid_points()
    source = samples(app, document, members[0], points, depth)
    reply = app.call('stack_mode', document_id=document, layers=members, mode='mean')
    assert reply['ok'] is True and reply['layers'] == members, reply
    mean = samples(app, document, reply['layer'], points, depth)
    before, after = pooled_std(source), pooled_std(mean)
    assert after > 0, 'the Mean layer is not flat'
    ratio, wanted = before / after, math.sqrt(count)
    assert abs(ratio - wanted) <= RATIO_TOLERANCE * wanted, (
        f'std(source) / std(Mean) = {before:.5g} / {after:.5g} = {ratio:.3f}; sqrt({count}) = '
        f'{wanted:.3f}, bound {RATIO_TOLERANCE:.0%}')
    close(app, document)
    return dict(std_source=round(before, 6), std_mean=round(after, 6), ratio=round(ratio, 3),
                plan_bytes=reply['working_memory_plan_bytes'])


def object_case(app, frames, carrying, scene, depth):
    paths = [frames[0], frames[1], carrying, frames[2], frames[3]]
    document, members = open_stack(app, paths, depth, scene)
    points = [point for point in grid_points() if inside(point, OBJECT_RECT)]
    assert len(points) >= 100, len(points)
    clean = [samples(app, document, members[index], points, depth) for index in (0, 1, 3, 4)]
    carried = samples(app, document, members[2], points, depth)
    white = 1.0 if depth == 'float' else 65535 if depth == 16 else 255
    # The fixture check: the object reads as white in the carrying frame (a
    # float stack converts the 8-bit frame, so within a part in 500 there).
    for value in carried:
        assert all(abs(v - white) <= 2e-3 * white for v in value), (value, white)
    start = cursor(app, document)
    reply = app.call('stack_mode', document_id=document, layers=members, mode='median')
    median = samples(app, document, reply['layer'], points, depth)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    reply = app.call('stack_mode', document_id=document, layers=members, mode='mean')
    mean = samples(app, document, reply['layer'], points, depth)
    least_margin = None
    for index, point in enumerate(points):
        for channel in range(3):
            around = [frame[index][channel] for frame in clean]
            low, high = min(around), max(around)
            assert low <= median[index][channel] <= high, (
                f'Median at {point} channel {channel} is {median[index][channel]}; the clean '
                f'frames span {low}...{high}')
            assert mean[index][channel] > high, (
                f'Mean at {point} channel {channel} is {mean[index][channel]}; the greatest '
                f'clean sample is {high}')
            margin = mean[index][channel] - high
            least_margin = margin if least_margin is None else min(least_margin, margin)
    close(app, document)
    return dict(points=len(points), least_mean_margin=least_margin)


def structure_case(app, frames, scene, depth):
    document, members = open_stack(app, frames[:3], depth, scene)
    count = len(rows(app, document))
    assert visibility(app, document) == [True] * count
    start = cursor(app, document)
    reply = app.call('stack_mode', document_id=document, layers=list(reversed(members)),
                     mode='median')
    top = members[-1]
    assert set(reply) >= {'ok', 'layer', 'name', 'mode', 'layers', 'hidden',
                          'working_memory_plan_bytes', 'document'}, sorted(reply)
    assert reply['ok'] is True and reply['mode'] == 'median', reply
    assert reply['layer'] == top + 1 and reply['name'] == TITLES['median'], reply
    assert reply['layers'] == members and reply['hidden'] == members, reply
    assert isinstance(reply['working_memory_plan_bytes'], int)
    assert reply['working_memory_plan_bytes'] > 3 * FRAME[0] * FRAME[1] * 4, reply
    summary = reply['document']
    assert summary['layer_count'] == count + 1 and summary['active_layer'] == top + 1, summary
    assert (summary['width'], summary['height']) == FRAME, summary
    listed = rows(app, document)
    assert len(listed) == count + 1
    assert listed[top + 1]['name'] == TITLES['median'], listed[top + 1]
    assert rect(listed[top + 1]) == (0, 0) + FRAME, rect(listed[top + 1])
    assert [row['visible'] for row in listed] == [
        index not in members for index in range(count + 1)], 'only the layers read are hidden'
    assert cursor(app, document) == start + 1, 'one history step'
    states = history(app, document)
    assert states['states'][states['cursor']]['name'] == 'Stack Mode', states['states']
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    assert len(rows(app, document)) == count, 'undo removes the new layer'
    assert visibility(app, document) == [True] * count, 'undo restores visibility'

    # The two lowest members: the layer lands above the second, under the third.
    lower = members[:2]
    reply = app.call('stack_mode', document_id=document, layers=lower, mode='range', name='Spread')
    assert reply['layer'] == lower[-1] + 1 and reply['name'] == 'Spread', reply
    assert reply['layers'] == lower and reply['hidden'] == lower, reply
    listed = rows(app, document)
    assert listed[lower[-1] + 1]['name'] == 'Spread', [row['name'] for row in listed]
    assert listed[lower[-1] + 2]['name'] == Path(frames[2]).stem, [row['name'] for row in listed]
    assert listed[lower[-1] + 2]['visible'] is True, 'a layer not read stays visible'
    assert [row['visible'] for row in listed] == [
        index not in lower for index in range(count + 1)]
    # Every mode answers, and names its layer after itself.
    app.call('undo', document_id=document)
    for mode in MODES:
        answer = app.call('stack_mode', document_id=document, layers=members, mode=mode)
        assert answer['mode'] == mode and answer['name'] == TITLES[mode], answer
        app.call('undo', document_id=document)
    assert cursor(app, document) == start
    close(app, document)
    return dict(layer=reply['layer'], plan_bytes=reply['working_memory_plan_bytes'])


def refusal_case(app, frames, scene, depth):
    document, members = open_stack(app, frames[:3], depth, scene)
    strict = depth == 8
    seen = {}

    def refuse(name, tool, *needles, **arguments):
        held, listed = cursor(app, document), rows(app, document)
        text = app.refused(tool, *(needles if strict else ()), document_id=document, **arguments)
        assert text, f'{name}: a refusal carries a sentence'
        assert cursor(app, document) == held, f'{name}: a refusal leaves no history step'
        after = rows(app, document)
        assert [(row['name'], row['visible'], rect(row)) for row in after] == [
            (row['name'], row['visible'], rect(row)) for row in listed], f'{name}: nothing changed'
        seen[name] = text

    refuse('one layer', 'stack_mode', 'at least 2', layers=members[:1], mode='mean')
    refuse('an unknown mode', 'stack_mode', 'trimmed_mean', 'median', layers=members,
           mode='trimmed_mean')
    refuse('no mode', 'stack_mode', 'mode is required', layers=members)
    refuse('a stack statistic for noise', 'reduce_noise_from_stack', 'range', 'mean', 'median',
           layers=members, mode='range')
    refuse('one layer for noise', 'reduce_noise_from_stack', 'at least 2', layers=members[:1],
           mode='mean')

    # Exposure at float: the float document admits it (`verify_phase11f_float`).
    if depth == 'float':
        app.call('add_adjustment_layer', document_id=document, op='exposure',
                 params={'exposure': 1.0}, name='Adj', layer=members[-1])
    else:
        app.call('add_adjustment_layer', document_id=document, op='invert', name='Adj',
                 layer=members[-1])
    adjustment = next(row['index'] for row in rows(app, document) if row['name'] == 'Adj')
    # Each command words the refusal with its own name.
    refuse('an adjustment layer', 'stack_mode', 'Adj', 'adjustment layer',
           'Stack Mode takes raster layers', layers=[members[0], adjustment], mode='mean')
    refuse('an adjustment layer for noise', 'reduce_noise_from_stack', 'Adj', 'adjustment layer',
           'Reduce Noise from Stack takes raster layers',
           layers=[members[0], adjustment], mode='mean')

    app.call('group_layers', document_id=document, layers=[members[1]], name='Holder')
    group = next(row['index'] for row in rows(app, document) if row['name'] == 'Holder')
    refuse('a group', 'stack_mode', 'Holder', 'is a group', 'Stack Mode takes raster layers',
           layers=[members[0], group], mode='mean')
    refuse('a group for noise', 'reduce_noise_from_stack', 'Holder', 'is a group',
           'Reduce Noise from Stack takes raster layers', layers=[members[0], group],
           mode='mean')
    close(app, document)
    return seen


def denoise_case(app, aligned, picture):
    a, b = aligned['a'], aligned['b-translation']
    document = load(app, [a, b, aligned['b-rotation']])['document']['id']
    start = cursor(app, document)
    before = mean_difference(app, document, 1, picture, SMALL, 8)
    reply = app.call('reduce_noise_from_stack', document_id=document, layers=[0, 1, 2], mode='mean')
    assert set(reply) >= {'ok', 'layer', 'name', 'mode', 'reference', 'layers', 'hidden',
                          'aligned', 'unchanged', 'excluded', 'registration_plan_bytes',
                          'working_memory_plan_bytes', 'document'}, sorted(reply)
    assert reply['ok'] is True and reply['mode'] == 'mean' and reply['reference'] == 0, reply
    assert reply['layer'] == 3 and reply['name'] == 'Noise Reduced', reply
    assert sorted(entry['layer'] for entry in reply['aligned']) == [1, 2], reply
    assert all(entry['method'] in ('homography', 'translation') for entry in reply['aligned'])
    assert reply['excluded'] == [] and reply['unchanged'] == [], reply
    assert reply['layers'] == [0, 1, 2] and reply['hidden'] == [0, 1, 2], reply
    assert reply['working_memory_plan_bytes'] >= reply['registration_plan_bytes'] > 0, reply
    listed = rows(app, document)
    assert [row['name'] for row in listed] == [
        Path(a).stem, Path(b).stem, Path(aligned['b-rotation']).stem, 'Noise Reduced'], listed
    assert [row['visible'] for row in listed] == [False, False, False, True], listed
    assert cursor(app, document) == start + 1, 'the warps and the new layer are one step'
    states = history(app, document)
    assert states['states'][states['cursor']]['name'] == 'Reduce Noise from Stack', states
    after = mean_difference(app, document, 3, picture, SMALL, 8)
    assert after < before / 2, (f'mean |result - A| is {after:.2f} levels; mean |b - A| before '
                                f'was {before:.2f}; it must fall below half')
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    listed = rows(app, document)
    assert len(listed) == 3 and [row['visible'] for row in listed] == [True] * 3, listed
    assert [rect(row) for row in listed] == [(0, 0) + SMALL] * 3, 'undo restores the warps too'
    close(app, document)
    return dict(mean_before=round(before, 2), mean_after=round(after, 2),
                methods=[entry['method'] for entry in reply['aligned']])


def denoise_excluded(app, aligned):
    a, b, far = aligned['a'], aligned['b-translation'], aligned['b-past-reach']
    document = load(app, [a, b, far])['document']['id']
    start = cursor(app, document)
    far_rect = rect(rows(app, document)[2])

    def sampled():
        return [app.call('sample_pixel', document_id=document, source='layer', layer=2,
                         x=x, y=y)['center_rgba_codes'] for x, y in centre_grid(SMALL)]

    before = sampled()
    reply = app.call('reduce_noise_from_stack', document_id=document, layers=[0, 1, 2],
                     mode='median')
    assert reply['ok'] is True and reply['mode'] == 'median', reply
    assert [(entry['layer'], entry['name']) for entry in reply['excluded']] == [
        (2, Path(far).stem)], reply
    assert reply['excluded'][0]['reason'], reply
    assert reply['layers'] == [0, 1] and reply['hidden'] == [0, 1], reply
    assert [entry['layer'] for entry in reply['aligned']] == [1], reply
    assert reply['layer'] == 3, 'the result is above the topmost layer given'
    listed = rows(app, document)
    assert listed[3]['name'] == 'Noise Reduced', [row['name'] for row in listed]
    assert listed[2]['name'] == Path(far).stem, [row['name'] for row in listed]
    assert listed[2]['visible'] is True and rect(listed[2]) == far_rect, listed[2]
    assert [row['visible'] for row in listed] == [False, False, True, True], listed
    assert sampled() == before, 'the excluded layer was not resampled'
    assert cursor(app, document) == start + 1
    close(app, document)
    return dict(excluded=reply['excluded'])


def denoise_refused(app, aligned):
    a, far = aligned['a'], aligned['b-past-reach']
    document = load(app, [a, far])['document']['id']
    start = cursor(app, document)
    listed = rows(app, document)
    text = app.refused('reduce_noise_from_stack', Path(far).stem, 'at least two',
                       document_id=document, layers=[0, 1], mode='mean')
    assert cursor(app, document) == start, 'a refusal leaves no history step'
    after = rows(app, document)
    assert [(row['name'], row['visible'], rect(row)) for row in after] == [
        (row['name'], row['visible'], rect(row)) for row in listed], 'nothing moved'
    close(app, document)
    return dict(refusal=text)


def six_modes_case(app, frames, scene, depth):
    """The six phase-1 modes against `statistic`, members with repeats."""
    paths = [frames[0], frames[1], frames[0], frames[2], frames[1]]
    document, members = open_stack(app, paths, depth, scene)
    points = grid_points()[::4]
    values = [samples(app, document, member, points, depth) for member in members]
    real = depth == 'float'
    full = 1.0 if real else 65535 if depth == 16 else 255
    worst = {}
    for mode in NEW_MODES:
        start = cursor(app, document)
        reply = app.call('stack_mode', document_id=document, layers=members, mode=mode)
        assert reply['mode'] == mode and reply['name'] == TITLES[mode], reply
        got = samples(app, document, reply['layer'], points, depth)
        apart = 0.0
        for index, point in enumerate(points):
            for channel in range(3):
                want = statistic(mode, [value[index][channel] for value in values], full, real)
                have = got[index][channel]
                bound = 1e-5 * max(1.0, abs(want)) if real else (1 if mode in REAL_STEP else 0)
                assert abs(have - want) <= bound, (
                    f'{mode} at {point} channel {channel}: {have} against {want} (bound {bound})')
                apart = max(apart, abs(have - want))
        worst[mode] = apart
        app.call('undo', document_id=document)
        assert cursor(app, document) == start, f'{mode}: undo walks the step back'
    close(app, document)
    return dict(points=len(points), worst=worst)


def cmyk_case(app, frames):
    """Median and Maximum per ink against the L-domain oracle."""
    document = load(app, frames[:3])['document']['id']
    converted = app.call('convert_mode', document_id=document, mode='cmyk')
    assert converted['color_mode'] == 'cmyk', converted
    points = grid_points()[::8]

    def inks():
        return [app.call('sample_pixel', document_id=document, x=x, y=y)['ink16'][:4]
                for x, y in points]

    start = cursor(app, document)
    # Each member is the composite with every layer above it hidden (all are opaque).
    members = [None, None, None]
    members[2] = inks()
    app.call('set_layer_properties', document_id=document, index=2, visible=False)
    members[1] = inks()
    app.call('set_layer_properties', document_id=document, index=1, visible=False)
    members[0] = inks()
    app.call('undo', document_id=document)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and visibility(app, document) == [True] * 3
    seen = {}
    for mode in ('median', 'maximum'):
        reply = app.call('stack_mode', document_id=document, layers=[0, 1, 2], mode=mode)
        assert reply['layer'] == 3 and reply['hidden'] == [0, 1, 2], reply
        got = inks()
        for index, point in enumerate(points):
            for plate in range(4):
                lightness = sorted(65535 - member[index][plate] for member in members)
                result = lightness[1] if mode == 'median' else lightness[-1]
                assert got[index][plate] == 65535 - result, (
                    f'{mode} at {point} plate {plate}: {got[index][plate]} against '
                    f'{65535 - result}; members {[m[index][plate] for m in members]}')
        seen[mode] = got[0]
        app.call('undo', document_id=document)
        assert cursor(app, document) == start
    close(app, document)
    return dict(points=len(points), first=seen)


# ------------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-stack-modes-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    frames, carrying, scene, aligned, picture = author(output / 'fixtures')

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
        for depth in (8, 16, 'float'):
            label = f'{depth}-bit' if depth != 'float' else 'float'
            for count in (4, 16):
                run(f'noise {count} {label}',
                    lambda: noise_case(app, frames, scene, count, depth))
            run(f'object {label}', lambda: object_case(app, frames, carrying, scene, depth))
            run(f'structure {label}', lambda: structure_case(app, frames, scene, depth))
            run(f'refusals {label}', lambda: refusal_case(app, frames, scene, depth))
            run(f'six modes {label}', lambda: six_modes_case(app, frames, scene, depth))
        run('denoise', lambda: denoise_case(app, aligned, picture))
        run('denoise with an excluded layer', lambda: denoise_excluded(app, aligned))
        run('denoise refused', lambda: denoise_refused(app, aligned))
        run('cmyk median and maximum', lambda: cmyk_case(app, frames))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
