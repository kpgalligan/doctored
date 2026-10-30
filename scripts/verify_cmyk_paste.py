#!/usr/bin/env python3
"""The #547 phase-4c MCP drive: copy and paste on and between CMYK documents,
across RGB and CMYK and across two CMYK profiles, the system pasteboard, the
layers paste and Duplicate to Document's MCP twin, Layer Via, Stamp Selected
and Merge Group, and the refusals.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket, a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.
PASTEBOARD ISOLATION: before the owned app starts this sets
`DR_PASTEBOARD_NAME=com.kgalligan.doctored.verify-cmyk-paste.<pid>`, so every
image copy and paste uses that private pasteboard, and pasteboard fixtures are
written to it by a small `xcrun swift` helper (`verify_phase12g_clipboard.py`'s
pattern) — never to the general pasteboard.

Fixtures are 16-bit chunky CMYK TIFFs written HERE in pure Python: macOS's
Generic CMYK Profile (always installed) and Adobe's U.S. Web Coated (SWOP) v2
at `SWOP` — absent, the drive FAILS naming it. Ink is read as `sample_color`'s
`ink16` (the composite); RGB codes as `sample_pixel` layer `mean_rgba16`.

- **P1 same profile.** In a Generic document, with a selection: Copy then
  Paste in Place, Paste (centred), Paste Into and Paste Outside into another
  Generic document, Cut, and Copy Merged pasted in place — each ONE undo step,
  the pasted ink16 EXACTLY the source's at every probe it covers, the documents
  still CMYK; the paste's history row is "Paste Layer" (no conversion).
- **P2 RGB16 → CMYK.** A 16-bit sRGB document's layer pasted in place into the
  Generic document equals `convert_mode cmyk` (Generic) of the same document
  at every probe, EXACTLY; the reply's `color_conversion` is RGB → CMYK
  (engine rasterize, intent relative, BPC true) and the history row starts
  "Paste Layer (RGB → CMYK)" (a PPI suffix may follow).
- **P3 SWOP → Generic.** A SWOP document's copy pasted into the Generic one
  answers `profile` "converted" with a CMYK → CMYK `color_conversion` and
  moves the ink; with `profile` "preserve" it answers "preserved" and keeps the
  ink EXACTLY.
- **P4 CMYK → RGB16.** The Generic document's layer pasted in place into a
  16-bit sRGB document equals `convert_mode rgb` (rgb_space srgb) of the same
  Generic file within 1/65535 per channel at every probe; history
  "Paste Layer (CMYK → RGB)".
- **P5 pasteboard.** A tagged Generic CMYK TIFF on the pasteboard pasted into
  the Generic document lands ink-EXACT (`external` true, no conversion); into
  a 16-bit RGB document it converts (`color_conversion` CMYK → RGB); an RGB
  PNG pasted into the Generic document converts (RGB → CMYK).
- **P6 layers.** `copy` mode layers of two Generic layers and `paste` into
  another Generic document, and `copy_layers_to_document` placement in_place,
  keep the tree and every probe's ink EXACTLY; `copy_layers_to_document`
  CMYK → RGB16 and RGB16 → CMYK answer their `color_conversion` and history
  "Paste Layers (CMYK → RGB)" / "Paste Layers (RGB → CMYK)".
- **P7 Layer Via.** `layer_via_copy` and `layer_via_cut` on the Generic
  document's layer with a selection: one undo step each, a new CMYK layer, the
  composite unchanged EXACTLY at every probe.
- **P8 Stamp Selected, Merge Group.** On the Generic document and on 8-, 16-
  and 32-bit RGB documents: `stamp_selected` of two layers and `merge_group`
  of an isolated Multiply group at 70 %, each ONE undo step, the reply's
  name and index ("Top (merged)" directly above Top; the group's own name in
  its slot), the composite unchanged within 2/65535 (CMYK, 16 bits; 2e-5 at
  float) and 1 code at 8 bits at every probe.
- **P9 refusals.** An RGB adjustment layer copied into the CMYK document and a
  CMYK adjustment layer copied into an RGB document refuse naming the layer;
  pixels pasted across modes with `profile` "preserve" refuse with the
  core's sentence.

Usage: python3 -B scripts/verify_cmyk_paste.py [--receipts <folder>]
"""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured -------------
GENERIC = Path('/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc')
SWOP = Path('/Library/Application Support/Adobe/Color/Profiles/Recommended/USWebCoatedSWOP.icc')
W, H = 32, 20
INK_A = [10, 20, 30, 5]
INK_B = [60, 10, 40, 20]
INK_C = [5, 5, 5, 5]
INK_D = [30, 50, 10, 0]
INK_TOP = [0, 70, 20, 10]
RECT = dict(x=12, y=4, width=8, height=8)
INSIDE = [(13, 5), (16, 8), (19, 11)]
PROBES = [(2, 2), (13, 5), (16, 8), (19, 11), (28, 17)]
CMYK_BOUND = 2
RGB16_BOUND = 1
RGB8_BOUND = 1
FLOAT_BOUND = 2e-5
CROSS_PRESERVE = ("Don't Convert keeps numbers within one colour mode; pixels pasted between "
                  "RGB and CMYK are always converted")


def codes(percent):
    return [int(p / 100 * 65535 + 0.5) for p in percent]


def cmyk_tiff(width, height, samples, icc):
    """A little-endian classic chunky 16-bit CMYK TIFF (Photometric 5)."""
    data = struct.pack(f'<{len(samples)}H', *samples)
    body = bytearray(data)

    def place(chunk):
        if len(body) % 2:
            body.append(0)
        at = 8 + len(body)
        body.extend(chunk)
        return at

    bits_at = place(struct.pack('<4H', 16, 16, 16, 16))
    icc_at = place(icc)
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
        (34675, 7, len(icc), struct.pack('<I', icc_at)),
    ]
    out = bytearray(b'II*\x00') + struct.pack('<I', 8 + len(body)) + body
    out += struct.pack('<H', len(entries))
    for tag, kind, count, value in entries:
        out += struct.pack('<HHI', tag, kind, count) + value
    out += struct.pack('<I', 0)
    return bytes(out)


def halves(path, icc, left=INK_A, right=INK_B):
    a, b = codes(left), codes(right)
    samples = []
    for _ in range(H):
        for x in range(W):
            samples += a if x < W // 2 else b
    path.write_bytes(cmyk_tiff(W, H, samples, icc))
    return path


def solid(path, icc, ink):
    path.write_bytes(cmyk_tiff(W, H, codes(ink) * (W * H), icc))
    return path


def rgb_png(width, height):
    """An untagged 8-bit RGB PNG: a two-colour pattern."""
    rows = b''
    for y in range(height):
        rows += b'\x00' + b''.join(bytes((200, 40, 90) if (x + y) % 2 else (20, 120, 220))
                                   for x in range(width))

    def chunk(kind, data):
        return (struct.pack('>I', len(data)) + kind + data
                + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF))

    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2,
                                                                0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


WRITER = r"""
import AppKit
let arguments = CommandLine.arguments
let pasteboard = NSPasteboard(name: NSPasteboard.Name(arguments[1]))
let data = try! Data(contentsOf: URL(fileURLWithPath: arguments[2]))
pasteboard.clearContents()
let item = NSPasteboardItem()
item.setData(data, forType: arguments[3] == "png" ? .png : .tiff)
exit(pasteboard.writeObjects([item]) ? 0 : 1)
"""


def write_pasteboard(folder, path, kind):
    """Puts ONLY `path`'s bytes, as a public TIFF or PNG, on the drive's
    private pasteboard (`verify_phase12g_clipboard.write_public_tiff`)."""
    script = folder / 'pasteboard-writer.swift'
    script.write_text(WRITER)
    subprocess.run(['xcrun', 'swift', str(script), os.environ['DR_PASTEBOARD_NAME'], str(path),
                    kind], check=True, capture_output=True, timeout=300)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), f'the Generic CMYK profile is missing: {GENERIC}'
    assert SWOP.is_file(), f'FAIL: the SWOP profile this drive needs is missing: {SWOP}'
    generic, swop = GENERIC.read_bytes(), SWOP.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-paste-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    pasteboard = f'com.kgalligan.doctored.verify-cmyk-paste.{os.getpid()}'
    os.environ['DR_PASTEBOARD_NAME'] = pasteboard
    from phase5_support import App  # after the pasteboard name is set
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_paste.py before any measurement',
                'pasteboard': pasteboard}
    serial = iter(range(1, 1000))

    def tiff(make, *args):
        return make(folder / f'fixture-{next(serial)}.tif', *args)

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def open_doc(path):
            return call('open_document', path=str(path))['document']['id']

        def ink(doc, x, y):
            sample = call('sample_color', document_id=doc, x=x, y=y)
            assert sample.get('color_mode') == 'cmyk', sample
            return sample['ink16'] + [sample['alpha16']]

        def inks(doc, probes=PROBES):
            return {p: ink(doc, *p) for p in probes}

        def rgb_layer(doc, layer, x, y):
            sample = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=layer)
            return sample['mean_rgba16']

        def rgb_composite(doc, depth, x, y):
            sample = call('sample_color', document_id=doc, x=x, y=y)
            if depth == 8:
                return [sample['r'], sample['g'], sample['b'], sample['a']]
            return sample['center_rgba32' if depth == 32 else 'center_rgba16']

        def history(doc):
            report = call('list_history', document_id=doc)
            return int(report['undo_count']), report['states'][report['cursor']]['name']

        def one_step(doc, tool, **arguments):
            before, _ = history(doc)
            reply = call(tool, document_id=doc, **arguments)
            after, name = history(doc)
            assert after == before + 1, (tool, before, after)
            return reply, name

        def is_cmyk(doc):
            return call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'

        def layers(doc):
            return call('get_document', document_id=doc)['layers']

        def index_of(doc, name):
            return next(l['index'] for l in layers(doc) if l['name'] == name)

        def near(got, want, bound):
            return all(abs(g - w) <= bound for g, w in zip(got, want))

        def rgb16_doc():
            doc = call('new_document', width=W, height=H, depth=16, profile='srgb',
                       background='white')['document']['id']
            call('select_rect', document_id=doc, x=0, y=0, width=16, height=20)
            call('fill_selection', document_id=doc, layer=0, color='#c8285a')
            call('select_rect', document_id=doc, x=16, y=0, width=16, height=10)
            call('fill_selection', document_id=doc, layer=0, color='#0a78dc')
            call('deselect', document_id=doc)
            return doc

        # ---- P1: same profile -------------------------------------------------
        source = open_doc(tiff(halves, generic))
        target = open_doc(tiff(solid, generic, INK_C))
        assert is_cmyk(source) and is_cmyk(target)
        source_ink = inks(source)
        call('select_rect', document_id=source, **RECT)
        copied = call('copy', document_id=source, layer=0)
        assert copied['typed'], copied
        assert call('get_clipboard')['mode'] == 'cmyk'
        p1 = {}
        reply, name = one_step(target, 'paste', placement='in_place')
        assert name == 'Paste Layer', name
        assert reply['color_conversion'] is None, reply
        for p in INSIDE:
            assert ink(target, *p) == source_ink[p], ('in place', p)
        p1['in_place'] = reply
        reply, _ = one_step(target, 'paste', placement='centered')
        dx, dy = (W - RECT['width']) // 2 - RECT['x'], (H - RECT['height']) // 2 - RECT['y']
        for (x, y) in INSIDE:
            assert ink(target, x + dx, y + dy) == ink(source, x, y), ('centered', x, y)
        p1['centered'] = reply
        for placement in ('into', 'outside'):
            call('select_rect', document_id=target, x=2, y=2, width=10, height=10)
            reply, _ = one_step(target, 'paste', placement=placement)
            assert reply['mask'] is not None, reply
            p1[placement] = reply
        assert ink(target, 3 + 2, 3 + 2) == ink(source, RECT['x'] + 2, RECT['y'] + 2), 'into'
        reply, _ = one_step(source, 'cut', layer=0)
        for p in INSIDE:
            assert ink(source, *p)[4] == 0, ('cut', p)
        p1['cut'] = reply
        call('undo', document_id=source)
        call('select_rect', document_id=source, **RECT)
        call('copy_merged', document_id=source)
        reply, _ = one_step(target, 'paste', placement='in_place')
        for p in INSIDE:
            assert ink(target, *p) == source_ink[p], ('copy merged', p)
        assert is_cmyk(source) and is_cmyk(target)
        p1['copy_merged'] = reply
        receipts['p1'] = p1
        print('PASS p1: same-profile copy, cut and every paste keep ink16 exactly', flush=True)

        # ---- P2: RGB16 -> CMYK ----------------------------------------------------
        rgb = rgb16_doc()
        call('select_all', document_id=rgb)
        call('copy', document_id=rgb, layer=0)
        call('deselect', document_id=rgb)
        cmyk_target = open_doc(tiff(solid, generic, INK_C))
        reply, name = one_step(cmyk_target, 'paste', placement='in_place')
        conversion = reply['color_conversion']
        assert conversion and conversion['from'] == 'RGB' and conversion['to'] == 'CMYK', reply
        assert conversion['engine'] == 'doctored' and conversion['intent'] == 'relative', reply
        assert conversion['black_point_compensation'] is True, reply
        assert name.startswith('Paste Layer (RGB → CMYK)'), name
        pasted = inks(cmyk_target)
        call('convert_mode', document_id=rgb, mode='cmyk', profile='file', path=str(GENERIC))
        converted = inks(rgb)
        for p in PROBES:
            assert pasted[p] == converted[p], ('rgb -> cmyk', p, pasted[p], converted[p])
        receipts['p2'] = {'reply': reply, 'history': name,
                          'ink': {str(p): v for p, v in pasted.items()}}
        print('PASS p2: RGB16 pasted into CMYK equals convert_mode cmyk exactly', flush=True)

        # ---- P3: SWOP -> Generic ------------------------------------------------------
        swop_doc = open_doc(tiff(halves, swop, INK_D, INK_B))
        swop_ink = inks(swop_doc)
        call('select_all', document_id=swop_doc)
        call('copy', document_id=swop_doc, layer=0)
        generic_doc = open_doc(tiff(solid, generic, INK_C))
        reply, _ = one_step(generic_doc, 'paste', placement='in_place')
        assert reply['profile'] == 'converted', reply
        conversion = reply['color_conversion']
        assert conversion and conversion['from'] == 'CMYK' and conversion['to'] == 'CMYK', reply
        moved = inks(generic_doc)
        assert any(moved[p] != swop_ink[p] for p in PROBES), 'the SWOP ink did not convert'
        kept, _ = one_step(generic_doc, 'paste', placement='in_place', profile='preserve')
        assert kept['profile'] == 'preserved', kept
        for p in PROBES:
            assert ink(generic_doc, *p) == swop_ink[p], ('preserve', p)
        receipts['p3'] = {'converted': reply, 'preserved': kept}
        print('PASS p3: SWOP into Generic converts, and Don\'t Convert keeps the ink', flush=True)

        # ---- P4: CMYK -> RGB16 ----------------------------------------------------------
        generic_path = tiff(halves, generic)
        cmyk_source = open_doc(generic_path)
        call('select_all', document_id=cmyk_source)
        call('copy', document_id=cmyk_source, layer=0)
        rgb_target = call('new_document', width=W, height=H, depth=16, profile='srgb',
                          background='white')['document']['id']
        reply, name = one_step(rgb_target, 'paste', placement='in_place')
        conversion = reply['color_conversion']
        assert conversion and conversion['from'] == 'CMYK' and conversion['to'] == 'RGB', reply
        assert name.startswith('Paste Layer (CMYK → RGB)'), name
        top = len(layers(rgb_target)) - 1
        pasted = {p: rgb_layer(rgb_target, top, *p) for p in PROBES}
        call('convert_mode', document_id=cmyk_source, mode='rgb', rgb_space='srgb')
        reference = {p: rgb_layer(cmyk_source, 0, *p) for p in PROBES}
        for p in PROBES:
            assert near(pasted[p], reference[p], RGB16_BOUND), ('cmyk -> rgb', p, pasted[p],
                                                                 reference[p])
        receipts['p4'] = {'reply': reply, 'history': name}
        print('PASS p4: CMYK pasted into RGB16 equals convert_mode rgb within 1/65535', flush=True)

        # ---- P5: the system pasteboard ----------------------------------------------------
        board_tiff = tiff(solid, generic, INK_D)
        write_pasteboard(folder, board_tiff, 'tiff')
        board_target = open_doc(tiff(solid, generic, INK_C))
        reply, _ = one_step(board_target, 'paste', placement='centered')
        assert reply['external'] and reply['color_conversion'] is None, reply
        for p in PROBES:
            assert ink(board_target, *p)[:4] == codes(INK_D), ('pasteboard tiff', p)
        rgb_board = call('new_document', width=W, height=H, depth=16, profile='srgb',
                         background='white')['document']['id']
        into_rgb, _ = one_step(rgb_board, 'paste', placement='centered')
        conversion = into_rgb['color_conversion']
        assert conversion and conversion['from'] == 'CMYK' and conversion['to'] == 'RGB', into_rgb
        png = folder / 'board.png'
        png.write_bytes(rgb_png(W, H))
        write_pasteboard(folder, png, 'png')
        png_target = open_doc(tiff(solid, generic, INK_C))
        from_png, name = one_step(png_target, 'paste', placement='centered')
        conversion = from_png['color_conversion']
        assert conversion and conversion['from'] == 'RGB' and conversion['to'] == 'CMYK', from_png
        assert is_cmyk(png_target)
        receipts['p5'] = {'cmyk_tiff': reply, 'into_rgb': into_rgb, 'rgb_png': from_png}
        print('PASS p5: a CMYK TIFF keeps its ink, converts into RGB, and an RGB PNG converts',
              flush=True)

        # ---- P6: layers -----------------------------------------------------------------
        layered = open_doc(tiff(halves, generic))
        call('new_layer', document_id=layered, name='Top')
        top = index_of(layered, 'Top')
        call('select_rect', document_id=layered, **RECT)
        call('fill_selection', document_id=layered, ink=INK_TOP, layer=top)
        call('deselect', document_id=layered)
        layered_ink = inks(layered)
        call('copy', document_id=layered, mode='layers', layers=[0, top])
        dest = open_doc(tiff(solid, generic, INK_C))
        reply, _ = one_step(dest, 'paste', placement='in_place')
        assert reply['mode'] == 'layers' and reply['color_conversion'] is None, reply
        assert [l['name'] for l in layers(dest)][-2:] == ['Background', 'Top'], layers(dest)
        for p in PROBES:
            assert ink(dest, *p) == layered_ink[p], ('layers paste', p)
        drag_dest = open_doc(tiff(solid, generic, INK_C))
        before, _ = history(drag_dest)
        dragged = call('copy_layers_to_document', document_id=layered, to_document_id=drag_dest,
                       layers=[0, top], placement='in_place')
        assert history(drag_dest)[0] == before + 1
        for p in PROBES:
            assert ink(drag_dest, *p) == layered_ink[p], ('copy_layers_to_document', p)
        rgb_dest = call('new_document', width=W, height=H, depth=16, profile='srgb',
                        background='white')['document']['id']
        to_rgb = call('copy_layers_to_document', document_id=layered, to_document_id=rgb_dest,
                      layers=[0, top], placement='in_place')
        assert to_rgb['color_conversion']['from'] == 'CMYK', to_rgb
        assert history(rgb_dest)[1].startswith('Paste Layers (CMYK → RGB)'), history(rgb_dest)
        rgb_source = rgb16_doc()
        cmyk_dest = open_doc(tiff(solid, generic, INK_C))
        to_cmyk = call('copy_layers_to_document', document_id=rgb_source, to_document_id=cmyk_dest,
                       layers=[0], placement='in_place')
        assert to_cmyk['color_conversion']['to'] == 'CMYK', to_cmyk
        assert history(cmyk_dest)[1].startswith('Paste Layers (RGB → CMYK)'), history(cmyk_dest)
        assert is_cmyk(cmyk_dest)
        receipts['p6'] = {'paste': reply, 'copy_layers': dragged, 'to_rgb': to_rgb,
                          'to_cmyk': to_cmyk}
        print('PASS p6: layers paste and copy_layers_to_document keep ink, and convert across '
              'modes', flush=True)

        # ---- P7: Layer Via ----------------------------------------------------------------
        via = open_doc(tiff(halves, generic))
        via_ink = inks(via)
        p7 = {}
        for tool in ('layer_via_copy', 'layer_via_cut'):
            call('select_rect', document_id=via, **RECT)
            reply, _ = one_step(via, tool, layer=0)
            assert len(layers(via)) == 2, layers(via)
            for p in PROBES:
                assert ink(via, *p) == via_ink[p], (tool, p)
            p7[tool] = reply
            call('undo', document_id=via)
        receipts['p7'] = p7
        print('PASS p7: Layer Via Copy and Cut keep the CMYK composite exactly', flush=True)

        # ---- P8: Stamp Selected, Merge Group ---------------------------------------------
        p8 = {}

        def stamp_and_merge(doc, sample, bound, fill):
            call('new_layer', document_id=doc, name='Top')
            top = index_of(doc, 'Top')
            call('select_rect', document_id=doc, **RECT)
            fill(top)
            call('deselect', document_id=doc)
            call('set_layer_properties', document_id=doc, index=top, opacity=0.6,
                 blend_mode='Multiply')
            before = {p: sample(doc, *p) for p in PROBES}
            reply, _ = one_step(doc, 'stamp_selected', layers=[0, top])
            assert reply['name'] == 'Top (merged)' and reply['layer'] == top + 1, reply
            for p in PROBES:
                assert near(sample(doc, *p), before[p], bound), ('stamp', p)
            call('undo', document_id=doc)
            group = call('group_layers', document_id=doc, layers=[top], name='G')['group']
            call('set_layer_properties', document_id=doc, index=group, opacity=0.7,
                 blend_mode='Multiply')
            grouped = {p: sample(doc, *p) for p in PROBES}
            merged, _ = one_step(doc, 'merge_group', layer=group)
            assert merged['name'] == 'G' and merged['layer'] == group - 1, merged
            assert layers(doc)[merged['layer']].get('kind') == 'layer', layers(doc)
            for p in PROBES:
                assert near(sample(doc, *p), grouped[p], bound), ('merge group', p)
            return {'stamp': reply, 'merge_group': merged}

        cmyk_doc = open_doc(tiff(halves, generic))
        p8['cmyk'] = stamp_and_merge(
            cmyk_doc, ink, CMYK_BOUND,
            lambda top: call('fill_selection', document_id=cmyk_doc, ink=INK_TOP, layer=top))
        for depth, bound in ((8, RGB8_BOUND), (16, RGB16_BOUND + 1), (32, FLOAT_BOUND)):
            doc = call('new_document', width=W, height=H, depth=depth, profile='srgb',
                       background='white')['document']['id']
            call('select_rect', document_id=doc, x=0, y=0, width=16, height=20)
            call('fill_selection', document_id=doc, layer=0, color='#3c64a0')
            call('deselect', document_id=doc)
            p8[str(depth)] = stamp_and_merge(
                doc, lambda d, x, y, depth=depth: rgb_composite(d, depth, x, y), bound,
                lambda top, doc=doc: call('fill_selection', document_id=doc, layer=top,
                                          color='#d2a03c'))
        receipts['p8'] = p8
        print('PASS p8: Stamp Selected and Merge Group keep the composite at 8, 16, 32 bits and '
              'CMYK', flush=True)

        # ---- P9: refusals ------------------------------------------------------------------
        adjusted_rgb = rgb16_doc()
        call('add_adjustment_layer', document_id=adjusted_rgb, op='invert', params={})
        adjustment = len(layers(adjusted_rgb)) - 1
        refusal_target = open_doc(tiff(solid, generic, INK_C))
        rgb_into_cmyk = call('copy_layers_to_document', fail=True, document_id=adjusted_rgb,
                             to_document_id=refusal_target, layers=[0, adjustment],
                             placement='in_place')
        assert 'cannot be pasted into a CMYK document' in rgb_into_cmyk, rgb_into_cmyk
        assert 'adjustment layer' in rgb_into_cmyk, rgb_into_cmyk
        adjusted_cmyk = open_doc(tiff(halves, generic))
        call('add_adjustment_layer', document_id=adjusted_cmyk, op='curves',
             params={'cmyk': [[0, 0], [128, 150], [255, 255]]})
        rgb_refusal_target = call('new_document', width=W, height=H, depth=16, profile='srgb',
                                  background='white')['document']['id']
        cmyk_into_rgb = call('copy_layers_to_document', fail=True, document_id=adjusted_cmyk,
                             to_document_id=rgb_refusal_target, layers=[0, 1],
                             placement='in_place')
        assert 'cannot be pasted into an RGB document' in cmyk_into_rgb, cmyk_into_rgb
        assert 'CMYK adjustment layer' in cmyk_into_rgb, cmyk_into_rgb
        call('select_all', document_id=rgb_refusal_target)
        call('copy', document_id=rgb_refusal_target, layer=0)
        preserve = call('paste', fail=True, document_id=refusal_target, placement='in_place',
                        profile='preserve')
        assert CROSS_PRESERVE in preserve, preserve
        receipts['p9'] = {'rgb_into_cmyk': rgb_into_cmyk, 'cmyk_into_rgb': cmyk_into_rgb,
                          'cross_preserve': preserve}
        print('PASS p9: other-mode adjustment layers and cross-mode Don\'t Convert refuse by '
              'name', flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True,
                                                    default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
