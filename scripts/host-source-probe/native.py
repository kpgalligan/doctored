"""Independent Phase 8F native source/resource reader; no product loader calls."""

import hashlib
import importlib.util
import json
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location('raw_probe_native', ROOT / 'scripts/raw-probe/native.py')
_raw = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_raw)
png_samples = _raw.png_samples
identity_body = _raw.identity_body
# The 12E-1 identity wrapper word -> the DRDC body version it may wrap.
INNER_VERSIONS = {45: (21,), 46: (22,)}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_boundary(data):
    body, shift, _, _, _ = identity_body(data, inner=INNER_VERSIONS)
    prefix = read_prefix(body)
    return prefix['version'], prefix['base_version'], prefix['source_boundary'] + shift


def fallback_pngs(path):
    """Return length-framed raster fallback PNGs, without searching pixel bytes."""
    body, _, _, _, _ = identity_body(Path(path).read_bytes(), inner=INNER_VERSIONS)
    return list(read_prefix(body)['fallbacks'].values())


class Reader:
    def __init__(self, data, at):
        self.data, self.at = data, at

    def take(self, size):
        assert 0 <= size <= len(self.data) - self.at, (self.at, size, len(self.data))
        value = self.data[self.at:self.at + size]
        self.at += size
        return value

    def number(self, code='I'):
        return struct.unpack('<' + code, self.take(struct.calcsize(code)))[0]

    def slot(self, code='I'):
        return self.take(self.number(code))

    def descriptor(self):
        start = self.at
        value = self.slot()
        return json.loads(value), (start, self.at)


def read_prefix(data):
    """Parse the documented byte/typed record framing up to its source section.

    No magic-marker search is used: PNG bodies, masks, profiles and metadata are
    skipped by their container lengths, so arbitrary embedded byte sequences
    cannot be mistaken for the source boundary or another raster PNG.
    """
    r = Reader(data, 0)
    assert r.take(4) in (b'RZDC', b'DRDC')
    version = r.number()
    assert version in (21, 22), version
    fallbacks = {}

    def flag():
        value = r.number('B')
        assert value in (0, 1)
        return value

    def optional():
        return r.slot() if flag() else None

    if version == 21:
        width, height, count = r.number(), r.number(), r.number()
        r.take(8)  # Global light: two f32 values.
        for layer in range(count):
            r.slot().decode('utf-8')
            r.take(16)  # x/y i32, opacity f32, blend u32.
            flag()
            fallbacks[layer] = r.slot()
            mask = flag()
            flag()  # Enabled is stored even when the mask is absent.
            if mask:
                size, mw, mh = r.number(), r.number(), r.number()
                assert size == mw * mh
                r.take(size)
            optional()  # Described byte content.
            flag()  # Clipped.
            optional()  # Layer style.
            r.take(10)  # Locks/link u32; tree depth u16.
            assert r.number('B') in (0, 1)
            flag()  # Group expanded.
            if mask:
                r.take(8); flag(); r.take(8)  # Offset, link, density/feather.
        for _ in range(r.number()):
            r.slot().decode('utf-8')
            r.take(7)  # RGB overlay and f32 opacity.
            flag()
            r.slot()  # Saved-channel grayscale PNG is not a raster fallback.
        r.take(8)  # Document PPI.
        profile = optional()
        for _ in range(3):
            optional()  # EXIF/XMP/IPTC.
        r.take(16)  # Ruler origin f64 pair.
        for _ in range(r.number()):
            assert r.number('B') in (0, 1)
            r.take(8)
    else:
        assert (r.number(), r.number()) == (1, 1)
        width, height, roots, declared = r.number(), r.number(), r.number(), r.number()
        profile = r.slot()
        r.take(8)
        for _ in range(3):
            optional()
        record_count, layer_index = 0, 0

        def records(count, depth=0):
            nonlocal record_count, layer_index
            assert depth <= 64 and count <= declared - record_count
            for _ in range(count):
                record_count += 1
                kind = r.number('B')
                assert kind in (0, 1, 2, 3)
                flag()
                assert r.number('H') == 0
                r.take(4); r.slot().decode('utf-8'); r.take(8)
                if flag():
                    flag(); flag()
                    assert r.number('H') == 0
                    r.take(8)
                    mw, mh = r.number(), r.number()
                    r.take(8)
                    assert r.number('Q') == mw * mh * 2
                    r.take(mw * mh * 2)
                if kind == 0:
                    r.take(8)
                    fallbacks[layer_index] = r.slot('Q')
                elif kind in (1, 2):
                    records(r.number(), depth + 1)
                else:
                    r.slot().decode('utf-8')
                layer_index += 1  # Public indices are postorder.
        records(roots)
        assert record_count == declared
        for _ in range(r.number()):
            r.slot().decode('utf-8')
            assert r.number('Q') == width * height * 2
            r.take(width * height * 2)
    boundary = r.at
    base_version = r.number()
    assert base_version == (16 if version == 21 else 17)
    assert r.take(4) == b'RSRC'
    return {'version': version, 'base_version': base_version, 'source_boundary': boundary,
            'width': width, 'height': height, 'profile': profile, 'fallbacks': fallbacks}


def read_native(path, *, pixels=True, verify_hashes=True):
    """Parse a native file; every returned byte range is in file coordinates."""
    body, shift, ids, word, projective = identity_body(Path(path).read_bytes(), inner=INNER_VERSIONS)
    prefix = read_prefix(body)
    result = _read_tail(body, prefix['version'], prefix['base_version'], prefix['source_boundary'],
                        pixels=pixels, verify_hashes=verify_hashes)
    for binding in result['bindings']:
        if binding['layer'] in projective:
            assert binding['transform'] == [1, 0, 0, 1, 0, 0]
            binding['transform_kind'] = 'projective'
            binding['projective_transform'] = projective[binding['layer']]
    prefix['source_boundary'] += shift
    result['document'] = prefix
    if shift:
        for asset in result['assets']:
            if asset['raw']:
                low, high = asset['raw']['descriptor_range']
                asset['raw']['descriptor_range'] = (low + shift, high + shift)
            if asset['attachment']:
                low, high = asset['attachment']['descriptor_range']
                asset['attachment']['descriptor_range'] = (low + shift, high + shift)
        result['resource_ranges'] = [(low + shift, high + shift) for low, high in result['resource_ranges']]
    result['identity_word'] = word
    result['layer_ids'] = ids
    return result


def _read_tail(data, version, base_version, boundary, *, pixels, verify_hashes):
    reader = Reader(data, boundary + 8)
    assets = []
    for _ in range(reader.number()):
        identity, revision = reader.slot().decode(), reader.slot().decode()
        depth = reader.number('B')
        assert depth in (8, 16) and reader.take(3) == bytes(3)
        profile = reader.slot()
        width, height = reader.number(), reader.number()
        png = reader.slot('Q')
        asset = {'source_id': identity, 'revision_id': revision, 'depth': depth,
                 'width': width, 'height': height, 'profile': profile,
                 'profile_sha256': digest(profile), 'png_sha256': digest(png)}
        if pixels:
            decoded = png_samples(png)
            assert (decoded['width'], decoded['height'], decoded['depth']) == (width, height, depth)
            asset['samples'] = decoded['samples']
        assets.append(asset)
    bindings = []
    for _ in range(reader.number()):
        layer, asset_index, sampler = reader.number(), reader.number(), reader.number('B')
        assert asset_index < len(assets) and reader.take(3) == bytes(3)
        transform = list(struct.unpack('<6d', reader.take(48)))
        linked = reader.number('B')
        assert linked in (0, 1)
        link = {'path': reader.slot().decode(), 'sha256': reader.take(64).decode()} if linked else None
        bindings.append({'layer': layer, 'asset': asset_index, 'sampler': sampler, 'transform': transform, 'link': link})
    assert reader.take(4) == b'ATCH'
    assert reader.number() == len(assets)
    used = set()
    for asset in assets:
        present = reader.number('B')
        assert present in (0, 1)
        asset['raw'] = None
        if present:
            reference = reader.number()
            descriptor, span = reader.descriptor()
            asset['raw'] = {'resource': reference, 'descriptor': descriptor, 'descriptor_range': span}
            used.add(reference)
        present = reader.number('B')
        assert present in (0, 1)
        asset['attachment'] = None
        if present:
            descriptor, span = reader.descriptor()
            refs = [reader.number() for _ in range(reader.number())]
            assert 1 <= len(refs) <= 4 and len(refs) == len(descriptor['resources'])
            asset['attachment'] = {'descriptor': descriptor, 'descriptor_range': span, 'resources': refs}
            used.update(refs)
    assert reader.take(4) == b'RBIN'
    resources, resource_ranges = [], []
    for _ in range(reader.number()):
        size = reader.number('Q')
        start = reader.at
        resources.append(reader.take(size))
        resource_ranges.append((start, reader.at))
    assert used == set(range(len(resources))), (used, len(resources))
    for asset in assets:
        if asset['raw']:
            raw = asset['raw']
            if verify_hashes:
                assert digest(resources[raw['resource']]) == raw['descriptor']['content_sha256']
        if asset['attachment']:
            attachment = asset['attachment']
            for record, reference in zip(attachment['descriptor']['resources'], attachment['resources']):
                encoded = resources[reference]
                assert len(encoded) == record['byte_length']
                if verify_hashes:
                    assert digest(encoded) == record['sha256']
    artwork = []
    if version == 22:
        assert reader.take(4) == b'ADSC'
        for _ in range(reader.number()):
            artwork.append({'layer': reader.number(), 'description': json.loads(reader.slot())})
    assert reader.at == len(data), (reader.at, len(data))
    return {'version': version, 'base_version': base_version, 'assets': assets, 'bindings': bindings,
            'resources': resources, 'resource_ranges': resource_ranges, 'artwork': artwork}


def mutate_attachment(source, destination, mutate, *, asset_index=0):
    parsed = read_native(source, pixels=False)
    attachment = parsed['assets'][asset_index]['attachment']
    assert attachment is not None
    descriptor = attachment['descriptor']
    mutate(descriptor)
    value = json.dumps(descriptor, ensure_ascii=False, separators=(',', ':'), sort_keys=True).encode()
    start, end = attachment['descriptor_range']
    data = Path(source).read_bytes()
    Path(destination).write_bytes(data[:start] + struct.pack('<I', len(value)) + value + data[end:])


def corrupt_resource_same_length(source, destination, *, resource_index=0):
    parsed = read_native(source, pixels=False)
    start, end = parsed['resource_ranges'][resource_index]
    assert end > start
    data = bytearray(Path(source).read_bytes())
    data[(start + end) // 2] ^= 0x40
    Path(destination).write_bytes(data)
