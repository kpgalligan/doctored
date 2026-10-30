"""Independent, strict decoder for the small 10A qualification fixtures only.

This test oracle does not call Doctored, its Rust PSD dependency, or GIMP.
Framing follows Adobe's public PSD specification; PackBits is decoded here
independently so the exact stored-plane gate does not use importer output.
https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
It is deliberately not a general PSD reader or an application import path.
"""
import struct
import sys
from array import array
from fractions import Fraction


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def take(self, count):
        assert 0 <= count <= len(self.data) - self.pos
        result = self.data[self.pos:self.pos + count]
        self.pos += count
        return result

    def number(self, code='I'):
        return struct.unpack('>' + code, self.take(struct.calcsize(code)))[0]

    def slot(self):
        return self.take(self.number())

    def rest(self):
        return self.take(len(self.data) - self.pos)


def packbits(data, width):
    reader, output = Reader(data), bytearray()
    while reader.pos < len(data):
        control = reader.number('b')
        if control >= 0:
            output.extend(reader.take(control + 1))
        elif control != -128:
            output.extend(reader.take(1) * (1 - control))
        assert len(output) <= width
    assert len(output) == width
    return bytes(output)


def stored_samples(plane, depth):
    """One stored plane as scalars: bytes at depth 8, native-endian u16 at 16.

    PSD stores 16-bit samples big-endian; this decodes them without using any
    application reader.
    """
    if depth == 8:
        return plane
    assert len(plane) % 2 == 0
    values = array('H', plane)
    if sys.byteorder == 'little':
        values.byteswap()
    return values


def planes(data, width, height, count=1, depth=8):
    row = width * depth // 8
    r = Reader(data)
    compression = r.number('H')
    assert compression in (0, 1), compression
    if compression == 0:
        result = [r.take(row * height) for _ in range(count)]
    else:
        lengths = [r.number('H') for _ in range(height * count)]
        rows = [packbits(r.take(size), row) for size in lengths]
        result = [b''.join(rows[i * height:(i + 1) * height]) for i in range(count)]
    assert not r.rest()
    return [stored_samples(plane, depth) for plane in result]


def rgba(channels, width, height, color, depth=8):
    n = width * height
    full = 255 if depth == 8 else 65535
    colors = [channels[i] for i in (0, 1, 2)] if color == 3 else [channels[0]] * 3
    opaque = bytes([full]) * n if depth == 8 else array('H', [full]) * n
    components = colors + [channels.get(-1, opaque)]
    assert all(len(component) == n for component in components)
    flat = [value for pixel in zip(*components) for value in pixel]
    return bytes(flat) if depth == 8 else array('H', flat)


def unmatte(encoded, depth=8):
    """Exact rational reference for PSD merged RGB encoded against white.

    GIMP's PSD writer stores F*a + white*(1-a); its merged reader reverses that
    encoding. Quantized merged bytes cannot recover all original low-alpha RGB.
    The import contract chooses nearest byte with half-up ties, clamp, and RGB0
    when alpha0. This is unrelated to ordinary straight layer/mask channels.
    """
    full = 255 if depth == 8 else 65535
    result = bytearray(encoded) if depth == 8 else array('H', encoded)
    for i in range(0, len(result), 4):
        alpha = encoded[i + 3]
        for c in range(3):
            value = Fraction((encoded[i + c] + alpha - full) * full, alpha) if alpha else Fraction(0)
            rounded = (value + Fraction(1, 2)).numerator // (value + Fraction(1, 2)).denominator
            result[i + c] = min(full, max(0, rounded))
    return bytes(result) if depth == 8 else result


# The eleven adjustment kinds a pixel-less record can carry (Adobe's additional
# layer information keys). A record with one of them and a zero rectangle holds
# no planes at all, so it has no straight RGBA to decode.
ADJUSTMENT_KEYS = {'nvrt', 'thrs', 'post', 'levl', 'curv', 'blnc', 'hue2', 'phfl',
                   'expA', 'vibA', 'blwh'}


def resources(data):
    r, result = Reader(data), {}
    while r.pos < len(data):
        assert r.take(4) == b'8BIM'
        key = r.number('H')
        size = r.number('B')
        r.take(size)
        if (size + 1) % 2:
            r.take(1)
        body = r.slot()
        if len(body) % 2:
            r.take(1)
        assert key not in result
        result[key] = body
    return result


def record(reader):
    bounds = [reader.number('i') for _ in range(4)]
    channels = [(reader.number('h'), reader.number()) for _ in range(reader.number('H'))]
    assert reader.take(4) == b'8BIM'
    blend = reader.take(4).decode('ascii')
    opacity, clipped, flags, _ = reader.take(4)
    extra = Reader(reader.slot())
    mask_body, blend_ranges = extra.slot(), extra.slot()
    length = extra.number('B')
    name = extra.take(length).decode('mac_roman')
    extra.take((-(length + 1)) % 4)
    tags = {}
    while extra.pos < len(extra.data):
        assert extra.take(4) == b'8BIM'
        key, body = extra.take(4).decode('ascii'), extra.slot()
        assert key not in tags
        tags[key] = body
        if len(body) % 2:
            extra.take(1)
    if 'luni' in tags:
        unicode = Reader(tags['luni'])
        name = unicode.take(unicode.number() * 2).decode('utf-16-be')
        assert not any(unicode.rest())
    divider = 0
    if 'lsct' in tags:
        group = Reader(tags['lsct'])
        divider = group.number()
        if len(group.data) >= 12:
            assert group.take(4) == b'8BIM'
            blend = group.take(4).decode('ascii')
    return dict(name=name, bounds=bounds, channel_lengths=channels, blend=blend,
                opacity=opacity / 255, visible=not bool(flags & 2), clipped=bool(clipped),
                divider=divider, tags=tags, mask_body=mask_body, blend_ranges=blend_ranges)


def decode(path):
    data = path.read_bytes()
    # This oracle is used only on a pinned, tiny corpus, not untrusted large files.
    assert len(data) < 16 * 1024 * 1024
    r = Reader(data)
    assert r.take(4) == b'8BPS' and r.number('H') == 1 and r.take(6) == bytes(6)
    count, height, width = r.number('H'), r.number(), r.number()
    depth, color = r.number('H'), r.number('H')
    assert depth in (8, 16) and color in (1, 3) and 0 < width <= 512 and 0 < height <= 512
    assert not r.slot()
    metadata = resources(r.slot())
    section = Reader(r.slot())
    layer_info = Reader(section.slot())
    signed_count = layer_info.number('h')
    assert 0 < abs(signed_count) <= 64
    records = [record(layer_info) for _ in range(abs(signed_count))]
    for row in records:
        top, left, bottom, right = row['bounds']
        w, h = right - left, bottom - top
        row['size'], row['offset'] = [w, h], [left, top]
        row['channels'] = {}
        row['compression'] = {}
        mask = row['mask_body']
        row['mask'] = None
        for key, length in row['channel_lengths']:
            encoded = layer_info.take(length)
            assert key not in row['compression']
            row['compression'][key] = int.from_bytes(encoded[:2], 'big')
            if key == -2:
                assert len(mask) == 20, len(mask)
                mt, ml, mb, mr, outside, flags = struct.unpack('>4iBB', mask[:18])
                assert flags & ~3 == 0 and outside in (0, 255)
                mw, mh = mr - ml, mb - mt
                assert 0 < mw <= 512 and 0 < mh <= 512
                samples = planes(encoded, mw, mh, 1, depth)[0]
                if flags & 1:
                    assert row['divider'] == 0
                    ml, mt = ml + left, mt + top
                row['mask'] = dict(size=[mw, mh], offset=[ml, mt],
                                   coverage=list(samples) if depth == 8 else samples,
                                   outside_coverage=outside, enabled=not bool(flags & 2),
                                   linked=False, density=1, feather=0)
            else:
                assert key in (-1, 0, 1, 2)
                assert 0 <= w <= 512 and 0 <= h <= 512
                row['channels'][key] = planes(encoded, w, h, 1, depth)[0]
        if not row['divider']:
            adjustment = sorted(ADJUSTMENT_KEYS & set(row['tags']))
            if w == 0 and h == 0 and adjustment:
                # A Photoshop adjustment layer: a zero rectangle whose extra
                # data carries the kind. There are no samples to read.
                assert len(adjustment) == 1, adjustment
                row['rgba'], row['adjustment'] = None, adjustment[0]
            else:
                assert w > 0 and h > 0
                row['rgba'] = rgba(row['channels'], w, h, color, depth)
    assert not any(layer_info.rest())
    if section.pos < len(section.data):
        assert not section.slot()  # No global layer mask in this corpus.
    assert not any(section.rest())
    roots, stack = [], []
    # PSD records are stored bottom first; walking them in reverse encounters
    # each folder header before its children and bounding divider.
    for row in reversed(records):
        if row['divider'] == 3:
            assert stack
            stack.pop()
            continue
        row['depth'] = len(stack)
        row['kind'] = ('group' if row['divider'] in (1, 2)
                       else 'adjustment' if row.get('adjustment') else 'raster')
        row['open'] = row['divider'] == 1
        roots.append(row)
        if row['kind'] == 'group':
            stack.append(row)
    assert not stack
    merged = planes(r.rest(), width, height, count, depth)
    base = 3 if color == 3 else 1
    mapped = {i: merged[i] for i in range(base)}
    if signed_count < 0 and count > base:
        mapped[-1] = merged[base]
    auxiliary = merged[base + int(-1 in mapped):]
    ppi = metadata.get(1005)
    resolution = [int.from_bytes(ppi[n:n + 4], 'big') / 65536 for n in (0, 8)] if ppi else [72, 72]
    encoded = rgba(mapped, width, height, color, depth)
    return dict(canvas=[width, height], color=color, depth=depth, layers=list(reversed(roots)),
                composite=unmatte(encoded, depth) if -1 in mapped else encoded,
                composite_encoded=encoded, merged_transparency=-1 in mapped, profile=metadata.get(1039),
                resolution=resolution, resources=metadata, auxiliary=auxiliary)
