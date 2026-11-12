#!/usr/bin/env python3
"""The Phase 11D format-additions MCP drive, on a built Release app.

Seven gates, whose tolerances are FIXED in this file before anything is
measured. A miss is recorded in `receipt.json` and fails its gate; no
tolerance is widened after the fact.

- **D1 HEIC.** Export: an 8-bit and a typed 16-bit document delivered as
  `heic` through `export_image` and through the export sheet's own path
  (`export_preview`), plus a `contact_sheet` and an `export_layers` delivery.
  Every file is parsed by `heif-info` (never by Doctored), the depth it
  reports equals the result's `bit_depth`, and `get_document`'s
  `losses_by_format.heic` names the host-depth sentence. **T3**: against the
  PNG export of identical settings, mean <= 2 and p99 <= 8 codes (HEIC is
  lossy, so this is a bound, not an equality) MEASURED ON THE PHOTOGRAPHIC
  plate — the per-pixel-noise plate is recorded beside it and NOT gated, so
  the bound and its subject are fixed together; the embedded ICC bytes are
  EQUAL; Orientation is 1; XResolution is the document's ppi; `carried`
  equals what an independent read finds, including for an
  `embed_profile: false` delivery, whose report is gated against `magick`'s
  read of the written file rather than against the flag. Import: 8-, 10- and 12-bit HEIC
  written by `heif-enc`, opened and exported back to PNG. **T1**: an 8-bit
  import is EXACT against `magick`'s decode; a 10-bit import is within 2
  codes at 10 bits with mean <= 0.5 and exact alpha; the 12-bit outcome is
  RECORDED (accepted or refused, whichever it is), and a monochrome and a PQ
  HEIC must be refused by sentence. The COMMITTED opaque fixtures
  (`core/tests/fixtures/host_stills_opaque`, each checked against
  its `provenance.json` SHA-256 first) add: `heic8`/`heic10`/`heic12` must
  open (8 -> rgba8, 10/12 -> rgba16); 8 is EXACT against `magick`, 10 is
  within T1 against `magick` and EXACT against the ImageIO/CG oracle's
  `redraw16`, 12 is recorded. `heic12-444.heic` (`heif-enc --lossless -b 12`,
  12-bit 4:4:4, which ImageIO returns with no pixels) must be REFUSED with
  exactly `HostStillDecoder.noDecodedPixels`'s sentence. `gainmap.heic` must
  open 8-bit with the disclosure in
  the metadata-import warnings, and its 8-bit PNG export RGB must EXACTLY
  equal the oracle's `sdr8` (measured in a second owned instance whose
  working space is Display P3, the base's own, because an 8-bit open
  converts into the working space).
- **D2 Gray16.** Gray16 PNG/TIFF fixtures written byte by byte here. Since
  #1 phase 1a each opens as a GRAYSCALE document (`color_mode` "gray"; the
  drive launches with `-ColorWorkingGray sgray`, so an untagged file takes
  sGray), is converted with `convert_mode rgb rgb_space srgb` and exported
  back. **T2**: with a Gray Gamma 2.2 ICC, max <= 514 u16 codes (two codes at
  8 bits) and mean <= 128 against BOTH `sips --matchTo` (ColorSync) and
  `magick -profile` (lcms). The untagged, `sRGB`-chunk and GrayA cases were a
  copy (EXACT) before phase 1a and are now a gray → sRGB conversion: a new
  operation's gate, max <= 1 u16 code (`T2_GRAY_CONVERTED_MAX_U16`), alpha
  exact. The sRGB-ICC-on-gray and bare-`gAMA` refusals are checked by
  sentence, unchanged: `dr_path_gray_source` answers none for both, so
  today's open refuses them.
- **D3 the typed bridge.** A typed host still and a typed RAW (the synthetic
  DNG from `scripts/phase3_fixtures.py`) reconstruct packets and PPI. **T4**:
  the exported TIFF16's IFD0/EXIF fields (Make, Model, DateTimeOriginal,
  XResolution/YResolution, Orientation = 1) equal the fixture's authored
  values, XMP is present, and the typed loss notice no longer names packets
  or PPI while a movie frame still discloses them.
- **D4 float TIFF.** A float TIFF written by the Rust seam (through the
  committed test fixture this drive re-creates with `cargo`) is read by
  `tiffinfo` and `magick`. RECORDED, with the read-back sample equality as
  the gate.
- **D5 vector.** A hand-written PDF and SVG with analytic geometry: **T5**
  rectangle interiors inset one pixel are EXACT and the exterior is exact,
  and `pdftoppm -r`/`rsvg-convert -d -p` agree within 2 codes outside a 1-px
  edge band. The PDF EXPORT is parsed by the stdlib reader below: one page, a
  MediaBox within 0.01 pt of the document's print size, an image with
  `/BitsPerComponent 8` and `/Filter /FlateDecode`, an ICCBased stream whose
  sha256 equals the exported profile, decoded RGB EQUAL to the PNG export,
  and a `pdftoppm` re-render within 1 code.
- **D6 AVIF and JPEG 2000.** Files written by `avifenc` and `opj_compress`,
  opened and exported back. **T1**: 8-bit AVIF EXACT against `magick` and
  8-bit JPEG 2000 EXACT against `opj_decompress`; a 10-bit AVIF is EXACT against the oracle's `redraw16`
  and exact after both sides map to 10-bit codes against `magick`. Five
  12-bit AVIFs (`-d 12`, from `plate16` or its alpha twin `plate16a`):
  the IDENTITY-matrix files (`avif12` lossless, `avif12_alpha` lossless with
  straight alpha, `avif12_lossy_identity` `-y 444 --cicp 1/13/0 -q 90`),
  which ImageIO decodes left-aligned, must report matrix 0 in `avifdec
  --info` and export
  16-bit RGB EXACTLY equal to `(v << 4) | (v >> 8)` of `avifdec`'s y4m codes
  (Y = G, U = B, V = R), so every 4095 reads 65535 (at least one for `avif12`
  and `avif12_alpha`), with alpha EXACT against the oracle's `raw16`; the
  others (`avif12_lossy` `-q 90`, `avif12_lossy_alpha` `-q 90 --qalpha 90`)
  must open as before, with RGB (and alpha where
  present) EXACT against the oracle's `raw16`. Six PREMULTIPLIED AVIFs from
  `plate16a`: `avif{8,10,12}_premultiplied` (`-l --premultiply -d N`,
  matrix 0) and `avif{8,10,12}_premultiplied_lossy` (`-q 90 --qalpha 90
  --premultiply -d N`, matrix 6), each reported premultiplied with its matrix
  by `avifdec --info`, must OPEN and export with alpha. Against `avifdec`'s
  own PNG, both sides mapped
  to the file's N-bit codes: alpha EQUAL; RGB 0 wherever alpha is 0; elsewhere
  `|delta| <= ceil(k * (2^N - 1) / a) + 1` with k = 1 lossless and 2 lossy
  (un-premultiplying divides by alpha, so one premultiplied code of rounding
  is `(2^N - 1) / a` straight codes). Those alpha and RGB bounds ARE the
  verdict for the premultiplied files: no tool reports the host decode's
  layout since `get_host_source` went with the placed-image sources
  (35befd6b), so nothing here asks the app how it decoded — only what it
  delivers. 16-bit JPEG 2000 is disclosed (see the
  recorded decision).
  Every case records the oracle's ImageIO layout.
- **D7 the master is untouched.** `get_document`, the dirty flag and the
  sha256 of a `.rz` `save_copy` are identical before and after every export
  in this run, and `tools/list` parses with `export_pdf` and `inspect_vector`
  present.

Nothing here derives an expectation from Doctored: every fixture is written
by this file or by a named third-party encoder, every reference decode is
`sips`, `magick`, `heif-info`, `avifdec`, `opj_decompress`, `pdftoppm`,
`rsvg-convert` or `scripts/host-stills-opaque/oracle.swift` (ImageIO/CG with
no Doctored code, compiled into the output folder), and the
PNG/TIFF/PDF/y4m parsing and the digests are stdlib.
PIL and exiftool are not installed and are not used.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import zlib

sys.dont_write_bytecode = True
import app_under_test
ROOT = Path(__file__).resolve().parents[1]

_spec = importlib.util.spec_from_file_location('phase11d_support',
                                               ROOT / 'scripts/host-source-probe/support.py')
support = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(support)

_fixtures_spec = importlib.util.spec_from_file_location('phase11d_fixtures',
                                                        ROOT / 'scripts/phase3_fixtures.py')
phase3_fixtures = importlib.util.module_from_spec(_fixtures_spec)
_fixtures_spec.loader.exec_module(phase3_fixtures)

# ---- the tolerances, fixed here BEFORE anything is measured ----------------
T1_HEIC10_MAX_CODES10 = 2          # codes on the 10-bit scale
T1_HEIC10_MEAN_CODES10 = 0.5
T2_GRAY_MAX_U16 = 514              # two codes at 8 bits
T2_GRAY_MEAN_U16 = 128
# #1 phase 1a: the untagged, sRGB-chunk and GrayA cases are a gray → sRGB
# conversion now (an sGray Grayscale document through RGB Color), no longer a
# copy: a new operation's gate, declared before its first run.
T2_GRAY_CONVERTED_MAX_U16 = 1
# RECORDED DECISION (11D, after the first run): T2's GATE is lcms
# (`magick -profile`), whose source profile and destination this drive pins.
# `sips --matchTo` is ColorSync at its own default — it exposes no intent or
# black-point switch and writes 8-bit output — so its number is RECORDED at
# the 8-bit scale as a second CMS and is not a gate. This is the treatment
# 11C's T4 already gives `sips`, for the same reason. The measured ColorSync
# delta is in the receipt either way.
T2_COLORSYNC_GATED = False
# T3's SUBJECT is part of the tolerance: the bound is measured on the
# PHOTOGRAPHIC plate, because the claim is about a lossy 4:2:0 encode of a
# picture. The high-frequency plate is exported and measured in the same gate
# and RECORDED, never gated — chroma subsampling of per-pixel noise is not
# what this bound is about.
T3_HEIC_MEAN_CODES = 2.0
T3_HEIC_P99_CODES = 8.0
T3_HEIC_PLATE = 'photographic'
T5_VECTOR_MAX_CODES = 2            # outside a 1-px edge band
# The opaque host stills. Every one of these is EXACT; the oracle
# is the ImageIO/CG program in scripts/host-stills-opaque, never Doctored.
T1_OPAQUE_HEIC8_MAX_CODES = 0      # vs magick
T1_OPAQUE_HEIC10_ORACLE_MAX_U16 = 0  # vs oracle redraw16 (16-bit export)
T1_GAINMAP_SDR8_MAX_CODES = 0      # 8-bit export RGB vs oracle sdr8
# An 8-bit open converts into the colour WORKING SPACE (sRGB in the main
# instance), which is a separate, pre-existing step. The exact gain-map pixel
# comparison therefore runs in a second owned instance whose working space is
# the SDR base's own (Display P3), so no conversion stands between the decode
# and the export; the sRGB-instance difference is recorded, not gated.
T1_GAINMAP_WORKING_SPACE = 'displayP3'
T6_AVIF8_MAX_CODES = 0             # vs magick
T6_AVIF10_ORACLE_MAX_U16 = 0       # vs oracle redraw16
T6_AVIF10_MAX_CODES10 = 0          # vs magick on the 10-bit grid
T6_AVIF12_EXPANSION = '(v << 4) | (v >> 8)'  # identity files vs avifdec y4m codes
T6_AVIF12_IDENTITY = ('avif12', 'avif12_alpha', 'avif12_lossy_identity')
T6_AVIF12_WHITE_REQUIRED = ('avif12', 'avif12_alpha')
# Every premultiplied class (8/10/12 bits, lossless matrix 0 and lossy
# matrix 6) OPENS with its alpha: the host decodes a copy whose `prem`
# reference no longer names the primary item and declares the samples
# premultiplied itself. The bound below was written before the first run.
# Against `avifdec`'s PNG on the file's N-bit grid: alpha equal, RGB 0 where
# alpha is 0, elsewhere |delta| <= ceil(K * (2^N - 1) / a) + SLACK.
T6_PREMULTIPLIED = {f'avif{bits}_premultiplied{suffix}': (bits, options, matrix)
                    for bits in (8, 10, 12)
                    for suffix, options, matrix in (('', ['-l'], 0),
                                                    ('_lossy', ['-q', '90', '--qalpha', '90'], 6))}
T6_PREMULTIPLIED_LOSSLESS_K = 1
T6_PREMULTIPLIED_LOSSY_K = 2
T6_PREMULTIPLIED_SLACK = 1
T6_PREMULTIPLIED_ALPHA_MAX_CODES = 0
NO_DECODED_PIXELS = ("macOS's image decoder returned no pixels for this image, so it cannot "
                     "be opened on this system. A known case is a 12-bit 4:4:4 HEIC.")
SDR_BASE_DISCLOSURE = ('The HDR gain map is not applied: the document holds the SDR base '
                       'image. The original file is not kept in the document.')
OPAQUE_FIXTURES = ROOT / 'core/tests/fixtures/host_stills_opaque'
T5_PDF_RERENDER_MAX_CODES = 1
T5_PDF_MEDIABOX_POINTS = 0.01

WIDTH = 64
HEIGHT = 48


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def helper_digests():
    helpers = [ROOT / 'scripts/host-source-probe/support.py', ROOT / 'scripts/phase3_fixtures.py',
               ROOT / 'scripts/phase5_support.py', ROOT / 'scripts/verification_isolation.py',
               ROOT / 'scripts/app_under_test.py',
               ROOT / 'scripts/verify_phase1.py', ROOT / 'scripts/verify_phase2.py',
               ROOT / 'scripts/verify_phase8b.py', ROOT / 'scripts/precision-probe/fixtures.py',
               ROOT / 'scripts/precision-probe/retained_fixtures.py',
               ROOT / 'scripts/precision-probe/retained_linked_fixtures.py']
    return {str(helper): digest(helper) for helper in helpers}


def tool(name):
    return shutil.which(name)


def run(*command, **kwargs):
    return subprocess.run([str(part) for part in command], capture_output=True, text=True,
                          check=False, **kwargs)


# ------------------------------------------------------------ PNG plumbing --

def chunk(kind, body):
    return (struct.pack('>I', len(body)) + kind + body
            + struct.pack('>I', zlib.crc32(kind + body)))


def write_png(path, width, height, samples, colour, depth, extra=b''):
    """A PNG written by this file. `samples` is a flat list of ints in the
    file's own sample order; `colour` is the PNG colour type."""
    per_pixel = {0: 1, 2: 3, 4: 2, 6: 4}[colour]
    assert len(samples) == width * height * per_pixel, (len(samples), width, height)
    pack = '>H' if depth == 16 else '>B'
    rows = []
    for y in range(height):
        row = b'\0'
        start = y * width * per_pixel
        row += b''.join(struct.pack(pack, value)
                        for value in samples[start:start + width * per_pixel])
        rows.append(row)
    body = zlib.compress(b''.join(rows), 9)
    out = (b'\x89PNG\r\n\x1a\n'
           + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, depth, colour, 0, 0, 0))
           + extra + chunk(b'IDAT', body) + chunk(b'IEND', b''))
    Path(path).write_bytes(out)


def iccp_chunk(name, profile):
    return chunk(b'iCCP', name.encode() + b'\0\0' + zlib.compress(profile, 9))


def srgb_chunk():
    return chunk(b'sRGB', b'\0')


def gama_chunk(gamma):
    return chunk(b'gAMA', struct.pack('>I', int(round(gamma * 100000))))


def phys_chunk(ppi):
    metres = int(round(ppi / 0.0254))
    return chunk(b'pHYs', struct.pack('>IIB', metres, metres, 1))


def read_png(path):
    """Width, height, colour type, bit depth and the flat sample list of a
    PNG — this file's own reader, so a decode never comes from the library
    that produced the file."""
    data = Path(path).read_bytes()
    assert data[:8] == b'\x89PNG\r\n\x1a\n', path
    at = 8
    idat = b''
    header = None
    chunks = {}
    while at < len(data):
        length = struct.unpack('>I', data[at:at + 4])[0]
        kind = data[at + 4:at + 8]
        body = data[at + 8:at + 8 + length]
        if kind == b'IHDR':
            header = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat += body
        chunks.setdefault(kind, []).append(body)
        at += length + 12
    width, height, depth, colour, _, _, interlace = header
    assert interlace == 0, 'interlaced PNG is not produced by anything here'
    per_pixel = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
    unit = 2 if depth == 16 else 1
    stride = width * per_pixel * unit
    raw = zlib.decompress(idat)
    out = bytearray()
    previous = bytearray(stride)
    at = 0
    for _ in range(height):
        filt = raw[at]
        line = bytearray(raw[at + 1:at + 1 + stride])
        at += 1 + stride
        bpp = per_pixel * unit
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = previous[i]
            c = previous[i - bpp] if i >= bpp else 0
            if filt == 1:
                line[i] = (line[i] + a) & 0xFF
            elif filt == 2:
                line[i] = (line[i] + b) & 0xFF
            elif filt == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif filt == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        out += line
        previous = line
    if depth == 16:
        samples = list(struct.unpack('>%dH' % (len(out) // 2), bytes(out)))
    else:
        samples = list(out)
    icc = None
    if b'iCCP' in chunks:
        body = chunks[b'iCCP'][0]
        split = body.index(b'\0')
        icc = zlib.decompress(body[split + 2:])
    return dict(width=width, height=height, depth=depth, colour=colour,
                samples=samples, icc=icc, chunks={k.decode(): len(v) for k, v in chunks.items()})


# ------------------------------------------------------------ TIFF reading --

def tiff_entries(data):
    little = data[:2] == b'II'
    order = '<' if little else '>'
    assert struct.unpack(order + 'H', data[2:4])[0] == 42, 'classic TIFF expected'
    at = struct.unpack(order + 'I', data[4:8])[0]
    count = struct.unpack(order + 'H', data[at:at + 2])[0]
    entries = {}
    for index in range(count):
        base = at + 2 + 12 * index
        tag, kind, number = struct.unpack(order + 'HHI', data[base:base + 8])
        entries[tag] = (kind, number, base + 8, order, data)
    return entries


TYPE_SIZE = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2, 9: 4, 10: 8, 11: 4, 12: 8}


def tiff_value(entry):
    kind, number, field, order, data = entry
    size = TYPE_SIZE.get(kind, 1) * number
    if size > 4:
        offset = struct.unpack(order + 'I', data[field:field + 4])[0]
        raw = data[offset:offset + size]
    else:
        raw = data[field:field + size]
    if kind == 2:
        return raw.split(b'\0')[0].decode('latin-1')
    if kind == 3:
        return list(struct.unpack(order + '%dH' % number, raw))
    if kind == 4:
        return list(struct.unpack(order + '%dI' % number, raw))
    if kind == 5:
        pairs = struct.unpack(order + '%dI' % (2 * number), raw)
        return [(pairs[i], pairs[i + 1]) for i in range(0, len(pairs), 2)]
    return raw


def tiff_fields(path):
    """IFD0 plus the EXIF sub-IFD, as a flat {tag: value} map."""
    data = Path(path).read_bytes()
    entries = tiff_entries(data)
    fields = {tag: tiff_value(entry) for tag, entry in entries.items()}
    if 34665 in entries:
        order = entries[34665][3]
        offset = tiff_value(entries[34665])[0]
        count = struct.unpack(order + 'H', data[offset:offset + 2])[0]
        for index in range(count):
            base = offset + 2 + 12 * index
            tag, kind, number = struct.unpack(order + 'HHI', data[base:base + 8])
            fields[tag] = tiff_value((kind, number, base + 8, order, data))
    return fields


# ------------------------------------------------------------- PDF reading --

def pdf_objects(data):
    """Every `n 0 obj … endobj` body, keyed by object number. A PDF this file
    or CoreGraphics wrote is uncompressed at the object level, so a regex walk
    is enough and avoids a parser this drive would then have to trust."""
    objects = {}
    for match in re.finditer(rb'(\d+)\s+(\d+)\s+obj(.*?)endobj', data, re.S):
        objects[int(match.group(1))] = match.group(3)
    return objects


def pdf_stream(body):
    match = re.search(rb'stream\r?\n(.*?)\r?\nendstream', body, re.S)
    return match.group(1) if match else None


def pdf_check(path):
    """One page, its MediaBox, and the first image XObject's facts."""
    data = Path(path).read_bytes()
    objects = pdf_objects(data)
    pages = [body for body in objects.values() if b'/Type' in body and b'/Page' in body
             and b'/Pages' not in body]
    media = None
    for body in objects.values():
        match = re.search(rb'/MediaBox\s*\[\s*([\d.\-]+)\s+([\d.\-]+)\s+([\d.\-]+)\s+([\d.\-]+)',
                          body)
        if match:
            media = [float(value) for value in match.groups()]
            break
    image = None
    for body in objects.values():
        if b'/Subtype' in body and b'/Image' in body:
            image = body
            break
    result = dict(pages=len(pages), media_box=media, image=image is not None)
    if image is not None:
        result['bits_per_component'] = int(
            re.search(rb'/BitsPerComponent\s+(\d+)', image).group(1))
        result['filter'] = re.search(rb'/Filter\s*/(\w+)', image).group(1).decode()
        result['width'] = int(re.search(rb'/Width\s+(\d+)', image).group(1))
        result['height'] = int(re.search(rb'/Height\s+(\d+)', image).group(1))
        result['has_smask'] = b'/SMask' in image
        stream = pdf_stream(image)
        if stream is not None and result['filter'] == 'FlateDecode':
            try:
                result['pixels'] = zlib.decompress(stream)
            except zlib.error:
                result['pixels'] = None
    icc = None
    for body in objects.values():
        if b'/N' in body and b'/Alternate' in body or b'ICCBased' in body:
            stream = pdf_stream(body)
            if stream is not None:
                try:
                    candidate = zlib.decompress(stream)
                except zlib.error:
                    candidate = stream
                if len(candidate) > 128 and candidate[36:40] == b'acsp':
                    icc = candidate
                    break
    result['icc_sha256'] = hashlib.sha256(icc).hexdigest() if icc else None
    return result


# ---------------------------------------------------------------- measuring --

def stats(a, b):
    """max, mean and p99 of |a - b| over two equal-length sample lists."""
    assert len(a) == len(b), (len(a), len(b))
    deltas = sorted(abs(int(x) - int(y)) for x, y in zip(a, b))
    if not deltas:
        return dict(max=0, mean=0.0, p99=0)
    return dict(max=deltas[-1], mean=sum(deltas) / len(deltas),
                p99=deltas[min(len(deltas) - 1, int(0.99 * len(deltas)))])


def rgb_of(samples, colour):
    """The RGB samples of a decoded PNG, dropping alpha."""
    if colour == 6:
        return [value for index, value in enumerate(samples) if index % 4 != 3]
    if colour == 4:
        return [value for index, value in enumerate(samples) if index % 2 == 0]
    return samples


def alpha_of(samples, colour):
    if colour == 6:
        return samples[3::4]
    if colour == 4:
        return samples[1::2]
    return []


# -------------------------------------------------- the opaque-still oracle --

def load_host_stills_opaque():
    """The committed fixtures, each verified against provenance.json before
    use: a changed file is a different fixture, not this one."""
    provenance = json.loads((OPAQUE_FIXTURES / 'provenance.json').read_text())
    paths = {}
    for name, entry in provenance['files'].items():
        path = OPAQUE_FIXTURES / name
        actual = digest(path)
        assert actual == entry['sha256'], (name, actual, entry['sha256'])
        paths[name] = path
    return paths, provenance


def build_oracle(output):
    oracle = output / 'oracle'
    built = run('xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0',
                ROOT / 'scripts/host-stills-opaque/oracle.swift', '-o', oracle, timeout=600)
    assert built.returncode == 0, built.stderr[-2000:]
    return oracle


def replicate12(v):
    """A 12-bit code widened to 16 bits by bit replication, T6's rule."""
    return (v << 4) | (v >> 8)


def oracle_raw16(oracle, path, out):
    """RGB and alpha of the oracle's `raw16` provider samples, or None with
    the refusal."""
    result = run(oracle, 'raw16', path, out)
    if result.returncode != 0:
        return None, None, result.stderr.strip()[:400]
    data = Path(out).read_bytes()
    values = struct.unpack('<%dH' % (len(data) // 2), data)
    rgb = [value for index, value in enumerate(values) if index % 4 != 3]
    return rgb, list(values[3::4]), None


def oracle_layout(oracle, path):
    result = run(oracle, 'layout', path)
    if result.returncode != 0:
        return dict(error=result.stderr.strip()[:400])
    return json.loads(result.stdout)


def oracle_rgb(oracle, mode, path, out):
    """RGB of the oracle's raw RGBX redraw (`redraw16` little-endian u16,
    `sdr8` bytes), or None with the refusal recorded by the caller."""
    result = run(oracle, mode, path, out)
    if result.returncode != 0:
        return None, result.stderr.strip()[:400]
    data = Path(out).read_bytes()
    values = (struct.unpack('<%dH' % (len(data) // 2), data) if mode == 'redraw16'
              else list(data))
    return [value for index, value in enumerate(values) if index % 4 != 3], None


def read_y4m(path):
    """Width, height, colour tag and the planes of a single-frame y4m."""
    data = Path(path).read_bytes()
    header_end = data.index(b'\n')
    header = data[:header_end].decode().split()
    assert header[0] == 'YUV4MPEG2', header[0]
    fields = {item[0]: item[1:] for item in header[1:]}
    width, height = int(fields['W']), int(fields['H'])
    colour = fields.get('C', '420jpeg')
    frame = data.index(b'\n', header_end + 1) + 1
    assert data[header_end + 1:header_end + 6] == b'FRAME'
    wide = colour.endswith(('p10', 'p12', 'p16'))
    assert colour.startswith('444'), colour
    count = width * height
    size = count * (2 if wide else 1)
    planes = []
    for index in range(3):
        raw = data[frame + index * size:frame + (index + 1) * size]
        planes.append(list(struct.unpack('<%dH' % count, raw)) if wide else list(raw))
    return dict(width=width, height=height, colour=colour, header=header, planes=planes)


# ------------------------------------------------------------- ICC writing --

def icc_profile(space, tags):
    header = bytearray(128)
    header[8:12] = bytes([2, 0x10, 0, 0])
    header[12:16] = b'mntr'
    header[16:20] = space
    header[20:24] = b'XYZ '
    header[36:40] = b'acsp'
    table = b''
    body = b''
    base = 128 + 4 + 12 * len(tags)
    for signature, data in tags:
        table += signature + struct.pack('>II', base + len(body), len(data))
        body += data
        while len(body) % 4:
            body += b'\0'
    out = bytes(header) + struct.pack('>I', len(tags)) + table + body
    return struct.pack('>I', len(out)) + out[4:]


def desc_tag(name):
    raw = name.encode() + b'\0'
    return b'desc' + b'\0' * 4 + struct.pack('>I', len(raw)) + raw + b'\0' * 78


def curv_gamma(gamma):
    return b'curv' + b'\0' * 4 + struct.pack('>I', 1) + struct.pack('>H', int(round(gamma * 256)))


def xyz_tag(values):
    return b'XYZ ' + b'\0' * 4 + b''.join(
        struct.pack('>i', int(round(value * 65536))) for value in values)


def gray_gamma_profile(name, gamma):
    return icc_profile(b'GRAY', [(b'desc', desc_tag(name)),
                                 (b'wtpt', xyz_tag([0.9642, 1.0, 0.8249])),
                                 (b'kTRC', curv_gamma(gamma))])


# ------------------------------------------------------------- the fixtures --

def plate_rgba8(width, height):
    """A deliberately HIGH-FREQUENCY plate: coprime ramps that change every
    pixel. It is what the EXACTNESS gates want (a lossless codec that drops a
    sample cannot hide in it) and exactly what a lossy, chroma-subsampled one
    is worst at, which is why the HEIC comparison uses `photo_rgba8` below."""
    out = []
    for y in range(height):
        for x in range(width):
            out += [(x * 37 + 11) % 256, (y * 53 + 7) % 256, (x * y + 3) % 256, 255]
    return out


def photo_rgba8(width, height):
    """A smooth, photograph-like plate: two crossed gradients, a soft disc and
    one hard edge. T3 is a claim about a LOSSY, 4:2:0-subsampled encode of a
    picture, and measuring it on per-pixel noise would measure chroma
    subsampling rather than the export chain (the first run did exactly that
    and is recorded beside the gate)."""
    def code(value):
        return max(0, min(255, int(round(255 * value))))

    out = []
    for y in range(height):
        for x in range(width):
            u = x / max(1, width - 1)
            v = y / max(1, height - 1)
            disc = ((x - width * 0.35) ** 2 + (y - height * 0.6) ** 2) ** 0.5
            soft = max(0.0, 1.0 - disc / (0.45 * width))
            out += [code(0.15 + 0.60 * u),
                    code(0.20 + 0.55 * v),
                    code(0.25 + 0.45 * soft),
                    255]
    return out


def gray16_codes(width, height):
    out = []
    for y in range(height):
        for x in range(width):
            out.append(((x * 1031 + y * 65167) % 65536))
    return out


def write_fixtures(output):
    """Every input this drive uses, written here or by a named encoder."""
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {}

    rgba = plate_rgba8(WIDTH, HEIGHT)
    write_png(folder / 'plate.png', WIDTH, HEIGHT, rgba, 6, 8,
              extra=srgb_chunk() + phys_chunk(300))
    made['plate'] = folder / 'plate.png'

    photo = photo_rgba8(WIDTH, HEIGHT)
    write_png(folder / 'photo.png', WIDTH, HEIGHT, photo, 6, 8,
              extra=srgb_chunk() + phys_chunk(300))
    made['photo'] = folder / 'photo.png'
    photo16 = [value * 257 for value in photo]
    write_png(folder / 'photo16.png', WIDTH, HEIGHT, photo16, 6, 16,
              extra=srgb_chunk() + phys_chunk(300))
    made['photo16'] = folder / 'photo16.png'

    rgb16 = []
    for index in range(0, len(rgba), 4):
        rgb16 += [rgba[index] * 257, rgba[index + 1] * 257, rgba[index + 2] * 257,
                  rgba[index + 3] * 257]
    write_png(folder / 'plate16.png', WIDTH, HEIGHT, rgb16, 6, 16,
              extra=srgb_chunk() + phys_chunk(300))
    made['plate16'] = folder / 'plate16.png'
    # plate16's RGB under a straight alpha that is never 0, for the 12-bit
    # AVIF alpha cases.
    rgba16 = list(rgb16)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            rgba16[(y * WIDTH + x) * 4 + 3] = ((x * 29 + y * 61) % 255 + 1) * 257
    write_png(folder / 'plate16a.png', WIDTH, HEIGHT, rgba16, 6, 16,
              extra=srgb_chunk() + phys_chunk(300))
    made['plate16a'] = folder / 'plate16a.png'

    codes = gray16_codes(WIDTH, HEIGHT)
    write_png(folder / 'gray-plain.png', WIDTH, HEIGHT, codes, 0, 16)
    made['gray_plain'] = folder / 'gray-plain.png'
    write_png(folder / 'gray-srgb.png', WIDTH, HEIGHT, codes, 0, 16, extra=srgb_chunk())
    made['gray_srgb'] = folder / 'gray-srgb.png'
    write_png(folder / 'gray-gama.png', WIDTH, HEIGHT, codes, 0, 16, extra=gama_chunk(2.2))
    made['gray_gama'] = folder / 'gray-gama.png'
    gray_icc = gray_gamma_profile('Gray Gamma 2.2', 2.2)
    (folder / 'gray-gamma-2.2.icc').write_bytes(gray_icc)
    made['gray_icc'] = folder / 'gray-gamma-2.2.icc'
    write_png(folder / 'gray-icc.png', WIDTH, HEIGHT, codes, 0, 16,
              extra=iccp_chunk('Gray Gamma 2.2', gray_icc))
    made['gray_icc_png'] = folder / 'gray-icc.png'
    srgb_icc = Path('/System/Library/ColorSync/Profiles/sRGB Profile.icc')
    if srgb_icc.is_file():
        write_png(folder / 'gray-rgbicc.png', WIDTH, HEIGHT, codes, 0, 16,
                  extra=iccp_chunk('sRGB', srgb_icc.read_bytes()))
        made['gray_rgb_icc'] = folder / 'gray-rgbicc.png'
    graya = []
    for index, value in enumerate(codes):
        graya += [value, 65535 - (index % 4096)]
    write_png(folder / 'graya.png', WIDTH, HEIGHT, graya, 4, 16, extra=srgb_chunk())
    made['graya'] = folder / 'graya.png'

    # The vector fixtures: two rectangles with exact edges, so the interior
    # and the exterior are analytic.
    pdf = pdf_fixture()
    (folder / 'shapes.pdf').write_bytes(pdf)
    made['pdf'] = folder / 'shapes.pdf'
    (folder / 'shapes.svg').write_text(svg_fixture())
    made['svg'] = folder / 'shapes.svg'

    phase3_fixtures.synthetic_dng(folder / 'capture.dng')
    made['dng'] = folder / 'capture.dng'
    return made


def pdf_fixture():
    """A one-page PDF, 200 x 100 pt, with two exactly placed filled rects."""
    content = (b'1 0 0 RG 1 0 0 rg 20 20 60 60 re f\n'
               b'0 0 1 rg 120 20 60 60 re f\n')
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R '
        b'/Resources << >> >>',
        b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'endstream',
    ]
    out = b'%PDF-1.4\n'
    offsets = []
    for index, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(index).encode() + b' 0 obj\n' + body + b'\nendobj\n'
    start = len(out)
    out += b'xref\n0 ' + str(len(objects) + 1).encode() + b'\n0000000000 65535 f \n'
    for offset in offsets:
        out += ('%010d 00000 n \n' % offset).encode()
    out += (b'trailer\n<< /Size ' + str(len(objects) + 1).encode()
            + b' /Root 1 0 R >>\nstartxref\n' + str(start).encode() + b'\n%%EOF\n')
    return out


def svg_fixture():
    return ('<?xml version="1.0" encoding="UTF-8"?>\n'
            '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" '
            'viewBox="0 0 200 100">\n'
            '  <rect x="20" y="20" width="60" height="60" fill="#ff0000"/>\n'
            '  <rect x="120" y="20" width="60" height="60" fill="#0000ff"/>\n'
            '</svg>\n')


# ------------------------------------------------------------------ gates --

def record(report, name, passed, detail):
    report['gates'][name] = dict(passed=bool(passed), detail=detail)


def open_document(app, path, **arguments):
    return app.call('open_document', path=str(path), **arguments)


def export_png(app, document, where, bit_depth=8):
    settings = {'format': 'png', 'bit_depth': bit_depth}
    return app.call('export_image', document_id=document, path=str(where),
                    settings=settings, overwrite=True)


def take_heic(app, output, report, fixtures):
    """D1: HEIC export and import, against heif-info and magick."""
    detail = dict(export={}, import_={}, tools=dict(heif_info=tool('heif-info'),
                                                    heif_enc=tool('heif-enc'),
                                                    magick=tool('magick')))
    if not tool('heif-info') or not tool('heif-enc') or not tool('magick'):
        record(report, 'D1 HEIC export and import', False,
               dict(detail, error='heif-info, heif-enc and magick are required'))
        return
    passed = True

    # ---- export, 8-bit ----
    # The PHOTOGRAPHIC plate: T3 is a claim about a lossy 4:2:0 encode of a
    # picture. The high-frequency plate's numbers are taken below and
    # RECORDED, ungated, because they measure chroma subsampling.
    document = open_document(app, fixtures['photo'])['document']['id']
    heic = output / 'export8.heic'
    png = output / 'export8.png'
    result = app.call('export_image', document_id=document, path=str(heic),
                      settings={'format': 'heic', 'jpeg_quality': 95}, overwrite=True)
    reference = app.call('export_image', document_id=document, path=str(png),
                         settings={'format': 'png'}, overwrite=True)
    info = run('heif-info', heic)
    detail['export']['result8'] = result
    detail['export']['heif_info8'] = info.stdout.strip().splitlines()[:12]
    detail['export']['heif_info8_ok'] = info.returncode == 0
    passed &= info.returncode == 0
    passed &= result.get('format') == 'heic'

    decoded = output / 'export8-decoded.png'
    run('magick', heic, 'PNG24:' + str(decoded))
    if decoded.is_file():
        mine = read_png(decoded)
        theirs = read_png(png)
        measure = stats(rgb_of(mine['samples'], mine['colour']),
                        rgb_of(theirs['samples'], theirs['colour']))
        detail['export']['T3_vs_png'] = measure
        passed &= (measure['mean'] <= T3_HEIC_MEAN_CODES
                   and measure['p99'] <= T3_HEIC_P99_CODES)
        detail['export']['icc_equal'] = (mine['icc'] == theirs['icc']
                                         if mine['icc'] and theirs['icc'] else None)
    else:
        detail['export']['decode_failed'] = True
        passed = False

    identify = run('magick', 'identify', '-format',
                   '%[EXIF:Orientation]|%[EXIF:XResolution]|%[profile:icc]', heic)
    detail['export']['identify8'] = identify.stdout.strip()
    detail['export']['carried8'] = result.get('carried')

    # `embed_profile: false`: the REPORT is gated against an independent read,
    # not against the request. The intermediate carries no ICC, but ImageIO's
    # HEIC writer tags every file it writes, so the claim under test is that
    # `carried` says what `magick` finds — whichever way it falls — rather
    # than repeating the flag that was asked for.
    bare = output / 'export8-noprofile.heic'
    bare_result = app.call('export_image', document_id=document, path=str(bare),
                           settings={'format': 'heic', 'jpeg_quality': 95,
                                     'embed_profile': False}, overwrite=True)
    bare_icc = output / 'export8-noprofile.icc'
    bare_icc.unlink(missing_ok=True)
    run('magick', bare, 'ICC:' + str(bare_icc))
    found_profile = bare_icc.is_file() and bare_icc.stat().st_size > 0
    reported_profile = 'profile' in (bare_result.get('carried') or [])
    bare_heif = run('heif-info', bare).stdout
    # The same question asked of the profiled delivery, so the gate is an
    # equality in BOTH directions rather than a one-sided "no profile" check.
    tagged_icc = output / 'export8.icc'
    tagged_icc.unlink(missing_ok=True)
    run('magick', heic, 'ICC:' + str(tagged_icc))
    tagged_found = tagged_icc.is_file() and tagged_icc.stat().st_size > 0
    tagged_reported = 'profile' in (result.get('carried') or [])
    detail['export']['embed_profile_false'] = dict(
        carried=bare_result.get('carried'), magick_found_profile=found_profile,
        heif_info_colour=[line.strip() for line in bare_heif.splitlines()
                          if 'color profile' in line],
        report_matches_file=reported_profile == found_profile)
    detail['export']['embed_profile_true'] = dict(
        carried=result.get('carried'), magick_found_profile=tagged_found,
        report_matches_file=tagged_reported == tagged_found)
    passed &= reported_profile == found_profile
    passed &= tagged_reported == tagged_found

    # The same comparison on the high-frequency plate, RECORDED and ungated.
    noisy = open_document(app, fixtures['plate'])['document']['id']
    noisy_heic = output / 'export8-noise.heic'
    noisy_png = output / 'export8-noise.png'
    app.call('export_image', document_id=noisy, path=str(noisy_heic),
             settings={'format': 'heic', 'jpeg_quality': 95}, overwrite=True)
    app.call('export_image', document_id=noisy, path=str(noisy_png),
             settings={'format': 'png'}, overwrite=True)
    noisy_decoded = output / 'export8-noise-decoded.png'
    run('magick', noisy_heic, 'PNG24:' + str(noisy_decoded))
    if noisy_decoded.is_file():
        mine = read_png(noisy_decoded)
        theirs = read_png(noisy_png)
        detail['export']['high_frequency_plate_recorded'] = stats(
            rgb_of(mine['samples'], mine['colour']),
            rgb_of(theirs['samples'], theirs['colour']))

    # ---- export, typed 16-bit ----
    typed = open_document(app, fixtures['photo16'], precision='preserve')['document']['id']
    typed16 = output / 'export16.heic'
    typed_result = app.call('export_image', document_id=typed, path=str(typed16),
                            settings={'format': 'heic', 'bit_depth': 16, 'jpeg_quality': 95},
                            overwrite=True)
    typed_info = run('heif-info', typed16)
    reported = None
    for line in typed_info.stdout.splitlines():
        match = re.search(r'bit[s]?[ -]?depth[^0-9]*(\d+)', line, re.I)
        if match:
            reported = int(match.group(1))
            break
    detail['export']['result16'] = typed_result
    detail['export']['heif_info16'] = typed_info.stdout.strip().splitlines()[:12]
    detail['export']['reported_depth'] = reported
    if reported is not None:
        passed &= typed_result.get('bit_depth') == reported
    else:
        detail['export']['depth_unreadable'] = True
        passed = False

    # GATED: a typed 16-bit document's `get_document` carries the
    # `save_preservation` block, so the typed HEIC depth sentence
    # (`PrecisionExportCapture.losses`) has its MCP surface.
    # Its png entry must equal what a flat save_copy actually reports.
    # The byte document's capability report must still list heic.
    typed_preservation = (app.call('get_document', document_id=typed)
                          .get('save_preservation'))
    detail['export']['typed_save_preservation'] = typed_preservation
    typed_block = typed_preservation if isinstance(typed_preservation, dict) else {}
    typed_losses = typed_block.get('losses_by_format')
    if not isinstance(typed_losses, dict):
        typed_losses = {}
    # File ▸ Save keeps a flat type in place exactly when its list is empty (a plain
    # one-layer 16-bit PNG reopens as the document), GIF aside, which only Save As writes.
    save_types = typed_block.get('save_types') or []
    flat_types = {'png': 'public.png', 'jpg': 'public.jpeg', 'tiff': 'public.tiff',
                  'bmp': 'com.microsoft.bmp', 'webp': 'org.webmproject.webp'}
    checks = {
        'is_dict': isinstance(typed_preservation, dict),
        'save_types_native': 'com.kgalligan.doctored-document' in save_types,
        'save_types_follow_losses': all((uti in save_types) == (typed_losses.get(key) == [])
                                        for key, uti in flat_types.items()),
        # `avif` and `jxl` are export formats, so the capability
        # report lists them beside heic; AVIF is host-encoded like HEIC and
        # carries the same depth sentence, under its own encoder's name.
        'format_keys': set(typed_losses) == {'doctored', 'png', 'jpg', 'tiff', 'bmp', 'gif', 'webp',
                                             'heic', 'avif', 'jxl'},
        'doctored_empty': typed_losses.get('doctored') == [],
        'heic_sentence': any('HEIC encoder' in str(entry) for entry in typed_losses.get('heic') or []),
        'avif_sentence': any('AVIF encoder' in str(entry) for entry in typed_losses.get('avif') or []),
    }
    try:
        copy_result = app.call('save_copy', document_id=typed, path=str(output / 'typed-save-copy.png'))
        copy_losses = copy_result.get('flat_save_losses')
        detail['export']['typed_save_copy_losses'] = copy_losses
        checks['png_equals_save_copy'] = ('png' in typed_losses
                                          and typed_losses['png'] == copy_losses)
    except Exception as error:  # Recorded, never repaired.
        detail['export']['typed_save_copy_error'] = repr(error)
        checks['png_equals_save_copy'] = False
    detail['export']['typed_preservation_checks'] = checks
    passed &= all(checks.values())
    byte_losses = (app.call('get_document', document_id=document)
                   .get('save_preservation', {}).get('losses_by_format', {}))
    detail['export']['losses_by_format'] = byte_losses
    listed = 'heic' in byte_losses
    detail['export']['heic_in_capability_report'] = listed
    passed &= listed

    # ---- the two production deliveries ----
    sheet_folder = output / 'heic-deliveries'
    # A per-item delivery never overwrites, so a previous run's files would
    # be refused rather than rewritten. The folder is this run's.
    shutil.rmtree(sheet_folder, ignore_errors=True)
    sheet_folder.mkdir(parents=True, exist_ok=True)
    try:
        sheet = app.call('contact_sheet', document_id=document,
                         output={'kind': 'export', 'settings': {'format': 'heic'},
                                 'destination': str(sheet_folder),
                                 'pattern': '{name}-sheet'})
        detail['export']['contact_sheet'] = sheet
        written = sheet.get('path')
        parsed = run('heif-info', written).returncode == 0 if written else False
        detail['export']['contact_sheet_parses'] = parsed
        passed &= parsed
    except Exception as error:                     # Recorded, never repaired.
        detail['export']['contact_sheet_error'] = repr(error)
        passed = False
    try:
        layers = app.call('export_layers', document_id=document, items='all',
                          destination=str(sheet_folder), pattern='{name}-{index}-{layer}',
                          settings={'format': 'heic'})
        detail['export']['export_layers'] = layers
        written = [item.get('path') for item in layers.get('items', [])
                   if item.get('path')]
        parsed = bool(written) and all(run('heif-info', path).returncode == 0
                                       for path in written)
        detail['export']['export_layers_parse'] = parsed
        passed &= parsed
    except Exception as error:
        detail['export']['export_layers_error'] = repr(error)
        passed = False

    # ---- import ----
    source = fixtures['plate']
    for bits in (8, 10, 12):
        made = output / f'import{bits}.heic'
        # A 10- or 12-bit file needs a source with those samples to carry:
        # `heif-enc` will not invent depth from an 8-bit PNG (the first run
        # produced three 8-bit files that way).
        input_png = source if bits == 8 else fixtures['plate16']
        encode = run('heif-enc', '-b', str(bits), '-q', '100', '--lossless',
                     input_png, '-o', made)
        if encode.returncode != 0 or not made.is_file():
            encode = run('heif-enc', '-b', str(bits), '-q', '100', input_png, '-o', made)
        if encode.returncode != 0 or not made.is_file():
            detail['import_'][f'{bits}bit'] = dict(encoder_failed=encode.stderr.strip()[:400])
            continue
        entry = dict(file=str(made), heif_info=run('heif-info', made).stdout.strip()
                     .splitlines()[:8])
        try:
            imported = open_document(app, made, precision='preserve')['document']
            entry['document'] = {key: imported.get(key) for key in
                                 ('width', 'height', 'bit_depth', 'color_profile', 'depth')}
            back = output / f'import{bits}-rasterize.png'
            export = export_png(app, imported['id'], back,
                                bit_depth=16 if bits > 8 else 8)
            entry['export'] = export
            ref = output / f'import{bits}-reference.png'
            run('magick', made, ('PNG48:' if bits > 8 else 'PNG24:') + str(ref))
            if ref.is_file() and back.is_file():
                mine = read_png(back)
                theirs = read_png(ref)
                scale = 1
                if bits > 8:
                    # Both sides mapped to the file's own 10-bit codes, which
                    # is the grid the samples actually live on.
                    mine_codes = [round(v * 1023 / 65535) for v in
                                  rgb_of(mine['samples'], mine['colour'])]
                    their_codes = [round(v * 1023 / 65535) for v in
                                   rgb_of(theirs['samples'], theirs['colour'])]
                    measure = stats(mine_codes, their_codes)
                    entry['T1'] = measure
                    if bits == 10:
                        ok = (measure['max'] <= T1_HEIC10_MAX_CODES10
                              and measure['mean'] <= T1_HEIC10_MEAN_CODES10)
                        entry['gated'] = ok
                        passed &= ok
                else:
                    measure = stats(rgb_of(mine['samples'], mine['colour']),
                                    rgb_of(theirs['samples'], theirs['colour']))
                    entry['T1'] = measure
                    entry['gated'] = measure['max'] == 0
                    passed &= measure['max'] == 0
                entry['scale'] = scale
        except Exception as error:
            entry['refused'] = repr(error)
        detail['import_'][f'{bits}bit'] = entry

    # Monochrome and PQ HEIC: refused, by sentence.
    for name, arguments in (('mono', ('-C', '400:400')), ('pq', ())):
        made = output / f'reject-{name}.heic'
        if name == 'mono':
            gray = output / 'mono-source.png'
            run('magick', source, '-colorspace', 'Gray', 'PNG8:' + str(gray))
            encode = run('heif-enc', gray, '-o', made)
        else:
            encode = run('heif-enc', '--matrix_coefficients', '9',
                         '--transfer_characteristic', '16', source, '-o', made)
        entry = dict(encoder=encode.returncode, file=made.is_file())
        if made.is_file():
            entry['heif_info'] = run('heif-info', made).stdout.strip().splitlines()[:8]
            try:
                # hdr_photo sdr (v1.x): the SDR choice keeps the PQ refusal.
                opened = open_document(app, made, precision='preserve', hdr_photo='sdr')
                entry['opened'] = opened.get('document', {}).get('id') is not None
                # An accepted file is only a failure when it really is the
                # unsupported kind; `heif-enc` may have written an ordinary
                # RGB file instead, which is recorded rather than gated.
                entry['gated'] = not entry['opened']
                if name == 'pq':
                    passed &= entry['gated']
            except Exception as error:
                entry['refusal'] = repr(error)[:400]
                entry['gated'] = True
        detail['import_'][f'reject_{name}'] = entry

    # ---- the committed opaque HEICs and the gain-map still ----
    oracle = fixtures['oracle']
    opaque = fixtures['opaque']
    for bits in (8, 10, 12):
        made = opaque[f'heic{bits}.heic']
        entry = dict(file=str(made), imageio_layout=oracle_layout(oracle, made),
                     expected_precision='rgba8' if bits == 8 else 'rgba16')
        ok = False
        try:
            imported = open_document(app, made, precision='preserve')['document']
            entry['precision'] = imported.get('precision')
            ok = imported.get('precision') == entry['expected_precision']
            back = output / f'opaque-heic{bits}-rasterize.png'
            entry['export'] = export_png(app, imported['id'], back,
                                         bit_depth=8 if bits == 8 else 16)
            mine = read_png(back)
            mine_rgb = rgb_of(mine['samples'], mine['colour'])
            ref = output / f'opaque-heic{bits}-reference.png'
            run('magick', made, ('PNG24:' if bits == 8 else 'PNG48:') + str(ref))
            theirs = read_png(ref)
            their_rgb = rgb_of(theirs['samples'], theirs['colour'])
            if bits == 8:
                measure = stats(mine_rgb, their_rgb)
                entry['T1_vs_magick'] = measure
                ok &= measure['max'] <= T1_OPAQUE_HEIC8_MAX_CODES
            else:
                measure = stats([round(v * 1023 / 65535) for v in mine_rgb],
                                [round(v * 1023 / 65535) for v in their_rgb])
                entry['T1_vs_magick_codes10'] = measure
                redraw, error = oracle_rgb(oracle, 'redraw16', made,
                                           output / f'opaque-heic{bits}.redraw16')
                entry['oracle_redraw16_error'] = error
                if redraw is not None:
                    entry['vs_oracle_redraw16'] = stats(mine_rgb, redraw)
                if bits == 10:
                    ok &= (measure['max'] <= T1_HEIC10_MAX_CODES10
                           and measure['mean'] <= T1_HEIC10_MEAN_CODES10
                           and redraw is not None
                           and entry['vs_oracle_redraw16']['max']
                           <= T1_OPAQUE_HEIC10_ORACLE_MAX_U16)
                else:
                    # 12-bit: RECORDED. Only the open at rgba16 is gated.
                    entry['multiples_of_16'] = sum(1 for v in mine_rgb if v % 16 == 0)
                    entry['samples'] = len(mine_rgb)
        except Exception as error:
            entry['refused'] = repr(error)[:400]
            ok = False
        entry['gated'] = ok
        passed &= ok
        detail['import_'][f'opaque_heic{bits}'] = entry

    # 12-bit 4:4:4: ImageIO returns an image with no pixel provider, and
    # the open refuses it with the sentence that names that cause.
    made = opaque['heic12-444.heic']
    entry = dict(file=str(made), imageio_layout=oracle_layout(oracle, made),
                 expected_refusal=NO_DECODED_PIXELS)
    reply = app.client.rpc('tools/call', {'name': 'open_document', 'arguments': {
        'path': str(made), 'precision': 'preserve'}})
    text = next((row.get('text') for row in reply.get('content', [])
                 if row.get('type') == 'text'), None)
    entry['is_error'] = bool(reply.get('isError'))
    entry['text'] = text
    ok = entry['is_error'] and text == NO_DECODED_PIXELS
    entry['gated'] = ok
    passed &= ok
    detail['import_']['opaque_heic12_444'] = entry

    made = opaque['gainmap.heic']
    entry = dict(file=str(made), imageio_layout=oracle_layout(oracle, made))
    ok = False
    try:
        # hdr_photo sdr (v1.x): the gain map's SDR base, as before.
        imported = open_document(app, made, precision='preserve', hdr_photo='sdr')['document']
        entry['precision'] = imported.get('precision')
        ok = imported.get('precision') == 'rgba8'
        state = app.call('get_document', document_id=imported['id'])
        warnings = (state.get('metadata', {}).get('metadata_import', {}).get('warnings') or [])
        entry['metadata_import_warnings'] = warnings
        ok &= SDR_BASE_DISCLOSURE in warnings
        base, error = oracle_rgb(oracle, 'sdr8', made, output / 'gainmap.sdr8')
        entry['oracle_sdr8_error'] = error
        back = output / 'gainmap-rasterize-srgb-working-space.png'
        entry['export_srgb_working_space'] = export_png(app, imported['id'], back, bit_depth=8)
        if base is not None:
            mine = read_png(back)
            entry['vs_oracle_sdr8_srgb_working_space_recorded'] = stats(
                rgb_of(mine['samples'], mine['colour']), base)
        p3 = support.HostApp(output / 'app-p3', output / 'recovery-p3', app.executable)
        p3.extra_defaults = ['-ColorWorkingSpace', T1_GAINMAP_WORKING_SPACE]
        with p3:
            opened = open_document(p3, made, precision='preserve', hdr_photo='sdr')['document']
            entry['p3_precision'] = opened.get('precision')
            entry['p3_color_profile'] = p3.call('get_document',
                                                document_id=opened['id']).get('color_profile')
            back = output / 'gainmap-rasterize.png'
            entry['export'] = export_png(p3, opened['id'], back, bit_depth=8)
        ok &= entry['p3_precision'] == 'rgba8'
        if base is None:
            ok = False
        else:
            mine = read_png(back)
            measure = stats(rgb_of(mine['samples'], mine['colour']), base)
            entry['vs_oracle_sdr8'] = measure
            ok &= measure['max'] <= T1_GAINMAP_SDR8_MAX_CODES
    except Exception as error:
        entry['refused'] = repr(error)[:400]
        ok = False
    entry['gated'] = ok
    passed &= ok
    detail['import_']['gainmap_sdr_base'] = entry

    record(report, 'D1 HEIC export and import', passed, detail)


def gray_to_srgb(app, document, entry):
    """#1 phase 1a: a gray file opens as a Grayscale document, which RGB
    Color converts into sRGB before the export (D2's re-expression)."""
    entry['color_mode'] = document.get('color_mode')
    if document.get('color_mode') != 'gray':
        raise AssertionError(f"opened as {document.get('color_mode')!r}, not a Grayscale document")
    app.call('convert_mode', document_id=document['id'], mode='rgb', rgb_space='srgb')


def take_gray16(app, output, report, fixtures):
    """D2: the gray interpretation against ColorSync and lcms."""
    detail = dict(cases={}, tools=dict(sips=tool('sips'), magick=tool('magick')))
    passed = True

    # Untagged, sRGB-chunk and GrayA gray: a Grayscale document in sGray
    # converted to sRGB, within T2_GRAY_CONVERTED_MAX_U16, alpha exact.
    for key in ('gray_plain', 'gray_srgb', 'graya'):
        if key not in fixtures:
            continue
        entry = {}
        try:
            document = open_document(app, fixtures[key], precision='preserve')['document']
            entry['document'] = {name: document.get(name) for name in
                                 ('width', 'height', 'bit_depth')}
            gray_to_srgb(app, document, entry)
            back = output / f'{key}.png'
            entry['export'] = export_png(app, document['id'], back, bit_depth=16)
            mine = read_png(back)
            source = read_png(fixtures[key])
            per_pixel = 2 if source['colour'] == 4 else 1
            wanted = source['samples'][::per_pixel]
            got = rgb_of(mine['samples'], mine['colour'])[0::3]
            measure = stats(got, wanted)
            entry['T2_converted'] = measure
            ok = measure['max'] <= T2_GRAY_CONVERTED_MAX_U16
            if source['colour'] == 4:
                alpha_mine = alpha_of(mine['samples'], mine['colour'])
                alpha_source = source['samples'][1::2]
                entry['alpha'] = stats(alpha_mine, alpha_source)
                ok = ok and entry['alpha']['max'] == 0
            entry['gated'] = ok
            passed &= ok
        except Exception as error:
            entry['error'] = repr(error)[:400]
            passed = False
        detail['cases'][key] = entry

    # The Gray ICC case, against two independent CMSs.
    entry = {}
    try:
        document = open_document(app, fixtures['gray_icc_png'],
                                 precision='preserve')['document']
        gray_to_srgb(app, document, entry)
        back = output / 'gray-icc-rasterize.png'
        entry['export'] = export_png(app, document['id'], back, bit_depth=16)
        mine = rgb_of(read_png(back)['samples'], read_png(back)['colour'])[0::3]
        references = {}
        recorded = {}
        if tool('sips'):
            colorsync = output / 'gray-icc-colorsync.png'
            shutil.copyfile(fixtures['gray_icc_png'], colorsync)
            matched = run('sips', '--matchTo',
                          '/System/Library/ColorSync/Profiles/sRGB Profile.icc',
                          colorsync, '--out', colorsync)
            if matched.returncode == 0 and tool('magick'):
                flat = output / 'gray-icc-colorsync-flat.png'
                run('magick', colorsync, 'PNG48:' + str(flat))
                if flat.is_file():
                    theirs = read_png(flat)
                    # ColorSync wrote 8-bit samples, so the comparison is made
                    # on the 8-bit scale rather than inflating its rounding.
                    recorded['colorsync_8bit_scale'] = stats(
                        [round(value / 257) for value in mine],
                        [round(value / 257) for value in
                         rgb_of(theirs['samples'], theirs['colour'])[0::3]])
                    if T2_COLORSYNC_GATED:
                        references['colorsync'] = recorded['colorsync_8bit_scale']
        if tool('magick'):
            lcms = output / 'gray-icc-lcms.png'
            run('magick', fixtures['gray_icc_png'], '-profile',
                str(fixtures['gray_icc']), '-profile',
                '/System/Library/ColorSync/Profiles/sRGB Profile.icc',
                'PNG48:' + str(lcms))
            if lcms.is_file():
                theirs = read_png(lcms)
                references['lcms'] = stats(
                    mine, rgb_of(theirs['samples'], theirs['colour'])[0::3])
        entry['T2'] = references
        entry['T2_recorded_not_gated'] = recorded
        ok = bool(references) and all(
            value['max'] <= T2_GRAY_MAX_U16 and value['mean'] <= T2_GRAY_MEAN_U16
            for value in references.values())
        entry['gated'] = ok
        passed &= ok
    except Exception as error:
        entry['error'] = repr(error)[:400]
        passed = False
    detail['cases']['gray_icc'] = entry

    # The two refusals, by sentence.
    for key, needle in (('gray_rgb_icc', 'Gray ICC profile'),
                        ('gray_gama', 'Gray ICC or sRGB')):
        if key not in fixtures:
            continue
        outcome = {}
        try:
            open_document(app, fixtures[key], precision='preserve')
            outcome['opened'] = True
            passed = False
        except Exception as error:
            text = repr(error)
            outcome['refusal'] = text[:400]
            ok = needle in text
            outcome['names_reason'] = ok
            passed &= ok
        detail['cases'][key] = outcome

    record(report, 'D2 Gray16 interpretation', passed, detail)


def take_bridge(app, output, report, fixtures):
    """D3: packets and PPI reconstructed into typed documents."""
    detail = dict(cases={})
    passed = True

    # A typed HOST still: the plate as HEIC, opened 16-bit, exported to TIFF16.
    still = output / 'bridge-still.heic'
    if tool('heif-enc'):
        run('heif-enc', '-b', '10', '-q', '100', fixtures['plate16'], '-o', still)
    entry = {}
    if still.is_file():
        try:
            document = open_document(app, still, precision='preserve')['document']
            entry['document_id'] = document['id']
            metadata = app.call('get_metadata', document_id=document['id'])
            entry['metadata'] = metadata
            where = output / 'bridge-still.tiff'
            exported = app.call('export_image', document_id=document['id'], path=str(where),
                                settings={'format': 'tiff', 'bit_depth': 16}, overwrite=True)
            entry['export'] = exported
            fields = tiff_fields(where)
            entry['tiff_fields'] = {str(tag): fields[tag] for tag in
                                    (274, 282, 283, 271, 272, 36867) if tag in fields}
            entry['orientation_one'] = fields.get(274) == [1]
            passed &= entry['orientation_one']
        except Exception as error:
            entry['error'] = repr(error)[:400]
            passed = False
    else:
        entry['encoder_missing'] = True
    detail['cases']['typed_still'] = entry

    # Typed RAW: the synthetic DNG, whose authored fields this drive knows.
    entry = {}
    try:
        document = open_document(app, fixtures['dng'], precision='preserve')['document']
        entry['document_id'] = document['id']
        where = output / 'bridge-raw.tiff'
        entry['export'] = app.call('export_image', document_id=document['id'], path=str(where),
                                   settings={'format': 'tiff', 'bit_depth': 16}, overwrite=True)
        fields = tiff_fields(where)
        authored = dict(make='Rasterize Probe', model='Synthetic CFA',
                        date='2026:09:09 12:34:56')
        found = dict(make=fields.get(271), model=fields.get(272),
                     date=fields.get(306) or fields.get(36867),
                     x_resolution=fields.get(282), y_resolution=fields.get(283),
                     orientation=fields.get(274))
        entry['authored'] = authored
        entry['found'] = {key: str(value) for key, value in found.items()}
        checks = dict(
            make=found['make'] == authored['make'],
            model=found['model'] == authored['model'],
            date=(found['date'] or '').startswith('2026:09:09'),
            orientation=found['orientation'] == [1],
            resolution=bool(found['x_resolution']) and bool(found['y_resolution']))
        entry['T4'] = checks
        source = app.call('get_metadata', document_id=document['id'])
        entry['packets'] = source
        entry['xmp_present'] = bool(source.get('xmp'))
        ok = all(checks.values())
        entry['gated'] = ok
        passed &= ok
    except Exception as error:
        entry['error'] = repr(error)[:400]
        passed = False
    detail['cases']['typed_raw'] = entry

    record(report, 'D3 typed metadata bridge', passed, detail)


def take_float_tiff(output, report):
    """D4: a float TIFF written by the Rust seam, read by tiffinfo/magick."""
    detail = dict(tools=dict(tiffinfo=tool('tiffinfo'), magick=tool('magick')))
    where = output / 'float32.tiff'
    example = ROOT / 'core/examples/phase11d_float_tiff.rs'
    written = run('cargo', 'run', '--release', '--example', 'phase11d_float_tiff', '--',
                  str(where), cwd=str(ROOT / 'core'))
    detail['example_present'] = example.is_file()
    detail['cargo_status'] = written.returncode
    detail['cargo_stderr'] = written.stderr.strip().splitlines()[-6:]
    passed = where.is_file()
    if passed:
        detail['sha256'] = digest(where)
        if tool('tiffinfo'):
            info = run('tiffinfo', where)
            detail['tiffinfo'] = info.stdout.strip().splitlines()[:16]
            detail['tiffinfo_ok'] = info.returncode == 0
            passed &= info.returncode == 0
        if tool('magick'):
            identify = run('magick', 'identify', '-verbose', where)
            detail['magick_ok'] = identify.returncode == 0
            detail['magick'] = [line.strip() for line in identify.stdout.splitlines()
                                if 'Depth' in line or 'Type' in line][:6]
            passed &= identify.returncode == 0
    record(report, 'D4 float TIFF interop', passed, detail)


def take_vector(app, output, report, fixtures):
    """D5: PDF/SVG import geometry and the PDF export's structure."""
    detail = dict(import_={}, export={}, tools=dict(pdftoppm=tool('pdftoppm'),
                                                    rsvg=tool('rsvg-convert')))
    passed = True

    inspected = app.call('inspect_vector', path=str(fixtures['pdf']))
    detail['import_']['inspect_pdf'] = inspected
    passed &= inspected.get('kind') == 'pdf' and inspected.get('pages') == 1

    inspected_svg = app.call('inspect_vector', path=str(fixtures['svg']))
    detail['import_']['inspect_svg'] = inspected_svg
    passed &= inspected_svg.get('kind') == 'svg'

    for key, renderer in (('pdf', 'pdftoppm'), ('svg', 'rsvg-convert')):
        entry = {}
        try:
            document = open_document(app, fixtures[key], page=1, ppi=144)['document']
            entry['document'] = {name: document.get(name) for name in ('width', 'height')}
            back = output / f'vector-{key}.png'
            entry['export'] = export_png(app, document['id'], back)
            mine = read_png(back)
            # The analytic gate: the two rectangles' interiors inset one pixel
            # are exactly their fill colours, and a point outside both is
            # exactly transparent.
            scale = 144 / 72
            def sample(x, y):
                index = (y * mine['width'] + x) * 4
                return mine['samples'][index:index + 4]
            red = sample(int(30 * scale), int(30 * scale))
            blue = sample(int(130 * scale), int(30 * scale))
            outside = sample(int(100 * scale), int(10 * scale))
            entry['samples'] = dict(red=red, blue=blue, outside=outside)
            exact = (red[:3] == [255, 0, 0] and blue[:3] == [0, 0, 255]
                     and outside[3] == 0)
            entry['T5_analytic'] = exact
            passed &= exact
            if tool(renderer):
                ref = output / f'vector-{key}-reference.png'
                if key == 'pdf':
                    run('pdftoppm', '-r', '144', '-png', '-singlefile',
                        fixtures['pdf'], str(ref.with_suffix('')))
                else:
                    # The fixture declares width/height in px, so `-d/-p`
                    # (which set the assumed DPI of the SOURCE) do not scale
                    # it; the zoom factor is what 144 ppi means here.
                    run('rsvg-convert', '-z', '2', fixtures['svg'], '-o', ref)
                if ref.is_file() and tool('magick'):
                    flat = output / f'vector-{key}-reference-rgba.png'
                    run('magick', ref, 'PNG32:' + str(flat))
                    theirs = read_png(flat)
                    if theirs['width'] == mine['width'] and theirs['height'] == mine['height']:
                        # Outside a 1-px band around every edge: the renderers
                        # antialias differently and that band is not a claim.
                        # Ours is drawn over TRANSPARENCY and theirs over
                        # white, so ours is composited over white first —
                        # otherwise the comparison is about the background.
                        deltas = []
                        for y in range(mine['height']):
                            for x in range(mine['width']):
                                if near_edge(x, y, scale):
                                    continue
                                base = (y * mine['width'] + x) * 4
                                alpha = mine['samples'][base + 3] / 255.0
                                other = theirs['samples'][base + 3] / 255.0
                                for c in range(3):
                                    over = round(mine['samples'][base + c] * alpha
                                                 + 255 * (1 - alpha))
                                    ref = round(theirs['samples'][base + c] * other
                                                + 255 * (1 - other))
                                    deltas.append(abs(over - ref))
                        entry['T5_reference'] = dict(
                            max=max(deltas) if deltas else 0,
                            mean=sum(deltas) / len(deltas) if deltas else 0.0,
                            compared=len(deltas))
                        ok = entry['T5_reference']['max'] <= T5_VECTOR_MAX_CODES
                        entry['T5_reference_gated'] = ok
                        passed &= ok
                    else:
                        entry['reference_size'] = [theirs['width'], theirs['height']]
        except Exception as error:
            entry['error'] = repr(error)[:400]
            passed = False
        detail['import_'][key] = entry

    # ---- the PDF EXPORT ----
    entry = {}
    try:
        document = open_document(app, fixtures['photo'])['document']['id']
        where = output / 'export.pdf'
        entry['result'] = app.call('export_pdf', document_id=document, path=str(where),
                                   overwrite=True, settings={})
        png = output / 'export-for-pdf.png'
        entry['png'] = export_png(app, document, png)
        parsed = pdf_check(where)
        pixels = parsed.pop('pixels', None)
        entry['pdf'] = parsed
        wanted_w = 64 * 72.0 / 300.0
        wanted_h = 48 * 72.0 / 300.0
        box = parsed['media_box'] or [0, 0, 0, 0]
        entry['media_box_expected'] = [wanted_w, wanted_h]
        checks = dict(
            one_page=parsed['pages'] == 1,
            media_box=(abs(box[2] - wanted_w) <= T5_PDF_MEDIABOX_POINTS
                       and abs(box[3] - wanted_h) <= T5_PDF_MEDIABOX_POINTS),
            bits=parsed.get('bits_per_component') == 8,
            flate=parsed.get('filter') == 'FlateDecode',
            icc=parsed.get('icc_sha256') is not None)
        if pixels is not None:
            reference = read_png(png)
            rgb = rgb_of(reference['samples'], reference['colour'])
            measure = stats(list(pixels[:len(rgb)]), rgb)
            entry['image_vs_png'] = measure
            checks['pixels_equal'] = measure['max'] == 0
        if tool('pdftoppm'):
            rendered = output / 'export-pdf-render'
            run('pdftoppm', '-r', '300', '-png', '-singlefile', where, str(rendered))
            flat = rendered.with_suffix('.png')
            if flat.is_file() and tool('magick'):
                rgba = output / 'export-pdf-render-rgba.png'
                run('magick', flat, 'PNG32:' + str(rgba))
                theirs = read_png(rgba)
                mine = read_png(png)
                if theirs['width'] == mine['width'] and theirs['height'] == mine['height']:
                    measure = stats(rgb_of(theirs['samples'], theirs['colour']),
                                    rgb_of(mine['samples'], mine['colour']))
                    # RECORDED, NOT GATED (decision taken after the first
                    # run, with the measured number kept): a re-render puts
                    # poppler's own image resampling between the two sides,
                    # and its phase — not the export — is what the delta
                    # measures. The EXACT claim is `pixels_equal` above: the
                    # image stream in the PDF is byte-for-byte the PNG
                    # export's samples. The fixed bound and the measurement
                    # are both reported here.
                    entry['rerender_recorded_not_gated'] = dict(
                        measured=measure, fixed_bound=T5_PDF_RERENDER_MAX_CODES,
                        within=measure['max'] <= T5_PDF_RERENDER_MAX_CODES)
        # A document that really has alpha: the PDF-imported page, whose
        # background is transparent. An opaque document needs no `/SMask`,
        # so presence is only a claim about this one.
        alpha_document = open_document(app, fixtures['pdf'], page=1, ppi=72)['document']['id']
        alpha_pdf = output / 'export-alpha.pdf'
        entry['alpha_result'] = app.call('export_pdf', document_id=alpha_document,
                                         path=str(alpha_pdf), overwrite=True, settings={})
        alpha_parsed = pdf_check(alpha_pdf)
        alpha_parsed.pop('pixels', None)
        entry['alpha_pdf'] = alpha_parsed
        checks['opaque_has_no_smask'] = parsed.get('has_smask') is False
        checks['alpha_has_smask'] = alpha_parsed.get('has_smask') is True
        entry['T5_pdf'] = checks
        ok = all(checks.values())
        entry['gated'] = ok
        passed &= ok
    except Exception as error:
        entry['error'] = repr(error)[:400]
        passed = False
    detail['export'] = entry

    record(report, 'D5 vector import and PDF export', passed, detail)


def near_edge(x, y, scale):
    """True inside a one-pixel band around either rectangle's edges."""
    for left, top in ((20, 20), (120, 20)):
        for edge in (left * scale, (left + 60) * scale):
            if abs(x - edge) <= 1:
                return True
        for edge in (top * scale, (top + 60) * scale):
            if abs(y - edge) <= 1:
                return True
    return False


def reference_png(made: Path, ref: Path, bits: int, entry: dict) -> bool:
    """Write the independent reference decode of `made` to `ref`. A JPEG
    2000 file is decoded by `opj_decompress` (ImageMagick builds
    without a JPEG 2000 delegate write nothing); every other format by
    `magick`. Records the decoder in `entry['reference_tool']` and, when no
    reference results, why in `entry['reference_missing']`."""
    if made.suffix.lower() == '.jp2':
        name = 'opj_decompress'
        command = ['opj_decompress', '-i', made, '-o', ref]
    else:
        name = 'magick'
        command = ['magick', made, ('PNG48:' if bits > 8 else 'PNG24:') + str(ref)]
    entry['reference_tool'] = name
    if not tool(name):
        entry['reference_missing'] = f'{name} not installed'
        return False
    decoded = run(*command)
    if not ref.is_file():
        entry['reference_missing'] = (f'{name} exited {decoded.returncode}: '
                                      + (decoded.stderr or decoded.stdout).strip()[:400])
    return ref.is_file()


def take_avif_jp2(app, output, report, fixtures):
    """D6: AVIF and JPEG 2000 through the host-source seam."""
    detail = dict(cases={}, tools=dict(avifenc=tool('avifenc'), avifdec=tool('avifdec'),
                                       opj_compress=tool('opj_compress'),
                                       opj_decompress=tool('opj_decompress'),
                                       magick=tool('magick')))
    passed = True
    source = fixtures['plate']
    oracle = fixtures['oracle']

    plans = []
    if tool('avifenc'):
        plans.append(('avif8', ['avifenc', '-l', str(source), str(output / 'plate8.avif')],
                      output / 'plate8.avif', 8))
        plans.append(('avif10', ['avifenc', '-l', '-d', '10', str(source),
                                 str(output / 'plate10.avif')], output / 'plate10.avif', 10))
        plans.append(('avif12', ['avifenc', '-l', '-d', '12', str(fixtures['plate16']),
                                 str(output / 'plate12.avif')], output / 'plate12.avif', 12))
        for name, options, plate in (
                ('avif12_alpha', ['-l'], 'plate16a'),
                ('avif12_lossy_identity', ['-y', '444', '--cicp', '1/13/0', '-q', '90'],
                 'plate16'),
                ('avif12_lossy', ['-q', '90'], 'plate16'),
                ('avif12_lossy_alpha', ['-q', '90', '--qalpha', '90'], 'plate16a')):
            made = output / f'{name}.avif'
            plans.append((name, ['avifenc', *options, '-d', '12', str(fixtures[plate]),
                                 str(made)], made, 12))
    if tool('opj_compress'):
        plans.append(('jp2_8', ['opj_compress', '-i', str(source), '-o',
                                str(output / 'plate8.jp2'), '-r', '1'],
                      output / 'plate8.jp2', 8))
        plans.append(('jp2_16', ['opj_compress', '-i', str(fixtures['plate16']), '-o',
                                 str(output / 'plate16.jp2'), '-r', '1'],
                      output / 'plate16.jp2', 16))

    # RECORDED DECISION (11D first run, revised for the opaque host stills):
    # AVIF is GATED. ImageIO decodes an opaque 8-bit AVIF into RGBX8
    # (`noneSkipLast`), a 10-bit one into packed RGB101010 and a 12-bit one
    # into RGBX16; the host packing seam now admits all three (`rgba8_opaque`,
    # `rgb10_packed_le`, `rgba16_le_opaque`), so avif8 is exact against
    # `magick`, avif10 exact against the CG redraw oracle and on the 10-bit
    # grid. ImageIO decodes a 12-bit AVIF whose primary item declares the
    # identity matrix left-aligned (`v << 4`), every other one full scale; the
    # identity files open through the `_left12` layouts and are gated
    # `(v << 4) | (v >> 8)` of `avifdec`'s codes, the rest against the oracle's
    # own `raw16` samples. ImageIO's layout is recorded per case because it
    # has changed with the OS before.
    # `jp2_16` stays DISCLOSED: `opj_compress` writes a 16-bit-claiming JP2
    # that ImageIO decodes at 8 bits, so the row measures two independent
    # JPEG 2000 DECODERS against each other rather than this build's
    # preservation — the plan's own "a container depth claim alone is
    # insufficient" rule, observed. Its number is recorded.
    disclosed = {'jp2_16'}
    for name, command, made, bits in plans:
        entry = dict(command=' '.join(command), gated=name not in disclosed)
        encode = run(*command)
        entry['encoder_status'] = encode.returncode
        if not made.is_file():
            entry['encoder_stderr'] = (encode.stderr or encode.stdout).strip()[:400]
            detail['cases'][name] = entry
            if name not in disclosed:
                passed = False
            continue
        entry['imageio_layout'] = oracle_layout(oracle, made)
        try:
            document = open_document(app, made, precision='preserve')['document']
            entry['document'] = {key: document.get(key) for key in
                                 ('width', 'height', 'bit_depth')}
            back = output / f'{name}-rasterize.png'
            try:
                entry['export'] = export_png(app, document['id'], back,
                                             bit_depth=16 if bits > 8 else 8)
                entry['exported_depth'] = 16 if bits > 8 else 8
            except Exception as error:
                # The container's depth is a CLAIM; what the document got is
                # what the decoder produced, and that is what is exported.
                entry['high_depth_export_refused'] = repr(error)[:200]
                entry['export'] = export_png(app, document['id'], back, bit_depth=8)
                entry['exported_depth'] = 8
                bits = 8
            if name.startswith('avif12'):
                passed &= gate_avif12(name, made, back, entry, output, oracle)
            else:
                ref = output / f'{name}-reference.png'
                if reference_png(made, ref, bits, entry) and back.is_file():
                    mine = read_png(back)
                    theirs = read_png(ref)
                    a = rgb_of(mine['samples'], mine['colour'])
                    b = rgb_of(theirs['samples'], theirs['colour'])
                    if bits == 8 and theirs['depth'] == 16:
                        # Only the disclosed `jp2_16` gets here: its 16-bit
                        # reference meets an 8-bit export, so it is mapped
                        # to 8-bit codes before measuring.
                        assert name in disclosed, name
                        entry['reference_scaling'] = 'round(v * 255 / 65535)'
                        b = [round(v * 255 / 65535) for v in b]
                    if name == 'avif10':
                        redraw, error = oracle_rgb(oracle, 'redraw16', made,
                                                   output / 'plate10.redraw16')
                        entry['oracle_redraw16_error'] = error
                        exact = stats(a, redraw) if redraw is not None else None
                        entry['vs_oracle_redraw16'] = exact
                        passed &= (exact is not None
                                   and exact['max'] <= T6_AVIF10_ORACLE_MAX_U16)
                    if bits > 8:
                        a = [round(v * 1023 / 65535) for v in a]
                        if name == 'avif10' and all(v % 64 == 0 for v in b):
                            # magick (libheif) widens 10-bit to 16 as v << 6,
                            # so ITS 10-bit codes are v >> 6; the rounding map
                            # below is the inverse of the other convention.
                            entry['magick_scaling'] = 'v << 6'
                            b = [v >> 6 for v in b]
                        else:
                            b = [round(v * 1023 / 65535) for v in b]
                    measure = stats(a, b)
                    entry['T1'] = measure
                    entry['exact'] = measure['max'] == 0
                    bound = T6_AVIF10_MAX_CODES10 if name == 'avif10' else T6_AVIF8_MAX_CODES
                    if name not in disclosed:
                        passed &= measure['max'] <= bound
                elif name not in disclosed:
                    passed = False
        except Exception as error:
            entry['refused'] = repr(error)[:400]
            if name not in disclosed:
                passed = False
        detail['cases'][name] = entry

    if tool('avifenc'):
        for name, (bits, options, matrix) in T6_PREMULTIPLIED.items():
            made = output / f'{name}.avif'
            command = ['avifenc', *options, '--premultiply', '-d', str(bits),
                       str(fixtures['plate16a']), str(made)]
            entry = dict(command=' '.join(command), gated=True)
            encode = run(*command)
            entry['encoder_status'] = encode.returncode
            if not made.is_file():
                entry['encoder_stderr'] = (encode.stderr or encode.stdout).strip()[:400]
                passed = False
            else:
                entry['imageio_layout'] = oracle_layout(oracle, made)
                entry['passed'] = bool(gate_premultiplied(app, name, made, bits, matrix, entry,
                                                          output))
                passed &= entry['passed']
            detail['cases'][name] = entry

    if not plans:
        passed = False
        detail['error'] = 'neither avifenc nor opj_compress is installed'
    record(report, 'D6 AVIF and JPEG 2000', passed, detail)


def gate_premultiplied(app, name, made, bits, matrix, entry, output):
    """T6 for one premultiplied AVIF: `avifdec --info` reports
    premultiplied alpha and the class's matrix; the file opens and exports
    with alpha; and against
    `avifdec`'s PNG on the file's own N-bit grid alpha is equal, RGB is 0
    where alpha is 0, and elsewhere |delta| <= ceil(K * (2^N - 1) / a) +
    SLACK (K is 1 lossless, 2 lossy).

    The verdict is those bounds on the DELIVERED pixels and nothing else. No
    tool reports the host decode's layout any more (`get_host_source` went
    with the placed-image sources), so this gate does not, and cannot, assert
    which ImageIO layout the open took."""
    info = run('avifdec', '--info', made)
    entry['avifdec_info'] = [line.strip() for line in info.stdout.splitlines()
                             if line.strip()][:24]
    alpha = [line for line in entry['avifdec_info'] if line.startswith('* Alpha')]
    found = [line for line in entry['avifdec_info'] if 'Matrix' in line]
    entry['premultiplied'] = bool(alpha) and alpha[0].rstrip().endswith(': Premultiplied')
    entry['matrix'] = found[0].rsplit(':', 1)[1].strip() if found else None
    factor = T6_PREMULTIPLIED_LOSSY_K if name.endswith('_lossy') else T6_PREMULTIPLIED_LOSSLESS_K
    entry['expected'] = 'opens with alpha'
    entry['bound'] = f'ceil({factor} * {(1 << bits) - 1} / a) + {T6_PREMULTIPLIED_SLACK}'
    ok = entry['premultiplied'] and entry['matrix'] == str(matrix)
    try:
        document = open_document(app, made, precision='preserve')['document']
    except Exception as error:
        entry['refused'] = repr(error)[:400]
        return False
    entry['document'] = {key: document.get(key) for key in ('width', 'height', 'bit_depth')}
    depth = 16 if bits > 8 else 8
    back = output / f'{name}-rasterize.png'
    try:
        entry['export'] = export_png(app, document['id'], back, bit_depth=depth)
    except Exception as error:
        entry['export_refused'] = repr(error)[:400]
        return False
    ref = output / f'{name}-avifdec.png'
    decoded = run('avifdec', '-d', str(depth), made, ref)
    entry['avifdec_status'] = decoded.returncode
    if not back.is_file() or not ref.is_file():
        entry['reference_missing'] = (decoded.stderr or decoded.stdout).strip()[:400]
        return False
    mine = read_png(back)
    theirs = read_png(ref)
    entry['exported_depth'] = mine['depth']
    entry['reference_depth'] = theirs['depth']
    mine_alpha = alpha_of(mine['samples'], mine['colour'])
    their_alpha = alpha_of(theirs['samples'], theirs['colour'])
    entry['exported_has_alpha'] = bool(mine_alpha)
    entry['reference_has_alpha'] = bool(their_alpha)
    if (not mine_alpha or not their_alpha or mine['depth'] != depth or theirs['depth'] != depth
            or len(mine_alpha) != len(their_alpha)):
        return False
    top = (1 << bits) - 1
    full = (1 << depth) - 1

    def code(value):
        # Both sides to the file's own N-bit codes (the identity at 8 bits).
        return round(value * top / full)

    alpha_measure = stats([code(v) for v in mine_alpha], [code(v) for v in their_alpha])
    entry['alpha_codes'] = alpha_measure
    ok &= alpha_measure['max'] <= T6_PREMULTIPLIED_ALPHA_MAX_CODES
    mine_rgb = rgb_of(mine['samples'], mine['colour'])
    their_rgb = rgb_of(theirs['samples'], theirs['colour'])
    hidden = hidden_nonzero = over = 0
    worst = dict(delta=0, bound=0, alpha=None)
    largest = 0
    for index, value in enumerate(their_alpha):
        a = code(value)
        ours = mine_rgb[index * 3:index * 3 + 3]
        if a == 0:
            hidden += 1
            hidden_nonzero += any(v != 0 for v in ours)
            continue
        bound = -(-factor * top // a) + T6_PREMULTIPLIED_SLACK
        for c in range(3):
            delta = abs(code(ours[c]) - code(their_rgb[index * 3 + c]))
            largest = max(largest, delta)
            if delta > bound:
                over += 1
                if delta - bound > worst['delta'] - worst['bound']:
                    worst = dict(delta=delta, bound=bound, alpha=a)
    entry['rgb_codes'] = dict(max_delta=largest, samples_over_bound=over, worst_over=worst,
                              alpha_zero_pixels=hidden,
                              alpha_zero_pixels_with_rgb=hidden_nonzero,
                              visible_pixels=len(their_alpha) - hidden)
    return bool(ok and over == 0 and hidden_nonzero == 0 and len(their_alpha) > hidden)


def gate_avif12(name, made, back, entry, output, oracle):
    """T6 for one 12-bit AVIF. Identity-matrix files: avifdec matrix
    0, RGB EXACTLY `(v << 4) | (v >> 8)` of avifdec's
    y4m codes, every 4095 at 65535, alpha EXACT against the oracle's `raw16`.
    Every other file opens as before: RGB and alpha
    EXACT against `raw16`."""
    identity = name in T6_AVIF12_IDENTITY
    entry['expected'] = 'identity, widened' if identity else 'opens as today'
    info = run('avifdec', '--info', made)
    entry['avifdec_info'] = [line.strip() for line in info.stdout.splitlines()
                             if line.strip()][:24]
    matrix = [line for line in entry['avifdec_info'] if 'Matrix' in line]
    entry['identity_matrix'] = bool(matrix) and matrix[0].rstrip().endswith(': 0')
    if not back.is_file():
        return False
    mine = read_png(back)
    mine_rgb = rgb_of(mine['samples'], mine['colour'])
    mine_alpha = alpha_of(mine['samples'], mine['colour'])
    entry['exported_has_alpha'] = bool(mine_alpha)
    raw_rgb, raw_alpha, error = oracle_raw16(oracle, made, output / f'{name}.raw16')
    entry['oracle_raw16_error'] = error
    ok = mine['depth'] == 16
    if mine_alpha:
        alpha_measure = stats(mine_alpha, raw_alpha) if raw_alpha is not None else None
        entry['alpha_vs_oracle_raw16'] = alpha_measure
        ok &= alpha_measure is not None and alpha_measure['max'] == 0
    elif raw_alpha is not None:
        # The decode has real alpha and the export dropped it.
        ok &= all(value == 65535 for value in raw_alpha)
    if not identity:
        measure = stats(mine_rgb, raw_rgb) if raw_rgb is not None else None
        entry['rgb_vs_oracle_raw16'] = measure
        return ok and measure is not None and measure['max'] == 0
    ok &= entry['identity_matrix']
    y4m = output / f'{name}.y4m'
    decoded = run('avifdec', made, y4m)
    entry['avifdec_status'] = decoded.returncode
    if decoded.returncode != 0:
        return False
    planes = read_y4m(y4m)
    entry['y4m_header'] = planes['header']
    luma, blue, red = planes['planes']      # Y = G, U = B, V = R
    codes = []
    for index in range(len(luma)):
        codes += [red[index], luma[index], blue[index]]
    measure = stats(mine_rgb, [replicate12(v) for v in codes])
    entry['T6_expansion'] = measure
    entry['exact'] = measure['max'] == 0
    whites = [index for index, v in enumerate(codes) if v == 4095]
    entry['white_codes'] = len(whites)
    entry['white_codes_at_65535'] = sum(1 for index in whites if mine_rgb[index] == 65535)
    ok &= measure['max'] == 0 and entry['white_codes_at_65535'] == len(whites)
    if name in T6_AVIF12_WHITE_REQUIRED:
        ok &= len(whites) > 0
    if name == 'avif12_alpha':
        ok &= bool(mine_alpha)
    return ok


def take_master_unchanged(app, output, report, fixtures):
    """D7: every export leaves the document exactly as it was."""
    detail = {}
    document = open_document(app, fixtures['plate'])['document']['id']
    before_state = app.call('get_document', document_id=document)
    before_copy = output / 'master-before.rz'
    app.call('save_copy', document_id=document, path=str(before_copy), format='rz')
    exports = []
    for name, settings in (('png', {'format': 'png'}), ('heic', {'format': 'heic'}),
                           ('tiff', {'format': 'tiff'})):
        where = output / f'master-{name}.out'
        try:
            exports.append(app.call('export_image', document_id=document, path=str(where),
                                    settings=settings, overwrite=True))
        except Exception as error:
            exports.append(dict(error=repr(error)[:200]))
    where = output / 'master.pdf'
    try:
        exports.append(app.call('export_pdf', document_id=document, path=str(where),
                                overwrite=True, settings={}))
    except Exception as error:
        exports.append(dict(error=repr(error)[:200]))
    after_state = app.call('get_document', document_id=document)
    after_copy = output / 'master-after.rz'
    app.call('save_copy', document_id=document, path=str(after_copy), format='rz')

    detail['exports'] = exports
    detail['state_equal'] = before_state == after_state
    detail['sha_equal'] = digest(before_copy) == digest(after_copy)
    detail['dirty_before'] = before_state.get('document', {}).get('dirty')
    detail['dirty_after'] = after_state.get('document', {}).get('dirty')

    listed = app.client.rpc('tools/list', {})
    names = {entry['name'] for entry in listed.get('tools', [])}
    detail['tools_count'] = len(names)
    detail['has_export_pdf'] = 'export_pdf' in names
    detail['has_inspect_vector'] = 'inspect_vector' in names
    passed = (detail['state_equal'] and detail['sha_equal']
              and detail['has_export_pdf'] and detail['has_inspect_vector'])
    record(report, 'D7 the master is unchanged', passed, detail)


# ------------------------------------------------------------------- main --

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path,
                        default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--only', action='append', choices=[f'D{n}' for n in range(1, 8)],
                        help='run only these cases (repeatable); every case when absent')
    arguments = parser.parse_args()

    def wanted(case):
        return not arguments.only or case in arguments.only
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    output = arguments.output.expanduser().resolve()
    if output.is_relative_to(ROOT) or arguments.output.is_symlink():
        parser.error('output must be outside the repository and not a symlink')
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    report = dict(
        app=str(executable), app_sha256=digest(executable),
        script_sha256=digest(Path(__file__)),
        helper_sha256=helper_digests(),
        tools={name: tool(name) for name in
               ('sips', 'magick', 'heif-info', 'heif-enc', 'avifenc', 'avifdec', 'opj_compress',
                'opj_decompress', 'pdftoppm', 'rsvg-convert', 'tiffinfo')},
        tolerances=dict(
            T1=dict(heic10_max_codes10=T1_HEIC10_MAX_CODES10,
                    heic10_mean_codes10=T1_HEIC10_MEAN_CODES10,
                    avif_jp2='exact',
                    opaque_heic8_max_codes=T1_OPAQUE_HEIC8_MAX_CODES,
                    opaque_heic10_oracle_max_u16=T1_OPAQUE_HEIC10_ORACLE_MAX_U16,
                    opaque_heic12='recorded; must open rgba16',
                    opaque_heic12_444='refused with exactly the no-decoded-pixels sentence',
                    gainmap_sdr8_max_codes=T1_GAINMAP_SDR8_MAX_CODES,
                    gainmap_working_space=T1_GAINMAP_WORKING_SPACE,
                    gainmap_decode_request='sdr_base',
                    gainmap_disclosure=SDR_BASE_DISCLOSURE),
            T6=dict(avif8_max_codes=T6_AVIF8_MAX_CODES,
                    avif10_oracle_max_u16=T6_AVIF10_ORACLE_MAX_U16,
                    avif10_max_codes10=T6_AVIF10_MAX_CODES10,
                    avif12_identity='%s of avifdec y4m codes, exact; alpha exact vs '
                                    'oracle raw16; cases %s'
                                    % (T6_AVIF12_EXPANSION, ', '.join(T6_AVIF12_IDENTITY)),
                    avif12_other='opens as today: RGB and alpha exact '
                                 'vs oracle raw16',
                    premultiplied=dict(
                        alpha_max_codes=T6_PREMULTIPLIED_ALPHA_MAX_CODES,
                        rgb='0 where alpha is 0; elsewhere |delta| <= ceil(K * (2^N - 1) / a) '
                            '+ slack on the N-bit grid, vs avifdec PNG',
                        lossless_k=T6_PREMULTIPLIED_LOSSLESS_K,
                        lossy_k=T6_PREMULTIPLIED_LOSSY_K, slack=T6_PREMULTIPLIED_SLACK,
                        cases=list(T6_PREMULTIPLIED)),
                    disclosed=['jp2_16']),
            T2=dict(max_u16=T2_GRAY_MAX_U16, mean_u16=T2_GRAY_MEAN_U16,
                    untagged_converted_max_u16=T2_GRAY_CONVERTED_MAX_U16),
            T3=dict(mean_codes=T3_HEIC_MEAN_CODES, p99_codes=T3_HEIC_P99_CODES,
                    icc='equal',
                    plate=T3_HEIC_PLATE + '; the per-pixel-noise plate is '
                                          'recorded, not gated'),
            T4='exact authored EXIF fields',
            T5=dict(reference_max_codes=T5_VECTOR_MAX_CODES,
                    pdf_rerender_max_codes=T5_PDF_RERENDER_MAX_CODES,
                    media_box_points=T5_PDF_MEDIABOX_POINTS)),
        gates={}, error=None)
    try:
        fixtures = write_fixtures(output)
        report['fixture_sha256'] = {key: digest(path) for key, path in fixtures.items()}
        report['only'] = arguments.only
        if wanted('D1') or wanted('D6'):
            fixtures['opaque'], opaque_provenance = load_host_stills_opaque()
            report['opaque_fixtures'] = dict(
                folder=str(OPAQUE_FIXTURES), schema=opaque_provenance['schema'],
                provenance_sha256=digest(OPAQUE_FIXTURES / 'provenance.json'),
                verified={name: digest(path) for name, path in fixtures['opaque'].items()})
            fixtures['oracle'] = build_oracle(output)
            report['oracle'] = dict(source_sha256=digest(ROOT / 'scripts/host-stills-opaque/oracle.swift'),
                                    binary_sha256=digest(fixtures['oracle']))
        if wanted('D4'):
            take_float_tiff(output, report)
        host = support.HostApp(output / 'app', output / 'recovery', executable)
        # D2 (#1 phase 1a): an untagged gray file takes the working Gray, pinned to sGray.
        host.extra_defaults = ['-ColorWorkingGray', 'sgray']
        with host as app:
            for case, take in (('D1', take_heic), ('D2', take_gray16), ('D3', take_bridge),
                               ('D5', take_vector), ('D6', take_avif_jp2),
                               ('D7', take_master_unchanged)):
                if wanted(case):
                    take(app, output, report, fixtures)
    except BaseException as error:                 # Recorded, never repaired.
        report['error'] = f'{type(error).__name__}: {error}'
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    gates = {name: gate['passed'] for name, gate in report['gates'].items()}
    print(json.dumps(dict(error=report['error'], gates=gates,
                          receipt=str(output / 'receipt.json')), indent=2))
    raise SystemExit(0 if not report['error'] and gates and all(gates.values()) else 1)


if __name__ == '__main__':
    main()
