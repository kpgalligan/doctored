"""Independent reader for Phase 8C DRDC source records and raw PNG samples.

Only the documented source trailer is decoded here. The application/core loader
is never called. Controlled fixtures must have one unambiguous RSRC trailer.

DRDC v18/v19 source records are read directly, or inside the 12E-1 identity
wrapper (word BYTE_IDENTITY/TYPED_IDENTITY (45/46) at offset 4 plus a trailing
LIDS table, optionally with a typed TYPED_GLOBAL_LIGHT (58) global-light word
and a fixed 16-byte LGLT table, a BYTE_REFUSAL/TYPED_REFUSAL (56/57) source
profile-refusal word and an RSPR table, a typed TYPED_GROUP_OPEN (55) group-open
word and an LOPN table, a BYTE_WARP/TYPED_WARP (53/54) warp word and a WARP
table, a BYTE_PROJECTIVE/TYPED_PROJECTIVE (51/52) projective word and an RSPJ
table, and a byte BYTE_KNOCKOUT (49) style-knockout word with no table), peeled
by `identity_body`. Those words are read from the core source constants
(`identity_native`, `precision_light_native`, `source_refusal_native`,
`precision_group_open_native`,
`warp_native`, `source_transform_native`, `style_knockout`), not written here
as literals. That function is the shared peel other probes import;
`source_profile_refusals`, `warp_entries` and `knockout_word` return the RSPR
sentences, the WARP entries and the 49 flag from the same peel. An unknown
outer wrapper word is refused on purpose rather than guessed at.
"""

from array import array
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import sys
import zlib

ROOT = Path(__file__).resolve().parents[2]


def core_word(relative, name):
    """A `u32` version constant read from the core source at ROOT/relative."""
    found = re.findall(rf'pub(?:\(crate\))? const {name}: u32 = (\d+);', (ROOT / relative).read_text())
    assert len(found) == 1, (relative, name, found)
    return int(found[0])


BYTE_IDENTITY = core_word('core/src/identity_native.rs', 'BYTE_VERSION')
TYPED_IDENTITY = core_word('core/src/identity_native.rs', 'PRECISION_VERSION')
BYTE_REFUSAL = core_word('core/src/source_refusal_native.rs', 'BYTE_VERSION')
TYPED_REFUSAL = core_word('core/src/source_refusal_native.rs', 'PRECISION_VERSION')
TYPED_GROUP_OPEN = core_word('core/src/precision_group_open_native.rs', 'PRECISION_VERSION')
TYPED_GLOBAL_LIGHT = core_word('core/src/precision_light_native.rs', 'PRECISION_VERSION')
BYTE_PROJECTIVE = core_word('core/src/source_transform_native.rs', 'BYTE_VERSION')
TYPED_PROJECTIVE = core_word('core/src/source_transform_native.rs', 'PRECISION_VERSION')
BYTE_WARP = core_word('core/src/warp_native.rs', 'BYTE_VERSION')
TYPED_WARP = core_word('core/src/warp_native.rs', 'PRECISION_VERSION')
BYTE_KNOCKOUT = core_word('core/src/style_knockout.rs', 'BYTE_VERSION')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def png_samples(data):
    """CRC-checked RGBA8/16 PNG decoder, returning compact native-endian samples."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at, chunks = 8, {}
    while at < len(data):
        size = struct.unpack_from('>I', data, at)[0]
        kind, body = data[at + 4:at + 8], data[at + 8:at + 8 + size]
        assert len(body) == size
        assert zlib.crc32(kind + body) == struct.unpack_from('>I', data, at + 8 + size)[0]
        chunks.setdefault(kind, []).append(body)
        at += size + 12
    assert at == len(data) and len(chunks[b'IHDR']) == 1 and b'IEND' in chunks
    width, height, depth, color, compression, filtering, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'][0])
    assert depth in (8, 16) and (color, compression, filtering, interlace) == (6, 0, 0, 0)
    unit = 4 * (depth // 8)
    stride = width * unit
    encoded = zlib.decompress(b''.join(chunks[b'IDAT']))
    assert len(encoded) == (stride + 1) * height
    previous, decoded = bytearray(stride), bytearray()
    for y in range(height):
        offset = y * (stride + 1)
        mode = encoded[offset]
        row = bytearray(encoded[offset + 1:offset + stride + 1])
        assert mode in range(5)
        if mode == 1:
            for x in range(unit, stride):
                row[x] = (row[x] + row[x - unit]) & 255
        elif mode == 2:
            row = bytearray((value + above) & 255 for value, above in zip(row, previous))
        elif mode == 3:
            for x in range(stride):
                left = row[x - unit] if x >= unit else 0
                row[x] = (row[x] + (left + previous[x]) // 2) & 255
        elif mode == 4:
            for x in range(stride):
                left, above = (row[x - unit] if x >= unit else 0), previous[x]
                corner = previous[x - unit] if x >= unit else 0
                p = left + above - corner
                a, b, c = abs(p - left), abs(p - above), abs(p - corner)
                predictor = left if a <= b and a <= c else (above if b <= c else corner)
                row[x] = (row[x] + predictor) & 255
        decoded.extend(row)
        previous = row
    samples = array('H' if depth == 16 else 'B')
    samples.frombytes(decoded)
    if depth == 16 and sys.byteorder == 'little':
        samples.byteswap()
    return {'width': width, 'height': height, 'depth': depth, 'samples': samples,
            'profile': zlib.decompress(chunks[b'iCCP'][0].split(b'\0', 1)[1][1:]) if b'iCCP' in chunks else None}


def group_open_table(data, start, end):
    """-> (table_start, closed) for the typed GROUP DISCLOSURE.

    Typed word 55, the first word inside 46 after any 57 word, adds one LOPN
    table (schema 1, count >= 1, count strictly ascending u32 postorder
    indices of the CLOSED groups, each below the record count) that closes
    just before RSPR when present, else LIDS; that table starts at `end`.
    Validated from both ends, never found by a marker search alone.
    """
    marker = data.rfind(b'LOPN', start, end)
    assert marker >= start and marker + 12 <= end
    schema, count = struct.unpack_from('<II', data, marker + 4)
    assert schema == 1 and count > 0 and marker + 12 + count * 4 == end
    closed = list(struct.unpack_from('<' + 'I' * count, data, marker + 12))
    assert all(a < b for a, b in zip(closed, closed[1:]))
    return marker, closed


def refusal_table(data, start, end):
    """-> (table_start, {asset index: sentence}) for the RSPR table.

    Byte word BYTE_REFUSAL (56) / typed TYPED_REFUSAL (57), the first word
    inside 45/46, adds one RSPR table (schema 1, count >= 1, then per entry a
    u32 asset index strictly ascending, a u32 byte length 1..4096 and that many
    UTF-8 bytes with no NUL) as the last table before LIDS, which starts at
    `end`. A sentence may itself contain the bytes `RSPR`, so every candidate
    from the right is tried and the first that parses exactly to `end` wins.
    The asset-count bound is the caller's.
    """
    at = data.rfind(b'RSPR', start, end - 8)
    while at >= start:
        refusals = _refusal_entries(data, at, end)
        if refusals is not None:
            return at, refusals
        at = data.rfind(b'RSPR', start, at)
    raise AssertionError('Malformed RSPR table')


def _refusal_entries(data, at, end):
    schema, count = struct.unpack_from('<II', data, at + 4)
    if schema != 1 or count < 1:
        return None
    position, last, refusals = at + 12, -1, {}
    for _ in range(count):
        if position + 8 > end:
            return None
        index, length = struct.unpack_from('<II', data, position)
        position += 8
        if index <= last or not 1 <= length <= 4096 or position + length > end:
            return None
        try:
            text = data[position:position + length].decode('utf-8')
        except UnicodeDecodeError:
            return None
        if '\0' in text:
            return None
        refusals[index] = text
        last, position = index, position + length
    return refusals if position == end else None


def warp_table(data, start, end, records, sample_bytes):
    """-> (table_start, {layer index: entry}) for the WARP table.

    Byte word BYTE_WARP (53) / typed TYPED_WARP (54) adds one WARP table that
    closes just before LOPN, RSPR or LIDS, which starts at `end`: schema 1,
    count 1..records, then per entry a u32 layer index (strictly ascending,
    below `records`), u8 target (0 pixels, 1 mask), u8 filter (0 bilinear,
    1 bicubic), u8 linked-mask flag, u8 mask outside coverage (0 or 255), u32
    columns and u32 rows (each 2..16), columns * rows (dx, dy) f64 pairs (finite,
    |v| <= 30000, not all zero), then the original pixel plane and the
    original mask plane: a u8 presence flag and, when present, i32 x, i32 y,
    u32 w > 0, u32 h > 0, u64 length = w * h * sample_bytes * (4 or 1) and
    those bytes. Pixels are present exactly for target 0, the mask exactly for
    target 1 or a linked mask, and a mask target is never linked
    (`warp_native::parse`, `WarpField::{validate,identity}`). Plane bytes may
    hold `WARP`, so every candidate from the right is tried and the first that
    parses exactly to `end` wins.
    """
    at = data.rfind(b'WARP', start, end - 8)
    while at >= start:
        entries = _warp_entries(data, at, end, records, sample_bytes)
        if entries is not None:
            return at, entries
        at = data.rfind(b'WARP', start, at)
    raise AssertionError('Malformed WARP table')


def _warp_entries(data, at, end, records, sample_bytes):
    schema, count = struct.unpack_from('<II', data, at + 4)
    if schema != 1 or not 1 <= count <= records:
        return None
    position, last, entries = at + 12, -1, {}
    for _ in range(count):
        if position + 16 > end:
            return None
        index, target, filtering, linked, outside, columns, rows = struct.unpack_from('<IBBBBII', data, position)
        position += 16
        if (index <= last or index >= records or target > 1 or filtering > 1 or linked > 1
                or outside not in (0, 255) or not 2 <= columns <= 16 or not 2 <= rows <= 16):
            return None
        if position + columns * rows * 16 > end:
            return None
        values = struct.unpack_from(f'<{columns * rows * 2}d', data, position)
        position += columns * rows * 16
        if not all(math.isfinite(v) and abs(v) <= 30000 for v in values) or not any(values):
            return None
        planes = []
        for channels in (4, 1):
            if position + 1 > end or data[position] > 1:
                return None
            present = data[position]
            position += 1
            if not present:
                planes.append(None)
                continue
            if position + 24 > end:
                return None
            x, y, width, height, length = struct.unpack_from('<iiIIQ', data, position)
            position += 24
            if (width == 0 or height == 0 or length != width * height * sample_bytes * channels
                    or position + length > end):
                return None
            planes.append({'offset': (x, y), 'dimensions': (width, height),
                           'sha256': digest(data[position:position + length])})
            position += length
        pixels, mask = planes
        if ((pixels is not None) != (target == 0) or (mask is not None) != (target == 1 or linked == 1)
                or (target == 1 and linked == 1)):
            return None
        entries[index] = {'target': ('pixels', 'mask')[target], 'filter': ('bilinear', 'bicubic')[filtering],
                          'linked_mask': linked == 1, 'mask_outside': outside, 'columns': columns, 'rows': rows,
                          'displacements': [(values[i], values[i + 1]) for i in range(0, len(values), 2)],
                          'pixels': pixels, 'mask': mask}
        last = index
    return entries if position == end else None


def _peel(data, inner=None):
    """-> (body, shift, ids, identity_word, projective, refusals, warp, knockout); see `identity_body`."""
    assert len(data) >= 8 and data[:4] in (b'RZDC', b'DRDC')
    outer = struct.unpack_from('<I', data, 4)[0]
    if outer not in (BYTE_IDENTITY, TYPED_IDENTITY):
        return data, 0, None, None, {}, {}, {}, False
    start = 8
    light_word = (outer == TYPED_IDENTITY and len(data) >= 12
                  and struct.unpack_from('<I', data, 8)[0] == TYPED_GLOBAL_LIGHT)
    if light_word:
        start = 12
    refusal_word = (len(data) >= start + 4 and struct.unpack_from('<I', data, start)[0]
                    == (BYTE_REFUSAL if outer == BYTE_IDENTITY else TYPED_REFUSAL))
    if refusal_word:
        start += 4
    open_word = (outer == TYPED_IDENTITY and len(data) >= start + 4
                 and struct.unpack_from('<I', data, start)[0] == TYPED_GROUP_OPEN)
    if open_word:
        start += 4
    warp_word = (len(data) >= start + 4
                 and struct.unpack_from('<I', data, start)[0] == (BYTE_WARP if outer == BYTE_IDENTITY else TYPED_WARP))
    if warp_word:
        start += 4
    projective_word = (len(data) >= start + 4
                       and struct.unpack_from('<I', data, start)[0] == (BYTE_PROJECTIVE if outer == BYTE_IDENTITY else TYPED_PROJECTIVE))
    if projective_word:
        start += 4
    knockout = (outer == BYTE_IDENTITY and len(data) >= start + 4
                and struct.unpack_from('<I', data, start)[0] == BYTE_KNOCKOUT)
    if knockout:
        start += 4
    lids = data.rfind(b'LIDS')
    assert lids > start and lids + 12 <= len(data)
    schema, count = struct.unpack_from('<II', data, lids + 4)
    assert schema == 1 and count > 0 and lids + 12 + count * 8 == len(data)
    ids = struct.unpack_from('<' + 'Q' * count, data, lids + 12)
    assert all(ids) and len(set(ids)) == count
    if inner is not None:
        assert start + 4 <= lids
        version = struct.unpack_from('<I', data, start)[0]
        assert version in inner.get(outer, ()), (outer, version)
    projective, refusals, warp = {}, {}, {}
    end = lids
    if light_word:
        # LGLT: tag, u32 schema 1, f32 angle, f32 altitude — fixed 16 bytes,
        # the last table before LIDS.
        assert end - 16 >= start and data[end - 16:end - 12] == b'LGLT'
        assert struct.unpack_from('<I', data, end - 12)[0] == 1
        end -= 16
    if refusal_word:
        end, refusals = refusal_table(data, start, end)
    if open_word:
        end, closed = group_open_table(data, start, end)
        assert all(index < len(ids) for index in closed)
    if warp_word:
        end, warp = warp_table(data, start, end, len(ids), 1 if outer == BYTE_IDENTITY else 2)
    if projective_word:
        marker = data.rfind(b'RSPJ', start, end)
        assert marker >= start and marker + 12 <= end
        schema, count = struct.unpack_from('<II', data, marker + 4)
        assert schema == 1 and 0 < count <= 100000
        assert marker + 12 + count * 76 == end
        for index in range(count):
            offset = marker + 12 + index * 76
            layer = struct.unpack_from('<I', data, offset)[0]
            assert layer not in projective
            projective[layer] = list(struct.unpack_from('<9d', data, offset + 4))
        end = marker
    return data[:4] + data[start:end], start - 4, ids, outer, projective, refusals, warp, knockout


def identity_body(data, inner=None):
    """-> (body, shift, ids, identity_word, projective)

    Word BYTE_IDENTITY/TYPED_IDENTITY (45/46) at offset 4 wraps an older body
    and appends one LIDS table (schema 1, count, count nonzero unique u64 ids)
    as the file's last bytes. Typed word TYPED_GLOBAL_LIGHT (58), when it is
    the first word inside, adds one fixed 16-byte LGLT table (tag, u32 schema
    1, f32 angle, f32 altitude) just before LIDS, which is peeled and dropped.
    Word BYTE_REFUSAL/TYPED_REFUSAL (56/57), next inside, adds one RSPR table
    just before LGLT or LIDS
    (`refusal_table`), which is peeled and set aside
    (`source_profile_refusals` returns it). Typed word TYPED_GROUP_OPEN (55)
    after those adds one LOPN table just before RSPR or LIDS
    (`group_open_table`), which is peeled and set aside. A BYTE_WARP/TYPED_WARP
    (53/54) word after those adds one WARP table just before LOPN, RSPR or LIDS
    (`warp_table`), which is peeled and set aside (`warp_entries` returns it). A
    BYTE_PROJECTIVE/TYPED_PROJECTIVE (51/52) word after those adds one RSPJ
    table (schema 1, count, count entries of u32 layer + 9 f64) just before
    WARP, LOPN, RSPR or LIDS. A byte BYTE_KNOCKOUT (49) word last adds no table
    (`knockout_word` reports it). All eleven words come from the core source
    constants (`core_word`). `body` is the DRDC magic
    plus the inner body, so body[4:8] is the inner version word; body position
    p >= 4 is file position p + shift. Any other outer word returns the data
    unchanged with shift 0, no ids, no identity word and no projective table.
    When `inner` is given, a wrapped body's version word must be listed in
    inner[identity_word]. Validate both ends; never treat an arbitrary marker in
    a PNG as a table.
    """
    return _peel(data, inner)[:5]


def source_profile_refusals(data):
    """{asset index: sentence} from the RSPR table; {} without a 56/57 word."""
    return _peel(data)[5]


def warp_entries(data):
    """{layer index: entry} from the WARP table (`warp_table`); {} without a 53/54 word."""
    return _peel(data)[6]


def knockout_word(data):
    """Whether a byte file carries the style-knockout word 49 inside 45."""
    return _peel(data)[7]


def read_native(path, *, pixels=True):
    data, shift, _, _, projective = identity_body(Path(path).read_bytes(), inner={BYTE_IDENTITY: (18,), TYPED_IDENTITY: (19,)})
    assert not projective
    assert data[:4] in (b'RZDC', b'DRDC')
    version = struct.unpack_from('<I', data, 4)[0]
    assert version in (18, 19), version
    offset = data.find(b'RSRC')
    assert offset >= 0 and data.find(b'RSRC', offset + 4) == -1
    at = offset + 4

    def number(code='I'):
        nonlocal at
        result = struct.unpack_from('<' + code, data, at)[0]
        at += struct.calcsize(code)
        return result

    def slot(code='I'):
        nonlocal at
        size = number(code)
        result = data[at:at + size]
        assert len(result) == size
        at += size
        return result

    assets = []
    for _ in range(number()):
        identity, revision = slot().decode(), slot().decode()
        depth = number('B')
        assert data[at:at + 3] == bytes(3)
        at += 3
        profile = slot()
        width, height = number(), number()
        png = slot('Q')
        raw = number('B')
        assert raw in (0, 1)
        payload_index, descriptor = None, None
        descriptor_range = None
        if raw:
            assert depth == 16
            payload_index = number()
            start = at
            descriptor_data = slot()
            descriptor = json.loads(descriptor_data)
            descriptor_range = (start + shift, at + shift)
        asset = {'source_id': identity, 'revision_id': revision, 'depth': depth,
                 'width': width, 'height': height, 'profile_sha256': digest(profile),
                 'png_sha256': digest(png), 'payload_index': payload_index,
                 'descriptor': descriptor, 'descriptor_range': descriptor_range}
        if pixels:
            decoded = png_samples(png)
            assert (decoded['width'], decoded['height'], decoded['depth']) == (width, height, depth)
            asset['samples'] = decoded['samples']
        assets.append(asset)
    bindings = []
    for _ in range(number()):
        layer, asset, sampler = number(), number(), number('B')
        assert data[at:at + 3] == bytes(3) and asset < len(assets)
        at += 3
        transform = list(struct.unpack_from('<6d', data, at))
        at += 48
        linked = number('B')
        assert linked in (0, 1)
        link = None
        if linked:
            link = {'path': slot().decode(), 'sha256': data[at:at + 64].decode()}
            at += 64
        bindings.append({'layer': layer, 'asset': asset, 'sampler': sampler,
                         'transform': transform, 'link': link})
    assert data[at:at + 4] == b'RRAW'
    at += 4
    payloads = [slot('Q') for _ in range(number())]
    assert at == len(data)
    for asset in assets:
        if asset['descriptor'] is not None:
            raw = payloads[asset['payload_index']]
            assert digest(raw) == asset['descriptor']['content_sha256']
    return {'version': version, 'assets': assets, 'bindings': bindings, 'payloads': payloads}


def incompatible_decoder(source, destination):
    """Change only opaque implementation provenance, preserving every saved pixel."""
    parsed = read_native(source, pixels=False)
    raw = next(asset for asset in parsed['assets'] if asset['descriptor'] is not None)
    start, end = raw['descriptor_range']
    descriptor = raw['descriptor']
    descriptor['decoder']['implementation'] = 'unavailable-independent-probe-implementation'
    value = json.dumps(descriptor, separators=(',', ':'), sort_keys=True).encode()
    data = Path(source).read_bytes()
    Path(destination).write_bytes(data[:start] + struct.pack('<I', len(value)) + value + data[end:])


def fallback_pngs(path):
    """Return the controlled native raster PNGs preceding the source trailer."""
    data = Path(path).read_bytes()
    end = data.find(b'RSRC')
    assert end > 0
    result, at = [], 0
    signature = b'\x89PNG\r\n\x1a\n'
    while True:
        start = data.find(signature, at, end)
        if start < 0:
            break
        at = start + 8
        while True:
            length = struct.unpack_from('>I', data, at)[0]
            kind = data[at + 4:at + 8]
            assert at + 12 + length <= end
            at += 12 + length
            if kind == b'IEND':
                break
        result.append(data[start:at])
    return result


def tiff_samples(path):
    """Independent classic TIFF16 strip decoder for the qualified output writer."""
    data = Path(path).read_bytes()
    endian = '<' if data[:2] == b'II' else '>'
    assert data[:2] in (b'II', b'MM') and struct.unpack_from(endian + 'H', data, 2)[0] == 42
    start = struct.unpack_from(endian + 'I', data, 4)[0]
    count = struct.unpack_from(endian + 'H', data, start)[0]
    tags = {}
    for index in range(count):
        at = start + 2 + index * 12
        tag, kind, length = struct.unpack_from(endian + 'HHI', data, at)
        size = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 7: 1}[kind] * length
        offset = at + 8 if size <= 4 else struct.unpack_from(endian + 'I', data, at + 8)[0]
        raw = data[offset:offset + size]
        assert len(raw) == size
        tags[tag] = tuple(item[0] for item in struct.iter_unpack(endian + ('H' if kind == 3 else 'I'), raw)) if kind in (3, 4) else raw
    width, height = tags[256][0], tags[257][0]
    assert tags[258] == (16, 16, 16, 16) and tags[262] == (2,) and tags[277] == (4,)
    assert tags[338] == (2,) and tags.get(284, (1,)) == (1,) and tags.get(317, (1,)) == (1,)
    assert tags[259] in ((1,), (8,), (32946,))
    decoded = bytearray()
    for offset, length in zip(tags[273], tags[279], strict=True):
        encoded = data[offset:offset + length]
        decoded.extend(encoded if tags[259] == (1,) else zlib.decompress(encoded))
    assert len(decoded) == width * height * 8
    values = array('H')
    values.frombytes(decoded)
    if (sys.byteorder == 'little') != (endian == '<'):
        values.byteswap()
    return {'width': width, 'height': height, 'depth': 16, 'samples': values, 'profile': tags[34675]}
