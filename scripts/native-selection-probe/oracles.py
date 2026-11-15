"""Independent native scalar, coverage and PNG inspection for Phase 9G."""
import base64
import math
import struct
import zlib


def q(value):
    return min(65535, max(0, math.floor(value + 0.5)))


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def icc_matrix_trc(profile):
    """(Y row, decode curves) of a matrix/TRC RGB ICC profile, read from the
    ICC specification rather than the product's parser: the tag table at
    offset 128 (u32 count, then signature, offset and size per tag), the Y of
    each `XYZ ` colorant tag rXYZ/gXYZ/bXYZ (s15Fixed16 X, Y, Z after the
    8-byte type header) and each rTRC/gTRC/bTRC as `curv` (no entry the
    identity, one a u8Fixed8 gamma, more a table of u16 outputs over evenly
    spaced inputs, linearly interpolated) or `para` (function types 0 to 4).
    Each curve takes and returns a value clamped to 0..1."""
    tags = {}
    for i in range(struct.unpack_from('>I', profile, 128)[0]):
        signature, offset, size = struct.unpack_from('>4sII', profile, 132 + 12*i)
        tags[signature] = profile[offset:offset + size]

    def fixed(tag, at):
        return struct.unpack_from('>i', tag, at)[0] / 65536

    def curve(tag):
        if tag[:4] == b'curv':
            count = struct.unpack_from('>I', tag, 8)[0]
            if count == 0:
                return lambda x: x
            if count == 1:
                gamma = struct.unpack_from('>H', tag, 12)[0] / 256
                return lambda x: x ** gamma
            table = struct.unpack_from(f'>{count}H', tag, 12)

            def sampled(x):
                position = x * (count - 1)
                i = min(int(position), count - 2)
                t = position - i
                return (table[i] * (1 - t) + table[i + 1] * t) / 65535
            return sampled
        assert tag[:4] == b'para', tag[:4]
        kind = struct.unpack_from('>H', tag, 8)[0]
        used = {0: 1, 1: 3, 2: 4, 3: 5, 4: 7}[kind]
        g, a, b, c, d, e, f = [fixed(tag, 12 + 4*k) for k in range(used)] + [0.0] * (7 - used)

        def power(x):
            return (a*x + b) ** g if a*x + b > 0 else 0.0

        def above(x):
            # Types 1 and 2 switch at x = -b/a; with a = 0 there is no
            # rising segment, only the constant.
            return a != 0 and x >= -b/a
        return {0: lambda x: x ** g,
                1: lambda x: power(x) if above(x) else 0.0,
                2: lambda x: power(x) + c if above(x) else c,
                3: lambda x: power(x) if x >= d else c*x,
                4: lambda x: power(x) + e if x >= d else c*x + f}[kind]

    row, curves = [], []
    for colorant in 'rgb':
        xyz = tags[colorant.encode() + b'XYZ']
        assert xyz[:4] == b'XYZ ', xyz[:4]
        row.append(fixed(xyz, 12))
        decode = curve(tags[colorant.encode() + b'TRC'])
        curves.append(lambda x, decode=decode: min(1.0, max(0.0, decode(min(1.0, max(0.0, x))))))
    return row, curves


def gray_working(rgb, row, curves, dot_gain=20):
    """The grey, 0..1, a Dot Gain `dot_gain` % Gray working space gives unit
    RGB codes `rgb`: the profile's relative PCS Y (`row` times the decoded
    codes, `icc_matrix_trc`), clamped to 0..1, raised to the power
    ln 0.5 / ln(0.5 - dot_gain/100), the curve that prints a 50 % grey at a
    reflectance of 0.5 - dot_gain/100."""
    y = min(1.0, max(0.0, sum(k * decode(v) for k, decode, v in zip(row, curves, rgb, strict=True))))
    return y ** (math.log(0.5) / math.log(0.5 - dot_gain/100))


def scalar_auto_endpoints(values, weights, clip):
    """Independent weighted Auto endpoints over raw opaque scalar codes, each
    pixel's mass its weight (65535 for a whole pixel).

    Black: the lowest code whose running mass from the bottom exceeds the clip
    mass `cut` (clip times the total mass), over u16 codes. White, at a
    positive clip: the white walk Auto Tone, Color and Contrast share at every
    depth, over 256 bins (code c in bin round(c/257), half up). From the top,
    a bin of mass b is clipped while T - (the mass above it) >= 0.7 b, with
    T = cut + 3 pixels' mass, and the lowest occupied bin never is; the white
    is the first unclipped bin, or one bin below the lowest clipped bin when
    the unclipped one holds under 3 pixels' mass, as a fraction of 255. A
    scalar's three planes are equal, so a pixel clipped in one channel is
    clipped in all and the walk alone decides. At clip 0 the white is the
    highest occupied u16 code. A black not below the white leaves (0, 1).
    """
    unit = 65535
    bins = {}
    for value, weight in zip(values, weights, strict=True):
        if weight:
            bins[value] = bins.get(value, 0) + weight
    if not bins:
        return 0.0, 1.0
    cut = sum(weights) * clip
    mass, low = 0, None
    for value in sorted(bins):
        mass += bins[value]
        if mass > cut:
            low = value
            break
    if cut <= 0:
        high = max(bins) / 65535
    else:
        eight = {}
        for value, weight in bins.items():
            eight[(2*value + 257) // 514] = eight.get((2*value + 257) // 514, 0) + weight
        occupied = sorted(eight, reverse=True)
        target, above, clipped = cut + 3*unit, 0, None
        for bin_ in occupied:
            if bin_ != occupied[-1] and target - above >= 0.7 * eight[bin_]:
                above += eight[bin_]
                clipped = bin_
                continue
            stop = bin_
            break
        high = (clipped - 1 if clipped is not None and eight[stop] < 3*unit else stop) / 255
    if low is None or f32(low / 65535) >= f32(high):
        return 0.0, 1.0
    return f32(low / 65535), f32(high)


def scalar_levels(values, weights, black, white, gamma):
    """Declared f32 Levels evaluation followed by native scalar writeback."""
    black, white, gamma = map(f32, (black, white, gamma))
    coefficients = list(map(f32, (0.2126, 0.7152, 0.0722)))
    output = []
    for value, weight in zip(values, weights, strict=True):
        source = f32(value / 65535)
        normalized = f32(f32(source-black) / f32(white-black))
        transformed = f32(min(1, max(0, normalized)) ** f32(1/gamma))
        code = value if transformed == source else sum(transformed*k for k in coefficients)*65535
        output.append(q(value+(code-value)*weight/65535))
    return tuple(output)


def scalar_gate(before, full, coverage, denominator=65535):
    return tuple(math.floor((a*(denominator-c)+b*c)/denominator+0.5)
                 for a, b, c in zip(before, full, coverage, strict=True))


def assert_gate(before, full, actual, coverage, denominator=65535):
    expected = scalar_gate(before, full, coverage, denominator)
    differences = []
    for i, (a, b, c) in enumerate(zip(actual, expected, coverage, strict=True)):
        difference = abs(a-b)
        assert difference <= (0 if c in (0, denominator) else 1), (i, a, b, c)
        differences.append(difference)
    return max(differences, default=0)


def rle16(gray, alpha=None):
    alpha = [65535]*len(gray) if alpha is None else alpha
    data = b''.join(struct.pack('<IHH', 1, g, a) for g, a in zip(gray, alpha, strict=True))
    return base64.b64encode(data).decode()


def png(data):
    """CRC-checked 8-bit grayscale/RGB/RGBA PNG; no product decoder."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    chunks, at = {}, 8
    while at < len(data):
        size = struct.unpack_from('>I', data, at)[0]
        kind, body = data[at+4:at+8], data[at+8:at+8+size]
        assert zlib.crc32(kind+body) == struct.unpack_from('>I', data, at+8+size)[0]
        chunks.setdefault(kind, []).append(body)
        at += size+12
    assert at == len(data) and b'IEND' in chunks
    w, h, depth, color, compression, filtering, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'][0])
    assert depth == 8 and compression == filtering == interlace == 0
    channels = {0:1, 2:3, 4:2, 6:4}[color]
    stride = w*channels
    raw = zlib.decompress(b''.join(chunks[b'IDAT']))
    assert len(raw) == h*(stride+1)
    above, out = bytes(stride), bytearray()
    for y in range(h):
        index = y*(stride+1)
        mode, row = raw[index], bytearray(raw[index+1:index+1+stride])
        assert mode in range(5)
        for x in range(stride):
            left = row[x-channels] if x >= channels else 0
            top = above[x]
            corner = above[x-channels] if x >= channels else 0
            if mode == 1:
                predict = left
            elif mode == 2:
                predict = top
            elif mode == 3:
                predict = (left+top)//2
            elif mode == 4:
                p = left+top-corner
                delta = [abs(p-v) for v in (left,top,corner)]
                predict = (left,top,corner)[delta.index(min(delta))]
            else:
                predict = 0
            row[x] = (row[x]+predict)&255
        out.extend(row)
        above = row
    return w, h, channels, tuple(out)


def channel(native, index):
    doc = native['document']
    channel = doc['channels'][index]
    if doc['depth'] == 16:
        return tuple(struct.unpack('<'+'H'*(doc['width']*doc['height']), channel['data']))
    w, h, count, data = png(channel['png'])
    assert (w,h,count) == (doc['width'],doc['height'],1)
    return data


def metadata(channel):
    return dict(name=channel['name'], color=list(channel.get('color',b'\xff\0\0')),
                opacity=channel.get('opacity',0.5), selected=channel.get('indicates_selected',False),
                provenance=channel.get('provenance'))


def translate(values, w, h, dx):
    out = []
    for y in range(h):
        for x in range(w):
            sample = x-dx
            left = math.floor(sample)
            fraction = sample-left
            a = values[y*w+left] if 0 <= left < w else 0
            b = values[y*w+left+1] if 0 <= left+1 < w else 0
            out.append(q(a*(1-fraction)+b*fraction))
    return tuple(out)


def morphology(values, w, h, kind, radius):
    output = []
    for i, value in enumerate(values):
        inside = value >= 32768
        distance = min((math.hypot(i%w-j%w,i//w-j//w) for j,v in enumerate(values)
                        if (v >= 32768) != inside), default=math.inf)
        signed = distance-0.5 if inside else 0.5-distance
        result = {'grow':signed+radius,'shrink':signed-radius,'border':radius/2-abs(signed)}[kind]
        output.append(q(min(1,max(0,result+0.5))*65535))
    return tuple(output)
