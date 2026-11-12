#!/usr/bin/env python3
"""Multipage TIFF through `open_document` on an owned Debug instance.

Every fixture is written here by a hand-rolled little-endian TIFF writer (one
uncompressed strip per page, solid colours), so the expectation is this
file's arithmetic, not an encoder's. Cases:

- 8-bit, 16-bit and 32-bit float files of three pages open as layers
  "Page 1".."Page 3", only Page 1 visible, each layer holding its own page's
  colour, the canvas page 1's size, and the import warnings saying so;
- an 8-bit file whose second page cannot be decoded opens page 1 through
  the platform decoder and says which pages are missing and why;
- a two-page grayscale file opens page 1 alone as a Grayscale document, and
  its import warnings say only the first image was imported;
- a contact sheet of the three-page 8-bit file shows page 1 alone.

Launches its own instance (the root CLAUDE.md's recipe) and never another.
Exit 0 when every case passes. `DR_VERIFY_APP` selects another app.
"""

import json
import os
import struct
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import phase5_support  # noqa: E402

W, H = 64, 48
SMALL = (32, 24)
COLOURS8 = [(200, 30, 40), (20, 180, 60), (30, 40, 190)]
COLOURS16 = [(51400, 7710, 10280), (5140, 46260, 15420), (7710, 10280, 48830)]
COLOURSF = [(2.5, -0.25, 0.75), (0.125, 1.5, 0.5), (0.25, 0.375, 3.0)]


def tiff(pages):
    """pages: dicts of width, height, bits, fmt (1 int, 3 float), pixel (a
    tuple per sample), compression, photometric (2, RGB, by default).
    Returns the file's bytes."""
    body = bytearray(b'\0' * 8)
    directories = []
    for page in pages:
        width, height, bits = page['width'], page['height'], page['bits']
        samples = len(page['pixel'])
        code = {8: 'B', 16: 'H', 32: 'f'}[bits]
        strip = struct.pack('<' + code * samples, *page['pixel']) * (width * height)
        if len(body) % 2:
            body += b'\0'
        strip_at = len(body)
        body += strip
        fields = [
            (256, 4, 1, struct.pack('<I', width)),
            (257, 4, 1, struct.pack('<I', height)),
            (258, 3, samples, struct.pack('<' + 'H' * samples, *([bits] * samples))),
            (259, 3, 1, struct.pack('<H', page.get('compression', 1))),
            (262, 3, 1, struct.pack('<H', page.get('photometric', 2))),
            (273, 4, 1, struct.pack('<I', strip_at)),
            (277, 3, 1, struct.pack('<H', samples)),
            (278, 4, 1, struct.pack('<I', height)),
            (279, 4, 1, struct.pack('<I', len(strip))),
            (284, 3, 1, struct.pack('<H', 1)),
            (339, 3, samples, struct.pack('<' + 'H' * samples, *([page['fmt']] * samples))),
        ]
        placed = []
        for tag, kind, count, value in fields:
            if len(value) > 4:
                if len(body) % 2:
                    body += b'\0'
                at = len(body)
                body += value
                value = struct.pack('<I', at)
            placed.append((tag, kind, count, value.ljust(4, b'\0')))
        directories.append(placed)
    if len(body) % 2:
        body += b'\0'
    offsets, at = [], len(body)
    for placed in directories:
        offsets.append(at)
        at += 2 + 12 * len(placed) + 4
    for index, placed in enumerate(directories):
        body += struct.pack('<H', len(placed))
        for tag, kind, count, value in placed:
            body += struct.pack('<HHI', tag, kind, count) + value
        body += struct.pack('<I', offsets[index + 1] if index + 1 < len(offsets) else 0)
    body[0:8] = b'II' + struct.pack('<HI', 42, offsets[0])
    return bytes(body)


def three(bits, fmt, colours):
    sizes = [(W, H), (W, H), SMALL]
    return tiff([dict(width=w, height=h, bits=bits, fmt=fmt, pixel=c)
                 for (w, h), c in zip(sizes, colours)])


def call(app, tool, **arguments):
    """(ok, reply or sentence)."""
    result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
    text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
    if result.get('isError'):
        return False, text
    try:
        return True, json.loads(text)
    except json.JSONDecodeError:
        return True, text


class Case:
    def __init__(self, results, name):
        self.results, self.name, self.failures = results, name, []

    def check(self, label, condition, value=None):
        if not condition:
            self.failures.append(f'{label}: {value!r}'[:600])
        return bool(condition)

    def finish(self):
        state = 'PASS' if not self.failures else 'FAIL'
        self.results[self.name] = state
        print(f'{self.name}: {state}', flush=True)
        for line in self.failures:
            print(f'  {line}', flush=True)


def warnings_of(document):
    return (((document.get('metadata') or {}).get('metadata_import') or {}).get('warnings')
            or [])


def layer_sample(app, doc, index, x, y):
    ok, reply = call(app, 'sample_pixel', document_id=doc, source='layer', layer=index, x=x, y=y)
    assert ok, reply
    for key in ('center_rgba32', 'center_rgba_codes'):
        if key in reply:
            return reply[key]
    return [reply['r'], reply['g'], reply['b'], reply['a']]


def pages_case(app, results, folder, name, data, colours, extra=None, close=None):
    case = Case(results, name)
    path = folder / f'{name}.tif'
    path.write_bytes(data)
    ok, reply = call(app, 'open_document', path=str(path), **(extra or {}))
    if case.check('opens', ok, reply):
        doc = reply['document']['id']
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        layers = [(l['name'], l['visible']) for l in document['layers']]
        case.check('layers', layers == [('Page 1', True), ('Page 2', False), ('Page 3', False)],
                   layers)
        case.check('canvas', (document['width'], document['height']) == (W, H),
                   (document['width'], document['height']))
        if len(layers) == 3:
            sizes = [(l['width'], l['height'], l['offset_x'], l['offset_y'])
                     for l in document['layers']]
            case.check('page 3 keeps its size at the top-left', sizes[2] == (*SMALL, 0, 0), sizes)
            for index, colour in enumerate(colours):
                got = layer_sample(app, doc, index, 4, 4)
                case.check(f'page {index + 1} colour', close(got[:3], colour), got)
        notes = warnings_of(document)
        case.check('warnings say the pages opened as layers',
                   any('opened as layers named by page number' in w for w in notes), notes)
        case.check('warnings name page 3 size',
                   any(w.startswith('Page 3 is a different size') for w in notes), notes)
        case.check('the first-image-only sentence is gone',
                   not any(w.startswith('Only the first TIFF image') for w in notes), notes)
    case.finish()


def broken_case(app, results, folder):
    case = Case(results, 'tiff8-broken-page-2')
    path = folder / 'broken8.tif'
    path.write_bytes(tiff([dict(width=W, height=H, bits=8, fmt=1, pixel=COLOURS8[0]),
                           dict(width=W, height=H, bits=8, fmt=1, pixel=COLOURS8[1],
                                compression=99)]))
    ok, reply = call(app, 'open_document', path=str(path))
    if case.check('opens page 1', ok, reply):
        doc = reply['document']['id']
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        case.check('one layer', len(document['layers']) == 1, document['layers'])
        notes = warnings_of(document)
        case.check('the missing page is named',
                   any(w.startswith("Only page 1 of the file's 2 pages was opened")
                       and 'page 2 of' in w for w in notes), notes)
    case.finish()


def gray_case(app, results, folder):
    """A two-page grayscale file's page 1 opens as a Grayscale document, alone,
    and get_document's import warnings say only the first image opened."""
    case = Case(results, 'tiff8-gray-first-page-alone')
    path = folder / 'gray8.tif'
    path.write_bytes(tiff([dict(width=W, height=H, bits=8, fmt=1, pixel=(grey,), photometric=1)
                           for grey in (60, 190)]))
    ok, reply = call(app, 'open_document', path=str(path))
    if case.check('opens', ok, reply):
        doc = reply['document']['id']
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        case.check('a Grayscale document', document.get('color_mode') == 'gray',
                   document.get('color_mode'))
        case.check('one layer', len(document['layers']) == 1, document['layers'])
        notes = warnings_of(document)
        case.check('the warnings say only the first image opened',
                   any(w.startswith('Only the first TIFF image is imported') for w in notes),
                   notes)
    case.finish()


def sheet_case(app, results, folder):
    case = Case(results, 'contact-sheet-shows-page-1')
    path = folder / 'sheet8.tif'
    path.write_bytes(three(8, 1, COLOURS8))
    ok, reply = call(app, 'contact_sheet_from_sources', files=[str(path)])
    if case.check('sheet', ok, reply):
        doc = reply.get('document', {}).get('id') or reply.get('document_id')
        ok, document = call(app, 'get_document', document_id=doc)
        assert ok, document
        seen = set()
        for gy in range(1, 24):
            for gx in range(1, 24):
                x = document['width'] * gx // 24
                y = document['height'] * gy // 24
                ok, sample = call(app, 'sample_color', document_id=doc, x=x, y=y)
                assert ok, sample
                seen.add((sample['r'], sample['g'], sample['b']))
        case.check('page 1 colour shows', COLOURS8[0] in seen, sorted(seen)[:20])
        case.check('pages 2 and 3 do not', not (set(COLOURS8[1:]) & seen), sorted(seen)[:20])
    case.finish()


def main():
    results = {}
    folder = Path(tempfile.mkdtemp(prefix='tiff-pages-'))
    exact = lambda got, want: list(got) == list(want)
    near = lambda got, want: all(abs(a - b) < 1e-6 for a, b in zip(got, want))
    os.environ['DR_BACKGROUND'] = '1'
    with phase5_support.App(folder / 'app', folder / 'recovery') as app:
        pages_case(app, results, folder, 'tiff8-three-pages', three(8, 1, COLOURS8), COLOURS8,
                   close=exact)
        pages_case(app, results, folder, 'tiff16-three-pages', three(16, 1, COLOURS16),
                   COLOURS16, extra=dict(precision='preserve'), close=exact)
        pages_case(app, results, folder, 'tiff32-three-pages', three(32, 3, COLOURSF), COLOURSF,
                   close=near)
        broken_case(app, results, folder)
        gray_case(app, results, folder)
        sheet_case(app, results, folder)
    failed = [name for name, state in results.items() if state != 'PASS']
    print(json.dumps(results, indent=1))
    print('verify_tiff_pages: ' + ('PASS' if not failed else f'FAIL ({", ".join(failed)})'))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
