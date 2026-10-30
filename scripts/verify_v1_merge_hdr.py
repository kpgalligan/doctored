#!/usr/bin/env python3
"""MCP drive for Merge to HDR: `merge_to_hdr` (File > Merge to HDR...)
and `merge_layers_to_hdr` (Layer > Merge Layers to HDR...) end to end.

The script owns its app instance (`verify_v1_stack_align.App`): the Debug app
(or `DR_VERIFY_APP`) on its own scratch MCP socket with `DR_BACKGROUND=1`,
a dummy `ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private
`DR_*_DIR`s. It terminates only that process. Every expected number is
computed here, never read back from the app, and no case is compared with
another editor's output.

THE BRACKET. A scene of KNOWN radiance: `R(x) * (1, 0.9, 0.8)` with `R` from
6.0e-4 to 6.0e-4 * 2^14 across a 464-pixel width (14 stops, linear light in
sRGB primaries). A frame at `ev` stops stores `srgb(clip(radiance * 2^ev))`,
quantized. Frames are written here in pure Python: baseline TIFFs at 8 and 16
bits whose Exif IFD carries ExposureTime, FNumber and ISO (f/8, ISO 100, the
time doubling per stop), and 8-bit PNGs that carry none. The merge is the
scene's radiance on the reference frame's scale, so with the reference at EV 0
the merged pixel must be `R(x) * (1, 0.9, 0.8)`.

BOUNDS, fixed before measuring and never widened:
- recovered radiance, per channel, relative: 0.06 at 8 bits; 5e-3 at 16 bits
  through the app;
- an estimated EV: within 0.1 of the truth;
- alignment: the mean |ln(merged / R)| over the 5x5 grid of cell centres with
  `align: true` falls below half its value with `align: false`;
- ghosts: 0 < `deghosted_pixels` <= the pixels of the union of the patch's
  footprints, and inside the union the merge is the reference.

CASES
- EXIF at 8 and at 16 bits: three TIFFs at -4, 0, +4. Every frame's
  `ev_source` is `exif` and its `ev` is log2(t * ISO / (100 * N^2)); the
  reference is the middle frame; the radiance bound.
- ESTIMATED: the same frames as PNGs. `ev_source` is `estimated` and each EV is
  within 0.1 of the truth relative to the first frame.
- GIVEN: the PNGs with `evs`: `ev_source` is `given`, the 8-bit radiance bound.
- NO SHARED EXPOSURE: two PNG frames 8 EV apart with no `evs` are refused with
  the estimate's sentence and open nothing; with `evs: [0, 8]` they merge.
- IDENTICAL EXPOSURES: one frame under two names is refused, naming both;
  with `evs` it merges.
- ALIGNMENT: a textured picture (the stack drive's value noise, radiance
  0.01 * 20^v) at -2, 0, +2, each frame sampled at a known whole-pixel shift.
  Every frame is `used`, the two outer frames are `aligned`, the middle one is
  the `reference`, and the alignment bound holds.
- ANOTHER PICTURE: a third frame of a different picture is left out by name
  with a reason and the other two merge.
- TOO FEW FRAMES: two frames of which one is left out are refused, naming it,
  and nothing opens.
- GHOSTS at 8 and 16 bits: exposures -2, 0, +2 of a flat 0.08 background with
  a 12x12 patch of 0.5 (linear, at the reference exposure) at a different
  place in each frame; the ghost bound, and without `deghost` the short
  frame's patch shows through.
- MERGE LAYERS at 8 and 16 bits: the bracket as a stack
  (`load_files_into_stack`), then `merge_layers_to_hdr` in a caller's order
  with `evs`: the radiance bound, and the source's history cursor, its sampled
  pixels and its layer rows are what they were. Without `evs` the layers'
  EVs are `estimated` within 0.1.
- FLOAT REFUSALS: a 32-bit float source document and a 32-bit float first
  file are refused and open nothing.
- SAVE AND REOPEN: `save_copy` of a merged document as .rz, .exr, .hdr and
  32-bit .tif reopens to the same samples (.hdr within RGBE's 1%).
- CAMERA RAW: ~/devel/doctored-fixtures/camera/camera.NEF (or
  `DR_MERGE_HDR_RAW`) twice is refused as one exposure, then merges with
  `evs` and reports `raw_development`. SKIP when the file is absent.

No exposure bracket of a real scene exists under ~/devel/doctored-fixtures
(checked 2026-10-12: seven single RAW frames only), so no case merges camera
frames of different exposures; the script prints that.

Usage: python3 scripts/verify_v1_merge_hdr.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_phase1 import png  # noqa: E402
from verify_v1_stack_align import (  # noqa: E402
    App, Picture, Skipped, centre_grid, close, cursor, documents, history, rows)

ROOT = Path(__file__).resolve().parents[1]
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
EXTERNAL = Path.home() / 'devel/doctored-fixtures'
RAW = Path(os.environ.get('DR_MERGE_HDR_RAW', EXTERNAL / 'camera/camera.NEF'))

BRACKET = (464, 16)
BRACKET_EVS = [-4, 0, 4]
TINT = (1.0, 0.9, 0.8)
BOUND = {8: 0.06, 16: 5e-3}
EV_BOUND = 0.1
PICTURE = (640, 480)
SHIFTS = [(14, -9), (0, 0), (-10, 6)]
PICTURE_EVS = [-2, 0, 2]
GHOST = (96, 64)
GHOST_EVS = [-2, 0, 2]
GHOST_PATCHES = [(8, 8), (40, 24), (72, 44)]
GHOST_SIDE = 12
BACKGROUND, PATCH = 0.08, 0.5
ESTIMATE_SENTENCE = 'could not be estimated (too few pixels are well exposed in both)'


# ------------------------------------------------------------- the scene --

def srgb_encode(linear):
    return 12.92 * linear if linear <= 0.0031308 else 1.055 * linear ** (1 / 2.4) - 0.055


def srgb_decode(encoded):
    return encoded / 12.92 if encoded <= 0.04045 else ((encoded + 0.055) / 1.055) ** 2.4


def stored(linear, full):
    """A linear value clipped at 1 and stored at `full`."""
    return round(srgb_encode(min(1.0, max(0.0, linear))) * full)


def radiance(x):
    """The bracket scene's R at column x: 14 stops across the width."""
    return 6.0e-4 * 2 ** (14 * x / (BRACKET[0] - 1))


def exposure_time(ev):
    """(numerator, denominator) of the exposure time at `ev`: 1/64 s at EV 0."""
    return (2 ** ev, 64) if ev >= 0 else (1, 64 * 2 ** -ev)


def exif_ev(ev):
    """What `MergeHDR.exifEV` computes for a frame written by `tiff`: f/8, ISO 100."""
    numerator, denominator = exposure_time(ev)
    return math.log2(numerator / denominator * 100 / (100 * 8 * 8))


# ----------------------------------------------------------------- files --

def tiff(path, size, pixel, bits, ev):
    """A baseline little-endian RGB TIFF, uncompressed, one strip, with an Exif
    IFD carrying ExposureTime, FNumber (f/8) and ISOSpeedRatings (100).
    `pixel(x, y)` answers three codes at `bits` per sample."""
    width, height = size
    samples = [code for y in range(height) for x in range(width) for code in pixel(x, y)]
    data = struct.pack(f'<{len(samples)}{"H" if bits == 16 else "B"}', *samples)
    data += b'\0' * (len(data) % 2)
    ifd = 8 + len(data)
    entries = 11
    after = ifd + 2 + entries * 12 + 4
    bits_at, exif_at = after, after + 6
    exif_after = exif_at + 2 + 3 * 12 + 4
    time_at, aperture_at = exif_after, exif_after + 8

    def entry(tag, kind, count, value):
        return struct.pack('<HHII', tag, kind, count, value)

    def short(tag, value):
        return struct.pack('<HHIHH', tag, 3, 1, value, 0)

    body = struct.pack('<H', entries)
    body += entry(256, 4, 1, width) + entry(257, 4, 1, height) + entry(258, 3, 3, bits_at)
    body += short(259, 1) + short(262, 2) + entry(273, 4, 1, 8) + short(277, 3)
    body += entry(278, 4, 1, height) + entry(279, 4, 1, len(samples) * bits // 8) + short(284, 1)
    body += entry(34665, 4, 1, exif_at) + struct.pack('<I', 0)
    body += struct.pack('<3H', bits, bits, bits)
    exif = struct.pack('<H', 3)
    exif += entry(33434, 5, 1, time_at) + entry(33437, 5, 1, aperture_at) + short(34855, 100)
    exif += struct.pack('<I', 0)
    numerator, denominator = exposure_time(ev)
    exif += struct.pack('<4I', numerator, denominator, 8, 1)
    Path(path).write_bytes(struct.pack('<2sHI', b'II', 42, ifd) + data + body + exif)


def png8(path, size, pixel):
    """An 8-bit RGBA PNG, opaque; `pixel(x, y)` answers three codes."""
    width, height = size
    out = bytearray(b'\xff' * (width * height * 4))
    for y in range(height):
        for x in range(width):
            p = (y * width + x) * 4
            out[p], out[p + 1], out[p + 2] = pixel(x, y)
    png(Path(path), width, height, bytes(out))


def bracket_pixel(ev, full):
    gain = 2 ** ev
    return lambda x, _: tuple(stored(radiance(x) * tint * gain, full) for tint in TINT)


def author(folder):
    """Every fixture, written once per output folder."""
    folder.mkdir(parents=True, exist_ok=True)
    made = {}

    def once(name, write):
        path = folder / name
        if not path.is_file():
            staged = folder / f'staged-{name}'
            write(staged)
            staged.rename(path)
        made[name.rsplit('.', 1)[0]] = path
        return path

    for ev in BRACKET_EVS:
        tag = f'{ev:+d}'
        once(f'tiff8 {tag}.tif', lambda p, ev=ev: tiff(p, BRACKET, bracket_pixel(ev, 255), 8, ev))
        once(f'tiff16 {tag}.tif',
             lambda p, ev=ev: tiff(p, BRACKET, bracket_pixel(ev, 65535), 16, ev))
        once(f'png {tag}.png', lambda p, ev=ev: png8(p, BRACKET, bracket_pixel(ev, 255)))
    once('png +8.png', lambda p: png8(p, BRACKET, bracket_pixel(8, 255)))
    once('same a.png', lambda p: png8(p, BRACKET, bracket_pixel(0, 255)))
    once('same b.png', lambda p: png8(p, BRACKET, bracket_pixel(0, 255)))

    # The textured picture: radiance 0.01 * 20^v of the stack drive's noise.
    width, height = PICTURE
    picture = Picture(-256, -256, width + 256, height + 256)
    other = Picture(3000 - 256, 2000 - 256, 3000 + width + 256, 2000 + height + 256)

    def texture(source, shift, ev, origin=(0, 0)):
        gain = 2 ** ev

        def pixel(x, y):
            v = source.value(x + shift[0] + origin[0] + 0.5, y + shift[1] + origin[1] + 0.5)
            code = stored(0.01 * 20 ** v * gain, 255)
            return code, code, code
        return pixel

    for shift, ev in zip(SHIFTS, PICTURE_EVS):
        once(f'picture {ev:+d}.png',
             lambda p, shift=shift, ev=ev: png8(p, PICTURE, texture(picture, shift, ev)))
    once('other +2.png', lambda p: png8(p, PICTURE, texture(other, (0, 0), 2, (3000, 2000))))

    def ghost_pixel(k, ev, full, patch=True):
        gain = 2 ** ev

        def pixel(x, y):
            px, py = GHOST_PATCHES[k]
            inside = patch and px <= x < px + GHOST_SIDE and py <= y < py + GHOST_SIDE
            code = stored((PATCH if inside else BACKGROUND) * gain, full)
            return code, code, code
        return pixel

    for k, ev in enumerate(GHOST_EVS):
        once(f'ghost8 {ev:+d}.png', lambda p, k=k, ev=ev: png8(p, GHOST, ghost_pixel(k, ev, 255)))
        once(f'ghost16 {ev:+d}.tif',
             lambda p, k=k, ev=ev: tiff(p, GHOST, ghost_pixel(k, ev, 65535), 16, ev))
    scene = folder / 'scene.tiff'
    if not scene.is_file():
        shutil.copyfile(FLOAT_FIXTURE, scene)
    made['scene'] = scene
    return made, picture


# --------------------------------------------------------------- plumbing --

def paths(fixtures, names):
    return [str(fixtures[name]) for name in names]


def sample(app, document, x, y):
    """The merged layer's linear RGBA at a pixel."""
    reply = app.call('sample_pixel', document_id=document, source='layer', layer=0, x=x, y=y)
    values = reply['center_rgba32']
    assert len(values) == 4 and all(isinstance(v, (int, float)) for v in values), reply
    return values


def check_merged(app, reply, names, fixtures, canvas, aligned):
    """What every merge must report, and what the new document must be."""
    assert reply['ok'] is True and reply['depth'] == 'rgba32_float', reply
    assert reply['canvas'] == dict(width=canvas[0], height=canvas[1]), reply['canvas']
    assert reply['aligned'] is aligned, reply
    frames = reply['frames']
    assert [frame['name'] for frame in frames] == [Path(fixtures[name]).stem for name in names]
    assert [frame['index'] for frame in frames] == list(range(len(names))), frames
    assert [frame['path'] for frame in frames] == paths(fixtures, names), frames
    for frame in frames:
        assert ('alignment' in frame) is aligned, frame
        assert isinstance(frame['ev'], (int, float)) and frame['ev_source'] != 'unknown', frame
    assert reply['working_memory_plan_bytes'] > 16 * canvas[0] * canvas[1], reply
    summary = reply['document']
    document = summary['id']
    assert (summary['width'], summary['height']) == canvas, summary
    assert summary['layer_count'] == 1 and summary['precision'] == 'rgba32_float', summary
    assert summary['unsaved_changes'] is True and summary['path'] is None, summary
    listed = rows(app, document)
    assert [row['name'] for row in listed] == ['Merged HDR'], listed
    states = history(app, document)
    assert [state['name'] for state in states['states']] == ['Initial State'], states['states']
    assert states['cursor'] == 0
    assert app.call('get_precision', document_id=document)['bits_per_channel'] == 32
    return document


def bracket_error(app, document):
    """The worst relative error per channel against R(x) * tint, every 8th column."""
    worst = 0.0
    for x in range(0, BRACKET[0], 8):
        got = sample(app, document, x, BRACKET[1] // 2)
        assert got[3] == 1.0, (x, got)
        for channel, tint in enumerate(TINT):
            want = radiance(x) * tint
            worst = max(worst, abs(got[channel] - want) / want)
    return worst


# ------------------------------------------------------------------ cases --

def exif_case(app, fixtures, bits):
    names = [f'tiff{bits} {ev:+d}' for ev in BRACKET_EVS]
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), align=False)
    document = check_merged(app, reply, names, fixtures, BRACKET, False)
    for frame, ev in zip(reply['frames'], BRACKET_EVS):
        assert frame['ev_source'] == 'exif' and frame['used'] is True, frame
        assert abs(frame['ev'] - exif_ev(ev)) <= 1e-9, (frame['ev'], exif_ev(ev))
    assert reply['reference'] == 1, 'the middle exposure is the reference'
    assert reply['all_clipped_pixels'] == 0 and reply['all_dark_pixels'] == 0, reply
    assert reply['deghost'] is False and reply['deghosted_pixels'] == 0, reply
    assert 'raw_development' not in reply, reply
    worst = bracket_error(app, document)
    assert worst <= BOUND[bits], f'radiance off by {worst:.5f} at {bits} bits, bound {BOUND[bits]}'
    close(app, document)
    return dict(worst=worst, plan_bytes=reply['working_memory_plan_bytes'])


def png_names():
    return [f'png {ev:+d}' for ev in BRACKET_EVS]


def estimated_case(app, fixtures):
    reply = app.call('merge_to_hdr', paths=paths(fixtures, png_names()), align=False)
    document = check_merged(app, reply, png_names(), fixtures, BRACKET, False)
    evs = [frame['ev'] for frame in reply['frames']]
    for frame, ev in zip(reply['frames'], BRACKET_EVS):
        assert frame['ev_source'] == 'estimated', frame
        off = abs((frame['ev'] - evs[0]) - (ev - BRACKET_EVS[0]))
        assert off <= EV_BOUND, f'{frame["name"]} is estimated {off:.3f} EV off, bound {EV_BOUND}'
    assert reply['reference'] == 1, reply
    close(app, document)
    return dict(evs=evs)


def given_case(app, fixtures):
    reply = app.call('merge_to_hdr', paths=paths(fixtures, png_names()), evs=BRACKET_EVS,
                     align=False, name='Given Bracket')
    document = check_merged(app, reply, png_names(), fixtures, BRACKET, False)
    assert [frame['ev_source'] for frame in reply['frames']] == ['given'] * 3, reply['frames']
    assert [frame['ev'] for frame in reply['frames']] == BRACKET_EVS, reply['frames']
    assert reply['document']['title'] == 'Given Bracket', reply['document']
    worst = bracket_error(app, document)
    assert worst <= BOUND[8], f'radiance off by {worst:.5f}, bound {BOUND[8]}'
    close(app, document)
    return dict(worst=worst)


def refused_opening_nothing(app, tool, *needles, **arguments):
    listed = documents(app)
    text = app.refused(tool, *needles, **arguments)
    assert documents(app) == listed, f'{tool}: a refusal opens no document'
    return text


def far_apart(app, fixtures):
    names = ['png +0', 'png +8']
    refusal = refused_opening_nothing(
        app, 'merge_to_hdr', ESTIMATE_SENTENCE, 'png +0', 'png +8', 'evs',
        paths=paths(fixtures, names), align=False)
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=[0, 8], align=False)
    document = check_merged(app, reply, names, fixtures, BRACKET, False)
    assert [frame['ev_source'] for frame in reply['frames']] == ['given'] * 2, reply['frames']
    close(app, document)
    return dict(refusal=refusal, reference=reply['reference'])


def identical(app, fixtures):
    names = ['same a', 'same b']
    refusal = refused_opening_nothing(
        app, 'merge_to_hdr', 'same a', 'same b', 'the same exposure',
        paths=paths(fixtures, names), align=False)
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=[0, 0], align=False)
    document = check_merged(app, reply, names, fixtures, BRACKET, False)
    # Two copies of the EV 0 frame at equal EVs merge to that frame's radiance
    # wherever it is not clipped.
    x = BRACKET[0] // 2
    got, want = sample(app, document, x, 4), radiance(x)
    assert abs(got[0] - want) / want <= BOUND[8], (got, want)
    close(app, document)
    return dict(refusal=refusal)


def picture_names():
    return [f'picture {ev:+d}' for ev in PICTURE_EVS]


def picture_error(app, document, picture):
    """mean |ln(merged / R)| over the cell-centre grid."""
    total = 0.0
    points = centre_grid(PICTURE)
    for x, y in points:
        got = sample(app, document, x, y)[0]
        want = 0.01 * 20 ** picture.value(x + 0.5, y + 0.5)
        assert got > 0, (x, y, got)
        total += abs(math.log(got / want))
    return total / len(points)


def alignment(app, fixtures, picture):
    names = picture_names()
    plain = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=PICTURE_EVS, align=False)
    unaligned = check_merged(app, plain, names, fixtures, PICTURE, False)
    before = picture_error(app, unaligned, picture)
    close(app, unaligned)
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=PICTURE_EVS)
    document = check_merged(app, reply, names, fixtures, PICTURE, True)
    assert reply['reference'] == 1, reply
    assert [frame['alignment'] for frame in reply['frames']] == [
        'aligned', 'reference', 'aligned'], reply['frames']
    assert all(frame['used'] is True and 'reason' not in frame for frame in reply['frames'])
    after = picture_error(app, document, picture)
    assert after < before / 2, (f'mean |ln(merged / R)| was {before:.4f} unaligned and is '
                                f'{after:.4f} aligned; it must fall below half')
    close(app, document)
    return dict(mean_unaligned=round(before, 4), mean_aligned=round(after, 4),
                plan_bytes=reply['working_memory_plan_bytes'])


def another_picture(app, fixtures):
    names = ['picture -2', 'picture +0', 'other +2']
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=[-2, 0, 2])
    document = check_merged(app, reply, names, fixtures, PICTURE, True)
    first, middle, last = reply['frames']
    assert first['alignment'] == 'aligned' and first['used'] is True, first
    assert middle['alignment'] == 'reference' and middle['used'] is True, middle
    assert last['alignment'] == 'unregistered' and last['used'] is False, last
    assert last['name'] == 'other +2' and 'does not match the reference' in last['reason'], last
    close(app, document)
    return dict(left_out=last)


def too_few(app, fixtures):
    names = ['picture +0', 'other +2']
    refusal = refused_opening_nothing(
        app, 'merge_to_hdr', 'at least 2 frames', 'other +2', 'Turn Align off',
        paths=paths(fixtures, names), evs=[0, 2])
    return dict(refusal=refusal)


def in_patch(k, x, y):
    px, py = GHOST_PATCHES[k]
    return px <= x < px + GHOST_SIDE and py <= y < py + GHOST_SIDE


def ghosts(app, fixtures, bits):
    full = 255 if bits == 8 else 65535
    names = [f'ghost{bits} {ev:+d}' for ev in GHOST_EVS]
    reply = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=GHOST_EVS, align=False,
                     deghost=True)
    document = check_merged(app, reply, names, fixtures, GHOST, False)
    assert reply['deghost'] is True and reply['reference'] == 1, reply
    union = 3 * GHOST_SIDE * GHOST_SIDE
    count = reply['deghosted_pixels']
    assert 0 < count <= union, f'{count} deghosted pixels for a union of {union}'

    def reference_value(x, y):
        """The reference frame's own stored code there, decoded."""
        linear = PATCH if in_patch(1, x, y) else BACKGROUND
        return srgb_decode(stored(linear, full) / full)

    centres = [(px + GHOST_SIDE // 2, py + GHOST_SIDE // 2) for px, py in GHOST_PATCHES]
    worst = 0.0
    # The short frame's patch and the reference's own: the reference alone.
    for x, y in centres[:2]:
        got, want = sample(app, document, x, y)[0], reference_value(x, y)
        worst = max(worst, abs(got - want) / want)
        assert abs(got - want) / want <= 2e-3, ((x, y), got, want)
    # The long frame's patch is clipped there and has no weight; the rest of
    # the frame is background.
    for x, y in (centres[2], (2, 2), (GHOST[0] - 3, GHOST[1] - 3)):
        got = sample(app, document, x, y)[0]
        assert abs(got - BACKGROUND) / BACKGROUND <= BOUND[bits], ((x, y), got)
    close(app, document)
    plain = app.call('merge_to_hdr', paths=paths(fixtures, names), evs=GHOST_EVS, align=False)
    shown = check_merged(app, plain, names, fixtures, GHOST, False)
    assert plain['deghosted_pixels'] == 0, plain
    x, y = centres[0]
    ghost = sample(app, shown, x, y)[0]
    assert abs(ghost - BACKGROUND) / BACKGROUND > 0.1, f'no ghost to remove: {ghost}'
    close(app, shown)
    return dict(deghosted_pixels=count, union=union, worst_in_patches=worst, ghost=ghost)


def merge_layers(app, fixtures, bits):
    names = png_names() if bits == 8 else [f'tiff16 {ev:+d}' for ev in BRACKET_EVS]
    stack = app.call('load_files_into_stack', paths=paths(fixtures, names))
    source = stack['document']['id']
    assert stack['depth'] == ('rgba8' if bits == 8 else 'rgba16'), stack['depth']
    points = [(x, 8) for x in range(20, BRACKET[0], 60)]

    def source_pixels():
        return [app.call('sample_pixel', document_id=source, source='layer', layer=layer,
                         x=x, y=y)['center_rgba_codes'] for layer in range(3) for x, y in points]

    held, listed, pixels = cursor(app, source), rows(app, source), source_pixels()
    before = documents(app)
    # The caller's own order: layer 2 (EV +4), layer 0 (EV -4), layer 1 (EV 0).
    reply = app.call('merge_layers_to_hdr', document_id=source, layers=[2, 0, 1], evs=[4, -4, 0],
                     align=False)
    assert reply['ok'] is True and reply['depth'] == 'rgba32_float', reply
    assert [frame['index'] for frame in reply['frames']] == [2, 0, 1], reply['frames']
    assert [frame['ev'] for frame in reply['frames']] == [4, -4, 0], reply['frames']
    assert all(frame['ev_source'] == 'given' and 'path' not in frame for frame in reply['frames'])
    assert reply['reference'] == 1, 'the reference is the EV 0 layer'
    assert reply['source_document']['id'] == source, reply['source_document']
    document = reply['document']['id']
    assert document != source and documents(app) == sorted(before + [document])
    assert reply['document']['precision'] == 'rgba32_float', reply['document']
    assert [row['name'] for row in rows(app, document)] == ['Merged HDR']
    worst = bracket_error(app, document)
    assert worst <= BOUND[bits], f'radiance off by {worst:.5f} at {bits} bits, bound {BOUND[bits]}'
    assert cursor(app, source) == held, 'the source history did not move'
    assert rows(app, source) == listed, 'the source layers are unchanged'
    assert source_pixels() == pixels, 'the source pixels are unchanged'
    close(app, document)
    # Without evs the layers' exposures are estimated from their pixels.
    estimated = app.call('merge_layers_to_hdr', document_id=source, layers=[0, 1, 2], align=False)
    evs = [frame['ev'] for frame in estimated['frames']]
    for frame, ev in zip(estimated['frames'], BRACKET_EVS):
        assert frame['ev_source'] == 'estimated', frame
        off = abs((frame['ev'] - evs[0]) - (ev - BRACKET_EVS[0]))
        assert off <= EV_BOUND, f'layer {frame["index"]} is estimated {off:.3f} EV off'
    assert cursor(app, source) == held, 'the source history did not move'
    close(app, estimated['document']['id'])
    refused_opening_nothing(app, 'merge_layers_to_hdr', 'one finite number per frame',
                            document_id=source, layers=[0, 1, 2], evs=[0, 4], align=False)
    refused_opening_nothing(app, 'merge_layers_to_hdr', 'listed twice',
                            document_id=source, layers=[0, 0], align=False)
    close(app, source)
    return dict(worst=worst, estimated=evs, plan_bytes=reply['working_memory_plan_bytes'])


def float_refusals(app, fixtures):
    names = ['scene', 'png -4', 'png +0']
    stack = app.call('load_files_into_stack', paths=paths(fixtures, names))
    source = stack['document']['id']
    assert stack['depth'] == 'rgba32_float', stack['depth']
    seen = {}
    seen['float source'] = refused_opening_nothing(
        app, 'merge_layers_to_hdr', '32-bit float', document_id=source, layers=[1, 2],
        evs=[-4, 0], align=False)
    close(app, source)
    seen['float first file'] = refused_opening_nothing(
        app, 'merge_to_hdr', 'scene.tiff', '32-bit float', '8- or 16-bit exposures',
        paths=paths(fixtures, names), evs=[0, -4, 0], align=False)
    seen['one file'] = refused_opening_nothing(
        app, 'merge_to_hdr', '2 to 16 files', paths=paths(fixtures, names[1:2]))
    seen['evs count'] = refused_opening_nothing(
        app, 'merge_to_hdr', 'one finite number per frame',
        paths=paths(fixtures, names[1:]), evs=[0], align=False)
    seen['an EV past the span'] = refused_opening_nothing(
        app, 'merge_to_hdr', 'png +0', '16.0 EV from the reference',
        paths=paths(fixtures, names[1:]), evs=[0, 16], align=False)
    return seen


def save_and_reopen(app, fixtures, folder):
    reply = app.call('merge_to_hdr', paths=paths(fixtures, png_names()), evs=BRACKET_EVS,
                     align=False)
    document = reply['document']['id']
    points = [(x, 8) for x in range(4, BRACKET[0], 46)]
    wanted = [sample(app, document, x, y) for x, y in points]
    assert max(pixel[0] for pixel in wanted) > 1.0, 'the merge holds values above white'
    worst = {}
    for name, extra, bound in (('merged.rz', {}, 0.0), ('merged.exr', {}, 1e-6),
                               ('merged.tif', dict(bit_depth=32), 0.0),
                               ('merged.hdr', {}, 0.01)):
        path = Path(folder) / name
        if path.exists():
            path.unlink()
        app.call('save_copy', document_id=document, path=str(path), **extra)
        assert path.is_file() and path.stat().st_size > 0, path
        opened = app.call('open_document', path=str(path))['document']['id']
        assert app.call('get_precision', document_id=opened)['bits_per_channel'] == 32, name
        error = 0.0
        for (x, y), want in zip(points, wanted):
            got = sample(app, opened, x, y)
            for channel in range(3):
                error = max(error, abs(got[channel] - want[channel]) / want[channel])
        assert error <= bound, f'{name} reopens {error:.2e} off, bound {bound}'
        worst[name] = error
        close(app, opened)
    close(app, document)
    return worst


def camera_raw(app):
    if not RAW.is_file():
        raise Skipped(f'{RAW} not found')
    refusal = refused_opening_nothing(app, 'merge_to_hdr', RAW.stem, 'the same exposure',
                                      paths=[str(RAW), str(RAW)])
    reply = app.call('merge_to_hdr', paths=[str(RAW), str(RAW)], evs=[0, 1])
    assert reply['ok'] is True and reply['depth'] == 'rgba32_float', reply
    assert reply['raw_development'] == ('tone curve and shadow boost off; a 16-bit development '
                                        "clipped at the decoder's white"), reply
    assert [frame['ev_source'] for frame in reply['frames']] == ['given'] * 2, reply['frames']
    assert all(frame['used'] is True for frame in reply['frames']), reply['frames']
    close(app, reply['document']['id'])
    return dict(refusal=refusal, canvas=reply['canvas'],
                alignment=[frame.get('alignment') for frame in reply['frames']],
                plan_bytes=reply['working_memory_plan_bytes'])


# ------------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the fixtures, the instance and the receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output).resolve() if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-merge-hdr-'))
    output.mkdir(parents=True, exist_ok=True)
    assert FLOAT_FIXTURE.is_file(), f'missing float fixture {FLOAT_FIXTURE}'
    fixtures, picture = author(output / 'fixtures')
    print('note: no exposure bracket of a real scene exists under ~/devel/doctored-fixtures '
          '(checked 2026-10-12: seven single RAW frames only); every bracket here is authored.',
          flush=True)

    results, failed, skipped = {}, [], []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except Skipped as reason:
            results[name] = dict(skipped=True, reason=str(reason))
            skipped.append(name)
            print(f'SKIP {name}: {reason}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output / 'instance', 2 * 1024 ** 3) as app:
        run('exif 8-bit', lambda: exif_case(app, fixtures, 8))
        run('exif 16-bit', lambda: exif_case(app, fixtures, 16))
        run('estimated', lambda: estimated_case(app, fixtures))
        run('given', lambda: given_case(app, fixtures))
        run('no shared exposure', lambda: far_apart(app, fixtures))
        run('identical exposures', lambda: identical(app, fixtures))
        run('alignment', lambda: alignment(app, fixtures, picture))
        run('another picture', lambda: another_picture(app, fixtures))
        run('too few frames', lambda: too_few(app, fixtures))
        run('ghosts 8-bit', lambda: ghosts(app, fixtures, 8))
        run('ghosts 16-bit', lambda: ghosts(app, fixtures, 16))
        run('merge layers 8-bit', lambda: merge_layers(app, fixtures, 8))
        run('merge layers 16-bit', lambda: merge_layers(app, fixtures, 16))
        run('float refusals', lambda: float_refusals(app, fixtures))
        run('save and reopen', lambda: save_and_reopen(app, fixtures, output))
        run('camera RAW', lambda: camera_raw(app))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS' + (f' ({len(skipped)} skipped: {", ".join(skipped)})' if skipped else ''))


if __name__ == '__main__':
    main()
