"""Independently authored native masks; no product encoder supplies expectations."""
import math
from pathlib import Path
import struct
import zlib

CANVAS = (4, 3)
PROFILE = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc')


def pack(code, *values):
    return struct.pack('<' + code, *values)


def slot(data, code='I'):
    return pack(code, len(data)) + data


def png(width, height, depth, samples):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    code = 'B' if depth == 8 else 'H'
    rows = b''.join(b'\0' + struct.pack('>' + code * (width * 4),
        *samples[y * width * 4:(y + 1) * width * 4]) for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def source(depth):
    byte = [(17, 63, 211, 255), (233, 91, 31, 127), (101, 151, 201, 0), (27, 67, 107, 255),
            (39, 79, 119, 255), (49, 89, 129, 255), (59, 99, 139, 127), (69, 109, 149, 255),
            (79, 119, 159, 255), (89, 129, 169, 255), (99, 139, 179, 255), (109, 149, 189, 255)]
    if depth == 8:
        return [c for pixel in byte for c in pixel]
    # Include genuine non-byte native codes, not a byte-promoted fixture.
    return [c * 257 + (component + 1 if component < 3 else 0)
            for pixel in byte for component, c in enumerate(pixel)]


def description(depth, *, group=False, outside=255, offset=(1, 1), enabled=True):
    return dict(canvas=list(CANVAS), depth=depth, rgba=source(depth), group=group,
                mask=dict(width=2, height=2, x=offset[0], y=offset[1], enabled=enabled,
                          linked=False, density=1.0, feather=0.0, outside_coverage=outside,
                          samples=[0, 63, 127, 255] if depth == 8 else [0, 123, 32769, 65535]))


def mask_bytes(mask, depth):
    return struct.pack('<' + ('B' if depth == 8 else 'H') * len(mask['samples']), *mask['samples'])


def byte_row(name, pixels, *, group=False, tree_depth=0, mask=None):
    result = slot(name.encode()) + pack('iifI', 0, 0, 1.0, 0) + b'\1' + slot(pixels)
    result += bytes((mask is not None, mask['enabled'] if mask else True))
    if mask:
        raw = mask_bytes(mask, 8)
        result += pack('III', len(raw), mask['width'], mask['height']) + raw
    result += b'\0\0\0' + pack('IIHBB', 0, 0, tree_depth, group, True)
    if mask:
        result += pack('iiBff', mask['x'], mask['y'], mask['linked'], mask['density'], mask['feather'])
    return result


def typed_row(name, *, mask=None, child=None, pixels=None):
    result = pack('BBHf', 1 if child is not None else 0, 1, 0, 1.0) + slot(name.encode()) + pack('ii', 0, 0)
    result += bytes((mask is not None,))
    if mask:
        raw = mask_bytes(mask, 16)
        result += pack('BBHiiIIffQ', mask['enabled'], mask['linked'], 0, mask['x'], mask['y'],
                       mask['width'], mask['height'], mask['density'], mask['feather'], len(raw)) + raw
    return result + (pack('I', 1) + child if child is not None else pack('II', *CANVAS) + slot(pixels, 'Q'))


def native(expected, profile):
    depth, group, mask = expected['depth'], expected['group'], expected['mask']
    encoded = png(*CANVAS, depth, expected['rgba'])
    if depth == 8:
        body = pack('IIIff', *CANVAS, 2 if group else 1, 120.0, 30.0)
        body += byte_row('Source', encoded, tree_depth=1 if group else 0, mask=None if group else mask)
        if group:
            body += byte_row('Mask group', png(1, 1, 8, [0, 0, 0, 0]), group=True, mask=mask)
        # No channels, authored resolution/profile, no XMP/IPTC/EXIF, no guides.
        body += pack('Iff', 0, 72.0, 72.0) + b'\1' + slot(profile) + b'\0\0\0' + pack('ddI', 0, 0, 0)
        inner, outer = 10, 28
    else:
        body = pack('IIIIII', 1, 1, *CANVAS, 1, 2 if group else 1) + slot(profile)
        body += pack('ff', 72.0, 72.0) + b'\0\0\0'
        child = typed_row('Source', pixels=encoded, mask=None if group else mask)
        body += typed_row('Mask group', mask=mask, child=child) if group else child
        body += pack('I', 0)
        inner, outer = 13, 29
    if mask['outside_coverage'] == 0:
        return b'RZDC' + pack('I', inner) + body
    return b'RZDC' + pack('II', outer, inner) + body + b'MOUT' + pack('III', 1, 1, 1 if group else 0)


def composite(expected, *, density=None, enabled=None, translate=0):
    """One straight source with a mask: independently compute final coverage."""
    mask, output = expected['mask'], []
    maximum = (1 << expected['depth']) - 1
    density = mask['density'] if density is None else density
    enabled = mask['enabled'] if enabled is None else enabled
    for y in range(CANVAS[1]):
        for x in range(CANVAS[0]):
            mx, my = x - mask['x'] - translate, y - mask['y']
            sample = mask['samples'][my * mask['width'] + mx] if 0 <= mx < mask['width'] and 0 <= my < mask['height'] else maximum * mask['outside_coverage'] // 255
            coverage = 1 - density + density * sample / maximum if enabled else 1
            pixel = expected['rgba'][(y * CANVAS[0] + x) * 4:][:4]
            alpha = math.floor(pixel[3] * coverage + .5)
            output.extend(pixel[:3] if alpha else [0, 0, 0])
            output.append(alpha)
    return output


def write(folder):
    folder.mkdir(exist_ok=False)
    profile = PROFILE.read_bytes()
    (folder / 'profile.icc').write_bytes(profile)
    cases = {}
    for depth in (8, 16):
        variants = [('outside-white', {}), ('outside-black', dict(outside=0)),
                    ('offcanvas', dict(offset=(-3, 1))), ('disabled', dict(enabled=False)),
                    ('group', dict(group=True))]
        for label, options in variants:
            expected = description(depth, **options)
            name = f'{depth}-{label}'
            path = folder / (name + '.rz')
            path.write_bytes(native(expected, profile))
            cases[name] = (path, expected)
    return cases
