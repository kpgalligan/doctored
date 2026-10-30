"""Small independent TIFF/DNG fixtures; no third-party Python packages."""

import struct


def synthetic_dng(path, width=512, height=384, orientation=6, samples=None):
    """Uncompressed 16-bit RGGB DNG with standard camera/capture metadata.

    This tests decoder plumbing and packet transfer, not photographic RAW quality.
    Offsets are built here independently of Doctored's TIFF implementation.

    `orientation` is TIFF tag 274. `samples`, when given, is the CFA plane
    replacing the built-in pattern: width*height u16 little-endian values (bytes),
    row 0 top, RGGB, black 64 and white 1023. The defaults write the bytes this
    function always wrote.
    """
    if samples is not None and len(samples) != width * height * 2:
        raise ValueError("synthetic DNG samples must be width*height u16 values")
    # Core Image requires an explicit full-resolution NewSubfileType;
    # without tag 254 it recognizes DNG metadata but offers no RAW decoder.
    # 512 x 384 is the smallest size qualified by the standalone develop probe.
    # Larger fixtures share the same independent framing for Phase 4 timings.
    if width < 512 or height < 384 or width * height > 100_000_000:
        raise ValueError("synthetic DNG dimensions must fit the qualified 100 MP range")
    entries = {}

    def shorts(tag, values):
        entries[tag] = (3, len(values), struct.pack("<" + "H" * len(values), *values))

    def longs(tag, values):
        entries[tag] = (4, len(values), struct.pack("<" + "I" * len(values), *values))

    def text(tag, value):
        raw = value.encode() + b"\0"
        entries[tag] = (2, len(raw), raw)

    def rational(tag, values, signed=False):
        data = b"".join(struct.pack("<ii" if signed else "<II", n, d) for n, d in values)
        entries[tag] = (10 if signed else 5, len(values), data)

    longs(254, [0])
    longs(256, [width])
    longs(257, [height])
    shorts(258, [16])
    shorts(259, [1])
    shorts(262, [32803])
    text(271, "Rasterize Probe")
    text(272, "Synthetic CFA")
    longs(273, [0])
    shorts(274, [orientation])
    shorts(277, [1])
    longs(278, [height])
    longs(279, [width * height * 2])
    rational(282, [(300, 1)])
    rational(283, [(150, 1)])
    shorts(284, [1])
    shorts(296, [2])
    text(306, "2026:09:09 12:34:56")
    text(315, "Phase Three Photographer")
    text(33432, "Copyright Phase Three")
    rational(33434, [(1, 125)])
    shorts(33421, [2, 2])
    entries[33422] = (1, 4, bytes([0, 1, 1, 2]))
    entries[50706] = (1, 4, bytes([1, 4, 0, 0]))
    entries[50707] = (1, 4, bytes([1, 1, 0, 0]))
    text(50708, "Rasterize Synthetic CFA")
    entries[50710] = (1, 3, bytes([0, 1, 2]))
    shorts(50711, [1])
    shorts(50713, [1, 1])
    rational(50714, [(64, 1)])
    longs(50717, [1023])
    rational(50718, [(1, 1), (1, 1)])
    longs(50719, [0, 0])
    longs(50720, [width, height])
    # XYZ D65 -> linear sRGB camera primaries, not Doctored's matrix builder.
    matrix = [32404, -15371, -4986, -9693, 18760, 415, 557, -2040, 10570]
    rational(50721, [(n, 10000) for n in matrix], signed=True)
    rational(50728, [(1, 1)] * 3)
    shorts(50778, [21])
    longs(50829, [0, 0, height, width])
    xmp = (b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
           b'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
           b'<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" '
           b'dc:description="Synthetic RAW capture"/></rdf:RDF></x:xmpmeta>')
    entries[700] = (1, len(xmp), xmp)
    header_size = 8 + 2 + len(entries) * 12 + 4
    body = bytearray()
    records = []
    for tag, (kind, count, payload) in sorted(entries.items()):
        if len(payload) > 4:
            offset = header_size + len(body)
            value = struct.pack("<I", offset)
            body.extend(payload)
            if len(body) % 2:
                body.append(0)
        else:
            value = payload.ljust(4, b"\0")
        records.append([tag, kind, count, value])
    strip_offset = header_size + len(body)
    for record in records:
        if record[0] == 273:
            record[3] = struct.pack("<I", strip_offset)
    data = bytearray(b"II*\0\x08\0\0\0")
    data.extend(struct.pack("<H", len(records)))
    for tag, kind, count, value in records:
        data.extend(struct.pack("<HHI", tag, kind, count) + value)
    data.extend(b"\0" * 4)
    data.extend(body)
    if samples is not None:
        data.extend(samples)
    else:
        for y in range(height):
            data.extend(struct.pack("<" + "H" * width,
                                    *(220 + (x * 5 + y * 3) % 500 for x in range(width))))
    path.write_bytes(data)
