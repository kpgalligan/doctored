"""Phase 8A source fixtures and independent source/ICC/resampling expectations."""

import json
import struct
import zlib

from fixtures import chunk, icc_reference


WIDTH, HEIGHT = 96, 64


def source_pixels(depth):
    maximum = (1 << depth) - 1
    alpha = ([0, 1, 2, 17, 128, 254, 255, 255] if depth == 8 else
             [0, 1, 2, 255, 256, 257, 32767, 65535])
    return [((17011 + i * 1031 + (i // WIDTH) * 419) & maximum,
             (31337 + i * 619) & maximum, (7919 + i * 3571) & maximum,
             alpha[i % len(alpha)]) for i in range(WIDTH * HEIGHT)]


def encode_png(width, height, depth, values, profile):
    pack = (lambda pixel: bytes(pixel)) if depth == 8 else (lambda pixel: struct.pack('>4H', *pixel))
    rows = [b'\0' + b''.join(pack(pixel) for pixel in values[y * width:(y + 1) * width])
            for y in range(height)]
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
            + chunk(b'iCCP', b'Display P3\0\0' + zlib.compress(profile))
            + chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))


def bilinear_eighth(values, depth):
    """Pixel-center affine sampling in straight storage, using f64 premultiplied math.

    An exact 1/8 scale samples source positions 3.5, 11.5, etc. Each output
    therefore combines four equally weighted source samples. Transparent RGB
    contributes no light; zero output alpha has zero RGB.
    """
    output = []
    for y in range(HEIGHT // 8):
        for x in range(WIDTH // 8):
            left, top = 8 * x + 3, 8 * y + 3
            neighbors = [values[(top + dy) * WIDTH + left + dx]
                         for dy in (0, 1) for dx in (0, 1)]
            alpha = sum(pixel[3] for pixel in neighbors) / 4
            rgb = [sum(pixel[c] * pixel[3] for pixel in neighbors) / (4 * alpha)
                   if alpha else 0 for c in range(3)]
            output.append([round(v) for v in rgb] + [round(alpha)])
    return output


def create_retained(directory, profile, destination_profile):
    reference = icc_reference(profile, destination_profile)
    for depth in (8, 16):
        values = source_pixels(depth)
        maximum = (1 << depth) - 1
        converted = [[round(channel * maximum) for channel in reference(
            [component * (65535 // maximum) for component in pixel[:3]])] + [pixel[3]]
            for pixel in values]
        (directory / f'retained-source{depth}.png').write_bytes(
            encode_png(WIDTH, HEIGHT, depth, values, profile))
        (directory / f'retained-reference{depth}.json').write_text(json.dumps({
            'width': WIDTH, 'height': HEIGHT, 'depth': depth,
            'source': values, 'converted': converted, 'reduced': bilinear_eighth(values, depth),
            'source_error_codes': 0, 'converted_error_codes': 2 if depth == 16 else 1,
            'reduced_error_codes': 2 if depth == 16 else 1,
            'policy': 'Immutable Display P3 source; exact identity restore; f64 premultiplied '
                      'bilinear at 1/8; independent ICC matrix/TRC P3-to-sRGB.'}, separators=(',', ':')))


def verify_retained(directory):
    """Check stored layer samples emitted by the real host, without app decoding."""
    for depth in (8, 16):
        reference = json.loads((directory / f'retained-reference{depth}.json').read_text())
        for stage, key, tolerance in [('restored', 'source', 0),
                                      ('native-restored', 'source', 0),
                                      ('converted', 'converted', reference['converted_error_codes'])]:
            data = (directory / f'retained-{depth}-{stage}.bin').read_bytes()
            actual = list(struct.iter_unpack('<4H' if depth == 16 else '4B', data))
            expected = reference[key]
            assert len(actual) == len(expected) == WIDTH * HEIGHT
            for index, (got, want) in enumerate(zip(actual, expected)):
                assert got[3] == want[3], (depth, stage, index, 'alpha', got, want)
                assert max(abs(a - b) for a, b in zip(got[:3], want[:3])) <= tolerance, (
                    depth, stage, index, got, want)
    print('PASS: independent retained byte/u16 exact restore, relocated native and original-source ICC oracle')
