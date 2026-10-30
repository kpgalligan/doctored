"""Independent Phase 8B fixtures, master-depth sampling and native asset decoding."""

import hashlib
import json
from pathlib import Path
import struct
import zlib

from retained_fixtures import WIDTH, HEIGHT, bilinear_eighth, encode_png
from fixtures import icc_reference, identity_tables


def linked_pixels(depth, variant=0, width=WIDTH, height=HEIGHT):
    maximum = (1 << depth) - 1
    alphas = ([0, 1, 2, 17, 128, 254, 255, 255] if depth == 8 else
              [0, 1, 2, 255, 256, 257, 32767, 65535])
    return [((17011 + i * 1031 + (i // width) * 419 + variant * 4051) & maximum,
             (31337 + i * 619 + variant * 107) & maximum,
             (7919 + i * 3571 + variant * 1801) & maximum,
             alphas[(i + variant * 3) % len(alphas)]) for i in range(width * height)]


def fallback_pixels(values, source_depth, document_depth):
    if source_depth == document_depth:
        return [list(pixel) for pixel in values]
    if source_depth == 8:
        return [[component * 257 for component in pixel] for pixel in values]
    return [[(component + 128) // 257 for component in pixel] for pixel in values]


def create_linked(directory, profile, destination_profile):
    convert = icc_reference(profile, destination_profile)
    for depth in (8, 16):
        for name, variant, width, height in [('source', 0, WIDTH, HEIGHT),
                                             ('changed', 1, WIDTH, HEIGHT),
                                             ('small', 2, 48, 32)]:
            pixels = linked_pixels(depth, variant, width, height)
            (directory / f'linked-{name}{depth}.png').write_bytes(
                encode_png(width, height, depth, pixels, profile))
        for target in (8, 16):
            source = linked_pixels(depth)
            # Byte sources enter a typed evaluator before interpolation when
            # the destination is typed. Typed masters remain typed until the
            # final explicit byte fallback conversion.
            sampling_depth = max(depth, target)
            sampling_source = fallback_pixels(source, depth, sampling_depth)
            reduced = bilinear_eighth(sampling_source, sampling_depth)
            converted = [[round(component * ((1 << sampling_depth) - 1)) for component in convert(
                [value * (257 if depth == 8 else 1) for value in pixel[:3]])]
                + [pixel[3] * (257 if depth == 8 and sampling_depth == 16 else 1)] for pixel in source]
            (directory / f'linked-reference-{depth}-to-{target}.json').write_text(json.dumps({
                'width': WIDTH, 'height': HEIGHT, 'source_depth': depth, 'document_depth': target,
                'source': source, 'fallback': fallback_pixels(source, depth, target),
                'changed_source': linked_pixels(depth, 1),
                'changed_fallback': fallback_pixels(linked_pixels(depth, 1), depth, target),
                'small_source': linked_pixels(depth, 2, 48, 32),
                'small_fallback': fallback_pixels(linked_pixels(depth, 2, 48, 32), depth, target),
                'reduced': fallback_pixels(reduced, sampling_depth, target),
                'converted_fallback': fallback_pixels(converted, sampling_depth, target),
                'converted_rgb_error_codes': 2 if target == 16 else 1,
                'identity_error_codes': 0, 'reduced_rgb_error_codes': 2 if target == 16 else 1,
                'reduced_alpha_error_codes': 1,
                'policy': 'Same original/document Display P3 ICC. Preserve actual master depth. '
                          'Typed masters or destinations use typed interpolation before final '
                          'fallback quantization; u8 promotion is exact×257, u16 reduction is '
                          'per-component nearest with undithered alpha.'}, separators=(',', ':')))


def decode_png(data):
    """Decode RGBA8/16 PNG with stdlib zlib and independent byte predictors."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    position, chunks = 8, {}
    while position < len(data):
        length = struct.unpack_from('>I', data, position)[0]
        kind = data[position + 4:position + 8]
        body = data[position + 8:position + 8 + length]
        assert len(body) == length
        crc = struct.unpack_from('>I', data, position + 8 + length)[0]
        assert zlib.crc32(kind + body) == crc
        chunks.setdefault(kind, []).append(body)
        position += 12 + length
    assert position == len(data) and len(chunks[b'IHDR']) == 1 and b'IEND' in chunks
    width, height, depth, color, compression, filtering, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'][0])
    assert depth in (8, 16) and (color, compression, filtering, interlace) == (6, 0, 0, 0)
    unit, stride = depth // 8 * 4, width * depth // 8 * 4
    encoded = zlib.decompress(b''.join(chunks[b'IDAT']))
    assert len(encoded) == height * (stride + 1)
    previous, decoded = bytearray(stride), bytearray()
    for y in range(height):
        at = y * (stride + 1)
        mode, row = encoded[at], bytearray(encoded[at + 1:at + 1 + stride])
        assert mode in range(5)
        for x in range(stride):
            left = row[x - unit] if x >= unit else 0
            above, corner = previous[x], previous[x - unit] if x >= unit else 0
            estimate = left + above - corner
            distances = [abs(estimate - value) for value in (left, above, corner)]
            paeth = (left, above, corner)[distances.index(min(distances))]
            row[x] = (row[x] + (0, left, above, (left + above) // 2, paeth)[mode]) & 255
        decoded.extend(row)
        previous = row
    return {'width': width, 'height': height, 'depth': depth,
            'pixels': list(struct.iter_unpack('>4H' if depth == 16 else '4B', decoded)),
            'chunks': chunks}


def read_native_sources(path):
    """Read documented RSRC v14–17 source records, without app/core readers."""
    data = path.read_bytes()
    assert data[:4] in (b'RZDC', b'DRDC')
    # The layer-identity wrapper (45 byte / 46 typed, `identity_native.rs`) is
    # the outermost version word; the source records' own version follows it
    # and any 56/57, 55 or 51/52 word, and its tables (RSPJ, LOPN, RSPR, then
    # LIDS) close the file after every earlier tail. Set them aside, checked.
    start, end, refusals, projective = identity_tables(data)
    version = struct.unpack_from('<I', data, start)[0]
    data = data[:end]
    assert version in (14, 15, 16, 17), version
    advanced = version >= 16
    offsets = [i for i in range(len(data) - 3) if data[i:i + 4] == b'RSRC']
    assert len(offsets) == 1, 'Controlled fixture must have one unambiguous source section'
    at = offsets[0] + 4

    def number():
        nonlocal at
        value = struct.unpack_from('<I', data, at)[0]
        at += 4
        return value

    def slot():
        nonlocal at
        size = number()
        value = data[at:at + size]
        assert len(value) == size
        at += size
        return value

    assets = []
    count = number()
    assert 0 < count < 1024
    for _ in range(count):
        identity = slot().decode('ascii')
        revision = slot().decode('ascii') if advanced else identity
        if advanced:
            depth = data[at]
            assert depth in (8, 16) and data[at + 1:at + 4] == b'\0\0\0'
            at += 4
        else:
            depth = 8 if version == 14 else 16
        profile = slot()
        width, height, size = struct.unpack_from('<IIQ', data, at)
        at += 16
        png = decode_png(data[at:at + size])
        at += size
        assert (png['width'], png['height'], png['depth']) == (width, height, depth)
        assets.append({'source_id': identity, 'revision_id': revision, 'depth': depth,
                       'width': width, 'height': height, 'profile': profile,
                       'pixels': png['pixels']})
    bindings = []
    count = number()
    assert 0 < count < 1024
    for _ in range(count):
        layer, asset = struct.unpack_from('<II', data, at)
        assert data[at + 9:at + 12] == b'\0\0\0' and asset < len(assets)
        filter_value = data[at + 8]
        transform = list(struct.unpack_from('<6d', data, at + 12))
        at += 60
        link = None
        if advanced:
            present = data[at]
            at += 1
            assert present in (0, 1)
            if present:
                link_path = slot().decode('utf8')
                fingerprint = data[at:at + 64].decode('ascii')
                at += 64
                assert link_path.startswith('/') and len(fingerprint) == 64
                assert all(c in '0123456789abcdef' for c in fingerprint)
                link = {'path': link_path, 'sha256': fingerprint}
        bindings.append({'layer': layer, 'asset': asset, 'filter': filter_value,
                         'transform': transform, 'link': link})
    assert at == len(data), 'Unexpected trailing source-section bytes'
    assert all(index < len(assets) for index in refusals), refusals
    # A projective layer is a binding whose legacy affine is the identity.
    legacy = {binding['layer']: binding['transform'] for binding in bindings}
    assert all(legacy.get(layer) == [1, 0, 0, 1, 0, 0] for layer in projective), projective
    return {'version': version, 'assets': assets, 'bindings': bindings, 'profile_refusals': refusals,
            'projective': projective}


def verify_linked_master(path, layer, source_depth, variant, profile, width=WIDTH, height=HEIGHT,
                         linked=None, source_id=None, revision_id=None):
    native = read_native_sources(path)
    binding = next(binding for binding in native['bindings'] if binding['layer'] == layer)
    asset = native['assets'][binding['asset']]
    assert (asset['width'], asset['height'], asset['depth']) == (width, height, source_depth)
    assert asset['pixels'] == linked_pixels(source_depth, variant, width, height)
    assert asset['profile'] == profile
    if linked is not None:
        assert (binding['link'] is not None) == linked
    if source_id is not None:
        assert asset['source_id'] == source_id
    if revision_id is not None:
        assert asset['revision_id'] == revision_id
    return {'native_version': native['version'], 'master_depth': asset['depth'],
            'master_profile_sha256': hashlib.sha256(asset['profile']).hexdigest(),
            'source_id': asset['source_id'], 'revision_id': asset['revision_id'],
            'linked': binding['link'] is not None, 'master_exact': True}


def verify_linked(directory, profile):
    report = json.loads((directory / 'linked-source-report.json').read_text())
    assert report['passed'] is True and len(report['cases']) == 4
    assert {(case['source_depth'], case['document_depth']) for case in report['cases']} == {
        (8, 8), (8, 16), (16, 8), (16, 16)}
    masters, fallbacks = [], []
    for case in report['cases']:
        assert case['passed'] is True
        for record in case['masters']:
            proof = verify_linked_master(Path(record['path']), layer=record['layer'],
                source_depth=record['source_depth'], variant=record['variant'], profile=profile,
                width=record['width'], height=record['height'], linked=record['linked'],
                source_id=record['source_id'], revision_id=record['revision_id'])
            masters.append({'path': record['path'], 'stage': record['stage'], **proof})
        for record in case['fallbacks']:
            if record.get('converted'):
                reference = json.loads((directory / f'linked-reference-{record["source_depth"]}-to-{record["document_depth"]}.json').read_text())
                expected, tolerance = reference['converted_fallback'], reference['converted_rgb_error_codes']
            else:
                expected = fallback_pixels(linked_pixels(record['source_depth'], record['variant'],
                    record['width'], record['height']), record['source_depth'], record['document_depth'])
                tolerance = 0
            actual = list(struct.iter_unpack('<4H' if record['document_depth'] == 16 else '4B',
                                            Path(record['path']).read_bytes()))
            assert len(actual) == len(expected)
            for got, wanted in zip(actual, expected):
                assert got[3] == wanted[3] and max(abs(a-b) for a,b in zip(got[:3], wanted[:3])) <= tolerance, record
            fallbacks.append({**record, 'sample_count': len(actual), 'rgb_error_codes': tolerance, 'alpha_exact': True})
    assert len(masters) == 28 and len(fallbacks) == 16
    (directory / 'linked-independent-report.json').write_text(json.dumps({
        'passed': True, 'masters': masters, 'fallbacks': fallbacks}, indent=2))
    print('PASS: independent native/package/recovery assets preserve original8/16-bit samples, ICC, links and revisions')
    print('PASS: independent byte↔u16 fallback/ICC conversion preserves original precision, component rounding and alpha for all four depth combinations')
