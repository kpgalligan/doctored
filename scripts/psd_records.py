#!/usr/bin/env python3
"""A bounded, lenient reader for the RECORD framing of a Photoshop-written PSD.

`scripts/psd_qualification_decode.py` is the strict decoder Doctored's PSD
export gates use; it asserts on constructs Photoshop writes and Doctored does
not (an `Lr16` tail block, a zero-length layer-information section, ZIP
channels, a mask body on every adjustment record), which is by design and is
why it is frozen. This module is its lenient twin for the OTHER direction, PSD
compatibility: it reads what a Photoshop-written file says about its records
and tagged blocks, decodes no pixels at all, and records rather than refuses.

Nothing here is derived from Doctored's importer: the framing comes from
Adobe's published "Photoshop File Formats" document, and every length is
checked against the buffer before it is used.

Both versions are read. A PSB (version 2, `large` below) differs from a PSD
in four widths, all Adobe's: the layer-and-mask section length and the
layer-information length are 8 bytes; a record's channel data length is 8
bytes; a tagged block whose key is in `LONG_LENGTH_KEYS` has an 8-byte length;
and an RLE plane's per-row byte counts are 4 bytes rather than 2. The last is
stated for completeness: this reader walks no row table, because it decodes no
pixels.
"""
import struct

MAX_RECORDS = 4096
MAX_BLOCKS = 4096
MAX_CHANNELS = 64
# Adobe's list of tagged blocks whose length field is 8 bytes rather than 4.
LONG_LENGTH_KEYS = {b'LMsk', b'Lr16', b'Lr32', b'Layr', b'Mt16', b'Mt32', b'Mtrn',
                    b'Alph', b'FMsk', b'lnk2', b'FEid', b'FXid', b'PxSD',
                    b'cinf', b'pattern data'}


class Reader:
    def __init__(self, data, at=0, end=None):
        self.data, self.at = data, at
        self.end = len(data) if end is None else end

    def take(self, count):
        assert count >= 0 and self.at + count <= self.end, 'truncated PSD'
        out = self.data[self.at:self.at + count]
        self.at += count
        return out

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack('>H', self.take(2))[0]

    def i16(self):
        return struct.unpack('>h', self.take(2))[0]

    def u32(self):
        return struct.unpack('>I', self.take(4))[0]

    def i32(self):
        return struct.unpack('>i', self.take(4))[0]

    def u64(self):
        return struct.unpack('>Q', self.take(8))[0]

    def remaining(self):
        return self.end - self.at


def _block_length(reader, signature, key, large):
    """A tagged block's length: 8 bytes for Adobe's long-length keys in a PSB
    (whatever its signature) or under an `8B64` signature, else 4."""
    if key in LONG_LENGTH_KEYS and (large or signature == b'8B64'):
        return reader.u64()
    return reader.u32()


def _records(reader, count, large=False):
    """`count` layer records, then their channel data, from one section."""
    records = []
    for _ in range(count):
        bounds = [reader.i32() for _ in range(4)]
        channels = reader.u16()
        assert channels <= MAX_CHANNELS, channels
        table = [(reader.i16(), reader.u64() if large else reader.u32())
                 for _ in range(channels)]
        assert reader.take(4) == b'8BIM', 'invalid layer blend signature'
        blend = reader.take(4)
        opacity, clipping, flags, _ = reader.take(4)
        extra_length = reader.u32()
        extra = Reader(reader.data, reader.at, reader.at + extra_length)
        reader.at = extra.end
        mask_length = extra.u32()
        # The body's ABSOLUTE offset in the file: `extra` walks the same
        # buffer, so a caller can patch the body in place without re-deriving
        # the record framing.
        mask_body_at = extra.at
        mask_body = extra.take(mask_length)
        ranges = extra.take(extra.u32())
        name_length = extra.u8()
        name = extra.take(((name_length + 4) & ~3) - 1)[:name_length]
        blocks = []
        while extra.remaining() >= 12 and len(blocks) < MAX_BLOCKS:
            signature = extra.take(4)
            if signature not in (b'8BIM', b'8B64'):
                blocks.append(('<unknown signature>', signature))
                break
            key = extra.take(4)
            length = _block_length(extra, signature, key, large)
            body_at = extra.at
            body = extra.take(length)
            # Photoshop pads a record's tagged block to a multiple of 4, just
            # as it pads the tail's; Adobe's table only requires an EVEN byte
            # count, and other writers (Doctored's own PSD export included)
            # take it at its word. Skipping the pad is what keeps the walk in
            # step, so the pad is chosen by looking at the bytes: the
            # multiple-of-4 skip stands unless it does not land on a
            # signature and the even skip does.
            after_four = min(extra.end, body_at + length + (-length % 4))
            after_even = min(extra.end, body_at + length + (-length % 2))
            extra.at = after_four
            if after_even != after_four \
                    and extra.data[after_even:after_even + 4] in (b'8BIM', b'8B64') \
                    and extra.data[after_four:after_four + 4] not in (b'8BIM', b'8B64'):
                extra.at = after_even
            blocks.append((key.decode('latin-1'), body))
        # What is LEFT of the record's extra data after the block walk. The
        # walk stops at `remaining() < 12` (a block cannot be shorter than its
        # own header), so a record whose blocks were framed correctly leaves
        # 0..11 bytes of the writer's own section padding; 0 on every
        # committed Photoshop and Doctored record measured so far. A large
        # value means the walk fell out of step — which is what makes this
        # number framing evidence rather than trivia.
        records.append(dict(name=name.decode('latin-1'), bounds=bounds, blend=blend.decode('latin-1'),
                            opacity=opacity, clipping=clipping, flags=flags,
                            mask_body=mask_body, mask_body_at=mask_body_at,
                            ranges=ranges, blocks=blocks, trailing=extra.remaining(),
                            _table=table))
    for record in records:
        record['channels'] = []
        for identifier, length in record.pop('_table'):
            body = reader.take(length)
            compression = struct.unpack('>H', body[:2])[0] if length >= 2 else None
            record['channels'].append((identifier, length, compression))
    return records


def _layer_section(reader, end, large=False):
    """The layer-information sub-section, its padding, and the tail blocks."""
    info_length = reader.u64() if large else reader.u32()
    info_end = reader.at + info_length
    assert info_end <= end, 'layer information overruns its section'
    layers, padding = [], 0
    if info_length:
        info = Reader(reader.data, reader.at, info_end)
        count = info.i16()
        assert abs(count) <= MAX_RECORDS, count
        layers = _records(info, abs(count), large)
        padding = info.remaining()
    reader.at = info_end
    # A writer may end the section right after the layer information: the
    # global layer mask block and the tail are both optional.
    global_mask = reader.u32() if reader.at + 4 <= end else 0
    reader.at = min(end, reader.at + global_mask)
    tail, tail_layers = [], []
    while reader.at + 12 <= end and len(tail) < MAX_BLOCKS:
        signature = reader.take(4)
        if signature not in (b'8BIM', b'8B64'):
            tail.append(('<unknown signature>', 0))
            break
        key = reader.take(4)
        length = _block_length(reader, signature, key, large)
        body_at = reader.at
        reader.take(length)
        # Tail blocks are padded to a multiple of 4.
        reader.at = min(end, body_at + length + (-length % 4))
        tail.append((key.decode('latin-1'), length))
        if key in (b'Lr16', b'Lr32') and not tail_layers:
            body = Reader(reader.data, body_at, body_at + length)
            count = body.i16()
            assert abs(count) <= MAX_RECORDS, count
            tail_layers = _records(body, abs(count), large)
    reader.at = end
    return info_length, layers, padding, tail, tail_layers


def read(path):
    """Everything this reader claims about a PSD's or PSB's framing. No pixel decode."""
    data = path.read_bytes()
    reader = Reader(data)
    assert reader.take(4) == b'8BPS', 'not a PSD'
    version = reader.u16()
    assert version in (1, 2), f'unknown PSD version {version}'
    large = version == 2
    reader.take(6)
    channels = reader.u16()
    height, width = reader.u32(), reader.u32()
    depth, colour = reader.u16(), reader.u16()
    reader.take(reader.u32())                      # colour mode data
    resources_length = reader.u32()
    resources_end = reader.at + resources_length
    resources = {}
    while reader.at + 8 <= resources_end:
        assert reader.take(4) == b'8BIM', 'invalid image resource signature'
        identifier = reader.u16()
        name_length = reader.u8()
        reader.take(name_length + (name_length + 1) % 2)
        length = reader.u32()
        resources[identifier] = length
        reader.take(length + length % 2)
    reader.at = resources_end
    section_length = reader.u64() if large else reader.u32()
    section_end = reader.at + section_length
    info_length, layers, padding, tail, tail_layers = _layer_section(reader, section_end, large)
    composite = reader.u16() if reader.remaining() >= 2 else None
    return dict(canvas=[width, height], depth=depth, color=colour, channels=channels,
                resources=resources, layer_info_len=info_length,
                layer_info_padding=padding,
                layers=layers or tail_layers, layer_source='layer_info' if layers else 'tail',
                tail=tail, composite_compression=composite, size=len(data))


def summary(record):
    """One record as plain JSON: bodies become lengths and hex previews."""
    return dict(name=record['name'], bounds=record['bounds'], blend=record['blend'],
                opacity=record['opacity'], clipping=record['clipping'], flags=record['flags'],
                mask_body=record['mask_body'].hex(), ranges_length=len(record['ranges']),
                channels=[list(c) for c in record['channels']],
                blocks=[[key, len(body)] for key, body in record['blocks']],
                trailing=record['trailing'])
