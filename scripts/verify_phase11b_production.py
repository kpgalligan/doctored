#!/usr/bin/env python3
"""The Phase 11B production-outputs MCP drive, on a built Release app.

Eight gates, fixed before the run; a failed gate IS the receipt.

- **P1 layer exports.** A three-item 16-bit document exports TRIMMED: the two
  items with pixels are written and their dimensions, read back by `sips`,
  equal the `size` the result reported; the fully transparent item is refused;
  a second run refuses every item that already exists.
- **P2 contact sheets.** `contact_sheet_from_sources` over those files writes a
  PNG whose dimensions equal the formula restated here, whose gutter pixel is
  the background colour and whose cell centre is not (decoded by this file).
  `contact_sheet(items=all)` on the 16-bit document reports one cell per item
  and puts the background colour at the transparent item's cell centre, and an
  `export` output at `bit_depth: 16` is refused before any cell is built.
- **P3 metadata policies.** Three saved policies applied through `export_image`
  (JPEG) and `quick_export` (PNG16): the packets THIS FILE parses out of the
  bytes equal the `metadata` report the tool returned, every written EXIF and
  XMP packet states Orientation 1, `density: drop` beside dropped EXIF and XMP
  leaves no APP0 density and no `pHYs`, and `density: drop` with `exif: keep`
  is refused.
- **P4 policies persist.** The library survives an app relaunch, in this
  drive's own private `DR_METADATA_POLICIES_DIR`.
- **P5 batch delivery.** `open_document` → `run_action` → `quick_export` over a
  three-file folder with a preset, numbered by `{index}`/`count`.
- **P6 Actions.** A recorded `export_layers` replays on a RENAMED copy.
- **P7 the master is untouched.** `get_document`, `get_precision`,
  `get_color_profile` and the dirty flag are identical before and after every
  call.
- **P8 catalog.** `tools/list` parses and its count is recorded.

Nothing here derives an expectation from Doctored. The fixtures are written by
this file, the dimensions come from `sips`, the digests from `hashlib`, and the
written packets from the bounded stdlib parser below — plus `exiftool` when it
happens to be installed, which on this machine it is not.
"""
import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]

_spec = importlib.util.spec_from_file_location('phase11b_support',
                                               ROOT / 'scripts/host-source-probe/support.py')
support = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(support)

WIDTH = 240
HEIGHT = 180
FULL = 65535
# The subject rectangle, in canvas coordinates. The trim gate compares the
# delivered file's dimensions against exactly this.
RECT = (40, 30, 100, 60)
PPI = 300
AUTHOR = 'Phase 11B Drive'
COPYRIGHT = '(c) 2026 Doctored'


# ------------------------------------------------------------- PNG plumbing --

def chunk(kind, body):
    return (struct.pack('>I', len(body)) + kind + body
            + struct.pack('>I', zlib.crc32(kind + body)))


def png16(path, width, height, samples, extra=b''):
    """An RGBA16 PNG carrying the sRGB chunk and any extra chunks, written by
    this file without any application."""
    assert len(samples) == width * height * 4
    rows = b''.join(b'\0' + struct.pack(f'>{width * 4}H',
                                        *samples[y * width * 4:(y + 1) * width * 4])
                    for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 16, 6, 0, 0, 0))
                     + chunk(b'sRGB', b'\0') + extra
                     + chunk(b'IDAT', zlib.compress(rows, 6))
                     + chunk(b'IEND', b''))


def plate(width=WIDTH, height=HEIGHT):
    """Coprime ramps with hard edges, so nothing sits at 0 or 65535."""
    span = 61000 - 4096
    out = []
    for y in range(height):
        for x in range(width):
            edge = 61000 if (y // 7) % 3 == 0 else 4096
            out += [4096 + span * x // (width - 1), edge,
                    4096 + span * ((x + y) % height) // (height - 1), FULL]
    return out


def png_chunks(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    out = []
    at = 8
    while at + 12 <= len(data):
        length = struct.unpack('>I', data[at:at + 4])[0]
        out.append((data[at + 4:at + 8], data[at + 8:at + 8 + length]))
        at += 12 + length
    return out


def png_decode_rgba8(path):
    """A bounded 8-bit RGB/RGBA PNG decoder: enough to read one pixel out of a
    contact sheet without asking Doctored what it drew."""
    data = Path(path).read_bytes()
    header = next(body for kind, body in png_chunks(data) if kind == b'IHDR')
    width, height, depth, colour, compression, filt, interlace = struct.unpack('>IIBBBBB', header)
    assert depth == 8 and colour in (2, 6) and compression == 0 and filt == 0 and interlace == 0, \
        f'unsupported PNG: depth {depth} colour {colour} interlace {interlace}'
    channels = 3 if colour == 2 else 4
    raw = zlib.decompress(b''.join(body for kind, body in png_chunks(data) if kind == b'IDAT'))
    stride = width * channels
    out = bytearray(height * stride)
    previous = bytearray(stride)
    at = 0
    for y in range(height):
        method = raw[at]
        line = bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            upleft = previous[i - channels] if i >= channels else 0
            if method == 0:
                value = line[i]
            elif method == 1:
                value = line[i] + left
            elif method == 2:
                value = line[i] + up
            elif method == 3:
                value = line[i] + (left + up) // 2
            elif method == 4:
                p = left + up - upleft
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - upleft)
                value = line[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else upleft)
            else:
                raise AssertionError(f'unknown PNG filter {method}')
            line[i] = value & 0xff
        out[y * stride:(y + 1) * stride] = line
        previous = line
    return width, height, channels, bytes(out)


def pixel_at(decoded, x, y):
    width, height, channels, data = decoded
    assert 0 <= x < width and 0 <= y < height, (x, y, width, height)
    at = (y * width + x) * channels
    values = list(data[at:at + channels])
    return values + [255] if channels == 3 else values


# ------------------------------------------- the metadata fixture (written here) --

def build_exif(orientation, ppi):
    """A little-endian EXIF TIFF stream: IFD0 with Orientation, the resolution
    trio and nothing else, assembled by this file."""
    entries = 4
    ifd0 = 8
    link = ifd0 + 2 + 12 * entries
    rationals = link + 4
    body = bytearray(rationals + 16)
    body[0:2] = b'II'
    body[2:4] = struct.pack('<H', 42)
    body[4:8] = struct.pack('<I', ifd0)
    body[ifd0:ifd0 + 2] = struct.pack('<H', entries)

    def entry(at, tag, kind, value):
        body[at:at + 2] = struct.pack('<H', tag)
        body[at + 2:at + 4] = struct.pack('<H', kind)
        body[at + 4:at + 8] = struct.pack('<I', 1)
        body[at + 8:at + 12] = value

    at = ifd0 + 2
    entry(at, 0x0112, 3, struct.pack('<HH', orientation, 0))          # Orientation
    entry(at + 12, 0x011A, 5, struct.pack('<I', rationals))           # XResolution
    entry(at + 24, 0x011B, 5, struct.pack('<I', rationals + 8))       # YResolution
    entry(at + 36, 0x0128, 3, struct.pack('<HH', 2, 0))               # ResolutionUnit = inch
    body[link:link + 4] = struct.pack('<I', 0)
    body[rationals:rationals + 16] = struct.pack('<IIII', ppi, 1, ppi, 1)
    return bytes(body)


XMP_PACKET = (
    b"<?xpacket begin='\xef\xbb\xbf' id='W5M0MpCehiHzreSzNTczkc9d'?>"
    b"<x:xmpmeta xmlns:x='adobe:ns:meta/'>"
    b"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>"
    b"<rdf:Description rdf:about='' xmlns:tiff='http://ns.adobe.com/tiff/1.0/' "
    b"tiff:Orientation='6' tiff:XResolution='300/1' tiff:YResolution='300/1' "
    b"tiff:ResolutionUnit='2'></rdf:Description>"
    b"</rdf:RDF></x:xmpmeta><?xpacket end='w'?>")


def itxt_xmp(packet):
    return chunk(b'iTXt', b'XML:com.adobe.xmp\0\0\0\0\0' + packet)


# -------------------------------------------------- the independent decoder --

def jpeg_segments(data):
    assert data[:2] == b'\xff\xd8', 'not a JPEG'
    out = []
    at = 2
    while at + 4 <= len(data):
        assert data[at] == 0xff, 'malformed JPEG segment'
        marker = data[at + 1]
        if marker == 0xda:
            break
        length = struct.unpack('>H', data[at + 2:at + 4])[0]
        out.append((marker, data[at + 4:at + 2 + length]))
        at += 2 + length
    return out


def app_payload(data, marker, prefix):
    for found, payload in jpeg_segments(data):
        if found == marker and payload.startswith(prefix):
            return payload[len(prefix):]
    return None


def exif_fields(blob):
    """IFD0's Orientation and XResolution, read straight out of the TIFF
    stream — both byte orders, no library."""
    if blob is None or len(blob) < 8 or blob[:2] not in (b'II', b'MM'):
        return None
    order = '<' if blob[:2] == b'II' else '>'
    offset = struct.unpack(order + 'I', blob[4:8])[0]
    if offset + 2 > len(blob):
        return None
    count = struct.unpack(order + 'H', blob[offset:offset + 2])[0]
    fields = {}
    for index in range(count):
        at = offset + 2 + 12 * index
        if at + 12 > len(blob):
            break
        tag, kind = struct.unpack(order + 'HH', blob[at:at + 4])
        if tag == 0x0112 and kind == 3:
            fields['orientation'] = struct.unpack(order + 'H', blob[at + 8:at + 10])[0]
        if tag in (0x011A, 0x011B) and kind == 5:
            where = struct.unpack(order + 'I', blob[at + 8:at + 12])[0]
            if where + 8 <= len(blob):
                num, den = struct.unpack(order + 'II', blob[where:where + 8])
                fields['xres' if tag == 0x011A else 'yres'] = (num, den)
    return fields


def xmp_orientations(packet):
    if packet is None:
        return []
    text = packet.decode('utf-8', 'replace')
    out = []
    for opener, closer in (("tiff:Orientation='", "'"), ('tiff:Orientation="', '"'),
                           ('<tiff:Orientation>', '<')):
        rest = text
        while opener in rest:
            tail = rest[rest.index(opener) + len(opener):]
            end = tail.find(closer)
            out.append(tail[:end] if end >= 0 else tail)
            rest = tail[max(end, 0):]
    return out


def png_xmp(data):
    for kind, body in png_chunks(data):
        if kind != b'iTXt' or not body.startswith(b'XML:com.adobe.xmp\0'):
            continue
        at = body.index(b'\0') + 3
        at = body.index(b'\0', at) + 1
        at = body.index(b'\0', at) + 1
        return body[at:]
    return None


def read_packets(path):
    """What the file ACTUALLY carries, by this file's own reading."""
    data = Path(path).read_bytes()
    if data[:2] == b'\xff\xd8':
        exif = app_payload(data, 0xe1, b'Exif\0\0')
        xmp = app_payload(data, 0xe1, b'http://ns.adobe.com/xap/1.0/\0')
        iptc = app_payload(data, 0xed, b'Photoshop 3.0\0')
        jfif = app_payload(data, 0xe0, b'JFIF\0')
        density = None
        if jfif is not None and len(jfif) >= 7:
            units, x_density, y_density = struct.unpack('>BHH', jfif[2:7])
            density = None if units == 0 else (units, x_density, y_density)
        return dict(container='jpeg', exif=exif is not None, xmp=xmp is not None,
                    iptc=iptc is not None, density=density is not None,
                    density_value=density, exif_fields=exif_fields(exif),
                    xmp_orientations=xmp_orientations(xmp),
                    xmp_text=(xmp or b'').decode('utf-8', 'replace'))
    if data[:8] == b'\x89PNG\r\n\x1a\n':
        kinds = [kind for kind, _ in png_chunks(data)]
        exif = next((body for kind, body in png_chunks(data) if kind == b'eXIf'), None)
        xmp = png_xmp(data)
        phys = next((body for kind, body in png_chunks(data) if kind == b'pHYs'), None)
        return dict(container='png', exif=exif is not None, xmp=xmp is not None,
                    iptc=False, density=phys is not None,
                    density_value=(struct.unpack('>IIB', phys) if phys else None),
                    exif_fields=exif_fields(exif), xmp_orientations=xmp_orientations(xmp),
                    xmp_text=(xmp or b'').decode('utf-8', 'replace'), chunks=kinds)
    raise AssertionError(f'unsupported container for {path}')


def exiftool(path):
    """Used ADDITIONALLY when it happens to be installed; never relied on."""
    tool = shutil.which('exiftool')
    if not tool:
        return None
    try:
        return json.loads(subprocess.run([tool, '-j', '-G', str(path)], check=True,
                                         capture_output=True, text=True).stdout)[0]
    except Exception as error:                  # Recorded, never repaired.
        return {'error': repr(error)}


# --------------------------------------------------------------- utilities --

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sips(path, *keys):
    arguments = []
    for key in keys:
        arguments += ['-g', key]
    text = subprocess.run(['/usr/bin/sips', *arguments, str(path)],
                          check=True, capture_output=True, text=True).stdout
    values = {}
    for line in text.splitlines()[1:]:
        if ':' in line:
            name, value = line.split(':', 1)
            values[name.strip()] = value.strip()
    return values


def dimensions(path):
    values = sips(path, 'pixelWidth', 'pixelHeight')
    return int(values.get('pixelWidth', 0)), int(values.get('pixelHeight', 0))


STATE_KEYS = ('width', 'height', 'layers', 'resolution', 'channels', 'guides',
              'selection', 'selected_layers', 'active_layer', 'precision')


def master_state(app, document):
    summary = next(entry for entry in app.call('list_documents')['documents']
                   if entry['id'] == document)
    state = app.call('get_document', document_id=document)
    return dict(
        document={key: state.get(key) for key in STATE_KEYS},
        precision=app.call('get_precision', document_id=document),
        profile=app.call('get_color_profile', document_id=document),
        unsaved_changes=summary['unsaved_changes'])


def sheet_formula(columns, cell, gutter, caption, count):
    rows = math.ceil(count / columns)
    return (gutter + columns * (cell[0] + gutter),
            gutter + rows * (cell[1] + caption + gutter), rows)


# ------------------------------------------------------------------- gates --

def build_document(app, output, report):
    """A three-item 16-bit document: an opaque background, an opaque rectangle
    placed on its own top-level layer, and a wholly transparent one.

    Built with `place_image` rather than `new_layer` + `fill`, because
    `new_layer` is deliberately unavailable in 16-bit mode — and a placed image
    keeps every sample exactly as this file wrote it, which is what makes the
    trim rectangle an assertion rather than an observation."""
    source = output / 'master16.png'
    png16(source, WIDTH, HEIGHT, plate())
    subject = output / 'subject16.png'
    png16(subject, RECT[2], RECT[3], [8192, 20480, 51200, FULL] * (RECT[2] * RECT[3]))
    empty = output / 'empty16.png'
    png16(empty, WIDTH, HEIGHT, [0, 0, 0, 0] * (WIDTH * HEIGHT))
    for path in (source, subject, empty):
        report['fixture_sha256'][path.name] = digest(path)

    document = app.call('open_document', path=str(source),
                        precision='preserve')['document']['id']
    app.call('place_image', document_id=document, path=str(subject), name='Subject',
             x=RECT[0], y=RECT[1])
    app.call('place_image', document_id=document, path=str(empty), name='Empty', x=0, y=0)
    return document, source


def take_layer_exports(app, output, report, document):
    items = output / 'items'
    items.mkdir()
    settings = {'format': 'png', 'bit_depth': 16, 'profile': 'document'}
    first = app.call('export_layers', document_id=document, items='all', trimmed=True,
                     settings=settings, destination=str(items),
                     pattern='{name}-{index}-{layer}')
    second = app.call('export_layers', document_id=document, items='all', trimmed=True,
                     settings=settings, destination=str(items),
                     pattern='{name}-{index}-{layer}')
    rows = []
    for entry in first['items']:
        row = dict(entry)
        if entry.get('path'):
            row['sips'] = dimensions(entry['path'])
            row['size_matches'] = list(row['sips']) == list(entry.get('size', []))
        rows.append(row)
    written = [row for row in rows if row.get('path')]
    refused = [row for row in rows if row.get('refusal')]
    transparent_refused = any('nothing to trim' in (row.get('refusal') or '')
                              for row in refused)
    second_all_exist = (second['written'] == 0
                        and all('already exists' in (entry.get('refusal') or '')
                                or 'nothing to trim' in (entry.get('refusal') or '')
                                for entry in second['items'])
                        and any('already exists' in (entry.get('refusal') or '')
                                for entry in second['items']))
    report['layer_exports'] = dict(first=first, second=second, rows=rows)
    report['gates']['P1 trimmed 16-bit layer exports'] = dict(
        passed=(first['ok'] is False and len(rows) == 3 and len(written) == 2
                and all(row['size_matches'] for row in written)
                and transparent_refused and second_all_exist),
        detail=dict(written=[row['path'] for row in written],
                    sizes=[row['sips'] for row in written],
                    refusals=[row['refusal'] for row in refused],
                    second_written=second['written']))
    return [Path(row['path']) for row in written]


def take_contact_sheets(app, output, report, document, files):
    layout = dict(columns=2, cell_width=120, cell_height=90, gutter=10,
                  caption_height=0, background='#204080', filter='nearest')
    sheets = output / 'sheets'
    sheets.mkdir()
    settings = {'format': 'png', 'bit_depth': 8, 'profile': 'srgb'}

    from_files = app.call('contact_sheet_from_sources', files=[str(path) for path in files],
                          layout=layout,
                          output=dict(kind='export', settings=settings,
                                      destination=str(sheets), pattern='{name}-from-files'))
    sheet_path = Path(from_files['path'])
    expected = sheet_formula(layout['columns'],
                             (layout['cell_width'], layout['cell_height']),
                             layout['gutter'], layout['caption_height'], len(files))
    decoded = png_decode_rgba8(sheet_path)
    background = [0x20, 0x40, 0x80, 255]
    gutter_pixel = pixel_at(decoded, 2, 2)
    centre = pixel_at(decoded, layout['gutter'] + layout['cell_width'] // 2,
                      layout['gutter'] + layout['cell_height'] // 2)
    report['contact_sheet_files'] = dict(
        result=from_files, expected=expected, decoded=(decoded[0], decoded[1]),
        gutter_pixel=gutter_pixel, cell_centre=centre)

    whole = app.call('contact_sheet', document_id=document, items='all', layout=layout,
                     output=dict(kind='export', settings=settings,
                                 destination=str(sheets), pattern='{name}-all-items'))
    whole_path = Path(whole['path'])
    whole_decoded = png_decode_rgba8(whole_path)
    # Panel order puts the transparent "Empty" item first, so cell 0's centre
    # must be the background colour exactly.
    empty_centre = pixel_at(whole_decoded, layout['gutter'] + layout['cell_width'] // 2,
                            layout['gutter'] + layout['cell_height'] // 2)
    whole_expected = sheet_formula(layout['columns'],
                                   (layout['cell_width'], layout['cell_height']),
                                   layout['gutter'], layout['caption_height'], 3)

    as_document = app.call('contact_sheet', document_id=document, items='all', layout=layout,
                           output=dict(kind='document'))
    refusal = app.call('contact_sheet', document_id=document, items='all', layout=layout,
                       output=dict(kind='export',
                                   settings={'format': 'png', 'bit_depth': 16},
                                   destination=str(sheets), pattern='{name}-16'),
                       fail=True)
    report['contact_sheet_document'] = dict(
        result=whole, expected=whole_expected, decoded=(whole_decoded[0], whole_decoded[1]),
        empty_cell_centre=empty_centre, as_document=as_document, refusal=refusal)
    report['gates']['P2 contact sheets compose, place and refuse'] = dict(
        passed=((decoded[0], decoded[1]) == (expected[0], expected[1])
                and from_files['width'] == expected[0]
                and from_files['height'] == expected[1]
                and gutter_pixel == background and centre != background
                and whole['cells'] == 3
                and (whole_decoded[0], whole_decoded[1]) == (whole_expected[0],
                                                             whole_expected[1])
                and empty_centre == background
                and as_document.get('ok') is True
                and as_document.get('document') is not None
                and '8 bits' in refusal),
        detail=dict(from_files=(decoded[0], decoded[1]), formula=expected,
                    gutter=gutter_pixel, centre=centre, empty_centre=empty_centre,
                    refusal=refusal))
    return as_document


def take_policies(app, output, report):
    """A fixture carrying all three packets, delivered under three policies."""
    source = output / 'packets16.png'
    png16(source, WIDTH, HEIGHT, plate(),
          extra=chunk(b'eXIf', build_exif(6, PPI))
          + chunk(b'pHYs', struct.pack('>IIB', 11811, 11811, 1))
          + itxt_xmp(XMP_PACKET))
    report['fixture_sha256'][source.name] = digest(source)

    policies = {
        'Everything': {},
        'No EXIF, stamped': {'exif': 'drop',
                             'stamp': {'author': AUTHOR, 'copyright': COPYRIGHT}},
        'No resolution': {'exif': 'drop', 'xmp': 'drop', 'density': 'drop'},
    }
    for name, policy in policies.items():
        saved = app.call('save_metadata_policy', name=name, policy=policy)
        assert saved['ok'], saved

    flat = app.call('open_document', path=str(source))['document']['id']
    typed = app.call('open_document', path=str(source), precision='preserve')['document']['id']
    deliveries = output / 'policy-deliveries'
    deliveries.mkdir()

    cases = []
    for name in policies:
        jpeg = deliveries / f'{name.replace(" ", "-").replace(",", "")}.jpg'
        written = app.call('export_image', document_id=flat, path=str(jpeg),
                           settings={'format': 'jpeg', 'jpeg_quality': 88},
                           metadata_policy=name)
        parsed = read_packets(jpeg)
        cases.append(dict(policy=name, container='jpeg', path=str(jpeg),
                          reported=written['metadata'], parsed=parsed,
                          exiftool=exiftool(jpeg),
                          agrees=(parsed['exif'] == written['metadata']['exif']
                                  and parsed['xmp'] == written['metadata']['xmp']
                                  and parsed['iptc'] == written['metadata']['iptc']
                                  and parsed['density'] == written['metadata']['density'])))

    quick = output / 'policy-quick'
    quick.mkdir()
    assert app.call('save_export_preset', name='Print PNG16',
                    settings={'format': 'png', 'bit_depth': 16, 'profile': 'document'},
                    destination=str(quick), pattern='{name}-{preset}')['ok']
    for name in policies:
        delivered = app.call('quick_export', document_id=typed, preset='Print PNG16',
                             metadata_policy=name)
        target = quick / f'packets16-Print PNG16.png'
        parsed = read_packets(delivered['path'])
        cases.append(dict(policy=name, container='png16', path=delivered['path'],
                          reported=delivered['metadata'], parsed=parsed,
                          exiftool=exiftool(delivered['path']),
                          agrees=(parsed['exif'] == delivered['metadata']['exif']
                                  and parsed['xmp'] == delivered['metadata']['xmp']
                                  and parsed['density'] == delivered['metadata']['density'])))
        Path(delivered['path']).unlink()     # the pattern is constant; make room
        _ = target

    orientations_ok = True
    for case in cases:
        fields = case['parsed'].get('exif_fields')
        if case['parsed']['exif'] and (not fields or fields.get('orientation') != 1):
            orientations_ok = False
        for value in case['parsed']['xmp_orientations']:
            if value.strip() != '1':
                orientations_ok = False

    stamped = [case for case in cases if case['policy'] == 'No EXIF, stamped']
    stamp_ok = all(case['reported']['stamped'] is True
                   and AUTHOR in case['parsed']['xmp_text']
                   and 'dc:rights' in case['parsed']['xmp_text'] for case in stamped)
    dropped = [case for case in cases if case['policy'] == 'No resolution']
    density_ok = all(case['reported']['density'] is False
                     and case['parsed']['density'] is False for case in dropped)

    refusal = app.call('export_image', document_id=flat,
                       path=str(deliveries / 'refused.jpg'),
                       settings={'format': 'jpeg',
                                 'metadata': {'exif': 'keep', 'xmp': 'drop',
                                              'density': 'drop'}},
                       fail=True)
    report['policy_cases'] = cases
    report['policy_refusal'] = refusal
    report['gates']['P3 metadata policies match the written bytes'] = dict(
        passed=(all(case['agrees'] for case in cases) and orientations_ok
                and stamp_ok and density_ok
                and 'each carries its own copy of the ppi' in refusal),
        detail=dict(agree=[case['agrees'] for case in cases],
                    orientations_ok=orientations_ok, stamp_ok=stamp_ok,
                    density_ok=density_ok, refusal=refusal))
    return list(policies)


def take_batch(app, output, report, source):
    """The delivery loop Batch's preset mode runs: open, play, quick_export
    with this file's own index and count."""
    folder = output / 'batch-in'
    folder.mkdir()
    for index in range(3):
        shutil.copyfile(source, folder / f'shot-{index + 1}.png')
    destination = output / 'batch-out'
    destination.mkdir()
    assert app.call('save_export_preset', name='Delivery',
                    settings={'format': 'png', 'profile': 'srgb',
                              'size': {'mode': 'long_edge', 'value': 120}},
                    destination=str(destination), pattern='{name}-{index}')['ok']
    # Recorded on a scratch copy of its own, so neither the master nor any of
    # the three inputs carries the recording's own edit.
    scratch = output / 'record-source.png'
    shutil.copyfile(source, scratch)
    recorder = app.call('open_document', path=str(scratch))['document']['id']
    app.call('start_recording')
    app.call('rotate', document_id=recorder, degrees=90)
    action = app.call('stop_recording', name='Phase 11B Delivery')
    inputs = sorted(folder.iterdir())
    delivered = []
    for position, path in enumerate(inputs):
        document = app.call('open_document', path=str(path))['document']['id']
        played = app.call('run_action', document_id=document, name='Phase 11B Delivery')
        assert played.get('ok') is True, played
        result = app.call('quick_export', document_id=document, preset='Delivery',
                          index=position + 1, count=len(inputs))
        delivered.append(result['path'])
    names = sorted(entry.name for entry in destination.iterdir())
    report['batch'] = dict(action=[step['tool'] for step in action['steps']],
                           delivered=delivered, names=names)
    report['gates']['P5 preset delivery over a folder numbers its outputs'] = dict(
        passed=(names == ['shot-1-1.png', 'shot-2-2.png', 'shot-3-3.png']),
        detail=dict(names=names))


def take_action_replay(app, output, report, document, source):
    replay = output / 'replay'
    replay.mkdir()
    app.call('start_recording')
    recorded = app.call('export_layers', document_id=document, items='all', trimmed=False,
                        settings={'format': 'png', 'bit_depth': 16},
                        destination=str(replay), pattern='{name}-{index}-{layer}')
    assert recorded['ok'], recorded
    action = app.call('stop_recording', name='Phase 11B Layer Export')
    tools = [step['tool'] for step in action['steps']]
    copy = output / 'renamed-copy.png'
    shutil.copyfile(source, copy)
    other = app.call('open_document', path=str(copy), precision='preserve')['document']['id']
    replayed = app.call('run_action', document_id=other, name='Phase 11B Layer Export')
    names = sorted(path.name for path in replay.iterdir())
    report['action_replay'] = dict(steps=tools, replayed=replayed, written=names)
    report['gates']['P6 a recorded layer export replays on another document'] = dict(
        passed=(tools == ['export_layers'] and replayed.get('ok') is True
                and any(name.startswith('renamed-copy-') for name in names)),
        detail=dict(steps=tools, written=names))


def take(app, output, report):
    document, source = build_document(app, output, report)
    before = master_state(app, document)

    files = take_layer_exports(app, output, report, document)
    take_contact_sheets(app, output, report, document, files)
    policies = take_policies(app, output, report)
    take_batch(app, output, report, source)
    take_action_replay(app, output, report, document, source)

    after = master_state(app, document)
    report['master_state'] = dict(before=before, after=after)
    report['gates']['P7 the editable master is untouched'] = dict(
        passed=after == before,
        detail=dict(unsaved_changes=after['unsaved_changes']))
    report['gates']['P8 the catalog parses'] = dict(
        passed=report['tool_count'] > 0, detail=dict(tool_count=report['tool_count']))
    return policies


def take_after_relaunch(app, report, policies):
    listed = app.call('list_metadata_policies')
    names = [entry['name'] for entry in listed['policies']]
    report['policies_after_relaunch'] = listed
    report['gates']['P4 the metadata policy library survives a relaunch'] = dict(
        passed=(sorted(names) == sorted(policies)),
        detail=dict(names=names))


def dependencies():
    """The code files `helper_sha256` hashes."""
    return [ROOT / 'scripts/host-source-probe/support.py',
            ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/precision-probe/fixtures.py',
            ROOT / 'scripts/precision-probe/retained_fixtures.py',
            ROOT / 'scripts/precision-probe/retained_linked_fixtures.py',
            ROOT / 'scripts/verification_isolation.py',
            ROOT / 'scripts/app_under_test.py', ROOT / 'scripts/verify_phase1.py',
            ROOT / 'scripts/verify_phase2.py', ROOT / 'scripts/verify_phase8b.py']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path, required=True)
    arguments = parser.parse_args()
    output = arguments.output.expanduser().resolve()
    if output.is_relative_to(ROOT) or arguments.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    assert not output.exists() or not any(output.iterdir()), 'use a fresh output folder'
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)),
                  helper_sha256={str(p): digest(p) for p in dependencies()},
                  exiftool=shutil.which('exiftool'),
                  fixture_sha256={}, tool_count=0, gates={}, error=None)
    try:
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            report['tool_count'] = len(app.client.rpc('tools/list', {})['tools'])
            policies = take(app, output, report)
        # The same private DR_METADATA_POLICIES_DIR, a brand new process.
        with support.HostApp(output / 'app', output / 'recovery', executable) as app:
            take_after_relaunch(app, report, policies)
    except BaseException as error:              # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'result.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          result=str(output / 'result.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
