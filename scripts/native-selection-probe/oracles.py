"""Independent native scalar, coverage and PNG inspection for Phase 9G."""
import base64
import math
import struct
import zlib


def q(value):
    return min(65535, max(0, math.floor(value + 0.5)))


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def scalar_auto_endpoints(values, weights, clip):
    """Independent weighted order statistics over raw opaque scalar codes."""
    bins = {}
    for value, weight in zip(values, weights, strict=True):
        bins[value] = bins.get(value, 0) + weight
    cut = sum(weights) * clip
    def endpoint(reverse):
        mass = 0
        for value in sorted(bins, reverse=reverse):
            mass += bins[value]
            if mass > cut:
                return value
        return None
    low, high = endpoint(False), endpoint(True)
    if low is None or high is None or low >= high:
        return 0.0, 1.0
    return f32(low / 65535), f32(high / 65535)


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
