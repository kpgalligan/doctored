#!/usr/bin/env python3
"""The Phase 11E linear-light MCP drive, on a built Release app.

Seven gates, FIXED before the run; a failed gate IS the receipt and no
tolerance is widened after the fact.

- **E1 an 8-bit document.** A layered fixture — an opaque base, an exposure
  `document_v2` adjustment layer DIRECTLY above it, a Multiply layer at 50 %
  and a masked Screen layer inside a Pass Through group — is switched with
  `set_evaluation` after `get_evaluation preview:true`, and its `render` is
  compared with an oracle written in this file: decode with the analytic sRGB
  curve, composite with the W3C formula, replicate the byte adjustment
  transcode's code rounding, re-encode with the same forward-threshold rule.
  **T1** mean |delta| <= 0.05, max <= 1 code. On a second copy of the same
  fixture the base is grouped ON ITS OWN and the group given a Normal blend
  mode, so the projection crosses `doc_group::rendered_group` — the one
  boundary that quantizes a private accumulator — and must be sample-identical
  to the ungrouped one.
- **E2 a 16-bit document.** The same shape at typed depth, exported through
  `export_image` at `bit_depth: 16` and read by the 16-bit PNG reader below.
  The typed transcode is CONTINUOUS, so the oracle's adjustment is
  `document_v2` Exposure (its pure gamma 2.2) with no rounding between.
  **T2** mean <= 2, max <= 8 codes of 65535. The same
  isolated-group identity is asserted here on the fixture's own `All` group,
  which typed grouping builds around every top-level layer at once.
- **E3 persistence.** `save_copy` to `.rz`, whose wrapper chain carries the
  byte evaluation word (38), reopened: the policy comes back `linear_light`
  and every rendered sample is identical.
- **E4 snapshots and recovery.** `save_snapshot` / `restore_snapshot`, and
  `write_recovery` + a forced crash + relaunch + `recover_document`, all keep
  the policy and the projection.
- **E5 undo.** One `undo` returns the document to `encoded` and reproduces
  the pre-switch export's sha256 exactly.
- **E6 encoded documents are untouched.** An encoded export taken before and
  after the whole run is byte-identical, and an encoded `.rz`'s wrapper chain
  carries no evaluation word.
- **E7 every operator is admitted.** A document with a Hue blend mode
  switches; on the switched E1 document a Hue blend mode, a colour overlay, a
  Blend If, a guided Shadows/Highlights and `image_size` each land, after
  each `get_evaluation` reports a null `linear_light_refusal` and an empty
  `refused` list, `render`'s sha256 changes, and one `undo` restores it. A
  placed image is admitted: on a fresh switched document `place_image`
  succeeds with a null `projection_error`.

Nothing here derives an expectation from Doctored: the fixtures are written
by this file, the colour arithmetic is written out below, the PNG readers are
this file's, and the digests come from `hashlib`.
"""
import argparse
import base64
import bisect
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import phase5_support
from phase5_support import App

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
T1_MEAN_CODES = 0.05
T1_MAX_CODES = 1
# A layer opacity of 0.5 composites as its opacity byte (`opacity_byte`).
OPACITY_HALF = round(0.5 * 255) / 255
# `document_v2` Exposure's exponent in an sRGB-shaped profile (`adjust_calibrated`).
EXPOSURE_GAMMA = 2.2
T2_MEAN_CODES = 2.0
T2_MAX_CODES = 8

WIDTH = 32
HEIGHT = 24
EXPOSURE_STOPS = 0.5
MASK_RECT = (6, 4, 18, 13)   # x, y, w, h — the Screen layer's from_selection mask
EVALUATION_BYTE_WORD = 38   # evaluation_native::BYTE_VERSION


# The `.rz` WRAPPER CHAIN. Every conditional wrapper from v25 on writes its
# OWN version word in front of the word it wraps, and 12E-1's identity wrapper
# (45 byte / 46 typed) is written UNCONDITIONALLY, so the retained-source
# record version — byte 14 / typed 15, `native_sources`'
# own — is no longer the word at byte offset 4. `wrapped_version` therefore walks the
# chain the way core/tests/common/identity.rs peels 45/46 off, and asserts
# every word it walks past is a KNOWN wrapper, so an unknown word can never be
# skipped silently: the walk stops at the first non-wrapper word, which is the
# one the claim below is about. The table must cover every word from 25 up to
# the core's current `DRDC_VERSION` (core/src/drdc.rs); `wrapped_version`
# asserts that, so a new wrapper fails loudly here until it is named.
WRAPPER_WORDS = {
    25: "channel metadata", 26: "blend basic", 27: "blend separable",
    28: "mask outside (byte)", 29: "mask outside (typed)",
    30: "blend four-mode", 31: "blend five-mode", 32: "typed clip",
    33: "blend HSL", 34: "blend threshold", 35: "Blend If", 36: "layer style",
    37: "adjustment blending", 38: "evaluation (byte)",
    39: "evaluation (typed)", 40: "float", 41: "labels (byte)",
    42: "labels (typed)", 43: "comps (byte)", 44: "comps (typed)",
    45: "identity (byte)", 46: "identity (typed)", 47: "locks and links",
    48: "guides", 49: "knockout (byte)", 50: "knockout (typed)",
    51: "projective retained source (byte)",
    52: "projective retained source (typed)",
    53: "warp (byte)", 54: "warp (typed)", 55: "group open (typed)",
    56: "source profile refusal (byte)", 57: "source profile refusal (typed)",
    58: "global light (typed)", 59: "color samplers (byte)",
    60: "color samplers (typed)", 61: "levels (byte)", 62: "levels (typed)",
    63: "curves (byte)", 64: "curves (typed)",
    65: "smart object (byte)", 66: "smart object (typed)",
    67: "float master (typed)", 68: "float smart object contents (typed)",
    69: "stack mode (byte)", 70: "stack mode (typed)", 71: "color lookup look",
    72: "float master (byte)", 73: "float contents (byte)",
    74: "paths (byte)", 75: "paths (typed)",
}


def drdc_version():
    """The newest `.doctored` version word the core writes (`drdc::DRDC_VERSION`)."""
    source = (Path(__file__).resolve().parents[1] / "core/src/drdc.rs").read_text()
    found = re.findall(r"pub\(crate\) const DRDC_VERSION: u32 = (\d+);", source)
    assert len(found) == 1, found
    return int(found[0])


def wrapped_version(data):
    """The version word the wrapper chain finally carries, and the wrappers
    peeled to reach it (each asserted to be a known one)."""
    assert set(WRAPPER_WORDS) == set(range(25, drdc_version() + 1)), \
        ("WRAPPER_WORDS must name every wrapper 25..DRDC_VERSION", sorted(WRAPPER_WORDS), drdc_version())
    peeled, at = [], 4
    while True:
        word = struct.unpack_from("<I", data, at)[0]
        if word not in WRAPPER_WORDS:
            return word, peeled
        peeled.append((word, WRAPPER_WORDS[word]))
        at += 4
        assert at < len(data), ("no version word past the wrappers", peeled)



# --------------------------------------------------------- colour oracle --

def srgb_decode(value):
    """Encoded [0, 1] -> linear, from the sRGB specification."""
    if value <= 0.04045:
        return value / 12.92
    return ((value + 0.055) / 1.055) ** 2.4


def srgb_encode(linear):
    """The analytic inverse — the CONTINUOUS encode the typed path uses."""
    linear = min(max(linear, 0.0), 1.0)
    if linear <= 0.0031308:
        return linear * 12.92
    return 1.055 * linear ** (1 / 2.4) - 0.055


# `thresh[k]` is the linear value at which output code k rounds up to k + 1:
# the curve evaluated FORWARD at (k + 0.5) / 255. The crate has no curve
# inverter and neither does this oracle.
THRESHOLDS = [srgb_decode((k + 0.5) / 255.0) for k in range(255)]


def encode_code(linear):
    """The byte forward-threshold encode: how many thresholds sit below."""
    return bisect.bisect_left(THRESHOLDS, min(max(linear, 0.0), 1.0))


def blend(mode, cb, cs):
    if mode == 'normal':
        return cs
    if mode == 'multiply':
        return cb * cs
    if mode == 'screen':
        return cb + cs - cb * cs
    raise AssertionError(f'unknown mode {mode}')


def over(backdrop, source, alpha, mode):
    """W3C source-over of a straight-alpha source over a straight backdrop."""
    if alpha <= 0.0:
        return backdrop
    ab = backdrop[3]
    ao = alpha + ab * (1.0 - alpha)
    out = [0.0, 0.0, 0.0, ao]
    for c in range(3):
        out[c] = (alpha * (1.0 - ab) * source[c]
                  + alpha * ab * blend(mode, backdrop[c], source[c])
                  + (1.0 - alpha) * ab * backdrop[c]) / ao
    return out


def exposure_kernel(encoded, gain):
    """`document_v2` Exposure on one encoded channel (`adjust_calibrated`): an
    sRGB-shaped profile's channel at its effective pure gamma 2.2, the gain,
    the clamp, and back through the same power."""
    return min(max(encoded ** EXPOSURE_GAMMA * gain, 0.0), 1.0) ** (1.0 / EXPOSURE_GAMMA)


def exposure_byte_transcode(linear, gain):
    """The BYTE adjustment transcode (`doc_evaluation`): the backdrop is
    re-encoded to a LATTICE code for the kernel, the kernel's answer is
    rounded to the 8-bit grid (the byte path's rule) and decoded straight
    back."""
    encoded = encode_code(linear) / 255.0
    kernel = round(exposure_kernel(encoded, gain) * 255.0) / 255.0
    return srgb_decode(kernel)


def exposure_typed_transcode(linear, gain):
    """The TYPED transcode is continuous: the kernel's answer is decoded with
    no rounding between."""
    return srgb_decode(exposure_kernel(srgb_encode(linear), gain))


# ----------------------------------------------------------- PNG plumbing --

def chunk(kind, body):
    return (struct.pack('>I', len(body)) + kind + body
            + struct.pack('>I', zlib.crc32(kind + body)))


def png8(path, width, height, pixels):
    rows = b''.join(b'\0' + bytes(pixels[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    Path(path).write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
        + chunk(b'sRGB', b'\0')
        + chunk(b'IDAT', zlib.compress(rows, 6))
        + chunk(b'IEND', b''))


def png16(path, width, height, samples):
    rows = b''.join(b'\0' + struct.pack(f'>{width * 4}H',
                                        *samples[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    Path(path).write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
        + chunk(b'sRGB', b'\0')
        + chunk(b'IDAT', zlib.compress(rows, 6))
        + chunk(b'IEND', b''))


def read_png(data):
    """Any 8- or 16-bit RGB/RGBA PNG, unfiltered here rather than by a library."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    offset, packed = 8, b''
    width = height = depth = color = 0
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        offset += size + 12
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', payload)
            assert interlace == 0 and depth in (8, 16), (depth, interlace)
        elif kind == b'IDAT':
            packed += payload
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[color]
    unit = channels * (depth // 8)
    stride = width * unit
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    out = []
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - unit] if i >= unit else 0
            up = previous[i]
            corner = previous[i - unit] if i >= unit else 0
            if kind == 0:
                predictor = 0
            elif kind == 1:
                predictor = left
            elif kind == 2:
                predictor = up
            elif kind == 3:
                predictor = (left + up) // 2
            else:
                assert kind == 4, kind
                p = left + up - corner
                distances = [abs(p - left), abs(p - up), abs(p - corner)]
                predictor = [left, up, corner][distances.index(min(distances))]
            row[i] = (row[i] + predictor) & 255
        if depth == 8:
            values = list(row)
        else:
            values = list(struct.unpack(f'>{width * channels}H', bytes(row)))
        for x in range(width):
            pixel = values[x * channels:(x + 1) * channels]
            full = 255 if depth == 8 else 65535
            if channels == 1:
                out.append((pixel[0], pixel[0], pixel[0], full))
            elif channels == 2:
                out.append((pixel[0], pixel[0], pixel[0], pixel[1]))
            elif channels == 3:
                out.append((pixel[0], pixel[1], pixel[2], full))
            else:
                out.append(tuple(pixel))
        previous = row
    return width, height, out


# ------------------------------------------------------------- fixtures --

def base_pixels8():
    """A deterministic, structured, opaque 8-bit plate."""
    pixels = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            pixels.extend((
                12 + (x * 7 + y * 3) % 230,
                40 + (x * 11 + y * 5) % 200,
                30 + ((x * x + y * y) % 210),
                255))
    return pixels


def base_samples16():
    samples = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            samples.extend((
                (1200 + (x * 1777 + y * 733) % 62000),
                (4000 + (x * 991 + y * 2311) % 58000),
                (2500 + ((x * x * 53 + y * y * 97) % 60000)),
                65535))
    return samples


def in_mask(x, y):
    mx, my, mw, mh = MASK_RECT
    return mx <= x < mx + mw and my <= y < my + mh


def oracle(base, maximum, transcode, exposure_last):
    """The linear-light projection of the E1/E2 stack, sample by sample.

    `base` is the flat RGBA sample list at `maximum` (255 or 65535); the two
    upper layers are DUPLICATES of the base, so their own pixels are the same
    numbers. `transcode` is the depth's adjustment rule. `exposure_last`
    follows the stack the typed path actually builds: 16-bit layers append at
    the TOP, so the adjustment sits above the pair there and directly above
    the opaque base at 8 bits.
    """
    gain = 2.0 ** EXPOSURE_STOPS
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            i = (y * WIDTH + x) * 4
            codes = [base[i + c] / maximum for c in range(4)]
            acc = [srgb_decode(codes[0]), srgb_decode(codes[1]), srgb_decode(codes[2]),
                   codes[3]]
            source = [srgb_decode(codes[0]), srgb_decode(codes[1]), srgb_decode(codes[2])]
            adjust = lambda value: [transcode(value[0], gain), transcode(value[1], gain),
                                    transcode(value[2], gain), value[3]]
            if not exposure_last:
                acc = adjust(acc)
            # The Multiply layer at 50 %, which composites at its opacity
            # BYTE, 128 / 255 (`opacity_byte`).
            acc = over(acc, source, codes[3] * OPACITY_HALF, 'multiply')
            # The masked Screen layer at 100 %.
            coverage = 1.0 if in_mask(x, y) else 0.0
            acc = over(acc, source, codes[3] * coverage, 'screen')
            if exposure_last:
                acc = adjust(acc)
            if maximum == 255:
                out.extend((encode_code(acc[0]), encode_code(acc[1]), encode_code(acc[2]),
                            int(round(acc[3] * 255.0))))
            else:
                out.extend((int(round(srgb_encode(acc[0]) * 65535.0)),
                            int(round(srgb_encode(acc[1]) * 65535.0)),
                            int(round(srgb_encode(acc[2]) * 65535.0)),
                            int(round(acc[3] * 65535.0))))
    return out


def measure(actual, expected, label):
    assert len(actual) == len(expected), (label, len(actual), len(expected))
    total = 0
    worst = 0
    for a, b in zip(actual, expected):
        delta = abs(int(a) - int(b))
        total += delta
        worst = max(worst, delta)
    return {'label': label, 'samples': len(actual),
            'mean_abs': total / max(1, len(actual)), 'max_abs': worst}


# ----------------------------------------------------------- the drive --

def flat(pixels):
    return [component for pixel in pixels for component in pixel]


def rendered(app, document):
    reply = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document, 'max_side': 4096}})
    assert not reply.get('isError'), reply
    data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
    raw = base64.b64decode(data)
    width, height, pixels = read_png(raw)
    assert (width, height) == (WIDTH, HEIGHT), (width, height)
    return flat(pixels), hashlib.sha256(raw).hexdigest()


def build_stack(app, document, sixteen):
    """The shared E1/E2 fixture: an opaque base, an exposure `document_v2`
    adjustment, a Multiply layer at 50 % and a masked Screen layer, with a
    group around them. The two upper rasters are DUPLICATES of the base, so
    the oracle knows their pixels exactly without reading them back.

    The 16-bit branch differs in two ways the typed model imposes, both
    recorded in the receipt: new typed layers append at the TOP (so the
    adjustment sits above the pair rather than directly over the base), and
    typed grouping takes every top-level layer at once.
    """
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    if sixteen:
        call('add_adjustment_layer', op='exposure',
             params={'exposure': EXPOSURE_STOPS, 'evaluation': 'document_v2'})
        call('duplicate_layer', index=0)
        call('duplicate_layer', index=0)
        call('set_layer_properties', index=1, name='Mul', opacity=0.5, blend_mode='multiply')
        call('set_layer_properties', index=2, name='Scr', blend_mode='screen')
        call('select_rect', x=MASK_RECT[0], y=MASK_RECT[1],
             width=MASK_RECT[2], height=MASK_RECT[3])
        call('add_layer_mask', layer=2, kind='from_selection')
        call('deselect')
        call('group_layers', layers=[0, 1, 2, 3], name='All')
    else:
        call('duplicate_layer', index=0)
        call('duplicate_layer', index=1)
        call('set_layer_properties', index=1, name='Mul', opacity=0.5, blend_mode='multiply')
        call('set_layer_properties', index=2, name='Scr', blend_mode='screen')
        call('select_rect', x=MASK_RECT[0], y=MASK_RECT[1],
             width=MASK_RECT[2], height=MASK_RECT[3])
        call('add_layer_mask', layer=2, kind='from_selection')
        call('deselect')
        call('group_layers', layers=[1, 2], name='Pair')
        call('set_active_layer', index=0)
        call('add_adjustment_layer', op='exposure',
             params={'exposure': EXPOSURE_STOPS, 'evaluation': 'document_v2'})
    state = call('get_document')
    names = [layer['name'] for layer in state['layers']]
    assert names[0] == 'Background' and 'Mul' in names and 'Scr' in names, names
    assert any(layer['kind'].endswith('group') for layer in state['layers']), names
    assert state['evaluation']['policy'] == 'encoded', state['evaluation']
    return state


def switch(app, document, receipts, gate):
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    preview = call('get_evaluation', preview=True)
    assert preview['policy'] == 'encoded', preview
    assert preview['linear_light_refusal'] is None, preview
    assert 'multiply' in preview['operators']['migrated'], preview['operators']
    assert preview['difference_policy'] == 'linear_light', preview
    receipts[f'{gate}_preview'] = preview['difference']
    reply = call('set_evaluation', policy='linear_light')
    assert reply['policy'] == 'linear_light' and reply['previous'] == 'encoded', reply
    assert call('get_document')['evaluation']['policy'] == 'linear_light'
    return preview


def isolated_group_identity(app, folder, receipts, sixteen):
    """E1/E2's ISOLATED-GROUP case, on its own copy of the gate's fixture.

    A Pass Through group allocates nothing and never enters
    `doc_group::rendered_group`, so the stack the gates measure never crosses
    the ONE boundary that quantizes a private accumulator. Here the same stack
    gains an isolated group — a group whose blend mode is Normal rather than
    Pass Through — and the identity asserted is the compositor's own: an
    isolated group of one opaque Normal layer at full opacity IS that layer.
    Under `linear_light` the boundary's accumulator holds LINEAR components,
    so it must re-encode through the document's transfer; quantizing it as
    codes would hand the enclosing level linear numbers and render the group
    far too dark, which is exactly what this identity refuses.

    At 8 bits the base is grouped on its own. At 16 bits typed grouping takes
    every top-level layer at once, so the fixture's own `All` group is the one
    isolated — over a transparent canvas at Normal and full opacity that is
    the same picture too.
    """
    import shutil
    tag = 'e2' if sixteen else 'e1'
    # A COPY under its own name: `open_document` returns the already-open
    # document for a path it is already showing, and the gate's own copy has
    # been switched already.
    source = folder / f'{tag}-isolated.png'
    shutil.copyfile(folder / f'{tag}.png', source)
    document = app.open(source)
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    build_stack(app, document, sixteen=sixteen)
    call('set_evaluation', policy='linear_light')
    before, before_sha = rendered(app, document)
    if sixteen:
        state = call('get_document')
        index = next(i for i, layer in enumerate(state['layers'])
                     if layer['name'] == 'All')
    else:
        index = call('group_layers', layers=[0], name='Base')['group']
    call('set_layer_properties', index=index, blend_mode='normal')
    state = call('get_document')
    # The byte path reports a display name, the typed one the DrBlendMode
    # integer; Normal is 0.
    reported = state['layers'][index]['blend_mode']
    if isinstance(reported, str):
        reported = reported.strip().lower()
    assert reported in ('normal', 0), state['layers'][index]
    after, after_sha = rendered(app, document)
    assert after == before, ('an isolated group boundary must not move the picture',
                             tag, max(abs(a - b) for a, b in zip(after, before)))
    receipts[f'{tag}_isolated_group'] = {'group': index, 'sha256': after_sha,
                                         'identical': after_sha == before_sha}
    print(f'PASS {tag.upper()}: the isolated group boundary re-encodes without moving '
          'the picture', flush=True)


def gate_e1(app, folder, receipts):
    source = folder / 'e1.png'
    base = base_pixels8()
    png8(source, WIDTH, HEIGHT, base)
    document = app.open(source)
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    build_stack(app, document, sixteen=False)
    encoded_export = folder / 'e1-encoded.png'
    call('export_image', path=str(encoded_export),
         settings={'format': 'png', 'embed_profile': True})
    before_sha = hashlib.sha256(encoded_export.read_bytes()).hexdigest()
    switch(app, document, receipts, 'e1')
    actual, render_sha = rendered(app, document)
    expected = oracle(base, 255, exposure_byte_transcode, exposure_last=False)
    stats = measure(actual, expected, 'E1 8-bit render vs oracle')
    receipts['e1'] = stats
    assert stats['mean_abs'] <= T1_MEAN_CODES and stats['max_abs'] <= T1_MAX_CODES, stats

    # The same projection through the file writer, not only the preview path.
    exported = folder / 'e1-linear.png'
    call('export_image', path=str(exported),
         settings={'format': 'png', 'embed_profile': True})
    _, _, pixels = read_png(exported.read_bytes())
    file_stats = measure(flat(pixels), expected, 'E1 8-bit export vs oracle')
    receipts['e1_export'] = file_stats
    assert file_stats['mean_abs'] <= T1_MEAN_CODES and file_stats['max_abs'] <= T1_MAX_CODES, file_stats
    print(f"PASS E1: 8-bit linear-light projection within {stats['mean_abs']:.4f} mean / "
          f"{stats['max_abs']} max codes", flush=True)
    isolated_group_identity(app, folder, receipts, sixteen=False)
    return document, before_sha, render_sha, actual


def gate_e2(app, folder, receipts):
    source = folder / 'e2.png'
    base = base_samples16()
    png16(source, WIDTH, HEIGHT, base)
    document = app.open(source)
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    assert call('get_document')['precision'] == 'rgba16', 'the 16-bit fixture must open typed'
    build_stack(app, document, sixteen=True)
    switch(app, document, receipts, 'e2')
    exported = folder / 'e2-linear.png'
    call('export_image', path=str(exported),
         settings={'format': 'png', 'bit_depth': 16, 'embed_profile': True})
    width, height, pixels = read_png(exported.read_bytes())
    assert (width, height) == (WIDTH, HEIGHT), (width, height)
    expected = oracle(base, 65535, exposure_typed_transcode, exposure_last=True)
    stats = measure(flat(pixels), expected, 'E2 16-bit export vs oracle')
    receipts['e2'] = stats
    assert stats['mean_abs'] <= T2_MEAN_CODES and stats['max_abs'] <= T2_MAX_CODES, stats
    print(f"PASS E2: 16-bit linear-light projection within {stats['mean_abs']:.4f} mean / "
          f"{stats['max_abs']} max codes of 65535", flush=True)
    isolated_group_identity(app, folder, receipts, sixteen=True)
    return document


def gate_e3(app, folder, document, samples, receipts):
    native = folder / 'e3.rz'
    app.call('save_copy', document_id=document, path=str(native), format='rz')
    _, chain = wrapped_version(native.read_bytes())
    words = [w for w, _ in chain]
    assert EVALUATION_BYTE_WORD in words, \
        f'a linear-light byte .rz carries the evaluation word {EVALUATION_BYTE_WORD}, not in {words}'
    reopened = app.open(native)
    state = app.call('get_document', document_id=reopened)
    assert state['evaluation']['policy'] == 'linear_light', state['evaluation']
    again, _ = rendered(app, reopened)
    assert again == samples, 'the reopened projection must be sample-identical'
    receipts['e3'] = {'wrapper_chain': words, 'policy': state['evaluation']['policy'],
                      'samples_identical': True}
    print(f'PASS E3: the wrapper chain carries evaluation word {EVALUATION_BYTE_WORD}; '
          'the policy and every sample survive a .rz round trip', flush=True)
    return reopened


def gate_e4(app, folder, document, samples, receipts):
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    call('create_snapshot', name='linear')
    call('set_evaluation', policy='encoded')
    assert call('get_document')['evaluation']['policy'] == 'encoded'
    snapshots = call('list_snapshots')['snapshots']
    entry = next(s for s in snapshots if s['name'] == 'linear')
    call('restore_snapshot', snapshot_id=entry['snapshot_id'])
    state = call('get_document')
    assert state['evaluation']['policy'] == 'linear_light', state['evaluation']
    restored, _ = rendered(app, document)
    assert restored == samples, 'a restored snapshot must be sample-identical'
    receipts['e4_snapshot'] = {'policy': 'linear_light', 'samples_identical': True}
    print('PASS E4a: snapshots keep the policy and the projection', flush=True)


def gate_e4_recovery(folder, receipts, executable):
    """A forced crash and a relaunched recovery, on their own owned instance."""
    recovery = folder / 'recovery'
    source = folder / 'e4.png'
    base = base_pixels8()
    png8(source, WIDTH, HEIGHT, base)
    native = folder / 'e4.rz'
    previous = phase5_support.APP
    phase5_support.APP = executable
    try:
        with App(folder / 'app-a', recovery, interval=600) as app:
            seed = app.open(source)
            app.call('save_copy', document_id=seed, path=str(native), format='rz')
            document = app.open(native)
            app.call('set_evaluation', document_id=document, policy='linear_light')
            expected, _ = rendered(app, document)
            app.call('write_recovery', document_id=document, wait=True)
            app.crash()
        with App(folder / 'app-b', recovery, interval=600) as app:
            available = app.call('get_recovery_status')['available']
            assert available, available
            reply = app.call('recover_document', recovery_id=available[0]['recovery_id'], show=True)
            recovered = reply['document']['id']
            state = app.call('get_document', document_id=recovered)
            assert state['evaluation']['policy'] == 'linear_light', state['evaluation']
            actual, _ = rendered(app, recovered)
            assert actual == expected, 'recovery must restore the same projection'
    finally:
        phase5_support.APP = previous
    receipts['e4_recovery'] = {'policy': 'linear_light', 'samples_identical': True}
    print('PASS E4b: a crash, a relaunch and recover_document keep the policy', flush=True)


def gate_e5(app, folder, document, before_sha, receipts):
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    call('undo')
    state = call('get_document')
    assert state['evaluation']['policy'] == 'encoded', state['evaluation']
    after = folder / 'e5-encoded.png'
    call('export_image', path=str(after),
         settings={'format': 'png', 'embed_profile': True})
    sha = hashlib.sha256(after.read_bytes()).hexdigest()
    assert sha == before_sha, ('undo must reproduce the pre-switch export', sha, before_sha)
    receipts['e5'] = {'policy': 'encoded', 'sha256': sha}
    print('PASS E5: undo returns the policy and the exact pre-switch export', flush=True)
    call('redo')
    assert call('get_document')['evaluation']['policy'] == 'linear_light'


def gate_e6(app, folder, receipts):
    source = folder / 'e6.png'
    base = base_pixels8()
    png8(source, WIDTH, HEIGHT, base)
    document = app.open(source)
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    build_stack(app, document, sixteen=False)
    first = folder / 'e6-a.png'
    call('export_image', path=str(first),
         settings={'format': 'png', 'embed_profile': True})
    native = folder / 'e6.rz'
    call('save_copy', path=str(native), format='rz')
    _, chain = wrapped_version(native.read_bytes())
    words = [w for w, _ in chain]
    assert EVALUATION_BYTE_WORD not in words, \
        f'an encoded document must not carry the evaluation wrapper: {words}'
    # Switch away and back: an encoded document must be exactly what it was.
    call('set_evaluation', policy='linear_light')
    call('set_evaluation', policy='encoded')
    second = folder / 'e6-b.png'
    call('export_image', path=str(second),
         settings={'format': 'png', 'embed_profile': True})
    assert first.read_bytes() == second.read_bytes(), 'encoded exports must be byte-identical'
    receipts['e6'] = {'wrapper_chain': words,
                      'sha256': hashlib.sha256(first.read_bytes()).hexdigest()}
    print(f'PASS E6: an encoded .rz carries no evaluation word (chain {words}) '
          'and exports stay byte-identical', flush=True)


def render_sha(app, document):
    """The sha256 of `render`'s PNG at any canvas size."""
    reply = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document, 'max_side': 4096}})
    assert not reply.get('isError'), reply
    data = next(item['data'] for item in reply['content'] if item['type'] == 'image')
    return hashlib.sha256(base64.b64decode(data)).hexdigest()


def gate_e7(app, folder, document, render_sha_before, receipts):
    call = lambda tool, **args: app.call(tool, document_id=document, **args)
    notes = {}

    # A document that already carries a non-separable mode switches.
    source = folder / 'e7-hue.png'
    png8(source, WIDTH, HEIGHT, base_pixels8())
    other = app.open(source)
    app.call('duplicate_layer', document_id=other, index=0)
    app.call('set_layer_properties', document_id=other, index=1, name='Hue layer',
             blend_mode='hue')
    app.call('set_evaluation', document_id=other, policy='linear_light')
    assert app.call('get_document', document_id=other)['evaluation']['policy'] == 'linear_light'
    notes['switch_hue'] = 'admitted'

    # On the SWITCHED document every operator phase 1 refused lands: the
    # projection changes, nothing is refused, and one undo restores it.
    state = call('get_document')
    raster = next(layer['index'] for layer in state['layers']
                  if layer.get('name') == 'Mul')
    steps = [
        ('blend_mode', 'set_layer_properties', {'index': raster, 'blend_mode': 'hue'}),
        ('layer_style', 'set_layer_style',
         {'index': raster, 'style': {'effects': [{'type': 'color_overlay', 'opacity': 0.5}]}}),
        ('blend_if', 'set_layer_style',
         {'index': raster, 'style': {'blend_if': {'channel': 'gray',
                                                  'this_layer': [0, 0, 120, 140],
                                                  'underlying': [0, 0, 255, 255]}}}),
        ('guided_shadows_highlights', 'add_adjustment_layer',
         {'op': 'shadows_highlights', 'params': {'shadows': {'amount': 0.5}, 'radius': 8.0},
          'name': 'SH'}),
        ('image_size', 'image_size', {'width': WIDTH * 2, 'height': HEIGHT * 2}),
    ]
    before = render_sha(app, document)
    assert before == render_sha_before, 'the switched projection is the one E1 measured'
    for key, tool, args in steps:
        call(tool, **args)
        evaluation = call('get_evaluation')
        assert evaluation['policy'] == 'linear_light', (key, evaluation)
        assert evaluation['linear_light_refusal'] is None, (key, evaluation)
        assert evaluation['operators']['refused'] == [], (key, evaluation)
        assert 'resize_refusal' not in evaluation['operators'], (key, evaluation)
        after = render_sha(app, document)
        assert after != before, f'{key} must change the projection'
        call('undo')
        assert render_sha(app, document) == before, f'one undo restores the projection ({key})'
        notes[key] = 'admitted'

    # A placed image is admitted under linear light.
    source = folder / 'e7-placed.png'
    png8(source, WIDTH, HEIGHT, base_pixels8())
    placed = app.open(source)
    app.call('set_evaluation', document_id=placed, policy='linear_light')
    app.call('place_image', document_id=placed, path=str(source))
    state = app.call('get_document', document_id=placed)
    assert state['evaluation']['policy'] == 'linear_light', state['evaluation']
    assert state['evaluation']['projection_error'] is None, state['evaluation']
    after = call('get_document')
    assert after['evaluation']['policy'] == 'linear_light', after['evaluation']
    assert after['evaluation']['projection_error'] is None, after['evaluation']
    receipts['e7'] = notes
    print('PASS E7: every operator is admitted under linear light, and each step undoes',
          flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', required=True)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    executable = Path(args.app)
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        import shutil
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    receipts = {'tolerances': {'T1_mean': T1_MEAN_CODES, 'T1_max': T1_MAX_CODES,
                               'T2_mean': T2_MEAN_CODES, 'T2_max': T2_MAX_CODES}}
    previous = phase5_support.APP
    phase5_support.APP = executable
    try:
        with App(folder / 'app', folder / 'idle-recovery', interval=600) as app:
            document, before_sha, render_sha, samples = gate_e1(app, folder, receipts)
            gate_e2(app, folder, receipts)
            gate_e3(app, folder, document, samples, receipts)
            gate_e4(app, folder, document, samples, receipts)
            gate_e5(app, folder, document, before_sha, receipts)
            gate_e6(app, folder, receipts)
            gate_e7(app, folder, document, render_sha, receipts)
    finally:
        phase5_support.APP = previous
    gate_e4_recovery(folder, receipts, executable)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2) + '\n')
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
