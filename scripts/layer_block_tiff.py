"""Hand-built TIFFs carrying a tag-37724 layer block, for the app checks of File ▸ Save
on a TIFF opened flat (`ImageDocument.sourceLayersUnopened`): `verify_mode_save_losses.py`'s
`unread` cells and `save-preservation-probe`.

`layered_tiff` writes a classic TIFF from the TIFF 6.0 layout: an 8-byte header, one
uncompressed strip, one directory of 12-byte entries in ascending tag order, values
longer than four bytes out of line and word-aligned. Tag 37724 holds the block's prefix
and one `Layr`/`Lr16`/`Lr32` entry whose body is the i16 record count and record bytes
nothing reads, laid out as Photoshop's notes give it (`core/src/tiff_layers.rs`' module
doc): in a little-endian file the codes are byte-reversed (`MIB8`, `ryaL`) and the length
and count are little-endian. The core's `tiff_layer_block` is the reader; nothing here
shares its code.
"""

import struct

PREFIX = b'Adobe Photoshop Document Data Block\x00'
KEYS = {8: b'Layr', 16: b'Lr16', 32: b'Lr32'}


def layered_tiff(order='<', photometric=2, samples=3, bits=8, float_samples=False,
                 count=1, width=4, height=3):
    """The bytes of a `width` × `height` TIFF in byte order `order` ('<' little-endian,
    '>' big-endian), every sample `bits` wide (IEEE float when `float_samples`), mid grey,
    whose layer block holds `count` records."""
    little = order == '<'
    sample = {8: 'B', 16: 'H', 32: 'f' if float_samples else 'I'}[bits]
    mid = 0.5 if float_samples else (1 << bits) // 2
    strip = struct.pack(f'{order}{width * height * samples}{sample}',
                        *([mid] * (width * height * samples)))
    key = KEYS[bits]
    signature = b'8BIM'
    if little:
        key, signature = key[::-1], signature[::-1]
    body = struct.pack(f'{order}h', count) + b'\x5a' * 6
    value = PREFIX + signature + key + struct.pack(f'{order}I', len(body)) + body
    value += b'\x00' * (-len(value) % 4)

    out = bytearray((b'II' if little else b'MM') + struct.pack(f'{order}HI', 42, 0))
    strip_at = len(out)
    out += strip

    def shorts(*values):
        return struct.pack(f'{order}{len(values)}H', *values)

    # (tag, type, count, value bytes)
    fields = [
        (256, 3, 1, shorts(width)),
        (257, 3, 1, shorts(height)),
        (258, 3, samples, shorts(*[bits] * samples)),
        (259, 3, 1, shorts(1)),
        (262, 3, 1, shorts(photometric)),
        (273, 4, 1, struct.pack(f'{order}I', strip_at)),
        (277, 3, 1, shorts(samples)),
        (278, 3, 1, shorts(height)),
        (279, 4, 1, struct.pack(f'{order}I', len(strip))),
        (284, 3, 1, shorts(1)),
    ]
    if float_samples:
        fields.append((339, 3, samples, shorts(*[3] * samples)))
    fields.append((37724, 7, len(value), value))
    slots = []
    for _, _, _, data in fields:
        if len(data) <= 4:
            slots.append(data.ljust(4, b'\x00'))
        else:
            if len(out) % 2:
                out += b'\x00'
            slots.append(struct.pack(f'{order}I', len(out)))
            out += data
    if len(out) % 2:
        out += b'\x00'
    directory = len(out)
    out += struct.pack(f'{order}H', len(fields))
    for (tag, kind, number, _), slot in zip(fields, slots):
        out += struct.pack(f'{order}HHI', tag, kind, number) + slot
    out += b'\x00' * 4
    out[4:8] = struct.pack(f'{order}I', directory)
    return bytes(out)
