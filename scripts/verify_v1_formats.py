#!/usr/bin/env python3
"""MCP drive for the v1 file formats, against the Debug app.

Every verdict is against something other than the code under test:

- **PSB round trip, 8 and 16 bits.** `export_psd` with `format: psb` writes a
  file whose header version is 2; `inspect_psd` names it PSB; reopened with
  `psd_mode: layers` it has the source's layers, and its PNG export equals the
  source's (ImageMagick `compare -metric AE` is 0).
- **A 30016 x 4 document.** `inspect_psd_export` refuses it as PSD in a
  sentence that names PSB, `export_psd` as PSD fails, and as PSB it is written
  and reopens 30016 wide.
- **Layered TIFF round trip, 8 and 16 bits.** `export_psd` with `format: tiff`
  writes a big-endian TIFF; `magick identify` lists the layer names (it reads
  Photoshop's layer block on its own); `open_document` with no `psd_mode`
  gives ONE layer whose picture is the flat image; with `psd_mode: layers` it
  gives the layers, at 16 bits for the 16-bit file, and the same picture. A
  `psd_mode: layers` request for the path that is already open flat is
  refused in a sentence saying so. A file replaced on disk after
  `inspect_psd` refuses `psd_mode: composite` with the stale fingerprint and
  opens flat with the current one.
- **A baked Gradient Map.** The export report's loss names `grdm`; the PSD
  reopens to a picture equal to the source's (the fixture is opaque).
- **AVIF.** `export_image` with `format: avif` is decoded by `avifdec` and is
  within `AVIF_MEAN_CODES` mean absolute 8-bit codes of the PNG export of the
  same document at `jpeg_quality` 90 (lossy; the bound is fixed here, before
  any measurement, and is HEIC's own 11D bound).
- **JPEG XL.** `export_image` with `format: jxl` is decoded by `djxl` to
  exactly the PNG export's pixels, and `export_preview` with `format: jxl`
  returns an image rather than an error.

`magick`, `avifdec` and `djxl` are required: a missing one FAILS its line
rather than skipping it, because the line would otherwise pass unmeasured.
The fixtures are PNGs this script writes itself.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import app_under_test  # noqa: E402

_spec = importlib.util.spec_from_file_location('v1_formats_support',
                                               ROOT / 'scripts/host-source-probe/support.py')
support = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(support)

# Fixed before measuring: mean absolute difference, in 8-bit codes, between
# avifdec's decode of the AVIF export and the PNG export of the same document.
AVIF_MEAN_CODES = 2.0
WIDTH, HEIGHT = 96, 64
GRADIENT = {'gradient': {'stops': [
    {'position': 0.0, 'color': '#102030', 'opacity': 1.0},
    {'position': 1.0, 'color': '#f0c040', 'opacity': 1.0}]}}

RESULTS = []


def record(name, passed, detail=''):
    RESULTS.append(passed)
    print(f"{'PASS' if passed else 'FAIL'}: {name}" + (f' — {detail}' if detail else ''))


def write_png(path, width, height, pixel):
    """An 8-bit RGBA PNG whose pixel (x, y) is `pixel(x, y)`."""
    def chunk(kind, body):
        return (struct.pack('>I', len(body)) + kind + body
                + struct.pack('>I', zlib.crc32(kind + body)))
    rows = b''.join(b'\0' + bytes(c for x in range(width) for c in pixel(x, y))
                    for y in range(height))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'
                     + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
                     + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def base_pixel(x, y):
    return (x * 255 // (WIDTH - 1), y * 255 // (HEIGHT - 1),
            (x + y) * 255 // (WIDTH + HEIGHT - 2), 255)


def upper_pixel(x, y):
    return (255 - x * 6, 40 + y * 9, 200 - x * 3, 255)


def run(*command):
    return subprocess.run([str(c) for c in command], capture_output=True, text=True)


def differing_pixels(a, b):
    """ImageMagick's count of pixels that differ at all, or None on failure."""
    if not shutil.which('magick'):
        return None
    result = run('magick', 'compare', '-metric', 'AE', a, b, 'null:')
    try:
        return float(result.stderr.strip().split()[0])
    except (ValueError, IndexError):
        return None


COPIES = [0]


def fresh(path):
    """A copy of `path` under a name no document has open. The app answers an
    `open_document` of a path it already has open with that same document, so
    every open in this drive is given a file of its own."""
    COPIES[0] += 1
    copy = path.with_name(f'{path.stem}-open{COPIES[0]}{path.suffix}')
    shutil.copyfile(path, copy)
    return copy


def layers_of(app, document):
    state = app.call('get_document', document_id=document)
    layers = state.get('layers')
    if layers is None:
        layers = state.get('document', {}).get('layers', [])
    return [layer.get('name') for layer in layers], state


def export_png(app, document, where, depth=8):
    app.call('export_image', document_id=document, path=str(where),
             settings={'format': 'png', 'bit_depth': depth}, overwrite=True)
    return where


def open_layers(app, path):
    path = fresh(path)
    report = app.call('inspect_psd', path=str(path))
    opened = app.call('open_document', path=str(path), psd_mode='layers',
                      psd_fingerprint=report['source_fingerprint'])
    return report, opened['document']


def layered_source(app, fixtures, depth):
    """base.png with upper.png placed above it, at `depth` bits."""
    document = app.call('open_document', path=str(fresh(fixtures['base'])))['document']['id']
    app.call('place_image', document_id=document, path=str(fixtures['upper']),
             name='Upper', x=20, y=12)
    if depth == 16:
        app.call('convert_precision', document_id=document, mode='rgba16', structure='preserve')
    return document


def take_psb(app, output, fixtures, depth):
    name = f'PSB round trip at {depth} bits'
    source = layered_source(app, fixtures, depth)
    names, _ = layers_of(app, source)
    path = output / f'layers{depth}.psb'
    written = app.call('export_psd', document_id=source, path=str(path), format='psb')
    version = struct.unpack('>H', path.read_bytes()[4:6])[0]
    report, reopened = open_layers(app, path)
    back, _ = layers_of(app, reopened['id'])
    diff = differing_pixels(export_png(app, source, output / f'psb{depth}-source.png', depth),
                            export_png(app, reopened['id'], output / f'psb{depth}-back.png', depth))
    record(name, written.get('format') == 'psb' and version == 2 and report.get('format') == 'PSB'
           and back == names and len(names) == 2 and written.get('bit_depth') == depth
           and reopened.get('precision') == f'rgba{depth}'
           and report.get('depth') == depth and diff == 0,
           f'version {version}, format {report.get("format")}, layers {back}, '
           f'differing pixels {diff}')


def take_wide(app, output, fixtures):
    name = 'a 30016 x 4 document is refused as PSD and written as PSB'
    document = app.call('open_document', path=str(fresh(fixtures['wide'])))['document']['id']
    classic = app.call('inspect_psd_export', document_id=document, format='psd')
    refused = app.client.call('export_psd', fail=True, document_id=document,
                              path=str(output / 'wide.psd'), format='psd')
    path = output / 'wide.psb'
    app.call('export_psd', document_id=document, path=str(path), format='psb')
    version = struct.unpack('>H', path.read_bytes()[4:6])[0]
    report, reopened = open_layers(app, path)
    refusal = classic.get('refusal') or ''
    record(name, classic.get('available') is False and 'PSB' in refusal and 'PSB' in refused
           and not (output / 'wide.psd').exists() and version == 2
           and report.get('width') == 30016 and reopened.get('width') == 30016,
           f'refusal: {refusal}')


def take_tiff(app, output, fixtures, depth):
    source = layered_source(app, fixtures, depth)
    names, _ = layers_of(app, source)
    path = output / f'layers{depth}.tif'
    written = app.call('export_psd', document_id=source, path=str(path), format='tiff')
    magic = path.read_bytes()[:4]
    identify = run('magick', 'identify', '-format', '%l\\n', path) if shutil.which('magick') else None
    listed = identify.stdout.split('\n') if identify else []
    source_png = export_png(app, source, output / f'tiff{depth}-source.png', depth)
    record(f'layered TIFF round trip at {depth} bits, magick identify lists the layer names',
           written.get('format') == 'tiff' and magic == b'MM\x00*' and len(names) == 2
           and all(layer in listed for layer in names),
           f'layers {names}, magick lists {[line for line in listed if line]}')

    flat_path = fresh(path)
    flat = app.call('open_document', path=str(flat_path))['document']
    flat_names, _ = layers_of(app, flat['id'])
    # That path is now open flat. A layers request for it must be refused by
    # name, never answered with the flat document it already has.
    flat_report = app.call('inspect_psd', path=str(flat_path))
    refused = app.client.call('open_document', fail=True, path=str(flat_path),
                              psd_mode='layers',
                              psd_fingerprint=flat_report['source_fingerprint'])
    record(f'the {depth}-bit layered TIFF already open flat refuses psd_mode layers by name',
           'already open flat' in refused, f'refusal: {refused}')
    flat_diff = differing_pixels(
        source_png, export_png(app, flat['id'], output / f'tiff{depth}-flat.png', depth))
    report, layered = open_layers(app, path)
    back, _ = layers_of(app, layered['id'])
    layered_diff = differing_pixels(
        source_png, export_png(app, layered['id'], output / f'tiff{depth}-layers.png', depth))
    record(f'the {depth}-bit layered TIFF opens flat as one layer and with psd_mode layers as '
           'its layers',
           len(flat_names) == 1 and flat_diff == 0 and report.get('format') == 'TIFF'
           and report.get('stored_composite_available') is False and back == names
           and layered.get('precision') == f'rgba{depth}' and layered_diff == 0,
           f'flat layers {flat_names} (differing pixels {flat_diff}); layered {back} as '
           f'{layered.get("precision")} (differing pixels {layered_diff})')


def take_tiff_changed(app, output, fixtures):
    """A layered TIFF inspected, then replaced on disk, then opened as `psd_mode: composite`
    with the fingerprint of the file that was inspected: refused, as a PSD is. The same request
    with the fingerprint of the file that is there opens it flat."""
    first, second = output / 'changed-a.tif', output / 'changed-b.tif'
    app.call('export_psd', document_id=layered_source(app, fixtures, 8), path=str(first),
             format='tiff')
    app.call('export_psd', document_id=layered_source(app, fixtures, 16), path=str(second),
             format='tiff')
    path = fresh(first)
    stale = app.call('inspect_psd', path=str(path))['source_fingerprint']
    shutil.copyfile(second, path)
    refused = app.client.call('open_document', fail=True, path=str(path), psd_mode='composite',
                              psd_fingerprint=stale)
    current = app.call('inspect_psd', path=str(path))['source_fingerprint']
    opened = app.call('open_document', path=str(path), psd_mode='composite',
                      psd_fingerprint=current)['document']
    names, _ = layers_of(app, opened['id'])
    record('a layered TIFF changed since inspect_psd refuses psd_mode composite; with its '
           'current fingerprint it opens flat',
           'TIFF changed since preflight' in refused and current != stale and len(names) == 1,
           f'refusal: {refused}; flat layers {names}')


def take_bake(app, output, fixtures):
    name = 'a baked Gradient Map reopens to the source picture and its loss names grdm'
    document = app.call('open_document', path=str(fresh(fixtures['base'])))['document']['id']
    app.call('add_adjustment_layer', document_id=document, op='gradient_map', params=GRADIENT,
             name='Map')
    report = app.call('inspect_psd_export', document_id=document)
    loss = next((line for line in report.get('losses', []) if 'Map' in line and 'grdm' in line), '')
    path = output / 'baked.psd'
    app.call('export_psd', document_id=document, path=str(path))
    _, reopened = open_layers(app, path)
    back, _ = layers_of(app, reopened['id'])
    plain = differing_pixels(fixtures['base'], export_png(app, document, output / 'bake-source.png'))
    diff = differing_pixels(output / 'bake-source.png',
                            export_png(app, reopened['id'], output / 'bake-back.png'))
    record(name, report.get('available') is True and bool(loss) and diff == 0
           and len(back) == 2 and plain not in (None, 0)
           and report.get('bake_scratch_bytes') == 3 * 4 * WIDTH * HEIGHT,
           f'differing pixels {diff} (the map itself changes {plain} of the base); loss: {loss}')


def take_modern(app, output, fixtures):
    document = app.call('open_document', path=str(fresh(fixtures['base'])))['document']['id']
    source_png = export_png(app, document, output / 'modern-source.png')

    avif = output / 'export.avif'
    result = app.call('export_image', document_id=document, path=str(avif),
                      settings={'format': 'avif', 'jpeg_quality': 90}, overwrite=True)
    mean = None
    if shutil.which('avifdec') and shutil.which('magick'):
        decoded = output / 'export-avifdec.png'
        run('avifdec', avif, decoded)
        measured = run('magick', 'compare', '-metric', 'MAE', source_png, decoded, 'null:')
        try:
            # "123.4 (0.0019)": the parenthesised value is normalised to 0..1.
            mean = float(measured.stderr.split('(')[1].split(')')[0]) * 255
        except (IndexError, ValueError):
            mean = None
    record(f'AVIF export decoded by avifdec is within {AVIF_MEAN_CODES} mean 8-bit codes of '
           'the PNG export',
           mean is not None and mean <= AVIF_MEAN_CODES and result.get('format') == 'avif',
           f'mean {mean}, result {json.dumps({k: result.get(k) for k in ("format", "bit_depth", "bytes")})}')

    jxl = output / 'export.jxl'
    result = app.call('export_image', document_id=document, path=str(jxl),
                      settings={'format': 'jxl'}, overwrite=True)
    diff = None
    if shutil.which('djxl'):
        decoded = output / 'export-djxl.png'
        run('djxl', jxl, decoded)
        diff = differing_pixels(source_png, decoded) if decoded.is_file() else None
    record('JPEG XL export decoded by djxl equals the 8-bit source exactly',
           diff == 0 and result.get('format') == 'jxl' and jxl.read_bytes()[:2] == b'\xff\x0a',
           f'differing pixels {diff}')

    preview = app.client.rpc('tools/call', {'name': 'export_preview', 'arguments': {
        'document_id': document, 'settings': {'format': 'jxl'}}})
    kinds = [item.get('type') for item in preview.get('content', [])]
    record('export_preview with format jxl returns an image, not an error',
           not preview.get('isError') and 'image' in kinds, f'content {kinds}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=app_under_test.default_app_bundle())
    parser.add_argument('--output', type=Path, default=None,
                        help='where files are written; a fresh temporary folder by default')
    arguments = parser.parse_args()
    os.environ['PYTHONDONTWRITEBYTECODE'] = '1'
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-v1-formats-'))).expanduser()
    output = output.resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    fixtures = dict(base=output / 'base.png', upper=output / 'upper.png', wide=output / 'wide.png')
    write_png(fixtures['base'], WIDTH, HEIGHT, base_pixel)
    write_png(fixtures['upper'], 32, 24, upper_pixel)
    write_png(fixtures['wide'], 30016, 4, lambda x, y: (x % 256, (x // 256) % 256, y * 60, 255))
    takes = [lambda app: take_psb(app, output, fixtures, 8),
             lambda app: take_psb(app, output, fixtures, 16),
             lambda app: take_wide(app, output, fixtures),
             lambda app: take_tiff(app, output, fixtures, 8),
             lambda app: take_tiff(app, output, fixtures, 16),
             lambda app: take_tiff_changed(app, output, fixtures),
             lambda app: take_bake(app, output, fixtures),
             lambda app: take_modern(app, output, fixtures)]
    with support.HostApp(output / 'app', output / 'recovery', executable) as app:
        for take in takes:
            try:
                take(app)
            except Exception as error:  # one failed take must not hide the others
                record(f'take raised {type(error).__name__}', False, str(error)[:600])
    print(f'files: {output}')
    raise SystemExit(0 if RESULTS and all(RESULTS) else 1)


if __name__ == '__main__':
    main()
