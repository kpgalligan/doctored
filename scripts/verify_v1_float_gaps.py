#!/usr/bin/env python3
"""MCP drive for the v1 float gaps batch.

A 32-bit float document runs Apply Layer Mask, Layer Via Copy / Cut, Assign
Profile, Convert to Profile, Calculations, Add Luminosity Masks and Contact
Sheet over MCP; Apply Image stays refused by name.

The script owns its app instance (`verify_v1_16bit.App`): the Debug app on
its own scratch MCP socket with `DR_BACKGROUND=1`, a dummy
`ANTHROPIC_API_KEY`, the 2 GiB working-limit pin and private `DR_*_DIR`s. It
terminates only that process. Every expected number is computed here, never
read back from the app; float pixels are read by 32-bit TIFF export
(`verify_phase11f_float.export32`), whose fixture and stdlib readers this
file shares.

Cases, on copies of `core/tests/fixtures/float_11f/scene.tiff` (48x32 linear
RGBA f32 with a block above white, a block with a negative component and a
half-alpha band):

- MASK: a feathered selection becomes the layer's mask and
  `remove_layer_mask apply: true` bakes it: alpha is a * c / 65535 with c the
  selection's u16 coverage, RGB kept (`rgb_kept`: bit for bit where the alpha
  is untouched, 1e-6 where it is fractional, because the composite export
  un-premultiplies such a pixel), reply `storage: rgba32_float`, one history
  step; undo restores the layer's bits.
- VIA: `layer_via_copy` under a feathered selection lands a layer whose RGB
  is the source's (`rgb_kept`) and whose alpha is a * c / 65535, leaving the
  source alone; `layer_via_cut` lands the same layer and leaves the source
  alpha a * (1 - c / 65535). One history step each; undo restores the bits.
- ASSIGN: `assign_profile display_p3` keeps every raster sample bit for bit
  and changes the profile, on a plain document and on one holding a shape
  layer; the shape stays a shape and re-renders as
  inverse(P3 colorants) * sRGB colorants * its linear fill numbers (F20's
  oracle with the new profile's colorants). One step; undo restores the bits.
- CONVERT: `convert_profile display_p3` moves every raster sample by
  inverse(P3 colorants) * document colorants (both read from the files' own
  ICC tags), no clamp, alpha bit for bit, on a plain document and on one
  holding a shape layer; the shape stays a shape with the
  same interior as after Assign. One step; undo restores the bits.
  `black_point_compensation: true` and `intent: "absolute"` are refused by
  the options sentence, and a LUT RGB destination answers `changed: false`
  with the float destination sentence, which names no assign_profile.
- LUMA: `add_luminosity_masks` adds nine channels; each, loaded as the
  selection, is L, L^2, L^3, (1-L), (1-L)^2, (1-L)^3 and
  1 - L^(n+1) - (1-L)^(n+1) for n = 1..3, with L = clamp(profile Y row . rgb),
  within 3 codes of 65535. One step; undo removes all nine.
- CALC: `calculations` of merged red (source 1) over merged blue (source 2)
  in Multiply is clamp(red) * clamp(blue), as the selection and as a new
  channel, within 3 codes of 65535. The channel is one step; undo removes it.
- SHEET: `contact_sheet` (one column, cell = canvas, no captions, a new
  document) is the 8-bit rendering: each opaque pixel is the clip tone map
  then the profile's own encode, within 2 codes of 255.
- REFUSED: `apply_image` names the float refusal and takes no step, and the
  operator table (`get_precision`) lists Apply Image as refused.

Usage: python3 scripts/verify_v1_float_gaps.py [--output DIR]
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
from verify_phase11f_float import (  # noqa: E402
    FIXTURE, HEIGHT, T_REL, WIDTH, bits_equal, encode_through, export32, f16_apply, f16_inv,
    f16_mul, f20_box, f20_columns, f20_interior, f20_png_icc, f20_rows, forward_tables,
    icc_basis, icc_trc_tables, read_float_tiff, scene, tone_map)

# ---- the tolerances, fixed here before anything is measured ----------------
T_ALPHA = 1e-6        # one f32 product against the f64 oracle
T_RGB = 1e-6          # RGB of a fractional-alpha pixel read through the composite
T_COVERAGE = 3        # LUMA and CALC: codes of 65535
T_CODE8 = 2           # SHEET: codes of 255

# The shape the ASSIGN and CONVERT documents hold (F20's (a)): authored at 32
# bits, so its fill numbers are linear light in sRGB's primaries.
SHAPE_FILL = [46 / 255.0, 128 / 255.0, 200 / 255.0]
SHAPE_RECT = dict(x=8, y=8, w=24, h=12)
SHAPE_INTERIOR = (10, 10, 30, 18)

FLOAT_DESTINATION = ('Doctored cannot convert a 32-bit float document into “{name}” '
                     "— it has no matrix/TRC model, and a float document's linear "
                     "values are defined by its profile's primaries. Choose a matrix/TRC "
                     'RGB profile.')
OPTIONS_REFUSAL = ('Convert to Profile in a 32-bit float document converts in linear light with '
                   'no clamp (relative colorimetric, no black-point compensation): {what} is '
                   'unavailable there.')
LUT_NAME = 'Synthetic LUT RGB'


# ------------------------------------------------------------- plumbing --

def fresh(app, folder, label):
    """A new float document: a private copy of the fixture, opened."""
    copy = folder / f'{label}.tiff'
    copy.write_bytes(FIXTURE.read_bytes())
    return app.call('open_document', path=str(copy))['document']['id']


def pixel(samples, x, y):
    i = (y * WIDTH + x) * 4
    return samples[i:i + 4]


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
    """A feathered rectangle crossing the above-white, negative and half-alpha
    parts of the scene; returns its coverage."""
    app.call('select_rect', document_id=document, x=6, y=4, width=30, height=24)
    app.call('modify_selection', document_id=document, operation='feather', radius=2)
    codes = coverage(app, document)
    assert 0 in codes and 65535 in codes and any(0 < c < 65535 for c in codes), (
        'the selection has full, partial and empty samples')
    return codes


def visible(app, document, index, shown):
    app.call('set_layer_properties', document_id=document, index=index, visible=shown)


def layer_alone(app, folder, document, index, count, label):
    """Layer `index`'s samples, read as the composite with every other layer
    hidden; the visibility steps are undone again."""
    others = [i for i in range(count) if i != index]
    for other in others:
        visible(app, document, other, False)
    samples = export32(app, document, folder / f'{label}.tiff')['samples']
    for _ in others:
        app.call('undo', document_id=document)
    return samples


def rgb_kept(now, was):
    """`now`'s RGB against `was`'s, as the composite export can show it: bit
    for bit where the alpha is the source's own, and within T_RGB where the
    alpha is fractional (the export un-premultiplies such a pixel, which is
    one f32 rounding; the core's tests hold the stored samples bit for bit)."""
    if bits_equal(now[3:], was[3:]):
        return bits_equal(now[:3], was[:3])
    return all(abs(n - w) <= T_RGB * max(abs(w), 1.0) for n, w in zip(now[:3], was[:3]))


def profile_name(app, document):
    return app.call('get_color_profile', document_id=document)['name']


def lut_profile():
    """An RGB LUT-only ICC v2 profile (identity `mft2` A2B0 / B2A0 on a 2-point
    grid, PCS XYZ, no matrix/TRC tags), built byte by byte."""
    def s15(v):
        return struct.pack('>i', round(v * 65536.0))

    def desc(name):
        raw = name.encode('ascii')
        return b'desc\0\0\0\0' + struct.pack('>I', len(raw) + 1) + raw + b'\0'

    def mft2():
        out = b'mft2\0\0\0\0' + bytes([3, 3, 2, 0])
        out += b''.join(s15(v) for v in (1, 0, 0, 0, 1, 0, 0, 0, 1))
        out += struct.pack('>HH', 2, 2)
        out += struct.pack('>HH', 0, 65535) * 3
        for index in range(8):
            for axis in range(3):
                out += struct.pack('>H', 65535 * ((index >> (2 - axis)) & 1))
        out += struct.pack('>HH', 0, 65535) * 3
        return out

    tags = [(b'desc', desc(LUT_NAME)),
            (b'wtpt', b'XYZ \0\0\0\0' + s15(0.9642) + s15(1.0) + s15(0.8249)),
            (b'A2B0', mft2()), (b'B2A0', mft2())]
    start = 132 + 12 * len(tags)
    table, payload = b'', b''
    for signature, data in tags:
        table += signature + struct.pack('>II', start + len(payload), len(data))
        payload += data + b'\0' * (-len(data) % 4)
    header = bytearray(128)
    struct.pack_into('>I', header, 0, start + len(payload))
    header[8:12] = bytes([0x02, 0x20, 0, 0])
    header[12:16], header[16:20], header[20:24] = b'mntr', b'RGB ', b'XYZ '
    header[36:40] = b'acsp'
    return bytes(header) + struct.pack('>I', len(tags)) + table + payload


def srgb_columns(app, folder):
    """The colorant matrix of the sRGB profile this build writes, read off a
    16-bit sRGB document's PNG export (F20's way)."""
    document = app.call('new_document', width=8, height=8, depth=16, profile='srgb',
                        background='transparent')['document']['id']
    probe = folder / 'srgb-probe.png'
    app.call('export_image', document_id=document, path=str(probe),
             settings={'format': 'png', 'bit_depth': 16}, overwrite=True)
    app.call('close_document', document_id=document, discard_changes=True)
    return f20_columns(f20_png_icc(probe))


def in_shape(x, y, margin=0):
    r = SHAPE_RECT
    return (r['x'] - margin <= x < r['x'] + r['w'] + margin
            and r['y'] - margin <= y < r['y'] + r['h'] + margin)


# ---------------------------------------------------------------- cases --

def mask(app, folder):
    document = fresh(app, folder, 'mask')
    base = scene()
    covered = feathered(app, document)
    app.call('add_layer_mask', document_id=document, layer=0, kind='from_selection')
    app.call('deselect', document_id=document)
    start = cursor(app, document)
    reply = app.call('remove_layer_mask', document_id=document, layer=0, apply=True)
    assert reply.get('ok') and reply.get('changed') and reply.get('applied'), reply
    assert reply.get('storage') == 'rgba32_float', reply
    assert cursor(app, document) == start + 1, 'apply is one history step'
    assert not f20_rows(app, document)[0].get('has_mask', False), 'the mask is consumed'
    got = export32(app, document, folder / 'mask-applied.tiff')['samples']
    worst, partial = 0.0, 0
    for k, (x, y) in enumerate(all_points()):
        was, now = pixel(base, x, y), pixel(got, x, y)
        want = was[3] * covered[k] / 65535.0
        worst = max(worst, abs(now[3] - want))
        partial += 0.0 < want < was[3]
        if want > 0.0:
            assert rgb_kept(now, was), ('RGB', x, y, now, was)
        if covered[k] == 65535:
            assert bits_equal(now[3:], was[3:]), ('full coverage is exact', x, y)
    assert worst <= T_ALPHA and partial >= 8, (worst, partial)
    # The above-white and negative blocks kept their numbers.
    assert pixel(got, 8, 6)[:3] == [8.0, 4.0, 2.0] and pixel(got, 14, 6)[0] == -0.25
    app.call('undo', document_id=document)
    assert cursor(app, document) == start
    # The mask is back; with it disabled the composite is the layer's bits.
    app.call('set_layer_mask_enabled', document_id=document, layer=0, enabled=False)
    restored = export32(app, document, folder / 'mask-undone.tiff')['samples']
    assert bits_equal(restored, base), 'undo restores the layer bit for bit'
    return dict(worst_alpha=worst, partial=partial)


def via(app, folder):
    document = fresh(app, folder, 'via')
    base = scene()
    covered = feathered(app, document)
    seen = {}
    for tool, cut in (('layer_via_copy', False), ('layer_via_cut', True)):
        start = cursor(app, document)
        reply = app.call(tool, document_id=document, layer=0)
        assert reply.get('ok') and reply['layer'] == 1, reply
        assert reply.get('storage') == 'rgba32_float', reply
        assert 'whole layer' not in reply.get('note', ''), reply
        assert cursor(app, document) == start + 1, f'{tool} is one history step'
        rows = f20_rows(app, document)
        assert len(rows) == 2 and rows[1]['name'] == reply['name'], rows
        copy = layer_alone(app, folder, document, 1, 2, f'via-{tool}-copy')
        source = layer_alone(app, folder, document, 0, 2, f'via-{tool}-source')
        worst = 0.0
        for k, (x, y) in enumerate(all_points()):
            was, c = pixel(base, x, y), covered[k] / 65535.0
            lifted, left = pixel(copy, x, y), pixel(source, x, y)
            worst = max(worst, abs(lifted[3] - was[3] * c))
            if was[3] * c > 0.0:
                assert rgb_kept(lifted, was), (tool, 'copy RGB', x, y, lifted, was)
            if covered[k] == 65535:
                assert bits_equal(lifted[3:], was[3:]), (tool, 'full coverage', x, y)
            want_left = was[3] * (1.0 - c) if cut else was[3]
            worst = max(worst, abs(left[3] - want_left))
            if want_left > 0.0:
                assert rgb_kept(left, was), (tool, 'source RGB', x, y, left, was)
            if not cut or covered[k] == 0:
                assert bits_equal(left, was), (tool, 'source untouched', x, y)
        assert worst <= T_ALPHA, (tool, worst)
        app.call('undo', document_id=document)
        assert cursor(app, document) == start and len(f20_rows(app, document)) == 1, tool
        undone = export32(app, document, folder / f'via-{tool}-undone.tiff')['samples']
        assert bits_equal(undone, base), f'undo restores the bits after {tool}'
        seen[tool] = dict(worst_alpha=worst, name=reply['name'])
    return seen


def with_shape(app, folder, label):
    """A fresh float document holding one shape layer; its id, the shape's
    index and its exported samples."""
    document = fresh(app, folder, label)
    shape = app.call('add_shape_layer', document_id=document, kind='rect',
                     fill_rgba=SHAPE_FILL + [1.0], **SHAPE_RECT)
    layer = shape['layer']
    row = f20_rows(app, document)[layer].get('shape') or {}
    assert row.get('ink') == 'linear_srgb', row
    before = export32(app, document, folder / f'{label}-before.tiff')
    return document, layer, before


def shape_check(app, document, layer, samples, columns, srgb):
    """The shape is still a shape, and its interior is the fill's linear sRGB
    numbers in the profile whose colorants are `columns`."""
    assert 'shape' in f20_rows(app, document)[layer], 'the layer is still a shape'
    want = f16_apply(f16_mul(f16_inv(columns), srgb), SHAPE_FILL)
    passed, detail = f20_interior(f20_box(samples, *SHAPE_INTERIOR), want, T_REL)
    assert passed, ('shape interior', detail, want)
    return dict(want=want, **detail)


def plain_assign(app, folder):
    """A document with no artwork takes the core's Assign directly: every
    sample bit for bit under the new profile, one step, undone bit for bit."""
    document = fresh(app, folder, 'assign-plain')
    base = scene()
    start = cursor(app, document)
    reply = app.call('assign_profile', document_id=document, profile='display_p3')
    assert reply.get('ok') and reply.get('changed'), reply
    assert cursor(app, document) == start + 1, 'assign is one history step'
    after = export32(app, document, folder / 'assign-plain-after.tiff')
    assert bits_equal(after['samples'], base), 'a plain Assign keeps every sample'
    assert after['icc'] != read_float_tiff(FIXTURE)['icc'], 'the exported profile changed'
    app.call('undo', document_id=document)
    undone = export32(app, document, folder / 'assign-plain-undone.tiff')
    assert cursor(app, document) == start and bits_equal(undone['samples'], base)


def converted(base, got, matrix, skip=lambda x, y: False):
    """`got` against `matrix` over `base`'s RGB, alpha bit for bit: the worst
    relative error, and how many pixels hold a negative or an above-white
    component."""
    worst, negative, above = 0.0, 0, 0
    for x, y in all_points():
        if skip(x, y):
            continue
        was, now = pixel(base, x, y), pixel(got, x, y)
        want = f16_apply(matrix, was[:3])
        for c in range(3):
            worst = max(worst, abs(now[c] - want[c]) / max(abs(want[c]), 1.0))
        assert bits_equal(now[3:], was[3:]), ('alpha bits', x, y)
        negative += any(v < 0.0 for v in now[:3])
        above += any(v > 1.0 for v in now[:3])
    return worst, negative, above


def plain_convert(app, folder):
    """A document with no artwork takes the core's Convert directly."""
    document = fresh(app, folder, 'convert-plain')
    base = scene()
    start = cursor(app, document)
    reply = app.call('convert_profile', document_id=document, profile='display_p3')
    assert reply.get('ok') and reply.get('changed'), reply
    assert cursor(app, document) == start + 1, 'convert is one history step'
    after = export32(app, document, folder / 'convert-plain-after.tiff')
    matrix = f16_mul(f16_inv(f20_columns(after['icc'])),
                     f20_columns(read_float_tiff(FIXTURE)['icc']))
    worst, negative, above = converted(base, after['samples'], matrix)
    assert worst <= T_REL and negative >= 8 and above >= 8, (worst, negative, above)
    app.call('undo', document_id=document)
    undone = export32(app, document, folder / 'convert-plain-undone.tiff')
    assert cursor(app, document) == start and bits_equal(undone['samples'], base)
    return worst


def assign(app, folder, srgb):
    plain_assign(app, folder)
    document, layer, before = with_shape(app, folder, 'assign')
    base = before['samples']
    name = profile_name(app, document)
    start = cursor(app, document)
    reply = app.call('assign_profile', document_id=document, profile='display_p3')
    assert reply.get('ok') and reply.get('changed'), reply
    assert cursor(app, document) == start + 1, 'assign is one history step'
    assigned = profile_name(app, document)
    assert assigned != name and reply.get('previous_profile') == name, (name, assigned, reply)
    after = export32(app, document, folder / 'assign-after.tiff')
    assert after['icc'] != before['icc'], 'the exported profile changed'
    got = after['samples']
    # Every raster sample outside the shape (one pixel of margin) is bit-identical.
    for x, y in all_points():
        if not in_shape(x, y, margin=1):
            assert bits_equal(pixel(got, x, y), pixel(base, x, y)), ('raster bits', x, y)
    shape = shape_check(app, document, layer, got, f20_columns(after['icc']), srgb)
    # The document's own profile is no change.
    again = app.call('assign_profile', document_id=document, profile='display_p3')
    assert again.get('changed') is False and cursor(app, document) == start + 1, again
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and profile_name(app, document) == name
    undone = export32(app, document, folder / 'assign-undone.tiff')
    assert bits_equal(undone['samples'], base) and undone['icc'] == before['icc'], (
        'undo restores the bits and the profile')
    assert 'shape' in f20_rows(app, document)[layer]
    return dict(profile=assigned, previous=name, shape=shape)


def convert(app, folder, srgb):
    plain_worst = plain_convert(app, folder)
    document, layer, before = with_shape(app, folder, 'convert')
    base = before['samples']
    name = profile_name(app, document)
    start = cursor(app, document)

    # The options a float Convert cannot follow refuse by name, and take no step.
    for extra, what in ((dict(black_point_compensation=True), 'black-point compensation'),
                        (dict(intent='absolute'), 'absolute colorimetric intent')):
        app.refused('convert_profile', OPTIONS_REFUSAL.format(what=what),
                    document_id=document, profile='display_p3', **extra)
    # A LUT RGB destination: in band, the float sentence, no pointer at Assign.
    lut = folder / 'synthetic-lut.icc'
    lut.write_bytes(lut_profile())
    reply = app.call('convert_profile', document_id=document, profile='file', path=str(lut))
    assert reply.get('changed') is False, reply
    assert FLOAT_DESTINATION.format(name=LUT_NAME) in reply.get('note', ''), reply
    assert 'assign_profile' not in json.dumps(reply), reply
    assert cursor(app, document) == start, 'a refused conversion leaves no step'

    reply = app.call('convert_profile', document_id=document, profile='display_p3')
    assert reply.get('ok') and reply.get('changed'), reply
    assert cursor(app, document) == start + 1, 'convert is one history step'
    landed = profile_name(app, document)
    assert landed != name, (name, landed)
    after = export32(app, document, folder / 'convert-after.tiff')
    got = after['samples']
    destination = f20_columns(after['icc'])
    matrix = f16_mul(f16_inv(destination), f20_columns(before['icc']))
    worst, negative, above = converted(base, got, matrix,
                                       skip=lambda x, y: in_shape(x, y, margin=1))
    # No clamp: values above white stay above white and some stay negative.
    assert worst <= T_REL and negative >= 8 and above >= 8, (worst, negative, above)
    shape = shape_check(app, document, layer, got, destination, srgb)
    app.call('undo', document_id=document)
    assert cursor(app, document) == start and profile_name(app, document) == name
    undone = export32(app, document, folder / 'convert-undone.tiff')
    assert bits_equal(undone['samples'], base) and undone['icc'] == before['icc'], (
        'undo restores the bits and the profile')
    assert 'shape' in f20_rows(app, document)[layer]
    return dict(profile=landed, previous=name, worst=worst, plain_worst=plain_worst,
                negative=negative, above=above, matrix=matrix, shape=shape)


def luminance():
    """The scene's L per pixel: the fixture profile's Y row over the unclamped
    linear RGB, the sum clamped to 0...1 (F13's oracle)."""
    y_row = icc_basis(read_float_tiff(FIXTURE)['icc'])[0][1]
    base = scene()
    return [min(1.0, max(0.0, sum(w * c for w, c in zip(y_row, pixel(base, x, y)[:3]))))
            for x, y in all_points()]


def code16(value):
    return int(min(1.0, max(0.0, value)) * 65535.0 + 0.5)


def luma(app, folder):
    document = fresh(app, folder, 'luma')
    before = len(app.call('list_channels', document_id=document)['channels'])
    start = cursor(app, document)
    reply = app.call('add_luminosity_masks', document_id=document)
    assert reply.get('ok') and reply.get('changed'), reply
    oracles = {}
    for n in (1, 2, 3):
        oracles[f'Lights {n}'] = lambda v, n=n: v ** n
        oracles[f'Darks {n}'] = lambda v, n=n: (1.0 - v) ** n
        oracles[f'Midtones {n}'] = lambda v, n=n: 1.0 - v ** (n + 1) - (1.0 - v) ** (n + 1)
    assert sorted(reply['added']) == sorted(oracles), reply
    assert cursor(app, document) == start + 1, 'the nine masks are one history step'
    channels = app.call('list_channels', document_id=document)['channels']
    assert len(channels) == before + 9, channels
    light = luminance()
    assert min(light) < 0.2 and max(light) == 1.0 and any(0.4 < v < 0.6 for v in light), (
        'the scene has dark, mid and clipped luminance')
    worst = {}
    for name, rule in oracles.items():
        app.call('load_selection', document_id=document, **{'from': 'channel', 'channel': name})
        got = coverage(app, document)
        worst[name] = max(abs(g - code16(rule(v))) for g, v in zip(got, light))
        assert worst[name] <= T_COVERAGE, (name, worst[name])
    app.call('deselect', document_id=document)
    # Undo back to the step before the masks: all nine go at once.
    while cursor(app, document) > start:
        app.call('undo', document_id=document)
    assert len(app.call('list_channels', document_id=document)['channels']) == before
    return dict(worst_codes=worst)


def calc(app, folder):
    document = fresh(app, folder, 'calc')
    base = scene()
    clamp = lambda v: min(1.0, max(0.0, v))
    want = [code16(clamp(pixel(base, x, y)[0]) * clamp(pixel(base, x, y)[2]))
            for x, y in all_points()]
    assert len(set(want)) > 16, 'the product varies across the scene'
    request = dict(document_id=document, source1='merged', source1_plane='red',
                   source2='merged', source2_plane='blue', blend_mode='Multiply')
    app.call('calculations', result='selection', **request)
    got = coverage(app, document)
    worst_selection = max(abs(g - w) for g, w in zip(got, want))
    assert worst_selection <= T_COVERAGE, worst_selection
    app.call('deselect', document_id=document)
    before = len(app.call('list_channels', document_id=document)['channels'])
    start = cursor(app, document)
    reply = app.call('calculations', result='new_channel', name='Product', **request)
    assert reply.get('ok') and reply.get('result') == 'new_channel', reply
    assert cursor(app, document) == start + 1, 'a new channel is one history step'
    assert len(app.call('list_channels', document_id=document)['channels']) == before + 1
    app.call('load_selection', document_id=document, **{'from': 'channel', 'channel': 'Product'})
    got = coverage(app, document)
    worst_channel = max(abs(g - w) for g, w in zip(got, want))
    assert worst_channel <= T_COVERAGE, worst_channel
    app.call('deselect', document_id=document)
    # Undo back to the step before the channel: it goes.
    while cursor(app, document) > start:
        app.call('undo', document_id=document)
    assert len(app.call('list_channels', document_id=document)['channels']) == before, (
        'undo removes the new channel')
    return dict(worst_selection=worst_selection, worst_channel=worst_channel)


def sheet(app, folder):
    document = fresh(app, folder, 'sheet')
    base = scene()
    start = cursor(app, document)
    reply = app.call('contact_sheet', document_id=document, items='all',
                     layout=dict(columns=1, cell_width=WIDTH, cell_height=HEIGHT, gutter=0,
                                 caption_height=0, captions=False, filter='nearest'),
                     output=dict(kind='document'))
    assert reply.get('ok') and reply.get('cells') == 1, reply
    assert (reply['width'], reply['height']) == (WIDTH, HEIGHT), reply
    assert cursor(app, document) == start, 'the source document is unchanged'
    made = reply['document']['id']
    state = app.call('get_document', document_id=made)
    assert state.get('precision') not in ('rgba32_float', 'rgba16_unorm'), state.get('precision')
    values = forward_tables(icc_trc_tables(read_float_tiff(FIXTURE)['icc']))
    worst, read = 0, 0
    # Opaque pixels only (y < 24): the half-alpha band shows the background.
    for y in range(0, 24, 3):
        for x in range(0, WIDTH, 5):
            got = app.call('sample_pixel', document_id=made, source='layer', layer=0, x=x, y=y)
            got = [int(v) for v in got['center_rgba_codes']]
            was = pixel(base, x, y)
            for c in range(3):
                want = round(encode_through(values[c], tone_map(was[c], 'clip')) * 255.0)
                worst = max(worst, abs(got[c] - want))
            assert got[3] == 255, (x, y, got)
            read += 1
    assert worst <= T_CODE8 and read >= 60, (worst, read)
    return dict(worst_codes=worst, pixels=read, size=[reply['width'], reply['height']])


def refused(app, folder):
    document = fresh(app, folder, 'refused')
    start = cursor(app, document)
    text = app.refused('apply_image', 'apply_image is unavailable in a 32-bit float document',
                       document_id=document, source='merged', source_plane='red',
                       blend_mode='Multiply')
    assert cursor(app, document) == start, 'a refused Apply Image leaves no step'
    table = app.call('get_precision', document_id=document)
    rows = [row.get('operator') for row in
            (table.get('support') or table).get('operators', {}).get('refused', [])]
    assert 'Apply Image' in rows, ('the operator table names Apply Image as refused', rows)
    return dict(refusal=text[:120], operator_row=True)


# ----------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-v1-float-gaps-'))
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
        srgb = srgb_columns(app, output)
        run('MASK', lambda: mask(app, output))
        run('VIA', lambda: via(app, output))
        run('ASSIGN', lambda: assign(app, output, srgb))
        run('CONVERT', lambda: convert(app, output, srgb))
        run('LUMA', lambda: luma(app, output))
        run('CALC', lambda: calc(app, output))
        run('SHEET', lambda: sheet(app, output))
        run('REFUSED', lambda: refused(app, output))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
