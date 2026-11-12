#!/usr/bin/env python3
"""The Pencil (`brush_stroke` / `quick_mask_stroke` with `aliased: true`, the Brush slot's
Pencil tool) and the user brush preset library (`list_brush_presets`, `save_brush_preset`,
`delete_brush_preset`, `import_brush_presets`, `export_brush_presets`; the paint options bar's
Preset popup) on owned Debug instances.

The gates are FIXED here before anything is measured. Each instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library, and one brush preset folder shared by the two launches) and ends with it;
no Doctored process this file did not start is signalled.

- **P1 two values.** At 8, 16 and 32 bits and in Grayscale and CMYK, an aliased stroke changes
  pixels to exactly ONE colour (its ink, read off the render), and the same call without
  `aliased` changes them to more than one: the render can see antialiasing. One undo restores
  the render.
- **P2 oracle.** At 8 bits, a one-point aliased call of an odd and an even size paints exactly
  the pixels this file's own oracle names: the dab snapped (odd diameter on a pixel centre,
  even on a corner), a pixel painted when at least 32 of its 8×8 samples lie in the disc.
- **P3 replay.** A recorded aliased stroke replays to the same render (not asked of CMYK, whose
  documents take no `run_action`).
- **P4 Quick Mask.** At 8 and 16 bits an aliased `quick_mask_stroke` leaves selection coverage
  of 0 and 255 only; the same stroke without it leaves a partial edge.
- **P5 refusals.** `aliased` with hardness 50, with spacing, and on `eraser_stroke` refuse by
  name and change nothing.
- **B1 library.** The built-ins list; save, the file on disk, a built-in's name, repeated
  values, a taken name, `replace`, an out-of-range value; a relaunch lists the same presets.
- **B2 exchange.** Export, delete, import back equal; an import file with a bad entry, a
  repeat of a built-in's values and a taken name imports the rest, skips those by name, and
  renames the taken one; a broken library file lists as `broken` and deletes by file.

Usage: python3 -B scripts/verify_pencil_brush_presets.py [--receipts <folder>]
"""
import argparse
import base64
import json
import math
import os
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))

# ---- the expectations, fixed here BEFORE anything is measured ----------------------------------
W, H = 64, 48
INK = '#c03070'
STROKE = [[10.3, 20.7], [50.2, 27.1], [30.6, 40.2]]
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
NO_ACTIONS = {'cmyk8'}
ONE_POINT = [((20.3, 15.8), 7), ((30.6, 30.4), 6), ((44.9, 12.2), 11)]
BUILT_INS = ['Hard Round', 'Soft Round', 'Airbrush', 'Calligraphy', 'Flat Shader', 'Stippled']
MINE = dict(hardness=40, flow=60, spacing=30, angle=10, roundness=70, airbrush=False)
MINE_REPLACED = dict(hardness=45, flow=65, spacing=35, angle=15, roundness=75, airbrush=True)
HARD_ROUND = dict(hardness=100, flow=100, spacing=25, angle=0, roundness=100, airbrush=False)


def png_pixels(data):
    from verify_phase2 import read_png
    return read_png(base64.b64decode(data))


def half_up(v):
    return math.floor(v + 0.5)


def oracle_dab(cx, cy, size):
    """The pixels one snapped aliased dab paints, from the rule alone."""
    d = max(1, half_up(size))
    if d % 2:
        sx, sy = math.floor(cx) + 0.5, math.floor(cy) + 0.5
    else:
        sx, sy = float(half_up(cx)), float(half_up(cy))
    r2 = (d / 2) ** 2
    painted = set()
    for y in range(H):
        for x in range(W):
            inside = sum(1 for j in range(8) for i in range(8)
                         if (x + (2 * i + 1) / 16 - sx) ** 2 + (y + (2 * j + 1) / 16 - sy) ** 2 <= r2)
            if inside >= 32:
                painted.add((x, y))
    return painted


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    args = parser.parse_args()
    output = Path(args.receipts or tempfile.mkdtemp(prefix='pencil-presets-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    presets_dir = folder / 'shared-brush-presets'
    os.environ['DR_BRUSH_PRESETS_DIR'] = str(presets_dir)
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_pencil_brush_presets.py'}

    with App(folder / 'app1', folder / 'recovery1', interval=600) as app:
        call = app.call

        def render(doc):
            reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'max_side': 4096}})
            assert not reply.get('isError'), reply
            return next(item['data'] for item in reply['content'] if item['type'] == 'image')

        def changed(before, after):
            """{(x, y): colour} of every pixel whose rendered colour moved."""
            a, b = png_pixels(before), png_pixels(after)
            assert a[:2] == b[:2] == (W, H), (a[:2], b[:2])
            return {(i % W, i // W): q for i, (p, q) in enumerate(zip(a[2], b[2])) if p != q}

        # ---- P1 two values, P3 replay --------------------------------------------------------
        for kind, spec in KINDS.items():
            doc = call('new_document', width=W, height=H, background='white', **spec)['document']['id']
            blank = render(doc)
            call('brush_stroke', document_id=doc, points=STROKE, size=9, color=INK, aliased=True)
            pencil = changed(blank, render(doc))
            colours = set(pencil.values())
            assert len(pencil) > 100 and len(colours) == 1, (kind, len(pencil), colours)
            call('undo', document_id=doc)
            assert render(doc) == blank or not changed(blank, render(doc)), (kind, 'undo')
            call('brush_stroke', document_id=doc, points=STROKE, size=9, color=INK,
                 stroke_version=1)
            brush = changed(blank, render(doc))
            assert len(set(brush.values())) > 1, (kind, 'the control shows one colour')
            call('undo', document_id=doc)
            # run_action is not among a CMYK document's tools, so its replay is not asked.
            if kind not in NO_ACTIONS:
                call('start_recording', discard=True)
                call('brush_stroke', document_id=doc, points=STROKE, size=9, color=INK, aliased=True)
                recorded = render(doc)
                call('stop_recording', name=f'pencil-{kind}')
                call('undo', document_id=doc)
                call('run_action', document_id=doc, name=f'pencil-{kind}')
                assert not changed(recorded, render(doc)), (kind, 'the recorded stroke replays differently')
            receipts[kind] = {'painted': len(pencil), 'ink': list(colours)[0],
                              'control_colours': len(set(brush.values()))}
            replay = '' if kind in NO_ACTIONS else ' and the recorded step replays'
            print(f'PASS {kind} p1/p3: the Pencil paints one colour over {len(pencil)} px, the '
                  f'Brush {len(set(brush.values()))}; undo restores{replay}', flush=True)
            call('close_document', document_id=doc, discard_changes=True)

        # ---- P2 oracle -----------------------------------------------------------------------
        doc = call('new_document', width=W, height=H, background='white', depth=8,
                   profile='srgb')['document']['id']
        blank = render(doc)
        for (x, y), size in ONE_POINT:
            call('brush_stroke', document_id=doc, points=[[x, y]], size=size, color=INK, aliased=True)
            painted = set(changed(blank, render(doc)))
            want = oracle_dab(x, y, size)
            assert painted == want, ((x, y, size), sorted(painted ^ want))
            call('undo', document_id=doc)
        print('PASS p2: one-point Pencil dabs paint the oracle\'s pixels at sizes 7, 6 and 11',
              flush=True)

        # ---- P5 refusals ---------------------------------------------------------------------
        before = render(doc)
        refusals = {
            'hardness': call('brush_stroke', fail=True, document_id=doc, points=STROKE, size=9,
                             aliased=True, hardness=50),
            'spacing': call('brush_stroke', fail=True, document_id=doc, points=STROKE, size=9,
                            aliased=True, spacing=40),
            'eraser': call('eraser_stroke', fail=True, document_id=doc, points=STROKE, size=9,
                           aliased=True),
        }
        assert 'hard at full flow' in refusals['hardness'], refusals
        assert 'its tip has no spacing' in refusals['spacing'], refusals
        assert "aliased is the Pencil's tip" in refusals['eraser'], refusals
        assert not changed(before, render(doc)), 'a refusal changed pixels'
        receipts['refusals'] = refusals
        call('close_document', document_id=doc, discard_changes=True)
        print('PASS p5: hardness, spacing and the Eraser refuse by name and change nothing',
              flush=True)

        # ---- P4 Quick Mask -------------------------------------------------------------------
        grid = [{'x': x, 'y': y} for y in range(8, 40, 2) for x in range(4, 60, 2)]
        for depth in (8, 16):
            values = {}
            for aliased in (True, False):
                doc = call('new_document', width=W, height=H, background='white', depth=depth,
                           profile='srgb')['document']['id']
                call('set_quick_mask', document_id=doc, active=True)
                extra = {'aliased': True} if aliased else {'stroke_version': 1}
                call('quick_mask_stroke', document_id=doc, points=STROKE, size=9, width=W,
                     height=H, opacity=1, **extra)
                call('set_quick_mask', document_id=doc, active=False)
                samples = call('get_selection', document_id=doc, points=grid)['samples']
                values[aliased] = {s['coverage'] for s in samples}
                call('close_document', document_id=doc, discard_changes=True)
            assert values[True] == {0, 255}, (depth, values[True])
            assert len(values[False] - {0, 255}) > 0, (depth, values[False])
            print(f'PASS p4 {depth}-bit: an aliased Quick Mask stroke leaves coverage 0 and 255 '
                  f'only; the Brush leaves {len(values[False])} levels', flush=True)

        # ---- B1 library ----------------------------------------------------------------------
        listing = call('list_brush_presets')
        assert [p['name'] for p in listing['presets']] == BUILT_INS, listing
        assert all(p['builtin'] and 'file' not in p for p in listing['presets']), listing
        assert listing['broken'] == [], listing
        saved = call('save_brush_preset', name='Mine', **MINE)
        assert saved['preset'] == dict(name='Mine', **MINE), saved
        files = sorted(presets_dir.glob('*.json'))
        assert len(files) == 1 and json.loads(files[0].read_text())['name'] == 'Mine', files
        library = {
            'builtin_name': call('save_brush_preset', fail=True, name='hard round', **MINE_REPLACED),
            'repeat': call('save_brush_preset', fail=True, name='Twin', **HARD_ROUND),
            'repeat_saved': call('save_brush_preset', fail=True, name='Twin', **MINE),
            'taken': call('save_brush_preset', fail=True, name='Mine', **MINE_REPLACED),
            'range': call('save_brush_preset', fail=True, name='Wide',
                          **dict(MINE_REPLACED, hardness=150)),
        }
        assert 'is a built-in brush preset' in library['builtin_name'], library
        assert '“Hard Round” already holds these values' in library['repeat'], library
        assert '“Mine” already holds these values' in library['repeat_saved'], library
        assert 'already exists' in library['taken'], library
        assert 'hardness must be a number from 0 to 100' in library['range'], library
        call('save_brush_preset', name='Mine', replace=True, **MINE_REPLACED)
        receipts['library_refusals'] = library
        print('PASS b1: built-ins list; save writes one file; a built-in name, repeated values, '
              'a taken name and an out-of-range value refuse; replace replaces', flush=True)

    with App(folder / 'app2', folder / 'recovery2', interval=600) as app:
        call = app.call
        rows = call('list_brush_presets')['presets']
        mine = [p for p in rows if not p['builtin']]
        assert [p['name'] for p in rows[:6]] == BUILT_INS and len(mine) == 1, rows
        assert {k: mine[0][k] for k in MINE_REPLACED} == MINE_REPLACED, mine
        print('PASS b1 relaunch: the saved preset lists again with its replaced values', flush=True)

        # ---- B2 exchange ---------------------------------------------------------------------
        exported = folder / 'mine.doctoredbrushes'
        reply = call('export_brush_presets', path=str(exported))
        assert reply['exported'] == ['Mine'], reply
        body = json.loads(exported.read_text())
        assert body['doctored_brush_presets'] == 1 and len(body['presets']) == 1, body
        assert 'built-in' in call('export_brush_presets', fail=True, path=str(exported),
                                  names=['Hard Round'])
        call('delete_brush_preset', name='Mine')
        assert [p['name'] for p in call('list_brush_presets')['presets']] == BUILT_INS
        assert 'no brush presets to export' in call('export_brush_presets', fail=True,
                                                    path=str(folder / 'empty.doctoredbrushes'))
        reply = call('import_brush_presets', path=str(exported))
        assert reply['imported'] == ['Mine'] and reply['skipped'] == [], reply
        assert call('list_brush_presets')['presets'] == rows, 'import does not restore the listing'
        mixed = folder / 'mixed.doctoredbrushes'
        entry = lambda name, **values: dict(schema_version=1, name=name, **values)
        mixed.write_text(json.dumps({'doctored_brush_presets': 1, 'presets': [
            entry('Other', hardness=20, flow=90, spacing=40, angle=-30, roundness=55, airbrush=False),
            entry('Bad', **dict(MINE, hardness='x')),
            entry('Twin', **HARD_ROUND),
            entry('Mine', hardness=5, flow=15, spacing=50, angle=90, roundness=20, airbrush=False),
            'not an object',
        ]}))
        reply = call('import_brush_presets', path=str(mixed))
        assert reply['imported'] == ['Other', 'Mine 2'], reply
        skipped = {s['name']: s['reason'] for s in reply['skipped']}
        assert set(skipped) == {'Bad', 'Twin', 'entry 5'}, skipped
        assert 'hardness' in skipped['Bad'] and 'Hard Round' in skipped['Twin'], skipped
        names = [p['name'] for p in call('list_brush_presets')['presets']]
        assert names == BUILT_INS + ['Mine', 'Mine 2', 'Other'], names
        (presets_dir / 'garbage.json').write_text('{')
        broken = call('list_brush_presets')['broken']
        assert [(b['file'], b['code']) for b in broken] == [('garbage.json', 'not_json')], broken
        call('delete_brush_preset', file='garbage.json')
        assert call('list_brush_presets')['broken'] == []
        receipts['import_skipped'] = skipped
        print('PASS b2: export, delete and import restore the listing; a mixed file imports two, '
              'skips three by name, renames the taken one; a broken file deletes by file', flush=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all; receipts in {output}', flush=True)


if __name__ == '__main__':
    main()
