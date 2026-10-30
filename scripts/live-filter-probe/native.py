"""Independent length-framed DRDC source/filter reader; never calls the product.

The old source formats remain readable so removing the final filter can resume
an older compatible writer. New23/24 framing extends the documented21/22 pool.
The 12E-1 identity word 45/46 and its trailing LIDS table are peeled first by
`_identity_body`, a thin wrapper over raw-probe's shared `identity_body`, and
every recorded position is shifted back into the file.
"""
from array import array
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('phase9a_png_reader', ROOT / 'scripts/raw-probe/native.py')
png_module = importlib.util.module_from_spec(spec); spec.loader.exec_module(png_module)
png_samples = png_module.png_samples


class Reader:
    def __init__(self, data):
        self.data, self.at = data, 0

    def take(self, size):
        assert 0 <= size <= len(self.data) - self.at, (self.at, size, len(self.data))
        value = self.data[self.at:self.at + size]
        self.at += size
        return value

    def number(self, code='I'):
        return struct.unpack('<' + code, self.take(struct.calcsize(code)))[0]

    def flag(self):
        value = self.number('B')
        assert value in (0, 1)
        return value

    def slot(self, code='I'):
        return self.take(self.number(code))

    def optional(self):
        return self.slot() if self.flag() else None

    def descriptor(self):
        start = self.at
        value = json.loads(self.slot())
        return value, (start, self.at)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def prefix(reader):
    r = reader
    assert r.take(4) in (b'RZDC', b'DRDC')
    header_version = r.number()
    # Outermost typed wrappers: 36 (layer styles, LSTY), then 35 (Blend If,
    # LBIF), then 32 (clipping, LCLP).
    style_version = header_version if header_version == 36 else None
    blend_if_word = r.number() if style_version else header_version
    if style_version:
        assert blend_if_word in (13, 15, 17, 19, 20, 22, 24, 25, 26, 27, 29, 30, 31, 32, 33, 34, 35, 37)
    blend_if_version = blend_if_word if blend_if_word == 35 else None
    clip_word = r.number() if blend_if_version else blend_if_word
    if blend_if_version:
        assert clip_word in (13, 15, 17, 19, 20, 22, 24, 25, 26, 27, 29, 30, 31, 32, 33, 34, 37)
    clipping_version = clip_word if clip_word == 32 else None
    mask_word = r.number() if clipping_version else clip_word
    if clipping_version:
        assert mask_word in (13, 15, 17, 19, 20, 22, 24, 25, 26, 27, 29, 30, 31, 33, 34, 37)
    mask_version = mask_word if mask_word in (28, 29) else None
    blend_version = r.number() if mask_version else mask_word
    if mask_version == 28:
        assert blend_version in (10, 14, 16, 18, 21, 23)
    elif mask_version == 29:
        assert blend_version in (13, 15, 17, 19, 20, 22, 24, 25, 26, 27, 30, 31, 33, 34, 37)
    inner_version = r.number() if blend_version in (26, 27, 30, 31, 33, 34, 37) else blend_version
    if blend_version in (26, 27, 30, 31, 33, 34, 37):
        assert inner_version in (13, 15, 17, 19, 20, 22, 24, 25)
    version = r.number() if inner_version == 25 else inner_version
    if inner_version == 25:
        assert version in (13, 15, 17, 19, 20, 22, 24)
    byte = version in (10, 14, 16, 18, 21, 23)
    assert byte or version in (13, 15, 17, 19, 20, 22, 24), version
    fallbacks, layers, channels = {}, {}, []
    if byte:
        width, height, count = r.number(), r.number(), r.number()
        r.take(8)  # Global light.
        for layer in range(count):
            name = r.slot().decode()
            x, y, opacity, blend = struct.unpack('<iifI', r.take(16))
            visible = r.flag()
            fallbacks[layer] = r.slot()
            mask, mask_enabled = r.flag(), r.flag()
            coverage = None
            if mask:
                size, mw, mh = r.number(), r.number(), r.number()
                assert size == mw * mh
                coverage = dict(width=mw, height=mh, data=r.take(size), enabled=mask_enabled)
            meta = r.optional()
            clipped, style = r.flag(), r.optional()
            locks, linked, tree_depth = struct.unpack('<IIH', r.take(10))
            kind, expanded = r.flag(), r.flag()
            if mask:
                mx, my = struct.unpack('<ii', r.take(8))
                mask_linked = r.flag()
                density, feather = struct.unpack('<ff', r.take(8))
                coverage.update(x=mx, y=my, linked=mask_linked, density=density, feather=feather)
            layers[layer] = dict(name=name, x=x, y=y, opacity=opacity, blend=blend, visible=visible,
                mask=coverage, meta=meta, clipped=clipped, style=style, locks=locks, linked=linked,
                tree_depth=tree_depth, kind=kind, expanded=expanded)
        for _ in range(r.number()):
            name = r.slot().decode()
            color, opacity = r.take(3), r.number('f')
            indicates_selected = r.flag()
            channels.append(dict(name=name, color=color, opacity=opacity,
                                 indicates_selected=indicates_selected, png=r.slot()))
        resolution = (r.number('f'), r.number('f'))
        profile = r.optional()
        for _ in range(3):
            r.optional()
        r.take(16)
        for _ in range(r.number()):
            r.flag(); r.take(8)
    else:
        assert (r.number(), r.number()) == (1, 1)
        width, height, roots, declared = r.number(), r.number(), r.number(), r.number()
        profile = r.slot(); resolution = (r.number('f'), r.number('f'))
        for _ in range(3):
            r.optional()
        records = 0
        index = 0

        def walk(count, depth=0):
            nonlocal records, index
            assert depth <= 64 and count <= declared - records
            for _ in range(count):
                records += 1
                kind, visible = r.number('B'), r.flag()
                assert kind in (0, 1, 2, 3) and r.number('H') == 0
                opacity = r.number('f'); name = r.slot().decode()
                x, y = struct.unpack('<ii', r.take(8))
                mask = None
                if r.flag():
                    enabled, linked = r.flag(), r.flag()
                    assert r.number('H') == 0
                    mx, my, mw, mh = struct.unpack('<iiII', r.take(16))
                    density, feather = struct.unpack('<ff', r.take(8))
                    assert r.number('Q') == mw * mh * 2
                    mask = dict(enabled=enabled, linked=linked, x=mx, y=my, width=mw, height=mh,
                        density=density, feather=feather, data=r.take(mw * mh * 2))
                values = dict(kind=kind, visible=visible, opacity=opacity, name=name, x=x, y=y, mask=mask,
                              blend=27 if kind == 2 else 0, clipped=False, blend_if=None, style=None)
                encoded = None
                if kind == 0:
                    values['width'], values['height'] = r.number(), r.number()
                    encoded = r.slot('Q')
                elif kind in (1, 2):
                    walk(r.number(), depth + 1)
                else:
                    values['adjustment'] = r.slot().decode()
                layers[index] = values
                if encoded is not None:
                    fallbacks[index] = encoded
                index += 1
        walk(roots)
        assert records == declared
        for _ in range(r.number()):
            name = r.slot().decode()
            assert r.number('Q') == width * height * 2
            channels.append(dict(name=name, data=r.take(width * height * 2)))
    source_version = version
    if version == 20:
        source_version = r.number()
        assert source_version in (13, 15, 17, 19)
    if version in (21, 22, 23, 24):
        assert r.number() == (16 if byte else 17)
    for layer in layers.values():
        if layer['mask'] is not None:
            layer['mask']['outside_coverage'] = 0
    return dict(version=header_version, style_version=style_version, blend_if_version=blend_if_version, clipping_version=clipping_version,
                mask_version=mask_version,
                blend_version=blend_version if blend_version in (26, 27, 30, 31, 33, 34, 37) else None,
                has_channel_metadata=inner_version == 25, body_version=version, source_version=source_version, depth=8 if byte else 16,
                width=width, height=height, profile=profile, resolution=resolution,
                layers=layers, fallbacks=fallbacks,
                channels=channels)



def read_layer_blending(reader, layers, version=26):
    """Independent LBLD schema1/2/3/4/5/6/7; version26 remains limited to Multiply/Screen.

    Schema7 (version37) adds no mode: it is schema6's twenty-six, now also
    admitted on a kind-3 adjustment record. Every older schema still refuses one.
    """
    schema6 = (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22,23,24,25,26,14,16,17,21)
    schemas = {26: (1, (1,2)), 27: (2, (1,2,3,4,5,6,7,8,9)),
               30: (3, (1,2,3,4,5,6,7,8,9,10,11,12,15)),
               31: (4, (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22)),
               33: (5, (1,2,3,4,5,6,7,8,9,10,11,12,13,15,18,19,20,22,23,24,25,26)),
               34: (6, schema6),
               37: (7, schema6)}
    assert version in schemas
    schema, allowed = schemas[version]
    assert reader.take(4) == b'LBLD' and reader.number() == schema
    count = reader.number()
    assert 0 < count <= len(layers) <= 1024
    kinds = (0, 1, 3) if version == 37 else (0, 1)
    records, previous = [], -1
    for _ in range(count):
        index, mode = reader.number(), reader.number()
        assert index in layers and previous < index
        assert mode in allowed
        assert layers[index]['kind'] in kinds
        previous = index
        records.append((index, mode))
    # Do not publish a partial property table if a later entry refuses.
    for index, mode in records:
        layers[index]['blend'] = mode
    return records

def read_mask_outside(reader, layers):
    """MOUT schema1 stores only indexes whose masks have raw outside255."""
    assert reader.take(4) == b'MOUT' and reader.number() == 1
    count = reader.number()
    assert 0 < count <= len(layers) <= 1024
    indexes, previous = [], -1
    for _ in range(count):
        index = reader.number()
        assert index in layers and previous < index and layers[index]['mask'] is not None
        indexes.append(index)
        previous = index
    # Refused metadata never partially changes decoded mask properties.
    for index in indexes:
        layers[index]['mask']['outside_coverage'] = 255
    return indexes

def read_clipping(reader, layers):
    """LCLP schema1: strictly ascending postorder indexes of clipping members (any kind)."""
    assert reader.take(4) == b'LCLP' and reader.number() == 1
    count = reader.number()
    assert 0 < count <= len(layers) <= 1024
    indexes, previous = [], -1
    for _ in range(count):
        index = reader.number()
        assert index in layers and previous < index
        indexes.append(index)
        previous = index
    # Refused metadata never partially changes decoded properties.
    for index in indexes:
        layers[index]['clipped'] = True
    return indexes

BLEND_IF_CHANNELS = ('gray', 'red', 'green', 'blue')

def read_blend_if(reader, layers):
    """LBIF schema1: ascending (index, channel, this[4], underlying[4]) on raster/isolated-group records."""
    assert reader.take(4) == b'LBIF' and reader.number() == 1
    count = reader.number()
    assert 0 < count <= len(layers) <= 1024
    records, previous = [], -1
    for _ in range(count):
        index, channel = reader.number(), reader.number()
        assert index in layers and previous < index and channel < 4
        assert layers[index]['kind'] in (0, 1)
        this, under = list(reader.take(4)), list(reader.take(4))
        for ramp in (this, under):
            assert ramp == sorted(ramp)
        assert not (this == [0, 0, 255, 255] and under == [0, 0, 255, 255])
        previous = index
        records.append((index, dict(channel=BLEND_IF_CHANNELS[channel], this_layer=this, underlying=under)))
    for index, value in records:
        layers[index]['blend_if'] = value
    return records

STYLE_TYPES = ('drop_shadow', 'outer_glow', 'stroke', 'gradient_overlay', 'color_overlay', 'satin',
               'inner_glow', 'inner_shadow', 'bevel_emboss')
MAX_STYLE_JSON_LEN = 65536

def read_layer_style(reader, layers):
    """LSTY schema1: ascending (index, canonical style JSON) on raster/isolated-group records; blend_if null, non-identity."""
    assert reader.take(4) == b'LSTY' and reader.number() == 1
    count = reader.number()
    assert 0 < count <= len(layers) <= 1024
    records, previous = [], -1
    for _ in range(count):
        index, length = reader.number(), reader.number()
        assert index in layers and previous < index
        assert 0 < length <= MAX_STYLE_JSON_LEN
        body = reader.take(length)
        style = json.loads(body.decode())
        assert isinstance(style, dict) and set(style) == {'version', 'fill_opacity', 'blend_if', 'effects'}
        assert style['version'] == 1 and style['blend_if'] is None
        assert isinstance(style['fill_opacity'], (int, float)) and 0 <= style['fill_opacity'] <= 1
        assert isinstance(style['effects'], list)
        types = [effect.get('type') for effect in style['effects']]
        assert all(t in STYLE_TYPES for t in types) and len(set(types)) == len(types)
        assert style['fill_opacity'] < 1 or any(effect.get('enabled', True) for effect in style['effects'])
        assert layers[index]['kind'] in (0, 1)
        # Canonical bytes: sorted keys, the writer's own separators.
        assert json.dumps(style, sort_keys=True, separators=(',', ':')) == body.decode()
        previous = index
        records.append((index, style))
    for index, style in records:
        layers[index]['style'] = style
    return records

def _identity_body(data):
    """(body, offset, layer ids, identity word) via raw-probe's shared
    `identity_body`; a projective (51/52) table is never expected here."""
    body, offset, ids, word, projective = png_module.identity_body(data)
    assert not projective
    return body, offset, ids, word


def read_native(path, *, pixels=True, verify_hashes=True):
    data = Path(path).read_bytes()
    body, offset, layer_ids, identity_version = _identity_body(data)
    r = Reader(body)
    document = prefix(r)
    version, source_version = document['body_version'], document['source_version']
    pooled = version in (21, 22, 23, 24)
    raw_version = source_version in (18, 19)
    advanced = pooled or source_version in (16, 17, 18, 19)
    assets, bindings, resources, spans = [], [], [], []
    if source_version not in (10, 13):
        assert r.take(4) == b'RSRC'
        for _ in range(r.number()):
            identity = r.slot().decode()
            revision = r.slot().decode() if advanced else identity
            depth = r.number('B') if advanced else document['depth']
            assert depth in (8, 16)
            if advanced:
                assert r.take(3) == bytes(3)
            profile = r.slot(); width, height = r.number(), r.number()
            encoded = r.slot('Q')
            asset = dict(source_id=identity, revision_id=revision, depth=depth, width=width, height=height,
                profile=profile, png_sha256=digest(encoded), raw=None, attachment=None)
            if pixels:
                decoded = png_samples(encoded)
                assert (decoded['width'], decoded['height'], decoded['depth']) == (width, height, depth)
                asset['samples'] = decoded['samples']
            if raw_version and r.flag():
                reference = r.number(); descriptor, span = r.descriptor()
                asset['raw'] = dict(resource=reference, descriptor=descriptor, descriptor_range=span)
            assets.append(asset)
        for _ in range(r.number()):
            layer, asset, sampler = r.number(), r.number(), r.number('B')
            assert r.take(3) == bytes(3) and asset < len(assets) and layer in document['layers']
            transform = list(struct.unpack('<6d', r.take(48)))
            link = dict(path=r.slot().decode(), sha256=r.take(64).decode()) if advanced and r.flag() else None
            bindings.append(dict(layer=layer, asset=asset, sampler=sampler, transform=transform, link=link, live_filter=None))
    used = set()
    if raw_version:
        assert r.take(4) == b'RRAW'
        for _ in range(r.number()):
            size = r.number('Q'); start = r.at
            resources.append(r.take(size)); spans.append((start, r.at))
    if pooled:
        assert r.take(4) == b'ATCH' and r.number() == len(assets)
        for asset in assets:
            if r.flag():
                reference = r.number(); descriptor, span = r.descriptor()
                asset['raw'] = dict(resource=reference, descriptor=descriptor, descriptor_range=span)
            if r.flag():
                descriptor, span = r.descriptor()
                refs = [r.number() for _ in range(r.number())]
                assert len(refs) == len(descriptor['resources'])
                asset['attachment'] = dict(descriptor=descriptor, descriptor_range=span, resources=refs)
                used.update(refs)
        assert r.take(4) == b'RBIN'
        for _ in range(r.number()):
            size = r.number('Q'); start = r.at
            resources.append(r.take(size)); spans.append((start, r.at))
    if version in (23, 24):
        assert r.take(4) == b'LFLT' and r.number() == len(bindings)
        for binding in bindings:
            if r.flag():
                descriptor, span = r.descriptor()
                count = r.number(); assert count <= 16
                refs = [r.number() for _ in range(count)]
                used.update(index for index in refs if index != 0xffffffff)
                binding['live_filter'] = dict(descriptor=descriptor, descriptor_range=span, mask_refs=refs)
    artwork = []
    if version in (20, 22, 24):
        assert r.take(4) == b'ADSC'
        for _ in range(r.number()):
            artwork.append(dict(layer=r.number(), description=json.loads(r.slot())))
    if document['has_channel_metadata']:
        assert r.take(4) == b'CHPR' and r.number() == len(document['channels'])
        for channel in document['channels']:
            channel.update(color=r.take(3), opacity=r.number('f'), indicates_selected=r.flag())
            text = r.slot()
            channel['provenance'] = json.loads(text) if text else None
    if document['blend_version']:
        document['layer_blend_records'] = read_layer_blending(r, document['layers'], document['blend_version'])
    if document['mask_version']:
        document['mask_outside_records'] = read_mask_outside(r, document['layers'])
    if document['clipping_version']:
        document['clipping_records'] = read_clipping(r, document['layers'])
    if document['blend_if_version']:
        document['blend_if_records'] = read_blend_if(r, document['layers'])
    if document['style_version']:
        document['layer_style_records'] = read_layer_style(r, document['layers'])
    assert r.at == len(r.data), (r.at, len(r.data))
    for asset in assets:
        if asset['raw']:
            raw = asset['raw']; used.add(raw['resource'])
            if verify_hashes:
                assert digest(resources[raw['resource']]) == raw['descriptor']['content_sha256']
        if asset['attachment']:
            attachment = asset['attachment']
            for entry, index in zip(attachment['descriptor']['resources'], attachment['resources'], strict=True):
                assert len(resources[index]) == entry['byte_length']
                if verify_hashes:
                    assert digest(resources[index]) == entry['sha256']
    assert used == set(range(len(resources))), (used, len(resources))
    if layer_ids is not None:
        assert len(layer_ids) == len(document['layers'])
        # Every recorded position is a BODY offset; shift it into the file.
        for binding in bindings:
            if binding['live_filter']:
                low, high = binding['live_filter']['descriptor_range']
                binding['live_filter']['descriptor_range'] = (low + offset, high + offset)
        for asset in assets:
            if asset['raw']:
                low, high = asset['raw']['descriptor_range']
                asset['raw']['descriptor_range'] = (low + offset, high + offset)
            if asset['attachment']:
                low, high = asset['attachment']['descriptor_range']
                asset['attachment']['descriptor_range'] = (low + offset, high + offset)
        spans = [(low + offset, high + offset) for low, high in spans]
    return dict(version=document['version'], document=document, assets=assets, bindings=bindings,
                resources=resources, resource_ranges=spans, artwork=artwork,
                identity_version=identity_version, layer_ids=layer_ids)


def read_mask(data):
    assert data[:4] == b'RZLM'
    version, width, height = struct.unpack_from('<III', data, 4)
    assert version == 1 and len(data) == 16 + width * height * 2
    samples = array('H'); samples.frombytes(data[16:])
    if sys.byteorder != 'little':
        samples.byteswap()
    return dict(width=width, height=height, samples=samples, sha256=digest(data))


def filter_binding(parsed, layer=0):
    matches = [binding for binding in parsed['bindings'] if binding['layer'] == layer]
    assert len(matches) == 1 and matches[0]['live_filter'] is not None
    return matches[0]


def mutate_filter(source, destination, mutate, layer=0):
    parsed = read_native(source, pixels=False)
    state = filter_binding(parsed, layer)['live_filter']
    descriptor = state['descriptor']; mutate(descriptor)
    encoded = json.dumps(descriptor, sort_keys=True, separators=(',', ':')).encode()
    start, end = state['descriptor_range']
    data = Path(source).read_bytes()
    Path(destination).write_bytes(data[:start] + struct.pack('<I', len(encoded)) + encoded + data[end:])
