#!/usr/bin/env python3
"""The #1 phase-3 MCP drive: layered CMYK files over MCP — a structured CMYK
document written as a layered PSD, PSB and TIFF and read back, CMYK TIFFs with
transparency and planes, Photoshop's own layered CMYK file, and Edit ▸ Color
Settings' CMYK policy on every PSD/TIFF import.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. Every instance is owned by this
file (`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.
Photoshop's take1 (`~/devel/doctored-fixtures/photoshop/cmyk-psd-547/take1`,
`scripts/photoshop-probe/cmyk_files_take.py`) is read, never driven.

The structured document (`build_structured`, also take (a)'s): `new_document`
color_mode cmyk, 16 bits, 64 × 48, the base filled with ink; rasters filled in
8 × 8 cells by `select_rect` + `fill_selection ink`; a Normal group (with a
group mask) holding a Multiply layer at 80 %, a Pass Through group holding a
Screen layer at 60 %, a clipped Overlay layer, a Fill 0.4 layer, a hidden
layer, and Levels, Curves and Hue/Saturation layers each masked to one cell
row band. Every opacity, Fill and Levels value is a whole 1/255 step and every
other parameter a whole Photoshop unit, so a file stores it exactly. Every
probe is a cell's centre; `sample_color`'s `ink16`.

- **F1 build + export.** The document builds and `export_psd` (layers, psd)
  writes it (`bit_depth` 16).
- **F2 PSD reopen.** `inspect_psd`: import_precision "cmyk", layered
  available, 11 layers and 2 groups; `open_document psd_mode layers`: the same
  `get_document` tree (names, kinds, modes, opacity byte, visibility,
  clipping, masks, Fill byte, adjustment ops, parents) and the same ink16 at
  every probe, exactly.
- **F3 edit, PSB and TIFF.** `fill_selection ink` on the reopened document,
  then `export_psd` psb and tiff: the PSB reopens (psd_mode layers) with the
  edited tree and ink exactly; the TIFF opened WITHOUT psd_mode is one CMYK
  layer with the edited composite's ink exactly; WITH psd_mode layers it is
  the tree, ink exact.
- **F4 flattened.** `export_psd mode flattened` psd, psb and tiff reopen
  (psd_mode composite for the PSD and PSB, a plain open for the TIFF) as one
  CMYK layer with the render's ink exactly.
- **F5 TIFF samples.** A 16-bit CMYK TIFF with unassociated alpha and an
  8-bit planar CMYK TIFF (both written here) open as CMYK documents whose
  composite alpha16 is the written transparency and whose ink16 is the
  written ink (8-bit codes ×257) — exactly where opaque, within 1 code where
  partly transparent (the composite's f32 over), none where transparent (the
  composite of nothing carries no ink).
- **F6 Photoshop's file.** take1's `unsupported8.psd` opens with psd_mode
  layers as a CMYK document of six layers, its report naming the type, shape
  and smart-object records and the two colour samplers as losses, and the
  drop shadow (in ink, the styles batch) and the gray Blend If ranges (the
  Blend If batch, #1) as carried.
- **F7 the open policy.** A second instance launched with
  `COLOR_SETTINGS_DEFAULTS` then `-ColorWorkingCMYK <Generic CMYK>` and
  `-ColorCMYKPolicy convert` opens take1's `layers8.psd` with psd_mode layers
  and composite and `layers8.tif` with psd_mode layers: each document's
  `cmyk_profile` (and its byte count) equals that instance's ordinary open of
  take1's `alpha8.tif`, and differs from the files' own SWOP.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

from phase5_support import App

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
TAKE1 = Path.home() / 'devel/doctored-fixtures/photoshop/cmyk-psd-547/take1'
W, H, CELL = 64, 48, 8
BASE_INK = [20, 10, 0, 0]
# (name, rect x, y, w, h, ink percent)
RASTERS = [
    ('Left', (0, 0, 32, 24), [60, 0, 20, 0]),
    ('Right', (32, 0, 32, 24), [0, 60, 0, 20]),
    ('Pass child', (0, 24, 64, 8), [0, 0, 80, 0]),
    ('Clip base', (0, 32, 32, 8), [40, 40, 40, 0]),
    ('Clipped', (16, 32, 48, 8), [0, 0, 0, 60]),
    ('Filled', (48, 0, 16, 48), [0, 40, 0, 40]),
    ('Hidden', (0, 0, 64, 48), [100, 100, 100, 100]),
]
GROUP_MASK = (0, 0, 48, 48)
ADJUSTMENTS = [
    ('Levels', 'levels', {'output_black': 0.2, 'output_white': 0.8, 'k': {'gamma': 1.2}},
     (0, 40, 16, 8)),
    ('Curves', 'curves', {'cmyk': [[0, 0], [100, 140], [255, 255]], 'c': [[0, 0], [128, 90], [255, 255]]},
     (16, 40, 16, 8)),
    ('Hue', 'hue_saturation', {'hue': 15, 'saturation': 0.1}, (32, 40, 16, 8)),
]
EDIT_RECT = (8, 8, 8, 8)
EDIT_INK = [0, 0, 100, 0]


def cells():
    return [dict(name=f'c{c}r{r}', centre=[c * CELL + CELL // 2, r * CELL + CELL // 2])
            for r in range(H // CELL) for c in range(W // CELL)]


def adjusted_cells():
    """The cells an adjustment layer's mask reaches (take (a) records them
    rather than gating them)."""
    out = []
    for _, _, _, (x, y, w, h) in ADJUSTMENTS:
        for cell in cells():
            cx, cy = cell['centre']
            if x <= cx < x + w and y <= cy < y + h:
                out.append(cell['name'])
    return out


def index_of(call, doc, name):
    return next(l['index'] for l in call('get_document', document_id=doc)['layers'] if l['name'] == name)


def build_structured(call):
    """The structured 16-bit CMYK document (module doc): its id, the regions
    (every cell) and the adjusted ones."""
    made = call('new_document', width=W, height=H, unit='px', color_mode='cmyk', depth=16,
                background='white')
    doc = made['document']['id']
    call('select_all', document_id=doc)
    call('fill_selection', document_id=doc, layer=0, ink=BASE_INK)
    for name, (x, y, w, h), ink in RASTERS:
        call('new_layer', document_id=doc, name=name)
        call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
        call('fill_selection', document_id=doc, layer=index_of(call, doc, name), ink=ink)
    call('deselect', document_id=doc)
    call('set_layer_properties', document_id=doc, index=index_of(call, doc, 'Right'),
         blend_mode='Multiply', opacity=0.8)
    call('group_layers', document_id=doc,
         layers=[index_of(call, doc, 'Left'), index_of(call, doc, 'Right')], name='Normal group')
    call('set_layer_properties', document_id=doc, index=index_of(call, doc, 'Normal group'),
         blend_mode='Normal')
    x, y, w, h = GROUP_MASK
    call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
    call('add_layer_mask', document_id=doc, layer=index_of(call, doc, 'Normal group'),
         kind='from_selection')
    call('deselect', document_id=doc)
    call('set_layer_properties', document_id=doc, index=index_of(call, doc, 'Pass child'),
         blend_mode='Screen', opacity=0.6)
    call('group_layers', document_id=doc, layers=[index_of(call, doc, 'Pass child')],
         name='Pass group')
    call('set_layer_properties', document_id=doc, index=index_of(call, doc, 'Clipped'),
         blend_mode='Overlay')
    call('set_layer_clipped', document_id=doc, layer=index_of(call, doc, 'Clipped'), clipped=True)
    call('set_layer_style', document_id=doc, layer=index_of(call, doc, 'Filled'),
         style={'fill_opacity': 0.4})
    call('set_layer_properties', document_id=doc, index=index_of(call, doc, 'Hidden'),
         visible=False)
    for name, op, params, (x, y, w, h) in ADJUSTMENTS:
        call('select_rect', document_id=doc, x=x, y=y, width=w, height=h)
        call('add_adjustment_layer', document_id=doc, op=op, params=params, name=name,
             position='top')
        call('deselect', document_id=doc)
    return dict(document_id=doc, regions=cells(), adjusted_regions=adjusted_cells())


def region_composites(call, doc, regions):
    """Each region's composite ink at its centre as 8-bit codes."""
    out = []
    for region in regions:
        x, y = region['centre']
        sample = call('sample_color', document_id=doc, x=x, y=y)
        out.append(dict(region, ink16=sample['ink16'], alpha16=sample.get('alpha16'),
                        code8=[(v + 128) // 257 for v in sample['ink16']]))
    return out


def tree(call, doc):
    """The document's records, comparable across a round trip."""
    rows = call('get_document', document_id=doc)['layers']
    names = {l['index']: l['name'] for l in rows}
    out = []
    for l in rows:
        style = l.get('style') or {}
        if isinstance(style, str):
            style = json.loads(style)
        out.append((l['name'], l.get('kind'), l.get('blend_mode'),
                    int(float(l.get('opacity', 1)) * 255 + 0.5), l.get('visible'),
                    bool(l.get('clipped')), bool(l.get('has_mask')), bool(l.get('mask_enabled')),
                    int(float(style.get('fill_opacity', 1)) * 255 + 0.5),
                    (l.get('adjustment') or {}).get('op'), names.get(l.get('parent'))))
    return out


def inks(call, doc):
    return {tuple(c['centre']): call('sample_color', document_id=doc, **dict(zip('xy', c['centre'])))['ink16']
            for c in cells()}


def reopen(call, path, mode):
    report = call('inspect_psd', path=str(path))
    return report, call('open_document', path=str(path), psd_mode=mode,
                        psd_fingerprint=report['source_fingerprint'])['document']['id']


def is_cmyk(call, doc):
    return call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'


def short(value, order):
    return struct.pack(order + 'HH', value, 0)


def tiff(width, height, bits, planes, extra, icc, planar):
    """A little-endian CMYK TIFF from `planes` (one list per sample at `bits`):
    chunky in one strip, or planar with one strip a plane; `extra` is the
    ExtraSamples list."""
    pack = (lambda vs: bytes(vs)) if bits == 8 else (lambda vs: struct.pack(f'<{len(vs)}H', *vs))
    if planar:
        strips = [pack(p) for p in planes]
    else:
        strips = [pack([p[i] for i in range(width * height) for p in planes])]
    body = bytearray()

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    offsets = [place(s) for s in strips]
    n = len(planes)
    bits_at = place(struct.pack(f'<{n}H', *([bits] * n)))
    offsets_at = place(struct.pack(f'<{len(offsets)}I', *offsets))
    counts_at = place(struct.pack(f'<{len(strips)}I', *[len(s) for s in strips]))
    extra_at = place(struct.pack(f'<{len(extra)}H', *extra)) if extra else 0
    icc_at = place(icc)
    if len(body) % 2:
        body.append(0)
    one = len(strips) == 1
    entries = [
        (256, 4, 1, struct.pack('<I', width)), (257, 4, 1, struct.pack('<I', height)),
        (258, 3, n, struct.pack('<I', bits_at)), (259, 3, 1, short(1, '<')),
        (262, 3, 1, short(5, '<')),
        (273, 4, len(strips), struct.pack('<I', offsets[0] if one else offsets_at)),
        (277, 3, 1, short(n, '<')), (278, 4, 1, struct.pack('<I', height)),
        (279, 4, len(strips), struct.pack('<I', len(strips[0]) if one else counts_at)),
        (284, 3, 1, short(2 if planar else 1, '<')),
    ]
    if extra:
        entries.append((338, 3, len(extra), short(extra[0], '<') if len(extra) == 1
                        else struct.pack('<I', extra_at)))
    entries.append((34675, 7, len(icc), struct.pack('<I', icc_at)))
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    assert (TAKE1 / 'manifest.json').is_file(), f'the Photoshop take {TAKE1} is required'
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-files-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_files.py before any measurement'}
    manifest = json.loads((TAKE1 / 'manifest.json').read_text())

    def take1(name):
        return TAKE1 / manifest['files'][name]['path']

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        # ---- F1 -------------------------------------------------------------------
        built = build_structured(call)
        doc = built['document_id']
        assert is_cmyk(call, doc)
        source_tree = tree(call, doc)
        source_ink = inks(call, doc)
        psd = folder / 'structured.psd'
        written = call('export_psd', document_id=doc, path=str(psd), format='psd', mode='layers')
        assert written.get('bit_depth') == 16, written
        receipts['f1'] = {'tree': source_tree, 'export': written}
        print('PASS f1: a structured CMYK document exports as a layered PSD', flush=True)

        # ---- F2 -------------------------------------------------------------------
        report, again = reopen(call, psd, 'layers')
        assert report['import_precision'] == 'cmyk', report
        assert report['layered_available'] is True, report
        assert report['layer_count'] == 11 and report['group_count'] == 2, report
        assert is_cmyk(call, again)
        assert tree(call, again) == source_tree, (tree(call, again), source_tree)
        assert inks(call, again) == source_ink
        receipts['f2'] = {'report_losses': report.get('verified_losses'),
                          'observations': report.get('observations')}
        print('PASS f2: Import Layers gives the same tree and ink', flush=True)

        # ---- F3 -------------------------------------------------------------------
        x, y, w, h = EDIT_RECT
        call('select_rect', document_id=again, x=x, y=y, width=w, height=h)
        call('fill_selection', document_id=again, layer=index_of(call, again, 'Left'), ink=EDIT_INK)
        call('deselect', document_id=again)
        edited_tree, edited_ink = tree(call, again), inks(call, again)
        assert edited_ink != source_ink, 'the edit moved nothing'
        psb, tif = folder / 'edited.psb', folder / 'edited.tif'
        call('export_psd', document_id=again, path=str(psb), format='psb', mode='layers')
        call('export_psd', document_id=again, path=str(tif), format='tiff', mode='layers')
        _, from_psb = reopen(call, psb, 'layers')
        assert tree(call, from_psb) == edited_tree and inks(call, from_psb) == edited_ink
        flat = call('open_document', path=str(tif))['document']['id']
        assert is_cmyk(call, flat)
        assert len(call('get_document', document_id=flat)['layers']) == 1
        assert inks(call, flat) == edited_ink
        call('close_document', document_id=flat, discard_changes=True)
        _, from_tif = reopen(call, tif, 'layers')
        assert tree(call, from_tif) == edited_tree and inks(call, from_tif) == edited_ink
        receipts['f3'] = {'tree': edited_tree}
        print('PASS f3: the edited document round trips as a PSB and a layered TIFF', flush=True)

        # ---- F4 -------------------------------------------------------------------
        flats = {}
        for container, ext in [('psd', 'psd'), ('psb', 'psb'), ('tiff', 'tif')]:
            path = folder / f'flat.{ext}'
            call('export_psd', document_id=again, path=str(path), format=container, mode='flattened')
            if container == 'tiff':
                opened = call('open_document', path=str(path))['document']['id']
            else:
                _, opened = reopen(call, path, 'composite')
            assert is_cmyk(call, opened)
            assert len(call('get_document', document_id=opened)['layers']) == 1
            assert inks(call, opened) == edited_ink, container
            flats[container] = 'ok'
        receipts['f4'] = flats
        print('PASS f4: flattened PSD, PSB and TIFF reopen to the render', flush=True)

        # ---- F5 -------------------------------------------------------------------
        icc = GENERIC.read_bytes()
        n = 4 * 3
        ink16 = [[(i * 4099 + c * 9973) % 65536 for i in range(n)] for c in range(4)]
        alpha16 = [(i * 7919) % 65536 for i in range(n)]
        alpha_tif = folder / 'alpha16.tif'
        alpha_tif.write_bytes(tiff(4, 3, 16, ink16 + [alpha16], [2], icc, planar=False))
        ink8 = [[(i * 37 + c * 61) % 256 for i in range(n)] for c in range(4)]
        planar_tif = folder / 'planar8.tif'
        planar_tif.write_bytes(tiff(4, 3, 8, ink8, [], icc, planar=True))
        for path, want_ink, want_alpha in [
                (alpha_tif, ink16, alpha16),
                (planar_tif, [[v * 257 for v in p] for p in ink8], [65535] * n)]:
            opened = call('open_document', path=str(path))['document']['id']
            assert is_cmyk(call, opened), path
            for i in range(n):
                sample = call('sample_color', document_id=opened, x=i % 4, y=i // 4)
                assert sample['alpha16'] == want_alpha[i], (path.name, i, sample)
                want = [p[i] for p in want_ink] if want_alpha[i] else [0, 0, 0, 0]
                bound = 0 if want_alpha[i] in (0, 65535) else 1
                assert all(abs(g - w) <= bound for g, w in zip(sample['ink16'], want)), \
                    (path.name, i, sample['ink16'], want)
        receipts['f5'] = 'ok'
        print('PASS f5: unassociated alpha and planar CMYK TIFFs open as CMYK', flush=True)

        # ---- F6 -------------------------------------------------------------------
        report, ps = reopen(call, take1('unsupported8.psd'), 'layers')
        assert is_cmyk(call, ps)
        assert len(call('get_document', document_id=ps)['layers']) == 6
        losses = '\n'.join(report['verified_losses'])
        for line in ['(“Type”) has text/type metadata', '(“Smart”) has placed/smart-object',
                     '(“Shape”) has vector stroke/origination metadata',
                     '2 colour samplers are not imported into a CMYK document.']:
            assert line in losses, (line, losses)
        observations = '\n'.join(report['observations'])
        assert '(“Shadow”) has Photoshop layer effects (drop_shadow' in observations, observations
        assert 'and Blend If ranges on gray); layered import carries them' in observations, \
            observations
        assert 'Blend If' not in losses, losses
        carried = next(l for l in call('get_document', document_id=ps)['layers']
                       if l['name'] == 'Blend If').get('style') or {}
        assert (carried.get('blend_if') or {}).get('channel') == 'gray', carried
        receipts['f6'] = report['verified_losses']
        print("PASS f6: Photoshop's unsupported constructs import as pixels, named", flush=True)

    # ---- F7: the open policy, in a second instance ---------------------------------
    policy = folder / 'policy'
    with App(policy / 'app', policy / 'recovery', interval=600,
             extra_defaults=['-ColorWorkingCMYK', str(GENERIC), '-ColorCMYKPolicy', 'convert']) as app:
        call = app.call

        def profile(doc):
            reply = call('get_color_profile', document_id=doc)
            return reply.get('cmyk_profile'), reply.get('cmyk_profile_bytes')

        ordinary = call('open_document', path=str(take1('alpha8.tif')))['document']['id']
        working = profile(ordinary)
        assert working[0] and 'SWOP' not in working[0], working
        got = {}
        for name, mode in [('layers8.psd', 'layers'), ('layers8.psd', 'composite'),
                           ('layers8.tif', 'layers')]:
            report, opened = reopen(call, take1(name), mode)
            got[f'{name} {mode}'] = profile(opened)
            assert got[f'{name} {mode}'] == working, (name, mode, got, working)
            call('close_document', document_id=opened, discard_changes=True)
        receipts['f7'] = {'working': working, 'imports': got}
        print('PASS f7: Import Layers and the composite follow the CMYK Convert policy', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True, default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
