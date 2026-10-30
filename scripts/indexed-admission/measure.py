#!/usr/bin/env python3
"""The #1 Indexed Color phase-1a, 1b, 1c, 2b, 2c, 2d and 3a admission drive, on a built Release app.

Every new Indexed plan is held to the measured-admission rule: plan >=
ceil(1.25 x max E), where E is `ri_interval_max_phys_footprint` - pre around
ONE MCP call, read once the footprint has been still for 2 s (so the result's
display projection is inside the interval). The gates are FIXED here before the
drive runs; a miss is recorded in `receipt.json` and fails, and the answer to a
miss is the plan's own new per-pixel term (`indexed_io::OPEN_BYTES_PER_PIXEL`
for the opens, `indexed_tiff::open_plan`'s samples term for the palette TIFF,
`indexed_convert::TO_INDEXED_BYTES_PER_PIXEL` and the Adaptive term for the
conversions and the colour table edit, `indexed_export::EXPORT_BYTES_PER_PIXEL` for
the kept exports, an Indexed-arm term in the host's `scratch` for the direct CMYK and
Lab conversions, `indexed_paint::INDEXED_PAINT_BYTES_PER_PIXEL` for the Brush,
`indexed_geometry::GEOMETRY_BYTES_PER_PIXEL` for the geometry), never
this bound and never a shared constant.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage` instead of the measured call), `--runs` times. The harness —
`call`, `footprint`, `settle`, `owned` — is `scripts/cmyk-admission/measure.py`'s
and the gray PNG writer `scripts/gray-admission/measure.py`'s, each loaded from
its file. Fixtures are 6000x4000 (24 MP), written HERE: the 16-bit RGB PNG from
`scripts/host-still-admission/author.swift`, the gray drive's 8-bit gray PNG,
an 8-bit palette PNG (and its twin whose `tRNS` makes index 0 transparent) and an
uncompressed 8-bit palette TIFF of 256 entries whose indices vary per pixel and row.

The plans:

- `open-indexed-png`: `open_document` of the palette PNG; plan =
  `indexed_io::open_scratch` as the core computes it: pixels x
  `OPEN_BYTES_PER_PIXEL` + the file's bytes + `BOOKKEEPING_BYTES` (both
  constants read from the source); the reply's `color_mode` indexed.
- `to-indexed-8`: the 16-bit RGB PNG opened `rgba8`; `convert_mode indexed`
  (Uniform, Diffusion, the sheet's defaults; promoted, then converted); plan =
  the reply's `working_memory_plan_bytes`.
- `to-indexed-gray8`: the 8-bit gray PNG (an 8-bit Grayscale document);
  `convert_mode indexed` (the same options); the same.
- `to-rgb-from-indexed`: the `to-indexed-8` document converted in the setup;
  `convert_mode rgb` (the RGB document its storage is, narrowed to 8 bits);
  the same.
- `to-indexed-adaptive` (phase 1b): the `to-indexed-8` source; `convert_mode
  indexed palette adaptive` (Diffusion); plan = the reply's
  `working_memory_plan_bytes` (the histogram and 8 B a distinct colour beside
  the conversion's own terms).
- `open-indexed-tiff` (phase 1b): `open_document` of the palette TIFF; plan =
  `indexed_tiff::open_plan`: `open_scratch` (as `open-indexed-png`) + pixels x
  1 sample; the reply's `color_mode` indexed.
- `export-indexed-png`, `export-indexed-gif`, `export-indexed-tiff` (phase
  1b): the palette PNG opened as an Indexed document in the setup;
  `export_image` of it (profile document, 8 bits: the kept-Indexed export);
  plan = the reply's `working_memory_plan_bytes` (the core's `scratch_bytes`:
  pixels x `EXPORT_BYTES_PER_PIXEL` + bookkeeping; no codec term).
- `export-indexed-png-alpha`, `export-indexed-gif-alpha`,
  `export-indexed-tiff-alpha` (phase 1b): the same over the palette PNG with
  a `tRNS` making index 0 transparent, so the TIFF writes the interleaved
  index and alpha samples (2 B/px, the heaviest kept export); the same plan.
- `set-color-table` (phase 1c): the palette PNG opened as an Indexed document,
  its table read in the setup; `set_color_table` of the same table with entry 0
  inverted (every pixel re-rendered); plan = the reply's
  `working_memory_plan_bytes` (pixels x `TO_INDEXED_BYTES_PER_PIXEL` +
  bookkeeping).
- `to-cmyk-from-indexed`, `to-lab-from-indexed` (phase 1c): the `to-indexed-8`
  document converted in the setup; `convert_mode cmyk` / `lab` (the palette
  expanded, every sample shared, then the RGB route at 8 bits); plan = the
  reply's `working_memory_plan_bytes`.
- `brush-indexed` (phase 2b): the `to-indexed-8` document converted in the
  setup; `brush_stroke` layer 0 along (100, 100)–(5900, 3900), size 200 (the
  native paint on the RGB storage, then the re-index and re-render); plan =
  the reply's `working_memory_plan_bytes` (the stroke job's admitted plan: the
  footprint's and the edit's, whose scratch adds pixels x
  `INDEXED_PAINT_BYTES_PER_PIXEL`, the 32 MiB nearest-colour memo and
  bookkeeping to the native paint's).
- `rotate90-indexed` (phase 2c): the `to-indexed-8` document converted in the
  setup; `rotate` degrees 90 (the exact path: the index plane's carrier turned
  and read back); plan = the reply's `working_memory_plan_bytes` (the core's
  `scratch_bytes`: the RGB geometry plan plus (old + new canvas) x
  `indexed_geometry::GEOMETRY_BYTES_PER_PIXEL`, the memo and bookkeeping).
- `rotate-indexed` (phase 2c): the palette PNG whose index 0 is transparent,
  opened as an Indexed document; `rotate` degrees 5 sampler bicubic (the
  resample path: the RGB rotation, then every pixel re-indexed to its nearest
  entry); the same plan. The paint family and Cut (phase 2c) add the
  `brush-indexed` re-index term beside the inner op's own admitted plan, so
  they take no case of their own.
- `transform-nearest-indexed` (phase 2d): the palette PNG whose index 0 is
  transparent; `transform_layers` layers [0], matrix translate (10, 7),
  sampler nearest (the exact path: the carrier moved and read back); plan =
  the reply's `working_memory_plan_bytes` (the core's `scratch_bytes`: the RGB
  `affine_set` plan plus `indexed_geometry::transform_scratch`).
- `transform-indexed` (phase 2d): the same document; `transform_layers`
  rotate 5° about the centre, sampler bicubic (the resample path: the RGB
  transform, then the canvas re-indexed against the old plane); the same plan.
  A miss raises `GEOMETRY_BYTES_PER_PIXEL`.
- `paste-indexed` (phase 2d): the `to-indexed-8` document converted in the
  setup, beside a second RGB document of the same pixels whose Select All and
  Copy the setup makes on the case's own private pasteboard
  (`DR_PASTEBOARD_NAME`); `paste` placement centered (the RGB paste, Merge
  Down and the re-index); plan = the reply's `working_memory_plan_bytes` (the
  core's `indexed_clipboard::paste_plan` scratch). A miss raises
  `indexed_clipboard::PASTE_MERGE_BYTES_PER_PIXEL`. The selected pixels'
  transform and Warp add the measured `transform_scratch` term beside their
  kernels' own plans, so they take no case of their own.
- `convert-profile-indexed` (phase 3a): the `to-indexed-8` document converted
  in the setup; `convert_profile` profile display_p3 (the colour table
  converted through the RGB route, the storage assigned, the one layer
  re-rendered from the converted table); plan = the reply's
  `working_memory_plan_bytes` (the core's conversion plan `scratch_bytes`:
  the storage's assign plan plus the Color Table edit's re-render, pixels x
  `TO_INDEXED_BYTES_PER_PIXEL` + bookkeeping). A miss raises
  `TO_INDEXED_BYTES_PER_PIXEL` and re-measures every case that uses it. The
  Move tool's moves (`move_layers`, `move_selected_pixels`) reuse
  `transform-nearest-indexed`'s measured exact path, so they take no case of
  their own.
- `open-indexed-psd` (#1 Indexed Color PSD): `open_document` psd_mode
  composite of a 24 MP Indexed Color PSD written here (the palette PNG's table
  and indices, a Raw composite), after `inspect_psd` in the setup of both
  twins; plan = `indexed_io::open_scratch`, as `open-indexed-png`'s; the
  reply's `color_mode` indexed. A miss raises `OPEN_BYTES_PER_PIXEL`.
- `export-indexed-psd` (#1 Indexed Color PSD): the palette PNG whose index 0
  is transparent opened as an Indexed document (so the file sets 1047, the
  transparent index; it holds the empty layer section and the RLE composite
  only, Photoshop's own form); `export_psd` format psd mode
  flattened; plan = the reply's `working_memory_plan_bytes` (the core's
  `indexed_psd_worst_case` + pixels x `EXPORT_BYTES_PER_PIXEL` + bookkeeping);
  the reply's `bit_depth` 8. A miss raises `EXPORT_BYTES_PER_PIXEL`.
"""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gray = load('gray_measure', 'scripts/gray-admission/measure.py')
cmyk = gray.cmyk

LIMIT = 8589934592
SIZE = (6000, 4000)
OPEN_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const OPEN_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/indexed_io.rs').read_text()).group(1))
TO_INDEXED_BYTES_PER_PIXEL = int(re.search(
    r'pub\(crate\) const TO_INDEXED_BYTES_PER_PIXEL: u64 = (\d+);',
    (ROOT / 'core/src/indexed_convert.rs').read_text()).group(1))
BOOKKEEPING_BYTES = math.prod(int(v) for v in re.search(
    r'pub\(crate\) const BOOKKEEPING_BYTES: u64 = ([\d *]+);',
    (ROOT / 'core/src/precision_image.rs').read_text()).group(1).split('*'))

# name: (the setup's fixture, the measured call)
CASES = {
    'open-indexed-png': ('none', 'open-indexed'),
    'to-indexed-8': ('png8', 'to-indexed'),
    'to-indexed-gray8': ('gpng8', 'to-indexed'),
    'to-rgb-from-indexed': ('indexed8', 'to-rgb'),
    'to-indexed-adaptive': ('png8', 'to-indexed-adaptive'),
    'open-indexed-tiff': ('none', 'open-indexed-tiff'),
    'export-indexed-png': ('ipng8', 'export-png'),
    'export-indexed-gif': ('ipng8', 'export-gif'),
    'export-indexed-tiff': ('ipng8', 'export-tiff'),
    'export-indexed-png-alpha': ('ipng8t', 'export-png'),
    'export-indexed-gif-alpha': ('ipng8t', 'export-gif'),
    'export-indexed-tiff-alpha': ('ipng8t', 'export-tiff'),
    'set-color-table': ('ipng8', 'set-color-table'),
    'to-cmyk-from-indexed': ('indexed8', 'to-cmyk'),
    'to-lab-from-indexed': ('indexed8', 'to-lab'),
    'brush-indexed': ('indexed8', 'brush'),
    'rotate90-indexed': ('indexed8', 'rotate90'),
    'rotate-indexed': ('ipng8t', 'rotate5'),
    'transform-nearest-indexed': ('ipng8t', 'transform-nearest'),
    'transform-indexed': ('ipng8t', 'transform5'),
    'paste-indexed': ('indexed8', 'paste'),
    'convert-profile-indexed': ('indexed8', 'convert-profile'),
    'open-indexed-psd': ('none', 'open-indexed-psd'),
    'export-indexed-psd': ('ipng8t', 'export-psd'),
}

# The measured `convert_mode` calls: the mode each converts to.
MODES = {'to-indexed': 'indexed', 'to-rgb': 'rgb', 'to-cmyk': 'cmyk', 'to-lab': 'lab'}


def palette_png(path, transparent=False):
    """A 6000x4000 8-bit palette PNG of 256 entries, its indices varying per pixel and row;
    with `transparent`, a `tRNS` of one alpha 0 makes index 0 the transparent index."""
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    palette = bytes(v for i in range(256) for v in ((i * 37) % 256, (i * 101) % 256, (255 - i)))
    compressor = zlib.compressobj(6)
    parts = []
    for y in range(SIZE[1]):
        row = bytes(((x * 7) ^ (y * 13) ^ (x * y)) & 255 for x in range(SIZE[0]))
        parts.append(compressor.compress(b'\0' + row))
    parts.append(compressor.flush())
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', *SIZE, 8, 3, 0, 0, 0))
                     + chunk(b'PLTE', palette)
                     + (chunk(b'tRNS', b'\0') if transparent else b'')
                     + chunk(b'IDAT', b''.join(parts))
                     + chunk(b'IEND', b''))


def palette_tiff(path):
    """A 6000x4000 uncompressed little-endian 8-bit palette TIFF, one strip, the
    palette PNG's 256 entries (each code x 257) and indices."""
    width, height = SIZE
    entries = [((i * 37) % 256, (i * 101) % 256, (255 - i)) for i in range(256)]
    color_map = [entries[i][c] * 257 for c in range(3) for i in range(256)]
    tags = 11
    map_at = 8 + 2 + tags * 12 + 4
    data_at = map_at + len(color_map) * 2
    fields = [(256, 4, 1, width), (257, 4, 1, height), (258, 3, 1, 8), (259, 3, 1, 1),
              (262, 3, 1, 3), (273, 4, 1, data_at), (277, 3, 1, 1), (278, 4, 1, height),
              (279, 4, 1, width * height), (284, 3, 1, 1), (320, 3, len(color_map), map_at)]
    with path.open('wb') as out:
        out.write(b'II*\0' + struct.pack('<I', 8) + struct.pack('<H', tags))
        for tag, kind, count, value in fields:
            out.write(struct.pack('<HHII', tag, kind, count, value))
        out.write(struct.pack('<I', 0))
        out.write(struct.pack(f'<{len(color_map)}H', *color_map))
        for y in range(height):
            out.write(bytes(((x * 7) ^ (y * 13) ^ (x * y)) & 255 for x in range(width)))


def palette_psd(path):
    """A 6000x4000 Indexed Color PSD (colour mode 2, Adobe's layout): the palette PNG's 256
    entries as the planar 768-byte table, resource 1046 = 256, an empty layer section and the
    palette PNG's indices as a Raw composite."""
    width, height = SIZE
    entries = [((i * 37) % 256, (i * 101) % 256, (255 - i)) for i in range(256)]
    table = bytes(entries[i][c] for c in range(3) for i in range(256))
    count = b'8BIM' + struct.pack('>HHI', 1046, 0, 2) + struct.pack('>H', 256)
    with path.open('wb') as out:
        out.write(b'8BPS' + struct.pack('>H', 1) + b'\0' * 6
                  + struct.pack('>HIIHH', 1, height, width, 8, 2)
                  + struct.pack('>I', len(table)) + table
                  + struct.pack('>I', len(count)) + count
                  + struct.pack('>I', 0) + struct.pack('>H', 0))
        for y in range(height):
            out.write(bytes(((x * 7) ^ (y * 13) ^ (x * y)) & 255 for x in range(width)))


def fixtures(output):
    folder = output / 'fixtures'
    folder.mkdir(parents=True, exist_ok=True)
    made = {'gpng8': folder / 'gray8.png', 'png16': folder / 'rgb16.png',
            'ipng8': folder / 'palette8.png', 'ipng8t': folder / 'palette8-trns.png',
            'itif8': folder / 'palette8.tif', 'ipsd8': folder / 'palette8.psd'}
    if not made['ipsd8'].is_file():
        palette_psd(made['ipsd8'])
    if not made['gpng8'].is_file():
        gray.gray_png(made['gpng8'], 8)
    if not made['ipng8'].is_file():
        palette_png(made['ipng8'])
    if not made['ipng8t'].is_file():
        palette_png(made['ipng8t'], transparent=True)
    if not made['itif8'].is_file():
        palette_tiff(made['itif8'])
    if not made['png16'].is_file():
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(cmyk.AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        subprocess.run([str(binary), 'png16', str(SIZE[0]), str(SIZE[1]), str(made['png16'])],
                       check=True, capture_output=True, timeout=600)
    # `paste-indexed`'s source: the same pixels under another path, so it opens as a second
    # document beside the converted one.
    made['png16paste'] = folder / 'rgb16-paste.png'
    if not made['png16paste'].is_file():
        made['png16paste'].write_bytes(made['png16'].read_bytes())
    return made


def measure_case(output, executable, made, label, case, baseline):
    """One case in its own app; a paste case on its OWN private pasteboard
    (`DR_PASTEBOARD_NAME`, `scripts/gray-admission/measure.py`'s pattern)."""
    if CASES[case][1] != 'paste':
        return measure_one(output, executable, made, label, case, baseline)
    os.environ['DR_PASTEBOARD_NAME'] = f'com.kgalligan.doctored.indexed-admission.{os.getpid()}.{label}'
    try:
        return measure_one(output, executable, made, label, case, baseline)
    finally:
        os.environ.pop('DR_PASTEBOARD_NAME', None)


def measure_one(output, executable, made, label, case, baseline):
    setup, measured = CASES[case]
    record = dict(case=case, label=label, baseline=baseline)
    pixels = SIZE[0] * SIZE[1]
    with cmyk.owned(output, label, executable) as app:
        document = None
        if setup != 'none':
            source = 'png16' if setup in ('png8', 'indexed8') else setup
            arguments = dict(path=str(made[source]))
            if source == 'png16':
                arguments['precision'] = 'rgba8'
            ok, opened = cmyk.call(app, 'open_document', **arguments)
            assert ok, opened
            document = opened['document']['id']
            if setup == 'ipng8t':
                ok, info = cmyk.call(app, 'get_document', document_id=document)
                assert ok and info.get('indexed', {}).get('transparent_index') == 0, info
            if setup == 'indexed8':
                ok, converted = cmyk.call(app, 'convert_mode', document_id=document, mode='indexed')
                assert ok and converted.get('color_mode') == 'indexed', converted
        if measured == 'paste':
            # The full-canvas RGB copy, made in the setup of both twins: a second RGB
            # document (the same pixels under another path), Select All, Copy.
            ok, source = cmyk.call(app, 'open_document', path=str(made['png16paste']),
                                   precision='rgba8')
            assert ok, source
            source = source['document']['id']
            for tool, extra in (('select_all', {}), ('copy', dict(layer=0))):
                ok, reply = cmyk.call(app, tool, document_id=source, **extra)
                assert ok, (tool, reply)
        # The Indexed PSD's preflight, in the setup of both twins: `open_document` needs its
        # fingerprint.
        fingerprint = None
        if measured == 'open-indexed-psd':
            ok, inspected = cmyk.call(app, 'inspect_psd', path=str(made['ipsd8']))
            assert ok and inspected.get('import_precision') == 'indexed', inspected
            fingerprint = inspected['source_fingerprint']
        table = None
        if measured == 'set-color-table' and not baseline:
            ok, table = cmyk.call(app, 'get_color_table', document_id=document)
            assert ok and len(table['colors']) == 256, table
            table['colors'][0] = [255 - v for v in table['colors'][0]]
        plan = None
        pid = app.process.pid
        cmyk.settle(pid)
        pre, _ = cmyk.footprint(pid, reset=True)
        if baseline:
            ok, reply = cmyk.call(app, 'resource_usage')
        elif measured == 'open-indexed':
            ok, reply = cmyk.call(app, 'open_document', path=str(made['ipng8']))
            plan = pixels * OPEN_BYTES_PER_PIXEL + made['ipng8'].stat().st_size + BOOKKEEPING_BYTES
            if ok and reply['document'].get('color_mode') != 'indexed':
                ok, reply = False, f'opened {reply["document"].get("color_mode")}'
        elif measured == 'open-indexed-tiff':
            ok, reply = cmyk.call(app, 'open_document', path=str(made['itif8']))
            plan = (pixels * OPEN_BYTES_PER_PIXEL + made['itif8'].stat().st_size
                    + BOOKKEEPING_BYTES + pixels)
            if ok and reply['document'].get('color_mode') != 'indexed':
                ok, reply = False, f'opened {reply["document"].get("color_mode")}'
        elif measured == 'open-indexed-psd':
            ok, reply = cmyk.call(app, 'open_document', path=str(made['ipsd8']),
                                  psd_mode='composite', psd_fingerprint=fingerprint)
            plan = pixels * OPEN_BYTES_PER_PIXEL + made['ipsd8'].stat().st_size + BOOKKEEPING_BYTES
            if ok and reply['document'].get('color_mode') != 'indexed':
                ok, reply = False, f'opened {reply["document"].get("color_mode")}'
        elif measured == 'export-psd':
            ok, reply = cmyk.call(app, 'export_psd', document_id=document,
                                  path=str(output / f'{label}.psd'), format='psd',
                                  mode='flattened')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('bit_depth') != 8:
                ok = False
        elif measured.startswith('export-'):
            ext = measured.split('-')[1]
            ok, reply = cmyk.call(app, 'export_image', document_id=document,
                                  path=str(output / f'{label}.{ext}'),
                                  settings={'format': ext, 'profile': 'document', 'bit_depth': 8})
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('bit_depth') != 8:
                ok = False
        elif measured == 'to-indexed-adaptive':
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode='indexed',
                                  palette='adaptive')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != 'indexed':
                ok = False
        elif measured == 'brush':
            ok, reply = cmyk.call(app, 'brush_stroke', document_id=document, layer=0,
                                  points=[[100, 100], [5900, 3900]], size=200)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('changed') is not True:
                ok = False
        elif measured in ('rotate90', 'rotate5'):
            turn = dict(degrees=90) if measured == 'rotate90' else dict(degrees=5,
                                                                         sampler='bicubic')
            ok, reply = cmyk.call(app, 'rotate', document_id=document, **turn)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('document', {}).get('color_mode') != 'indexed':
                ok = False
        elif measured in ('transform-nearest', 'transform5'):
            if measured == 'transform-nearest':
                matrix, sampler = [1, 0, 0, 1, 10, 7], 'nearest'
            else:
                c, s = math.cos(math.radians(5)), math.sin(math.radians(5))
                cx, cy = SIZE[0] / 2, SIZE[1] / 2
                matrix = [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
                sampler = 'bicubic'
            ok, reply = cmyk.call(app, 'transform_layers', document_id=document, layers=[0],
                                  matrix=matrix, sampler=sampler)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('changed') is not True:
                ok = False
        elif measured == 'paste':
            ok, reply = cmyk.call(app, 'paste', document_id=document, placement='centered',
                                  mode='pixels')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('layer') != 0:
                ok = False
        elif measured == 'convert-profile':
            ok, reply = cmyk.call(app, 'convert_profile', document_id=document,
                                  profile='display_p3')
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('changed') is not True:
                ok = False
        elif measured == 'set-color-table':
            ok, reply = cmyk.call(app, 'set_color_table', document_id=document,
                                  colors=table['colors'], transparent_index=table['transparent_index'])
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('changed') is not True:
                ok = False
        else:
            mode = MODES[measured]
            ok, reply = cmyk.call(app, 'convert_mode', document_id=document, mode=mode)
            plan = reply.get('working_memory_plan_bytes') if ok else None
            if ok and reply.get('color_mode') != mode:
                ok = False
        cmyk.settle(pid)
        post, peak = cmyk.footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if not baseline:
            if not ok:
                record['refusal'] = str(reply)[:800]
            record['plan'] = plan
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path,
                        default=Path(os.environ.get('DR_VERIFY_APP',
                                                    'build/Build/Products/Release/Doctored.app')))
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES))
    arguments = parser.parse_args()
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-indexed-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(LIMIT)
    app = arguments.app if arguments.app.is_absolute() else ROOT / arguments.app
    executable = (app / 'Contents/MacOS/Doctored' if app.suffix == '.app' else app).resolve()
    made = fixtures(output)
    report = dict(app=str(executable), app_sha256=cmyk.digest(executable),
                  script_sha256=cmyk.digest(Path(__file__)), runs=arguments.runs,
                  working_limit_bytes=LIMIT, open_bytes_per_pixel=OPEN_BYTES_PER_PIXEL,
                  to_indexed_bytes_per_pixel=TO_INDEXED_BYTES_PER_PIXEL,
                  bookkeeping_bytes=BOOKKEEPING_BYTES,
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size)
                            for name, path in made.items()},
                  records=[])
    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                try:
                    record = measure_case(output, executable, made, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)
    table, passed, counted = {}, True, 0
    for case in selected:
        measured = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(measured) and all(r.get('ok') for r in measured)
        plans = [r.get('plan') for r in measured if r.get('plan') is not None]
        plan = min(plans) if plans and len(plans) == len(measured) else None
        max_e = max((r.get('E', 0) for r in measured), default=0)
        baseline_e = max((r.get('E') or 0 for r in baselines), default=0)
        need = math.ceil(1.25 * max_e)
        skipped = ok and plan is not None and max_e <= baseline_e
        row = dict(E=[r.get('E') for r in measured], baseline_E=[r.get('E') for r in baselines],
                   plans=plans, plan=plan, need=need, ratio=(plan / max_e) if plan and max_e else None,
                   skipped=skipped, passed=ok and plan is not None and plan >= need)
        if not skipped:
            counted += 1
            passed &= row['passed']
        table[case] = row
    if selected and not counted:
        passed = False
    report['table'] = table
    report['passed'] = passed
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        verdict = 'SKIP (no rise)' if row['skipped'] else 'PASS' if row['passed'] else 'FAIL'
        print(f'{case:20} plan {row["plan"]} need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-indexed: {"PASS" if passed else "FAIL"}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if passed else 1)


if __name__ == '__main__':
    main()
