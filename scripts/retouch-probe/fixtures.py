#!/usr/bin/env python3
"""Self-contained fixtures for the Phase 9D retouch-sampling probe.

The geometry has to express things the 9B plate cannot: a blemish with clean
surroundings for the heal's contour to sit on, a ramp so a membrane solve has
real illumination to match, and a canvas big enough that a stroke and its
source can be far apart.
"""
import argparse
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
WIDTH, HEIGHT = 64, 48
# The blemish and the clean patch the retouch samples FROM, in canvas px.
BLEMISH = (26, 18, 34, 26)
SOURCE = (10, 18, 18, 26)


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(width, height, depth, pixels, profile):
    pack = (lambda p: struct.pack('>4H', *p)) if depth == 16 else bytes
    rows = [b'\0' + b''.join(pack(pixel) for pixel in pixels[y * width:(y + 1) * width])
            for y in range(height)]
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
            + chunk(b'iCCP', b'Fixture RGB\0\0' + zlib.compress(profile))
            + chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))


def photograph(depth, width=WIDTH, height=HEIGHT):
    """A gentle two-axis ramp with a dark square blemish, fully opaque.

    Opaque on purpose: every sampling mode's plate is then comparable pixel
    for pixel without an alpha term, and the destructive-alpha rules are the
    Rust tests' business, not this probe's.
    """
    maximum = (1 << depth) - 1
    scale = maximum // 255
    result = []
    x0, y0, x1, y1 = BLEMISH
    for y in range(height):
        for x in range(width):
            if x0 <= x < x1 and y0 <= y < y1:
                value = 20 * scale
            else:
                value = (110 + x + y) * scale
            result.append((value, value, value, maximum))
    return result


def grade(depth, width=WIDTH, height=HEIGHT):
    """A transparent canvas-sized plate: what an adjustment layer's own pixels
    are, since the compositor ignores them and reads its meta instead."""
    return [(0, 0, 0, 0)] * (width * height)


def srgb_profile():
    return Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc').read_bytes()


def build(folder):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    profile = srgb_profile()
    (folder / 'srgb.icc').write_bytes(profile)
    written = []
    for depth in (8, 16):
        path = folder / f'photo-{depth}.png'
        path.write_bytes(png(WIDTH, HEIGHT, depth, photograph(depth), profile))
        written.append(path)
        empty = folder / f'empty-{depth}.png'
        empty.write_bytes(png(WIDTH, HEIGHT, depth, grade(depth), profile))
        written.append(empty)
    return written


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('/tmp/rz-phase9d-fixtures'))
    arguments = parser.parse_args()
    for path in build(arguments.output):
        print(path)


if __name__ == '__main__':
    main()
