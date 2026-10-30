#!/usr/bin/env python3
"""MCP drive for Photoshop Solid Color fill layers (`SoCo`) through the app.

The script owns its app instance: the Debug app (`phase5_support.App`) on its
own scratch MCP socket with `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`,
the 2 GiB working-limit pin and private `DR_*_DIR`s. It terminates only that
process and never launches Photoshop. The files are the take's own
(`core/tests/fixtures/psd_photoshop/solid_fill_layer`, take1 of
`photoshop-probe/fill_layer_672_take.py`); every expected number is the file's
own `Clr ` (read here from its bytes) or this file's Display P3 oracle (the
published primaries, both D65), never read back from the app.

Cases, for S8, SM8, SP8, SP16 and SC8:
- OPEN: inspect_psd observes the `SoCo` and names no loss; open_document
  psd_mode layers holds layer 1 as a solid fill (`fill.kind` solid) whose
  pixels are the file's colour within 1 code (16-bit: v / 257; SC8: the
  composite ink, `sample_color` ink16 / 257, within 1 code of the `CMYC`
  percent of 255).
- REFIT: canvas_size grows the canvas 10 x 6 about its centre; the fill is
  re-rendered over the new canvas (host refit, its wide colour kept) and a
  pixel in the grown band is the file's colour within 1 code.
- EDIT (SP8 and S8): edit_fill_layer color #00FF00 takes one step and
  re-renders: SP8's pixel is then sRGB green in Display P3 (the oracle), so
  its wide (0, 255, 0) was dropped, and S8's is (0, 255, 0); undo restores
  the file's colour.
- PSD: export_psd layers, then inspect_psd and open_document psd_mode layers
  again: the pixel is the file's colour within 1 code.

Usage: python3 scripts/verify_solid_fill_psd.py --output DIR
Prints `PASS <case>` per case and exits 0 when every case passed.
"""
import argparse
import json
import os
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = ROOT / 'core/tests/fixtures/psd_photoshop/solid_fill_layer'
KEYS = ['S8', 'SM8', 'SP8', 'SP16', 'SC8']
GROW = (10, 6)


def soco_colour(path):
    """The file's `SoCo` `Clr ` numbers: RGBC 0..255 or CMYC percent."""
    data = Path(path).read_bytes()
    at = data.index(b'8BIMSoCo')
    length = struct.unpack('>I', data[at + 8:at + 12])[0]
    body = data[at + 12:at + 12 + length]
    keys = [b'Cyn doub', b'Mgntdoub', b'Ylw doub', b'Blckdoub'] if b'CMYC' in body \
        else [b'Rd  doub', b'Grn doub', b'Bl  doub']
    out = []
    for key in keys:
        i = body.index(key)
        out.append(struct.unpack('>d', body[i + 8:i + 16])[0])
    return ('cmyk' if len(out) == 4 else 'rgb'), out


# ---------------------------------------------------------- the oracle --

SRGB = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]]
P3 = [[0.680, 0.320], [0.265, 0.690], [0.150, 0.060]]


def to_xyz(primaries):
    white = [0.3127 / 0.3290, 1.0, (1 - 0.3127 - 0.3290) / 0.3290]
    p = [[[x / y, 1.0, (1 - x - y) / y][row] for x, y in primaries] for row in range(3)]
    s = mul_vec(inverse(p), white)
    return [[p[r][c] * s[c] for c in range(3)] for r in range(3)]


def inverse(m):
    def cof(r, k):
        r1, r2, k1, k2 = (r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3
        return m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    det = sum(m[0][k] * cof(0, k) for k in range(3))
    return [[cof(k, r) / det for k in range(3)] for r in range(3)]


def mul(a, b):
    return [[sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)] for r in range(3)]


def mul_vec(a, v):
    return [sum(a[r][k] * v[k] for k in range(3)) for r in range(3)]


def encode(v):
    v = min(max(v, 0.0), 1.0)
    return v * 12.92 if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def decode(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def srgb_in_p3(rgb):
    """sRGB numbers 0..255 as Display P3 numbers 0..255."""
    m = mul(inverse(to_xyz(P3)), to_xyz(SRGB))
    return [encode(v) * 255 for v in mul_vec(m, [decode(c / 255) for c in rgb])]


# ------------------------------------------------------------- the app --

class Drive:
    def __init__(self, app, folder):
        self.app, self.folder = app, Path(folder)

    def call(self, tool, **arguments):
        return self.app.call(tool, **arguments)

    def open(self, path):
        report = self.call('inspect_psd', path=str(path))
        doc = self.call('open_document', path=str(path), psd_mode='layers',
                        psd_fingerprint=report['source_fingerprint'])['document']['id']
        return report, doc

    def pixel(self, doc, kind, x, y, depth):
        """Layer 1's pixel (RGB, 0..255) or the composite ink (CMYK, 0..255)."""
        if kind == 'cmyk':
            sample = self.call('sample_color', document_id=doc, x=x, y=y)
            return [v / 257 for v in sample['ink16']]
        reply = self.call('sample_pixel', document_id=doc, source='layer', layer=1, x=x, y=y)
        scale = 257.0 if depth == 16 else 1.0
        return [v / scale for v in reply['center_rgba_codes'][:3]]


def want_of(kind, numbers):
    return [n * 255 / 100 for n in numbers] if kind == 'cmyk' else numbers


def near(got, want, bound=1.0):
    return len(got) == len(want) and all(abs(a - b) <= bound for a, b in zip(got, want))


def case(drive, key):
    path = FIXTURES / f'{key}.psd'
    kind, numbers = soco_colour(path)
    want = want_of(kind, numbers)
    manifest = json.loads((FIXTURES / 'photoshop.json').read_text())['files'][key]
    depth = manifest['case']['depth']
    report, doc = drive.open(path)
    observed = [o for o in report['observations'] if 'Solid Color Fill layer (SoCo)' in o]
    assert observed, ('observation', key, report['observations'])
    assert not any('SF' in loss for loss in report['verified_losses']), report['verified_losses']
    layer = drive.call('get_document', document_id=doc)['layers'][1]
    assert (layer.get('fill') or {}).get('kind') == 'solid', layer
    got = drive.pixel(doc, kind, 20, 20, depth)
    assert near(got, want), ('OPEN', key, got, want)
    # REFIT: the canvas grows; a pixel in the new band is the colour.
    reply = drive.call('canvas_size', document_id=doc, width=64 + GROW[0], height=48 + GROW[1],
                       anchor='center')
    assert reply.get('ok'), reply
    got = drive.pixel(doc, kind, 1, 1, depth)
    assert near(got, want), ('REFIT', key, got, want)
    edited = None
    if key in ('SP8', 'S8'):
        start = drive.call('list_history', document_id=doc)['cursor']
        drive.call('edit_fill_layer', document_id=doc, layer=1, color='#00FF00')
        assert drive.call('list_history', document_id=doc)['cursor'] == start + 1, 'one step'
        green = srgb_in_p3([0, 255, 0]) if key == 'SP8' else [0, 255, 0]
        edited = drive.pixel(doc, kind, 1, 1, depth)
        assert near(edited, green, 2.0), ('EDIT', key, edited, green)
        if key == 'SP8':
            assert abs(edited[0] - want[0]) > 50, ('the wide colour is dropped', edited, want)
        drive.call('undo', document_id=doc)
        got = drive.pixel(doc, kind, 1, 1, depth)
        assert near(got, want), ('UNDO', key, got, want)
    again = drive.folder / f'{key}-again.psd'
    drive.call('export_psd', document_id=doc, path=str(again), format='psd', mode='layers')
    _, back = drive.open(again)
    got = drive.pixel(back, kind, 1, 1, depth)
    assert near(got, want), ('PSD', key, got, want)
    for d in (doc, back):
        drive.call('close_document', document_id=d, discard_changes=True)
    return dict(file=str(path), colour=numbers, edited=edited)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--output', required=True, help='a fresh folder')
    arguments = parser.parse_args()
    output = Path(arguments.output).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=False)
    os.environ.setdefault('DR_BACKGROUND', '1')
    sys.path.insert(0, str(HERE))
    from phase5_support import App
    results, failed = {}, []
    with App(output / 'app', output / 'recovery', interval=600) as app:
        drive = Drive(app, output)
        for key in KEYS:
            try:
                results[key] = dict(passed=True, detail=case(drive, key))
                print(f'PASS {key}', flush=True)
            except BaseException as error:  # noqa: BLE001 - every case reports
                results[key] = dict(passed=False, error=repr(error)[:2000])
                failed.append(key)
                print(f'FAIL {key}: {error!r}'[:2400], flush=True)
    (output / 'receipt.json').write_text(json.dumps(results, indent=2) + '\n')
    print('ALL CASES PASS' if not failed else f'FAILED: {failed}')
    raise SystemExit(1 if failed else 0)


if __name__ == '__main__':
    main()
