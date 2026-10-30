#!/usr/bin/env python3
"""The #547 channels-batch MCP drive: on a 16-bit Generic CMYK document the
alpha channels, Save/Load Selection and their arithmetic, Load Selection from
an ink plate and from the composite, luminosity masks, Quick Mask, paths and
vector masks, the Select commands, the carries through Image ▸ Mode and
8/16 Bits, the refusals by name, one ink plate's selected pixels, and the
`.rz` round trip.

The gates are FIXED here BEFORE anything is measured; a failed gate IS the
receipt and no bound is widened afterwards. The instance is owned by this file
(`phase5_support.App` on its own scratch MCP socket: a dummy API key and a
PRIVATE directory for every preference library) and is terminated by its own
context manager; no Doctored process this file did not start is signalled.

Every fixture is written HERE in pure Python. The CMYK profile is macOS's
Generic CMYK Profile. Ink codes are `percent / 100 × 65535` rounded half up.
Coverage is read with `get_selection`'s `coverage16` at points; a channel is
read by loading it (`load_selection`) and reading the selection. `ALL` is
every pixel of the canvas.

Fixture `split.tif` (32 x 24): the left half ink A [60, 10, 10, 0], the right
half ink B [10, 60, 10, 0], and a 4 x 4 patch of A at (24, 10) inside B.
Fixture `krect.tif` (48 x 40): paper with a 100 % K rectangle (12, 10, 24, 20).

- **C1 channel CRUD.** add_channel empty "One"; select_rect (2, 2, 10, 8) and
  add_channel from selection "Two"; duplicate_channel "Two" → "Two copy";
  rename_channel "Two copy" → "Three"; set_channel_options "Three"
  color_indicates selected; invert_channel "Three"; delete_channel "One".
  list_channels then names ["Two", "Three"], "Three" indicates selected and
  reads 65535 − "Two" at every pixel; the document is still CMYK and the ink
  at (5, 5) is A exactly.
- **C2 Save / Load arithmetic.** a = the rect R1 (2, 2, 12, 12) saved as "S"
  (new); b = R2 (8, 6, 14, 12) feathered 3 px, read at ALL. save_selection
  into "S" with each of replace, add, subtract, intersect (each from a fresh
  "S" = a) reads, at ALL, b, max(a, b), min(a, 65535 − b) and min(a, b)
  exactly. With b the current selection, load_selection "A" (= a, binary) with
  add / subtract / intersect reads max(b, a), min(b, 65535 − a), min(b, a)
  exactly.
- **C3 plate and composite loads.** load_selection plane cyan, magenta, yellow
  and black read 65535 − ink16 of that plate (`sample_color`) at ALL exactly;
  plane luma reads, at ALL, within 2 codes of the Rec. 709 luma of the
  document's own `convert_mode rgb` (srgb) copy's `center_rgba16` (the core's
  LP: that copy's sRGB). add_luminosity_masks adds 9 channels; "Lights 1"
  equals the luma load at ALL within 2 codes.
- **C4 Quick Mask.** With nothing selected on a freshly opened document,
  set_quick_mask active true, fill_selection color #ffffff (the Quick Mask's
  coverage), set_quick_mask active false: the selection covers the whole canvas
  (bounds 0, 0, 32, 24) and `list_documents`' `unsaved_changes` is what it was
  before (Quick Mask dirties nothing).
- **C5 paths and vector masks.** select_rect (4, 4, 12, 10), selection_to_path
  (a work path), set_vector_mask layer 0 path "Work Path"; new_layer, group
  it, set_vector_mask on the group with a triangle; a channel "Keep" from the
  selection. save_copy rz and reopen: get_document says CMYK, list_channels
  names the same, "Keep" reads the same at ALL, get_vector_mask of layer 0 and
  of the group answer the same subpaths, and the ink at the probes is
  bit-exact.
- **C6 Select commands.** modify_selection invert, feather 2, grow 2,
  shrink 2, border 2 and smooth 2, and transform_selection rotate 15 °, each
  from select_rect (6, 5, 14, 10), equal the same calls on an RGB16 twin (the
  same file opened again and `convert_mode rgb`) at ALL exactly (mask-only
  ops). On `krect.tif`, select_magnetic around the rectangle equals it at
  every pixel farther than 1 px from its edge. select_similar tolerance 10
  from select_rect (2, 2, 4, 4) (Similar, global) selects every A pixel (the
  patch included) and no B pixel; contiguous true (Grow) selects the left half
  and not the patch — PN7's answer for these inks (on a fresh copy of the
  fixture).
- **C7 carries.** On a document with channels "Two", "Three" and the vector
  mask on layer 0: convert_mode rgb, then convert_mode cmyk (working) keep the
  channel names and samples at ALL and get_vector_mask answers a vector mask;
  convert_precision cmyk8 keeps the channel names, every channel sample is on
  the 8-bit grid (a multiple of 257) and the vector mask stays.
- **C8 refusals and admissions.** load_selection plane red refuses with
  "plane \\"red\\" is an RGB plane; a CMYK document's planes are cyan,
  magenta, yellow, black, luma and alpha". Since #547's remaining-gaps batch,
  on a fresh copy with a work path from select_rect (4, 4, 12, 10): fill_path
  ink [0, 0, 0, 100] on layer 0 makes (6, 6) [0, 0, 0, 65535] exactly and
  leaves (24, 20) as it was; stroke_path ink [100, 0, 0, 0] size 3 raises C at
  (4, 8) and leaves (24, 20) as it was; brush_stroke target cyan (the plate's
  scalar Brush, white = no ink, as on an RGB plane) along (20, 2)–(28, 2) size 3
  takes cyan at (24, 2) to 0 and leaves M, Y, K and alpha there exactly.
- **C9 one plate's selected pixels.** select_rect (2, 4, 8, 8);
  move_selected_pixels target cyan dx 10 dy 2 background #DC1EAA (take1's
  swatch (220, 30, 170)): at every probe M, Y, K and alpha are unchanged; C in
  the destination is the source's C shifted; C in the vacated area (the source
  rect minus the destination) is HG, 255 - the swatch's 30/59/11 gray rounded
  half up = 153, stored 153 x 257. transform_selected_pixels target cyan
  rotate 90 sampler nearest background #FFFFFF from select_rect (12, 4, 8, 8)
  (across the A/B edge) changes C, and only C.
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
W, H = 32, 24
INK_A = [60, 10, 10, 0]
INK_B = [10, 60, 10, 0]
PATCH = (24, 10, 4, 4)
KRECT = (12, 10, 24, 20)
RGB_PLANE = ('plane "red" is an RGB plane; a CMYK document\'s planes are cyan, magenta, yellow, '
             'black, luma and alpha')
LUMA = (0.2126, 0.7152, 0.0722)
ALL = [(x, y) for y in range(H) for x in range(W)]


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


def split_samples():
    px, py, pw, ph = PATCH
    out = []
    for y in range(H):
        for x in range(W):
            inside = px <= x < px + pw and py <= y < py + ph
            out += codes(INK_A if x < W // 2 or inside else INK_B)
    return out


def krect_samples():
    x0, y0, w, h = KRECT
    out = []
    for y in range(40):
        for x in range(48):
            out += codes([0, 0, 0, 100] if x0 <= x < x0 + w and y0 <= y < y0 + h else [0, 0, 0, 0])
    return out


def luma(rgb16):
    return max(0, min(65535, int(LUMA[0] * rgb16[0] + LUMA[1] * rgb16[1] + LUMA[2] * rgb16[2] + 0.5)))


def rect_plane(x0, y0, w, h, width=W, height=H):
    return [65535 if x0 <= x < x0 + w and y0 <= y < y0 + h else 0
            for y in range(height) for x in range(width)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    assert GENERIC.is_file(), GENERIC
    profile = GENERIC.read_bytes()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='cmyk-channels-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    receipts = {'expectations': 'fixed in scripts/verify_cmyk_channels.py before any measurement'}
    split = folder / 'split.tif'
    split.write_bytes(cmyk_tiff(W, H, split_samples(), profile))
    # The same picture under other names: an open of an already open path answers that
    # document, so every twin is its own file.
    copies = {}
    for name in ('twin', 'quick', 'rgb', 'plate', 'similar', 'paths'):
        copies[name] = folder / f'split_{name}.tif'
        copies[name].write_bytes(split.read_bytes())
    krect = folder / 'krect.tif'
    krect.write_bytes(cmyk_tiff(48, 40, krect_samples(), profile))

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def attempt(tool, **arguments):
            result = app.client.rpc('tools/call', {'name': tool, 'arguments': arguments})
            text = next((c['text'] for c in result.get('content', []) if c['type'] == 'text'), '')
            return bool(result.get('isError')), text

        def refused(tool, sentence, **arguments):
            failed, text = attempt(tool, **arguments)
            assert failed and sentence in text, (tool, text)
            return text

        def sample(doc, x, y):
            reply = call('sample_color', document_id=doc, x=x, y=y)
            return reply['ink16'] + [reply['alpha16']]

        def coverage(doc, points=ALL):
            reply = call('get_selection', document_id=doc,
                         points=[{'x': x, 'y': y} for x, y in points])
            return [s['coverage16'] for s in reply.get('samples', [])]

        def channel(doc, name, points=ALL):
            call('load_selection', document_id=doc, **{'from': 'channel'}, channel=name)
            return coverage(doc, points)

        def unsaved(doc):
            rows = call('list_documents')['documents']
            return next(r for r in rows if r['id'] == doc)['unsaved_changes']

        def names(doc):
            return [c['name'] for c in call('list_channels', document_id=doc)['channels']]

        doc = call('open_document', path=str(split))['document']['id']
        assert call('get_color_profile', document_id=doc).get('color_mode') == 'cmyk'
        a, b = codes(INK_A), codes(INK_B)

        # ---- C1: channel CRUD ---------------------------------------------------------
        call('add_channel', document_id=doc, **{'from': 'empty'}, name='One')
        call('select_rect', document_id=doc, x=2, y=2, width=10, height=8)
        call('add_channel', document_id=doc, **{'from': 'selection'}, name='Two')
        call('duplicate_channel', document_id=doc, channel='Two')
        call('rename_channel', document_id=doc, channel='Two copy', name='Three')
        call('set_channel_options', document_id=doc, channel='Three', color_indicates='selected')
        call('invert_channel', document_id=doc, channel='Three')
        call('delete_channel', document_id=doc, channel='One')
        assert names(doc) == ['Two', 'Three'], names(doc)
        rows = call('list_channels', document_id=doc)['channels']
        assert rows[1].get('color_indicates') == 'selected', rows
        two = channel(doc, 'Two')
        assert two == rect_plane(2, 2, 10, 8), 'Two is the rectangle'
        assert channel(doc, 'Three') == [65535 - v for v in two], 'Three is its inverse'
        assert sample(doc, 5, 5)[:4] == a, sample(doc, 5, 5)
        assert call('get_document', document_id=doc).get('color_mode') == 'cmyk'
        call('deselect', document_id=doc)
        receipts['c1'] = rows
        print('PASS c1: channels are added, duplicated, renamed, inverted and deleted', flush=True)

        # ---- C2: Save / Load arithmetic -----------------------------------------------
        a_plane = rect_plane(2, 2, 12, 12)
        call('select_rect', document_id=doc, x=2, y=2, width=12, height=12)
        call('save_selection', document_id=doc, name='A')
        call('select_rect', document_id=doc, x=8, y=6, width=14, height=12)
        call('modify_selection', document_id=doc, operation='feather', radius=3)
        b_plane = coverage(doc)
        assert any(0 < v < 65535 for v in b_plane), 'b is feathered'
        want = {'replace': b_plane,
                'add': [max(x, y) for x, y in zip(a_plane, b_plane)],
                'subtract': [min(x, 65535 - y) for x, y in zip(a_plane, b_plane)],
                'intersect': [min(x, y) for x, y in zip(a_plane, b_plane)]}
        saved = {}
        for mode, expected in want.items():
            # A fresh "S <mode>" = a, then b combined into it.
            call('select_rect', document_id=doc, x=2, y=2, width=12, height=12)
            call('save_selection', document_id=doc, name=f'S {mode}')
            call('select_rect', document_id=doc, x=8, y=6, width=14, height=12)
            call('modify_selection', document_id=doc, operation='feather', radius=3)
            call('save_selection', document_id=doc, channel=f'S {mode}', mode=mode)
            got = channel(doc, f'S {mode}')
            assert got == expected, (mode, [i for i, (g, e) in enumerate(zip(got, expected)) if g != e][:8])
            saved[mode] = 'exact'
        load = {'add': [max(y, x) for x, y in zip(a_plane, b_plane)],
                'subtract': [min(y, 65535 - x) for x, y in zip(a_plane, b_plane)],
                'intersect': [min(y, x) for x, y in zip(a_plane, b_plane)]}
        for mode, expected in load.items():
            call('select_rect', document_id=doc, x=8, y=6, width=14, height=12)
            call('modify_selection', document_id=doc, operation='feather', radius=3)
            call('load_selection', document_id=doc, **{'from': 'channel'}, channel='A', mode=mode)
            got = coverage(doc)
            assert got == expected, (mode, [i for i, (g, e) in enumerate(zip(got, expected)) if g != e][:8])
            saved[f'load {mode}'] = 'exact'
        call('deselect', document_id=doc)
        receipts['c2'] = saved
        print('PASS c2: Save and Load Selection combine exactly', flush=True)

        # ---- C3: plate and composite loads --------------------------------------------
        inks = [sample(doc, x, y) for x, y in ALL]
        for k, plate in enumerate(['cyan', 'magenta', 'yellow', 'black']):
            call('load_selection', document_id=doc, **{'from': 'plane'}, plane=plate)
            got = coverage(doc)
            assert got == [65535 - p[k] for p in inks], plate
        call('load_selection', document_id=doc, **{'from': 'plane'}, plane='luma')
        composite = coverage(doc)
        twin = call('open_document', path=str(copies['twin']))['document']['id']
        call('convert_mode', document_id=twin, mode='rgb', rgb_space='srgb')
        lp = [luma(call('sample_color', document_id=twin, x=x, y=y)['center_rgba16'][:3])
              for x, y in ALL]
        worst = max(abs(g - w) for g, w in zip(composite, lp))
        assert worst <= 2, worst
        call('close_document', document_id=twin, discard_changes=True)
        before = len(names(doc))
        call('add_luminosity_masks', document_id=doc)
        assert len(names(doc)) == before + 9, names(doc)
        lights = channel(doc, 'Lights 1')
        assert max(abs(g - w) for g, w in zip(lights, composite)) <= 2, 'Lights 1 is the luma'
        call('deselect', document_id=doc)
        receipts['c3'] = {'luma_worst': worst}
        print('PASS c3: a plate loads 1 - ink, the composite its sRGB luma; 9 luminosity masks',
              flush=True)

        # ---- C4: Quick Mask -----------------------------------------------------------
        quick = call('open_document', path=str(copies['quick']))['document']['id']
        clean = unsaved(quick)
        call('set_quick_mask', document_id=quick, active=True)
        call('fill_selection', document_id=quick, color='#ffffff')
        call('set_quick_mask', document_id=quick, active=False)
        reply = call('get_selection', document_id=quick)
        assert reply.get('bounds') == {'x': 0, 'y': 0, 'width': W, 'height': H}, reply
        assert unsaved(quick) == clean, (clean, unsaved(quick))
        reply['unsaved_changes_before_and_after'] = clean
        call('close_document', document_id=quick, discard_changes=True)
        receipts['c4'] = reply
        print('PASS c4: Quick Mask edits coverage and leaves the document clean', flush=True)

        # ---- C5: paths and vector masks -------------------------------------------------
        call('select_rect', document_id=doc, x=4, y=4, width=12, height=10)
        path = call('selection_to_path', document_id=doc)
        call('save_selection', document_id=doc, name='Keep')
        call('set_vector_mask', document_id=doc, layer=0, path='Work Path')
        call('deselect', document_id=doc)
        call('new_layer', document_id=doc, name='Top')
        top = len(call('get_document', document_id=doc)['layers']) - 1
        call('fill', document_id=doc, layer=top, x=1, y=1, ink=[0, 0, 100, 0], tolerance=255)
        call('group_layers', document_id=doc, layers=[top])
        group = len(call('get_document', document_id=doc)['layers']) - 1
        triangle = [{'closed': True, 'anchors': [[16, 2], [30, 20], [2, 20]]}]
        call('set_vector_mask', document_id=doc, layer=group, subpaths=triangle)
        probes = [(1, 1), (5, 5), (10, 8), (16, 12), (25, 12), (30, 22)]
        rz = folder / 'channels.rz'
        call('save_copy', document_id=doc, path=str(rz), format='rz')
        again = call('open_document', path=str(rz))['document']['id']
        assert call('get_document', document_id=again).get('color_mode') == 'cmyk'
        assert names(again) == names(doc), (names(again), names(doc))
        assert channel(again, 'Keep') == channel(doc, 'Keep')
        for layer in (0, group):
            mine = call('get_vector_mask', document_id=doc, layer=layer)
            theirs = call('get_vector_mask', document_id=again, layer=layer)
            assert mine.get('vector_mask') is True and theirs.get('subpaths') == mine.get('subpaths'), \
                (layer, mine, theirs)
        for p in probes:
            assert sample(again, *p) == sample(doc, *p), p
        call('deselect', document_id=doc)
        call('deselect', document_id=again)
        receipts['c5'] = {'path': path, 'probes': {str(p): sample(again, *p) for p in probes}}
        print('PASS c5: paths and vector masks on a raster and a group survive the .rz', flush=True)

        # ---- C6: Select commands -------------------------------------------------------
        rgb = call('open_document', path=str(copies['rgb']))['document']['id']
        call('convert_mode', document_id=rgb, mode='rgb', rgb_space='srgb')
        ops = [('modify_selection', {'operation': 'invert'}),
               ('modify_selection', {'operation': 'feather', 'radius': 2}),
               ('modify_selection', {'operation': 'grow', 'radius': 2}),
               ('modify_selection', {'operation': 'shrink', 'radius': 2}),
               ('modify_selection', {'operation': 'border', 'width': 2}),
               ('modify_selection', {'operation': 'smooth', 'radius': 2}),
               ('transform_selection', {'angle': 15})]
        modified = {}
        for tool, arguments in ops:
            planes = []
            for d in (doc, rgb):
                call('select_rect', document_id=d, x=6, y=5, width=14, height=10)
                call(tool, document_id=d, **arguments)
                planes.append(coverage(d))
            assert planes[0] == planes[1], (tool, arguments)
            modified[f'{tool} {arguments}'] = 'equal'
        kdoc = call('open_document', path=str(krect))['document']['id']
        x0, y0, w, h = KRECT
        outline = [[x0, y0], [x0 + w // 2, y0], [x0 + w, y0], [x0 + w, y0 + h // 2],
                   [x0 + w, y0 + h], [x0 + w // 2, y0 + h], [x0, y0 + h], [x0, y0 + h // 2]]
        call('select_magnetic', document_id=kdoc, points=outline, width=4)
        kpoints = [(x, y) for y in range(40) for x in range(48)]
        got = coverage(kdoc, kpoints)
        misses = []
        for (x, y), v in zip(kpoints, got):
            inside = x0 <= x < x0 + w and y0 <= y < y0 + h
            near = (x0 - 2 < x < x0 + w + 1 and y0 - 2 < y < y0 + h + 1
                    and not (x0 + 1 < x < x0 + w - 2 and y0 + 1 < y < y0 + h - 2))
            if not near and (v >= 32768) != inside:
                misses.append((x, y, v))
        assert not misses, misses[:10]
        call('close_document', document_id=kdoc, discard_changes=True)
        fresh = call('open_document', path=str(copies['similar']))['document']['id']
        call('select_rect', document_id=fresh, x=2, y=2, width=4, height=4)
        call('select_similar', document_id=fresh, tolerance=10, contiguous=False)
        similar = coverage(fresh, [(5, 5), (25, 12), (20, 5)])
        assert similar == [65535, 65535, 0], similar
        call('select_rect', document_id=fresh, x=2, y=2, width=4, height=4)
        call('select_similar', document_id=fresh, tolerance=10, contiguous=True)
        grown = coverage(fresh, [(5, 5), (15, 20), (25, 12), (20, 5)])
        assert grown == [65535, 65535, 0, 0], grown
        call('close_document', document_id=fresh, discard_changes=True)
        call('deselect', document_id=doc)
        call('close_document', document_id=rgb, discard_changes=True)
        receipts['c6'] = modified
        print('PASS c6: Modify and Transform Selection, the Magnetic Lasso, Grow and Similar',
              flush=True)

        # ---- C7: carries ------------------------------------------------------------------
        kept = {n: channel(doc, n) for n in ('Two', 'Three')}
        call('deselect', document_id=doc)
        call('convert_mode', document_id=doc, mode='rgb', rgb_space='srgb')
        assert call('get_document', document_id=doc).get('color_mode') != 'cmyk'
        assert all(channel(doc, n) == v for n, v in kept.items()), 'RGB keeps the channels'
        assert call('get_vector_mask', document_id=doc, layer=0).get('vector_mask') is True
        call('convert_mode', document_id=doc, mode='cmyk')
        assert call('get_document', document_id=doc).get('color_mode') == 'cmyk'
        assert all(channel(doc, n) == v for n, v in kept.items()), 'CMYK keeps the channels'
        assert call('get_vector_mask', document_id=doc, layer=0).get('vector_mask') is True
        call('convert_precision', document_id=doc, mode='cmyk8')
        assert all(all(s % 257 == 0 for s in channel(doc, n)) for n in kept), 'on the 8-bit grid'
        assert call('get_vector_mask', document_id=doc, layer=0).get('vector_mask') is True
        call('deselect', document_id=doc)
        receipts['c7'] = names(doc)
        print('PASS c7: Image > Mode both ways and 16 -> 8 keep channels and vector masks',
              flush=True)

        # ---- C8: refusals and the admitted path and plate paints ------------------------------
        texts = [refused('load_selection', RGB_PLANE, document_id=again, **{'from': 'plane'},
                         plane='red')]
        paths = call('open_document', path=str(copies['paths']))['document']['id']
        call('select_rect', document_id=paths, x=4, y=4, width=12, height=10)
        call('selection_to_path', document_id=paths)
        call('deselect', document_id=paths)
        far = sample(paths, 24, 20)
        call('fill_path', document_id=paths, path='Work Path', layer=0, ink=[0, 0, 0, 100])
        assert sample(paths, 6, 6)[:4] == [0, 0, 0, 65535], sample(paths, 6, 6)
        assert sample(paths, 24, 20) == far, 'fill_path stays inside the path'
        edge = sample(paths, 4, 8)
        call('stroke_path', document_id=paths, path='Work Path', layer=0, ink=[100, 0, 0, 0], size=3)
        assert sample(paths, 4, 8)[0] > edge[0], (sample(paths, 4, 8), edge)
        assert sample(paths, 24, 20) == far, 'stroke_path stays on the path'
        top = sample(paths, 24, 2)
        call('brush_stroke', document_id=paths, target='cyan', points=[[20, 2], [28, 2]], size=3)
        now = sample(paths, 24, 2)
        assert now[0] == 0 and now[1:] == top[1:], (now, top)
        call('close_document', document_id=paths, discard_changes=True)
        receipts['c8'] = {'refusals': texts, 'stroke_edge': sample(again, 4, 8)}
        print('PASS c8: the RGB plane refuses; Fill/Stroke Path paint ink and the Brush one plate',
              flush=True)

        # ---- C9: one plate's selected pixels -------------------------------------------------
        plate = call('open_document', path=str(copies['plate']))['document']['id']
        before = {(x, y): sample(plate, x, y) for x, y in ALL}
        call('select_rect', document_id=plate, x=2, y=4, width=8, height=8)
        reply = call('move_selected_pixels', document_id=plate, target='cyan', dx=10, dy=2,
                     background='#DC1EAA')
        gray = (30 * 220 + 59 * 30 + 11 * 170 + 50) // 100
        hg = (255 - gray) * 257
        for (x, y), old in before.items():
            now = sample(plate, x, y)
            assert now[1:] == old[1:], ((x, y), now, old)
            src = (x - 10, y - 2)
            if 2 <= src[0] < 10 and 4 <= src[1] < 12:
                assert now[0] == before[src][0], ((x, y), now, before[src])
            elif 2 <= x < 10 and 4 <= y < 12:
                assert now[0] == hg, ((x, y), now, hg)
            else:
                assert now[0] == old[0], ((x, y), now, old)
        call('undo', document_id=plate)
        call('select_rect', document_id=plate, x=12, y=4, width=8, height=8)
        turned = call('transform_selected_pixels', document_id=plate, target='cyan', rotate=90,
                      sampler='nearest', background='#FFFFFF')
        changed = False
        for (x, y), old in before.items():
            now = sample(plate, x, y)
            assert now[1:] == old[1:], ((x, y), now, old)
            changed |= now[0] != old[0]
        assert changed, 'the turn moved some cyan'
        call('close_document', document_id=plate, discard_changes=True)
        receipts['c9'] = {'move': reply, 'transform': turned}
        print('PASS c9: one ink plate\'s selected pixels move and turn, the rest untouched',
              flush=True)

    (output / 'receipt.json').write_text(json.dumps(receipts, indent=2, sort_keys=True,
                                                     default=str) + '\n')
    print(f'receipts: {output / "receipt.json"}', flush=True)
    print('ALL GATES PASS', flush=True)


if __name__ == '__main__':
    main()
