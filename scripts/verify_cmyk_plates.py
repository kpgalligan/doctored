#!/usr/bin/env python3
"""The #1 remaining-gaps MCP drive: on a 16-bit Generic CMYK document one ink
plate as an edit target, Fill and Stroke Path in ink, Apply Image and
Calculations on plates and channels, an alpha channel's Levels and the scalar
histograms, Gamut Warning against a proof press, and the reduced-resolution
region render.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python. The CMYK profile is macOS's
Generic CMYK Profile; ink codes are `percent / 100 × 65535` rounded half up. A
plate's number is its stored `L = 65535 − ink` (`cmyk_plate_scalar`), read off
`sample_color`'s `ink16`. The RGB TWINS are 16-bit sRGB TIFFs whose planes hold
plates' numbers: `twin` R, G, B = L of C, M, Y; `twin_k` R, G, B = L of K, C, M.
"An operation on a plate equals the twin's" is the oracle where the kernel's own
rounding would otherwise have to be restated (Levels, Curves, the Brush, the
gradient, the plane blends): the same scalar kernel on the same numbers must
answer the same numbers. Rounding below is half up.

Fixture `split.tif` (32 x 24): the left half ink A [60, 10, 10, 0], the right
half ink B [10, 60, 10, 20], and a 4 x 4 patch of A at (24, 10) inside B.

- **P1 one plate.** Each from the fixture's own state (`undo` after each):
  fill_selection target cyan color #000000 opacity 0.5 inside select_rect
  (4, 4, 10, 8) makes L' = round(L / 2) there and changes nothing else; fill
  target magenta at (2, 2) color #000000 makes M 65535 on the left half (x < 16)
  and changes nothing else (the patch is not contiguous); apply_filter invert
  target black makes K' = 65535 − K everywhere; gradient target yellow
  (0, 12) → (32, 12) #000000 → #ffffff, apply_filter levels target magenta
  output_white 0.5, apply_filter curves target cyan params rgb_map (the inverting
  map) and brush_stroke target black (2, 20)–(30, 20) size 5 each equal the
  twin's same call on its matching plane (yellow ↔ twin blue, magenta ↔ twin
  green, cyan ↔ twin red, black ↔ twin_k red) at every pixel, the other plates
  exact. fill_selection target cyan ink [0, 0, 0, 100] refuses with "A plate or
  alpha channel paints in grey: pass color (its grey is painted; white leaves no
  ink)." and apply_filter curves target cyan params cmyk_map refuses with
  "\\"cmyk_map\\" is a CMYK layer's Levels/Curves channel; a plate or alpha channel
  target takes rgb (and Curves' rgb_map).", each leaving every pixel exact.
- **P2 Fill and Stroke Path.** A work path from select_rect (4, 4, 12, 10):
  fill_path ink [0, 0, 0, 100] layer 0 makes (6, 6) [0, 0, 0, 65535] exactly and
  leaves (24, 20) exact; fill_path target cyan color #000000 makes C 65535 at
  (6, 6) with M, Y, K exact; stroke_path ink [100, 0, 0, 0] size 3 raises C at
  (4, 8) and leaves (24, 20) exact.
- **P3 Apply Image and Calculations.** apply_image target cyan source_plane
  magenta (merged, Normal, 1) makes C = M at every pixel, M, Y, K exact;
  apply_image target layer source_plane cmyk blend Multiply equals, plate for
  plate, the twin's layer Multiply (C, M, Y ↔ R, G, B) and twin_k's target red
  source_plane red Multiply (K); calculations source1_plane cyan source2_plane
  black Multiply result new_channel equals twin_k's calculations source1_plane
  green source2_plane red Multiply at every pixel.
- **P4 a channel's Levels and the scalar histograms.** A channel "Ch" from
  select_rect (6, 4, 14, 12) feathered 3 on both the document and the twin;
  apply_filter levels target channel:Ch output_white 0.5 equals the twin's at
  every pixel. histogram source scalar target cyan (selection false) reports
  weighted_code_sums_rgba[0] = Σ (65535 − C) · 65535, and target channel:Ch the
  channel's Σ v · 65535.
- **P5 Gamut Warning.** On `gamut.tif` (inks 100 % C, 100 % M, 100 % Y, C+M,
  paper and a light grey) proofed through a second installed proofable CMYK
  press: out_of_gamut is above 0 and below the pixel count, the same with
  gamut_warning false, the warning's PNG differs from the plain proof, and
  preserve_numbers true flags 0.
- **P6 reduced regions.** render region (0, 0, 32, 24) at divisor 1 is the
  full view; regions (0, 0, 32, 24) and (1, 2, 29, 22) at divisors 2, 3 and 7 are
  ceil(w/d) × ceil(h/d) and every pixel is the full view's at (x + i·d, y + j·d).
"""
import argparse
import base64
import json
import math
from pathlib import Path
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App
from verify_cmyk_channels import cmyk_tiff

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SRGB = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc')
W, H = 32, 24
INK_A = [60, 10, 10, 0]
INK_B = [10, 60, 10, 20]
PATCH = (24, 10, 4, 4)
GAMUT_INKS = [[100, 0, 0, 0], [0, 100, 0, 0], [0, 0, 100, 0], [100, 100, 0, 0], [0, 0, 0, 0],
              [10, 8, 8, 0]]
PLATE_INK = ('A plate or alpha channel paints in grey: pass color (its grey is painted; white '
             'leaves no ink).')
CMYK_KEY = ('"cmyk_map" is a CMYK layer\'s Levels/Curves channel; a plate or alpha channel target '
            'takes rgb (and Curves\' rgb_map).')
ALL = [(x, y) for y in range(H) for x in range(W)]
DIVISORS = (2, 3, 7)


def codes(percent):
    return [int(p / 100 * 65535 + 0.5) for p in percent]


def half_up(v):
    return int(math.floor(v + 0.5))


def rgb16_tiff(width, height, samples, icc):
    """A little-endian classic chunky 16-bit RGB TIFF (Photometric 2)."""
    data = struct.pack(f'<{len(samples)}H', *samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<3H', 16, 16, 16))
    icc_at = place(icc)
    if len(body) % 2:
        body.append(0)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 3, struct.pack('<I', bits_at)), (259, 3, 1, short(1)),
        (262, 3, 1, short(2)), (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(3)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))), (284, 3, 1, short(1)),
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def split_inks():
    px, py, pw, ph = PATCH
    out = []
    for y in range(H):
        for x in range(W):
            inside = px <= x < px + pw and py <= y < py + ph
            out.append(codes(INK_A if x < W // 2 or inside else INK_B))
    return out


def png_pixels(data):
    """Width, height and the pixels (tuples) of an 8- or 16-bit RGB/RGBA PNG —
    this file's own reader, so a decode never comes from the app."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    offset, packed = 8, b''
    width = height = color = depth = 0
    while offset < len(data):
        size = struct.unpack_from('>I', data, offset)[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        offset += size + 12
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', payload)
            assert depth in (8, 16) and interlace == 0 and color in (2, 6), (depth, color)
        elif kind == b'IDAT':
            packed += payload
    channels = 4 if color == 6 else 3
    unit = channels * depth // 8
    stride = width * unit
    raw = zlib.decompress(packed)
    previous = bytearray(stride)
    pixels = []
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
        fmt = '>' + ('H' if depth == 16 else 'B') * (width * channels)
        values = struct.unpack(fmt, bytes(row))
        pixels.append([tuple(values[x * channels:(x + 1) * channels]) for x in range(width)])
        previous = row
    return width, height, pixels


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    assert SRGB.is_file(), SRGB
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-plates-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_plates.py before any measurement'}
    inks = split_inks()
    split = folder / 'split.tif'
    split.write_bytes(cmyk_tiff(W, H, [v for ink in inks for v in ink], profile))
    numbers = [[65535 - v for v in ink] for ink in inks]
    twin = folder / 'twin.tif'
    twin.write_bytes(rgb16_tiff(W, H, [v for n in numbers for v in n[:3]], SRGB.read_bytes()))
    twin_k = folder / 'twin_k.tif'
    twin_k.write_bytes(rgb16_tiff(W, H, [v for n in numbers for v in (n[3], n[0], n[1])],
                                  SRGB.read_bytes()))
    gamut = folder / 'gamut.tif'
    gamut_samples = []
    for y in range(12):
        for x in range(24):
            gamut_samples += codes(GAMUT_INKS[x // 4])
    gamut.write_bytes(cmyk_tiff(24, 12, gamut_samples, profile))

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def attempt(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return bool(result.get('isError')), text

        def refused(tool, sentence, **arguments):
            failed, text = attempt(tool, **arguments)
            assert failed and sentence in text, (tool, text)
            return text

        def image(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            assert not result.get('isError'), (tool, result)
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            text = next((c['text'] for c in result['content'] if c['type'] == 'text'), '{}')
            try:
                info = json.loads(text)
            except json.JSONDecodeError:
                info = {'text': text}
            return png_pixels(base64.b64decode(data)), info

        def plates(doc, points=ALL):
            """The stored ink16 + alpha16 at every point."""
            out = []
            for x, y in points:
                reply = call('sample_color', document_id=doc, x=x, y=y)
                out.append(reply['ink16'] + [reply['alpha16']])
            return out

        def rgb(doc, points=ALL):
            return [call('sample_color', document_id=doc, x=x, y=y)['center_rgba16']
                    for x, y in points]

        def coverage(doc, points=ALL):
            reply = call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for x, y in points])
            return [s['coverage16'] for s in reply.get('samples', [])]

        def channel(doc, name):
            call('load_selection', document_id=doc, **{'from': 'channel'}, channel=name)
            got = coverage(doc)
            call('deselect', document_id=doc)
            return got

        def only(before, after, k, want, what):
            """Plate k of `after` is `want` (ink16 per pixel); every other component exact."""
            for i, (b, a) in enumerate(zip(before, after)):
                for c in range(5):
                    expected = want[i] if c == k else b[c]
                    assert a[c] == expected, (what, ALL[i], c, a, b, expected)

        doc = call('open_document', path=str(split))['document']['id']
        rgb_doc = call('open_document', path=str(twin))['document']['id']
        k_doc = call('open_document', path=str(twin_k))['document']['id']
        assert call('get_document', document_id=doc).get('color_mode') == 'cmyk'
        base = plates(doc)
        assert [p[:4] for p in base] == inks, 'the fixture reads back exactly'
        base_rgb, base_k = rgb(rgb_doc), rgb(k_doc)

        def twin_plate(k):
            """(twin document, its plane index, its target name) for plate k."""
            return [(rgb_doc, 0, 'red'), (rgb_doc, 1, 'green'), (rgb_doc, 2, 'blue'),
                    (k_doc, 0, 'red')][k]

        def undo(*docs):
            for d in docs:
                call('undo', document_id=d)
            assert plates(doc) == base, 'undo restores the fixture'

        # ---- P1: one plate ---------------------------------------------------------------
        p1 = {}
        call('select_rect', document_id=doc, x=4, y=4, width=10, height=8)
        call('fill_selection', document_id=doc, target='cyan', color='#000000', opacity=0.5)
        inside = lambda x, y: 4 <= x < 14 and 4 <= y < 12
        want = [65535 - half_up(numbers[i][0] * 0.5) if inside(*ALL[i]) else base[i][0]
                for i in range(len(ALL))]
        only(base, plates(doc), 0, want, 'fill_selection cyan')
        undo(doc)
        call('deselect', document_id=doc)
        p1['fill_selection'] = 'exact'

        call('fill', document_id=doc, target='magenta', x=2, y=2, color='#000000')
        want = [65535 if ALL[i][0] < W // 2 else base[i][1] for i in range(len(ALL))]
        only(base, plates(doc), 1, want, 'fill magenta')
        undo(doc)
        p1['fill'] = 'exact'

        call('apply_filter', document_id=doc, filter='invert', target='black')
        only(base, plates(doc), 3, [65535 - p[3] for p in base], 'invert black')
        undo(doc)
        p1['invert'] = 'exact'

        twinned = [
            ('gradient', 2, {'x0': 0, 'y0': 12, 'x1': 32, 'y1': 12, 'start_color': '#000000',
                             'end_color': '#ffffff'}),
            ('levels', 1, {'output_white': 0.5}),
            ('curves', 0, {'params': {'rgb_map': [255 - i for i in range(256)]}}),
            ('brush_stroke', 3, {'points': [[2, 20], [30, 20]], 'size': 5}),
        ]
        for tool, k, arguments in twinned:
            other, plane, name = twin_plate(k)
            if tool in ('levels', 'curves'):
                call('apply_filter', document_id=doc, filter=tool, target=['cyan', 'magenta', 'yellow',
                     'black'][k], **arguments)
                call('apply_filter', document_id=other, filter=tool, target=name, **arguments)
            else:
                call(tool, document_id=doc, target=['cyan', 'magenta', 'yellow', 'black'][k],
                     **arguments)
                call(tool, document_id=other, target=name, **arguments)
            got = rgb(other)
            only(base, plates(doc), k, [65535 - p[plane] for p in got], tool)
            assert any(g[plane] != b[plane] for g, b in
                       zip(got, base_rgb if other == rgb_doc else base_k)), (tool, 'changes the plane')
            undo(doc, other)
            p1[tool] = 'equals the twin'

        texts = [refused('fill_selection', PLATE_INK, document_id=doc, target='cyan',
                         ink=[0, 0, 0, 100]),
                 refused('apply_filter', CMYK_KEY, document_id=doc, filter='curves', target='cyan',
                         params={'cmyk_map': list(range(256))})]
        assert plates(doc) == base, 'a refusal changes nothing'
        p1['refusals'] = texts
        receipts['p1'] = p1
        print('PASS p1: a plate fills, buckets, inverts and equals the twin under the gradient, '
              'Levels, Curves and the Brush; ink and CMYK keys refuse by name', flush=True)

        # ---- P2: Fill and Stroke Path ------------------------------------------------------
        call('select_rect', document_id=doc, x=4, y=4, width=12, height=10)
        call('selection_to_path', document_id=doc)
        call('deselect', document_id=doc)
        far = plates(doc, [(24, 20)])
        call('fill_path', document_id=doc, path='Work Path', layer=0, ink=[0, 0, 0, 100])
        assert plates(doc, [(6, 6)])[0][:4] == [0, 0, 0, 65535], plates(doc, [(6, 6)])
        assert plates(doc, [(24, 20)]) == far
        call('undo', document_id=doc)
        before = plates(doc, [(6, 6)])[0]
        call('fill_path', document_id=doc, path='Work Path', target='cyan', color='#000000')
        after = plates(doc, [(6, 6)])[0]
        assert after[0] == 65535 and after[1:] == before[1:], (after, before)
        call('undo', document_id=doc)
        edge = plates(doc, [(4, 8)])[0]
        call('stroke_path', document_id=doc, path='Work Path', layer=0, ink=[100, 0, 0, 0], size=3)
        assert plates(doc, [(4, 8)])[0][0] > edge[0], (plates(doc, [(4, 8)]), edge)
        assert plates(doc, [(24, 20)]) == far
        call('undo', document_id=doc)
        assert plates(doc) == base
        receipts['p2'] = {'edge_before': edge}
        print('PASS p2: Fill Path and Stroke Path paint ink on the layer and grey on a plate',
              flush=True)

        # ---- P3: Apply Image and Calculations ----------------------------------------------
        call('apply_image', document_id=doc, target='cyan', source_plane='magenta')
        only(base, plates(doc), 0, [p[1] for p in base], 'apply_image cyan from magenta')
        undo(doc)
        call('apply_image', document_id=doc, target='layer', source_plane='cmyk',
             blend_mode='Multiply')
        call('apply_image', document_id=rgb_doc, target='layer', source_plane='rgb',
             blend_mode='Multiply')
        call('apply_image', document_id=k_doc, target='red', source_plane='red',
             blend_mode='Multiply')
        got, got_rgb, got_k = plates(doc), rgb(rgb_doc), rgb(k_doc)
        for i in range(len(ALL)):
            want = [65535 - got_rgb[i][0], 65535 - got_rgb[i][1], 65535 - got_rgb[i][2],
                    65535 - got_k[i][0], base[i][4]]
            assert got[i] == want, ('apply_image layer multiply', ALL[i], got[i], want)
        assert got != base, 'Multiply moved the ink'
        undo(doc, rgb_doc, k_doc)
        call('calculations', document_id=doc, source1_plane='cyan', source2_plane='black',
             blend_mode='Multiply', result='new_channel', name='Calc')
        call('calculations', document_id=k_doc, source1_plane='green', source2_plane='red',
             blend_mode='Multiply', result='new_channel', name='Calc')
        calc = channel(doc, 'Calc')
        assert calc == channel(k_doc, 'Calc'), 'Calculations equals the twin'
        assert len(set(calc)) > 1, 'the channel varies'
        receipts['p3'] = {'calc_distinct': len(set(calc))}
        print('PASS p3: Apply Image onto a plate and the layer, and Calculations from plates',
              flush=True)

        # ---- P4: a channel's Levels and the scalar histograms ------------------------------
        for d in (doc, rgb_doc):
            call('select_rect', document_id=d, x=6, y=4, width=14, height=12)
            call('modify_selection', document_id=d, operation='feather', radius=3)
            call('add_channel', document_id=d, **{'from': 'selection'}, name='Ch')
            call('deselect', document_id=d)
        ch = channel(doc, 'Ch')
        assert ch == channel(rgb_doc, 'Ch') and any(0 < v < 65535 for v in ch), 'feathered twins'
        for d in (doc, rgb_doc):
            call('apply_filter', document_id=d, filter='levels', target='channel:Ch',
                 output_white=0.5)
        leveled = channel(doc, 'Ch')
        assert leveled == channel(rgb_doc, 'Ch') and leveled != ch, 'channel Levels equals the twin'
        cyan = call('histogram', document_id=doc, source='scalar', target='cyan', selection=False)
        assert cyan['weighted_code_sums_rgba'][0] == sum((65535 - p[0]) * 65535 for p in base), cyan
        chist = call('histogram', document_id=doc, source='scalar', target='channel:Ch',
                     selection=False)
        assert chist['weighted_code_sums_rgba'][0] == sum(v * 65535 for v in leveled), chist
        receipts['p4'] = {'cyan_sum': cyan['weighted_code_sums_rgba'][0]}
        print('PASS p4: an alpha channel takes Levels; plate and channel histograms count their '
              'numbers', flush=True)

        # ---- P5: Gamut Warning ------------------------------------------------------------
        presses = call('list_installed_profiles', kinds=['cmyk'])['profiles']
        second = next((p for p in presses if p['proofable'] and p['name'] != 'Generic CMYK Profile'),
                      None)
        assert second, f'no second proofable CMYK profile is installed: {presses}'
        gdoc = call('open_document', path=str(gamut))['document']['id']
        proof = {'profile': 'installed', 'name': second['name']}
        (_, _, warned_px), warned = image('render_proof', document_id=gdoc, gamut_warning=True,
                                         preserve_numbers=False, **proof)
        (_, _, plain_px), plain = image('render_proof', document_id=gdoc, gamut_warning=False,
                                       preserve_numbers=False, **proof)
        _, kept = image('render_proof', document_id=gdoc, gamut_warning=True, preserve_numbers=True,
                        **proof)
        count = warned['out_of_gamut']
        assert 0 < count < 24 * 12, warned
        assert plain['out_of_gamut'] == count and warned['gamut_warning'] is True, (plain, warned)
        assert warned_px != plain_px, 'the warning overlays the flagged inks'
        assert kept['out_of_gamut'] == 0, kept
        call('close_document', document_id=gdoc, discard_changes=True)
        receipts['p5'] = {'press': second['name'], 'out_of_gamut': count}
        print(f'PASS p5: Gamut Warning flags {count} px against {second["name"]}; preserved '
              'numbers flag none', flush=True)

        # ---- P6: reduced regions -----------------------------------------------------------
        (fw, fh, full), _ = image('render', document_id=doc, region={'x': 0, 'y': 0, 'width': W,
                                  'height': H}, max_side=4096)
        assert (fw, fh) == (W, H), (fw, fh)
        (_, _, whole), _ = image('render', document_id=doc, max_side=4096)
        assert whole == full, 'region at divisor 1 is the full view'
        checked = 0
        for d in DIVISORS:
            for x, y, rw, rh in [(0, 0, W, H), (1, 2, W - 3, H - 2)]:
                (ow, oh, px), _ = image('render', document_id=doc,
                                        region={'x': x, 'y': y, 'width': rw, 'height': rh},
                                        preview_divisor=d, max_side=4096)
                assert (ow, oh) == (-(-rw // d), -(-rh // d)), (d, ow, oh)
                for j in range(oh):
                    for i in range(ow):
                        assert px[j][i] == full[y + j * d][x + i * d], (d, x, y, i, j)
                        checked += 1
        receipts['p6'] = {'pixels_checked': checked}
        print(f'PASS p6: divisor 2, 3 and 7 regions sample the full view ({checked} px)',
              flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True,
                                                     default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
