"""Independent small PNG16 fixture and PNG/TIFF16 output checks; standard library only."""

import json
import math
import struct
import zlib


WIDTH, HEIGHT = 32, 12


def byte_unit(value):
    """The byte a unit layer Opacity composites at below 32 bits (`opacity_byte`):
    floor(255 v + 0.5 + 1e-3) / 255, written from the integer rule, never from the core."""
    return math.floor(min(max(value, 0.0), 1.0) * 255 + 0.5 + 1e-3) / 255
XMP = b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" dc:title="Phase 7 preserving source"/></rdf:RDF></x:xmpmeta>'


def pixels():
    alphas = [0, 1, 2, 255, 256, 257, 32767, 65535]
    return [(30000 + i, 12345 + i, 40000 + i, alphas[i % 8])
            for i in range(WIDTH * HEIGHT)]


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def create(directory, profile):
    values = pixels()
    rows = [b'\0' + b''.join(struct.pack('>4H', *v) for v in values[y * WIDTH:(y + 1) * WIDTH])
            for y in range(HEIGHT)]
    data = (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', WIDTH, HEIGHT, 16, 6, 0, 0, 0))
            + chunk(b'iCCP', b'Display P3\0\0' + zlib.compress(profile))
            + chunk(b'pHYs', struct.pack('>IIB', 11811, 5906, 1))
            + chunk(b'iTXt', b'XML:com.adobe.xmp\0\0\0\0\0' + XMP)
            + chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))
    (directory / 'source16.png').write_bytes(data)
    (directory / 'source-profile.icc').write_bytes(profile)
    (directory / 'expected-rgba16.bin').write_bytes(b''.join(struct.pack('<4H', *v) for v in values))
    # A supported legacy image with an unsupported typed depth must refuse.
    low_rows = [b'\0' + bytes([128, 64, 32, 255]) * WIDTH for _ in range(HEIGHT)]
    (directory / 'source8.png').write_bytes(b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', WIDTH, HEIGHT, 8, 6, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(b''.join(low_rows))) + chunk(b'IEND', b''))


def create_tone(directory, profile, destination_profile):
    """Frozen native16 ramp, independent v11 native, and scalar ICC reference."""
    width = 65536
    raw = b'\0' + b''.join(struct.pack('>4H', i, (i * 13) % width,
                                    (i * 173) % width, 65535) for i in range(width))
    (directory / 'tone-source16.png').write_bytes(b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, 1, 16, 6, 0, 0, 0))
        + chunk(b'iCCP', b'Display P3\0\0' + zlib.compress(profile))
        + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
    (directory / 'destination-profile.icc').write_bytes(destination_profile)
    # An independent scalar f64 matrix/TRC implementation reads the actual ICC
    # coefficients. No app/core output is used to construct these expectations.
    reference = icc_reference(profile, destination_profile)
    (directory / 'expected-converted16.bin').write_bytes(b''.join(
        struct.pack('<4H', *(round(x * 65535) for x in reference(v[:3])), v[3])
        for v in pixels()))

    # Write the published Phase7B framing directly, not by changing a v12 header.
    # This fixture remains a real version11 file after the current writer moves on.
    name = b'Independent v11 source'
    native = (b'RZDC' + struct.pack('<7I', 11, 1, 1, WIDTH, HEIGHT, 1, 1)
        + struct.pack('<I', len(profile)) + profile + struct.pack('<2f', 300, 150)
        + b'\0\1' + struct.pack('<I', len(XMP)) + XMP + b'\0'
        + struct.pack('<BBHfI', 0, 1, 0, 1, len(name)) + name
        + struct.pack('<iiII', 0, 0, WIDTH, HEIGHT))
    # Native source planes carry pixels only; ICC/PPI/packets belong to the
    # outer document header above, never to the embedded PNG records.
    rows = [b'\0' + b''.join(struct.pack('>4H', *v)
        for v in pixels()[y * WIDTH:(y + 1) * WIDTH]) for y in range(HEIGHT)]
    plane = (b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', WIDTH, HEIGHT, 16, 6, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b''))
    native += struct.pack('<Q', len(plane)) + plane
    (directory / 'independent-v11.rz').write_bytes(native)
    # A real version12 fixture adds its newly supported tagged adjustment but
    # has neither version13 mask slots nor the channel tail.
    description = json.dumps({'type': 'adjust', 'evaluation': 'precision_v1',
        'op': 'invert', 'params': {}}, sort_keys=True, separators=(',', ':')).encode()
    layer_name = b'Independent v12 Invert'
    adjustment = (struct.pack('<BBHfI', 3, 1, 0, 0.375, len(layer_name)) + layer_name
        + struct.pack('<iiI', 0, 0, len(description)) + description)
    native12 = b'RZDC' + struct.pack('<7I', 12, 1, 1, WIDTH, HEIGHT, 2, 2) + native[32:] + adjustment
    (directory / 'independent-v12.rz').write_bytes(native12)
    archive = directory / 'independent-v1-archive'
    archive.mkdir()
    (archive / 'image.rz').write_bytes(native)
    (archive / 'precision-state.json').write_text(json.dumps({
        'version': 1, 'kind': 'precision-neutral', 'name': 'Phase7B captured source',
        'documentFile': 'image.rz', 'storage': 'rgba16_unorm',
        'evaluation': 'encoded_rgb_f32_v1', 'width': WIDTH, 'height': HEIGHT,
        'decodeEstimateBytes': 16 * 1024 * 1024}, sort_keys=True))
    archive12 = directory / 'independent-v2-archive'
    archive12.mkdir()
    (archive12 / 'image.rz').write_bytes(native12)
    manifest = json.loads((archive / 'precision-state.json').read_text())
    manifest.update(version=2, kind='precision-document', name='Phase7C captured adjustment')
    (archive12 / 'precision-state.json').write_text(json.dumps(manifest, sort_keys=True))


def icc_reference(source, destination):
    def parse(data):
        tags = {}
        for i in range(struct.unpack_from('>I', data, 128)[0]):
            key, offset, length = struct.unpack_from('>4sII', data, 132 + i * 12)
            tags[key] = data[offset:offset + length]

        def xyz(tag):
            assert tag[:4] == b'XYZ '
            return [v / 65536 for v in struct.unpack_from('>3i', tag, 8)]

        def curve(tag):
            if tag[:4] == b'para':
                assert struct.unpack_from('>H', tag, 8)[0] == 3
                g, a, b, c, d = [v / 65536 for v in struct.unpack_from('>5i', tag, 12)]
                return lambda x: (a * x + b) ** g if x >= d else c * x
            assert tag[:4] == b'curv'
            count = struct.unpack_from('>I', tag, 8)[0]
            assert count > 1
            table = [v / 65535 for v in struct.unpack_from(f'>{count}H', tag, 12)]
            assert all(a <= b for a, b in zip(table, table[1:]))

            def evaluate(x):
                position = max(0, min(1, x)) * (count - 1)
                left = min(count - 2, int(position))
                return table[left] + (table[left + 1] - table[left]) * (position - left)
            return evaluate

        columns = [xyz(tags[key]) for key in [b'rXYZ', b'gXYZ', b'bXYZ']]
        matrix = [[columns[c][r] for c in range(3)] for r in range(3)]
        return matrix, [curve(tags[key]) for key in [b'rTRC', b'gTRC', b'bTRC']]

    source_matrix, source_trc = parse(source)
    destination_matrix, destination_trc = parse(destination)
    augmented = [row[:] + [float(i == j) for j in range(3)]
                 for i, row in enumerate(destination_matrix)]
    for i in range(3):
        pivot = max(range(i, 3), key=lambda j: abs(augmented[j][i]))
        augmented[i], augmented[pivot] = augmented[pivot], augmented[i]
        divisor = augmented[i][i]
        assert abs(divisor) > 1e-12
        augmented[i] = [v / divisor for v in augmented[i]]
        for j in range(3):
            if j != i:
                multiplier = augmented[j][i]
                augmented[j] = [a - multiplier * b for a, b in zip(augmented[j], augmented[i])]
    inverse = [row[3:] for row in augmented]

    def encode(curve, value):
        if value <= curve(0):
            return 0.0
        if value >= curve(1):
            return 1.0
        low, high = 0.0, 1.0
        for _ in range(64):
            middle = (low + high) / 2
            if curve(middle) < value:
                low = middle
            else:
                high = middle
        return (low + high) / 2

    def transform(pixel):
        linear = [f(v / 65535) for f, v in zip(source_trc, pixel)]
        xyz = [sum(a * b for a, b in zip(row, linear)) for row in source_matrix]
        output = [sum(a * b for a, b in zip(row, xyz)) for row in inverse]
        return [encode(f, v) for f, v in zip(destination_trc, output)]
    return transform


def create_large(directory, profile):
    """Native24MP source, streamed row by row; never allocate a Python image tree."""
    width, height = 6000, 4000
    alphas = [0, 1, 2, 255, 256, 257, 32767, 65535]
    compressor = zlib.compressobj(level=1)
    with (directory / 'large-source16.png').open('xb') as output:
        output.write(b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
            + chunk(b'iCCP', b'Display P3\0\0' + zlib.compress(profile))
            + chunk(b'pHYs', struct.pack('>IIB', 11811, 5906, 1)))
        for y in range(height):
            row = bytearray(1 + width * 8)
            for x in range(width):
                i = y * width + x
                struct.pack_into('>4H', row, 1 + x * 8, (30000 + i) % 65536,
                                 (12345 + i) % 65536, (40000 + i) % 65536, alphas[i % 8])
            encoded = compressor.compress(row)
            if encoded:
                output.write(chunk(b'IDAT', encoded))
        output.write(chunk(b'IDAT', compressor.flush()) + chunk(b'IEND', b''))


def create_large_color_reference(directory, profile, destination_profile):
    """Independent scalar samples for the frozen24MP color-stage protocol."""
    trc = None
    for i in range(struct.unpack_from('>I', profile, 128)[0]):
        key, offset, length = struct.unpack_from('>4sII', profile, 132 + i * 12)
        if key == b'rTRC':
            trc = profile[offset:offset + length]
    assert trc[:4] == b'para' and struct.unpack_from('>H', trc, 8)[0] == 3
    g, a, b, c, d = [v / 65536 for v in struct.unpack_from('>5i', trc, 12)]

    def exposure(x):
        linear = (a * x + b) ** g if x >= d else c * x
        linear = min(1, max(0, linear * 2 ** 0.25))
        return (linear ** (1 / g) - b) / a if linear >= c * d else linear / c

    convert = icc_reference(profile, destination_profile)
    alphas = [0, 1, 2, 255, 256, 257, 32767, 65535]
    samples = []
    for n in range(257):
        i = n * (6000 * 4000 - 1) // 256
        source = [(30000 + i) % 65536, (12345 + i) % 65536,
                  (40000 + i) % 65536, alphas[i % 8]]
        adjusted = [v / 65535 * 0.9 for v in source[:3]]
        live = [x * 0.25 + exposure(x) * 0.75 for x in adjusted] if source[3] else [0.0] * 3
        live += [source[3] / 65535 * 0.8 * 0.9]
        destructive = ([round((v / 65535) ** 1.25 * 65535) for v in source[:3]]
                       if source[3] else source[:3]) + [source[3]]
        converted = [round(v * 65535) for v in convert(destructive[:3])] + [source[3]]
        samples.append({'index': i, 'source': source, 'live': live,
                        'destructive': destructive, 'converted': converted})
    (directory / 'large-color-reference.json').write_text(json.dumps({
        'width': 6000, 'height': 4000, 'samples': samples,
        'live_rgb_error_bound': 2 / 65535, 'live_alpha_error_bound': 2e-6 + 0.5 / 65535,
        'destructive_error_codes': 1, 'converted_error_codes': 2,
        'policy': 'Source -> Curves endpoints0,204 atopacity0.5 -> isolated group0.8 '
                  '-> Exposure0.25stops atopacity0.75 -> pass-through group0.9; '
                  'separate source -> destructive Levels gamma0.8 -> sRGB conversion'
    }, sort_keys=True))


def read_png(path):
    data = path.read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    pos, records = 8, {}
    while pos < len(data):
        count = struct.unpack_from('>I', data, pos)[0]
        kind, value = data[pos + 4:pos + 8], data[pos + 8:pos + 8 + count]
        assert zlib.crc32(kind + value) == struct.unpack_from('>I', data, pos + 8 + count)[0]
        records.setdefault(kind, []).append(value)
        pos += 12 + count
    w, h, depth, color, compression, filtering, interlace = struct.unpack('>IIBBBBB', records[b'IHDR'][0])
    assert (w, h, depth, color, compression, filtering, interlace) == (WIDTH, HEIGHT, 16, 6, 0, 0, 0)
    encoded = zlib.decompress(b''.join(records[b'IDAT']))
    stride, previous, decoded = w * 8, bytearray(w * 8), bytearray()
    assert len(encoded) == h * (stride + 1)
    for y in range(h):
        mode = encoded[y * (stride + 1)]
        row = bytearray(encoded[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for x in range(stride):
            a, b, c = row[x - 8] if x >= 8 else 0, previous[x], previous[x - 8] if x >= 8 else 0
            p = a + b - c
            paeth = a if abs(p - a) <= abs(p - b) and abs(p - a) <= abs(p - c) else b if abs(p - b) <= abs(p - c) else c
            assert mode in range(5)
            row[x] = (row[x] + [0, a, b, (a + b) // 2, paeth][mode]) & 255
        decoded.extend(row)
        previous = row
    return list(struct.iter_unpack('>4H', decoded)), records


def read_tiff(path):
    data = path.read_bytes()
    endian = '<' if data[:2] == b'II' else '>'
    assert data[:2] in (b'II', b'MM') and struct.unpack_from(endian + 'H', data, 2)[0] == 42
    offset = struct.unpack_from(endian + 'I', data, 4)[0]
    count = struct.unpack_from(endian + 'H', data, offset)[0]
    tags = {}
    for i in range(count):
        at = offset + 2 + i * 12
        tag, kind, length = struct.unpack_from(endian + 'HHI', data, at)
        size = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 7: 1}[kind] * length
        start = at + 8 if size <= 4 else struct.unpack_from(endian + 'I', data, at + 8)[0]
        raw = data[start:start + size]
        assert len(raw) == size
        tags[tag] = tuple(v[0] for v in struct.iter_unpack(endian + ('H' if kind == 3 else 'I'), raw)) if kind in (3, 4) else raw
    assert tags[256] == (WIDTH,) and tags[257] == (HEIGHT,)
    assert tags[258] == (16, 16, 16, 16) and tags[262] == (2,) and tags[277] == (4,)
    assert tags[338] == (2,), 'TIFF alpha must be explicitly straight'
    assert tags.get(284, (1,)) == (1,) and tags.get(317, (1,)) == (1,)
    assert tags[259] in [(1,), (8,), (32946,)]
    strips = []
    for start, length in zip(tags[273], tags[279], strict=True):
        strip = data[start:start + length]
        strips.append(strip if tags[259] == (1,) else zlib.decompress(strip))
    raw = b''.join(strips)
    assert len(raw) == WIDTH * HEIGHT * 8
    return list(struct.iter_unpack(endian + '4H', raw)), tags


def verify(directory, profile):
    expected = [v if v[3] else (0, 0, 0, 0) for v in pixels()]
    png, chunks = read_png(directory / 'captured-projection16.png')
    tiff, tags = read_tiff(directory / 'captured-projection16.tiff')
    for actual in [png, tiff]:
        assert len(actual) == len(expected)
        assert max(abs(a - b) for p, q in zip(actual, expected) for a, b in zip(p, q)) <= 1
    assert zlib.decompress(chunks[b'iCCP'][0].split(b'\0', 1)[1][1:]) == profile
    assert tags[34675] == profile
    assert any(XMP in value for value in chunks.get(b'iTXt', []))
    assert tags[700] == XMP
    assert tags[296] == (2,)
    for tag, expected_ppi in [(282, 300), (283, 150)]:
        n, d = struct.unpack(('<' if (directory / 'captured-projection16.tiff').read_bytes()[:2] == b'II' else '>') + 'II', tags[tag])
        assert abs(n / d - expected_ppi) < 0.02
    old_projection, old_chunks = read_png(directory / 'public-captured-projection16.png')
    current_projection, current_tags = read_tiff(directory / 'public-current-projection16.tiff')
    # The later edit's Opacity 0.5 composites at its byte, 128/255.
    later = byte_unit(0.5)
    for old, current, source in zip(old_projection, current_projection, pixels(), strict=True):
        assert old[3] == source[3]
        assert abs(current[3] - round(source[3] * later)) <= 1
        for c in range(3):
            expected_color = source[c] if source[3] else 0
            assert abs(old[c] - expected_color) <= 1
            assert abs(current[c] - expected_color) <= 1
    assert zlib.decompress(old_chunks[b'iCCP'][0].split(b'\0', 1)[1][1:]) == profile
    assert current_tags[34675] == profile and current_tags[700] == XMP
    assert any(XMP in value for value in old_chunks.get(b'iTXt', []))
    print('PASS: independent RGB16 PNG/TIFF, profile/PPI/XMP, alpha, captured projection reuse and later opacity oracle')
    captured, _ = read_png(directory / 'captured-adjustment16.png')
    # The Invert's Opacity 0.375 composites at its byte, 96/255.
    invert = byte_unit(0.375)
    for actual, source in zip(captured, pixels(), strict=True):
        assert actual[3] == source[3]
        for c in range(3):
            x = source[c] / 65535
            reference = x * (1 - invert) + (1 - x) * invert if source[3] else 0
            assert abs(actual[c] / 65535 - reference) <= 2e-6 + 0.5 / 65535
    native = read_native_adjustments(directory / 'adjusted-current.rz')
    archived = read_native_adjustments(directory / 'adjusted-archive/image.rz')
    assert native == archived
    assert [json.loads(item) for item in native] == [
        {'type': 'adjust', 'evaluation': 'precision_v1', 'op': 'invert', 'params': {}}]
    scalar = read_native_structure(directory / 'scalar-v13.rz')
    assert scalar['version'] == 13
    assert len(scalar['masks']) == 1 and scalar['masks'][0]['values'] == [0, 1, 256, 257, 32768, 65535]
    assert scalar['masks'][0]['extent'] == [2, 3, 3, 2]
    assert scalar['channels'] == [{'name': 'Fine coverage', 'values': [(i * 257 + 1) % 65536 for i in range(384)]}]
    print('PASS: independent captured-adjustment PNG, native13 u16 masks/channels and exact archive operator descriptions')


def read_native_adjustments(path):
    return read_native_structure(path)['descriptions']


def refusal_table(data, start, end):
    """-> (table start, {asset index: sentence}) for an RSPR table ending at `end`.

    RSPR: u32 schema 1, u32 count >= 1, then per entry a u32 asset
    index (strictly ascending), a u32 byte length 1..4096 and that many UTF-8
    bytes with no NUL. A sentence may itself hold the bytes `RSPR`, so every
    candidate in [start, end - 12] is tried from the right; the first that
    parses exactly to `end` wins. The asset-count bound is the caller's.
    """
    at = data.rfind(b'RSPR', start, end - 8)
    while at >= start:
        schema, count = struct.unpack_from('<II', data, at + 4)
        position, last, refusals = at + 12, -1, {}
        valid = schema == 1 and count >= 1
        for _ in range(count if valid else 0):
            if position + 8 > end:
                valid = False
                break
            index, length = struct.unpack_from('<II', data, position)
            position += 8
            if index <= last or not 1 <= length <= 4096 or position + length > end:
                valid = False
                break
            try:
                text = data[position:position + length].decode('utf-8')
            except UnicodeDecodeError:
                valid = False
                break
            if '\0' in text:
                valid = False
                break
            refusals[index] = text
            last, position = index, position + length
        if valid and position == end:
            return at, refusals
        at = data.rfind(b'RSPR', start, at)
    raise AssertionError('Malformed RSPR table')


def projective_table(data, start, end):
    """-> (table start, {layer: 9 f64 matrix}) for an RSPJ table ending at `end`.

    RSPJ (`source_transform_native.rs`): u32 schema 1, u32 count >= 1, then
    count entries of a u32 layer (strictly ascending) and 9 f64, 76 bytes
    each. Every candidate from the right is tried; the first whose size closes
    exactly at `end` wins. The layer-count bound is the caller's.
    """
    at = data.rfind(b'RSPJ', start, end - 8)
    while at >= start:
        schema, count = struct.unpack_from('<II', data, at + 4)
        if schema == 1 and count >= 1 and at + 12 + 76 * count == end:
            layers = [struct.unpack_from('<I', data, at + 12 + 76 * i)[0] for i in range(count)]
            if all(a < b for a, b in zip(layers, layers[1:])):
                return at, {layer: list(struct.unpack_from('<9d', data, at + 16 + 76 * i))
                            for i, layer in enumerate(layers)}
        at = data.rfind(b'RSPJ', start, at)
    raise AssertionError('Malformed RSPJ table')


def identity_tables(data):
    """-> (start of the inner version word, end of the inner body, refusals, projective).

    Every current save carries the layer-identity wrapper (`identity_native.rs`):
    an outer version word 45 (byte) / 46 (typed) at offset 4 and a LIDS table
    (tag, u32 schema 1, u32 count, count u64 ids) closing the file. Word 56 /
    57 sits first inside it with its RSPR table the last one before LIDS;
    typed word 55 follows with its LOPN table (closed groups) just before
    that; word 51 / 52 next carries the RSPJ table (`projective_table`)
    before those, each layer below the LIDS count. Without a 45/46 word the
    whole file is the body. Warp (53/54) and knockout (49) words are not
    learned here: the body version word then refuses them.
    """
    outer = struct.unpack_from('<I', data, 4)[0]
    if outer not in (45, 46):
        return 4, len(data), {}, {}
    lids = data.rfind(b'LIDS')
    assert lids > 0, 'Layer-identity wrapper without its LIDS table'
    schema, count = struct.unpack_from('<II', data, lids + 4)
    assert schema == 1 and lids + 12 + 8 * count == len(data), 'Malformed LIDS table'
    start, end, refusals = 8, lids, {}
    if struct.unpack_from('<I', data, 8)[0] == (56 if outer == 45 else 57):
        start = 12
        end, refusals = refusal_table(data, start, end)
    if outer == 46 and struct.unpack_from('<I', data, start)[0] == 55:
        start += 4
        lopn = data.rfind(b'LOPN', 0, end)
        schema, closed = struct.unpack_from('<II', data, lopn + 4)
        assert lopn > 0 and schema == 1 and closed > 0 and lopn + 12 + 4 * closed == end, \
            'Malformed LOPN table'
        end = lopn
    projective = {}
    if struct.unpack_from('<I', data, start)[0] == (51 if outer == 45 else 52):
        start += 4
        end, projective = projective_table(data, start, end)
        assert all(layer < count for layer in projective), 'RSPJ layer beyond the LIDS count'
    return start, end, refusals, projective


def read_native_structure(path):
    """Read this small probe's published native11–13 framing, independently."""
    data = path.read_bytes()
    assert len(data) < 16 * 1024 * 1024
    # Check the layer-identity wrapper and its tables (`identity_tables`),
    # then read the wrapped body as published.
    if data[:4] in (b'RZDC', b'DRDC'):
        start, end, _, _ = identity_tables(data)
        data = data[:4] + data[start:end]
    position = 0

    def take(n):
        nonlocal position
        assert 0 <= n <= len(data) - position
        result = data[position:position + n]
        position += n
        return result

    def u32():
        return struct.unpack('<I', take(4))[0]

    def blob():
        return take(u32())

    assert take(4) in (b'RZDC', b'DRDC')
    version, storage, evaluation, width, height, roots, records = struct.unpack('<7I', take(28))
    assert version in [11, 12, 13]
    assert (storage, evaluation, width, height) == (1, 1, WIDTH, HEIGHT)
    assert 1 <= roots <= records <= 32
    blob()  # ICC
    take(8)  # PPI
    for _ in range(3):
        flag = take(1)[0]
        assert flag in [0, 1]
        if flag:
            blob()
    descriptions = []
    masks = []
    channels = []
    count = 0

    def record(depth):
        nonlocal count
        count += 1
        assert count <= records and depth <= 16
        kind, visible, reserved, opacity = struct.unpack('<BBHf', take(8))
        assert visible in [0, 1] and reserved == 0 and 0 <= opacity <= 1
        name = blob().decode('utf-8')
        offset = take(8)
        if version >= 13:
            present = take(1)[0]
            assert present in [0, 1]
            if present:
                enabled, linked, flags = struct.unpack('<BBH', take(4))
                assert enabled in [0, 1] and linked in [0, 1] and flags == 0
                x, y, w, h, density, feather = struct.unpack('<iiIIff', take(24))
                length = struct.unpack('<Q', take(8))[0]
                assert length == w * h * 2
                values = list(struct.unpack(f'<{w * h}H', take(length)))
                masks.append({'name': name, 'extent': [x, y, w, h], 'enabled': enabled,
                              'linked': linked, 'density': density, 'feather': feather, 'values': values})
        if kind == 0:
            take(8)  # source dimensions
            take(struct.unpack('<Q', take(8))[0])
        elif kind == 3:
            assert version >= 12 and offset == bytes(8)
            descriptions.append(blob().decode('utf-8'))
        else:
            assert kind in [1, 2] and offset == bytes(8)
            for _ in range(u32()):
                record(depth + 1)
    for _ in range(roots):
        record(0)
    if version >= 13:
        count_channels = u32()
        assert count_channels <= 32
        for _ in range(count_channels):
            name = blob().decode('utf-8')
            length = struct.unpack('<Q', take(8))[0]
            assert length == width * height * 2
            channels.append({'name': name, 'values': list(struct.unpack(f'<{width * height}H', take(length)))})
    assert count == records and position == len(data)
    return {'version': version, 'descriptions': descriptions, 'masks': masks, 'channels': channels}
