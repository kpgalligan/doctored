"""Independent DRDC21/22 source framing and RZLM1 local coverage decoding."""
from array import array
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('phase8g_host_native', ROOT / 'scripts/host-source-probe/native.py')
host = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host)
png_samples = host.png_samples


def _legacy_body(data):
    # Identity45/46 (and 51/52 projective) wrap the exact 8G21/22 body; the
    # shared host peel validates both ends of the LIDS/RSPJ tables.
    body, offset, _, _, projective = host.identity_body(data, inner=host.INNER_VERSIONS)
    version = struct.unpack_from('<I', body, 4)[0]
    assert version in (21, 22), version
    return body, offset, projective


def read_native(path, *, pixels=True, verify_hashes=True):
    return host.read_native(path, pixels=pixels, verify_hashes=verify_hashes)


def fallback_pngs(path):
    return host.fallback_pngs(path)


def independent_mask_digests(path):
    """Read raw independent layer-mask payloads from the validated native prefix."""
    body, _, _ = _legacy_body(Path(path).read_bytes())
    reader = host.Reader(body, 0)
    assert reader.take(4) in (b'RZDC', b'DRDC')
    version = reader.number()
    assert version in (21, 22)
    found = []

    def flag():
        value = reader.number('B')
        assert value in (0, 1)
        return value

    def optional():
        return reader.slot() if flag() else None

    if version == 21:
        reader.number(); reader.number()
        count = reader.number()
        reader.take(8)
        for _ in range(count):
            reader.slot(); reader.take(16); flag(); reader.slot()
            present = flag(); flag()
            if present:
                size, width, height = reader.number(), reader.number(), reader.number()
                assert size == width * height
                found.append(hashlib.sha256(reader.take(size)).hexdigest())
            optional(); flag(); optional(); reader.take(10); flag(); flag()
            if present:
                reader.take(8); flag(); reader.take(8)
    else:
        assert (reader.number(), reader.number()) == (1, 1)
        reader.number(); reader.number()
        roots, declared = reader.number(), reader.number()
        assert roots == declared
        reader.slot(); reader.take(8)
        for _ in range(3):
            optional()
        for _ in range(roots):
            assert reader.number('B') == 0
            flag(); assert reader.number('H') == 0
            reader.take(4); reader.slot(); reader.take(8)
            if flag():
                flag(); flag(); assert reader.number('H') == 0
                reader.take(8)
                width, height = reader.number(), reader.number()
                reader.take(8)
                length = reader.number('Q')
                assert length == width * height * 2
                found.append(hashlib.sha256(reader.take(length)).hexdigest())
            reader.take(8); reader.slot('Q')
    return found


def mutate_attachment(source, destination, mutate, *, asset_index=0):
    parsed = read_native(source, pixels=False)
    attachment = parsed['assets'][asset_index]['attachment']
    assert attachment is not None
    descriptor = attachment['descriptor']
    mutate(descriptor)
    value = json.dumps(descriptor, ensure_ascii=False,
                       separators=(',', ':'), sort_keys=True).encode()
    low, high = attachment['descriptor_range']
    data = Path(source).read_bytes()
    Path(destination).write_bytes(data[:low] + struct.pack('<I', len(value)) + value + data[high:])


def read_mask(data):
    assert data[:4] == b'RZLM' and len(data) >= 16
    version, width, height = struct.unpack_from('<III', data, 4)
    assert version == 1 and 0 < width * height <= 64_000_000
    assert len(data) == 16 + width * height * 2
    values = array('H')
    values.frombytes(data[16:])
    if sys.byteorder != 'little':
        values.byteswap()
    return dict(width=width, height=height, samples=values,
                sha256=hashlib.sha256(data).hexdigest())


def local_asset(path, layer=0, recipe_versions=(3,)):
    parsed = read_native(path)
    matches = [binding for binding in parsed['bindings'] if binding['layer'] == layer]
    assert len(matches) == 1
    asset = parsed['assets'][matches[0]['asset']]
    assert asset['depth'] == 16 and asset['raw'] is not None
    descriptor = asset['raw']['descriptor']
    recipe = descriptor['recipe']
    assert recipe['version'] in recipe_versions
    nodes = recipe['enhancements']['locals']
    attachment = asset['attachment']
    assert attachment['descriptor']['kind'] == 'raw_local_masks'
    assert attachment['descriptor']['state'] == dict(schema_version=1,
        coordinates='oriented-source-pixel-centers-v1', width=asset['width'], height=asset['height'],
        nodes=[node['id'] for node in nodes])
    masks = [read_mask(parsed['resources'][index]) for index in attachment['resources']]
    assert len(masks) == len(nodes) <= 4
    assert all((mask['width'], mask['height']) == (asset['width'], asset['height']) for mask in masks)
    for node, resource, mask in zip(nodes, attachment['descriptor']['resources'], masks, strict=True):
        assert resource['id'] == node['id'] and resource['sha256'] == mask['sha256']
    return parsed, asset, masks
