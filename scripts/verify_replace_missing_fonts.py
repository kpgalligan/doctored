#!/usr/bin/env python3
"""Replace Missing Fonts (Layer ▸ Replace Missing Fonts…, MCP `list_missing_fonts` and
`replace_missing_fonts`; designs/replace-missing-fonts.md) on an owned Debug instance, in every
document kind that holds text layers: RGB at 8, 16 and 32 bits, Grayscale, Lab and CMYK.

A family this Mac lacks is made without the new tools: the document is saved as `.doctored`
with text layers in Helvetica Neue, and every `"font":"Helvetica Neue"` in the file is
byte-patched to `"font":"Zz Absent Font"` (the same UTF-8 length, so every length prefix
holds). That the family is absent is checked independently: `add_text_layer` refuses it. The
oracle for every pixel comparison is the never-patched document's own render, not a stored
image.

The gates are FIXED here before anything is measured. The instance is owned by this file
(`phase5_support.App`: a scratch MCP socket, a dummy API key, a private directory for every
preference library) and ends with it; no Doctored process this file did not start is signalled.

Per kind, four text layers: A (identity, two colour runs), B (rotated -20 degrees about its
origin, a fixed leading, two lines), C (Times New Roman, never missing), D (scaled 2x).

- **M1 make.** The composite and each layer's render (R0), each layer's text object and bounds;
  `save_copy` as `.doctored`; the patch replaces at least three family names.
- **M2 missing.** The patched file opens with R0's pixels; A, B and D report font
  "Zz Absent Font" and `font_missing` true, C has no `font_missing`; `list_missing_fonts`
  names one family with layers A, B and D.
- **M3 replace back.** `replace_missing_fonts` with Helvetica Neue is ONE undo step; every
  layer's bounds equal M1's (a layer that moved is named first); the composite and every layer
  render equal R0; every text object (origin, colours, ink and runs included) equals M1's;
  `list_missing_fonts` is empty. B is the layer the fallback face would move: its anchor
  recovered from its pixels with the system face lands pixels away from where it was made.
- **M4 reopen.** `save_copy` and `open_document`: text objects and render equal M3's; a size
  round trip on B (`edit_text_layer` size + 1, then back) renders R0 again.
- **M5 another family.** On the patched file reopened, Courier New: A's and D's origin and
  bounds x, y are unchanged (their left edge and top do not depend on the face); B's bounds
  centre is within 1 px of its saved centre; C is untouched; one undo restores M2's render and
  fonts, redo returns Courier New.
- **M6 refusals.** Each leaves the undo count unchanged: no replacements; a `font` no layer
  misses; a `with` that is not an installed family exactly ("helvetica neue", "Zz Nope"); A
  pixel-locked (the sentence names the lock).
- **M7 gates.** `get_precision`'s `supported_tools` with its `shared_tools` hold both tools in
  the 16-bit, 32-bit, Grayscale, Lab and CMYK documents.

Usage: python3 -B scripts/verify_replace_missing_fonts.py [--receipts <folder>] [--kinds k,…]
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
ORIGINAL = 'Helvetica Neue'
ABSENT = 'Zz Absent Font'
OTHER = 'Courier New'
STEADY = 'Times New Roman'
assert len(ORIGINAL.encode()) == len(ABSENT.encode())
W, H = 420, 300
ANGLE = math.radians(-20)
KINDS = {
    'rgb8': dict(depth=8, profile='srgb'),
    'rgb16': dict(depth=16, profile='srgb'),
    'rgb32': dict(depth=32, profile='srgb'),
    'gray8': dict(color_mode='gray', depth=8),
    'lab8': dict(color_mode='lab', depth=8),
    'cmyk8': dict(color_mode='cmyk', depth=8),
}
LAYERS = {
    'A': dict(text='Alpha Ag', x=16, y=12, size=40, font=ORIGINAL, wrap_width=0,
              rgba=[0.1, 0.2, 0.6, 1],
              color_runs=[{'location': 0, 'length': 2, 'rgba': [0.8, 0.1, 0.1, 1]}]),
    'B': dict(text='Bravo turns\nthe second line', x=190, y=70, size=30, leading=50,
              font=ORIGINAL, wrap_width=0, rgba=[0.2, 0.5, 0.2, 1],
              transform=[math.cos(ANGLE), -math.sin(ANGLE), math.sin(ANGLE), math.cos(ANGLE)]),
    'C': dict(text='Charlie', x=16, y=240, size=28, font=STEADY, wrap_width=0,
              rgba=[0.4, 0.1, 0.5, 1]),
    'D': dict(text='Delta', x=16, y=120, size=22, font=ORIGINAL, wrap_width=0,
              rgba=[0.6, 0.3, 0.0, 1], transform=[2, 0, 0, 2]),
}
MISSING_LAYERS = ['A', 'B', 'D']
PATCH_FROM = f'"font":"{ORIGINAL}"'.encode()
PATCH_TO = f'"font":"{ABSENT}"'.encode()
TOOLS = ['list_missing_fonts', 'replace_missing_fonts']
GATED_KINDS = ['rgb16', 'rgb32', 'gray8', 'lab8', 'cmyk8']
CENTRE_SLACK = 1


def png_pixels(data):
    from verify_phase2 import read_png
    return read_png(base64.b64decode(data))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--receipts', default=None)
    parser.add_argument('--kinds', default=','.join(KINDS))
    args = parser.parse_args()
    kinds = [k for k in args.kinds.split(',') if k]
    assert all(k in KINDS for k in kinds), kinds
    output = Path(args.receipts or tempfile.mkdtemp(prefix='replace-fonts-')).expanduser()
    output.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix='work-', dir=output))
    # A background instance: never activates itself or takes the screen (`BackgroundLaunch`).
    os.environ.setdefault('DR_BACKGROUND', '1')
    from phase5_support import App
    receipts = {'expectations': 'fixed in scripts/verify_replace_missing_fonts.py'}

    with App(folder / 'app', folder / 'recovery', interval=600) as app:
        call = app.call

        def render(doc, **extra):
            reply = app.client.rpc('tools/call', {'name': 'render', 'arguments': {
                'document_id': doc, 'max_side': 4096, **extra}})
            assert not reply.get('isError'), reply
            return next(item['data'] for item in reply['content'] if item['type'] == 'image')

        def same(first, second):
            """Equal renders: identical PNG bytes, else identical decoded pixels."""
            if first == second:
                return True, 0
            a, b = png_pixels(first), png_pixels(second)
            if a[:2] != b[:2]:
                return False, f'size {a[:2]} vs {b[:2]}'
            differing = sum(1 for p, q in zip(a[2], b[2]) if p != q)
            return differing == 0, differing

        def rows(doc):
            return {row['index']: row for row in call('get_document', document_id=doc)['layers']}

        def bounds(row):
            return (row['offset_x'], row['offset_y'], row['width'], row['height'])

        def undo_count(doc):
            return call('list_history', document_id=doc)['undo_count']

        def snapshot(doc, indices):
            state = rows(doc)
            return {
                'composite': render(doc),
                'layers': {name: render(doc, layer=index) for name, index in indices.items()},
                'text': {name: state[index].get('text') for name, index in indices.items()},
                'bounds': {name: bounds(state[index]) for name, index in indices.items()},
            }

        def assert_renders(label, now, then):
            """The composite and every layer equal; a failure names each one that differs."""
            pairs = {'composite': (now['composite'], then['composite'])}
            pairs.update({name: (now['layers'][name], then['layers'][name])
                          for name in now['layers']})
            differing = {}
            for name, (first, second) in pairs.items():
                ok, detail = same(first, second)
                if not ok:
                    differing[name] = detail
            assert not differing, (label, 'differing pixels', differing)

        # ---- the fonts this drive needs, and the absent one, checked without the new tools ----
        probe = call('new_document', width=64, height=64, depth=8, profile='srgb',
                     background='white')['document']['id']
        for family in (ORIGINAL, OTHER, STEADY):
            call('add_text_layer', document_id=probe, text='x', x=2, y=2, font=family)
        absent = call('add_text_layer', fail=True, document_id=probe, text='x', x=2, y=2,
                      font=ABSENT)
        assert 'No font family named' in absent, absent
        receipts['absent_refusal'] = absent

        for kind in kinds:
            # ---- M1 make ----------------------------------------------------------------------
            doc = call('new_document', width=W, height=H, background='white',
                       **KINDS[kind])['document']['id']
            indices = {}
            for name, spec in LAYERS.items():
                reply = call('add_text_layer', document_id=doc, **spec)
                indices[name] = reply['layer']
            made = snapshot(doc, indices)
            for name in LAYERS:
                assert made['text'][name] and 'font_missing' not in made['text'][name], (kind, name)
            clean = folder / f'{kind}.doctored'
            call('save_copy', document_id=doc, path=str(clean), format='doctored')
            data = clean.read_bytes()
            occurrences = data.count(PATCH_FROM)
            assert occurrences >= len(MISSING_LAYERS), (kind, occurrences)
            # One copy per open: opening a path that is already open answers that document.
            copies = [folder / f'{kind}-missing-{n}.doctored' for n in range(3)]
            for copy in copies:
                copy.write_bytes(data.replace(PATCH_FROM, PATCH_TO))
            patched = copies[0]
            print(f'PASS {kind} m1: four text layers made, saved, {occurrences} family names patched',
                  flush=True)

            # ---- M2 missing --------------------------------------------------------------------
            missing_doc = call('open_document', path=str(patched))['document']['id']
            missing = snapshot(missing_doc, indices)
            assert_renders(f'{kind} m2', missing, made)
            for name in LAYERS:
                text = missing['text'][name]
                if name in MISSING_LAYERS:
                    assert text['font'] == ABSENT and text.get('font_missing') is True, (kind, name, text)
                else:
                    assert text['font'] == STEADY and 'font_missing' not in text, (kind, name, text)
            listed = call('list_missing_fonts', document_id=missing_doc)
            expected = {'missing': [{'font': ABSENT, 'layers': [
                {'layer': indices[n], 'name': rows(missing_doc)[indices[n]]['name']}
                for n in MISSING_LAYERS]}], 'count': 1}
            assert listed == expected, (kind, listed, expected)
            print(f'PASS {kind} m2: the patched file opens with its saved pixels and names '
                  f'{ABSENT} on A, B and D', flush=True)

            # ---- M3 replace back ---------------------------------------------------------------
            before = undo_count(missing_doc)
            reply = call('replace_missing_fonts', document_id=missing_doc,
                         replacements=[{'font': ABSENT, 'with': ORIGINAL}])
            assert reply['ok'] and [r['layer'] for r in reply['replaced']] == \
                [indices[n] for n in MISSING_LAYERS], (kind, reply)
            assert undo_count(missing_doc) == before + 1, (kind, before, undo_count(missing_doc))
            restored = snapshot(missing_doc, indices)
            # Placement first: a layer that moved is named with its old and new bounds.
            moved = {name: (made['bounds'][name], restored['bounds'][name]) for name in LAYERS
                     if restored['bounds'][name] != made['bounds'][name]}
            assert not moved, (kind, 'm3 moved', moved)
            assert_renders(f'{kind} m3', restored, made)
            for name in LAYERS:
                assert restored['text'][name] == made['text'][name], \
                    (kind, name, restored['text'][name], made['text'][name])
            assert call('list_missing_fonts', document_id=missing_doc) == {'missing': [], 'count': 0}
            print(f'PASS {kind} m3: replacing with {ORIGINAL} is one undo step and reproduces '
                  'every layer exactly', flush=True)

            # ---- M4 reopen ---------------------------------------------------------------------
            replaced = folder / f'{kind}-replaced.doctored'
            call('save_copy', document_id=missing_doc, path=str(replaced), format='doctored')
            reopened = call('open_document', path=str(replaced))['document']['id']
            again = snapshot(reopened, indices)
            assert_renders(f'{kind} m4 reopen', again, made)
            assert again['text'] == restored['text'], (kind, again['text'], restored['text'])
            size = LAYERS['B']['size']
            call('edit_text_layer', document_id=reopened, layer=indices['B'], size=size + 1)
            call('edit_text_layer', document_id=reopened, layer=indices['B'], size=size)
            round_trip = snapshot(reopened, indices)
            assert_renders(f'{kind} m4 re-render', round_trip, made)
            assert round_trip['text'] == made['text'], (kind, round_trip['text'])
            print(f'PASS {kind} m4: the replaced file reopens and B re-renders identically',
                  flush=True)

            # ---- M5 another family -------------------------------------------------------------
            other_doc = call('open_document', path=str(copies[1]))['document']['id']
            before = undo_count(other_doc)
            call('replace_missing_fonts', document_id=other_doc,
                 replacements=[{'font': ABSENT, 'with': OTHER}])
            assert undo_count(other_doc) == before + 1
            swapped = snapshot(other_doc, indices)
            for name in ('A', 'D'):
                was, now = made['text'][name], swapped['text'][name]
                assert now['font'] == OTHER and 'font_missing' not in now, (kind, name, now)
                assert now['origin'] == was['origin'], (kind, name, now['origin'], was['origin'])
                assert swapped['bounds'][name][:2] == made['bounds'][name][:2], \
                    (kind, name, swapped['bounds'][name], made['bounds'][name])
            old_b, new_b = made['bounds']['B'], swapped['bounds']['B']
            centres = [(old_b[0] + old_b[2] / 2, old_b[1] + old_b[3] / 2),
                       (new_b[0] + new_b[2] / 2, new_b[1] + new_b[3] / 2)]
            assert all(abs(a - b) <= CENTRE_SLACK for a, b in zip(*centres)), (kind, centres)
            assert swapped['text']['B']['font'] == OTHER
            assert swapped['text']['C'] == made['text']['C'], kind
            assert swapped['bounds']['C'] == made['bounds']['C'], kind
            ok, detail = same(swapped['layers']['C'], made['layers']['C'])
            assert ok, (kind, 'C', detail)
            call('undo', document_id=other_doc)
            undone = snapshot(other_doc, indices)
            assert_renders(f'{kind} m5 undo', undone, missing)
            assert undone['text'] == missing['text'], kind
            call('redo', document_id=other_doc)
            assert all(rows(other_doc)[indices[n]]['text']['font'] == OTHER for n in MISSING_LAYERS)
            receipts[kind] = {'b_centres': centres, 'a_origin': swapped['text']['A']['origin'],
                              'd_origin': swapped['text']['D']['origin']}
            print(f'PASS {kind} m5: {OTHER} keeps A and D at their origin, centres B, leaves C; '
                  'undo and redo', flush=True)

            # ---- M6 refusals -------------------------------------------------------------------
            refuse_doc = call('open_document', path=str(copies[2]))['document']['id']
            before = undo_count(refuse_doc)
            refusals = {
                'empty': call('replace_missing_fonts', fail=True, document_id=refuse_doc,
                              replacements=[]),
                'not_missing': call('replace_missing_fonts', fail=True, document_id=refuse_doc,
                                    replacements=[{'font': STEADY, 'with': ORIGINAL}]),
                'lowercase': call('replace_missing_fonts', fail=True, document_id=refuse_doc,
                                  replacements=[{'font': ABSENT, 'with': ORIGINAL.lower()}]),
                'uninstalled': call('replace_missing_fonts', fail=True, document_id=refuse_doc,
                                    replacements=[{'font': ABSENT, 'with': 'Zz Nope'}]),
            }
            assert 'replacements must be' in refusals['empty'], refusals
            assert 'list_missing_fonts' in refusals['not_missing'], refusals
            assert 'with names no installed font family' in refusals['lowercase'], refusals
            assert 'with names no installed font family' in refusals['uninstalled'], refusals
            assert undo_count(refuse_doc) == before
            call('set_layer_lock', document_id=refuse_doc, layer=indices['A'], locks=['pixels'])
            locked_before = undo_count(refuse_doc)
            refusals['locked'] = call('replace_missing_fonts', fail=True, document_id=refuse_doc,
                                      replacements=[{'font': ABSENT, 'with': ORIGINAL}])
            assert 'set_layer_lock' in refusals['locked'], refusals
            assert undo_count(refuse_doc) == locked_before
            still = rows(refuse_doc)
            assert all(still[indices[n]]['text']['font'] == ABSENT for n in MISSING_LAYERS), kind
            receipts[f'{kind}_refusals'] = refusals
            print(f'PASS {kind} m6: every refusal is in band and changes nothing', flush=True)

            # ---- M7 gates ----------------------------------------------------------------------
            if kind in GATED_KINDS:
                # A typed RGB document lists its depth's own tools apart from the shared ones.
                reply = call('get_precision', document_id=doc)
                supported = set(reply.get('supported_tools') or []) | set(reply.get('shared_tools') or [])
                assert all(tool in supported for tool in TOOLS), (kind, sorted(supported))
                print(f'PASS {kind} m7: get_precision lists both tools', flush=True)
            for opened in (missing_doc, reopened, other_doc, refuse_doc, doc):
                call('close_document', document_id=opened, discard_changes=True)

    (output / 'receipts.json').write_text(json.dumps(receipts, indent=2, sort_keys=True) + '\n')
    print(f'PASS all ({", ".join(kinds)}); receipts in {output}', flush=True)


if __name__ == '__main__':
    main()
