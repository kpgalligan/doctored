#!/usr/bin/env python3
"""The MCP drive: an untagged CMYK TIFF opens through the working CMYK.

Since #1 such a file opens as a CMYK DOCUMENT assigned the working CMYK
profile (no conversion to sRGB), so the twins are two CMYK documents with the
same ink and the same profile, and the sentence is the assignment's.

Two gates, FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no expectation is relaxed afterwards. The instance is owned by
this file (`HostApp`: a free port, a dummy API key and a PRIVATE directory for
every preference library the app reads) and is terminated by its own context
manager; no Doctored process this file did not start is ever signalled.

The fixtures are written here in pure Python: an untagged 8-bit chunky CMYK
TIFF of a 5⁴ lattice of inks, and its twin carrying the default working
CMYK, the installed `USWebCoatedSWOP.icc`. The drive FAILS when that
file is absent: it is what the app resolves as the working CMYK on a machine
with Adobe's profiles, and the twin needs the same bytes.

- **V1 pixels.** Both files `open_document`; their `render` PNGs decode to the
  same pixels — the untagged file was converted exactly as the SWOP-tagged one.
- **V2 sentence.** `get_color_profile.on_open_refusal` of the untagged file is
  the working-profile ASSIGNMENT sentence naming `U.S. Web Coated (SWOP) v2`
  (#1), and both documents report `color_mode` "cmyk" with that profile.
"""
import argparse
import base64
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-source-probe'))

import app_under_test
from support import HostApp

# ---- the expectations, fixed here BEFORE anything is measured -------------
SWOP = Path('/Library/Application Support/Adobe/Color/Profiles/Recommended/USWebCoatedSWOP.icc')
SWOP_NAME = 'U.S. Web Coated (SWOP) v2'
SENTENCE = ('The image is CMYK with no embedded colour profile, so the working CMYK profile '
            f'“{SWOP_NAME}” was assigned as it was read.')
LEVELS = [0, 64, 128, 191, 255]
DEFAULT_APP = app_under_test.default_app_bundle()


def lattice():
    return bytes(v for c in LEVELS for m in LEVELS for y in LEVELS for k in LEVELS
                 for v in (c, m, y, k))


def cmyk_tiff(width, height, data, icc=None):
    """A little-endian classic TIFF: Photometric 5 (separated), chunky, one
    uncompressed strip of four 8-bit samples, and an optional ICC (34675)."""
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<4H', 8, 8, 8, 8))
    icc_at = place(icc) if icc is not None else None
    if len(body) % 2:
        body.append(0)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [
        (256, 4, 1, struct.pack('<I', width)),
        (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 4, struct.pack('<I', bits_at)),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(5)),
        (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(4)),
        (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))),
        (284, 3, 1, short(1)),
    ]
    if icc is not None:
        entries.append((34675, 7, len(icc), struct.pack('<I', icc_at)))
    out = bytearray(b'II*\x00')
    out += struct.pack('<I', 8 + len(body))
    out += body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def png_pixels(data):
    """Width, height and the unfiltered rows of an 8-bit RGB/RGBA PNG — this
    file's own reader, so a decode never comes from the app."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    offset, packed = 8, b''
    width = height = color = 0
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        offset += size + 12
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', payload)
            assert depth == 8 and interlace == 0 and color in (2, 6), (depth, color, interlace)
        elif kind == b'IDAT':
            packed += payload
    unit = 4 if color == 6 else 3
    stride = width * unit
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    rows = []
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        row = bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            left = row[i - unit] if i >= unit else 0
            up = previous[i]
            corner = previous[i - unit] if i >= unit else 0
            if kind == 0:
                predictor = 0
            elif kind == 1:
                predictor = left
            elif kind == 2:
                predictor = up
            elif kind == 3:
                predictor = (left + up) // 2
            else:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                predictor = left if pa <= pb and pa <= pc else (up if pb <= pc else corner)
            row[i] = (row[i] + predictor) & 0xFF
        rows.append(bytes(row))
        previous = row
    return width, height, b''.join(rows)


def render(app, document):
    response = app.client.rpc('tools/call', {
        'name': 'render', 'arguments': {'document_id': document, 'max_side': 4096}})
    assert not response.get('isError'), response
    data = next(item['data'] for item in response['content'] if item['type'] == 'image')
    return png_pixels(base64.b64decode(data))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', default=str(DEFAULT_APP))
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert SWOP.is_file(), f'{SWOP} is not installed; this drive needs it'
    executable = Path(args.app).expanduser()
    if executable.is_dir():
        executable = executable / 'Contents/MacOS/Doctored'
    assert executable.is_file(), executable
    output = Path(args.receipts or tempfile.mkdtemp(prefix='issue327-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)

    data = lattice()
    side = 25
    assert len(data) == side * side * 4
    untagged = folder / 'untagged.tif'
    tagged = folder / 'swop-tagged.tif'
    untagged.write_bytes(cmyk_tiff(side, side, data))
    tagged.write_bytes(cmyk_tiff(side, side, data, SWOP.read_bytes()))

    receipts = {'expectations': 'fixed in scripts/verify_issue327_untagged_cmyk.py before any '
                                'measurement', 'swop': str(SWOP)}
    with HostApp(folder / 'app', folder / 'recovery', executable) as app:
        plain = app.call('open_document', path=str(untagged))['document']['id']
        twin = app.call('open_document', path=str(tagged))['document']['id']

        plain_pixels, twin_pixels = render(app, plain), render(app, twin)
        assert plain_pixels[:2] == (side, side), plain_pixels[:2]
        assert plain_pixels == twin_pixels, 'the untagged render differs from the SWOP twin'
        receipts['v1'] = {'size': plain_pixels[:2], 'equal': True}
        print('PASS v1: the untagged CMYK TIFF renders as its SWOP-tagged twin', flush=True)

        profile = app.call('get_color_profile', document_id=plain)
        refusal = profile.get('on_open_refusal')
        assert refusal == SENTENCE, (refusal, SENTENCE)
        assert profile.get('color_mode') == 'cmyk', profile
        assert profile.get('cmyk_profile') == SWOP_NAME, profile
        twin_profile = app.call('get_color_profile', document_id=twin)
        assert twin_profile.get('color_mode') == 'cmyk', twin_profile
        assert twin_profile.get('cmyk_profile') == SWOP_NAME, twin_profile
        receipts['v2'] = {'on_open_refusal': refusal,
                          'twin_on_open_refusal':
                              app.call('get_color_profile', document_id=twin)
                              .get('on_open_refusal')}
        print(f'PASS v2: on_open_refusal names {SWOP_NAME}', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
