#!/usr/bin/env python3
"""Edit ▸ Define Pattern…, the user pattern library and Edit ▸ Fill…'s Pattern contents over MCP
(`define_pattern`, `list_patterns`, `delete_pattern`, `import_patterns`, `export_patterns`,
`fill_selection`'s `pattern`, and `{"name": …}` wherever a pattern object is taken) on owned
Debug instances.

The gates are FIXED here before anything is measured. Each instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library, and one pattern folder shared by the two launches) and ends with it; no
Doctored process this file did not start is signalled.

- **T1 large tile.** A 160 × 160 pattern defined from a whole 8-bit canvas (its request is past
  the common 64 KiB bound) fills a 16-bit document exactly (texel × 257 on the canvas grid) and a
  CMYK document periodically (pixel (x, y) equals (x + 160, y) and (x, y + 160)).
- **T2 round trip.** In each kind of document a 3 × 2 block of known cells at (3, 2) is defined
  from a rectangular selection and filled over the whole canvas; every pixel then equals the
  block's cell (3 + x mod 3, 2 + y mod 2) — exactly at 8 and 16 bits, in Grayscale, Indexed and
  Bitmap; within half an 8-bit code of linear light at 32 bits (the stated 8-bit linear loss);
  within 3 codes of 8-bit L*a*b* in Lab and 3 % ink per plate in CMYK (the sRGB round trip). One
  undo restores the canvas.
- **T3 elsewhere.** `{name}` is accepted by add_fill_layer, set_layer_style (object and string)
  and convert_mode's Bitmap custom pattern, each embedding the tile.
- **T4 replay.** A recorded `fill_selection` with `{name}`, and a recorded `set_layer_style` whose
  style is a JSON STRING naming the pattern, replay the same pixels and style after the pattern is
  deleted from the library: each step carries the tile.
- **T5 refusals.** An elliptical selection, a side past 512, a taken name without replace, a
  built-in's name and id, an unknown name, pattern beside color, a mask target and Quick Mask
  refuse by name and change nothing.
- **T6 library.** list_patterns lists the four built-ins then the library; a relaunch lists the
  same; export, delete and import restore it; an import with a bad entry and a taken name
  imports the rest, skips the bad one by name and renames the taken one.
- **T7 opaque white.** A 512 × 512 opaque white tile, whose base64 is all `/`, converts Gray to
  Bitmap as an inline Custom Pattern (inside the core's 2 MiB options bound), saves as a library
  pattern (inside its 1.5 MiB file bound), exports as a file no larger than that bound (so 64 such
  patterns stay inside the 96 MiB a pattern file import reads) and imports back.

Usage: python3 -B scripts/verify_define_pattern.py [--receipts <folder>]
"""
import argparse
import base64
import json
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 10, 7
BLOCK = (3, 2)
CELLS = ['#c81e28', '#0ad25a', '#3c46e6', '#fafa14', '#202020', '#808080']
BITMAP_CELLS = ['#000000', '#ffffff', '#000000', '#ffffff', '#ffffff', '#000000']
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'lab8': dict(color_mode='lab', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
    'indexed': dict(depth=8, profile='srgb'),
    'bitmap': dict(color_mode='gray', depth=8),
}
TOLERANCE = {'rgb8': 0, 'rgb16': 0, 'gray8': 0, 'indexed': 0, 'bitmap': 0,
             'rgb32': 0.5 / 255 + 1e-6, 'lab8': 3 * 257, 'cmyk8': 0.03 * 65535}
BUILT_INS = ['Checkerboard', 'Stripes', 'Dots', 'Grid']
BIG = 160
WHITE = 512
# `PatternLibraryCodec.maximumFileBytes`; `PatternLibrary.maximumFileBytes` (96 MiB, what an
# import reads) is 64 of them.
PATTERN_FILE_BOUND = 1_572_864


def read(call, doc, x, y):
    """The pixel's own numbers: CMYK ink16 + alpha16, linear f32 at 32 bits, else u16 codes."""
    reply = call('sample_pixel', document_id=doc, x=x, y=y, source='layer', layer=0)
    if 'ink16' in reply:
        return list(reply['ink16']) + [reply['alpha16']]
    if reply.get('mean_rgba32') is not None:
        return list(reply['mean_rgba32'])
    return list(reply['mean_rgba_codes'])


def grid(call, doc, width=W, height=H):
    return {(x, y): read(call, doc, x, y) for y in range(height) for x in range(width)}


def close(a, b, tolerance):
    return len(a) == len(b) and all(abs(p - q) <= tolerance for p, q in zip(a, b))


def make(call, kind, width=W, height=H):
    """A white `kind` document with the 3 × 2 block of CELLS at BLOCK (Indexed made from RGB and
    Bitmap from Gray after painting)."""
    doc = call('new_document', width=width, height=height, background='white',
               **KINDS[kind])['document']['id']
    colours = BITMAP_CELLS if kind == 'bitmap' else CELLS
    for i, colour in enumerate(colours):
        call('select_rect', document_id=doc, x=BLOCK[0] + i % 3, y=BLOCK[1] + i // 3, width=1,
             height=1)
        call('fill_selection', document_id=doc, color=colour)
    call('deselect', document_id=doc)
    if kind == 'indexed':
        call('convert_mode', document_id=doc, mode='indexed', palette='exact', dither='none')
    if kind == 'bitmap':
        call('convert_mode', document_id=doc, mode='bitmap', method='threshold')
    return doc


def tile_object(width, height, rgba):
    return {'name': 'Inline', 'width': width, 'height': height,
            'rgba': base64.b64encode(bytes(rgba)).decode()}


def replay(call):
    """T4: a recorded fill and a recorded string-form style naming 'Replay' replay without it."""
    doc = make(call, 'rgb8')
    call('select_rect', document_id=doc, x=BLOCK[0], y=BLOCK[1], width=3, height=2)
    call('define_pattern', document_id=doc, name='Replay')
    call('deselect', document_id=doc)
    call('start_recording', discard=True)
    call('fill_selection', document_id=doc, pattern={'name': 'Replay'})
    recorded = grid(call, doc)
    overlay = {'effects': [{'type': 'pattern_overlay', 'pattern': {'name': 'Replay'}}]}
    call('set_layer_style', document_id=doc, layer=0, style=json.dumps(overlay))
    styled = call('get_document', document_id=doc)['layers'][0]['style']
    call('stop_recording', name='pattern-replay')
    call('undo', document_id=doc)
    call('undo', document_id=doc)
    call('delete_pattern', name='Replay')
    call('run_action', document_id=doc, name='pattern-replay')
    assert grid(call, doc) == recorded, 'the fill replays differently once the pattern is deleted'
    replayed = call('get_document', document_id=doc)['layers'][0].get('style')
    assert replayed == styled, ('the string-form style replays differently once the pattern is '
                                'deleted', replayed)
    call('close_document', document_id=doc, discard_changes=True)


def white_bitmap(call):
    """T7: an inline opaque white 512 × 512 Custom Pattern converts Gray to Bitmap."""
    gray = call('new_document', width=8, height=8, background='white', **KINDS['gray8'])
    doc = gray['document']['id']
    inline = tile_object(WHITE, WHITE, [255] * (WHITE * WHITE * 4))
    converted = call('convert_mode', document_id=doc, mode='bitmap', method='custom',
                     pattern=inline)
    assert converted.get('ok', True), converted
    call('close_document', document_id=doc, discard_changes=True)


def white_library(call, folder):
    """T7: the white tile saves, exports inside the per-pattern bound and imports back; returns
    the exported file's size."""
    doc = call('new_document', width=WHITE, height=WHITE, background='white', depth=8,
               profile='srgb')['document']['id']
    reply = call('define_pattern', document_id=doc, name='White')
    assert reply['pattern'] == {'name': 'White', 'width': WHITE, 'height': WHITE}, reply
    call('close_document', document_id=doc, discard_changes=True)
    exported = folder / 'white.doctoredpatterns'
    assert call('export_patterns', path=str(exported), names=['White'])['exported'] == ['White']
    size = exported.stat().st_size
    assert size <= PATTERN_FILE_BOUND, ('one white pattern exports past its bound', size)
    call('delete_pattern', name='White')
    reply = call('import_patterns', path=str(exported))
    assert reply['imported'] == ['White'] and reply['skipped'] == [], reply
    return size


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='define-pattern-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    patterns_dir = folder / 'shared-patterns'
    os.environ['DR_PATTERNS_DIR'] = str(patterns_dir)
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_define_pattern.py'}

    with App(folder / 'app1', folder / 'recovery1', interval=600) as app:
        call = app.call

        # ---- T1 large tile -------------------------------------------------------------------
        texels = [((x * 37 + y * 3) % 256, (x * 5 + y * 41) % 256, (x ^ y) % 256, 255)
                  for y in range(BIG) for x in range(BIG)]
        source = call('new_document', width=BIG, height=BIG, background='white', depth=8,
                      profile='srgb')['document']['id']
        data = base64.b64encode(bytes(v for p in texels for v in p)).decode()
        call('put_pixels', document_id=source, layer=0,
             buffer=dict(x=0, y=0, width=BIG, height=BIG, data=data))
        reply = call('define_pattern', document_id=source, name='Big')
        assert reply['pattern'] == {'name': 'Big', 'width': BIG, 'height': BIG}, reply
        assert reply['source'] == 'canvas', reply
        assert len(json.dumps(tile_object(BIG, BIG, [v for p in texels for v in p]))) > 64 * 1024
        for kind in ('rgb16', 'cmyk8'):
            doc = call('new_document', width=BIG + 10, height=BIG + 5, background='white',
                       **KINDS[kind])['document']['id']
            filled = call('fill_selection', document_id=doc, pattern={'name': 'Big'})
            assert filled['changed'] and filled['pattern'] == reply['pattern'], filled
            for x, y in [(0, 0), (7, 3), (159, 4), (2, 159)]:
                here = read(call, doc, x, y)
                if x + BIG < BIG + 10:
                    assert here == read(call, doc, x + BIG, y), (kind, x, y)
                if y + BIG < BIG + 5:
                    assert here == read(call, doc, x, y + BIG), (kind, x, y)
                if kind == 'rgb16':
                    assert here == [c * 257 for c in texels[y * BIG + x]], (x, y, here)
            call('close_document', document_id=doc, discard_changes=True)
        call('close_document', document_id=source, discard_changes=True)
        print(f'PASS t1: a {BIG} x {BIG} pattern (request past 64 KiB) fills 16-bit exactly and '
              'CMYK periodically', flush=True)

        # ---- T2 round trip -------------------------------------------------------------------
        for kind in KINDS:
            doc = make(call, kind)
            before = grid(call, doc)
            call('select_rect', document_id=doc, x=BLOCK[0], y=BLOCK[1], width=3, height=2)
            reply = call('define_pattern', document_id=doc, name=f'Block {kind}')
            assert reply['pattern'] == {'name': f'Block {kind}', 'width': 3, 'height': 2}, reply
            assert reply['source'] == 'selection', reply
            call('deselect', document_id=doc)
            filled = call('fill_selection', document_id=doc, pattern={'name': f'Block {kind}'})
            assert filled['changed'] is True, (kind, filled)
            after = grid(call, doc)
            worst = 0.0
            for (x, y), value in after.items():
                want = before[(BLOCK[0] + x % 3, BLOCK[1] + y % 2)]
                worst = max(worst, max(abs(p - q) for p, q in zip(value, want)))
                assert close(value, want, TOLERANCE[kind]), (kind, x, y, value, want)
            assert any(after[(x, 0)] != after[(0, 0)] for x in range(1, 3)), (kind, 'uniform')
            call('undo', document_id=doc)
            assert close(read(call, doc, 0, 0), before[(0, 0)], 0), (kind, 'undo')
            receipts[kind] = {'worst_difference': worst}
            print(f'PASS t2 {kind}: defined from the selection, filled on the canvas grid; worst '
                  f'difference {worst:g} (bound {TOLERANCE[kind]:g}); undo restores', flush=True)
            call('close_document', document_id=doc, discard_changes=True)

        # ---- T3 elsewhere --------------------------------------------------------------------
        doc = make(call, 'rgb8')
        layer = call('add_fill_layer', document_id=doc, kind='pattern',
                     pattern={'pattern': {'name': 'Block rgb8'}})
        echoed = layer['fill']['pattern']['pattern']
        assert echoed == {'name': 'Block rgb8', 'width': 3, 'height': 2}, layer
        style = {'effects': [{'type': 'pattern_overlay', 'pattern': {'name': 'Block rgb8'}}]}
        for form in (style, json.dumps(style)):
            call('set_layer_style', document_id=doc, layer=0, style=form)
            stored = call('get_document', document_id=doc)['layers'][0]['style']
            tile = stored['effects'][0]['pattern']
            assert tile['name'] == 'Block rgb8' and tile['width'] == 3 and 'rgba' in tile, tile
            call('set_layer_style', document_id=doc, layer=0, style=None)
        call('close_document', document_id=doc, discard_changes=True)
        gray = make(call, 'gray8')
        converted = call('convert_mode', document_id=gray, mode='bitmap', method='custom',
                         pattern={'name': 'Block gray8'})
        assert converted.get('ok', True), converted
        call('close_document', document_id=gray, discard_changes=True)
        print('PASS t3: {name} fills a fill layer, sets a style as object and as string, and '
              'converts to Bitmap by custom pattern, each embedding the tile', flush=True)

        # ---- T4 replay -----------------------------------------------------------------------
        replay(call)
        print('PASS t4: the recorded fill and string-form style replay after their pattern is '
              'deleted', flush=True)

        # ---- T5 refusals ---------------------------------------------------------------------
        doc = make(call, 'rgb8')
        before = grid(call, doc)
        call('select_ellipse', document_id=doc, x=1, y=1, width=6, height=5)
        refusals = {'ellipse': call('define_pattern', fail=True, document_id=doc, name='E')}
        call('deselect', document_id=doc)
        wide = call('new_document', width=600, height=20, background='white', depth=8,
                    profile='srgb')['document']['id']
        refusals['wide'] = call('define_pattern', fail=True, document_id=wide, name='Wide')
        call('close_document', document_id=wide, discard_changes=True)
        refusals['taken'] = call('define_pattern', fail=True, document_id=doc, name='block RGB8')
        call('define_pattern', document_id=doc, name='Block rgb8', replace=True)
        refusals['builtin'] = call('define_pattern', fail=True, document_id=doc,
                                   name='Checkerboard')
        refusals['builtin_id'] = call('define_pattern', fail=True, document_id=doc, name='stripes')
        refusals['unknown'] = call('fill_selection', fail=True, document_id=doc,
                                   pattern={'name': 'Nope'})
        refusals['color'] = call('fill_selection', fail=True, document_id=doc, color='#ff0000',
                                 pattern={'name': 'Block rgb8'})
        call('add_layer_mask', document_id=doc, layer=0, kind='reveal_all')
        refusals['mask'] = call('fill_selection', fail=True, document_id=doc, target='mask',
                                pattern={'name': 'Block rgb8'})
        call('set_quick_mask', document_id=doc, active=True)
        refusals['quick_mask'] = call('fill_selection', fail=True, document_id=doc,
                                      pattern={'name': 'Block rgb8'})
        call('set_quick_mask', document_id=doc, active=False)
        assert 'hard-edged rectangular' in refusals['ellipse'], refusals
        assert 'at most 512' in refusals['wide'], refusals
        assert 'already exists' in refusals['taken'], refusals
        assert "built-in pattern's name" in refusals['builtin'], refusals
        assert "built-in pattern's name" in refusals['builtin_id'], refusals
        assert 'No pattern named' in refusals['unknown'], refusals
        assert 'not both' in refusals['color'], refusals
        assert "Pattern fills a layer's pixels" in refusals['mask'], refusals
        assert "Pattern fills a layer's pixels" in refusals['quick_mask'], refusals
        assert grid(call, doc) == before, 'a refusal changed pixels'
        call('close_document', document_id=doc, discard_changes=True)
        receipts['refusals'] = refusals
        print('PASS t5: an ellipse, a 600 px side, a taken name, a built-in name and id, an '
              'unknown name, pattern with color, a mask and Quick Mask refuse by name', flush=True)

        listing = call('list_patterns')
        names = [p['name'] for p in listing['patterns']]
        assert names[:4] == BUILT_INS and all(p['builtin'] for p in listing['patterns'][:4]), names
        saved = sorted(names[4:])
        assert saved == sorted(['Big'] + [f'Block {k}' for k in KINDS]), saved
        assert listing['broken'] == [], listing

    with App(folder / 'app2', folder / 'recovery2', interval=600) as app:
        call = app.call
        # ---- T6 library ----------------------------------------------------------------------
        rows = call('list_patterns')['patterns']
        assert sorted(p['name'] for p in rows[4:]) == saved, rows
        exported = folder / 'mine.doctoredpatterns'
        reply = call('export_patterns', path=str(exported), names=['Block rgb8', 'Block gray8'])
        assert reply['exported'] == ['Block rgb8', 'Block gray8'], reply
        body = json.loads(exported.read_text())
        assert body['doctored_patterns'] == 1 and len(body['patterns']) == 2, list(body)
        assert 'built-in' in call('export_patterns', fail=True, path=str(exported),
                                  names=['Grid'])
        call('delete_pattern', name='Block rgb8')
        call('delete_pattern', name='Block gray8')
        reply = call('import_patterns', path=str(exported))
        assert sorted(reply['imported']) == ['Block gray8', 'Block rgb8'], reply
        assert call('list_patterns')['patterns'] == rows, 'import does not restore the listing'
        mixed = folder / 'mixed.doctoredpatterns'
        good = dict(schema_version=1, **tile_object(1, 1, [1, 2, 3, 255]))
        mixed.write_text(json.dumps({'doctored_patterns': 1, 'patterns': [
            dict(good, name='Dot'), dict(good, name='Bad', width=2), dict(good, name='Big')]}))
        reply = call('import_patterns', path=str(mixed))
        assert reply['imported'] == ['Dot', 'Big 2'], reply
        assert [s['name'] for s in reply['skipped']] == ['Bad'], reply
        receipts['import'] = reply
        print('PASS t6: the library lists again after a relaunch; export, delete and import '
              'restore it; a mixed file imports two, skips the bad entry, renames the taken name',
              flush=True)

        # ---- T7 opaque white -----------------------------------------------------------------
        white_bitmap(call)
        size = white_library(call, folder)
        receipts['white_export_bytes'] = size
        print(f'PASS t7: an opaque white {WHITE} x {WHITE} tile converts to Bitmap inline, saves, '
              f'exports in {size} bytes (bound {PATTERN_FILE_BOUND}) and imports back', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all; receipts in {output}', flush=True)


if __name__ == '__main__':
    main()
