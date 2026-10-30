#!/usr/bin/env python3
"""The #1 phase-1 MCP drive: CMYK documents open, edit, save, reopen and
convert over MCP, and everything else refuses by name.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python, and every file the app writes is
read back by this file's own TIFF, JPEG-marker and PSD readers, never by the
app. The CMYK profile is macOS's Generic CMYK Profile (always installed).

- **G1 TIFF 8/16.** A chunky CMYK TIFF at 8 and at 16 bits opens as a CMYK
  document (`get_color_profile.color_mode` "cmyk", the Generic profile), and
  `save_copy tiff` writes the SAME samples at the same depth (bit-exact) with
  the profile embedded.
- **G2 edit + .rz.** duplicate_layer, set_layer_properties (name, opacity),
  set_layer_label, reorder_layer and flatten_image succeed; `save_copy rz`
  reopens as a CMYK document whose `sample_color` ink equals the edited one.
- **G3 JPEG.** `save_copy jpeg` writes a four-component JPEG with an Adobe
  APP14 that reopens as CMYK within 6/255 of each ink on the flat patch.
- **G4 PSD.** `export_psd mode flattened` writes colour mode 4; `inspect_psd`
  answers `import_precision` "cmyk"; `open_document psd_mode composite`
  reopens it with the same ink (bit-exact).
- **G5 ImageMagick PSD.** A CMYK PSD ImageMagick writes from the 8-bit TIFF
  opens with the TIFF's ink (skipped, and said so, when `magick` is absent).
- **G6 convert_mode.** An 8-bit RGB PNG converts to CMYK (depth 8, the working
  CMYK profile), undo restores RGB, redo restores CMYK, and `convert_mode rgb`
  returns an 8-bit RGB document; `render channel cyan` answers an image.
- **G7 refusals.** depth_blur (RGB only; the retouch tools run on ink since
  #1's retouch batch), a layer style with a Blend If on Red (layer effects
  are admitted since the styles batch, Gray and plate Blend If since the
  Blend If batch, #1) and `save_copy png` refuse by name in a
  CMYK document (`export_psd mode layers` writes since phase 3,
  `verify_cmyk_files.py`).
- **G8 Multichannel.** `convert_mode mode multichannel` refuses by name on the
  RGB and the CMYK document and leaves each mode unchanged; a crafted 8-bit
  colour-mode-7 PSD reports color_mode "Multichannel" with both choices
  refused by the core's sentence, and its open refuses with it (#1,
  `designs/multichannel-mode.md`).
- **G9 Auto-Align and Reduce Noise.** `verify_v1_stack_align.py`'s
  value-noise picture at 640x480: A, B = A(K^-1 q) with K = translation(14, -9)
  and C with K = translation(-8, 6), loaded with `load_files_into_stack` and
  converted with `convert_mode cmyk`. G9a: `auto_align_layers` [0, 1] answers
  `changed` true and `aligned` [1], its homography within BOUND_PX (2.0 px) of
  K^-1 over the 5x5 corner grid, one history step; per ink, mean |after - A|
  of `sample_color` ink16 over the 5x5 cell centres (shifted by the reply's
  `canvas.origin`) is below half mean |before - A|, A read with layer 1
  hidden; `undo` restores the layer rectangle and the canvas. G9b: on [A, B,
  C], `reduce_noise_from_stack mode median` answers `aligned` [1, 2],
  `unchanged` and `excluded` empty, `layers` and `hidden` [0, 1, 2], the new
  layer "Noise Reduced" at index 3 in a still-CMYK document; per ink, mean
  |composite - A| over the cell centres is below half mean |B unaligned - A|;
  one `undo` removes the layer. G9c: `auto_align_layers` with vignette true
  and with distortion true refuse with "Vignette Removal does not work in
  CMYK." and "Geometric Distortion does not work in CMYK.", the history cursor
  unmoved.
"""
import argparse
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

import phase5_support
from phase5_support import App
from verify_phase1 import png
from verify_v1_stack_align import (
    BOUND_PX, Picture, apply, centre_grid, corner_grid, fixture, inverse, translation)

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
GENERIC_NAME = 'Generic CMYK Profile'
JPEG_BOUND = 6  # 8-bit codes per ink, a lossy codec on a flat patch
REFUSED = 'is unavailable in a CMYK document'
MULTICHANNEL_TOOL = 'Multichannel documents are not supported: Doctored has no spot channels.'
MULTICHANNEL_FILE = 'Multichannel files are not supported: Doctored has no spot channels'
ALIGN_SIZE = (640, 480)
ALIGN_K = {'b': translation(14, -9), 'c': translation(-8, 6)}
VIGNETTE_CMYK = 'Vignette Removal does not work in CMYK.'
DISTORTION_CMYK = 'Geometric Distortion does not work in CMYK.'


def inks(call, doc, origin=(0, 0)):
    """`sample_color` ink16 of the composite over the 5x5 cell centres."""
    return [call('sample_color', document_id=doc, x=x + origin[0], y=y + origin[1])['ink16']
            for x, y in centre_grid(ALIGN_SIZE)]


def mean_ink_error(got, want):
    """Per ink, mean |got - want| over the grid."""
    return [sum(abs(g[c] - w[c]) for g, w in zip(got, want)) / len(want) for c in range(4)]


def homography_error(g, wanted):
    """max |G(q) - K^-1(q)| over the 5x5 corner grid."""
    worst = 0.0
    for x, y in corner_grid(ALIGN_SIZE):
        gx, gy = apply(g, x, y)
        wx, wy = apply(wanted, x, y)
        worst = max(worst, ((gx - wx) ** 2 + (gy - wy) ** 2) ** 0.5)
    return worst


def cmyk_stack(call, paths):
    doc = call('load_files_into_stack', paths=[str(path) for path in paths])['document']['id']
    converted = call('convert_mode', document_id=doc, mode='cmyk')
    assert converted['color_mode'] == 'cmyk', converted
    return doc


def layout(call, doc):
    summary = call('get_document', document_id=doc)
    return ((summary['width'], summary['height']),
            [(row['offset_x'], row['offset_y'], row['width'], row['height'])
             for row in summary['layers']])


def cmyk_tiff(width, height, bits, samples, icc=None):
    """A little-endian classic chunky CMYK TIFF (Photometric 5), one strip."""
    if bits == 8:
        data = bytes(samples)
    else:
        data = struct.pack(f'<{len(samples)}H', *samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<4H', bits, bits, bits, bits))
    icc_at = place(icc) if icc is not None else None
    if len(body) % 2:
        body.append(0)

    def short(v):
        return struct.pack('<HH', v, 0)

    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 4, struct.pack('<I', bits_at)), (259, 3, 1, short(1)),
        (262, 3, 1, short(5)), (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(4)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))), (284, 3, 1, short(1)),
    ]
    if icc is not None:
        entries.append((34675, 7, len(icc), struct.pack('<I', icc_at)))
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def read_tiff(data):
    """(photometric, bits, samples per pixel, samples, icc) of an
    uncompressed classic TIFF, this file's own reader."""
    order = '<' if data[:2] == b'II' else '>'
    ifd = struct.unpack_from(order + 'I', data, 4)[0]
    count = struct.unpack_from(order + 'H', data, ifd)[0]
    sizes = {1: 1, 2: 1, 3: 2, 4: 4, 7: 1}
    tags = {}
    for i in range(count):
        tag, kind, n = struct.unpack_from(order + 'HHI', data, ifd + 2 + 12 * i)
        size = sizes.get(kind, 1) * n
        at = ifd + 2 + 12 * i + 8 if size <= 4 else struct.unpack_from(order + 'I', data, ifd + 2 + 12 * i + 8)[0]
        raw = data[at:at + size]
        if kind == 3:
            tags[tag] = list(struct.unpack(order + f'{n}H', raw))
        elif kind == 4:
            tags[tag] = list(struct.unpack(order + f'{n}I', raw))
        else:
            tags[tag] = raw
    assert tags.get(259, [1])[0] == 1, 'compressed TIFF'
    strips = b''.join(data[o:o + c] for o, c in zip(tags[273], tags[279]))
    bits = tags[258][0]
    samples = list(strips) if bits == 8 else list(struct.unpack(order + f'{len(strips) // 2}H', strips))
    return tags[262][0], bits, tags[277][0], samples, tags.get(34675)


def read_psd(data):
    """(channels, depth, mode, icc, planes) of a classic PSD's composite."""
    assert data[:4] == b'8BPS'
    channels, height, width, depth, mode = struct.unpack_from('>HIIHH', data, 12)
    at = 26
    at += 4 + struct.unpack_from('>I', data, at)[0]
    length = struct.unpack_from('>I', data, at)[0]
    r, end, icc = at + 4, at + 4 + length, None
    while r < end:
        assert data[r:r + 4] == b'8BIM'
        rid = struct.unpack_from('>H', data, r + 4)[0]
        name = data[r + 6]
        r += 6 + ((name + 2) & ~1)
        size = struct.unpack_from('>I', data, r)[0]
        if rid == 1039:
            icc = data[r + 4:r + 4 + size]
        r += 4 + size + (size & 1)
    at = end
    at += 4 + struct.unpack_from('>I', data, at)[0]
    compression = struct.unpack_from('>H', data, at)[0]
    at += 2
    row = width * depth // 8
    planes = []
    if compression == 0:
        for c in range(channels):
            planes.append(data[at + c * row * height:at + (c + 1) * row * height])
    else:
        counts = struct.unpack_from(f'>{channels * height}H', data, at)
        at += 2 * channels * height
        for c in range(channels):
            plane = bytearray()
            for y in range(height):
                n = counts[c * height + y]
                chunk, i = data[at:at + n], 0
                at += n
                while i < len(chunk):
                    head = chunk[i] if chunk[i] < 128 else chunk[i] - 256
                    if head >= 0:
                        plane += chunk[i + 1:i + 2 + head]
                        i += 2 + head
                    elif head != -128:
                        plane += bytes([chunk[i + 1]]) * (1 - head)
                        i += 2
                    else:
                        i += 1
            planes.append(bytes(plane))
    return channels, depth, mode, icc, planes


def jpeg_components(data):
    """(components, adobe transform) of a baseline/progressive JPEG."""
    at, components, adobe = 2, None, None
    while at < len(data):
        assert data[at] == 0xFF
        marker = data[at + 1]
        size = struct.unpack_from('>H', data, at + 2)[0]
        payload = data[at + 4:at + 2 + size]
        if marker == 0xEE and payload[:5] == b'Adobe':
            adobe = payload[11]
        if marker in (0xC0, 0xC1, 0xC2):
            components = payload[5]
        if marker == 0xDA:
            break
        at += 2 + size
    return components, adobe


def pattern(width, height, bits):
    top = 255 if bits == 8 else 65535
    return [(i * 7919 + c * 104729) % (top + 1) for i in range(width * height) for c in range(4)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-mode-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = output / 'work'
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_mode.py before any measurement'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        # ---- G1: TIFF 8/16, bit-exact ------------------------------------
        for bits in (8, 16):
            w, h = 29, 13
            samples = pattern(w, h, bits)
            source = folder / f'in{bits}.tif'
            source.write_bytes(cmyk_tiff(w, h, bits, samples, profile))
            doc = call('open_document', path=str(source))['document']['id']
            color = call('get_color_profile', document_id=doc)
            assert color.get('color_mode') == 'cmyk', color
            assert color.get('cmyk_profile') == GENERIC_NAME, color
            assert color.get('cmyk_depth') == bits, color
            out = folder / f'out{bits}.tif'
            call('save_copy', document_id=doc, path=str(out), format='tiff')
            photometric, wrote_bits, spp, wrote, icc = read_tiff(out.read_bytes())
            assert (photometric, wrote_bits, spp) == (5, bits, 4), (photometric, wrote_bits, spp)
            assert wrote == samples, f'{bits}-bit TIFF samples differ'
            assert icc == profile, 'the CMYK profile was not embedded verbatim'
            call('close_document', document_id=doc)
            receipts[f'g1_{bits}'] = {'samples': len(samples), 'bit_exact': True}
        print('PASS g1: CMYK TIFF 8/16 opens as CMYK and saves bit-exact', flush=True)

        # ---- G2: edits and .rz --------------------------------------------
        w, h = 8, 8
        flat = [52000, 9000, 30000, 12000] * (w * h)
        source = folder / 'flat16.tif'
        source.write_bytes(cmyk_tiff(w, h, 16, flat, profile))
        doc = call('open_document', path=str(source))['document']['id']
        call('duplicate_layer', document_id=doc, index=0)
        call('set_layer_properties', document_id=doc, index=1, name='Copy', opacity=0.5)
        call('set_layer_label', document_id=doc, index=1, label='red')
        call('reorder_layer', document_id=doc, **{'from': 1, 'to': 0})
        call('flatten_image', document_id=doc)
        edited = call('sample_color', document_id=doc, x=3, y=3)
        assert edited.get('color_mode') == 'cmyk', edited
        assert edited['ink16'] == [52000, 9000, 30000, 12000], edited
        rz = folder / 'edited.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        reopened = call('open_document', path=str(rz))['document']['id']
        again = call('sample_color', document_id=reopened, x=3, y=3)
        assert again['ink16'] == edited['ink16'], (again, edited)
        receipts['g2'] = {'ink16': again['ink16']}
        print('PASS g2: structure edits and the .rz round trip keep the ink', flush=True)

        # ---- G3: JPEG -------------------------------------------------------
        jpeg = folder / 'flat.jpg'
        call('save_copy', document_id=doc, path=str(jpeg), format='jpeg')
        components, adobe = jpeg_components(jpeg.read_bytes())
        assert components == 4 and adobe is not None, (components, adobe)
        jdoc = call('open_document', path=str(jpeg))['document']['id']
        jsample = call('sample_color', document_id=jdoc, x=3, y=3)
        assert jsample.get('color_mode') == 'cmyk', jsample
        for got, want in zip(jsample['ink16'], edited['ink16']):
            assert abs(round(got / 257) - round(want / 257)) <= JPEG_BOUND, (jsample, edited)
        receipts['g3'] = {'ink16': jsample['ink16']}
        print('PASS g3: the CMYK JPEG reopens as CMYK within 6/255', flush=True)

        # ---- G4: PSD ---------------------------------------------------------
        psd = folder / 'flat.psd'
        call('export_psd', document_id=doc, path=str(psd), mode='flattened')
        channels, depth, mode, icc, planes = read_psd(psd.read_bytes())
        assert (channels, depth, mode) == (4, 16, 4), (channels, depth, mode)
        assert icc == profile
        stored = [struct.unpack('>H', planes[c][0:2])[0] for c in range(4)]
        assert stored == [65535 - v for v in edited['ink16']], stored
        report = call('inspect_psd', path=str(psd))
        assert report.get('import_precision') == 'cmyk', report
        pdoc = call('open_document', path=str(psd), psd_mode='composite',
                    psd_fingerprint=report['source_fingerprint'])['document']['id']
        psample = call('sample_color', document_id=pdoc, x=3, y=3)
        assert psample['ink16'] == edited['ink16'], psample
        receipts['g4'] = {'ink16': psample['ink16']}
        print('PASS g4: the flattened CMYK PSD reopens through the report with the same ink', flush=True)

        # ---- G5: an ImageMagick PSD -------------------------------------------
        magick = shutil.which('magick')
        if magick:
            src = folder / 'im.tif'
            samples = pattern(17, 9, 8)
            src.write_bytes(cmyk_tiff(17, 9, 8, samples, profile))
            impsd = folder / 'im.psd'
            subprocess.run([magick, str(src), str(impsd)], check=True)
            report = call('inspect_psd', path=str(impsd))
            assert report.get('import_precision') == 'cmyk', report
            idoc = call('open_document', path=str(impsd), psd_mode='composite',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
            first = call('sample_color', document_id=idoc, x=0, y=0)
            assert first['ink16'] == [v * 257 for v in samples[:4]], first
            receipts['g5'] = {'ink16': first['ink16']}
            print('PASS g5: the ImageMagick CMYK PSD opens with the TIFF ink', flush=True)
        else:
            receipts['g5'] = 'skipped: magick is absent'
            print('SKIP g5: ImageMagick is absent', flush=True)

        # ---- G6: convert_mode ---------------------------------------------------
        rgb_path = folder / 'rgb.png'
        png(rgb_path, 16, 16, bytes(v for i in range(256) for v in (200, 80 + i % 50, 40, 255)))
        rdoc = call('open_document', path=str(rgb_path))['document']['id']
        converted = call('convert_mode', document_id=rdoc, mode='cmyk')
        assert converted['color_mode'] == 'cmyk' and converted['depth'] == 8, converted
        assert call('get_color_profile', document_id=rdoc).get('color_mode') == 'cmyk'
        call('undo', document_id=rdoc)
        assert call('get_color_profile', document_id=rdoc).get('color_mode') != 'cmyk'
        call('redo', document_id=rdoc)
        assert call('get_color_profile', document_id=rdoc).get('color_mode') == 'cmyk'
        plane = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
            'document_id': rdoc, 'channel': 'cyan'}})
        assert not plane.get('isError') and any(c['type'] == 'image' for c in plane['content']), plane
        back = call('convert_mode', document_id=rdoc, mode='rgb')
        assert back['color_mode'] == 'rgb' and back['depth'] == 8, back
        receipts['g6'] = {'to_cmyk': converted, 'to_rgb': back}
        print('PASS g6: convert_mode both ways with one undo step', flush=True)

        # ---- G7: refusals -------------------------------------------------------
        refusals = {
            'depth_blur': call('depth_blur', fail=True, document_id=doc, layer=0),
            'blend': call('set_layer_style', fail=True, document_id=doc, layer=0,
                          style={'blend_if': {'channel': 'red',
                                              'this_layer': [10, 20, 240, 255]}}),
            'png': call('save_copy', fail=True, document_id=doc, path=str(folder / 'x.png'),
                        format='png'),
        }
        assert REFUSED in refusals['depth_blur'], refusals
        assert 'Blend If on the Red channel' + ' ' + REFUSED in refusals['blend'], refusals
        assert 'saves as' in refusals['png'], refusals
        receipts['g7'] = refusals
        print('PASS g7: everything else refuses by name', flush=True)

        # ---- G8: Multichannel (#1, designs/multichannel-mode.md) ---------------
        g8 = {}
        for name, target in (('rgb', rdoc), ('cmyk', doc)):
            before = call('get_color_profile', document_id=target).get('color_mode')
            refusal = call('convert_mode', fail=True, document_id=target, mode='multichannel')
            assert MULTICHANNEL_TOOL in refusal, refusal
            after = call('get_color_profile', document_id=target).get('color_mode')
            assert after == before and (before == 'cmyk') == (name == 'cmyk'), (before, after)
            g8[f'convert_{name}'] = refusal
        mw, mh = 4, 3
        mc = folder / 'multichannel.psd'
        mc.write_bytes(struct.pack('>4sH6xHIIHH', b'8BPS', 1, 3, mh, mw, 8, 7)
                       + struct.pack('>III', 0, 0, 0) + struct.pack('>H', 0)
                       + bytes((i * 37) % 251 for i in range(3 * mw * mh)))
        report = call('inspect_psd', path=str(mc))
        assert report.get('color_mode') == 'Multichannel', report
        assert report.get('layered_available') is False, report
        assert report.get('stored_composite_available') is False, report
        for key in ('layered_refusal', 'stored_composite_refusal'):
            assert MULTICHANNEL_FILE in (report.get(key) or ''), report
        opened = call('open_document', fail=True, path=str(mc), psd_mode='composite',
                      psd_fingerprint=report['source_fingerprint'])
        assert MULTICHANNEL_FILE in opened, opened
        g8['inspect_psd'] = {k: report.get(k) for k in (
            'color_mode', 'layered_available', 'stored_composite_available',
            'layered_refusal', 'stored_composite_refusal')}
        g8['open_document'] = opened
        receipts['g8'] = g8
        print('PASS g8: Multichannel refuses by name in convert_mode, inspect_psd and the open',
              flush=True)

        # ---- G9: Auto-Align and Reduce Noise from Stack --------------------
        app.client.timeout = 600  # a Debug build aligns and takes the median in one call
        g9 = {}
        fixtures = folder / 'g9'
        fixtures.mkdir()
        noise = Picture(-64, -64, ALIGN_SIZE[0] + 64, ALIGN_SIZE[1] + 64)
        frames = {'a': fixture(fixtures, 'frame-a', noise, ALIGN_SIZE, [1, 0, 0, 0, 1, 0, 0, 0, 1])}
        for key, k in ALIGN_K.items():
            frames[key] = fixture(fixtures, f'frame-{key}', noise, ALIGN_SIZE, inverse(k))

        def cursor(target):
            return call('list_history', document_id=target)['cursor']

        # G9a: the pair aligns on the view and every ink lands on A.
        adoc = cmyk_stack(call, [frames['a'], frames['b']])
        start, start_layout = cursor(adoc), layout(call, adoc)
        before = inks(call, adoc)
        call('set_layer_properties', document_id=adoc, index=1, visible=False)
        reference = inks(call, adoc)
        call('undo', document_id=adoc)
        assert cursor(adoc) == start, 'undo walks the hide back'
        reply = call('auto_align_layers', document_id=adoc, layers=[0, 1])
        assert reply['ok'] is True and reply['changed'] is True, reply
        assert [entry['layer'] for entry in reply['aligned']] == [1], reply
        worst = homography_error(reply['aligned'][0]['homography'], inverse(ALIGN_K['b']))
        assert worst <= BOUND_PX, f'max |G(q) - K^-1(q)| = {worst:.3f} px, bound {BOUND_PX}'
        assert cursor(adoc) == start + 1, 'one history step'
        grown = reply.get('canvas')
        origin = (grown['origin']['x'], grown['origin']['y']) if grown else (0, 0)
        after = inks(call, adoc, origin)
        was, now = mean_ink_error(before, reference), mean_ink_error(after, reference)
        for c in range(4):
            assert now[c] < was[c] / 2, (f'ink {c}: mean |after - A| {now[c]:.1f} is not below '
                                         f'half of mean |before - A| {was[c]:.1f}')
        call('undo', document_id=adoc)
        assert cursor(adoc) == start, 'undo walks the align back'
        assert layout(call, adoc) == start_layout, 'undo restores the layer rectangle and canvas'
        g9['align'] = {'worst_px': round(worst, 3), 'origin': origin,
                       'mean_before': [round(v, 1) for v in was],
                       'mean_after': [round(v, 1) for v in now]}

        # G9c: the lens corrections refuse by name, with no step.
        for key, sentence in (('vignette', VIGNETTE_CMYK), ('distortion', DISTORTION_CMYK)):
            refusal = call('auto_align_layers', fail=True, document_id=adoc, layers=[0, 1],
                           **{key: True})
            assert sentence in refusal, (key, refusal)
            assert cursor(adoc) == start, f'the {key} refusal leaves no step'
            g9[f'{key}_refusal'] = refusal
        call('close_document', document_id=adoc, discard_changes=True)

        # G9b: Reduce Noise aligns B and C and takes the median per ink.
        ndoc = cmyk_stack(call, [frames['a'], frames['b'], frames['c']])
        start = cursor(ndoc)
        call('set_layer_properties', document_id=ndoc, index=2, visible=False)
        unaligned = inks(call, ndoc)
        call('set_layer_properties', document_id=ndoc, index=1, visible=False)
        reference = inks(call, ndoc)
        call('undo', document_id=ndoc)
        call('undo', document_id=ndoc)
        assert cursor(ndoc) == start, 'undo walks the hides back'
        reply = call('reduce_noise_from_stack', document_id=ndoc, layers=[0, 1, 2], mode='median')
        assert [entry['layer'] for entry in reply['aligned']] == [1, 2], reply
        assert reply['unchanged'] == [] and reply['excluded'] == [], reply
        assert reply['layers'] == [0, 1, 2] and reply['hidden'] == [0, 1, 2], reply
        assert reply['layer'] == 3 and reply['name'] == 'Noise Reduced', reply
        assert call('get_color_profile', document_id=ndoc).get('color_mode') == 'cmyk'
        reduced = inks(call, ndoc)
        was, now = mean_ink_error(unaligned, reference), mean_ink_error(reduced, reference)
        for c in range(4):
            assert now[c] < was[c] / 2, (f'ink {c}: mean |reduced - A| {now[c]:.1f} is not below '
                                         f'half of mean |B unaligned - A| {was[c]:.1f}')
        call('undo', document_id=ndoc)
        assert cursor(ndoc) == start, 'one undo removes the result'
        assert len(call('get_document', document_id=ndoc)['layers']) == 3, 'the layer is gone'
        g9['reduce_noise'] = {'mean_unaligned': [round(v, 1) for v in was],
                              'mean_reduced': [round(v, 1) for v in now]}
        call('close_document', document_id=ndoc, discard_changes=True)
        receipts['g9'] = g9
        print('PASS g9: Auto-Align and Reduce Noise from Stack run on CMYK, the lens refuses',
              flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
