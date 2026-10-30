#!/usr/bin/env python3
"""The #1 Duotone MCP drive (phases 1a to 2b and the close): an 8-bit sGray document converts to
Duotone in one undo step, `render` shows the gray through the inks as the declared model says, the
inks can be edited later, Grayscale renders them into the gray, `.rz` keeps them, a Gray edit and a
16-bit source work, Actions replay the conversion, and what the mode does not hold refuses by its
sentence; on the Debug app.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the receipt and no bound is
widened afterwards. The instance is owned by this file (`phase5_support.App` on its own scratch
MCP socket: a dummy API key and a PRIVATE directory for every `DR_*_DIR`) and is terminated by its
own context manager; no Doctored process this file did not start is signalled. It never launches
Photoshop. Every fixture is written HERE in pure Python under `--output`.

The oracle (`oracle`) is written here from the status doc's declaration, not from the app: per
8-bit gray code g, k = 1 − g/255; each ink's coverage is its curve (a straight two-point curve, so
the natural spline is the line) at 100·k; c′ = 1 − sRGB_decode(1 − c) (sGray's tone curve); the
Neugebauer sum over every ink subset in D50 XYZ (CIE Lab → XYZ, paper Lab (100, 0, 0), a default
overprint the paper times the inks' ratios to it); the ICC sRGB D50 colorants inverted; clamped;
sRGB-encoded; 8-bit.

- **D1 convert.** A 12 × 2 sGray PNG of `CODES` opens as gray; `convert_mode` duotone with `TWO`
  answers color_mode duotone and duotone {type duotone, inks 2}; `get_document` reports
  color_mode duotone and the same duotone; undo answers gray, redo duotone (one undo step).
- **D2 the view.** `render` of D1's document is `oracle(TWO)` per pixel within ±2 8-bit codes.
- **D3 edit the inks.** `set_duotone` with `THREE` answers changed; `get_duotone` is tritone; one
  undo restores D1's `get_duotone` exactly.
- **D4 to Grayscale.** `convert_mode gray` on D1's document answers gray with no duotone;
  `gray_space` refuses with `GRAY_SPACE`; undo answers duotone with D2's `render` (D24 holds the
  converted gray).
- **D5 .rz.** `save_copy rz` of D1's document reopens as duotone with an equal `get_duotone` and
  an equal `render`.
- **D6 Gray edits and 16 bits.** On D1's document `select_rect` (0, 0, 3, 2) and `fill_selection`
  `#808080` answer ok and the document stays duotone with the filled pixels' render one colour
  and the rest unchanged; a 16-bit sGray PNG converts with `TWO` at depth 16 and renders within
  ±2 of the oracle at its codes' 8-bit values.
- **D7 refusals.** Exactly (phase 1b's and 2a's items no longer refuse; since phase 2b
  Merge to HDR is admitted and the profile and proof tools refuse by D17–D19 and D22; since the
  close Bitmap converts, D25): `convert_precision` rgba32 → `DEPTH_32`; `convert_mode duotone` on
  an RGB document → `SOURCE`; `get_duotone` on a Gray document → `NOT_DUOTONE`; five inks →
  `FIVE`.
- **D8 Actions.** `save_action` of the one `convert_mode` duotone step with `TWO`, then
  `run_action` on a fresh copy of the PNG: duotone, `get_duotone` equal to D1's, `render` equal.
- **D9 Monotone Black.** `MONOTONE_BLACK` renders every pixel within ±1 8-bit code of the
  pre-conversion Grayscale `render`.

Phase 1b (#1 Duotone phase 1b), each on a fresh 8-bit Duotone document of `TWO`:

- **D10 RGB.** `convert_mode rgb` answers color_mode rgb; its `render` is within ±`ROUTE_TOLERANCE`
  of the Duotone `render` per pixel; one undo answers duotone again.
- **D11 CMYK.** `convert_mode cmyk` the same way, its `render` held against the same route
  (`convert_mode rgb`, then `convert_mode cmyk`, of a fresh Duotone document), since the CMYK
  profile's own round trip moves dark inks by more than the bound (drive-1: 3 codes against the
  Duotone `render`, recorded beside it as `d11_against_duotone`).
- **D12 Lab.** `convert_mode lab` the same way.
- **D13 Indexed.** `convert_mode indexed` with palette exact the same way.
- **D14 exports.** `export_image` png (8 bits, profile document) answers ok with `duotone` ==
  `EXPORT_NOTE`, and the file's pixels are within ±`ROUTE_TOLERANCE` of the Duotone `render`;
  `export_image` webp with profile document answers ok with `duotone` == `EXPORT_NOTE`.
- **D15 PSD.** `export_psd`, then `open_document psd_mode layers`: color_mode duotone and the same
  `get_duotone` within the file's quantization (Lab within 0.01, curve points within 0.1 %), every
  overprint still null; a second `export_psd` of the reopened document writes colour-mode data
  equal byte for byte to the first file's (both read here by Adobe's framing), colour mode 8.
- **D16 .ado and the save.** `get_duotone path` writes an `.ado`; `set_duotone path` of it on a
  document set to `THREE` brings back `get_duotone` equal to D1's; `get_duotone` to the same path
  refuses without overwrite; `save_copy tiff` refuses with `SAVE`.

Phase 2a (#1 Duotone phase 2a), corrected by phase 2b (the `profile` and `proof` rows refuse),
each on a fresh 8-bit Duotone document of `TWO`:

- **D17 Assign Profile.** `assign_profile gray_gamma_22` answers isError with
  `DISABLED.format('assign_profile')` and `list_history`'s undo_count is unchanged.
- **D18 Convert to Profile.** `convert_profile gray_gamma_22` the same way.
- **D19 the proof.** `render_proof` (profile working_cmyk) and `render_proof exact:true` answer
  isError with `DISABLED.format('render_proof')` and the undo_count is unchanged.
- **D20 New from Clipboard.** `select_all`, `copy`, `new_from_clipboard`: the new document is
  color_mode gray with no duotone, 8 bits in sGray as the source (`get_precision`), and its
  `render` equals the pre-conversion Grayscale `render` exactly. The pasteboard is a private one (`DR_PASTEBOARD_NAME`).
- **D21 the row's name.** `get_duotone` answers channel_name Duotone, and Tritone after
  `set_duotone` with `THREE` (confirmed by the walk).

Phase 2b (#1 Duotone phase 2b):

- **D22 the walk's final table.** Read from `TAKE1` (`decided.json`; SKIP printed when absent):
  each `walk.final.photoshop_disabled_tools` entry, called with only `document_id` on a fresh
  Duotone document, answers isError with `DISABLED.format(tool)`; each `admitted_tools` entry
  answers no sentence naming Duotone.
- **D23 Merge to HDR.** On a two-layer Duotone document (`duplicate_layer` index 0, then
  `convert_mode duotone` with `TWO`) and its plain Grayscale twin, `merge_layers_to_hdr layers
  [0, 1] evs [0, 1]` gives the same outcome: both answer ok, or both the same refusal.

The close (#1, `designs/duotone-mode.md`), each on a fresh 8-bit Duotone document of `TWO`:

- **D24 Grayscale through the inks.** `convert_mode gray` answers gray with no duotone, and its
  `render` is, per pixel, `gray_oracle(TWO)` (the oracle's mix Y, clamped, sRGB-encoded: sGray's
  curve) within ±`GRAY_TOLERANCE` 8-bit codes; one undo restores the Duotone `render` exactly. With
  an adjustment layer (`add_adjustment_layer` levels) it refuses in-band naming flatten, and
  `flatten true` converts.
- **D25 Bitmap.** `convert_mode bitmap method threshold` answers bitmap, and its `render` equals
  exactly a fresh twin's taken through `convert_mode gray` then `convert_mode bitmap method
  threshold`; one undo answers duotone with the Duotone `render`. A 16-bit Duotone document refuses
  with `BITMAP` (the 8 Bits/Channel sentence); `convert_mode duotone` on a Bitmap document refuses
  and the document stays bitmap.
- **D26 the walk's closed paths.** Each of `CLOSED_TOOLS` (with only `document_id`) answers
  `DISABLED.format(tool)`, `apply_filter` with each of `CLOSED_FILTERS` answers
  `DISABLED.format(filter)`, `convert_precision rgba16` answers `SIXTEEN`, and the undo_count is
  unchanged.
- **D27 Proof Colors off.** On a Gray document `set_proof_setup proof_colors true`, then
  `convert_mode duotone`: `set_proof_setup proof_colors true` and `get_proof_setup` refuse by
  `DISABLED`, and `set_proof_setup proof_colors false` answers ok with the undo_count unchanged.
"""

import argparse
import base64
import json
import os
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
CODES = [0, 20, 45, 70, 96, 128, 150, 175, 200, 225, 245, 255]
WIDTH, HEIGHT = len(CODES), 2
LINE = lambda low, high: [low] + [None] * 11 + [high]
TWO = [{'name': 'Warm Black', 'lab': [20, 2, 4], 'curve': LINE(0, 100)},
       {'name': 'Rust', 'lab': [55, 45, 55], 'curve': LINE(10, 70)}]
THREE = TWO + [{'name': 'Lime', 'lab': [80, -25, 70], 'curve': LINE(0, 50)}]
MONOTONE_BLACK = [{'name': 'Black', 'lab': [0, 0, 0], 'curve': LINE(0, 100)}]
VIEW_TOLERANCE = 2
BLACK_TOLERANCE = 1
DISABLED = '{} is not available in Duotone.'
TAKE1 = Path.home() / 'devel/doctored-fixtures/photoshop/duotone-mode-547/take1/decided.json'
ROUTE_TOLERANCE = 2
BITMAP = ('Bitmap converts an 8-bit Grayscale document; convert to 8 '
          'Bits/Channel first.')
SIXTEEN = '16 Bits/Channel is not available in Duotone.'
GRAY_TOLERANCE = 2
# The close's ruling 3: the walk's unmapped paths with a Doctored tool, refused for good.
CLOSED_TOOLS = ['content_aware_fill', 'preview_content_aware_fill',
                'discard_content_aware_fill_preview', 'puppet_warp', 'perspective_warp',
                'select_subject', 'select_sky', 'depth_blur', 'super_zoom', 'jpeg_artifacts_removal',
                'neural_colorize']
CLOSED_FILTERS = ['develop_filter', 'lens_correction', 'tilt_shift']
SAVE = ('A Duotone document saves as .doctored, or as PSD or PSB through File ▸ Export as '
        'PSD; File ▸ Export renders its inks to RGB.')
EXPORT_NOTE = ('A Duotone document exports through its inks: the file is RGB, rendered as the canvas '
               'shows it.')
LAB_QUANTUM, CURVE_QUANTUM = 0.01, 0.1
DEPTH_32 = ('32 Bits/Channel is unavailable in a Duotone document: the 32-bit '
            'modes are RGB and Grayscale.')
SOURCE = ('Duotone converts an 8- or 16-bit Grayscale document; convert to '
          'Grayscale first.')
NOT_DUOTONE = ('get_duotone applies to a Duotone document, as Image ▸ Mode ▸ Duotone… does; '
               'convert_mode duotone converts a Grayscale document.')
FIVE = 'A Duotone holds 1 to 4 inks (Monotone, Duotone, Tritone or Quadtone), not 5.'
GRAY_SPACE = ("gray_space does not apply to a Duotone document: Grayscale keeps the document's own "
              'Gray profile.')

# ---- the oracle ------------------------------------------------------------
D50 = (0.9642, 1.0, 0.8249)
SRGB = ((0.436035, 0.385117, 0.143036),
        (0.222485, 0.716888, 0.060608),
        (0.013916, 0.097061, 0.713928))


def lab_xyz(lab):
    fy = (lab[0] + 16) / 116
    f = (fy + lab[1] / 500, fy, fy - lab[2] / 200)
    return tuple((t ** 3 if t ** 3 > 216 / 24389 else (116 * t - 16) * 27 / 24389) * w
                 for t, w in zip(f, D50))


def inverse(m):
    cof = [[m[(r + 1) % 3][(c + 1) % 3] * m[(r + 2) % 3][(c + 2) % 3]
            - m[(r + 1) % 3][(c + 2) % 3] * m[(r + 2) % 3][(c + 1) % 3] for c in range(3)]
           for r in range(3)]
    det = sum(m[0][c] * cof[0][c] for c in range(3))
    return [[cof[j][i] / det for j in range(3)] for i in range(3)]


TO_LINEAR = inverse(SRGB)


def decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def encode(y):
    return 12.92 * y if y <= 0.0031308 else 1.055 * y ** (1 / 2.4) - 0.055


def mix(g16, inks):
    """The inks' D50 XYZ mix at 16-bit gray code `g16` (straight curves, default overprints)."""
    k = 1 - g16 / 65535
    cover = [1 - decode(1 - (ink['curve'][0] + (ink['curve'][12] - ink['curve'][0]) * k) / 100)
             for ink in inks]
    paper = lab_xyz((100, 0, 0))
    colours = [lab_xyz(ink['lab']) for ink in inks]
    xyz = [0.0, 0.0, 0.0]
    for mask in range(1 << len(inks)):
        weight = 1.0
        primary = list(paper)
        for i, c in enumerate(cover):
            if mask & (1 << i):
                weight *= c
                primary = [p * colours[i][ch] / paper[ch] for ch, p in enumerate(primary)]
            else:
                weight *= 1 - c
        xyz = [x + weight * p for x, p in zip(xyz, primary)]
    return xyz


def oracle(g16, inks):
    """The declared view of 16-bit gray code `g16` under `inks` (straight curves, default
    overprints): 8-bit sRGB."""
    xyz = mix(g16, inks)
    linear = [sum(TO_LINEAR[i][j] * xyz[j] for j in range(3)) for i in range(3)]
    return tuple(round(encode(min(max(v, 0.0), 1.0)) * 255) for v in linear)


def gray_oracle(g16, inks):
    """Duotone → Grayscale's declared code of `g16` under `inks` in sGray: the mix's Y, clamped,
    through sGray's tone curve (the sRGB curve), 8-bit."""
    return round(encode(min(max(mix(g16, inks)[1], 0.0), 1.0)) * 255)


# ---- fixtures and the render decoder ---------------------------------------
def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def gray_png(path, codes, depth):
    """A `len(codes)` × HEIGHT sGray PNG (colour type 0, an `sRGB` chunk) at `depth`."""
    rows = b''
    for _ in range(HEIGHT):
        rows += b'\0' + b''.join(struct.pack('>H', v) if depth == 16 else bytes([v]) for v in codes)
    body = chunk(b'IHDR', struct.pack('>IIBBBBB', len(codes), HEIGHT, depth, 0, 0, 0, 0))
    body += chunk(b'sRGB', b'\0')
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + body + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def rgb_png(path):
    rows = b''.join(b'\0' + bytes([200, 40, 40]) * 4 for _ in range(2))
    body = chunk(b'IHDR', struct.pack('>IIBBBBB', 4, 2, 8, 2, 0, 0, 0))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + body + chunk(b'IDAT', zlib.compress(rows))
                     + chunk(b'IEND', b''))


def png_pixels(data):
    """RGBA tuples of an 8/16-bit non-interlaced PNG of colour type 0, 2, 4 or 6."""
    at, packed = 8, b''
    width = height = depth = colour = 0
    while at + 8 <= len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            width, height, depth, colour, _, _, _ = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            packed += body
        at += 12 + length
    channels = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
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
            if kind == 1:
                row[i] = (row[i] + left) & 255
            elif kind == 2:
                row[i] = (row[i] + up) & 255
            elif kind == 3:
                row[i] = (row[i] + (left + up) // 2) & 255
            elif kind == 4:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                row[i] = (row[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else corner)) & 255
        previous = row
        for x in range(width):
            px = row[x * unit:(x + 1) * unit]
            values = (tuple(v >> 8 for v in struct.unpack(f'>{channels}H', bytes(px)))
                      if depth == 16 else tuple(px))
            if channels == 1:
                values = values * 3 + (255,)
            elif channels == 2:
                values = values[:1] * 3 + values[1:]
            elif channels == 3:
                values = values + (255,)
            pixels.append(values)
    return pixels


def psd_mode_data(data):
    """A PSD's header colour mode and its colour-mode data, by Adobe's framing."""
    assert data[:4] == b'8BPS', data[:4]
    mode = struct.unpack('>H', data[24:26])[0]
    length = struct.unpack('>I', data[26:30])[0]
    return mode, data[30:30 + length]


def near(a, b, tolerance):
    return all(abs(x - y) <= tolerance for x, y in zip(a[:3], b[:3]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    output = Path(args.output).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    folder.mkdir(parents=True, exist_ok=False)
    receipts = {'expectations': 'fixed in scripts/verify_duotone_mode.py before any measurement'}
    # D20 copies: a private pasteboard, never the person's.
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.verify-duotone-mode.{os.getpid()}'

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def summary(doc):
            return next(d for d in call('list_documents')['documents'] if d['id'] == doc)

        def render(doc):
            result = app.client.rpc('tools/call', {'name': 'render',
                                                   'arguments': {'document_id': doc}})
            assert not result.get('isError'), result
            data = next(c['data'] for c in result['content'] if c['type'] == 'image')
            return png_pixels(base64.b64decode(data))

        def options(doc):
            reply = call('get_duotone', document_id=doc)
            reply.pop('document', None)
            return reply

        def opened(path):
            return call('open_document', path=str(path))['document']['id']

        eight = folder / 'gray8.png'
        gray_png(eight, CODES, 8)
        codes16 = [v * 257 for v in CODES] * HEIGHT

        # ---- D1 ------------------------------------------------------------
        doc = opened(eight)
        assert summary(doc)['color_mode'] == 'gray', summary(doc)
        gray_render = render(doc)
        reply = call('convert_mode', document_id=doc, mode='duotone', inks=TWO)
        assert reply['color_mode'] == 'duotone', reply
        assert reply['duotone'] == {'type': 'duotone', 'inks': 2}, reply
        info = call('get_document', document_id=doc)
        assert info['color_mode'] == 'duotone' and info['duotone'] == reply['duotone'], info
        call('undo', document_id=doc)
        assert summary(doc)['color_mode'] == 'gray'
        call('redo', document_id=doc)
        assert summary(doc)['color_mode'] == 'duotone'
        two_options = options(doc)
        receipts['d1'] = {'reply': {k: reply[k] for k in ('color_mode', 'duotone', 'depth')},
                          'options': two_options}
        print('PASS d1: Grayscale → Duotone is one undo step and get_document reports it',
              flush=True)

        # ---- D2 ------------------------------------------------------------
        view = render(doc)
        worst = 0
        for pixel, g16 in zip(view, codes16):
            expected = oracle(g16, TWO)
            worst = max(worst, max(abs(a - b) for a, b in zip(pixel[:3], expected)))
            assert near(pixel, expected, VIEW_TOLERANCE), (g16, pixel, expected)
        receipts['d2'] = {'worst': worst, 'render': view[:WIDTH]}
        print(f'PASS d2: the view is the declared Neugebauer mix within ±{VIEW_TOLERANCE} '
              f'(worst {worst})', flush=True)

        # ---- D3 ------------------------------------------------------------
        edit = call('set_duotone', document_id=doc, inks=THREE)
        assert edit['changed'] is True and edit['duotone'] == {'type': 'tritone', 'inks': 3}, edit
        assert options(doc)['type'] == 'tritone'
        call('undo', document_id=doc)
        assert options(doc) == two_options
        print('PASS d3: set_duotone edits the inks in one undo step', flush=True)

        # ---- D4 ------------------------------------------------------------
        refused = call('convert_mode', fail=True, document_id=doc, mode='gray',
                       gray_space='gray_gamma_22')
        assert refused == GRAY_SPACE, refused
        back = call('convert_mode', document_id=doc, mode='gray')
        assert back['color_mode'] == 'gray' and back.get('duotone') is None, back
        assert 'duotone' not in call('get_document', document_id=doc)
        call('undo', document_id=doc)
        assert summary(doc)['color_mode'] == 'duotone'
        assert render(doc) == view
        print('PASS d4: Duotone → Grayscale drops the inks in one undo step', flush=True)

        # ---- D5 ------------------------------------------------------------
        rz = folder / 'duotone.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        reopened = opened(rz)
        assert summary(reopened)['color_mode'] == 'duotone'
        assert options(reopened) == two_options
        assert render(reopened) == view
        print('PASS d5: .rz reopens the Duotone document with its inks', flush=True)

        # ---- D6 ------------------------------------------------------------
        call('select_rect', document_id=doc, x=0, y=0, width=3, height=2)
        call('fill_selection', document_id=doc, color='#808080')
        call('deselect', document_id=doc)
        filled = render(doc)
        assert summary(doc)['color_mode'] == 'duotone'
        inside = {filled[y * WIDTH + x][:3] for y in range(HEIGHT) for x in range(3)}
        assert len(inside) == 1, inside
        for y in range(HEIGHT):
            for x in range(3, WIDTH):
                assert filled[y * WIDTH + x] == view[y * WIDTH + x], (x, y)
        sixteen = folder / 'gray16.png'
        gray_png(sixteen, [v * 257 for v in CODES], 16)
        doc16 = opened(sixteen)
        reply16 = call('convert_mode', document_id=doc16, mode='duotone', inks=TWO)
        assert reply16['color_mode'] == 'duotone' and reply16['depth'] == 16, reply16
        for pixel, g16 in zip(render(doc16), codes16):
            assert near(pixel, oracle(g16, TWO), VIEW_TOLERANCE), (g16, pixel)
        receipts['d6'] = {'filled': sorted(inside)}
        print('PASS d6: a Gray edit runs on a Duotone document; a 16-bit source converts',
              flush=True)

        # ---- D7 ------------------------------------------------------------
        rgb = folder / 'rgb.png'
        rgb_png(rgb)
        rgb_doc = opened(rgb)
        plain = folder / 'plain.png'
        gray_png(plain, CODES, 8)
        plain_doc = opened(plain)
        refusals = {
            'convert_precision': (call('convert_precision', fail=True, document_id=doc,
                                       mode='rgba32', structure='preserve'), DEPTH_32),
            'rgb_source': (call('convert_mode', fail=True, document_id=rgb_doc, mode='duotone',
                                inks=TWO), SOURCE),
            'get_duotone': (call('get_duotone', fail=True, document_id=plain_doc), NOT_DUOTONE),
            'five_inks': (call('set_duotone', fail=True, document_id=doc, inks=THREE + TWO), FIVE),
        }
        for name, (actual, expected) in refusals.items():
            assert actual == expected, (name, actual, expected)
        receipts['d7'] = {name: actual for name, (actual, _) in refusals.items()}
        print('PASS d7: the mode refuses by its sentences', flush=True)

        # ---- D8 ------------------------------------------------------------
        call('save_action', name='verify-duotone-d8',
             steps=[{'tool': 'convert_mode', 'arguments': {'mode': 'duotone', 'inks': TWO}}])
        fresh = folder / 'fresh.png'
        gray_png(fresh, CODES, 8)
        fresh_doc = opened(fresh)
        call('run_action', document_id=fresh_doc, name='verify-duotone-d8')
        assert summary(fresh_doc)['color_mode'] == 'duotone'
        assert options(fresh_doc) == two_options
        assert render(fresh_doc) == view
        print('PASS d8: an Action replays convert_mode duotone', flush=True)

        # ---- D9 ------------------------------------------------------------
        black = folder / 'black.png'
        gray_png(black, CODES, 8)
        black_doc = opened(black)
        call('convert_mode', document_id=black_doc, mode='duotone', inks=MONOTONE_BLACK)
        worst = 0
        for pixel, gray in zip(render(black_doc), gray_render):
            worst = max(worst, max(abs(a - b) for a, b in zip(pixel[:3], gray[:3])))
            assert near(pixel, gray, BLACK_TOLERANCE), (pixel, gray)
        receipts['d9'] = {'worst': worst}
        print(f'PASS d9: Monotone Black is the Grayscale display within ±{BLACK_TOLERANCE} '
              f'(worst {worst})', flush=True)

        # ---- phase 1b --------------------------------------------------------
        def fresh(name):
            path = folder / f'{name}.png'
            gray_png(path, CODES, 8)
            fresh_id = opened(path)
            call('convert_mode', document_id=fresh_id, mode='duotone', inks=TWO)
            return fresh_id

        def worst_of(a, b):
            assert len(a) == len(b), (len(a), len(b))
            return max(max(abs(x - y) for x, y in zip(p[:3], q[:3])) for p, q in zip(a, b))

        for case, mode, extra in (('d10', 'rgb', {}), ('d11', 'cmyk', {}), ('d12', 'lab', {}),
                                  ('d13', 'indexed', {'palette': 'exact'})):
            doc_n = fresh(case)
            duotone_view = render(doc_n)
            reply = call('convert_mode', document_id=doc_n, mode=mode, **extra)
            assert reply['color_mode'] == mode, reply
            assert summary(doc_n)['color_mode'] == mode
            converted = render(doc_n)
            reference = duotone_view
            if mode == 'cmyk':
                # The CMYK profile's own round trip moves dark inks by more than the bound
                # (drive-1: 3 codes), so D11 holds the route itself: RGB Color from Duotone, then
                # the same CMYK conversion of that RGB document.
                twin = fresh('d11-rgb')
                call('convert_mode', document_id=twin, mode='rgb')
                call('convert_mode', document_id=twin, mode='cmyk')
                reference = render(twin)
                receipts['d11_against_duotone'] = worst_of(converted, duotone_view)
            worst = worst_of(converted, reference)
            assert worst <= ROUTE_TOLERANCE, (case, worst, converted, reference)
            call('undo', document_id=doc_n)
            assert summary(doc_n)['color_mode'] == 'duotone', (case, summary(doc_n))
            assert render(doc_n) == duotone_view
            receipts[case] = {'worst': worst, 'reply': {k: reply.get(k) for k in
                                                        ('color_mode', 'depth', 'profile')}}
            print(f'PASS {case}: Duotone → {mode} through the inks, one undo step (worst {worst})',
                  flush=True)

        # ---- D14 -----------------------------------------------------------
        doc14 = fresh('d14')
        view14 = render(doc14)
        png_path = folder / 'd14-export.png'
        reply = call('export_image', document_id=doc14, path=str(png_path),
                     settings={'format': 'png', 'bit_depth': 8, 'profile': 'document'})
        assert reply.get('ok') is True and reply.get('duotone') == EXPORT_NOTE, reply
        exported = png_pixels(png_path.read_bytes())
        worst = worst_of(exported, view14)
        assert worst <= ROUTE_TOLERANCE, (worst, exported, view14)
        webp = call('export_image', document_id=doc14, path=str(folder / 'd14.webp'),
                    settings={'format': 'webp', 'bit_depth': 8, 'profile': 'document'})
        assert webp.get('ok') is True and webp.get('duotone') == EXPORT_NOTE, webp
        assert (folder / 'd14.webp').read_bytes()[8:12] == b'WEBP'
        receipts['d14'] = {'worst': worst, 'png': {k: reply.get(k) for k in ('format', 'profile')},
                           'webp': {k: webp.get(k) for k in ('format', 'profile', 'bytes')}}
        print(f'PASS d14: exports go through the inks and say so (png worst {worst})', flush=True)

        # ---- D15 -----------------------------------------------------------
        doc15 = fresh('d15')
        before = options(doc15)
        first = folder / 'd15.psd'
        call('export_psd', document_id=doc15, path=str(first))
        report = call('inspect_psd', path=str(first))
        assert report['import_precision'] == 'gray' and report['color_mode'] == 'Duotone', report
        reopened15 = call('open_document', path=str(first), psd_mode='layers',
                          psd_fingerprint=report['source_fingerprint'])['document']['id']
        assert summary(reopened15)['color_mode'] == 'duotone'
        after = options(reopened15)
        assert after['type'] == before['type'], (before, after)
        assert all(v is None for v in after['overprints'].values()), after
        for ink_before, ink_after in zip(before['inks'], after['inks']):
            assert ink_before['name'] == ink_after['name']
            assert all(abs(a - b) <= LAB_QUANTUM + 1e-9
                       for a, b in zip(ink_before['lab'], ink_after['lab'])), (ink_before, ink_after)
            for a, b in zip(ink_before['curve'], ink_after['curve']):
                assert (a is None) == (b is None) and (a is None or abs(a - b) <= CURVE_QUANTUM + 1e-9)
        second = folder / 'd15-again.psd'
        call('export_psd', document_id=reopened15, path=str(second))
        mode_a, data_a = psd_mode_data(first.read_bytes())
        mode_b, data_b = psd_mode_data(second.read_bytes())
        assert mode_a == mode_b == 8 and data_a == data_b and len(data_a) == 524, (mode_a, mode_b)
        receipts['d15'] = {'mode_data_bytes': len(data_a), 'options': after}
        print('PASS d15: a Duotone PSD round-trips its inks and its colour-mode data', flush=True)

        # ---- D16 -----------------------------------------------------------
        doc16 = fresh('d16')
        ado = folder / 'two.ado'
        written = call('get_duotone', document_id=doc16, path=str(ado))
        assert written.get('path') == str(ado) and ado.is_file(), written
        refused = call('get_duotone', fail=True, document_id=doc16, path=str(ado))
        assert 'overwrite' in refused, refused
        call('set_duotone', document_id=doc16, inks=THREE)
        assert options(doc16)['type'] == 'tritone'
        loaded = call('set_duotone', document_id=doc16, path=str(ado))
        assert loaded['changed'] is True, loaded
        assert options(doc16) == two_options, (options(doc16), two_options)
        save = call('save_copy', fail=True, document_id=doc16, path=str(folder / 'd16.tif'),
                    format='tiff')
        assert save == SAVE, save
        receipts['d16'] = {'ado_bytes': ado.stat().st_size, 'save': save}
        print('PASS d16: .ado saves and loads; a TIFF save refuses by its sentence', flush=True)

        # ---- phase 2a --------------------------------------------------------
        def undo_count(doc_id):
            return int(call('list_history', document_id=doc_id)['undo_count'])

        # ---- D17, D18 ------------------------------------------------------
        for case, tool in (('d17', 'assign_profile'), ('d18', 'convert_profile')):
            doc_n = fresh(case)
            before_count = undo_count(doc_n)
            refused = call(tool, fail=True, document_id=doc_n, profile='gray_gamma_22')
            assert refused == DISABLED.format(tool), (case, refused)
            assert undo_count(doc_n) == before_count, (case, undo_count(doc_n), before_count)
            assert summary(doc_n)['color_mode'] == 'duotone'
            receipts[case] = {'refusal': refused}
            print(f'PASS {case}: {tool} refuses on Duotone', flush=True)

        # ---- D19 -----------------------------------------------------------
        doc19 = fresh('d19')
        before19 = undo_count(doc19)
        proofed = call('render_proof', fail=True, document_id=doc19, profile='working_cmyk')
        exact = call('render_proof', fail=True, document_id=doc19, profile='working_cmyk',
                     exact=True)
        assert proofed == exact == DISABLED.format('render_proof'), (proofed, exact)
        assert undo_count(doc19) == before19, (undo_count(doc19), before19)
        receipts['d19'] = {'refusal': proofed}
        print('PASS d19: render_proof refuses on Duotone',
              flush=True)

        # ---- D20 -----------------------------------------------------------
        doc20 = fresh('d20')
        call('select_all', document_id=doc20)
        call('copy', document_id=doc20, layer=0)
        call('deselect', document_id=doc20)
        made = call('new_from_clipboard')
        new20 = made['document']['id']
        assert summary(new20)['color_mode'] == 'gray', summary(new20)
        assert 'duotone' not in call('get_document', document_id=new20)
        assert render(new20) == gray_render
        source20 = call('get_precision', document_id=doc20)
        landed20 = call('get_precision', document_id=new20)
        assert landed20['bits_per_channel'] == source20.get('bits_per_channel') == 8, (
            source20, landed20)
        assert landed20['mode_description'] == 'Grayscale, 8-bit, sGray', landed20
        receipts['d20'] = {k: made.get(k) for k in ('typed', 'origin_depth', 'depth',
                                                    'profile_description')}
        receipts['d20']['landed'] = landed20['mode_description']
        print('PASS d20: New from Clipboard of a Duotone copy is a Grayscale document of its gray',
              flush=True)

        # ---- D21 -----------------------------------------------------------
        doc21 = fresh('d21')
        named = options(doc21)['channel_name']
        assert named == 'Duotone', named
        call('set_duotone', document_id=doc21, inks=THREE)
        tritone = options(doc21)['channel_name']
        assert tritone == 'Tritone', tritone
        receipts['d21'] = {'two': named, 'three': tritone}
        print('PASS d21: the Channels row is named after the type', flush=True)

        # ---- D22 -----------------------------------------------------------
        if not TAKE1.is_file():
            print(f'SKIP d22: {TAKE1} is absent', flush=True)
            receipts['d22'] = 'SKIP'
        else:
            final = json.loads(TAKE1.read_text())['walk']['final']
            doc22 = fresh('d22')
            answers = {}
            for tool in final['photoshop_disabled_tools']:
                refused = call(tool, fail=True, document_id=doc22)
                assert refused == DISABLED.format(tool), (tool, refused)
                answers[tool] = refused
            for tool in final['admitted_tools']:
                result = app.client.rpc('tools/call', {'name': tool,
                                                       'arguments': {'document_id': doc22}})
                text = next((c['text'] for c in result.get('content', [])
                             if c['type'] == 'text'), '')
                assert 'Duotone' not in text, (tool, text)
                answers[tool] = {'isError': bool(result.get('isError')), 'text': text[:200]}
            receipts['d22'] = {'final': final, 'answers': answers}
            print(f"PASS d22: the walk's {len(final['photoshop_disabled_tools'])} disabled tools "
                  f"refuse and its {len(final['admitted_tools'])} admitted tools do not",
                  flush=True)

        # ---- D23 -----------------------------------------------------------
        def hdr_outcome(doc_id):
            result = app.client.rpc('tools/call', {'name': 'merge_layers_to_hdr', 'arguments': {
                'document_id': doc_id, 'layers': [0, 1], 'evs': [0, 1]}})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return ('refused', text) if result.get('isError') else ('ok', None)

        duo23 = folder / 'd23.png'
        gray_png(duo23, CODES, 8)
        doc23 = opened(duo23)
        call('duplicate_layer', document_id=doc23, index=0)
        call('convert_mode', document_id=doc23, mode='duotone', inks=TWO)
        twin23 = folder / 'd23-gray.png'
        gray_png(twin23, CODES, 8)
        gray23 = opened(twin23)
        call('duplicate_layer', document_id=gray23, index=0)
        duotone_hdr, gray_hdr = hdr_outcome(doc23), hdr_outcome(gray23)
        assert duotone_hdr == gray_hdr, (duotone_hdr, gray_hdr)
        receipts['d23'] = {'duotone': duotone_hdr, 'gray': gray_hdr}
        print(f'PASS d23: Merge to HDR answers on Duotone as on Grayscale ({duotone_hdr[0]})',
              flush=True)

        # ---- D24 -----------------------------------------------------------
        doc24 = fresh('d24')
        view24 = render(doc24)
        reply = call('convert_mode', document_id=doc24, mode='gray')
        assert reply['color_mode'] == 'gray' and reply.get('duotone') is None, reply
        assert 'duotone' not in call('get_document', document_id=doc24)
        toned = render(doc24)
        worst = 0
        for pixel, g16 in zip(toned, codes16):
            expected = gray_oracle(g16, TWO)
            worst = max(worst, max(abs(c - expected) for c in pixel[:3]))
            assert all(abs(c - expected) <= GRAY_TOLERANCE for c in pixel[:3]), (g16, pixel, expected)
        call('undo', document_id=doc24)
        assert summary(doc24)['color_mode'] == 'duotone', summary(doc24)
        assert render(doc24) == view24
        adjusted24 = fresh('d24-adjusted')
        call('add_adjustment_layer', document_id=adjusted24, name='Levels', op='levels',
             params={'gamma': 1.2})
        blocked = call('convert_mode', fail=True, document_id=adjusted24, mode='gray')
        assert 'flatten' in blocked, blocked
        assert summary(adjusted24)['color_mode'] == 'duotone'
        flattened = call('convert_mode', document_id=adjusted24, mode='gray', flatten=True)
        assert flattened['color_mode'] == 'gray', flattened
        receipts['d24'] = {'worst': worst, 'blocked': blocked}
        print(f"PASS d24: Duotone → Grayscale is the inks' luminance within ±{GRAY_TOLERANCE} "
              f'(worst {worst}), one undo step; a blocker needs flatten', flush=True)

        # ---- D25 -----------------------------------------------------------
        doc25 = fresh('d25')
        view25 = render(doc25)
        reply = call('convert_mode', document_id=doc25, mode='bitmap', method='threshold')
        assert reply['color_mode'] == 'bitmap', reply
        bitmap25 = render(doc25)
        twin25 = fresh('d25-twin')
        call('convert_mode', document_id=twin25, mode='gray')
        call('convert_mode', document_id=twin25, mode='bitmap', method='threshold')
        assert render(twin25) == bitmap25
        call('undo', document_id=doc25)
        assert summary(doc25)['color_mode'] == 'duotone', summary(doc25)
        assert render(doc25) == view25
        doc25_16 = opened(sixteen)
        call('convert_mode', document_id=doc25_16, mode='duotone', inks=TWO)
        refused16 = call('convert_mode', fail=True, document_id=doc25_16, mode='bitmap',
                         method='threshold')
        assert refused16 == BITMAP, refused16
        bitmap_path = folder / 'd25-bitmap.png'
        gray_png(bitmap_path, CODES, 8)
        bitmap_doc = opened(bitmap_path)
        call('convert_mode', document_id=bitmap_doc, mode='bitmap', method='threshold')
        refused_back = call('convert_mode', fail=True, document_id=bitmap_doc, mode='duotone',
                            inks=TWO)
        assert summary(bitmap_doc)['color_mode'] == 'bitmap', summary(bitmap_doc)
        receipts['d25'] = {'sixteen': refused16, 'bitmap_to_duotone': refused_back}
        print('PASS d25: Duotone → Bitmap is Grayscale then Bitmap, one undo step; 16 bits and '
              'Bitmap → Duotone refuse', flush=True)

        # ---- D26 -----------------------------------------------------------
        doc26 = fresh('d26')
        before26 = undo_count(doc26)
        answers = {}
        for tool in CLOSED_TOOLS:
            answers[tool] = call(tool, fail=True, document_id=doc26)
            assert answers[tool] == DISABLED.format(tool), (tool, answers[tool])
        for name in CLOSED_FILTERS:
            answers[name] = call('apply_filter', fail=True, document_id=doc26, filter=name)
            assert answers[name] == DISABLED.format(name), (name, answers[name])
        answers['rgba16'] = call('convert_precision', fail=True, document_id=doc26, mode='rgba16')
        assert answers['rgba16'] == SIXTEEN, answers['rgba16']
        assert undo_count(doc26) == before26, (undo_count(doc26), before26)
        receipts['d26'] = answers
        print(f'PASS d26: the walk\'s {len(answers)} closed paths refuse by their sentences',
              flush=True)

        # ---- D27 -----------------------------------------------------------
        proof_path = folder / 'd27.png'
        gray_png(proof_path, CODES, 8)
        doc27 = opened(proof_path)
        on = call('set_proof_setup', document_id=doc27, proof_colors=True)
        assert on.get('ok') is True, on
        call('convert_mode', document_id=doc27, mode='duotone', inks=TWO)
        before27 = undo_count(doc27)
        again = call('set_proof_setup', fail=True, document_id=doc27, proof_colors=True)
        assert again == DISABLED.format('set_proof_setup'), again
        read = call('get_proof_setup', fail=True, document_id=doc27)
        assert read == DISABLED.format('get_proof_setup'), read
        off = call('set_proof_setup', document_id=doc27, proof_colors=False)
        assert off.get('ok') is True, off
        assert off.get('proof_colors') is False, off
        assert undo_count(doc27) == before27, (undo_count(doc27), before27)
        receipts['d27'] = {'on_again': again, 'read': read,
                           'off': {k: off.get(k) for k in ('ok', 'proof_colors')}}
        print('PASS d27: a Duotone document converted with Proof Colors on can turn it off',
              flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, default=str))
    print('verify_duotone_mode: every D-case PASS', flush=True)


if __name__ == '__main__':
    main()
