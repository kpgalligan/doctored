#!/usr/bin/env python3
"""Colours on a document tagged with an RGB LUT profile, and a 16-bit
Convert to Profile into one, over MCP against an owned Debug instance.

The LUT is the core tests' synthetic 16-235 profile (`core/tests/
precision_lut_conversion.rs`), written here byte by byte: an RGB `mntr`
profile with an XYZ PCS whose grid-2 `mft2` B2A0 maps the encoded XYZ input
x to d = 16/255 + x·219/255 and whose A2B0 inverts it through a 256-entry
input curve. The oracle is this file's arithmetic: the IEC 61966-2-1 curve,
sRGB's published primaries Bradford-adapted to the ICC PCS white, the `mft2`
XYZ encoding and the corner map. At 8 and 16 bits:

- DOCUMENT HEX: `fill` with `color_space: "document"` lands the hex's own
  numbers (8 bits: `sample_color`'s hex; 16 bits: every code v·257), and the
  same hex authored in sRGB lands elsewhere — the check that would see the
  document spelling routed through sRGB.
- AUTHORED: a plain (sRGB) hex lands on the oracle's numbers within one 8-bit
  code.
- READING: `sample_color` reports `srgb_rgba` within 1/255 of the oracle's
  decode of the sampled numbers (at 16 bits also within 1/255 of the
  authored colour) and `srgb_in_gamut` true, which a LUT-tagged document
  once omitted.
- CONVERT (16 bits): `convert_profile` of an sRGB document into the LUT
  answers changed:true, the sample is on the oracle within 2/255, the result's
  `get_color_profile` can_decode is true, and converting back to sRGB lands
  within 2/255 of the start. A 32-bit float document still answers
  changed:false.

Usage: python3 scripts/verify_lut_document_colours.py [--output DIR]
"""

import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
from verify_v1_16bit import App  # noqa: E402

LUT_NAME = 'Synthetic 16-235 LUT'
COLOUR = (0x2E, 0x80, 0xC8)
HEX = '#2e80c8'
D50 = (0.9642, 1.0, 0.8249)


# -------------------------------------------------------------- profile --

def lut_profile():
    def s15(v):
        return struct.pack('>i', round(v * 65536.0))

    def desc(name):
        raw = name.encode('ascii')
        return b'desc\0\0\0\0' + struct.pack('>I', len(raw) + 1) + raw + b'\0'

    def mft2(input_curve, corner):
        out = b'mft2\0\0\0\0' + bytes([3, 3, 2, 0])
        out += b''.join(s15(v) for v in (1, 0, 0, 0, 1, 0, 0, 0, 1))
        out += struct.pack('>HH', len(input_curve), 2)
        out += b''.join(struct.pack('>H', v) for v in input_curve) * 3
        for index in range(8):
            coords = [(index >> (2 - axis)) & 1 for axis in range(3)]
            for value in corner(coords):
                out += struct.pack('>H', round(min(max(value, 0.0), 1.0) * 65535))
        out += struct.pack('>HH', 0, 65535) * 3
        return out

    inverse = [round(min(max((k - 16) / 219, 0.0), 1.0) * 65535) for k in range(256)]
    tags = [(b'desc', desc(LUT_NAME)),
            (b'wtpt', b'XYZ \0\0\0\0' + s15(D50[0]) + s15(D50[1]) + s15(D50[2])),
            (b'A2B0', mft2(inverse, lambda c: [float(v) for v in c])),
            (b'B2A0', mft2([0, 65535], lambda c: [16 / 255 + v * 219 / 255 for v in c]))]
    start = 132 + 12 * len(tags)
    table, payload = b'', b''
    for signature, data in tags:
        table += signature + struct.pack('>II', start + len(payload), len(data))
        payload += data + b'\0' * (-len(data) % 4)
    header = bytearray(128)
    struct.pack_into('>I', header, 0, start + len(payload))
    header[8:12] = bytes([0x02, 0x20, 0, 0])
    header[12:16], header[16:20], header[20:24] = b'mntr', b'RGB ', b'XYZ '
    header[36:40] = b'acsp'
    return bytes(header) + struct.pack('>I', len(tags)) + table + payload


# --------------------------------------------------------------- oracle --

def inverse3(m):
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
           - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))

    def cof(r, c):
        r0, r1, c0, c1 = (r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3
        return m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    return [[cof(c, r) / det for c in range(3)] for r in range(3)]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def matmul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def srgb_d50():
    def xyz(x, y):
        return [x / y, 1.0, (1 - x - y) / y]
    cols = [xyz(0.64, 0.33), xyz(0.30, 0.60), xyz(0.15, 0.06)]
    basis = [[cols[k][i] for k in range(3)] for i in range(3)]
    white = xyz(0.3127, 0.3290)
    scale = apply(inverse3(basis), white)
    d65 = [[basis[i][k] * scale[k] for k in range(3)] for i in range(3)]
    brad = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]]
    s, d = apply(brad, white), apply(brad, list(D50))
    cone = [[d[0] / s[0], 0, 0], [0, d[1] / s[1], 0], [0, 0, d[2] / s[2]]]
    return matmul(matmul(inverse3(brad), matmul(cone, brad)), d65)


def decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def encode(v):
    v = min(max(v, 0.0), 1.0)
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def into_lut(rgb):
    """sRGB unit values as the LUT document's unit numbers."""
    xyz = apply(srgb_d50(), [decode(v) for v in rgb])
    return [16 / 255 + min(max(v * 32768 / 65535, 0.0), 1.0) * 219 / 255 for v in xyz]


def out_of_lut(device):
    """The LUT document's unit numbers as sRGB unit values: the profile's own
    u16 input curve by linear interpolation, the `mft2` XYZ decoding, the
    matrix inverted, clamped and encoded."""
    curve = [round(min(max((k - 16) / 219, 0.0), 1.0) * 65535) for k in range(256)]
    x = []
    for d in device:
        t = min(max(d, 0.0), 1.0) * 255
        i = min(int(t), 254)
        f = t - i
        x.append((curve[i] * (1 - f) + curve[i + 1] * f) / 65535)
    xyz = [v * 65535 / 32768 for v in x]
    return [encode(v) for v in apply(inverse3(srgb_d50()), xyz)]


# ---------------------------------------------------------------- cases --

def fresh(app, depth, lut_path):
    document = app.call('new_document', width=8, height=8, depth=depth, profile='srgb',
                        background='white')['document']['id']
    reply = app.call('assign_profile', document_id=document, profile='file', path=str(lut_path))
    assert reply.get('changed'), reply
    return document


def unit_sample(app, document, depth):
    reply = app.call('sample_color', document_id=document, x=3, y=3)
    if depth == 8:
        return [reply[k] / 255 for k in 'rgb'], reply
    return reply['normalized_rgba'][:3], reply


def fill(app, document, **colour):
    reply = app.call('fill', document_id=document, x=0, y=0, tolerance=255, **colour)
    assert reply.get('ok', True), reply


def colours_case(app, depth, lut_path):
    detail = {}
    # DOCUMENT HEX: the hex's own numbers.
    document = fresh(app, depth, lut_path)
    fill(app, document, color=HEX, color_space='document')
    numbers, reply = unit_sample(app, document, depth)
    detail['document_hex'] = reply
    want = [v / 255 for v in COLOUR]
    if depth == 8:
        assert reply['hex'].lower() == HEX, reply
    else:
        assert reply['center_rgba_codes'][:3] == [v * 257 for v in COLOUR], reply
    # AUTHORED: the same hex in sRGB lands on the oracle, elsewhere.
    fill(app, document, color=HEX)
    authored, reply = unit_sample(app, document, depth)
    detail['authored'] = reply
    oracle = into_lut(want)
    detail['oracle'] = oracle
    assert all(abs(a - o) <= 1 / 255 + 1e-9 for a, o in zip(authored, oracle)), (authored, oracle)
    assert max(abs(a - d) for a, d in zip(authored, numbers)) > 2 / 255, (authored, numbers)
    # READING: srgb_rgba is the sampled numbers' sRGB colour. At 8 bits a
    # number is one code of the LUT's 219-code range, so that reading is the
    # decode of the stored numbers, not the authored hex (4 codes from it in
    # red here); at 16 bits the two agree.
    assert reply.get('srgb_in_gamut') is True, reply
    srgb = reply['srgb_rgba']
    reading = out_of_lut(authored)
    detail['reading_oracle'] = reading
    assert all(abs(s - r) <= 1 / 255 + 1e-9 for s, r in zip(srgb[:3], reading)), (srgb, reading)
    if depth == 16:
        assert all(abs(s - w) <= 1 / 255 + 1e-9 for s, w in zip(srgb[:3], want)), (srgb, want)
    app.call('close_document', document_id=document, discard_changes=True)
    return detail


def convert_case(app, lut_path):
    detail = {}
    document = app.call('new_document', width=8, height=8, depth=16, profile='srgb',
                        background='white')['document']['id']
    fill(app, document, color=HEX)
    start, _ = unit_sample(app, document, 16)
    reply = app.call('convert_profile', document_id=document, profile='file', path=str(lut_path))
    detail['convert'] = reply
    assert reply.get('changed') is True, reply
    converted, sample = unit_sample(app, document, 16)
    oracle = into_lut(start)
    detail['sample'], detail['oracle'] = sample, oracle
    assert all(abs(c - o) <= 2 / 255 for c, o in zip(converted, oracle)), (converted, oracle)
    profile = app.call('get_color_profile', document_id=document)
    detail['profile'] = profile
    assert profile.get('can_decode') is True, profile
    back = app.call('convert_profile', document_id=document, profile='srgb')
    assert back.get('changed') is True, back
    returned, _ = unit_sample(app, document, 16)
    detail['returned'] = returned
    assert all(abs(r - s) <= 2 / 255 for r, s in zip(returned, start)), (returned, start)
    app.call('close_document', document_id=document, discard_changes=True)
    # A float document keeps its matrix/TRC rule.
    floating = app.call('new_document', width=8, height=8, depth=32, profile='srgb',
                        background='white')['document']['id']
    reply = app.call('convert_profile', document_id=floating, profile='file', path=str(lut_path))
    detail['float'] = reply
    assert reply.get('changed') is False, reply
    assert '32-bit float' in json.dumps(reply), reply
    app.call('close_document', document_id=floating, discard_changes=True)
    return detail


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', help='folder for the instance and its receipt')
    arguments = parser.parse_args()
    output = Path(arguments.output) if arguments.output else Path(
        tempfile.mkdtemp(prefix='rz-lut-colours-'))
    output.mkdir(parents=True, exist_ok=True)
    lut_path = output / 'synthetic-16-235.icc'
    lut_path.write_bytes(lut_profile())
    results, failed = {}, []

    def run(name, step):
        try:
            results[name] = dict(passed=True, detail=step())
            print(f'PASS {name}', flush=True)
        except BaseException as error:  # noqa: BLE001 - every case reports
            results[name] = dict(passed=False, error=repr(error)[:2000])
            failed.append(name)
            print(f'FAIL {name}: {error!r}'[:2400], flush=True)

    with App(output) as app:
        run('COLOURS 8', lambda: colours_case(app, 8, lut_path))
        run('COLOURS 16', lambda: colours_case(app, 16, lut_path))
        run('CONVERT 16', lambda: convert_case(app, lut_path))

    (output / 'receipt.json').write_text(json.dumps(results, indent=2, default=str) + '\n')
    print(f'receipt: {output / "receipt.json"}')
    if failed:
        print(f'{len(failed)} case(s) failed: ' + ', '.join(failed))
        raise SystemExit(1)
    print('ALL CASES PASS')


if __name__ == '__main__':
    main()
