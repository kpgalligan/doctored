#!/usr/bin/env python3
"""MCP drive for a float document's 32-bit PSD and PSB export and reopen
(which replaced the earlier refusal) and for Export as PDF at 16 bits and a
JPEG-compressed page.

Every gate is decided from the written FILES, parsed here: the PDF's image
XObject (bits per component, filter, soft mask, the decompressed samples) is
read by this file's own regex walk, never by the app. The 11D drive's helpers
(`scripts/verify_phase11d_formats.py`) supply the PNG writer/reader and the
PDF object walk, loaded by path so nothing here edits them.

- G1: a float document's inspect_psd_export is available at depth 32 for
  psd and psb in both modes, with the 32-bit first limitation written out
  literally; format tiff is available too, with its own sentence, in
  both modes, export_psd writes a big-endian TIFF of 32 bits a sample in
  both, and the layered TIFF inspects as rgba32 and reopens (psd_mode
  layers) as a float document whose 32-bit TIFF export equals the source's
  bit for bit; export_psd layers and flattened report bit_depth
  32 and write a depth-32 header; the layered file inspects as rgba32 and
  reopens (open_document psd_mode layers) as a float document whose 32-bit
  TIFF export equals the source's bit for bit; Photoshop's own 32-bit file
  (`core/tests/fixtures/psd_photoshop/depth32/source32.psd`, committed by
  the Photoshop take) inspects as rgba32 and opens layered; 8-bit and
  16-bit documents still export at their own depth. Never launches Photoshop.
- G2: a byte document's default PDF is 8 bpc, ZIP, no soft mask, and its image
  stream equals the 8-bit PNG export's RGB.
- G3: a 16-bit document's default PDF is 16 bpc, its RGB and 16-bit soft mask
  equal the 16-bit PNG export exactly; bit_depth 8 equals the 8-bit PNG export.
- G4: a float document's 16-bit page equals its 16-bit PNG export (tone
  mapped), and settings={} succeeds at the PDF default depth (16).
- G5: JPEG on an opaque document: DCTDecode, the stream starts FFD8, the image
  is ICCBased, no soft mask; sips's decode against the PNG export is within
  T5_JPEG_MEAN_CODES mean |delta| (fixed before measuring).
- G6: JPEG on a transparent document: its soft mask equals the PNG alpha.
- G7: every refusal is in-band, names its reason, and writes no file.
- G8: every result's bits_per_component / compression / has_soft_mask equals
  the parsed file.

Recorded, never gated: wall-clock and polled max RSS of a 16-bit export at
6000 px wide (and at 4000 px when the working-memory admission refuses 6000).
"""
import app_under_test
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import threading
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]

_spec = importlib.util.spec_from_file_location('phase11d_formats',
                                               ROOT / 'scripts/verify_phase11d_formats.py')
d11 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(d11)
_spec = importlib.util.spec_from_file_location('phase11f_float',
                                               ROOT / 'scripts/verify_phase11f_float.py')
f11 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(f11)

# ---- fixed before anything is measured -------------------------------------
T5_JPEG_MEAN_CODES = 3.0
FLOAT_FIXTURE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
WIDTH = 64
HEIGHT = 48

FLOAT_FIRST = {
    'psd': ("Only 32-bit RGB classic PSD is written for a 32-bit float document: linear-light "
            "float samples in the document's own primaries under its profile's linear (gamma "
            "1.0) twin, uncompressed (Raw) channels and layer records in Photoshop's Lr32 "
            "block. A style or adjustment layer that 8 and 16 bits would rasterize refuses the "
            "layered mode instead, and a 32-bit document is never written at a lower depth."),
    'psb': ("Only 32-bit RGB large-document PSB is written for a 32-bit float document: "
            "linear-light float samples in the document's own primaries under its profile's "
            "linear (gamma 1.0) twin, uncompressed (Raw) channels and layer records in "
            "Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would "
            "rasterize refuses the layered mode instead, and a 32-bit document is never written "
            "at a lower depth."),
    'tiff': ("Only a 32-bit RGB TIFF is written for a 32-bit float document: a flat image of "
             "linear-light float samples with Photoshop's layer block beside it, layer records "
             "in Photoshop's Lr32 block, uncompressed (Raw) channels, Macintosh byte order, "
             "under the profile's linear (gamma 1.0) twin. A style or adjustment layer that 8 "
             "and 16 bits would rasterize refuses the layered mode instead, and a 32-bit "
             "document is never written at a lower depth."),
}
SOURCE32 = ROOT / 'core/tests/fixtures/psd_photoshop/depth32/source32.psd'
JPEG16_REFUSAL = 'JPEG compression is 8-bit only; choose ZIP to keep 16 bits.'
DEPTH_REFUSAL = 'A PDF page holds an 8-bit or 16-bit image; bit_depth must be 8 or 16.'
BYTE16_REFUSAL = 'an 8-bit document exports at 8 bits; convert it to 16-bit first'
FORMAT_REFUSAL = ('export_pdf fixes format: a PDF page holds one image. Remove the format key; '
                  'choose bit_depth and compression instead.')
DITHER_REFUSAL = 'dithering applies only when reducing to 8 bits'
COMPRESSION_REFUSAL = 'export_pdf compression must be zip or jpeg.'
FILTER_NAMES = {'FlateDecode': 'zip', 'DCTDecode': 'jpeg', None: 'none'}


def rgba16_fixture():
    """16-bit RGBA whose codes are not multiples of 257 (bar opaque alpha), with
    partial alpha on the right half."""
    out = []
    for y in range(HEIGHT):
        for x in range(WIDTH):
            pixel = [(x * 997 + y * 131 + 1) % 65536,
                     (x * 311 + y * 1733 + 7) % 65536,
                     (x * 53 + y * 419 + 12345) % 65536,
                     65535 if x < WIDTH // 2 else 1000 + (x * 1021 + y * 97) % 60000]
            # Opaque alpha stays 65535; every other code is moved off the
            # x257 grid, so an 8-bit round trip cannot pass for 16 bits.
            out += [value if value == 65535 else value + 1 if value % 257 == 0 else value
                    for value in pixel]
    return out


def pdf_images(path):
    """The colour image XObject (it references /SMask, or its ColorSpace is not
    /DeviceGray) and its soft mask, resolved by object reference."""
    data = Path(path).read_bytes()
    objects = d11.pdf_objects(data)
    colour = None
    for number, body in objects.items():
        head = body.split(b'stream', 1)[0]
        if b'/Subtype' not in head or b'/Image' not in head:
            continue
        if b'/SMask' in head or not re.search(rb'/ColorSpace\s*/DeviceGray', head):
            colour = (number, body)
            break
    if colour is None:
        return None
    number, body = colour

    def facts(body):
        head = body.split(b'stream', 1)[0]
        bits = int(re.search(rb'/BitsPerComponent\s+(\d+)', head).group(1))
        match = re.search(rb'/Filter\s*\[?\s*/(\w+)', head)
        name = match.group(1).decode() if match else None
        stream = d11.pdf_stream(body)
        raw = stream
        if name == 'FlateDecode' and stream is not None:
            raw = zlib.decompress(stream)
        if raw is not None and name in (None, 'FlateDecode'):
            samples = (list(struct.unpack('>%dH' % (len(raw) // 2), raw)) if bits == 16
                       else list(raw))
        else:
            samples = None
        return dict(bits=bits, filter=name, stream=stream, samples=samples,
                    predictor=b'/Predictor' in head,
                    width=int(re.search(rb'/Width\s+(\d+)', head).group(1)),
                    height=int(re.search(rb'/Height\s+(\d+)', head).group(1)))

    image = facts(body)
    head = body.split(b'stream', 1)[0]
    space = re.search(rb'/ColorSpace\s*(\d+)\s+0\s+R', head)
    icc = b'/ICCBased' in head
    if space and int(space.group(1)) in objects:
        icc = icc or b'/ICCBased' in objects[int(space.group(1))]
    image['icc_based'] = icc
    mask_ref = re.search(rb'/SMask\s+(\d+)\s+0\s+R', head)
    image['has_smask'] = mask_ref is not None
    image['smask'] = facts(objects[int(mask_ref.group(1))]) if mask_ref else None
    image['object'] = number
    return image


def summary(image):
    """What the receipt keeps of a parsed image (no sample arrays)."""
    if image is None:
        return None
    out = {k: v for k, v in image.items() if k not in ('stream', 'samples', 'smask')}
    out['stream_bytes'] = len(image['stream'] or b'')
    if image.get('smask'):
        out['smask'] = {k: v for k, v in image['smask'].items() if k not in ('stream', 'samples')}
    return out


class Drive:
    def __init__(self, app, output):
        self.app = app
        self.output = output
        self.report = dict(gates={}, g8=[], recorded={})

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def refuse(self, tool, **arguments):
        return self.app.call(tool, fail=True, **arguments)

    def open(self, path, **arguments):
        return self.call('open_document', path=str(path), **arguments)['document']['id']

    def png(self, document, name, bit_depth):
        where = self.output / name
        self.call('export_image', document_id=document, path=str(where),
                  settings={'format': 'png', 'bit_depth': bit_depth}, overwrite=True)
        return d11.read_png(where)

    def pdf(self, document, name, settings=None, **arguments):
        where = self.output / name
        result = self.call('export_pdf', document_id=document, path=str(where), overwrite=True,
                           settings=settings if settings is not None else {}, **arguments)
        image = pdf_images(where)
        self.g8(name, result, image)
        return result, image, where

    def g8(self, name, result, image):
        measured = None if image is None else dict(
            bits_per_component=image['bits'], compression=FILTER_NAMES.get(
                image['filter'], (image['filter'] or '').lower()),
            has_soft_mask=image['has_smask'])
        reported = {key: result.get(key) for key in
                    ('bits_per_component', 'compression', 'has_soft_mask')}
        self.report['g8'].append(dict(name=name, reported=reported, measured=measured,
                                      passed=measured == reported))

    def gate(self, name, checks, detail):
        passed = bool(checks) and all(checks.values())
        detail = dict(detail, checks=checks)
        self.report['gates'][name] = dict(passed=passed, detail=detail)


def float_tiff_bytes(drive, document, name):
    """The document's flat 32-bit TIFF export, its f32 samples packed back to
    bytes so equality is by bits."""
    where = drive.output / name
    reply = drive.call('export_image', document_id=document, path=str(where),
                       settings={'format': 'tiff', 'bit_depth': 32}, overwrite=True)
    assert reply.get('ok'), reply
    tiff = f11.read_float_tiff(where)
    return (tiff['width'], tiff['height'], tiff['channels'],
            struct.pack('<%df' % len(tiff['samples']), *tiff['samples']))


def tiff_header(where):
    """A TIFF's byte order and IFD0's BitsPerSample values, parsed here."""
    data = where.read_bytes() if where.exists() else b''
    if data[:4] not in (b'MM\0*', b'II*\0'):
        return None, []
    e = '>' if data[:2] == b'MM' else '<'
    first = struct.unpack(e + 'I', data[4:8])[0]
    count = struct.unpack(e + 'H', data[first:first + 2])[0]
    for i in range(count):
        at = first + 2 + 12 * i
        tag, kind, n = struct.unpack(e + 'HHI', data[at:at + 8])
        if tag == 258 and kind == 3:
            raw = data[at + 8:at + 12] if n <= 2 else data[
                struct.unpack(e + 'I', data[at + 8:at + 12])[0]:][:2 * n]
            return data[:2].decode(), list(struct.unpack(e + 'H' * n, raw[:2 * n]))
    return data[:2].decode(), []


def g1_psd(drive, docs):
    checks, detail = {}, {}
    for container in ('psd', 'psb', 'tiff'):
        for mode in ('layers', 'flattened'):
            report = drive.call('inspect_psd_export', document_id=docs['float'], mode=mode,
                                format=container)
            detail[f'inspect_{container}_{mode}'] = {
                k: report.get(k) for k in ('available', 'refusal', 'depth', 'format', 'mode')}
            first = (report.get('general_limitations') or [None])[0]
            checks[f'inspect_{container}_{mode}'] = (
                report.get('available') is True and report.get('refusal') is None
                and report.get('depth') == 32 and report.get('mode') == mode
                and first == FLOAT_FIRST[container])
            if container == 'tiff':
                # The 32-bit layered TIFF, big-endian, 32 bits a sample.
                where = drive.output / f'float-{mode}.tif'
                result = drive.call('export_psd', document_id=docs['float'], path=str(where),
                                    mode=mode, format='tiff')
                order, bits = tiff_header(where)
                detail[f'export_tiff_{mode}'] = dict(
                    {k: result.get(k) for k in ('ok', 'bit_depth', 'mode')},
                    order=order, bits=bits)
                checks[f'export_tiff_{mode}_32'] = (
                    result.get('ok') is True and result.get('bit_depth') == 32
                    and order == 'MM' and bits and all(b == 32 for b in bits))
    tiff = drive.output / 'float-layers.tif'
    report = drive.call('inspect_psd', path=str(tiff))
    detail['tiff_reopen_report'] = {k: report.get(k) for k in
                                    ('format', 'import_precision', 'layered_available',
                                     'stored_composite_available', 'depth')}
    checks['tiff_reopen_rgba32'] = (report.get('format') == 'TIFF'
                                    and report.get('import_precision') == 'rgba32'
                                    and report.get('layered_available') is True)
    reopened = drive.call('open_document', path=str(tiff), psd_mode='layers',
                          psd_fingerprint=report['source_fingerprint'])['document']['id']
    source = float_tiff_bytes(drive, docs['float'], 'source32-for-tiff.tif')
    back = float_tiff_bytes(drive, reopened, 'reopened32-from-tiff.tif')
    detail['tiff_reopen_shapes'] = dict(source=source[:3], reopened=back[:3])
    checks['tiff_reopen_bit_for_bit'] = source == back
    drive.call('close_document', document_id=reopened, discard_changes=True)
    for mode in ('layers', 'flattened'):
        where = drive.output / f'float-{mode}.psd'
        result = drive.call('export_psd', document_id=docs['float'], path=str(where), mode=mode)
        detail[f'export_{mode}'] = {k: result.get(k) for k in ('ok', 'bit_depth', 'mode')}
        header = where.read_bytes()[:26] if where.exists() else b''
        checks[f'export_{mode}_32'] = (result.get('ok') is True and result.get('bit_depth') == 32
                                       and header[:4] == b'8BPS'
                                       and struct.unpack('>H', header[22:24])[0] == 32)
    layered = drive.output / 'float-layers.psd'
    report = drive.call('inspect_psd', path=str(layered))
    detail['reopen_report'] = {k: report.get(k) for k in
                               ('import_precision', 'layered_available',
                                'stored_composite_available', 'depth')}
    checks['reopen_rgba32'] = (report.get('import_precision') == 'rgba32'
                               and report.get('layered_available') is True
                               and report.get('stored_composite_available') is True)
    reopened = drive.call('open_document', path=str(layered), psd_mode='layers',
                          psd_fingerprint=report['source_fingerprint'])['document']['id']
    source = float_tiff_bytes(drive, docs['float'], 'source32.tif')
    back = float_tiff_bytes(drive, reopened, 'reopened32.tif')
    detail['tiff_shapes'] = dict(source=source[:3], reopened=back[:3])
    checks['reopen_tiff_bit_for_bit'] = source == back
    drive.call('close_document', document_id=reopened, discard_changes=True)
    if SOURCE32.exists():
        report = drive.call('inspect_psd', path=str(SOURCE32))
        detail['photoshop_report'] = {k: report.get(k) for k in
                                      ('import_precision', 'layered_available',
                                       'stored_composite_available', 'layer_count')}
        opened = drive.call('open_document', path=str(SOURCE32), psd_mode='layers',
                            psd_fingerprint=report['source_fingerprint'])['document']
        detail['photoshop_open'] = {k: opened.get(k) for k in ('id', 'precision', 'layer_count')}
        checks['photoshop_source32_rgba32'] = (report.get('import_precision') == 'rgba32'
                                               and report.get('layered_available') is True
                                               and report.get('stored_composite_available')
                                               is True)
        drive.call('close_document', document_id=opened['id'], discard_changes=True)
    else:
        detail['photoshop_source32'] = 'absent: the Photoshop take has not committed it'
    for key, depth in (('photo', 8), ('rgba16', 16)):
        result = drive.call('export_psd', document_id=docs[key],
                            path=str(drive.output / f'{key}.psd'), mode='layers')
        detail[f'{key}_psd'] = {k: result.get(k) for k in ('ok', 'bit_depth', 'mode')}
        checks[f'{key}_psd_{depth}'] = result.get('ok') is True and result.get('bit_depth') == depth
    drive.gate('G1 float PSD and PSB at 32 bits', checks, detail)


def g2_byte_default(drive, docs):
    result, image, _ = drive.pdf(docs['photo'], 'photo-default.pdf')
    png = drive.png(docs['photo'], 'photo-8.png', 8)
    rgb = d11.rgb_of(png['samples'], png['colour'])
    checks = dict(bits=image['bits'] == 8, zip=image['filter'] == 'FlateDecode',
                  no_smask=not image['has_smask'],
                  rgb_equal=image['samples'] == rgb)
    drive.gate('G2 byte default PDF unchanged', checks,
               dict(result=result, image=summary(image)))


def g3_sixteen(drive, docs):
    checks, detail = {}, {}
    result, image, _ = drive.pdf(docs['rgba16'], 'rgba16-default.pdf')
    png = drive.png(docs['rgba16'], 'rgba16-16.png', 16)
    rgb = d11.rgb_of(png['samples'], png['colour'])
    alpha = d11.alpha_of(png['samples'], png['colour'])
    detail['default'] = dict(result=result, image=summary(image))
    checks['default_16'] = image['bits'] == 16 and image['filter'] == 'FlateDecode'
    checks['default_rgb_equal'] = image['samples'] == rgb
    if image['samples'] != rgb and image['samples'] is not None:
        detail['default_rgb_stats'] = d11.stats(image['samples'][:len(rgb)], rgb)
    mask = image.get('smask') or {}
    checks['default_smask_16'] = mask.get('bits') == 16
    checks['default_alpha_equal'] = mask.get('samples') == alpha
    if mask.get('samples') and mask.get('samples') != alpha:
        detail['default_alpha_stats'] = d11.stats(mask['samples'][:len(alpha)], alpha)

    result8, image8, _ = drive.pdf(docs['rgba16'], 'rgba16-8.pdf', settings={'bit_depth': 8})
    png8 = drive.png(docs['rgba16'], 'rgba16-8.png', 8)
    detail['eight'] = dict(result=result8, image=summary(image8))
    checks['eight_8'] = image8['bits'] == 8
    checks['eight_rgb_equal'] = image8['samples'] == d11.rgb_of(png8['samples'], png8['colour'])
    checks['eight_alpha_equal'] = ((image8.get('smask') or {}).get('samples')
                                   == d11.alpha_of(png8['samples'], png8['colour']))
    drive.gate('G3 16-bit document PDF', checks, detail)


def g4_float(drive, docs):
    checks, detail = {}, {}
    result, image, _ = drive.pdf(docs['float'], 'float-16.pdf', settings={'bit_depth': 16})
    png = drive.png(docs['float'], 'float-16.png', 16)
    detail['sixteen'] = dict(result=result, image=summary(image))
    checks['sixteen_16'] = image['bits'] == 16
    checks['sixteen_rgb_equal'] = image['samples'] == d11.rgb_of(png['samples'], png['colour'])
    alpha = d11.alpha_of(png['samples'], png['colour'])
    if image['has_smask']:
        checks['sixteen_alpha_equal'] = image['smask'].get('samples') == alpha
    else:
        checks['sixteen_opaque'] = all(value == 65535 for value in alpha)
    default, default_image, _ = drive.pdf(docs['float'], 'float-default.pdf')
    detail['default'] = dict(result=default, image=summary(default_image))
    checks['default_ok_16'] = default.get('ok') is True and default_image['bits'] == 16
    drive.gate('G4 float document PDF', checks, detail)


def g5_jpeg_opaque(drive, docs):
    checks, detail = {}, {}
    result, image, where = drive.pdf(docs['photo'], 'photo-jpeg.pdf', compression='jpeg')
    detail['result'] = result
    detail['image'] = summary(image)
    checks['dct'] = image['filter'] == 'DCTDecode'
    checks['ffd8'] = (image['stream'] or b'')[:2] == b'\xff\xd8'
    checks['icc_based'] = image['icc_based']
    checks['no_smask'] = not image['has_smask']
    jpeg = drive.output / 'photo-jpeg-stream.jpg'
    jpeg.write_bytes(image['stream'] or b'')
    decoded = drive.output / 'photo-jpeg-sips.png'
    run = subprocess.run(['sips', '-s', 'format', 'png', str(jpeg), '--out', str(decoded)],
                         capture_output=True, text=True, check=False)
    detail['sips'] = run.returncode
    png = d11.read_png(drive.output / 'photo-8.png')
    if decoded.is_file():
        theirs = d11.read_png(decoded)
        measured = d11.stats(d11.rgb_of(theirs['samples'], theirs['colour']),
                             d11.rgb_of(png['samples'], png['colour']))
        detail['sips_vs_png'] = dict(measured=measured, fixed_mean_bound=T5_JPEG_MEAN_CODES)
        checks['sips_mean_within'] = measured['mean'] <= T5_JPEG_MEAN_CODES
    else:
        checks['sips_decoded'] = False
    zip_bytes = (drive.output / 'photo-default.pdf').stat().st_size
    detail['sizes_recorded'] = dict(zip_pdf=zip_bytes, jpeg_pdf=where.stat().st_size)
    drive.gate('G5 JPEG opaque page', checks, detail)


def g6_jpeg_alpha(drive, docs):
    checks, detail = {}, {}
    result, image, _ = drive.pdf(docs['pdf'], 'pdf-jpeg.pdf', compression='jpeg')
    png = drive.png(docs['pdf'], 'pdf-8.png', 8)
    detail['result'] = result
    detail['image'] = summary(image)
    checks['dct'] = image['filter'] == 'DCTDecode'
    mask = image.get('smask') or {}
    alpha = d11.alpha_of(png['samples'], png['colour'])
    checks['smask_8'] = mask.get('bits') == 8
    checks['alpha_equal'] = mask.get('samples') == alpha
    checks['has_transparency'] = any(value < 255 for value in alpha)
    drive.gate('G6 JPEG transparent page', checks, detail)


def g7_refusals(drive, docs):
    checks, detail = {}, {}
    cases = [
        ('jpeg16', docs['rgba16'], dict(settings={'bit_depth': 16}, compression='jpeg'),
         lambda text: text == JPEG16_REFUSAL),
        ('float32', docs['float'], dict(settings={'bit_depth': 32}),
         lambda text: text == DEPTH_REFUSAL),
        ('byte16', docs['photo'], dict(settings={'bit_depth': 16}),
         lambda text: BYTE16_REFUSAL in text),
        ('format', docs['photo'], dict(settings={'format': 'png'}),
         lambda text: text == FORMAT_REFUSAL),
        ('dither16', docs['rgba16'], dict(settings={'bit_depth': 16, 'dither': 'ordered'}),
         lambda text: DITHER_REFUSAL in text),
        ('lzw', docs['photo'], dict(settings={}, compression='lzw'),
         lambda text: text == COMPRESSION_REFUSAL),
    ]
    for name, document, arguments, accepts in cases:
        where = drive.output / f'refused-{name}.pdf'
        text = drive.refuse('export_pdf', document_id=document, path=str(where), **arguments)
        detail[name] = text
        checks[name] = accepts(text) and not where.exists()
    drive.gate('G7 refusals', checks, detail)


def recorded_large(drive, docs):
    """The 6000-px 16-bit export's wall clock and polled max RSS. When the
    working-memory admission refuses it, the refusal is recorded and 4000 px
    is measured as well."""
    for width in (6000, 4000):
        outcome = measure_large(drive, docs, width)
        drive.report['recorded'][f'large_16bit_export_{width}'] = outcome
        if 'error' not in outcome['result']:
            break


def measure_large(drive, docs, width):
    pid = drive.app.process.pid
    peak = [0]
    done = threading.Event()

    def rss():
        out = subprocess.run(['ps', '-o', 'rss=', '-p', str(pid)], capture_output=True,
                             text=True, check=False).stdout.strip()
        return int(out) if out.isdigit() else None

    def poll():
        while not done.is_set():
            value = rss()
            if value is not None:
                peak[0] = max(peak[0], value)
            time.sleep(0.02)

    thread = threading.Thread(target=poll, daemon=True)
    before = rss()
    thread.start()
    start = time.monotonic()
    where = drive.output / f'large-16-{width}.pdf'
    try:
        result = drive.call('export_pdf', document_id=docs['rgba16'], path=str(where),
                            overwrite=True,
                            settings={'bit_depth': 16, 'size': {'mode': 'width', 'value': width}})
        outcome = {k: result.get(k) for k in ('ok', 'width', 'height', 'bytes',
                                              'bits_per_component', 'compression')}
    except BaseException as error:
        outcome = dict(error=repr(error)[:400])
    elapsed = time.monotonic() - start
    done.set()
    thread.join()
    if where.exists():
        where.unlink()
    return dict(seconds=round(elapsed, 3), rss_before_kib=before, max_rss_kib=peak[0],
                result=outcome)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    output = arguments.output.expanduser().resolve()
    if output.is_relative_to(ROOT) or arguments.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    receipt = dict(app=str(executable), app_sha256=d11.digest(executable),
                   script_sha256=d11.digest(Path(__file__)),
                   tolerances=dict(G5_jpeg_mean_codes=T5_JPEG_MEAN_CODES), error=None)
    drive = None
    try:
        fixtures = output / 'fixtures'
        fixtures.mkdir(exist_ok=True)
        photo = fixtures / 'photo.png'
        d11.write_png(photo, WIDTH, HEIGHT, d11.photo_rgba8(WIDTH, HEIGHT), 6, 8,
                      extra=d11.srgb_chunk() + d11.phys_chunk(300))
        rgba16 = fixtures / 'rgba16.png'
        d11.write_png(rgba16, WIDTH, HEIGHT, rgba16_fixture(), 6, 16,
                      extra=d11.srgb_chunk() + d11.phys_chunk(300))
        pdf = fixtures / 'page.pdf'
        pdf.write_bytes(d11.pdf_fixture())
        receipt['fixture_sha256'] = {p.name: d11.digest(p) for p in (photo, rgba16, pdf)}
        receipt['fixture_sha256']['scene.tiff'] = d11.digest(FLOAT_FIXTURE)
        with d11.support.HostApp(output / 'app', output / 'recovery', executable) as app:
            drive = Drive(app, output)
            docs = dict(photo=drive.open(photo), rgba16=drive.open(rgba16, precision='preserve'),
                        pdf=drive.open(pdf, page=1, ppi=72), float=drive.open(FLOAT_FIXTURE))
            for step in (g2_byte_default, g3_sixteen, g4_float, g5_jpeg_opaque, g6_jpeg_alpha,
                         g7_refusals, recorded_large, g1_psd):
                try:
                    step(drive, docs)
                except BaseException as error:
                    drive.report['gates'][step.__name__] = dict(
                        passed=False, detail=dict(error=repr(error)[:600]))
            g8 = drive.report['g8']
            drive.report['gates']['G8 reported equals measured'] = dict(
                passed=bool(g8) and all(entry['passed'] for entry in g8), detail=dict(cases=g8))
    except BaseException as error:
        receipt['error'] = f'{type(error).__name__}: {error}'
    if drive is not None:
        receipt.update(gates=drive.report['gates'], recorded=drive.report['recorded'])
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in receipt.get('gates', {}).items()}
    print(json.dumps(dict(error=receipt['error'], gates=gates,
                          recorded=receipt.get('recorded'),
                          receipt=str(output / 'receipt.json')), indent=2))
    raise SystemExit(0 if not receipt['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
