#!/usr/bin/env python3
"""The #1 spot channels MCP drive (phases 1a-2 and 1c-1): on an 8-bit RGB
document a spot channel is added from the selection, painted, edited, undone and
redone, saved and reopened as `.rz`, composited into a flattened PNG and into
`render` (full, region and preview_divisor) as its overprint, kept by a TIFF
(1c-2a) and a layered TIFF (1c-2b) and refused by name in a PDF; on a 16-bit document it is
added and its flattened PNG composites it; a default-ink spot channel shows; Merge
Spot Channel flattens and bakes it, and undoes; an alpha channel becomes a spot
channel and back.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python and every PNG read back by this
file's own reader (stdlib `zlib`). The fixture `base.png` (32 x 24, opaque,
untagged so sRGB) holds R = 13 + 7x, G = 9 + 9y, B = 201 - 5x. Coverage is
read by loading the spot channel (`load_selection from channel`) and reading
`get_selection`'s byte `coverage` at every pixel (`ALL`).

THE ORACLE (Doctored's declared overprint, `spot_overprint.rs`): per plane, in
unit values, w = cov / 255, s = solidity / 100,
`out = round(255 · ((1 − w)·c + w·((1 − s)·c·ink + s·ink)))`. A black ink
(Lab 0, 0, 0) is ink 0, so out = round((1 − w)·c) at any solidity. A red ink's
sRGB is this file's own D50 Lab → XYZ → the ICC sRGB matrix (D50-adapted
primaries) → the sRGB curve, clamped.

- **S1 add.** select_rect (4, 4, 12, 10); add_spot_channel "Spot A" lab
  [0, 0, 0] solidity 0. The list_channels row and get_document's row carry
  spot {lab [0, 0, 0], solidity 0}; the channel reads 255 inside the rectangle
  and 0 outside.
- **S2 paint.** deselect; brush_stroke target "channel:Spot A" (18, 18) to
  (28, 18) size 4 colour white opacity 1: the coverage at (23, 18) rises from
  0, and inside the rectangle it stays 255.
- **S3 set, undo, redo.** set_spot_channel solidity 60: the row says 60; undo:
  0; redo: 60.
- **S4 .rz.** save_copy rz, open_document: the reopened row's spot is
  {lab [0, 0, 0], solidity 60} and its coverage equals the original's at ALL.
- **S5 overprint.** save_copy png of the S3 document equals the black oracle
  within 1 code at ALL, and a pixel at coverage 0 is exactly the fixture's. On
  a second copy, select_rect (6, 5, 14, 10) feathered 2 and add_spot_channel
  "Red" lab [54.29, 80.81, 69.89] solidity 60: its save_copy png equals the
  red oracle within 2 codes at ALL and a coverage-0 pixel is exact; render at
  full size equals that PNG within 1 code and its text names "1 spot channel
  overprinted"; a region render (0, 0, 16, 12) equals the full render's crop
  EXACTLY and a preview_divisor 2 render equals its top-left samples EXACTLY
  (output (i, j) is canvas (2i, 2j)), each text naming "1 spot channel
  overprinted" (1c-1).
- **S6 refusals and the disclosure.** save_copy tif succeeds (1c-2a; S13
  measures it); export_psd psd succeeds, and so does its layered TIFF
  (export_psd format tiff mode layers, 1c-2b; S14 measures it); export_pdf
  names "Spot channels are not saved to PDF: Doctored's PDF
  cannot write them as separate inks" (1c-2a's PDF-FORM, SEPARATIONS); the S5 PNG
  copy's flat_save_losses holds "spot channels (composited into the image as
  their overprint)"; export_image png's reply has spot_channels.
- **S7 16 bits.** On a fresh copy, convert_precision rgba16 flatten; select_rect
  and add_spot_channel: list_channels' row has spot; save_copy png succeeds
  (1c-1 composites it; S11 measures it). convert_mode indexed of the 8-bit S5
  "Red" document names
  "Indexed Color has no spot channels" (a 16-bit document refuses Indexed on
  its depth first, so the 8-bit one carries this gate).
- **S8 the default ink shows.** On a fresh copy, select_rect (8, 6, 10, 8) and
  add_spot_channel with no lab or solidity: the reply's solidity is 0, and the
  full render inside the selection equals the oracle for the reply's lab
  within 2 codes and differs from the fixture there; a pixel outside is
  exactly the fixture's.
- **S9 Merge Spot Channel (1c-1).** On the S5 "Red" document, new_layer (an
  empty one: 2 layers), merge_spot_channel "Red": the reply says 1 layer and 0
  channels, list_channels has no "Red", and save_copy png equals the red oracle
  within 2 codes at ALL; one undo restores the channel with its ink, the 2
  layers and S5's full render exactly.
- **S10 alpha ↔ spot (1c-1).** On a fresh copy, select_rect (5, 3, 16, 12)
  feathered 2, add_channel from the selection "Mask"; set_channel_kind spot
  (the reply and the row carry the ink), then alpha (spot null); one undo gives
  the spot ink again and its coverage is 255 − the alpha coverage at ALL;
  set_channel_kind alpha again gives the original coverage at ALL.
- **S11 a 16-bit PNG's overprint (1c-1).** On a fresh copy, convert_precision
  rgba16 flatten, select_rect (6, 5, 14, 10) unfeathered, add_spot_channel
  "Red" lab [54.29, 80.81, 69.89] solidity 60; save_copy png bit_depth 16 (read
  back by this file's 16-bit reader): inside the rect within 514 codes (2 at
  8 bits) of `65535 · ((1 − s)·c·ink + s·ink)`, outside exactly the fixture ×
  257, and flat_save_losses holds the spot loss line.
- **S12 convert_precision keeps the channels (#1).** On a fresh copy,
  select_rect (5, 3, 16, 12) feathered 2, add_channel from the selection
  "Mask", set_channel_options overlay_color #00c800, overlay_opacity 0.35,
  color_indicates selected; deselect; select_rect (6, 5, 14, 10),
  add_spot_channel "Red" lab [54.29, 80.81, 69.89] solidity 60; deselect. The
  list_channels rows (index, name, overlay_color, overlay_opacity,
  color_indicates, spot) and both coverages at ALL are recorded. Phase A (no
  coverage read between a conversion and the undos, because a coverage read
  adds two selection undo steps): convert_precision rgba16 flatten lists
  saved_channels in conversion.preserved and not in conversion.discarded,
  get_document's precision is rgba16 and the rows are equal; rgba8 flatten the
  same with rgba8; undo gives rgba16 with equal rows, undo again rgba8 with
  equal rows; then both coverages equal the originals at ALL. Phase B (no undo
  after): rgba16 flatten again, both coverages equal the originals at ALL;
  rgba8 flatten again, both coverages equal the originals at ALL.
- **S13 a TIFF keeps the spot channel (#1 1c-2a).** On a fresh 8-bit copy,
  select_rect (6, 5, 14, 10) feathered 2, add_spot_channel "Red" lab
  [54.29, 80.81, 69.89] solidity 60, deselect; its coverage at ALL is recorded.
  save_copy tif succeeds, its flat_save_losses lacks the spot loss line and its
  reply has working_memory_plan_bytes; open_document of it lists exactly one
  spot channel, "Red", with spot {lab [54.29, 80.81, 69.89], solidity 60}, and
  its coverage equals the source's at ALL. Undo on the source (at most 8 steps,
  the coverage reads' selection steps included) until it lists no spot
  channel; a second save_copy tif reopens with no spot channel. Then the same
  on a fresh copy after convert_precision rgba16 flatten, with save_copy
  bit_depth 16.
- **S14 a layered TIFF keeps the spot channel (#1 1c-2b).** On fresh
  copies built as S13's sources (8 bits, and after convert_precision rgba16
  flatten): export_psd format tiff mode layers succeeds and its observations
  name "1 spot channels are written as Photoshop spot channels"; inspect_psd
  of the file lists the observation "1 spot channels open as spot channels
  with their names, inks (D50 Lab) and solidity: “Red”."; open_document with
  psd_mode layers and inspect_psd's source_fingerprint lists exactly one spot
  channel, "Red", with spot {lab [54.29, 80.81, 69.89], solidity 60}, and its
  coverage equals the source's at ALL.
- **S15 a 32-bit document (#1).** On a fresh copy, convert_precision rgba32
  structure preserve; render (full, and exposure 1) before any spot channel is
  the spotless display. select_rect (6, 5, 14, 10) feathered 2 and
  add_spot_channel "Ink" lab [60, 30, -20] (a mid ink: no channel at 0 or 1,
  so its encoded and linear values differ) solidity 60. (a) render equals the
  oracle over the spotless render within 2 codes at ALL, its text naming "1
  spot channel overprinted" and "32-bit float"; (b) a region render equals the
  full render's crop and a preview_divisor 2 render its top-left samples,
  EXACTLY; (c) render exposure 1 equals the oracle over the spotless render at
  exposure 1 within 2 codes; (d) save_copy png equals the same oracle as (a)
  within 2 codes (the file keeps what the view shows) and lists the spot loss
  line; (e) merge_spot_channel "Ink" leaves 1 layer and 0 channels, and its
  render equals (d)'s PNG within 1 code and the oracle within 2; one undo
  gives the channel back with its ink and (a)'s render exactly.
- **S16 a 32-bit Grayscale document (#1).** On a fresh copy, convert_precision
  rgba32 structure preserve and convert_mode gray gray_space gray_gamma_22
  (precision gray32); render before any spot channel; the same feathered
  selection and add_spot_channel "Grey" lab [50, 0, 0] solidity 60. The render
  is the twin's codes converted to sRGB, so its oracle runs on the twin's
  values: per plane `t = sRGB⁻¹(c)^(1/2.2)`, the ink `Y(L* 50)^(1/2.2)`, the
  overprint on `t`, then `sRGB((·)^2.2)`; the render after equals it within 3
  codes at ALL (two 8-bit roundings and the display conversion), its text
  naming "1 spot channel overprinted". merge_spot_channel "Grey", then render
  equals the render before the merge within 3 codes; one undo gives that
  render exactly.

S15 and S16 record every sub-check, print each section's failures, and assert
once after both, so a run lists every failed bound together.
"""
import argparse
import base64
import json
from pathlib import Path
import struct
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
W, H = 32, 24
ALL = [(x, y) for y in range(H) for x in range(W)]
RECT = (4, 4, 12, 10)
RED_RECT = (6, 5, 14, 10)
DEFAULT_RECT = (8, 6, 10, 8)
BLACK = [0.0, 0.0, 0.0]
RED = [54.29, 80.81, 69.89]
WRITTEN = '1 spot channels are written as Photoshop spot channels'
OPENED = ('1 spot channels open as spot channels with their names, inks (D50 Lab) and '
          'solidity: “Red”.')
PDF_REFUSAL = "Spot channels are not saved to PDF: Doctored's PDF cannot write them as separate inks"
UNDO_LIMIT = 8
INDEXED_REFUSAL = 'Indexed Color has no spot channels'
LOSS = 'spot channels (composited into the image as their overprint)'
SPOT_NOTE = '1 spot channel overprinted'
REGION = (0, 0, 16, 12)
KIND_RECT = (5, 3, 16, 12)
BLACK_BOUND = 1
RED_BOUND = 2
RENDER_BOUND = 1
DEEP_BOUND = 514
MASK_OVERLAY = '#00c800'
MASK_OPACITY = 0.35
ROW_KEYS = ('index', 'name', 'overlay_color', 'overlay_opacity', 'color_indicates', 'spot')
FLOAT_INK = [60.0, 30.0, -20.0]
GREY_INK = [50.0, 0.0, 0.0]
FLOAT_NOTE = '32-bit float'
GRAY_BOUND = 3

D50 = (0.9642, 1.0, 0.8249)
# The ICC sRGB profile's D50-adapted primaries (XYZ = M · linear RGB).
SRGB_D50 = [[0.4360747, 0.3850649, 0.1430804],
            [0.2225045, 0.7168786, 0.0606169],
            [0.0139322, 0.0971045, 0.7141733]]


def base_pixel(x, y):
    return (13 + 7 * x, 9 + 9 * y, 201 - 5 * x)


def png_bytes(width, height, rgba):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

    rows = b''.join(b'\0' + rgba[y * width * 4:(y + 1) * width * 4] for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def fixture():
    out = bytearray()
    for x, y in ALL:
        out += bytes(base_pixel(x, y)) + b'\xff'
    return png_bytes(W, H, bytes(out))


def read_png(data):
    """8-bit RGB or RGBA PNG → [(r, g, b)] row-major; every filter type."""
    depth, pixels = decode_png(data)
    assert depth == 8, depth
    return pixels


def read_png16(data):
    """16-bit RGB or RGBA PNG → [(r, g, b)] in 16-bit codes, row-major."""
    depth, pixels = decode_png(data)
    assert depth == 16, depth
    return pixels


def decode_png(data):
    """8- or 16-bit RGB or RGBA PNG → (depth, [(r, g, b)]) row-major; every filter type."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'not a PNG'
    at, idat, header = 8, b'', None
    while at < len(data):
        length, kind = struct.unpack('>I4s', data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat += body
        at += 12 + length
    width, height, depth, colour, _, _, interlace = header
    assert depth in (8, 16) and colour in (2, 6) and interlace == 0, header
    channels = 4 if colour == 6 else 3
    size = depth // 8
    bpp = channels * size
    raw = zlib.decompress(idat)
    stride = width * bpp
    rows, previous, at = [], bytearray(stride), 0
    for _ in range(height):
        kind, line = raw[at], bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            left = line[i - bpp] if i >= bpp else 0
            up = previous[i]
            corner = previous[i - bpp] if i >= bpp else 0
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + (left + up) // 2) & 255
            elif kind == 4:
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                pred = left if pa <= pb and pa <= pc else up if pb <= pc else corner
                line[i] = (line[i] + pred) & 255
        rows.append(line)
        previous = line

    def sample(row, x, c):
        at = (x * channels + c) * size
        return row[at] if size == 1 else (row[at] << 8) | row[at + 1]

    return depth, [tuple(sample(rows[y], x, c) for c in range(3))
                   for y in range(height) for x in range(width)]


def lab_to_srgb(lab):
    """D50 Lab → encoded sRGB in [0, 1] through the ICC sRGB matrix."""
    L, a, b = lab
    fy = (L + 16) / 116
    fx, fz = fy + a / 500, fy - b / 200

    def finv(t):
        return t ** 3 if t > 6 / 29 else 3 * (6 / 29) ** 2 * (t - 4 / 29)

    xyz = [D50[0] * finv(fx), D50[1] * finv(fy), D50[2] * finv(fz)]
    m = SRGB_D50
    det = (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
           - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
           + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]))
    inv = [[(m[(j + 1) % 3][(i + 1) % 3] * m[(j + 2) % 3][(i + 2) % 3]
             - m[(j + 1) % 3][(i + 2) % 3] * m[(j + 2) % 3][(i + 1) % 3]) / det
            for j in range(3)] for i in range(3)]
    linear = [min(1.0, max(0.0, sum(inv[r][c] * xyz[c] for c in range(3)))) for r in range(3)]
    return [12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055 for v in linear]


def oracle(c, cov, ink, solidity):
    w, s = cov / 255, solidity / 100
    out = []
    for plane in range(3):
        cu = c[plane] / 255
        blend = (1 - s) * cu * ink[plane] + s * ink[plane]
        out.append(int(255 * ((1 - w) * cu + w * blend) + 0.5))
    return tuple(out)


def srgb_decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def srgb_encode(v):
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def lab_luminance(l_star):
    """D50 Lab L* alone → relative luminance Y (CIE's inverse)."""
    fy = (l_star + 16) / 116
    return fy ** 3 if fy ** 3 > 216 / 24389 else l_star * 27 / 24389


def gray_oracle(c, cov, ink, solidity):
    """S16: the overprint on a Gray Gamma 2.2 twin's codes, read from and written back to an
    sRGB render: per plane t = sRGB⁻¹(c)^(1/2.2), the rule on t, then sRGB((·)^2.2)."""
    w, s = cov / 255, solidity / 100
    out = []
    for plane in range(3):
        t = srgb_decode(c[plane] / 255) ** (1 / 2.2)
        printed = (1 - w) * t + w * ((1 - s) * t * ink + s * ink)
        out.append(int(255 * srgb_encode(max(printed, 0.0) ** 2.2) + 0.5))
    return tuple(out)


def worst(got, want, points=range(W * H)):
    return max(max(abs(g - e) for g, e in zip(got[i], want[i])) for i in points)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='spot-channels-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_spot_channels.py before any measurement'}
    copies = {}
    for name in ('base', 'red', 'typed', 'default', 'kind', 'deep', 'convert', 'tiff8', 'tiff16',
                 'layered8', 'layered16', 'float32', 'gray32'):
        copies[name] = folder / f'{name}.png'
        copies[name].write_bytes(fixture())
    base = [base_pixel(x, y) for x, y in ALL]

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def refused(tool, sentence, **arguments):
            text = call(tool, fail=True, **arguments)
            assert sentence in text, (tool, text)
            return text

        def spot_row(doc, name):
            rows = call('list_channels', document_id=doc)['channels']
            return next(r for r in rows if r['name'] == name)

        def coverage(doc, name):
            call('load_selection', document_id=doc, **{'from': 'channel'}, channel=name)
            reply = call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for x, y in ALL])
            call('deselect', document_id=doc)
            return [s['coverage'] for s in reply['samples']]

        def render(doc, **arguments):
            result = call('render', document_id=doc, max_side=4096, **arguments)
            image = next(c for c in result['content'] if c['type'] == 'image')
            text = next(c['text'] for c in result['content'] if c['type'] == 'text')
            return read_png(base64.b64decode(image['data'])), text

        def saved_png(doc, name):
            path = folder / name
            reply = call('save_copy', document_id=doc, path=str(path), format='png')
            return read_png(path.read_bytes()), reply

        # ---- S1: add ----------------------------------------------------------------
        doc = call('open_document', path=str(copies['base']))['document']['id']
        x0, y0, w0, h0 = RECT
        call('select_rect', document_id=doc, x=x0, y=y0, width=w0, height=h0)
        added = call('add_spot_channel', document_id=doc, name='Spot A', lab=BLACK, solidity=0)
        assert added['ok'] and added['from'] == 'selection' and added['solidity'] == 0, added
        row = spot_row(doc, 'Spot A')
        assert row['spot'] == {'lab': BLACK, 'solidity': 0}, row
        doc_row = next(r for r in call('get_document', document_id=doc)['channels']
                       if r['name'] == 'Spot A')
        assert doc_row['spot'] == row['spot'], doc_row
        rect = [255 if x0 <= x < x0 + w0 and y0 <= y < y0 + h0 else 0 for x, y in ALL]
        assert coverage(doc, 'Spot A') == rect, 'the coverage is the selection'
        receipts['s1'] = row
        print('PASS S1: add_spot_channel lists its ink and takes the selection', flush=True)

        # ---- S2: paint --------------------------------------------------------------
        probe = ALL.index((23, 18))
        call('brush_stroke', document_id=doc, target='channel:Spot A',
             points=[[18, 18], [28, 18]], size=4, color='#ffffff', opacity=1)
        painted = coverage(doc, 'Spot A')
        assert painted[probe] > 0, painted[probe]
        assert all(painted[i] == 255 for i, v in enumerate(rect) if v == 255), 'the rect stays'
        receipts['s2'] = {'coverage_at_23_18': painted[probe]}
        print('PASS S2: a brush stroke on the spot channel adds ink coverage', flush=True)

        # ---- S3: set, undo, redo ----------------------------------------------------
        set_reply = call('set_spot_channel', document_id=doc, channel='Spot A', solidity=60)
        assert set_reply['solidity'] == 60 and set_reply['lab'] == BLACK, set_reply
        assert spot_row(doc, 'Spot A')['spot']['solidity'] == 60
        call('undo', document_id=doc)
        assert spot_row(doc, 'Spot A')['spot']['solidity'] == 0
        call('redo', document_id=doc)
        assert spot_row(doc, 'Spot A')['spot']['solidity'] == 60
        print('PASS S3: set_spot_channel solidity 60 undoes to 0 and redoes', flush=True)

        # ---- S4: .rz round trip -----------------------------------------------------
        rz = folder / 'spot.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        reopened = call('open_document', path=str(rz))['document']['id']
        assert spot_row(reopened, 'Spot A')['spot'] == {'lab': BLACK, 'solidity': 60}
        assert coverage(reopened, 'Spot A') == painted, 'the reopened coverage'
        call('close_document', document_id=reopened, discard_changes=True)
        print('PASS S4: the spot channel survives .rz', flush=True)

        # ---- S5: the overprint ------------------------------------------------------
        black_png, black_reply = saved_png(doc, 'black.png')
        black_want = [oracle(base[i], painted[i], [0, 0, 0], 60) for i in range(W * H)]
        black_worst = worst(black_png, black_want)
        assert black_worst <= BLACK_BOUND, black_worst
        zero = [i for i, v in enumerate(painted) if v == 0]
        assert zero and all(black_png[i] == base[i] for i in zero), 'coverage 0 is untouched'

        red_doc = call('open_document', path=str(copies['red']))['document']['id']
        rx, ry, rw, rh = RED_RECT
        call('select_rect', document_id=red_doc, x=rx, y=ry, width=rw, height=rh)
        call('modify_selection', document_id=red_doc, operation='feather', radius=2)
        call('add_spot_channel', document_id=red_doc, name='Red', lab=RED, solidity=60)
        call('deselect', document_id=red_doc)
        red_cov = coverage(red_doc, 'Red')
        assert any(0 < v < 255 for v in red_cov), 'the feather gives partial coverage'
        ink = lab_to_srgb(RED)
        red_png, red_reply = saved_png(red_doc, 'red.png')
        red_want = [oracle(base[i], red_cov[i], ink, 60) for i in range(W * H)]
        red_worst = worst(red_png, red_want)
        assert red_worst <= RED_BOUND, red_worst
        red_zero = [i for i, v in enumerate(red_cov) if v == 0]
        assert red_zero and all(red_png[i] == base[i] for i in red_zero), 'red coverage 0'
        full, text = render(red_doc)
        render_worst = worst(full, red_png)
        assert render_worst <= RENDER_BOUND, render_worst
        assert SPOT_NOTE in text, text
        # 1c-1: a region and a preview_divisor render overprint the full render's samples.
        gx, gy, gw, gh = REGION
        region_png, region_text = render(red_doc, region={'x': gx, 'y': gy, 'width': gw, 'height': gh})
        crop = [full[(gy + j) * W + gx + i] for j in range(gh) for i in range(gw)]
        assert region_png == crop, 'the region render is the full render cropped'
        assert SPOT_NOTE in region_text, region_text
        preview_png, preview_text = render(red_doc, preview_divisor=2)
        sampled = [full[(2 * j) * W + 2 * i] for j in range(H // 2) for i in range(W // 2)]
        assert preview_png == sampled, 'the preview render is the full render sampled'
        assert SPOT_NOTE in preview_text, preview_text
        receipts['s5'] = {'black_worst': black_worst, 'red_worst': red_worst,
                          'render_worst': render_worst, 'ink_srgb': ink,
                          'region': 'exact', 'preview_divisor_2': 'exact'}
        print(f'PASS S5: PNG and render are the overprint (black {black_worst}, red {red_worst}, '
              f'render {render_worst} codes; region and preview exact)', flush=True)

        # ---- S6: refusals and the disclosure ----------------------------------------
        # Since #1 1c-2a a TIFF keeps the spot channels (S13 measures it).
        tif_reply = call('save_copy', document_id=red_doc, path=str(folder / 'red.tif'), format='tif')
        assert tif_reply['ok'] and (folder / 'red.tif').is_file(), tif_reply
        # Since #1 phase 1b an 8-bit PSD carries the spot channels, and since
        # 1c-2b the layered TIFF (S14 measures it).
        call('export_psd', document_id=red_doc, path=str(folder / 'red.psd'), format='psd',
             mode='layers')
        assert (folder / 'red.psd').is_file(), 'the PSD carries the spot channel'
        call('export_psd', document_id=red_doc, path=str(folder / 'red-layers.tif'),
             format='tiff', mode='layers')
        assert (folder / 'red-layers.tif').is_file(), 'the layered TIFF carries the spot channel'
        refused('export_pdf', PDF_REFUSAL, document_id=red_doc, path=str(folder / 'red.pdf'),
                overwrite=True)
        assert LOSS in red_reply['flat_save_losses'], red_reply
        exported = call('export_image', document_id=red_doc, path=str(folder / 'export.png'),
                        settings={'format': 'png'})
        assert 'spot_channels' in exported, exported
        assert read_png((folder / 'export.png').read_bytes()) == red_png, 'export equals the copy'
        print('PASS S6: the PSD, TIFF and layered TIFF save; PDF refuses by name; '
              'the PNG discloses the overprint', flush=True)

        # ---- S7: 16 bits ------------------------------------------------------------
        typed = call('open_document', path=str(copies['typed']))['document']['id']
        call('convert_precision', document_id=typed, mode='rgba16', flatten=True)
        call('select_rect', document_id=typed, x=x0, y=y0, width=w0, height=h0)
        call('add_spot_channel', document_id=typed, name='Deep', lab=BLACK, solidity=20)
        assert spot_row(typed, 'Deep')['spot'] == {'lab': BLACK, 'solidity': 20}
        typed_reply = call('save_copy', document_id=typed, path=str(folder / 'typed.png'), format='png')
        assert typed_reply['ok'] and (folder / 'typed.png').is_file(), typed_reply
        refused('convert_mode', INDEXED_REFUSAL, document_id=red_doc, mode='indexed',
                palette='exact', dither='none')
        print('PASS S7: a 16-bit spot channel adds, lists, saves a flat PNG, and refuses Indexed',
              flush=True)

        # ---- S8: the default ink shows ----------------------------------------------
        fresh = call('open_document', path=str(copies['default']))['document']['id']
        dx, dy, dw, dh = DEFAULT_RECT
        call('select_rect', document_id=fresh, x=dx, y=dy, width=dw, height=dh)
        default = call('add_spot_channel', document_id=fresh)
        assert default['solidity'] == 0 and default['from'] == 'selection', default
        call('deselect', document_id=fresh)
        shown, _ = render(fresh)
        inside = [i for i, (x, y) in enumerate(ALL) if dx <= x < dx + dw and dy <= y < dy + dh]
        outside = [i for i in range(W * H) if i not in set(inside)]
        default_ink = lab_to_srgb(default['lab'])
        want = [oracle(base[i], 255 if i in set(inside) else 0, default_ink, 0) for i in range(W * H)]
        default_worst = worst(shown, want, inside)
        assert default_worst <= RED_BOUND, default_worst
        assert all(shown[i] != base[i] for i in inside), 'the default ink changes the render'
        assert all(shown[i] == base[i] for i in outside), 'outside is untouched'
        receipts['s8'] = {'lab': default['lab'], 'worst': default_worst}
        print(f'PASS S8: a default spot channel shows ({default_worst} codes)', flush=True)

        # ---- S9: Merge Spot Channel -------------------------------------------------
        def layer_count(doc):
            return len(call('get_document', document_id=doc)['layers'])

        def channel_names(doc):
            return [r['name'] for r in call('list_channels', document_id=doc)['channels']]

        call('new_layer', document_id=red_doc, name='Empty')
        assert layer_count(red_doc) == 2, 'the empty layer'
        merged = call('merge_spot_channel', document_id=red_doc, channel='Red')
        assert merged['ok'] and merged['layers'] == 1 and merged['channels'] == 0, merged
        assert 'Red' not in channel_names(red_doc), 'the spot channel is gone'
        assert layer_count(red_doc) == 1, 'flattened'
        merged_png, _ = saved_png(red_doc, 'merged.png')
        merged_worst = worst(merged_png, red_want)
        assert merged_worst <= RED_BOUND, merged_worst
        call('undo', document_id=red_doc)
        assert spot_row(red_doc, 'Red')['spot'] == {'lab': RED, 'solidity': 60}, 'undo restores it'
        assert layer_count(red_doc) == 2, 'undo restores both layers'
        restored, _ = render(red_doc)
        assert restored == full, "undo restores S5's render"
        receipts['s9'] = {'worst': merged_worst}
        print(f'PASS S9: Merge Spot Channel flattens and bakes the ink ({merged_worst} codes), '
              'and undoes', flush=True)

        # ---- S10: alpha ↔ spot ------------------------------------------------------
        kind_doc = call('open_document', path=str(copies['kind']))['document']['id']
        kx, ky, kw, kh = KIND_RECT
        call('select_rect', document_id=kind_doc, x=kx, y=ky, width=kw, height=kh)
        call('modify_selection', document_id=kind_doc, operation='feather', radius=2)
        call('add_channel', document_id=kind_doc, **{'from': 'selection'}, name='Mask')
        call('deselect', document_id=kind_doc)
        alpha = coverage(kind_doc, 'Mask')
        assert any(0 < v < 255 for v in alpha), 'the feather gives partial values'
        made = call('set_channel_kind', document_id=kind_doc, channel='Mask', kind='spot')
        assert made['ok'] and made['kind'] == 'spot' and made['spot'], made
        assert spot_row(kind_doc, 'Mask')['spot'] == made['spot'], 'the row lists the ink'
        unmade = call('set_channel_kind', document_id=kind_doc, channel='Mask', kind='alpha')
        assert unmade['ok'] and unmade['spot'] is None, unmade
        assert spot_row(kind_doc, 'Mask').get('spot') is None, 'an alpha channel again'
        call('undo', document_id=kind_doc)
        assert spot_row(kind_doc, 'Mask')['spot'] == made['spot'], 'one undo gives spot again'
        assert coverage(kind_doc, 'Mask') == [255 - v for v in alpha], 'spot coverage is 255 − alpha'
        call('set_channel_kind', document_id=kind_doc, channel='Mask', kind='alpha')
        assert coverage(kind_doc, 'Mask') == alpha, 'alpha again is the original'
        receipts['s10'] = {'spot': made['spot']}
        print('PASS S10: alpha → spot → alpha inverts the plane both ways, one undo step each',
              flush=True)

        # ---- S11: a 16-bit PNG export's overprint -----------------------------------
        deep = call('open_document', path=str(copies['deep']))['document']['id']
        call('convert_precision', document_id=deep, mode='rgba16', flatten=True)
        call('select_rect', document_id=deep, x=rx, y=ry, width=rw, height=rh)
        call('add_spot_channel', document_id=deep, name='Red', lab=RED, solidity=60)
        call('deselect', document_id=deep)
        deep_path = folder / 'deep.png'
        deep_reply = call('save_copy', document_id=deep, path=str(deep_path), format='png',
                          bit_depth=16)
        assert deep_reply['bit_depth'] == 16 and LOSS in deep_reply['flat_save_losses'], deep_reply
        deep_png = read_png16(deep_path.read_bytes())
        deep_worst = 0
        for i, (x, y) in enumerate(ALL):
            if rx <= x < rx + rw and ry <= y < ry + rh:
                for plane in range(3):
                    cu = base[i][plane] / 255
                    want = 65535 * ((1 - 0.6) * cu * ink[plane] + 0.6 * ink[plane])
                    deep_worst = max(deep_worst, abs(deep_png[i][plane] - want))
            else:
                assert deep_png[i] == tuple(v * 257 for v in base[i]), (x, y, deep_png[i])
        assert deep_worst <= DEEP_BOUND, deep_worst
        receipts['s11'] = {'worst_16bit_codes': deep_worst}
        print(f'PASS S11: a 16-bit PNG composites the overprint ({deep_worst:.1f} codes) and '
              'discloses it', flush=True)

        # ---- S12: convert_precision keeps the channels ------------------------------
        conv = call('open_document', path=str(copies['convert']))['document']['id']
        call('select_rect', document_id=conv, x=kx, y=ky, width=kw, height=kh)
        call('modify_selection', document_id=conv, operation='feather', radius=2)
        call('add_channel', document_id=conv, **{'from': 'selection'}, name='Mask')
        call('set_channel_options', document_id=conv, channel='Mask', overlay_color=MASK_OVERLAY,
             overlay_opacity=MASK_OPACITY, color_indicates='selected')
        call('deselect', document_id=conv)
        call('select_rect', document_id=conv, x=rx, y=ry, width=rw, height=rh)
        call('add_spot_channel', document_id=conv, name='Red', lab=RED, solidity=60)
        call('deselect', document_id=conv)

        def channel_rows(doc):
            return [{k: r.get(k) for k in ROW_KEYS}
                    for r in call('list_channels', document_id=doc)['channels']]

        def precision(doc):
            return call('get_document', document_id=doc)['precision']

        rows = channel_rows(conv)
        assert [r['name'] for r in rows] == ['Mask', 'Red'], rows
        assert rows[0]['color_indicates'] == 'selected' and rows[0]['spot'] is None, rows
        assert str(rows[0]['overlay_color']).lower() == MASK_OVERLAY, rows
        assert abs(float(rows[0]['overlay_opacity']) - MASK_OPACITY) < 1e-3, rows
        assert rows[1]['spot'] == {'lab': RED, 'solidity': 60}, rows
        mask_cov, red_cov8 = coverage(conv, 'Mask'), coverage(conv, 'Red')
        assert any(0 < v < 255 for v in mask_cov), 'the feather gives partial values'

        def converted(mode):
            reply = call('convert_precision', document_id=conv, mode=mode, flatten=True)
            plan = reply['conversion']
            assert 'saved_channels' in plan['preserved'], plan
            assert 'saved_channels' not in plan['discarded'], plan
            assert precision(conv) == mode, (mode, precision(conv))
            assert channel_rows(conv) == rows, (mode, channel_rows(conv))

        def same_coverage(label):
            assert coverage(conv, 'Mask') == mask_cov, f'{label}: the alpha coverage'
            assert coverage(conv, 'Red') == red_cov8, f'{label}: the spot coverage'

        # Phase A: both conversions and both undos, no coverage read between.
        converted('rgba16')
        converted('rgba8')
        call('undo', document_id=conv)
        assert precision(conv) == 'rgba16' and channel_rows(conv) == rows, 'undo to 16 bits'
        call('undo', document_id=conv)
        assert precision(conv) == 'rgba8' and channel_rows(conv) == rows, 'undo to 8 bits'
        same_coverage('after both undos')
        # Phase B: the coverage at each depth, no undo after.
        converted('rgba16')
        same_coverage('at 16 bits')
        converted('rgba8')
        same_coverage('back at 8 bits')
        receipts['s12'] = {'rows': rows}
        print('PASS S12: convert_precision 8→16→8 keeps the alpha and spot channels (rows and '
              'coverage), and undo restores each depth with them', flush=True)

        # ---- S13: a TIFF keeps the spot channel (1c-2a) -----------------------------
        def spot_names(doc):
            return [r['name'] for r in call('list_channels', document_id=doc)['channels']
                    if r.get('spot') is not None]

        s13 = {}
        for label, copy, bits in (('8-bit', copies['tiff8'], None), ('16-bit', copies['tiff16'], 16)):
            src = call('open_document', path=str(copy))['document']['id']
            if bits:
                call('convert_precision', document_id=src, mode='rgba16', flatten=True)
            call('select_rect', document_id=src, x=rx, y=ry, width=rw, height=rh)
            call('modify_selection', document_id=src, operation='feather', radius=2)
            call('add_spot_channel', document_id=src, name='Red', lab=RED, solidity=60)
            call('deselect', document_id=src)
            src_cov = coverage(src, 'Red')
            depth = {'bit_depth': bits} if bits else {}
            path = folder / f'kept-{bits or 8}.tif'
            reply = call('save_copy', document_id=src, path=str(path), format='tif', **depth)
            assert reply['ok'] and LOSS not in reply['flat_save_losses'], (label, reply)
            assert 'working_memory_plan_bytes' in reply, (label, reply)
            back = call('open_document', path=str(path))['document']['id']
            assert spot_names(back) == ['Red'], (label, spot_names(back))
            assert spot_row(back, 'Red')['spot'] == {'lab': RED, 'solidity': 60}, label
            assert coverage(back, 'Red') == src_cov, f'{label}: the reopened coverage'
            call('close_document', document_id=back, discard_changes=True)
            undos = 0
            while spot_names(src) and undos < UNDO_LIMIT:
                call('undo', document_id=src)
                undos += 1
            assert not spot_names(src), f'{label}: undo removes the spot channel'
            again = folder / f'undone-{bits or 8}.tif'
            call('save_copy', document_id=src, path=str(again), format='tif', **depth)
            empty = call('open_document', path=str(again))['document']['id']
            assert spot_names(empty) == [], (label, spot_names(empty))
            call('close_document', document_id=empty, discard_changes=True)
            s13[label] = {'plan': reply['working_memory_plan_bytes'], 'undos': undos}
        receipts['s13'] = s13
        print('PASS S13: save_copy tif keeps the spot channel at 8 and 16 bits (ink and coverage '
              'reopen equal), and after undo the TIFF holds none', flush=True)

        # ---- S14: a layered TIFF keeps the spot channel (1c-2b) ---------------------
        s14 = {}
        for label, copy, bits in (('8-bit', copies['layered8'], None),
                                  ('16-bit', copies['layered16'], 16)):
            src = call('open_document', path=str(copy))['document']['id']
            if bits:
                call('convert_precision', document_id=src, mode='rgba16', flatten=True)
            call('select_rect', document_id=src, x=rx, y=ry, width=rw, height=rh)
            call('modify_selection', document_id=src, operation='feather', radius=2)
            call('add_spot_channel', document_id=src, name='Red', lab=RED, solidity=60)
            call('deselect', document_id=src)
            src_cov = coverage(src, 'Red')
            path = folder / f'layered-{bits or 8}.tif'
            reply = call('export_psd', document_id=src, path=str(path), format='tiff',
                         mode='layers')
            assert any(o.startswith(WRITTEN) for o in reply['observations']), (label, reply)
            report = call('inspect_psd', path=str(path))
            assert OPENED in report['observations'], (label, report['observations'])
            back = call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
            assert spot_names(back) == ['Red'], (label, spot_names(back))
            assert spot_row(back, 'Red')['spot'] == {'lab': RED, 'solidity': 60}, label
            assert coverage(back, 'Red') == src_cov, f'{label}: the reopened coverage'
            call('close_document', document_id=back, discard_changes=True)
            call('close_document', document_id=src, discard_changes=True)
            s14[label] = {'observations': reply['observations']}
        receipts['s14'] = s14
        print('PASS S14: export_psd tiff layers keeps the spot channel at 8 and 16 bits (the '
              'report names it; ink and coverage reopen layered equal)', flush=True)

        def checker(record):
            """A sub-check recorder: (name, ok, detail) into `record`; returns the failures."""
            failed = []

            def check(name, ok, detail):
                record[name] = {'ok': bool(ok), 'detail': detail}
                if not ok:
                    failed.append(f'{name}: {detail}')
            return check, failed

        def zero_exact(got, want, cov):
            return all(got[i] == want[i] for i, v in enumerate(cov) if v == 0)

        # ---- S15: a 32-bit document's spot channel (#1) -----------------------------
        s15 = {}
        check, failed = checker(s15)
        flt = call('open_document', path=str(copies['float32']))['document']['id']
        call('convert_precision', document_id=flt, mode='rgba32', structure='preserve')
        check('precision', precision(flt) == 'rgba32_float', precision(flt))
        bare, _ = render(flt)
        bare1, _ = render(flt, exposure=1)
        call('select_rect', document_id=flt, x=rx, y=ry, width=rw, height=rh)
        call('modify_selection', document_id=flt, operation='feather', radius=2)
        call('add_spot_channel', document_id=flt, name='Ink', lab=FLOAT_INK, solidity=60)
        call('deselect', document_id=flt)
        flt_cov = coverage(flt, 'Ink')
        check('partial coverage', any(0 < v < 255 for v in flt_cov), 'the feather')
        flt_ink = lab_to_srgb(FLOAT_INK)
        flt_want = [oracle(bare[i], flt_cov[i], flt_ink, 60) for i in range(W * H)]
        # (a) the full render.
        shown, shown_text = render(flt)
        a_worst = worst(shown, flt_want)
        check('a render', a_worst <= RED_BOUND and zero_exact(shown, bare, flt_cov),
              {'worst': a_worst, 'zero_exact': zero_exact(shown, bare, flt_cov)})
        check('a text', SPOT_NOTE in shown_text and FLOAT_NOTE in shown_text, shown_text)
        # (b) region and preview_divisor.
        gx, gy, gw, gh = REGION
        region_png, region_text = render(flt, region={'x': gx, 'y': gy, 'width': gw, 'height': gh})
        crop = [shown[(gy + j) * W + gx + i] for j in range(gh) for i in range(gw)]
        check('b region', region_png == crop and SPOT_NOTE in region_text,
              {'exact': region_png == crop, 'text': region_text})
        preview_png, preview_text = render(flt, preview_divisor=2)
        sampled = [shown[(2 * j) * W + 2 * i] for j in range(H // 2) for i in range(W // 2)]
        check('b preview', preview_png == sampled and SPOT_NOTE in preview_text,
              {'exact': preview_png == sampled, 'text': preview_text})
        # (c) the render's exposure.
        shown1, _ = render(flt, exposure=1)
        want1 = [oracle(bare1[i], flt_cov[i], flt_ink, 60) for i in range(W * H)]
        c_worst = worst(shown1, want1)
        check('c exposure 1', c_worst <= RED_BOUND and zero_exact(shown1, bare1, flt_cov),
              {'worst': c_worst, 'zero_exact': zero_exact(shown1, bare1, flt_cov)})
        # (d) the flattened PNG keeps what the view shows (8 bits, undithered, as the render).
        flt_path = folder / 'float32.png'
        flt_reply = call('save_copy', document_id=flt, path=str(flt_path), format='png',
                         bit_depth=8, dither='none')
        flt_png = read_png(flt_path.read_bytes())
        d_worst = worst(flt_png, flt_want)
        check('d png', d_worst <= RED_BOUND, {'worst': d_worst})
        check('d loss line', LOSS in flt_reply['flat_save_losses'], flt_reply['flat_save_losses'])
        # (e) Merge Spot Channel keeps it too, and undoes.
        merged = call('merge_spot_channel', document_id=flt, channel='Ink')
        check('e reply', merged['ok'] and merged['layers'] == 1 and merged['channels'] == 0, merged)
        flt_merged, _ = render(flt)
        e_png, e_want = worst(flt_merged, flt_png), worst(flt_merged, flt_want)
        check('e merged render', e_png <= RENDER_BOUND and e_want <= RED_BOUND,
              {'against_png': e_png, 'against_oracle': e_want})
        call('undo', document_id=flt)
        check('e undo ink', spot_row(flt, 'Ink')['spot'] == {'lab': FLOAT_INK, 'solidity': 60},
              spot_row(flt, 'Ink')['spot'])
        flt_restored, _ = render(flt)
        check('e undo render', flt_restored == shown, 'exactly (a)')
        receipts['s15'] = s15
        failed15 = failed
        if failed15:
            print(f'FAIL S15: {failed15}', flush=True)
        else:
            print(f'PASS S15: a 32-bit document overprints its spot channel in render ({a_worst} '
                  f'codes; region and preview exact; exposure 1 {c_worst}), its PNG ({d_worst}) '
                  f'and its merge ({e_want} against the oracle, {e_png} against the PNG), and '
                  'undoes', flush=True)

        # ---- S16: a 32-bit Grayscale document's spot channel (#1) -------------------
        s16 = {}
        check, failed = checker(s16)
        gry = call('open_document', path=str(copies['gray32']))['document']['id']
        call('convert_precision', document_id=gry, mode='rgba32', structure='preserve')
        call('convert_mode', document_id=gry, mode='gray', gray_space='gray_gamma_22')
        check('precision', precision(gry) == 'gray32', precision(gry))
        gray_bare, _ = render(gry)
        call('select_rect', document_id=gry, x=rx, y=ry, width=rw, height=rh)
        call('modify_selection', document_id=gry, operation='feather', radius=2)
        call('add_spot_channel', document_id=gry, name='Grey', lab=GREY_INK, solidity=60)
        call('deselect', document_id=gry)
        gray_cov = coverage(gry, 'Grey')
        grey_ink = lab_luminance(GREY_INK[0]) ** (1 / 2.2)
        gray_want = [gray_oracle(gray_bare[i], gray_cov[i], grey_ink, 60) for i in range(W * H)]
        gray_shown, gray_text = render(gry)
        g_worst = worst(gray_shown, gray_want)
        check('render', g_worst <= GRAY_BOUND and zero_exact(gray_shown, gray_bare, gray_cov),
              {'worst': g_worst, 'zero_exact': zero_exact(gray_shown, gray_bare, gray_cov)})
        check('text', SPOT_NOTE in gray_text, gray_text)
        gray_merged_reply = call('merge_spot_channel', document_id=gry, channel='Grey')
        check('merge reply', gray_merged_reply['ok'] and gray_merged_reply['channels'] == 0,
              gray_merged_reply)
        gray_merged, _ = render(gry)
        m_worst = worst(gray_merged, gray_shown)
        check('merged render', m_worst <= GRAY_BOUND, {'against_view': m_worst})
        call('undo', document_id=gry)
        gray_restored, _ = render(gry)
        check('undo render', gray_restored == gray_shown, 'exactly the render before the merge')
        receipts['s16'] = s16
        if failed:
            print(f'FAIL S16: {failed}', flush=True)
        else:
            print(f'PASS S16: a 32-bit Grayscale document overprints its spot channel in render '
                  f'({g_worst} codes) and its merge keeps it ({m_worst}), and undoes', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2))
    assert not (failed15 + failed), failed15 + failed
    print('verify_spot_channels: all gates PASS', flush=True)


if __name__ == '__main__':
    main()
