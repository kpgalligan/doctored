#!/usr/bin/env python3
"""The #547 kept-CMYK export drive (round 2): a CMYK document exported with
profile "document" as TIFF or JPEG — resized on ink, output-sharpened, at the
other depth, under a metadata policy — and the files read back; the Export
workspace's preview and plan; and the PSD export's vector-mask loss line.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App`: its own scratch MCP socket, a dummy API key and a PRIVATE
directory for every preference library) and is terminated by its own context
manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python, in `verify_cmyk_colour.cmyk_tiff`'s
layout (little-endian, chunky, one uncompressed strip, Generic CMYK Profile
embedded) plus, for the metadata fixture, an XMP packet (700), an EXIF IFD
(34665: DateTimeOriginal) beside IFD0's Make, and an IPTC resource (34377).
Every written TIFF and JPEG is parsed here, never by the app.

- **X1 resize.** On a 32 × 24 16-bit grid: `tiff` bit_depth 16, size width 16,
  filter nearest reopens 16 × 12 and every sample is the source's at the
  pixel-centre index map ((2x+1)·32/32, (2y+1)·24/24); scale 1.5, lanczos3
  writes 36 rows (48 × 36).
- **X2 sharpen.** On a C/K step edge at x = 16: Unsharp Mask (radius 1.5,
  amount 1) changes the ink of edge columns 15 and 16 against the unsharpened
  export, and the flat interior probe (4, 12) is unchanged; a `smart_sharpen`
  export succeeds.
- **X3 depth.** The 16-bit grid's `bit_depth` 8 TIFF reopens at depth 8 with
  every probed ink16 round(v/257)·257 of the document's, and its samples are
  round(v/257) of the 16-bit export's; an 8-bit fixture's `bit_depth` 16 TIFF
  reopens at depth 16 with every probed ink16 equal.
- **X4 metadata.** Under {exif: drop, xmp: keep, iptc: keep, stamp: {copyright}}:
  the TIFF has no ExifIFD (34665), XMP holding dc:rights and the copyright, the
  IPTC resource (34377) and the ICC; the JPEG has no `Exif\\0\\0` APP1 and an XMP
  APP1 holding the copyright and a JFIF density in dots per inch (density true);
  for both, the reply's metadata (exif false) equals what the bytes carry (exif,
  xmp, iptc, profile). The JPEG reopens CMYK. Under {exif: keep, xmp: drop,
  iptc: drop} the JPEG has an `Exif\\0\\0` APP1, no XMP APP1 and no IPTC
  APP13, and the reply's metadata (exif true, xmp false, iptc false) equals
  what the bytes carry. A default-policy kept JPEG still carries an
  `Exif\\0\\0` APP1 and reports exif true.
- **X5 preview.** `export_preview` of a kept TIFF at width 16 with Unsharp Mask
  answers a 16 × 12 PNG and bit_depth 16; of a kept JPEG, bit_depth 8.
  `export_image`'s `working_memory_plan_bytes` at scale 1.5 exceeds the one at
  size none.
- **X6 PSD.** On a CMYK document and an RGB document, each with a vector mask
  (`set_vector_mask`) on layer 0 and an alpha channel: `inspect_psd_export`'s
  losses hold "The vector mask on “<layer 0's name>” is written as pixels, an
  ordinary layer mask of its rendered shape; its path is not written." and "1
  saved channels are not written; this export stores no PSD alpha channels.",
  and `export_psd`'s reply has the same two lines.
"""
import argparse
import base64
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App
from verify_phase1 import png

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
W, H = 32, 24
EDGE = 16
FLAT_PROBE = (4, 12)
PROBES = [(0, 0), (5, 3), (13, 9), (22, 17), (31, 23)]
COPYRIGHT = '(c) 2026 Drive Press'
POLICY = {'exif': 'drop', 'xmp': 'keep', 'iptc': 'keep', 'stamp': {'copyright': COPYRIGHT}}
EXIF_ONLY = {'exif': 'keep', 'xmp': 'drop', 'iptc': 'drop'}
VECTOR = ('The vector mask on “{name}” is written as pixels, an ordinary layer mask of its '
          'rendered shape; its path is not written.')
CHANNELS = '1 saved channels are not written; this export stores no PSD alpha channels.'
XMP = (b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF "
       b"xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' "
       b"xmlns:dc='http://purl.org/dc/elements/1.1/'></rdf:Description></rdf:RDF></x:xmpmeta>")
IIM = b'\x1c\x02\x19\x00\x05Drive'


def iptc_resource():
    run = b'8BIM\x04\x04\x00\x00' + struct.pack('>I', len(IIM)) + IIM
    return run + (b'\x00' if len(IIM) % 2 else b'')


def cmyk_tiff(width, height, samples, icc, bits=16, metadata=False):
    """`verify_cmyk_colour.cmyk_tiff`'s layout at `bits`, plus (metadata) Make,
    XMP, the IPTC resource and an EXIF IFD."""
    data = struct.pack(f'<{len(samples)}H', *samples) if bits == 16 else bytes(samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    def short(v):
        return struct.pack('<HH', v, 0)

    bits_at = place(struct.pack('<4H', bits, bits, bits, bits))
    icc_at = place(icc)
    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, 4, struct.pack('<I', bits_at)), (259, 3, 1, short(1)),
        (262, 3, 1, short(5)), (273, 4, 1, struct.pack('<I', 8)),
        (277, 3, 1, short(4)), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, 1, struct.pack('<I', len(data))), (284, 3, 1, short(1)),
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    if metadata:
        make = b'DriveCam\x00'
        make_at = place(make)
        xmp_at = place(XMP)
        iptc = iptc_resource()
        iptc_at = place(iptc)
        stamp = b'2026:10:15 10:00:00\x00'
        stamp_at = place(stamp)
        if len(body) % 2:
            body.append(0)
        exif_at = 8 + len(body)
        exif = struct.pack('<H', 1) + struct.pack('<HHI', 36867, 2, len(stamp)) \
            + struct.pack('<I', stamp_at) + struct.pack('<I', 0)
        body.extend(exif)
        entries += [
            (271, 2, len(make), struct.pack('<I', make_at)),
            (700, 1, len(XMP), struct.pack('<I', xmp_at)),
            (34377, 7, len(iptc), struct.pack('<I', iptc_at)),
            (34665, 4, 1, struct.pack('<I', exif_at)),
        ]
    if len(body) % 2:
        body.append(0)
    entries.sort(key=lambda e: e[0])
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def tiff_read(path):
    """A written classic TIFF's IFD0 tags and its chunky samples:
    {tag: (type, count, value-or-offset)}, (width, height), bits, samples."""
    data = Path(path).read_bytes()
    endian = '<' if data[:2] == b'II' else '>'
    assert struct.unpack(endian + 'H', data[2:4])[0] == 42, 'classic TIFF'
    ifd = struct.unpack(endian + 'I', data[4:8])[0]
    count = struct.unpack(endian + 'H', data[ifd:ifd + 2])[0]
    sizes = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 7: 1, 16: 8}
    tags = {}
    for i in range(count):
        at = ifd + 2 + 12 * i
        tag, kind, n = struct.unpack(endian + 'HHI', data[at:at + 8])
        size = sizes.get(kind, 1) * n
        raw = data[at + 8:at + 12] if size <= 4 else \
            data[struct.unpack(endian + 'I', data[at + 8:at + 12])[0]:][:size]
        tags[tag] = (kind, n, raw)

    def numbers(tag):
        kind, n, raw = tags[tag]
        code = {3: 'H', 4: 'I'}[kind]
        return list(struct.unpack(endian + code * n, raw[:struct.calcsize(code) * n]))

    width, height = numbers(256)[0], numbers(257)[0]
    bits = numbers(258)[0]
    assert numbers(262)[0] == 5 and numbers(277)[0] == 4, 'chunky CMYK'
    assert numbers(259)[0] == 1, 'uncompressed'
    strips = b''.join(data[o:o + c] for o, c in zip(numbers(273), numbers(279)))
    samples = list(struct.unpack(endian + 'H' * (len(strips) // 2), strips)) if bits == 16 \
        else list(strips)
    return tags, (width, height), bits, samples


def jpeg_segments(path):
    """(marker, payload) up to the scan."""
    data = Path(path).read_bytes()
    assert data[:2] == b'\xff\xd8', 'SOI'
    out, at = [], 2
    while at + 4 <= len(data) and data[at] == 0xFF:
        marker = data[at + 1]
        if marker in (0xDA, 0xD9):
            break
        length = struct.unpack('>H', data[at + 2:at + 4])[0]
        out.append((marker, data[at + 4:at + 2 + length]))
        at += 2 + length
    return out


def jpeg_carries(path):
    segments = jpeg_segments(path)
    xmp = b''.join(p for m, p in segments if m == 0xE1 and p.startswith(b'http://ns.adobe.com/xap/1.0/\x00'))
    return {
        'exif': any(m == 0xE1 and p.startswith(b'Exif\x00\x00') for m, p in segments),
        'xmp': bool(xmp),
        'iptc': any(m == 0xED and p.startswith(b'Photoshop 3.0\x00') and b'8BIM\x04\x04' in p
                    for m, p in segments),
        'profile': any(m == 0xE2 and p.startswith(b'ICC_PROFILE\x00') for m, p in segments),
    }, xmp.decode('utf-8', 'replace')


def png_size(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n', 'a PNG'
    return struct.unpack('>II', data[16:24])


def codes(percent):
    return [int(p / 100 * 65535 + 0.5) for p in percent]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-export-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_export.py before any measurement'}

    # A 16-bit grid with every pixel distinct, its ink kept for the oracles.
    grid = []
    for y in range(H):
        for x in range(W):
            grid.append([(x * 1987 + y * 3001 + c * 7919) % 60000 + 1000 for c in range(4)])
    grid_path = folder / 'grid.tif'
    grid_path.write_bytes(cmyk_tiff(W, H, [v for p in grid for v in p], profile, metadata=True))
    eight_path = folder / 'grid8.tif'
    eight_path.write_bytes(cmyk_tiff(W, H, [v // 257 for p in grid for v in p], profile, bits=8))
    left, right = codes([20, 30, 10, 60]), codes([70, 30, 10, 10])
    step_path = folder / 'step.tif'
    step_path.write_bytes(cmyk_tiff(W, H, [v for y in range(H) for x in range(W)
                                           for v in (left if x < EDGE else right)], profile))

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def ink(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            assert reply.get('color_mode') == 'cmyk', reply
            return reply['ink16']

        outputs = folder / 'out'
        outputs.mkdir()

        def export(doc, name, **settings):
            path = outputs / name
            reply = call('export_image', document_id=doc, path=str(path),
                         settings=dict(profile='document', **settings))
            return path, reply

        def reopened(path):
            doc = call('open_document', path=str(path))['document']['id']
            return doc, call('get_precision', document_id=doc)

        doc = call('open_document', path=str(grid_path))['document']['id']
        assert call('get_precision', document_id=doc).get('color_mode') == 'cmyk'

        # ---- X1: resize ------------------------------------------------------------
        path, reply = export(doc, 'nearest.tif', format='tiff', bit_depth=16, filter='nearest',
                             size={'mode': 'width', 'value': 16})
        assert (reply.get('width'), reply.get('height')) == (16, 12), reply
        tags, size, bits, samples = tiff_read(path)
        assert size == (16, 12) and bits == 16, (size, bits)
        for y in range(12):
            for x in range(16):
                sx, sy = (2 * x + 1) * W // (2 * 16), (2 * y + 1) * H // (2 * 12)
                assert samples[(y * 16 + x) * 4:(y * 16 + x) * 4 + 4] == grid[sy * W + sx], (x, y)
        again, precision = reopened(path)
        layer = call('get_document', document_id=again)['layers'][0]
        assert (layer.get('width'), layer.get('height')) == (16, 12), layer
        call('close_document', document_id=again, discard_changes=True)
        path, reply = export(doc, 'lanczos.tif', format='tiff', bit_depth=16, filter='lanczos3',
                             size={'mode': 'scale', 'value': 1.5})
        _, size, _, _ = tiff_read(path)
        assert size == (48, 36) and reply.get('height') == 36, (size, reply)
        receipts['x1'] = {'nearest': [16, 12], 'lanczos3': list(size)}
        print('PASS x1: nearest resize is the index map on ink; scale 1.5 writes 36 rows', flush=True)

        # ---- X2: sharpen -----------------------------------------------------------
        step = call('open_document', path=str(step_path))['document']['id']
        plain, _ = export(step, 'step.tif', format='tiff', bit_depth=16)
        sharp, _ = export(step, 'step-sharp.tif', format='tiff', bit_depth=16,
                          sharpen={'radius': 1.5, 'amount': 1.0, 'threshold': 0.0})
        _, _, _, before = tiff_read(plain)
        _, _, _, after = tiff_read(sharp)

        def at(samples, x, y):
            return samples[(y * W + x) * 4:(y * W + x) * 4 + 4]

        for x in (EDGE - 1, EDGE):
            assert at(after, x, H // 2) != at(before, x, H // 2), ('edge column', x)
        assert at(after, *FLAT_PROBE) == at(before, *FLAT_PROBE), 'flat interior unchanged'
        smart, reply = export(step, 'step-smart.tif', format='tiff', bit_depth=16,
                              sharpen={'method': 'smart_sharpen'})
        assert smart.is_file() and reply.get('ok') is True, reply
        receipts['x2'] = {'edge_before': at(before, EDGE, H // 2), 'edge_after': at(after, EDGE, H // 2)}
        call('close_document', document_id=step, discard_changes=True)
        print('PASS x2: Unsharp Mask sharpens the edge and leaves flat ink; Smart Sharpen exports',
              flush=True)

        # ---- X3: depth -------------------------------------------------------------
        sixteen, _ = export(doc, 'sixteen.tif', format='tiff', bit_depth=16)
        eight, reply = export(doc, 'eight.tif', format='tiff', bit_depth=8)
        assert reply.get('bit_depth') == 8, reply
        _, _, bits16, s16 = tiff_read(sixteen)
        _, _, bits8, s8 = tiff_read(eight)
        assert (bits16, bits8) == (16, 8)
        assert s8 == [round(v / 257) for v in s16], '16 -> 8 rounds'
        again, precision = reopened(eight)
        assert precision.get('bits_per_channel') == 8, precision
        for p in PROBES:
            assert ink(again, *p) == [round(v / 257) * 257 for v in ink(doc, *p)], p
        call('close_document', document_id=again, discard_changes=True)
        small = call('open_document', path=str(eight_path))['document']['id']
        assert call('get_precision', document_id=small).get('bits_per_channel') == 8
        wide, reply = export(small, 'wide.tif', format='tiff', bit_depth=16)
        assert reply.get('bit_depth') == 16, reply
        again, precision = reopened(wide)
        assert precision.get('bits_per_channel') == 16, precision
        for p in PROBES:
            assert ink(again, *p) == ink(small, *p), p
        call('close_document', document_id=again, discard_changes=True)
        call('close_document', document_id=small, discard_changes=True)
        receipts['x3'] = {'probes': {str(p): ink(doc, *p) for p in PROBES}}
        print('PASS x3: 16 -> 8 rounds to the 8-bit grid and 8 -> 16 keeps the ink', flush=True)

        # ---- X4: metadata ----------------------------------------------------------
        path, reply = export(doc, 'policy.tif', format='tiff', bit_depth=16, metadata=POLICY)
        tags, _, _, _ = tiff_read(path)
        xmp = tags.get(700, (0, 0, b''))[2].decode('utf-8', 'replace')
        carried = {'exif': 34665 in tags, 'xmp': 700 in tags, 'iptc': 34377 in tags,
                   'profile': 34675 in tags}
        assert carried == {'exif': False, 'xmp': True, 'iptc': True, 'profile': True}, carried
        assert 'dc:rights' in xmp and COPYRIGHT in xmp, xmp
        metadata = reply.get('metadata') or {}
        assert {k: metadata.get(k) for k in carried} == carried, (metadata, carried)
        path, reply = export(doc, 'policy.jpg', format='jpeg', metadata=POLICY)
        carried, xmp = jpeg_carries(path)
        assert not carried['exif'], 'no Exif APP1'
        assert carried['xmp'] and COPYRIGHT in xmp, xmp
        metadata = reply.get('metadata') or {}
        assert {k: metadata.get(k) for k in carried} == carried, (metadata, carried)
        assert metadata.get('exif') is False, metadata
        jfif = next(p for m, p in jpeg_segments(path) if m == 0xE0 and p.startswith(b'JFIF\x00'))
        assert jfif[7] == 1 and metadata.get('density') is True, (jfif, metadata)
        again = call('open_document', path=str(path))['document']['id']
        assert call('get_color_profile', document_id=again).get('color_mode') == 'cmyk'
        call('close_document', document_id=again, discard_changes=True)
        path, reply = export(doc, 'exif-only.jpg', format='jpeg', metadata=EXIF_ONLY)
        exif_only, _ = jpeg_carries(path)
        assert (exif_only['exif'], exif_only['xmp'], exif_only['iptc']) == (True, False, False), exif_only
        metadata = reply.get('metadata') or {}
        assert {k: metadata.get(k) for k in exif_only} == exif_only, (metadata, exif_only)
        default, reply = export(doc, 'default.jpg', format='jpeg')
        kept, _ = jpeg_carries(default)
        assert kept['exif'] and (reply.get('metadata') or {}).get('exif') is True, (kept, reply)
        receipts['x4'] = {'jpeg': carried, 'exif_only_jpeg': exif_only, 'default_jpeg': kept}
        print('PASS x4: the TIFF and the JPEG follow the policy and report what they carry',
              flush=True)

        # ---- X5: preview and plan ---------------------------------------------------
        result = app.client.rpc('tools/call', {'name': 'export_preview', 'arguments': dict(
            document_id=doc, settings={'format': 'tiff', 'bit_depth': 16, 'profile': 'document',
                                       'size': {'mode': 'width', 'value': 16},
                                       'sharpen': {'radius': 1.0, 'amount': 1.0,
                                                   'threshold': 0.0}})})
        assert not result.get('isError'), result
        image = next(c for c in result['content'] if c['type'] == 'image')
        fields = json.loads(next(c['text'] for c in result['content'] if c['type'] == 'text'))
        assert png_size(base64.b64decode(image['data'])) == (16, 12)
        assert fields.get('bit_depth') == 16, fields
        jpeg = call('export_preview', document_id=doc,
                    settings={'format': 'jpeg', 'profile': 'document'})
        assert jpeg.get('bit_depth') == 8, jpeg
        _, none = export(doc, 'plan-none.tif', format='tiff', bit_depth=16)
        _, scaled = export(doc, 'plan-scaled.tif', format='tiff', bit_depth=16,
                           size={'mode': 'scale', 'value': 1.5})
        assert scaled['working_memory_plan_bytes'] > none['working_memory_plan_bytes'], (scaled, none)
        receipts['x5'] = {'plan_none': none['working_memory_plan_bytes'],
                          'plan_scaled': scaled['working_memory_plan_bytes']}
        call('close_document', document_id=doc, discard_changes=True)
        print('PASS x5: the preview is the resized kept TIFF at 16 bits; the plan follows the size',
              flush=True)

        # ---- X6: PSD ---------------------------------------------------------------
        rgb_path = folder / 'rgb.png'
        png(rgb_path, W, H, bytes([90, 140, 200, 255] * (W * H)))
        shape = [{'closed': True, 'anchors': [[4, 3], [26, 5], [24, 20], [6, 18]]}]
        psd = {}
        for label, source in (('cmyk', step_path), ('rgb', rgb_path)):
            document = call('open_document', path=str(source))['document']['id']
            call('set_vector_mask', document_id=document, layer=0, subpaths=shape)
            call('add_channel', document_id=document, name='Alpha 1')
            name = call('get_document', document_id=document)['layers'][0]['name']
            sentence = VECTOR.format(name=name)
            report = call('inspect_psd_export', document_id=document, format='psd', mode='layers')
            losses = report.get('losses') or []
            assert sentence in losses and CHANNELS in losses, (label, losses)
            written = call('export_psd', document_id=document, path=str(folder / f'{label}.psd'),
                           format='psd', mode='layers')
            assert sentence in written['losses'] and CHANNELS in written['losses'], (label, written)
            psd[label] = losses
            call('close_document', document_id=document, discard_changes=True)
        receipts['x6'] = psd
        print('PASS x6: the PSD export names the vector mask written as pixels and the channels',
              flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True, default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
