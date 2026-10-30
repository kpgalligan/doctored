#!/usr/bin/env python3
"""PNG in and out for Doctored's verification drives, plus a base test picture.

The reader accepts colour types 2 (RGB) and 6 (RGBA) at 8 and 16 bit and gives
an RGB file full alpha; Photoshop-written reference PNGs (colour type 2, no ICC
chunk, transparency matted against white) read through the same path. The
writers emit an `sRGB` chunk so the file says what it is without an ICC
payload; nothing here decodes or applies a profile.

Deliberately dependency-free: `zlib` and `struct` only, no Pillow, no numpy,
and no Doctored code, so the reference side of a drive shares nothing with the
side it measures.
"""
import colorsys
import struct
import zlib

SIGNATURE = b'\x89PNG\r\n\x1a\n'


# --------------------------------------------------------------- reading --

def _chunks(data):
    assert data[:8] == SIGNATURE, 'not a PNG'
    at = 8
    while at + 8 <= len(data):
        length = struct.unpack('>I', data[at:at + 4])[0]
        kind = data[at + 4:at + 8]
        body = data[at + 8:at + 8 + length]
        assert len(body) == length, f'truncated {kind!r} chunk'
        expected = struct.unpack('>I', data[at + 8 + length:at + 12 + length])[0]
        assert zlib.crc32(kind + body) == expected, f'bad CRC on {kind!r}'
        yield kind, body
        at += 12 + length


def _unfilter(raw, width, height, channels, depth):
    """The five PNG filter types, per-byte, over `bpp`-spaced predictors."""
    bpp = channels * (depth // 8)
    stride = width * bpp
    out = bytearray()
    previous = bytearray(stride)
    at = 0
    for _ in range(height):
        kind = raw[at]
        line = bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = previous[i]
            c = previous[i - bpp] if i >= bpp else 0
            if kind == 0:
                value = line[i]
            elif kind == 1:
                value = line[i] + a
            elif kind == 2:
                value = line[i] + b
            elif kind == 3:
                value = line[i] + (a + b) // 2
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                value = line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)
            else:
                raise AssertionError(f'unknown PNG filter {kind}')
            line[i] = value & 0xFF
        out += line
        previous = line
    return bytes(out)


def png_rgba(data):
    """`dict(width, height, depth, samples)` — RGBA in file order, row 0 first.

    `samples` is a list of ints at the file's own depth (0…255 or 0…65535);
    an RGB file is given full alpha. Interlaced and paletted files refuse.
    """
    header, idat, transfer = None, b'', {}
    for kind, body in _chunks(data):
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat += body
        elif kind in (b'sRGB', b'gAMA', b'iCCP'):
            transfer[kind.decode()] = len(body)
    assert header is not None, 'no IHDR'
    width, height, depth, colour, compression, filtering, interlace = header
    assert compression == 0 and filtering == 0, 'unknown PNG compression/filtering'
    assert interlace == 0, 'interlaced PNG'
    assert depth in (8, 16), f'unsupported bit depth {depth}'
    assert colour in (2, 6), f'unsupported colour type {colour}'
    channels = 3 if colour == 2 else 4
    raw = _unfilter(zlib.decompress(idat), width, height, channels, depth)
    count = width * height * channels
    if depth == 8:
        flat = list(raw)
    else:
        flat = list(struct.unpack(f'>{count}H', raw))
    assert len(flat) == count, (len(flat), count)
    full = 255 if depth == 8 else 65535
    if channels == 4:
        samples = flat
    else:
        samples = []
        for i in range(0, count, 3):
            samples += flat[i:i + 3] + [full]
    return dict(width=width, height=height, depth=depth, samples=samples,
                colour_type=colour, chunks=transfer)


# --------------------------------------------------------------- writing --

def _chunk(kind, body):
    return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))


def _write(path, width, height, depth, rows):
    path.write_bytes(
        SIGNATURE
        + _chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
        + _chunk(b'sRGB', b'\0')
        + _chunk(b'IDAT', zlib.compress(rows, 9))
        + _chunk(b'IEND', b''))


def write_png8(path, width, height, rgba):
    """An RGBA8 PNG with an `sRGB` chunk. `rgba` is bytes, row 0 first."""
    assert len(rgba) == width * height * 4
    stride = width * 4
    _write(path, width, height, 8,
           b''.join(b'\0' + bytes(rgba[y * stride:(y + 1) * stride]) for y in range(height)))


def write_png16(path, width, height, samples):
    """An RGBA16 PNG with an `sRGB` chunk. `samples` is ints, row 0 first."""
    assert len(samples) == width * height * 4
    stride = width * 4
    _write(path, width, height, 16,
           b''.join(b'\0' + struct.pack(f'>{stride}H', *samples[y * stride:(y + 1) * stride])
                    for y in range(height)))


# ---------------------------------------------------------- the picture --

def base_picture(width=64, height=64):
    """The 64 x 64 opaque RGBA8 base every parity case is authored over.

    Rows 0…31 are the three-ramp gradient of `verify_phase10_receipts.base8`
    widened to span 0…255, so every kind sees both endpoints (Threshold and
    Levels clip there, and Invert is only interesting across the full range).
    Rows 32…63 are an HSV sweep at full value, hue 0…360 across and saturation
    1 → 0.2 down, because the six hue bands of Hue/Saturation, Black & White
    and Vibrance are indexed by hue and a grey ramp exercises none of them.
    """
    assert width >= 2 and height == 64
    out = bytearray()
    band = height // 2
    for y in range(band):
        for x in range(width):
            out += bytes((255 * x // (width - 1), 255 * y // (band - 1),
                          255 * (x + y) // (width + band - 2), 255))
    for y in range(band):
        for x in range(width):
            hue = x / width
            saturation = 1.0 - 0.8 * y / (band - 1)
            r, g, b = colorsys.hsv_to_rgb(hue, saturation, 1.0)
            out += bytes((round(r * 255), round(g * 255), round(b * 255), 255))
    return bytes(out)
